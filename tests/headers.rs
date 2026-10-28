//! End-to-end custom request header tests: precedence, duplicate lines,
//! Authorization override, and fail-fast validation.

mod common;

use common::{MockServer, install, request_text, run_cli, temp_dir};

fn server_base(server: &MockServer) -> String {
    format!("http://{}", server.addr)
}

/// Returns the values of every header line matching `name` (case-insensitive).
fn header_values(request: &str, name: &str) -> Vec<String> {
    request
        .lines()
        .skip(1)
        .take_while(|line| !line.is_empty())
        .filter_map(|line| {
            let (header_name, value) = line.split_once(':')?;
            header_name
                .eq_ignore_ascii_case(name)
                .then(|| value.trim().to_owned())
        })
        .collect()
}

#[test]
fn custom_header_reaches_server_with_exact_value() {
    let dir = temp_dir("headers-basic");
    let server = MockServer::start(1);
    let base = server_base(&server);
    install(&dir, "pets", Some(&base));

    let invoke = run_cli(
        &dir,
        &[
            "pets",
            "pets",
            "get-pet",
            "--pet-id",
            "42",
            "-H",
            "X-Custom: hello world",
        ],
        None,
    );
    assert!(
        invoke.status.success(),
        "{}",
        String::from_utf8_lossy(&invoke.stderr)
    );

    let sent = request_text(&server.request(0));
    assert_eq!(header_values(&sent, "x-custom"), vec!["hello world"]);
    server.handle.join().unwrap();
}

#[test]
fn two_different_custom_headers_are_both_sent() {
    let dir = temp_dir("headers-two");
    let server = MockServer::start(1);
    let base = server_base(&server);
    install(&dir, "pets", Some(&base));

    let invoke = run_cli(
        &dir,
        &[
            "pets",
            "pets",
            "get-pet",
            "--pet-id",
            "42",
            "-H",
            "X-One: first",
            "-H",
            "X-Two: second",
        ],
        None,
    );
    assert!(
        invoke.status.success(),
        "{}",
        String::from_utf8_lossy(&invoke.stderr)
    );

    let sent = request_text(&server.request(0));
    assert_eq!(header_values(&sent, "x-one"), vec!["first"]);
    assert_eq!(header_values(&sent, "x-two"), vec!["second"]);
    server.handle.join().unwrap();
}

#[test]
fn repeated_same_name_headers_send_one_line_per_occurrence_in_order() {
    let dir = temp_dir("headers-duplicate");
    let server = MockServer::start(1);
    let base = server_base(&server);
    install(&dir, "pets", Some(&base));

    let invoke = run_cli(
        &dir,
        &[
            "pets",
            "pets",
            "get-pet",
            "--pet-id",
            "42",
            "-H",
            "X-Dup: one",
            "-H",
            "x-dup: two",
        ],
        None,
    );
    assert!(
        invoke.status.success(),
        "{}",
        String::from_utf8_lossy(&invoke.stderr)
    );

    let sent = request_text(&server.request(0));
    assert_eq!(header_values(&sent, "x-dup"), vec!["one", "two"]);
    server.handle.join().unwrap();
}

#[test]
fn empty_custom_header_value_reaches_server() {
    let dir = temp_dir("headers-empty-value");
    let server = MockServer::start(1);
    let base = server_base(&server);
    install(&dir, "pets", Some(&base));

    let invoke = run_cli(
        &dir,
        &[
            "pets", "pets", "get-pet", "--pet-id", "42", "-H", "X-Empty:",
        ],
        None,
    );
    assert!(
        invoke.status.success(),
        "{}",
        String::from_utf8_lossy(&invoke.stderr)
    );

    let sent = request_text(&server.request(0));
    assert_eq!(header_values(&sent, "x-empty"), vec![""]);
    server.handle.join().unwrap();
}

#[test]
fn duplicate_authorization_headers_stay_separate_lines() {
    // Pins the duplicate-lines wire format for Authorization specifically:
    // two occurrences are sent as two header lines, never collapsed or
    // comma-joined, and never merged with a resolved bearer token.
    let dir = temp_dir("headers-duplicate-auth");
    let server = MockServer::start(1);
    let base = server_base(&server);
    install(&dir, "pets", Some(&base));

    let invoke = run_cli(
        &dir,
        &[
            "pets",
            "pets",
            "get-pet",
            "--pet-id",
            "42",
            "-H",
            "Authorization: Token first",
            "-H",
            "authorization: Token second",
        ],
        None,
    );
    assert!(
        invoke.status.success(),
        "{}",
        String::from_utf8_lossy(&invoke.stderr)
    );

    let sent = request_text(&server.request(0));
    assert_eq!(
        header_values(&sent, "authorization"),
        vec!["Token first", "Token second"]
    );
    server.handle.join().unwrap();
}

