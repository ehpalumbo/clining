//! End-to-end Bearer auth tests: install with `--token-env` / `--token-file`,
//! assert the `Authorization` header reaches the mock server, and verify that
//! unresolvable sources fail without sending a request.

mod common;

use common::{
    MockServer, install, request_text, run_cli, run_cli_with_envs, run_cli_with_envs_in_cwd,
    temp_dir,
};
use std::fs;

fn server_base(server: &MockServer) -> String {
    format!("http://{}", server.addr)
}

fn install_with_token_env(
    dir: &std::path::Path,
    base: &str,
    var: &str,
    value: &str,
) -> std::process::Output {
    let spec = dir.join("spec.json");
    fs::write(&spec, common::fixture_spec()).unwrap();
    let base_arg = format!("--base-url={base}");
    let token_arg = format!("--token-env={var}");
    run_cli_with_envs(
        dir,
        &[
            "api",
            "install",
            "pets",
            spec.to_str().unwrap(),
            &base_arg,
            &token_arg,
        ],
        None,
        &[(var, value)],
    )
}

fn install_with_token_file(
    dir: &std::path::Path,
    base: &str,
    token_path: &std::path::Path,
) -> std::process::Output {
    let spec = dir.join("spec.json");
    fs::write(&spec, common::fixture_spec()).unwrap();
    let base_arg = format!("--base-url={base}");
    let token_arg = format!("--token-file={}", token_path.display());
    run_cli(
        dir,
        &[
            "api",
            "install",
            "pets",
            spec.to_str().unwrap(),
            &base_arg,
            &token_arg,
        ],
        None,
    )
}

#[test]
fn env_token_reaches_server_as_bearer() {
    let dir = temp_dir("auth-env");
    let server = MockServer::start(1);
    let base = server_base(&server);

    let var = "CLINING_E2E_AUTH_TOKEN";
    let out = install_with_token_env(&dir, &base, var, "env-secret-123");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    // Reference persisted, secret never in the model file.
    let model_raw = fs::read_to_string(dir.join("pets.json")).unwrap();
    assert!(model_raw.contains(var), "{model_raw}");
    assert!(!model_raw.contains("env-secret-123"), "{model_raw}");

    let invoke = run_cli_with_envs(
        &dir,
        &["pets", "pets", "get-pet", "--pet-id", "42"],
        None,
        &[(var, "env-secret-123")],
    );
    assert!(
        invoke.status.success(),
        "{}",
        String::from_utf8_lossy(&invoke.stderr)
    );
    let sent = request_text(&server.request(0)).to_ascii_lowercase();
    assert!(
        sent.contains("authorization: bearer env-secret-123"),
        "{sent}"
    );

    server.handle.join().unwrap();
}

#[test]
fn file_token_reaches_server_with_newline_trimmed() {
    let dir = temp_dir("auth-file");
    let server = MockServer::start(1);
    let base = server_base(&server);

    let token_path = dir.join("token.txt");
    fs::write(&token_path, b"file-secret-456\n").unwrap();
    let out = install_with_token_file(&dir, &base, &token_path);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let model_raw = fs::read_to_string(dir.join("pets.json")).unwrap();
    assert!(!model_raw.contains("file-secret-456"), "{model_raw}");

    let invoke = run_cli(&dir, &["pets", "pets", "get-pet", "--pet-id", "42"], None);
    assert!(
        invoke.status.success(),
        "{}",
        String::from_utf8_lossy(&invoke.stderr)
    );
    let sent = request_text(&server.request(0)).to_ascii_lowercase();
    assert!(
        sent.contains("authorization: bearer file-secret-456"),
        "{sent}"
    );
    // No trailing newline leaks into the header value.
    assert!(!sent.contains("file-secret-456\n"), "{sent}");

    server.handle.join().unwrap();
}

