//! Fixed wire fixtures were encoded independently by Python gzip/zlib/Brotli/pyzstd.
use std::{collections::BTreeMap, time::Duration};

use babeloxide_core::{
    ErrorKind, HttpBody, HttpRequest, HttpTransport, RetryRuntime, TranslationRequest,
    TranslationService, TranslatorConfig,
};
use reqwest::Method;
use serde_json::json;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::oneshot,
    task::JoinHandle,
    time::timeout,
};
use tokio_util::sync::CancellationToken;

fn fixtures(kind: &str) -> Vec<(&'static str, &'static [u8])> {
    match kind {
        "json" => vec![
            ("gzip", include_bytes!("fixtures/compression/json.gzip")),
            (
                "deflate",
                include_bytes!("fixtures/compression/json.deflate"),
            ),
            ("br", include_bytes!("fixtures/compression/json.br")),
            ("zstd", include_bytes!("fixtures/compression/json.zstd")),
        ],
        _ => vec![
            ("gzip", include_bytes!("fixtures/compression/html.gzip")),
            (
                "deflate",
                include_bytes!("fixtures/compression/html.deflate"),
            ),
            ("br", include_bytes!("fixtures/compression/html.br")),
            ("zstd", include_bytes!("fixtures/compression/html.zstd")),
        ],
    }
}

async fn read_request(stream: &mut TcpStream) -> String {
    let mut headers = Vec::new();
    while !headers.ends_with(b"\r\n\r\n") {
        assert!(headers.len() < 16 * 1024);
        headers.push(stream.read_u8().await.unwrap());
    }
    let headers = String::from_utf8(headers).unwrap();
    let length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().unwrap())
        })
        .unwrap_or(0);
    assert!(length < 1024 * 1024);
    stream.read_exact(&mut vec![0; length]).await.unwrap();
    headers
}

async fn server(encoding: &str, body: &'static [u8]) -> (String, JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let encoding = encoding.to_owned();
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let headers = read_request(&mut stream).await;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Encoding: {encoding}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(response.as_bytes()).await.unwrap();
        let _ = stream.write_all(body).await;
        headers
    });
    (format!("http://{address}"), task)
}

fn request(url: String) -> HttpRequest {
    HttpRequest {
        method: Method::GET,
        url,
        headers: BTreeMap::new(),
        query: vec![],
        body: HttpBody::Empty,
        timeout: Some(Duration::from_secs(3)),
    }
}

#[tokio::test]
async fn compressed_json_is_decoded_before_parsing_for_all_four_encodings() {
    for (encoding, body) in fixtures("json") {
        let (url, server) = server(encoding, body).await;
        let transport = HttpTransport::new(RetryRuntime::default(), None);
        let response = transport
            .send(request(url), &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(
            response.json().unwrap()["choices"][0]["message"]["content"],
            "壓縮後的翻譯"
        );
        let headers = server.await.unwrap().to_ascii_lowercase();
        assert!(headers.contains("accept-encoding:"));
        assert!(headers.contains(encoding));
    }
}

#[tokio::test]
async fn compressed_html_preserves_the_text_and_markup_for_all_four_encodings() {
    for (encoding, body) in fixtures("html") {
        let (url, server) = server(encoding, body).await;
        let transport = HttpTransport::new(RetryRuntime::default(), None);
        let response = transport
            .send(request(url), &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(
            response.body,
            include_bytes!("fixtures/compression/html.plain")
        );
        server.await.unwrap();
    }
}

#[tokio::test]
async fn openai_service_consumes_compressed_json_over_real_http() {
    let (url, server) = server("gzip", include_bytes!("fixtures/compression/json.gzip")).await;
    let service = TranslationService::new(TranslatorConfig {
        provider: "openai".into(),
        endpoint: format!("{url}/v1"),
        api_key: "synthetic-unused-key".into(),
        model: "compression-fixture".into(),
        timeout_seconds: 3.0,
        lang_out: "zh-TW".into(),
        options: json!({"qps":1000}),
    })
    .unwrap();
    let request = TranslationRequest {
        id: "compressed".into(),
        text: "fixture prompt".into(),
        method: "llm_translate".into(),
        ignore_cache: true,
        rate_limit_params: serde_json::Value::Null,
        translator: "main".into(),
    };
    assert_eq!(
        service
            .translate(&request, &CancellationToken::new())
            .await
            .unwrap(),
        "壓縮後的翻譯"
    );
    assert_eq!(service.metrics().http_attempts, 1);
    server.await.unwrap();
}

#[tokio::test]
async fn response_limit_applies_to_decompressed_bytes() {
    let (url, server) = server("gzip", include_bytes!("fixtures/compression/oversize.gzip")).await;
    let transport = HttpTransport::new(RetryRuntime::default(), None);
    let Err(error) = transport
        .send(request(url), &CancellationToken::new())
        .await
    else {
        panic!("decompression must not bypass the body limit");
    };
    assert_eq!(error.kind, ErrorKind::MalformedResponse);
    server.await.unwrap();
}

#[tokio::test]
async fn malformed_compressed_body_is_a_read_transport_failure() {
    let (url, server) = server("gzip", b"invalid compressed body").await;
    let transport = HttpTransport::new(RetryRuntime::default(), None);
    let Err(error) = transport
        .send(request(url), &CancellationToken::new())
        .await
    else {
        panic!("invalid gzip must fail");
    };
    assert_eq!(error.kind, ErrorKind::ReadTransport);
    server.await.unwrap();
}

#[tokio::test]
async fn cancellation_interrupts_a_pending_compressed_body_read() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (started, receiving) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        read_request(&mut stream).await;
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: 1000\r\n\r\n")
            .await
            .unwrap();
        stream
            .write_all(&include_bytes!("fixtures/compression/json.gzip")[..10])
            .await
            .unwrap();
        let _ = started.send(());
        std::future::pending::<()>().await;
    });
    let transport = HttpTransport::new(RetryRuntime::default(), None);
    let cancel = CancellationToken::new();
    let operation = transport.send(request(format!("http://{address}")), &cancel);
    tokio::pin!(operation);
    tokio::select! {
        result = &mut operation => panic!("request ended before cancellation: {}", result.is_ok()),
        ready = timeout(Duration::from_secs(3), receiving) => {
            ready.expect("server did not start the compressed body").expect("server ended before its body gate");
        },
    }
    cancel.cancel();
    let Err(error) = timeout(Duration::from_secs(1), &mut operation)
        .await
        .unwrap()
    else {
        panic!("cancel must fail the request");
    };
    assert_eq!(error.kind, ErrorKind::Cancelled);
    server.abort();
    let _ = server.await;
}
