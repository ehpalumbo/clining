//! Shared helpers for end-to-end integration tests: a local mock HTTP server,
//! fixture-spec loading, and a driver for the compiled `clining` binary.
#![allow(dead_code)]

use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};

/// Arbitrary non-UTF-8 bytes to prove stdout is byte-exact.
pub const BINARY_BODY: &[u8] = &[0x00, 0x01, 0xfe, 0x0a, 0xff, b'x', 0x80];

/// Absolute path to the shared petstore fixture spec.
pub fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/petstore.json")
}

/// Contents of the shared petstore fixture spec.
pub fn fixture_spec() -> String {
    fs::read_to_string(fixture_path()).unwrap()
}

/// Creates a fresh isolated store directory.
pub fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("clining-e2e-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Runs the compiled binary with an isolated store and optional stdin bytes.
pub fn run_cli(dir: &Path, args: &[&str], stdin: Option<&[u8]>) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_clining"));
    cmd.args(args).env("CLINING_DIR", dir);
    let stdin_mode = if stdin.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    };
    let mut child = cmd
        .stdin(stdin_mode)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if let Some(bytes) = stdin {
        child.stdin.take().unwrap().write_all(bytes).unwrap();
    }
    child.wait_with_output().unwrap()
}

/// Installs the petstore fixture under `name`, optionally pointing at a base URL.
pub fn install(dir: &Path, name: &str, base_url: Option<&str>) -> Output {
    let spec = dir.join("spec.json");
    fs::write(&spec, fixture_spec()).unwrap();
    let mut args = vec!["install", name, spec.to_str().unwrap()];
    let base = format!(
        "--base-url={}",
        base_url.unwrap_or("https://api.example.com/v1")
    );
    if base_url.is_some() {
        args.push(&base);
    }
    run_cli(dir, &args, None)
}

/// Returns the request text sent to the mock server at a given connection index.
pub fn request_text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Finds the end of the HTTP header block (`\r\n\r\n`).
fn headers_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

/// Reads a full HTTP request (headers plus any Content-Length body) from a stream.
fn read_full_request(stream: &mut TcpStream) -> Vec<u8> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = stream.read(&mut chunk).unwrap();
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(end) = headers_end(&buf) {
            let head = String::from_utf8_lossy(&buf[..end]);
            let content_length: usize = head
                .lines()
                .map(str::to_ascii_lowercase)
                .filter_map(|line| {
                    line.strip_prefix("content-length:")
                        .map(|v| v.trim().parse().unwrap_or(0))
                })
                .next()
                .unwrap_or(0);
            if buf.len() >= end + 4 + content_length {
                break;
            }
        }
    }
    buf
}

/// A tiny in-process mock HTTP server that records requests and answers the
/// routes declared by the petstore fixture.
pub struct MockServer {
    pub addr: std::net::SocketAddr,
    requests: Arc<Mutex<Vec<Vec<u8>>>>,
    pub handle: std::thread::JoinHandle<()>,
}

impl MockServer {
    pub fn start(connections: usize) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let server_requests = Arc::clone(&requests);
        let handle = std::thread::spawn(move || {
            for _ in 0..connections {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
                let request = read_full_request(&mut stream);
                let request_line = String::from_utf8_lossy(&request);
                server_requests.lock().unwrap().push(request.clone());
                let (status, body): (u16, &[u8]) = if request_line.starts_with("GET /pets?") {
                    (200, b"{\"name\":\"fluffy\"}")
                } else if request_line.starts_with("GET /binary") {
                    (200, BINARY_BODY)
                } else if request_line.starts_with("GET /pets/42") {
                    (200, b"{\"id\":42}")
                } else if request_line.starts_with("POST /pets") {
                    (201, b"{\"id\":7}")
                } else if request_line.starts_with("POST /store/orders") {
                    (200, b"{\"orderId\":1}")
                } else {
                    (404, b"not found")
                };
                let head = format!(
                    "HTTP/1.1 {status} X-Status\r\nContent-Type: application/octet-stream\r\nX-Request-Id: e2e-1\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                stream.write_all(head.as_bytes()).unwrap();
                stream.write_all(body).unwrap();
            }
        });
        Self {
            addr,
            requests,
            handle,
        }
    }

    pub fn request(&self, index: usize) -> Vec<u8> {
        self.requests.lock().unwrap()[index].clone()
    }
}