#[test]
fn unresolvable_env_fails_without_sending() {
    let dir = temp_dir("auth-missing-env");
    // Install without auth first (valid), then hand-write an auth reference
    // pointing at a var that is never set, to exercise invoke-time failure.
    install(&dir, "pets", None);
    let model_path = dir.join("pets.json");
    let mut model: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&model_path).unwrap()).unwrap();
    model["auth"] = serde_json::json!({"source": "env", "var": "CLINING_E2E_DEFINITELY_UNSET_XYZ"});
    // Point the model at a live server so that, if the code wrongly sent the
    // request, the mock would observe it. With 0 expected connections the
    // server thread exits immediately; a sent request would fail differently.
    let server = MockServer::start(0);
    let base = server_base(&server);
    model["base_url"] = serde_json::Value::String(base);
    fs::write(&model_path, serde_json::to_string_pretty(&model).unwrap()).unwrap();

    let invoke = run_cli(&dir, &["pets", "pets", "get-pet", "--pet-id", "42"], None);
    assert!(!invoke.status.success());
    assert_eq!(invoke.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&invoke.stderr);
    assert!(stderr.contains("error:"), "{stderr}");
    assert!(
        stderr.to_ascii_lowercase().contains("environment variable"),
        "{stderr}"
    );
    assert!(invoke.stdout.is_empty(), "no body on auth failure");

    server.handle.join().unwrap();
}

#[test]
fn unresolvable_file_fails_without_sending() {
    let dir = temp_dir("auth-missing-file");
    install(&dir, "pets", None);
    let model_path = dir.join("pets.json");
    let mut model: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&model_path).unwrap()).unwrap();
    model["auth"] = serde_json::json!({"source": "file", "path": "/nonexistent-clining-token-xyz"});
    let server = MockServer::start(0);
    let base = server_base(&server);
    model["base_url"] = serde_json::Value::String(base);
    fs::write(&model_path, serde_json::to_string_pretty(&model).unwrap()).unwrap();

    let invoke = run_cli(&dir, &["pets", "pets", "get-pet", "--pet-id", "42"], None);
    assert!(!invoke.status.success());
    assert_eq!(invoke.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&invoke.stderr);
    assert!(stderr.contains("error:"), "{stderr}");
    assert!(
        stderr.to_ascii_lowercase().contains("token file"),
        "{stderr}"
    );

    server.handle.join().unwrap();
}

#[test]
fn install_rejects_both_token_sources() {
    let dir = temp_dir("auth-both");
    let spec = dir.join("spec.json");
    fs::write(&spec, common::fixture_spec()).unwrap();
    let out = run_cli(
        &dir,
        &[
            "api",
            "install",
            "pets",
            spec.to_str().unwrap(),
            "--token-env",
            "A",
            "--token-file",
            "/tmp/t",
        ],
        None,
    );
    assert!(!out.status.success());
    // clap conflict → exit code 2.
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn install_validates_token_source() {
    // Unset var at install time must fail and persist nothing.
    let dir = temp_dir("auth-install-validate");
    let spec = dir.join("spec.json");
    fs::write(&spec, common::fixture_spec()).unwrap();
    let out = run_cli(
        &dir,
        &[
            "api",
            "install",
            "pets",
            spec.to_str().unwrap(),
            "--token-env",
            "CLINING_E2E_DEFINITELY_UNSET_INSTALL_XYZ",
        ],
        None,
    );
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("error:"), "{stderr}");
    assert!(!dir.join("pets.json").exists());
}

