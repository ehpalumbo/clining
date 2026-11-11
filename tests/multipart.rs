//! End-to-end multipart/form-data tests: install the upload fixture, invoke the
//! command against a local mock server, and assert the exact wire bytes,
//! stream/exit-code contract, override precedence, and error paths.

mod common;

use std::fs;
use std::process::Output;

use common::{BINARY_BODY, MockServer, install_multipart, request_text, run_cli, temp_dir};

fn server_base(server: &MockServer) -> String {
    format!("http://{}", server.addr)
}

fn header_values(head: &[u8], name: &str) -> Vec<String> {
    let head = String::from_utf8_lossy(head);
    head.lines()
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

fn split_request(bytes: &[u8]) -> (&[u8], &[u8]) {
    let end = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("request has a header terminator");
    (&bytes[..end], &bytes[end + 4..])
}

fn multipart_boundary(head: &[u8]) -> String {
    let content_type = header_values(head, "content-type")
        .into_iter()
        .next()
        .expect("request has a Content-Type header");
    content_type
        .split_once("boundary=")
        .map(|(_, boundary)| boundary.trim().to_owned())
        .expect("multipart Content-Type has a boundary")
}

fn count_occurrences(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .filter(|window| *window == needle)
        .count()
}

fn position(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
        .unwrap_or_else(|| panic!("missing {:?}", String::from_utf8_lossy(needle)))
}

fn assert_error(invoke: Output, needle: &str) {
    assert!(!invoke.status.success(), "unexpected success");
    assert_eq!(invoke.status.code(), Some(1));
    assert!(invoke.stdout.is_empty(), "stdout must stay empty");
    let stderr = String::from_utf8_lossy(&invoke.stderr);
    assert_eq!(
        stderr
            .lines()
            .filter(|line| line.starts_with("error:"))
            .count(),
        1,
        "{stderr}"
    );
    assert!(stderr.contains(needle), "{stderr}");
    assert!(
        !stderr.to_ascii_lowercase().contains("network error"),
        "{stderr}"
    );
}

fn upload_file(dir: &std::path::Path) -> std::path::PathBuf {
    let path = dir.join("cat.bin");
    fs::write(&path, BINARY_BODY).unwrap();
    path
}

#[test]
fn multipart_upload_sends_ordered_frames_with_binary_safe_file_bytes() {
    let dir = temp_dir("multipart-happy");
    let server = MockServer::start(1);
    let base = server_base(&server);
    assert!(install_multipart(&dir, "uploads", &base).status.success());
    let photo = upload_file(&dir);

    let invoke = run_cli(
        &dir,
        &[
            "uploads",
            "uploads",
            "create-upload",
            "-F",
            "name=fluffy",
            "-F",
            &format!("photo=@{}", photo.display()),
        ],
        None,
    );
    assert!(
        invoke.status.success(),
        "{}",
        String::from_utf8_lossy(&invoke.stderr)
    );
    assert_eq!(invoke.stdout, b"{\"uploadId\":9}");
    assert!(
        String::from_utf8_lossy(&invoke.stderr).contains("HTTP/1.1 201"),
        "{}",
        String::from_utf8_lossy(&invoke.stderr)
    );

    let request = server.request(0);
    let (head, body) = split_request(&request);
    let boundary = multipart_boundary(head);
    assert!(boundary.starts_with("clining-"), "{boundary}");

    let expected = [
        format!("--{boundary}\r\n"),
        "Content-Disposition: form-data; name=\"name\"\r\n".to_owned(),
        "\r\n".to_owned(),
        "fluffy\r\n".to_owned(),
        format!("--{boundary}\r\n"),
        "Content-Disposition: form-data; name=\"photo\"; filename=\"cat.bin\"\r\n".to_owned(),
        "Content-Type: image/jpeg\r\n".to_owned(),
        "\r\n".to_owned(),
    ]
    .concat()
    .into_bytes()
    .into_iter()
    .chain(BINARY_BODY.iter().copied())
    .chain(format!("\r\n--{boundary}--\r\n").into_bytes())
    .collect::<Vec<u8>>();
    assert_eq!(body, expected.as_slice());
    server.handle.join().unwrap();
}

#[test]
fn caller_type_overrides_the_spec_derived_part_content_type() {
    let dir = temp_dir("multipart-type-override");
    let server = MockServer::start(1);
    let base = server_base(&server);
    assert!(install_multipart(&dir, "uploads", &base).status.success());
    let photo = upload_file(&dir);

    let invoke = run_cli(
        &dir,
        &[
            "uploads",
            "uploads",
            "create-upload",
            "-F",
            "name=fluffy",
            "-F",
            &format!("photo=@{};type=application/x-custom", photo.display()),
        ],
        None,
    );
    assert!(
        invoke.status.success(),
        "{}",
        String::from_utf8_lossy(&invoke.stderr)
    );

    let request = server.request(0);
    let (_, body) = split_request(&request);
    assert!(
        body.windows(b"Content-Type: application/x-custom".len())
            .any(|window| window == b"Content-Type: application/x-custom"),
        "{}",
        request_text(body)
    );
    assert!(
        !body
            .windows(b"Content-Type: image/jpeg".len())
            .any(|window| window == b"Content-Type: image/jpeg")
    );
    server.handle.join().unwrap();
}

#[test]
fn repeated_field_names_produce_two_parts_in_input_order() {
    let dir = temp_dir("multipart-repeated");
    let server = MockServer::start(1);
    let base = server_base(&server);
    assert!(install_multipart(&dir, "uploads", &base).status.success());

    let invoke = run_cli(
        &dir,
        &[
            "uploads",
            "uploads",
            "create-upload",
            "-F",
            "name=first",
            "-F",
            "name=second",
        ],
        None,
    );
    assert!(
        invoke.status.success(),
        "{}",
        String::from_utf8_lossy(&invoke.stderr)
    );

    let request = server.request(0);
    let (_, body) = split_request(&request);
    assert_eq!(count_occurrences(body, b"name=\"name\""), 2);
    assert!(position(body, b"first\r\n") < position(body, b"second\r\n"));
    server.handle.join().unwrap();
}

#[test]
fn stdin_is_sent_raw_when_a_custom_content_type_is_supplied() {
    let dir = temp_dir("multipart-override");
    let server = MockServer::start(1);
    let base = server_base(&server);
    assert!(install_multipart(&dir, "uploads", &base).status.success());

    let body = b"{\"raw\":true}";
    let invoke = run_cli(
        &dir,
        &[
            "uploads",
            "uploads",
            "create-upload",
            "-H",
            "Content-Type: application/json",
        ],
        Some(body),
    );
    assert!(
        invoke.status.success(),
        "{}",
        String::from_utf8_lossy(&invoke.stderr)
    );

    let request = server.request(0);
    let (head, sent_body) = split_request(&request);
    assert_eq!(sent_body, body);
    assert_eq!(
        header_values(head, "content-type"),
        vec!["application/json"]
    );
    server.handle.join().unwrap();
}

#[test]
fn form_parts_with_a_custom_content_type_are_rejected_without_a_request() {
    let dir = temp_dir("multipart-conflict");
    let server = MockServer::start(0);
    let base = server_base(&server);
    assert!(install_multipart(&dir, "uploads", &base).status.success());

    let invoke = run_cli(
        &dir,
        &[
            "uploads",
            "uploads",
            "create-upload",
            "-F",
            "name=fluffy",
            "-H",
            "Content-Type: application/json",
        ],
        None,
    );
    assert_error(
        invoke,
        "cannot combine -F form parts with a custom Content-Type header",
    );
    server.handle.join().unwrap();
}

#[test]
fn bare_stdin_on_a_multipart_command_is_rejected_without_a_request() {
    let dir = temp_dir("multipart-stdin-rejected");
    let server = MockServer::start(0);
    let base = server_base(&server);
    assert!(install_multipart(&dir, "uploads", &base).status.success());

    let invoke = run_cli(
        &dir,
        &["uploads", "uploads", "create-upload"],
        Some(b"anything"),
    );
    assert_error(
        invoke,
        "takes multipart form fields; pass -F <field>=@<path> parts",
    );
    server.handle.join().unwrap();
}

#[test]
fn missing_required_multipart_body_hints_at_form_parts() {
    let dir = temp_dir("multipart-required");
    let server = MockServer::start(0);
    let base = server_base(&server);
    assert!(install_multipart(&dir, "uploads", &base).status.success());

    let invoke = run_cli(&dir, &["uploads", "uploads", "create-upload"], None);
    assert_error(invoke, "-F <field>=<value>");
    server.handle.join().unwrap();
}

#[test]
fn unreadable_form_file_is_a_single_line_io_error_without_a_request() {
    let dir = temp_dir("multipart-missing-file");
    let server = MockServer::start(0);
    let base = server_base(&server);
    assert!(install_multipart(&dir, "uploads", &base).status.success());

    let invoke = run_cli(
        &dir,
        &[
            "uploads",
            "uploads",
            "create-upload",
            "-F",
            "name=fluffy",
            "-F",
            "photo=@/definitely/not/here.png",
        ],
        None,
    );
    assert_error(invoke, "failed to read form file");
    server.handle.join().unwrap();
}

#[test]
fn malformed_form_part_is_a_single_line_error_without_a_request() {
    let dir = temp_dir("multipart-malformed");
    let server = MockServer::start(0);
    let base = server_base(&server);
    assert!(install_multipart(&dir, "uploads", &base).status.success());

    for raw in ["name", "=value", "photo=@", "name=x;type=text/plain"] {
        let invoke = run_cli(
            &dir,
            &["uploads", "uploads", "create-upload", "-F", raw],
            None,
        );
        assert_error(invoke, "invalid form part:");
    }
    server.handle.join().unwrap();
}

#[test]
fn help_documents_the_form_flag_guidance_and_part_metadata() {
    let dir = temp_dir("multipart-help");
    assert!(
        install_multipart(&dir, "uploads", "https://api.example.com")
            .status
            .success()
    );

    let invoke = run_cli(
        &dir,
        &["uploads", "uploads", "create-upload", "--help"],
        None,
    );
    assert!(
        invoke.status.success(),
        "{}",
        String::from_utf8_lossy(&invoke.stderr)
    );
    let help = String::from_utf8_lossy(&invoke.stdout);
    assert!(help.contains("-F, --clining-form <FORM>"), "{help}");
    assert!(help.contains("Parts: -F '<field>=<value>'"), "{help}");
    assert!(help.contains("name: text (required)"), "{help}");
    assert!(help.contains("photo: file upload"), "{help}");
    assert!(help.contains("content-type: image/jpeg"), "{help}");
    assert!(help.contains("tags: text (optional; repeatable)"), "{help}");
}

#[test]
fn json_body_commands_do_not_expose_the_form_flag() {
    let dir = temp_dir("multipart-non-multipart-help");
    common::install(&dir, "pets", None);
    let invoke = run_cli(&dir, &["pets", "pets", "create-pet", "--help"], None);
    assert!(invoke.status.success());
    let help = String::from_utf8_lossy(&invoke.stdout);
    assert!(!help.contains("--clining-form"), "{help}");
    assert!(!help.contains("Parts: -F"), "{help}");
    assert!(!help.contains("-F <FORM>"), "{help}");
}

#[test]
fn form_parts_on_a_json_body_command_are_rejected_without_a_request() {
    let dir = temp_dir("multipart-json-rejected");
    let server = MockServer::start(0);
    let base = server_base(&server);
    common::install(&dir, "pets", Some(&base));

    let invoke = run_cli(
        &dir,
        &["pets", "pets", "create-pet", "-F", "name=fluffy"],
        None,
    );
    assert_error(
        invoke,
        "-F form parts support multipart/form-data commands only",
    );
    server.handle.join().unwrap();
}
