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
}

#[test]
fn zero_args_prints_help_and_exits_nonzero() {
    let dir = temp_dir("zero-args");
    fs::create_dir_all(&dir).unwrap();

    let out = run_cli(&dir, &[], None);
    assert!(!out.status.success());
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("Usage"), "{stderr}");
    assert!(stderr.contains("install"), "{stderr}");
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
        &["install", "pets", "/definitely/not/here/spec.json"],
        None,
    );
    assert!(!invoke.status.success());
    assert_eq!(invoke.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&invoke.stderr);
    assert!(stderr.contains("error:"), "{stderr}");
    assert!(stderr.contains("spec"), "{stderr}");
}