#[test]
fn reinstall_without_flags_clears_auth() {
    let dir = temp_dir("auth-clear");
    let server = MockServer::start(1);
    let base = server_base(&server);

    let var = "CLINING_E2E_CLEAR_TOKEN";
    let out = install_with_token_env(&dir, &base, var, "s3cret");
    assert!(out.status.success());
    assert!(
        fs::read_to_string(dir.join("pets.json"))
            .unwrap()
            .contains(var)
    );

    // Re-install without token flags (no auth validation needed).
    let out = install(&dir, "pets", Some(&base));
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let model_raw = fs::read_to_string(dir.join("pets.json")).unwrap();
    assert!(!model_raw.contains(var), "{model_raw}");

    // Invoke without any env var now succeeds with no Authorization header.
    let invoke = run_cli(&dir, &["pets", "pets", "get-pet", "--pet-id", "42"], None);
    assert!(
        invoke.status.success(),
        "{}",
        String::from_utf8_lossy(&invoke.stderr)
    );
    let sent = request_text(&server.request(0)).to_ascii_lowercase();
    assert!(!sent.contains("authorization:"), "{sent}");

    server.handle.join().unwrap();
}

#[test]
fn custom_authorization_header_bypasses_unresolvable_token_source() {
    let dir = temp_dir("auth-custom-header-override");
    install(&dir, "pets", None);
    let model_path = dir.join("pets.json");
    let mut model: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&model_path).unwrap()).unwrap();
    model["auth"] = serde_json::json!({
        "source": "env",
        "var": "CLINING_E2E_DEFINITELY_UNSET_CUSTOM_AUTH_XYZ"
    });

    let server = MockServer::start(1);
    let base = server_base(&server);
    model["base_url"] = serde_json::Value::String(base);
    fs::write(&model_path, serde_json::to_string_pretty(&model).unwrap()).unwrap();

    let invoke = run_cli(
        &dir,
        &[
            "pets",
            "pets",
            "get-pet",
            "--pet-id",
            "42",
            "-H",
            "Authorization: Token override-secret",
        ],
        None,
    );
    assert!(
        invoke.status.success(),
        "{}",
        String::from_utf8_lossy(&invoke.stderr)
    );
    let sent = request_text(&server.request(0)).to_ascii_lowercase();
    assert!(
        sent.contains("authorization: token override-secret"),
        "{sent}"
    );
    assert!(!sent.contains("bearer"), "{sent}");

    server.handle.join().unwrap();
}

#[test]
fn relative_token_file_is_stored_absolute_and_invokable_from_other_cwd() {
    let store = temp_dir("auth-rel");
    let work_a = store.join("a");
    let work_b = store.join("b");
    fs::create_dir_all(&work_a).unwrap();
    fs::create_dir_all(&work_b).unwrap();

    let server = MockServer::start(1);
    let base = server_base(&server);

    let spec = store.join("spec.json");
    fs::write(&spec, common::fixture_spec()).unwrap();
    fs::write(work_a.join("tok.txt"), b"rel-secret-789\n").unwrap();

    let base_arg = format!("--base-url={base}");
    let out = run_cli_with_envs_in_cwd(
        &store,
        &work_a,
        &[
            "api",
            "install",
            "pets",
            spec.to_str().unwrap(),
            &base_arg,
            "--token-file=tok.txt",
        ],
        None,
        &[],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let model_raw = fs::read_to_string(store.join("pets.json")).unwrap();
    let model: serde_json::Value = serde_json::from_str(&model_raw).unwrap();
    let stored_path = model["auth"]["path"].as_str().unwrap().to_owned();
    assert!(
        std::path::Path::new(&stored_path).is_absolute(),
        "{stored_path}"
    );
    assert_eq!(
        stored_path,
        work_a.join("tok.txt").to_string_lossy(),
        "{stored_path}"
    );
    assert!(!model_raw.contains("rel-secret-789"), "{model_raw}");

    let invoke = run_cli_with_envs_in_cwd(
        &store,
        &work_b,
        &["pets", "pets", "get-pet", "--pet-id", "42"],
        None,
        &[],
    );
    assert!(
        invoke.status.success(),
        "{}",
        String::from_utf8_lossy(&invoke.stderr)
    );
    let sent = request_text(&server.request(0)).to_ascii_lowercase();
    assert!(
        sent.contains("authorization: bearer rel-secret-789"),
        "{sent}"
    );

    server.handle.join().unwrap();
}
