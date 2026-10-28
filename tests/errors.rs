//! End-to-end error-UX and edge-case tests (Phase 4): every failure path exits
//! non-zero with a one-line `error:` message on stderr.

mod common;

use std::fs;

use common::{install, run_cli, temp_dir};

#[test]
fn unknown_api_in_empty_store_is_helpful() {
    let dir = temp_dir("empty-store");
    fs::create_dir_all(&dir).unwrap();

    let invoke = run_cli(&dir, &["nope", "store", "get-pets"], None);
    assert!(!invoke.status.success());
    assert_eq!(invoke.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&invoke.stderr);
    assert!(
        stderr.contains("no API installed under the name 'nope'"),
        "{stderr}"
    );
    assert!(stderr.contains("install it first"), "{stderr}");
    assert!(
        stderr.contains("clining api install <name> <spec>"),
        "{stderr}"
    );
}

#[test]
fn zero_args_prints_help_listing_and_exits_nonzero() {
    let dir = temp_dir("zero-args");
    fs::create_dir_all(&dir).unwrap();

    let out = run_cli(&dir, &[], None);
    assert!(!out.status.success());
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("Usage"), "{stderr}");
    assert!(stderr.contains("api"), "{stderr}");
}

#[test]
fn missing_required_body_is_rejected_before_network() {
    let dir = temp_dir("missing-body");
    install(&dir, "pets", Some("http://127.0.0.1:9"));

    let invoke = run_cli(&dir, &["pets", "pets", "get-pets"], None);
    assert!(!invoke.status.success());
    assert_eq!(invoke.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&invoke.stderr);
    assert!(stderr.contains("requires a request body"), "{stderr}");
    assert!(!stderr.contains("network error"), "{stderr}");
}

#[test]
fn unexpected_body_is_rejected() {
    let dir = temp_dir("unexpected-body");
    install(&dir, "pets", Some("http://127.0.0.1:9"));

    let invoke = run_cli(
        &dir,
        &["pets", "pets", "get-pet", "--pet-id", "42"],
        Some(b"{}"),
    );
    assert!(!invoke.status.success());
    assert_eq!(invoke.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&invoke.stderr);
    assert!(stderr.contains("declares no request body"), "{stderr}");
}

#[test]
fn missing_required_path_param_is_a_clap_error() {
    let dir = temp_dir("missing-param");
    install(&dir, "pets", None);

    let invoke = run_cli(&dir, &["pets", "pets", "get-pet"], None);
    assert!(!invoke.status.success());
    assert_eq!(invoke.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&invoke.stderr);
    assert!(stderr.contains("--pet-id"), "{stderr}");
}

#[test]
fn invalid_custom_header_is_rejected_before_network() {
    let dir = temp_dir("invalid-header");
    install(&dir, "pets", Some("http://127.0.0.1:9"));

    let invoke = run_cli(
        &dir,
        &[
            "pets",
            "pets",
            "get-pet",
            "--pet-id",
            "42",
            "-H",
            "Connection: keep-alive",
        ],
        None,
    );
    assert!(!invoke.status.success());
    assert_eq!(invoke.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&invoke.stderr);
    assert_eq!(
        stderr
            .lines()
            .filter(|line| line.starts_with("error:"))
            .count(),
        1,
        "{stderr}"
    );
    assert!(stderr.contains("invalid header:"), "{stderr}");
    assert!(!stderr.contains("keep-alive"), "{stderr}");
    assert!(!stderr.contains("network error"), "{stderr}");
    assert!(invoke.stdout.is_empty());
}

#[test]
fn reinstall_overwrites_existing_model() {
    let dir = temp_dir("reinstall");
    install(&dir, "pets", None);
    install(&dir, "pets", Some("http://replacement.example.com"));

    let model = fs::read_to_string(dir.join("pets.json")).unwrap();
    let value: serde_json::Value = serde_json::from_str(&model).unwrap();
    assert_eq!(value["base_url"], "http://replacement.example.com");
    assert_eq!(value["operation_groups"].as_array().unwrap().len(), 3);
}

#[test]
fn spec_source_not_found_is_descriptive() {
    let dir = temp_dir("spec-missing");
    let invoke = run_cli(
        &dir,
        &["api", "install", "pets", "/definitely/not/here/spec.json"],
        None,
    );
    assert!(!invoke.status.success());
    assert_eq!(invoke.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&invoke.stderr);
    assert!(stderr.contains("error:"), "{stderr}");
    assert!(stderr.contains("spec"), "{stderr}");
}

#[test]
fn reserved_name_api_is_rejected_without_persisting() {
    let dir = temp_dir("reserved-name");
    let spec = dir.join("spec.json");
    fs::write(&spec, common::fixture_spec()).unwrap();

    let out = run_cli(
        &dir,
        &["api", "install", "api", spec.to_str().unwrap()],
        None,
    );
    assert!(!out.status.success());
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    let error_lines = stderr
        .lines()
        .filter(|line| line.starts_with("error:"))
        .count();
    assert_eq!(error_lines, 1, "{stderr}");
    assert!(
        stderr.contains("reserved command group name 'api'"),
        "{stderr}"
    );
    assert!(out.stdout.is_empty(), "{:?}", out.stdout);
    assert!(!dir.join("api.json").exists(), "nothing must be persisted");
}

#[test]
fn api_group_help_lists_install_and_exits_zero() {
    let dir = temp_dir("api-group-help");

    let out = run_cli(&dir, &["api", "--help"], None);
    assert!(out.status.success());
    assert_eq!(out.status.code(), Some(0));
    assert!(
        out.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("install"), "{stdout}");
    assert!(stdout.contains("Manage installed API models"), "{stdout}");
}

#[test]
fn api_unknown_verb_is_a_clap_usage_error() {
    let dir = temp_dir("api-unknown-verb");

    let out = run_cli(&dir, &["api", "frobnicate"], None);
    assert!(!out.status.success());
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("Usage"), "{stderr}");
    assert!(out.stdout.is_empty(), "{:?}", out.stdout);
}

#[test]
fn bare_api_is_a_clap_usage_error() {
    let dir = temp_dir("api-bare");

    let out = run_cli(&dir, &["api"], None);
    assert!(!out.status.success());
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("Usage"), "{stderr}");
    assert!(out.stdout.is_empty(), "{:?}", out.stdout);
}

#[test]
fn top_level_install_word_treated_as_missing_api_name_with_hint() {
    let dir = temp_dir("install-migration-hint");
    let spec = dir.join("spec.json");
    fs::write(&spec, common::fixture_spec()).unwrap();

    let out = run_cli(&dir, &["install", "pets", spec.to_str().unwrap()], None);
    assert!(!out.status.success());
    // Not a usage error (exit 2): there is no top-level install, so the word
    // `install` is an ordinary missing API name; the hint points at the group.
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    let error_lines = stderr
        .lines()
        .filter(|line| line.starts_with("error:"))
        .count();
    assert_eq!(error_lines, 1, "{stderr}");
    assert!(
        stderr.contains("no API installed under the name 'install'"),
        "{stderr}"
    );
    assert!(
        stderr.contains("clining api install <name> <spec>"),
        "{stderr}"
    );
    assert!(out.stdout.is_empty(), "{:?}", out.stdout);
    assert!(!dir.join("pets.json").exists(), "nothing must be persisted");
}
