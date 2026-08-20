//! End-to-end discovery tests (Phase 4): `--help` at the API, group, and
//! command levels renders spec-derived descriptions, params, and body hints.

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
fn command_help_shows_params_and_body_schema() {
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
    assert!(stdout.contains("--status"), "{stdout}");
    assert!(stdout.contains("Filter by status"), "{stdout}");
    assert!(stdout.contains("--tag"), "{stdout}");
    assert!(stdout.contains("Request: GET /pets"), "{stdout}");
    assert!(stdout.contains("application/json"), "{stdout}");
    assert!(stdout.contains("required"), "{stdout}");
    assert!(stdout.contains("schema: {\"type\":\"object\"}"), "{stdout}");
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
}

#[test]
fn command_help_shows_resolved_reference_schema() {
    let dir = temp_dir("help-ref");
    install(&dir, "pets", None);

    let out = run_cli(&dir, &["pets", "store", "place-order", "--help"], None);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("application/json"), "{stdout}");
    assert!(stdout.contains("schema: {\"properties\":"), "{stdout}");
    assert!(stdout.contains("\"required\":true"), "{stdout}");
    assert!(stdout.contains("\"description\":\"Order id\""), "{stdout}");
    assert!(!stdout.contains("\"$ref\""), "{stdout}");
}
