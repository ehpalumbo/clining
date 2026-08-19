//! End-to-end tests driving the compiled binary against a local mock server
//! (Phase 3-4): install fixture -> invoke -> assert stdout/stderr/exit-code split.

mod common;

use common::{BINARY_BODY, MockServer, install, request_text, run_cli, temp_dir};

fn server_base(server: &MockServer) -> String {
    format!("http://{}", server.addr)
}

#[test]
fn install_then_invoke_end_to_end() {
    let dir = temp_dir("happy");
    let server = MockServer::start(2);
    let base = server_base(&server);

    let install = install(&dir, "pets", Some(&base));
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
fn repeated_query_values_become_repeated_keys() {
    let dir = temp_dir("repeat");
    let server = MockServer::start(1);
    let base = server_base(&server);
    install(&dir, "pets", Some(&base));

    let body = b"{}";
    let invoke = run_cli(
        &dir,
        &["pets", "pets", "get-pets", "--tag", "a", "--tag", "b"],
        Some(body),
    );
    assert!(
        invoke.status.success(),
        "{}",
        String::from_utf8_lossy(&invoke.stderr)
    );
    let sent = request_text(&server.request(0));
    assert!(sent.starts_with("GET /pets?tag=a&tag=b HTTP/1.1"), "{sent}");

    server.handle.join().unwrap();
}

#[test]
fn post_with_body_reaches_endpoint() {
    let dir = temp_dir("post");
    let server = MockServer::start(1);
    let base = server_base(&server);
    install(&dir, "pets", Some(&base));

    let body = b"{\"name\":\"rex\"}";
    let invoke = run_cli(&dir, &["pets", "pets", "create-pet"], Some(body));
    assert!(
        invoke.status.success(),
        "{}",
        String::from_utf8_lossy(&invoke.stderr)
    );
    assert_eq!(invoke.stdout, b"{\"id\":7}");
    let stderr = String::from_utf8_lossy(&invoke.stderr);
    assert!(stderr.contains("HTTP/1.1 201"), "{stderr}");
    let sent = request_text(&server.request(0));
    assert!(sent.starts_with("POST /pets HTTP/1.1"), "{sent}");
    assert!(sent.ends_with("{\"name\":\"rex\"}"), "{sent}");

    server.handle.join().unwrap();
}

#[test]
fn non_2xx_exits_1_with_status_on_stderr() {
    let dir = temp_dir("notfound");
    let server = MockServer::start(1);
    let base = server_base(&server);
    install(&dir, "pets", Some(&base));

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
    let base = server_base(&server);
    install(&dir, "pets", Some(&base));

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
fn unknown_command_group_is_a_clap_error() {
    let dir = temp_dir("unknown-group");
    install(&dir, "pets", None);

    let invoke = run_cli(&dir, &["pets", "nosuch", "get-pets"], None);
    assert!(!invoke.status.success());
    assert_eq!(invoke.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&invoke.stderr);
    assert!(stderr.contains("nosuch"), "{stderr}");
}

#[test]
fn unknown_command_is_a_clap_error() {
    let dir = temp_dir("unknown-cmd");
    install(&dir, "pets", None);

    let invoke = run_cli(&dir, &["pets", "pets", "nosuch"], None);
    assert!(!invoke.status.success());
    assert_eq!(invoke.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&invoke.stderr);
    assert!(stderr.contains("nosuch"), "{stderr}");
    assert!(stderr.contains("pets"), "{stderr}");
}

#[test]
fn corrupt_stored_model_is_not_not_found() {
    let dir = temp_dir("corrupt");
    std::fs::write(dir.join("pets.json"), b"not json").unwrap();

    let invoke = run_cli(&dir, &["pets", "pets", "get-pets"], None);
    assert!(!invoke.status.success());
    let stderr = String::from_utf8_lossy(&invoke.stderr);
    assert!(stderr.contains("invalid"), "{stderr}");
    assert!(!stderr.contains("no API installed"), "{stderr}");
}
