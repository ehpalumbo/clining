//! End-to-end discovery tests (Phase 4 + issue #5): `--help` at the API,
//! group, and command levels renders spec-derived descriptions, parameter
//! value shapes, request bodies, and response schemas without sending a request.

mod common;

use common::{install, run_cli, temp_dir};

#[test]
fn api_help_lists_groups_with_descriptions() {
    let dir = temp_dir("help-api");
    install(&dir, "pets", None);

    let out = run_cli(&dir, &["pets", "--help"], None);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Commands for API 'pets'"), "{stdout}");
    assert!(stdout.contains("pets"), "{stdout}");
    assert!(stdout.contains("store"), "{stdout}");
    assert!(stdout.contains("default"), "{stdout}");
    assert!(stdout.contains("Everything about your pets"), "{stdout}");
    assert!(stdout.contains("Access to store orders"), "{stdout}");
}

#[test]
fn group_help_lists_commands_with_summaries() {
    let dir = temp_dir("help-group");
    install(&dir, "pets", None);

    let out = run_cli(&dir, &["pets", "pets", "--help"], None);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("get-pets"), "{stdout}");
    assert!(stdout.contains("List pets"), "{stdout}");
    assert!(stdout.contains("create-pet"), "{stdout}");
    assert!(stdout.contains("get-pet"), "{stdout}");
    assert!(stdout.contains("Get a pet by id"), "{stdout}");
    assert!(stdout.contains("get-binary"), "{stdout}");
}

#[test]
fn untagged_operation_appears_in_default_group_help() {
    let dir = temp_dir("help-default");
    install(&dir, "pets", None);

    let out = run_cli(&dir, &["pets", "default", "--help"], None);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("get-ping"), "{stdout}");
    assert!(stdout.contains("Health check"), "{stdout}");
}

#[test]
fn command_help_shows_summary_request_line_and_parameter_shapes() {
    let dir = temp_dir("help-command");
    install(&dir, "pets", None);

    let out = run_cli(&dir, &["pets", "pets", "get-pets", "--help"], None);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("List pets"), "{stdout}");
    assert!(stdout.contains("Request: GET /pets"), "{stdout}");
    assert!(stdout.contains("--status"), "{stdout}");
    assert!(stdout.contains("Filter by status"), "{stdout}");
    assert!(stdout.contains("[optional]"), "{stdout}");
    assert!(
        stdout.contains(
            r#"type: string; enum: "available", "pending", "sold"; example: "available""#
        ),
        "{stdout}"
    );
    assert!(stdout.contains("--tag"), "{stdout}");
    assert!(stdout.contains("type: array<string>"), "{stdout}");
}

#[test]
fn command_help_shows_body_walkthrough_with_required_fields() {
    let dir = temp_dir("help-body");
    install(&dir, "pets", None);

    let out = run_cli(&dir, &["pets", "pets", "create-pet", "--help"], None);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Request body"), "{stdout}");
    assert!(stdout.contains("application/json (required)"), "{stdout}");
    assert!(stdout.contains("Body (Pet, object)"), "{stdout}");
    assert!(
        stdout.contains("id (integer, required): Pet id [minimum: 1]"),
        "{stdout}"
    );
    assert!(
        stdout.contains("name (string, required): Pet name [minLength: 1]"),
        "{stdout}"
    );
    assert!(
        stdout.contains(r#"status (string): Pet status in the store [enum: "available", "pending", "sold"; example: "available"]"#),
        "{stdout}"
    );
    assert!(stdout.contains("owner (Owner, object)"), "{stdout}");
    assert!(
        stdout.contains("name (string, required): Owner display name"),
        "{stdout}"
    );
    assert!(!stdout.contains("$ref"), "{stdout}");
}

#[test]
fn command_help_shows_required_marker_and_description() {
    let dir = temp_dir("help-required");
    install(&dir, "pets", None);

    let out = run_cli(&dir, &["pets", "pets", "get-pet", "--help"], None);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("--pet-id"), "{stdout}");
    assert!(stdout.contains("Numeric id of the pet"), "{stdout}");
    assert!(stdout.contains("[required]"), "{stdout}");
    assert!(
        stdout.contains("type: integer; minimum: 1; example: 42"),
        "{stdout}"
    );
}

#[test]
fn command_help_shows_resolved_reference_schema_and_responses() {
    let dir = temp_dir("help-ref");
    install(&dir, "pets", None);

    let out = run_cli(&dir, &["pets", "store", "place-order", "--help"], None);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Request: POST /store/orders"), "{stdout}");
    assert!(stdout.contains("Body (Order, object)"), "{stdout}");
    assert!(
        stdout.contains("id (integer, required): Order id [minimum: 1]"),
        "{stdout}"
    );
    assert!(
        stdout.contains("quantity (integer): Number of units [minimum: 1; default: 1]"),
        "{stdout}"
    );
    assert!(stdout.contains("Responses"), "{stdout}");
    assert!(stdout.contains("200 application/json"), "{stdout}");
    assert!(stdout.contains("404 application/json"), "{stdout}");
    assert!(stdout.contains("Body (Error, object)"), "{stdout}");
    assert!(
        stdout.contains("message (string, required): Human-readable error message"),
        "{stdout}"
    );
    assert!(!stdout.contains("$ref"), "{stdout}");
}

#[test]
fn command_help_renders_array_response_schemas() {
    let dir = temp_dir("help-array-response");
    install(&dir, "pets", None);

    let out = run_cli(&dir, &["pets", "pets", "get-pets", "--help"], None);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("200 application/json"), "{stdout}");
    assert!(stdout.contains("Body (array<Pet>)"), "{stdout}");
    assert!(stdout.contains("items (Pet, object)"), "{stdout}");
    assert!(stdout.contains("tags (array<string>)"), "{stdout}");
    assert!(stdout.contains("owner (Owner, object)"), "{stdout}");
    assert!(stdout.contains("400 application/json"), "{stdout}");
    assert!(!stdout.contains("$ref"), "{stdout}");
}

#[test]
fn command_help_lists_contentless_response_status_codes() {
    let dir = temp_dir("help-no-content");
    install(&dir, "pets", None);

    let out = run_cli(&dir, &["pets", "default", "get-ping", "--help"], None);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Responses"), "{stdout}");
    assert!(stdout.contains("204"), "{stdout}");
    assert!(!stdout.contains("schema not documented"), "{stdout}");
}

#[test]
fn command_help_does_not_send_a_request() {
    let dir = temp_dir("help-read-only");
    // Port 1 is not serving HTTP; a request attempt would fail and exit 1.
    install(&dir, "pets", Some("http://127.0.0.1:1"));

    let out = run_cli(&dir, &["pets", "store", "place-order", "--help"], None);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Request: POST /store/orders"), "{stdout}");
}
