//! End-to-end tests driving the compiled binary against a local mock server
//! (Phase 3): install fixture -> invoke -> assert stdout/stderr/exit-code split.

use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};

const FIXTURE: &str = r#"{
    "openapi": "3.0.0",
    "info": { "title": "Petstore", "version": "1.0.0" },
    "paths": {
        "/pets": {
            "get": {
                "operationId": "getPets",
                "summary": "List pets",
                "tags": ["pets"],
                "parameters": [
                    { "name": "status", "in": "query" }
                ],
                "requestBody": {
                    "required": true,
                    "content": {
                        "application/json": { "schema": { "type": "object" } }
                    }
                }
            }
        },
        "/pets/{petId}": {
            "get": {
                "operationId": "getPet",
                "tags": ["pets"],
                "parameters": [
                    { "name": "petId", "in": "path", "required": true }
                ]
            }
        },
        "/binary": {
            "get": {
                "operationId": "getBinary",
                "tags": ["pets"]
            }
        }
    }
}"#;

const BINARY_BODY: &[u8] = &[0x00, 0x01, 0xfe, 0x0a, 0xff, b'x', 0x80];

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("clining-e2e-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn headers_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

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

struct MockServer {
    addr: std::net::SocketAddr,
    requests: Arc<Mutex<Vec<Vec<u8>>>>,
    handle: std::thread::JoinHandle<()>,
}

impl MockServer {
    fn start(connections: usize) -> Self {
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

    fn request(&self, index: usize) -> Vec<u8> {
        self.requests.lock().unwrap()[index].clone()
    }
}

fn run_cli(dir: &Path, args: &[&str], stdin: Option<&[u8]>) -> Output {
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

fn request_text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn install_then_invoke_end_to_end() {
    let dir = temp_dir("happy");
    let server = MockServer::start(2);
    let spec = dir.join("spec.json");
    fs::write(&spec, FIXTURE).unwrap();
    let base = format!("http://{}", server.addr);

    let install = run_cli(
        &dir,
        &[
            "install",
            "pets",
            spec.to_str().unwrap(),
            "--base-url",
            &base,
        ],
        None,
    );
    assert!(
        install.status.success(),
        "{}",
        String::from_utf8_lossy(&install.stderr)
    );
    assert!(String::from_utf8_lossy(&install.stdout).contains("Installed pets"));

    let body = b"{\"status\":\"open\"}";
    let invoke = run_cli(
        &dir,
        &["pets", "pets", "get-pets", "--status", "available"],
        Some(body),
    );
    assert!(
        invoke.status.success(),
        "{}",
        String::from_utf8_lossy(&invoke.stderr)
    );
    assert_eq!(invoke.stdout, b"{\"name\":\"fluffy\"}");
    let stderr = String::from_utf8_lossy(&invoke.stderr);
    assert!(stderr.contains("HTTP/1.1 200"), "{stderr}");
    assert!(stderr.contains("x-request-id: e2e-1"), "{stderr}");

    let sent = request_text(&server.request(0));
    assert!(
        sent.starts_with("GET /pets?status=available HTTP/1.1"),
        "{sent}"
    );
    assert!(sent.contains("content-type: application/json"), "{sent}");
    assert!(sent.ends_with("{\"status\":\"open\"}"), "{sent}");

    let path = run_cli(&dir, &["pets", "pets", "get-pet", "--pet-id", "42"], None);
    assert!(
        path.status.success(),
        "{}",
        String::from_utf8_lossy(&path.stderr)
    );
    assert_eq!(path.stdout, b"{\"id\":42}");
    let sent = request_text(&server.request(1));
    assert!(sent.starts_with("GET /pets/42 HTTP/1.1"), "{sent}");

    server.handle.join().unwrap();
}

#[test]
fn non_2xx_exits_1_with_status_on_stderr() {
    let dir = temp_dir("notfound");
    let server = MockServer::start(1);
    let spec = dir.join("spec.json");
    fs::write(&spec, FIXTURE).unwrap();
    let base = format!("http://{}", server.addr);
    run_cli(
        &dir,
        &[
            "install",
            "pets",
            spec.to_str().unwrap(),
            "--base-url",
            &base,
        ],
        None,
    );

    let invoke = run_cli(&dir, &["pets", "pets", "get-pet", "--pet-id", "999"], None);
    assert!(!invoke.status.success());
    assert_eq!(invoke.status.code(), Some(1));
    assert_eq!(invoke.stdout, b"not found");
    let stderr = String::from_utf8_lossy(&invoke.stderr);
    assert!(stderr.contains("HTTP/1.1 404"), "{stderr}");
    assert!(!stderr.contains("200"), "{stderr}");

    server.handle.join().unwrap();
}

#[test]
fn response_body_reaches_stdout_binary_safe() {
    let dir = temp_dir("binary");
    let server = MockServer::start(1);
    let spec = dir.join("spec.json");
    fs::write(&spec, FIXTURE).unwrap();
    let base = format!("http://{}", server.addr);
    run_cli(
        &dir,
        &[
            "install",
            "pets",
            spec.to_str().unwrap(),
            "--base-url",
            &base,
        ],
        None,
    );

    let invoke = run_cli(&dir, &["pets", "pets", "get-binary"], None);
    assert!(
        invoke.status.success(),
        "{}",
        String::from_utf8_lossy(&invoke.stderr)
    );
    assert_eq!(invoke.stdout, BINARY_BODY);
    let stderr = String::from_utf8_lossy(&invoke.stderr);
    assert!(stderr.contains("HTTP/1.1 200"), "{stderr}");
    assert!(stderr.contains("content-length"), "{stderr}");

    server.handle.join().unwrap();
}

#[test]
fn unknown_command_group_is_an_error() {
    let dir = temp_dir("unknown");
    let spec = dir.join("spec.json");
    fs::write(&spec, FIXTURE).unwrap();
    run_cli(
        &dir,
        &[
            "install",
            "pets",
            spec.to_str().unwrap(),
            "--base-url",
            "http://127.0.0.1:9",
        ],
        None,
    );

    let invoke = run_cli(&dir, &["pets", "store", "get-pets"], None);
    assert!(!invoke.status.success());
    let stderr = String::from_utf8_lossy(&invoke.stderr);
    assert!(stderr.contains("store"), "{stderr}");
}

#[test]
fn corrupt_stored_model_is_not_not_found() {
    let dir = temp_dir("corrupt");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("pets.json"), b"not json").unwrap();

    let invoke = run_cli(&dir, &["pets", "pets", "get-pets"], None);
    assert!(!invoke.status.success());
    let stderr = String::from_utf8_lossy(&invoke.stderr);
    assert!(stderr.contains("invalid"), "{stderr}");
    assert!(!stderr.contains("no API installed"), "{stderr}");
}
