#[cfg(test)]
mod tests {
    use crate::*;
    use serde_json::{Value, json};
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::sync::oneshot;
    use tokio::time::Instant;

    fn config(endpoint: String) -> TranslatorConfig {
        TranslatorConfig {
            provider: "openai".into(),
            endpoint,
            api_key: "synthetic-private-key".into(),
            model: "synthetic-model".into(),
            timeout_seconds: 2.0,
            lang_out: "zh-TW".into(),
            options: json!({ "qps": 1000, "max_concurrency": 2 }),
        }
    }

    fn request(method: &str) -> TranslationRequest {
        TranslationRequest {
            id: "request-1".into(),
            text: "A formula {v0} and <style id='1'>text</style>.".into(),
            method: method.into(),
            ignore_cache: true,
            rate_limit_params: Value::Null,
            translator: "openai".into(),
        }
    }

    async fn read_request(stream: &mut TcpStream) -> (String, Value) {
        let mut data = Vec::new();
        let header_end = loop {
            let mut chunk = [0; 1024];
            let count = stream.read(&mut chunk).await.unwrap();
            assert!(count > 0, "client closed before request headers");
            data.extend_from_slice(&chunk[..count]);
            if let Some(index) = data.windows(4).position(|part| part == b"\r\n\r\n") {
                break index + 4;
            }
        };
        let headers = String::from_utf8(data[..header_end].to_vec()).unwrap();
        let length = headers
            .lines()
            .find_map(|line| {
                let (key, value) = line.split_once(':')?;
                key.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().unwrap())
            })
            .unwrap();
        while data.len() < header_end + length {
            let mut chunk = [0; 1024];
            let count = stream.read(&mut chunk).await.unwrap();
            assert!(count > 0, "client closed before request body");
            data.extend_from_slice(&chunk[..count]);
        }
        let body = serde_json::from_slice(&data[header_end..header_end + length]).unwrap();
        (headers, body)
    }

    async fn server(status: &str, body: String) -> (String, oneshot::Receiver<(String, Value)>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        let (send, receive) = oneshot::channel();
        let status = status.to_owned();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let request = read_request(&mut stream).await;
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
            let _ = send.send(request);
        });
        (endpoint, receive)
    }

    #[tokio::test]
    async fn translate_posts_legacy_prompt_and_removes_only_the_initial_thought() {
        let (endpoint, received) = server("200 OK", json!({"choices":[{"message":{"content":"  <think>private reasoning</think>\n譯文  "}}]}).to_string()).await;
        let service = TranslationService::new(config(endpoint)).unwrap();
        let request = request("translate");
        assert_eq!(
            service
                .translate(&request, &CancellationToken::new())
                .await
                .unwrap(),
            "\n譯文"
        );
        let (headers, body) = received.await.unwrap();
        assert!(headers.starts_with("POST /v1/chat/completions HTTP/1.1"));
        assert!(
            headers
                .to_lowercase()
                .contains("authorization: bearer synthetic-private-key")
        );
        assert_eq!(body["model"], "synthetic-model");
        assert_eq!(
            body["messages"],
            json!([{ "role": "user", "content": format!("You are a professional,authentic machine translation engine.\n\n;; Treat next line as plain text input and translate it into zh-TW, output translation ONLY. If translation is unnecessary (e.g. proper nouns, codes, {{{{1}}}}, etc. ), return the original text. NO explanations. NO notes. Input:\n\n{}", request.text) }])
        );
        assert!(body.get("response_format").is_none());
    }

    #[tokio::test]
    async fn llm_translation_preserves_raw_prompt_and_requests_json_when_enabled() {
        let (endpoint, received) = server(
            "200 OK",
            json!({"choices":[{"message":{"content":"{\"result\":true}"}}]}).to_string(),
        )
        .await;
        let mut config = config(endpoint);
        config.options["enable_json_mode"] = json!(true);
        config.options["temperature"] = json!(0.0);
        config.options["extra_body"] = json!({ "seed": 7 });
        let service = TranslationService::new(config).unwrap();
        let mut request = request("llm_translate");
        request.rate_limit_params = json!({ "request_json_mode": true });
        assert_eq!(
            service
                .translate(&request, &CancellationToken::new())
                .await
                .unwrap(),
            "{\"result\":true}"
        );
        let (_, body) = received.await.unwrap();
        assert_eq!(
            body["messages"],
            json!([{ "role": "user", "content": request.text }])
        );
        assert_eq!(body["response_format"], json!({"type":"json_object"}));
        assert_eq!(body["temperature"], 0.0);
        assert_eq!(body["seed"], 7);
    }

    #[tokio::test]
    async fn errors_do_not_expose_provider_body_or_credentials() {
        let (endpoint, received) = server(
            "401 Unauthorized",
            "synthetic-private-key private-source-text endpoint-secret".into(),
        )
        .await;
        let service = TranslationService::new(config(endpoint)).unwrap();
        let error = service
            .translate(&request("translate"), &CancellationToken::new())
            .await
            .unwrap_err();
        assert_eq!(
            format!("{error:#}"),
            "translation provider returned HTTP 401"
        );
        received.await.unwrap();
    }

    #[tokio::test]
    async fn malformed_empty_and_null_responses_fail_without_echoing_content() {
        for body in [
            "private-source-text".into(),
            json!({"choices":[]}).to_string(),
            json!({"choices":[{"message":{"content":null}}]}).to_string(),
            json!({"choices":[{"message":{}}]}).to_string(),
        ] {
            let (endpoint, received) = server("200 OK", body).await;
            let service = TranslationService::new(config(endpoint)).unwrap();
            let error = service
                .translate(&request("translate"), &CancellationToken::new())
                .await
                .unwrap_err();
            assert!(!format!("{error:#}").contains("private-source-text"));
            received.await.unwrap();
        }
    }

    #[tokio::test]
    async fn oversized_response_is_rejected() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            read_request(&mut stream).await;
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
                        MAX_RESPONSE_BYTES + 1
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
        });
        let service = TranslationService::new(config(endpoint)).unwrap();
        let error = service
            .translate(&request("translate"), &CancellationToken::new())
            .await
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "translation response exceeded the 4 MiB limit"
        );
        task.await.unwrap();
    }

    #[tokio::test]
    async fn oversized_body_without_content_length_is_also_rejected() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            read_request(&mut stream).await;
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
            // The client is allowed to close the connection as soon as the cap is reached.
            let _ = stream.write_all(&vec![b'x'; MAX_RESPONSE_BYTES + 1]).await;
        });
        let service = TranslationService::new(config(endpoint)).unwrap();
        let error = service
            .translate(&request("translate"), &CancellationToken::new())
            .await
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "translation response exceeded the 4 MiB limit"
        );
        task.await.unwrap();
    }

    #[tokio::test]
    async fn cancellation_interrupts_in_flight_http() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let (started, received) = oneshot::channel();
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            read_request(&mut stream).await;
            let _ = started.send(());
            std::future::pending::<()>().await;
        });
        let service = TranslationService::new(config(endpoint)).unwrap();
        let cancel = CancellationToken::new();
        let request = request("translate");
        let translation = service.translate(&request, &cancel);
        tokio::pin!(translation);
        tokio::select! {
            result = &mut translation => panic!("request completed before cancellation: {}", result.is_ok()),
            _ = received => {},
        }
        cancel.cancel();
        let error = tokio::time::timeout(Duration::from_millis(200), translation)
            .await
            .unwrap()
            .unwrap_err();
        assert_eq!(error.to_string(), "translation cancelled");
        task.abort();
    }

    #[tokio::test]
    async fn request_timeout_is_bounded_and_safe() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            read_request(&mut stream).await;
            std::future::pending::<()>().await;
        });
        let mut config = config(endpoint);
        config.timeout_seconds = 0.05;
        let service = TranslationService::new(config).unwrap();
        let error = service
            .translate(&request("translate"), &CancellationToken::new())
            .await
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "translation request timed out (ReadTimeout)"
        );
        task.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn rate_limit_waits_in_rust_and_can_be_cancelled() {
        let limiter = RateLimiter::new(2);
        let cancel = CancellationToken::new();
        let start = Instant::now();
        limiter.wait(&cancel).await.unwrap();
        limiter.wait(&cancel).await.unwrap();
        assert_eq!(Instant::now() - start, Duration::from_millis(500));
        cancel.cancel();
        assert_eq!(
            limiter.wait(&cancel).await.unwrap_err().to_string(),
            "translation cancelled"
        );
        assert_eq!(Instant::now() - start, Duration::from_millis(500));
    }

    #[test]
    fn thought_removal_matches_the_legacy_nonempty_anchored_pattern() {
        assert_eq!(
            remove_think("<think></think>answer"),
            "<think></think>answer"
        );
        assert_eq!(remove_think("<think></think>x</think>answer"), "answer");
        assert_eq!(
            remove_think("prefix<think>x</think>answer"),
            "prefix<think>x</think>answer"
        );
        assert_eq!(remove_think("<think>one\ntwo</think>\nanswer"), "\nanswer");
    }
}
