use std::{collections::BTreeMap, sync::atomic::AtomicU64};

use reqwest::Method;
use serde_json::{Value, json};

use super::{openai::append_path, update_num_predict};
use crate::{
    ErrorKind, HttpBody, HttpRequest, ProviderContext, ProviderOutput, ProviderResult, RetryPolicy,
    RetrySelector, Usage, WaitPolicy, duration, legacy_strip, remove_think,
};

pub async fn ollama(
    ctx: &ProviderContext<'_>,
    num_predict: &AtomicU64,
) -> ProviderResult<ProviderOutput> {
    ctx.retry(RetryPolicy{layer:"ollama.application",max_attempts:100,selector:RetrySelector::OllamaResponse,wait:WaitPolicy::E,wrap_exhaustion:true},||async{
        let predict=update_num_predict(num_predict, (ctx.request.text.chars().count() as u64).saturating_mul(5));
        let content=if ctx.is_llm_request(){ctx.request.text.clone()}else{ctx.prompt(&ctx.request.text)};
        let mut options=json!({"temperature":0,"num_predict":predict});
        if let Some(extra)=ctx.option("ollama_options").and_then(Value::as_object){for(key,value)in extra{options[key]=value.clone();}}
        let endpoint=if ctx.config.endpoint.is_empty(){"http://127.0.0.1:11434"}else{&ctx.config.endpoint};
        let timeout=ctx.option("timeout_seconds").and_then(Value::as_f64).map(duration).transpose()?;
        let response=ctx.http.send(HttpRequest{method:Method::POST,url:append_path(endpoint,"api/chat")?,headers:BTreeMap::new(),query:vec![],body:HttpBody::Json(json!({"model":ctx.config.model,"messages":[{"role":"user","content":content}],"options":options,"stream":false})),timeout},ctx.cancel).await?;
        if !(200..300).contains(&response.status){let error=ctx.http_error(&response);return Err(ctx.error(ErrorKind::OllamaResponse(response.status),error.message));}
        let value=response.json()?;
        // Nonstream ChatResponse does not promote an extra error field to
        // ResponseError. Missing required role/message is a validation failure.
        if value.pointer("/message/role").and_then(Value::as_str).is_none(){return Err(ctx.error(ErrorKind::MalformedResponse,"Ollama response has no message role"));}
        let prompt=value.get("prompt_eval_count").and_then(Value::as_u64).ok_or_else(||ctx.error(ErrorKind::MalformedResponse,"Ollama response has no prompt token count"))?;
        let completion=value.get("eval_count").and_then(Value::as_u64).ok_or_else(||ctx.error(ErrorKind::MalformedResponse,"Ollama response has no completion token count"))?;
        let usage=Usage{total_tokens:prompt.saturating_add(completion),prompt_tokens:prompt,completion_tokens:completion,cache_hit_prompt_tokens:0};
        ctx.record_usage(usage);
        let content=value.pointer("/message/content").and_then(Value::as_str).ok_or_else(||ctx.error(ErrorKind::MalformedResponse,"Ollama response has no message content"))?;
        Ok(ProviderOutput{text:remove_think(legacy_strip(content)).to_owned(),usage})
    }).await
}

pub async fn preflight_xinference(ctx: &ProviderContext<'_>) -> ProviderResult<()> {
    if ctx.option("skip_preflight").and_then(Value::as_bool) == Some(true) {
        return Ok(());
    }
    if ctx.config.endpoint.is_empty() {
        return Err(ctx.error(
            ErrorKind::InvalidConfiguration,
            "Xinference requires a host endpoint",
        ));
    }
    let response = ctx
        .http
        .send(
            HttpRequest {
                method: Method::GET,
                url: append_path(&ctx.config.endpoint, "v1/cluster/auth")?,
                headers: BTreeMap::new(),
                query: vec![],
                body: HttpBody::Empty,
                timeout: None,
            },
            ctx.cancel,
        )
        .await?;
    let authenticated = if response.status == 404 {
        false
    } else {
        if response.status != 200 {
            return Err(ctx.error(ErrorKind::Runtime, ctx.http_error(&response).message));
        }
        let data = response.json()?;
        let auth = data.get("auth").ok_or_else(|| {
            ctx.error(
                ErrorKind::MalformedResponse,
                "Xinference cluster response has no auth field",
            )
        })?;
        match auth {
            Value::Null => false,
            Value::Bool(value) => *value,
            Value::Number(value) => value.as_f64().is_some_and(|value| value != 0.0),
            Value::String(value) => !value.is_empty(),
            Value::Array(value) => !value.is_empty(),
            Value::Object(value) => !value.is_empty(),
        }
    };
    ctx.http
        .state
        .lock()
        .await
        .insert("xinference.authenticated".into(), json!(authenticated));
    Ok(())
}