#[test]
fn custom_generated_headers_win_without_duplication() {
    let dir = temp_dir("headers-precedence");
    let server = MockServer::start(1);
    let base = server_base(&server);
    install(&dir, "pets", Some(&base));

    let body = b"{\"name\":\"rex\"}";
    let invoke = run_cli(
        &dir,
        &[
            "pets",
            "pets",
            "create-pet",
            "-H",
            "User-Agent: custom-agent/2",
            "-H",
            "Accept: text/plain",
            "-H",
            "Content-Type: text/x-test",
        ],
        Some(body),
    );
    assert!(
        invoke.status.success(),
        "{}",
        String::from_utf8_lossy(&invoke.stderr)
    );

    let sent = request_text(&server.request(0));
    assert_eq!(header_values(&sent, "user-agent"), vec!["custom-agent/2"]);
    assert_eq!(header_values(&sent, "accept"), vec!["text/plain"]);
    assert_eq!(header_values(&sent, "content-type"), vec!["text/x-test"]);
    assert!(!sent.to_ascii_lowercase().contains("clining/"), "{sent}");
    server.handle.join().unwrap();
}

#[test]
fn custom_host_is_sent_as_supplied() {
    let dir = temp_dir("headers-host");
    let server = MockServer::start(1);
    let base = server_base(&server);
    install(&dir, "pets", Some(&base));

    let invoke = run_cli(
        &dir,
        &[
            "pets",
            "pets",
            "get-pet",
            "--pet-id",
            "42",
            "-H",
            "Host: example.test",
        ],
        None,
    );
    assert!(
        invoke.status.success(),
        "{}",
        String::from_utf8_lossy(&invoke.stderr)
    );

    let sent = request_text(&server.request(0));
    assert_eq!(header_values(&sent, "host"), vec!["example.test"]);
    server.handle.join().unwrap();
}

#[test]
fn custom_authorization_overrides_install_time_bearer_auth() {
    let dir = temp_dir("headers-auth-override");
    let server = MockServer::start(1);
    let base = server_base(&server);
    install(&dir, "pets", Some(&base));

    // Point the model at a token source that is never set: the custom header
    // must bypass resolution and still send the request.
    let model_path = dir.join("pets.json");
    let mut model: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&model_path).unwrap()).unwrap();
    model["auth"] =
        serde_json::json!({"source": "env", "var": "CLINING_E2E_DEFINITELY_UNSET_HEADERS_XYZ"});
    std::fs::write(&model_path, serde_json::to_string_pretty(&model).unwrap()).unwrap();

    let invoke = run_cli(
        &dir,
        &[
            "pets",
            "pets",
            "get-pet",
            "--pet-id",
            "42",
            "-H",
            "Authorization: Token custom-secret",
        ],
        None,
    );
    assert!(
        invoke.status.success(),
        "{}",
        String::from_utf8_lossy(&invoke.stderr)
    );

    let sent = request_text(&server.request(0));
    assert_eq!(
        header_values(&sent, "authorization"),
        vec!["Token custom-secret"]
    );
    assert!(!sent.to_ascii_lowercase().contains("bearer"), "{sent}");
    server.handle.join().unwrap();
}

#[test]
fn invalid_headers_fail_before_any_request_with_one_error_line() {
    let dir = temp_dir("headers-invalid");
    let server = MockServer::start(0);
    let base = server_base(&server);
    install(&dir, "pets", Some(&base));

    let cases = [
        ("Content-Length: 5", "5"),
        ("cOnNeCtIoN: close", "close"),
        ("X-Test: caf\u{e9}", "caf\u{e9}"),
        ("X-Test: s3cr3t\r\nX-Evil: 1", "s3cr3t"),
        ("X-NoColon", "X-NoColon"),
        (": value", "value"),
    ];

    for (raw, secret) in cases {
        let invoke = run_cli(
            &dir,
            &["pets", "pets", "get-pet", "--pet-id", "42", "-H", raw],
            None,
        );
        assert!(!invoke.status.success(), "{raw:?}: unexpectedly succeeded");
        assert_eq!(invoke.status.code(), Some(1), "{raw:?}");
        let stderr = String::from_utf8_lossy(&invoke.stderr);
        assert_eq!(
            stderr
                .lines()
                .filter(|line| line.starts_with("error:"))
                .count(),
            1,
            "{raw:?}: {stderr}"
        );
        assert!(
            !stderr.to_ascii_lowercase().contains("network error"),
            "{raw:?}: {stderr}"
        );
        assert!(!stderr.contains(secret), "{raw:?}: {stderr}");
        assert!(invoke.stdout.is_empty(), "{raw:?}");
    }

    server.handle.join().unwrap();
}

#[test]
fn missing_custom_header_value_is_a_clap_error() {
    let dir = temp_dir("headers-missing-value");
    install(&dir, "pets", None);

    let invoke = run_cli(
        &dir,
        &["pets", "pets", "get-pet", "--pet-id", "42", "-H"],
        None,
    );
    assert!(!invoke.status.success());
    assert_eq!(invoke.status.code(), Some(2));
}
