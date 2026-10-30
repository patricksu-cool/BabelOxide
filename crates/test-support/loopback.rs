// Test-only OpenAI wire fixture shared by CLI, GUI and task integration tests.
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

pub struct LoopbackProvider {
    endpoint: String,
    requests: Arc<Mutex<Vec<Value>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}
#[derive(Clone, Copy)]
enum ReplyKind {
    OpenAi,
    Free,
    Google,
}

#[allow(dead_code)] // Each consumer uses only the boundary cases it needs.
impl LoopbackProvider {
    pub fn new() -> std::io::Result<Self> {
        Self::start(200, false, ReplyKind::OpenAi)
    }
    pub fn rejecting() -> std::io::Result<Self> {
        Self::start(401, false, ReplyKind::OpenAi)
    }
    pub fn stalled() -> std::io::Result<Self> {
        Self::start(200, true, ReplyKind::OpenAi)
    }
    pub fn free() -> std::io::Result<Self> {
        Self::start(200, false, ReplyKind::Free)
    }
    pub fn google() -> std::io::Result<Self> {
        Self::start(200, false, ReplyKind::Google)
    }
    fn start(status: u16, stalled: bool, kind: ReplyKind) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let endpoint = format!("http://{}/v1", listener.local_addr()?);
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (recorded, shutdown) = (requests.clone(), stop.clone());
        let thread = thread::spawn(move || {
            while !shutdown.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream
                            .set_read_timeout(Some(Duration::from_secs(1)))
                            .unwrap();
                        stream
                            .set_write_timeout(Some(Duration::from_secs(1)))
                            .unwrap();
                        if let Ok(body) = read_request(&mut stream) {
                            recorded.lock().unwrap().push(body.clone());
                            while stalled && !shutdown.load(Ordering::Acquire) {
                                thread::sleep(Duration::from_millis(2));
                            }
                            if shutdown.load(Ordering::Acquire) {
                                break;
                            }
                            let reply = if status == 200 {
                                match kind {
                                    ReplyKind::OpenAi => {
                                        json!({"choices":[{"message":{"content":body["model"]}}], "usage":{"prompt_tokens":3,"completion_tokens":2,"total_tokens":5}})
                                    }
                                    ReplyKind::Free
                                        if body["_path"].as_str().unwrap().ends_with("/check") =>
                                    {
                                        json!({"status":"ok"})
                                    }
                                    ReplyKind::Free
                                        if body["_path"].as_str().unwrap().ends_with("/config") =>
                                    {
                                        json!({"status":"ok","qps":7,"max_pool_size":9})
                                    }
                                    ReplyKind::Free => json!({"content":"controlled free result"}),
                                    ReplyKind::Google => Value::Null,
                                }
                            } else {
                                json!({"error":{"message":"Controlled authentication failure","type":"authentication_error"}})
                            };
                            let reply = if matches!(kind, ReplyKind::Google) {
                                b"<div class=\"result-container\">controlled result</div>".to_vec()
                            } else {
                                serde_json::to_vec(&reply).unwrap()
                            };
                            let _ = write!(
                                stream,
                                "HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n",
                                reply.len()
                            );
                            let _ = stream.write_all(&reply);
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2))
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            endpoint,
            requests,
            stop,
            thread: Some(thread),
        })
    }
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }
    pub fn requests(&self) -> Vec<Value> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for LoopbackProvider {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}

fn read_request(stream: &mut TcpStream) -> std::io::Result<Value> {
    let mut header = Vec::new();
    while !header.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        stream.read_exact(&mut byte)?;
        header.push(byte[0]);
        if header.len() > 64 * 1024 {
            return Err(std::io::Error::other("Oversized fixture header"));
        }
    }
    let header = String::from_utf8_lossy(&header);
    let mut first = header.lines().next().unwrap().split_whitespace();
    let method = first.next().unwrap_or_default();
    let path = first.next().unwrap_or_default();
    let length = header
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    if length > 64 * 1024 {
        return Err(std::io::Error::other("Oversized fixture body"));
    }
    let mut body = vec![0; length];
    stream.read_exact(&mut body)?;
    let mut value = if body.is_empty() {
        json!({})
    } else {
        serde_json::from_slice(&body).map_err(std::io::Error::other)?
    };
    value["_method"] = json!(method);
    value["_path"] = json!(path);
    Ok(value)
}
