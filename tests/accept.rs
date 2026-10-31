//! End-to-end `Accept` content-negotiation tests: the generated `Accept`
//! header is derived from the operation's declared success-response media
//! types; `*/*` is sent when the operation declares no success body, and a
//! custom `-H Accept: ...` still overrides the generated value.

mod common;

use common::{MockServer, install, request_text, run_cli, temp_dir};

/// Returns the values of every `Accept` header line in the request text.
fn accept_values(request: &str) -> Vec<String> {
    request
        .lines()
        .skip(1)
        .take_while(|line| !line.is_empty())
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            (name.eq_ignore_ascii_case("Accept")).then(|| value.trim().to_owned())
        })
        .collect()
}

fn expect_success(output: &std::process::Output, what: &str) {
    assert!(
        output.status.success(),
        "{what}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn accept_matches_declared_success_media_type() {
    let dir = temp_dir("accept-declared");
    let server = MockServer::start(1);
    install(&dir, "pets", Some(&format!("http://{}", server.addr)));

    // get-binary declares a single `application/octet-stream` success body.
    let invoke = run_cli(&dir, &["pets", "pets", "get-binary"], None);
    expect_success(&invoke, "get-binary");

    let sent = request_text(&server.request(0));
    assert_eq!(accept_values(&sent), vec!["application/octet-stream"]);
    server.handle.join().unwrap();
}

#[test]
fn accept_wildcard_when_response_declares_no_body() {
    let dir = temp_dir("accept-status-only");
    let server = MockServer::start(1);
    install(&dir, "pets", Some(&format!("http://{}", server.addr)));

    // get-ping declares only a bodyless `204` response.
    let invoke = run_cli(&dir, &["pets", "default", "get-ping"], None);
    expect_success(&invoke, "get-ping");

    let sent = request_text(&server.request(0));
    assert_eq!(accept_values(&sent), vec!["*/*"]);
    server.handle.join().unwrap();
}

#[test]
fn accept_wildcard_for_stored_model_captured_before_responses() {
    let dir = temp_dir("accept-legacy-model");
    let server = MockServer::start(1);
    install(&dir, "pets", Some(&format!("http://{}", server.addr)));

    // Simulate a model learned before response content was recorded: get-pet
    // keeps its command, parameters, and request line but no response specs.
    let model_path = dir.join("pets.json");
    let mut model: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&model_path).unwrap()).unwrap();
    for operation in model["operation_groups"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .flat_map(|group| group["operations"].as_array_mut().unwrap())
    {
        operation["responses"] = serde_json::json!([]);
    }
    std::fs::write(&model_path, serde_json::to_string_pretty(&model).unwrap()).unwrap();

    let invoke = run_cli(&dir, &["pets", "pets", "get-pet", "--pet-id", "42"], None);
    expect_success(&invoke, "get-pet");

    let sent = request_text(&server.request(0));
    assert_eq!(accept_values(&sent), vec!["*/*"]);
    server.handle.join().unwrap();
}

#[test]
fn accept_union_of_media_types_across_success_responses() {
    let dir = temp_dir("accept-union");
    let server = MockServer::start(1);
    install(&dir, "pets", Some(&format!("http://{}", server.addr)));

    // Extend get-pet's model with a second success media type to prove the
    // generated header is a comma-separated union in first-seen order.
    let model_path = dir.join("pets.json");
    let mut model: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&model_path).unwrap()).unwrap();
    let second_content = serde_json::json!({
        "status_code": "200",
        "description": "The requested pet as CSV",
        "content_type": "text/csv"
    });
    for operation in model["operation_groups"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .flat_map(|group| group["operations"].as_array_mut().unwrap())
    {
        if operation["name"] == "get-pet" {
            operation["responses"]
                .as_array_mut()
                .unwrap()
                .push(second_content.clone());
        }
    }
    std::fs::write(&model_path, serde_json::to_string_pretty(&model).unwrap()).unwrap();

    let invoke = run_cli(&dir, &["pets", "pets", "get-pet", "--pet-id", "42"], None);
    expect_success(&invoke, "get-pet");

    let sent = request_text(&server.request(0));
    assert_eq!(accept_values(&sent), vec!["application/json, text/csv"]);
    server.handle.join().unwrap();
}

#[test]
fn custom_accept_header_overrides_spec_derived_value() {
    let dir = temp_dir("accept-override");
    let server = MockServer::start(1);
    install(&dir, "pets", Some(&format!("http://{}", server.addr)));

    // get-binary declares application/octet-stream; the caller forces
    // text/plain and only that one Accept line is sent.
    let invoke = run_cli(
        &dir,
        &["pets", "pets", "get-binary", "-H", "Accept: text/plain"],
        None,
    );
    expect_success(&invoke, "get-binary");

    let sent = request_text(&server.request(0));
    assert_eq!(accept_values(&sent), vec!["text/plain"]);
    assert!(!sent.contains("octet-stream"), "{sent}");
    server.handle.join().unwrap();
}