pub async fn xinference(ctx: &ProviderContext<'_>) -> ProviderResult<ProviderOutput> {
    // RuntimeError is the decorator selector, but every per-model Exception is
    // caught and the final exception is plain Exception. Do not do 100 rounds.
    for model in ctx.config.model.split(';') {
        if ctx.cancel.is_cancelled() {
            return Err(crate::ProviderError::cancelled());
        }
        let result = xinference_model(ctx, model).await;
        match result {
            Ok(value) => return Ok(value),
            Err(error) if error.is_cancelled() || matches!(error.kind, ErrorKind::Interrupt) => {
                return Err(error);
            }
            Err(_) => {}
        }
    }
    Err(ctx.error(ErrorKind::Provider, "All models failed"))
}

async fn xinference_model(
    ctx: &ProviderContext<'_>,
    model: &str,
) -> ProviderResult<ProviderOutput> {
    let timeout = ctx
        .option("timeout_seconds")
        .and_then(Value::as_f64)
        .map(duration)
        .transpose()?;
    let mut headers = BTreeMap::new();
    // The old application did not pass api_key to xinference-client. An
    // explicitly selected SDK authentication option is available to hosts.
    if ctx.option("use_api_key").and_then(Value::as_bool) == Some(true)
        && ctx
            .http
            .state
            .lock()
            .await
            .get("xinference.authenticated")
            .and_then(Value::as_bool)
            == Some(true)
    {
        headers.insert(
            "authorization".into(),
            format!("Bearer {}", ctx.config.api_key),
        );
    }
    let response = ctx
        .http
        .send(
            HttpRequest {
                method: Method::GET,
                url: append_path(&ctx.config.endpoint, &format!("v1/models/{model}"))?,
                headers: headers.clone(),
                query: vec![],
                body: HttpBody::Empty,
                timeout,
            },
            ctx.cancel,
        )
        .await?;
    if response.status != 200 {
        return Err(ctx.error(ErrorKind::Runtime, ctx.http_error(&response).message));
    }
    let description = response.json()?;
    if description.get("model_type").and_then(Value::as_str) != Some("LLM")
        || !description
            .get("model_ability")
            .and_then(Value::as_array)
            .is_some_and(|values| values.iter().any(|value| value.as_str() == Some("chat")))
    {
        return Err(ctx.error(ErrorKind::Value, "Xinference model does not provide chat"));
    }
    let content = if ctx.is_llm_request() {
        ctx.request.text.clone()
    } else {
        ctx.prompt(&ctx.request.text)
    };
    let response=ctx.http.send(HttpRequest{method:Method::POST,url:append_path(&ctx.config.endpoint,"v1/chat/completions")?,headers,query:vec![],body:HttpBody::Json(json!({"model":model,"messages":[{"role":"user","content":content}],"temperature":0})),timeout},ctx.cancel).await?;
    if response.status != 200 {
        return Err(ctx.error(ErrorKind::Runtime, ctx.http_error(&response).message));
    }
    let value = response.json()?;
    let content = value
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ctx.error(
                ErrorKind::MalformedResponse,
                "Xinference response has no message content",
            )
        })?;
    Ok(ProviderOutput::text(
        legacy_strip(&content.replace("<end_of_turn>", "")).to_owned(),
    ))
}
