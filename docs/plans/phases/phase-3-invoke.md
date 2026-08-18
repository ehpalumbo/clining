---
type: workflow
title: "Phase 3 — \"Invoke\" Vertical Slice"
description: "Implements the invoke flow: resolving a command from the stored model, building the HTTP request from params and stdin body, sending it, and streaming the response."
tags:
  - "clining"
  - "implementation-plan"
  - "phase-3"
timestamp: "2026-08-17T19:47:50Z"
related:
  - "[Phased Implementation Plan](../implementation-plan.md)"
---

# Phase 3 — "Invoke" Vertical Slice

**Status:** Completed

## Overview

The second core flow: `clining <name> <group> <command> --param value < body.json` performs a real HTTP request. Adds the request builder, the reqwest adapter, the runtime-built clap tree, response streaming, and exit-code semantics. Satisfies FR-3 and completes FR-5's invoke-side behavior.

Implementation notes: `HttpRequest`/`HttpResponse`/`HttpInvoker` are domain-agnostic (no infra imports). The request builder substitutes path params (encoding values), expands query params (repeated values become repeated keys), and enforces body requiredness/unexpected-body rules. `InvokeCommandService` loads the model via `ApiStore`, resolves group/command with errors that list valid names, builds the request, and sends it. The CLI dispatches on the first positional: `install` → static path; anything else loads the model and builds a dynamic clap tree (`<group>` → `<command>` with `--long` args, `ArgAction::Append` for repeatable query params, path params required). `NotFound` now carries the store path so unknown-API errors point at `~/.clining/`; corrupt stored models surface as `InvalidStoredModel`, never `NotFound`. Response body is written byte-exact to stdout, an `HTTP/1.1 <status>` line plus headers go to stderr, and the exit code is `0` on 2xx and `1` otherwise.

## Task Details

### 1. Extend the domain with request/response types and the `HttpInvoker` port

- **Prerequisites / Dependencies:** Phase 2.
- **Affected Files:**
  - `src/domain/model.rs`
  - `src/domain/ports.rs`
- **Affected Symbols:** `HttpRequest`, `HttpResponse`, `HttpInvoker::send`
- **Description:** Domain-agnostic `HttpRequest { method, url, headers, body }` and `HttpResponse { status, headers, body }`; `HttpInvoker` port returning domain errors. Added so the request builder and use case depend only on the domain.
- **Acceptance Criteria:**
  - [x] Port and types compile with no infrastructure imports.

### 2. Implement the pure request builder

- **Prerequisites / Dependencies:** Task 1.
- **Affected Files:**
  - `src/application/request_builder.rs`
- **Affected Symbols:** `build_request(command, params, body)`
- **Description:** Substitute `{param}` path segments with supplied values; serialize query params (repeated values → repeated query keys); attach body bytes with the content type from `BodySpec` (default `application/json`); set `Accept`/`User-Agent`. Errors: missing required path/query param; body supplied where none declared; required body missing. The builder maps CLI names (`cli_name`) back to the original parameter names for substitution.
- **Acceptance Criteria:**
  - [x] Path template + query expansion correct in unit tests (e.g. `GET /pets/{id}?status=...`).
  - [x] Missing required param → error; repeated param → repeated query key.
  - [x] Required body missing → error; unexpected body → error.

### 3. Implement the reqwest HTTP adapter

- **Prerequisites / Dependencies:** Task 1.
- **Affected Files:**
  - `src/infra/http/reqwest_http.rs`
- **Affected Symbols:** `ReqwestHttpClient`
- **Description:** Blocking reqwest client implementing `HttpInvoker`; maps status + headers + body into `HttpResponse`; network errors surfaced with context.
- **Acceptance Criteria:**
  - [x] Test against a local `std::net::TcpListener` mock: request line, headers, and body match; response roundtrips.

### 4. Implement the InvokeCommandService use case

- **Prerequisites / Dependencies:** Tasks 1–3.
- **Affected Files:**
  - `src/application/invoke_command.rs`
- **Affected Symbols:** `InvokeCommandService::invoke(api_name, group, command, params, body)`
- **Description:** Load model via `ApiStore`; resolve group + command (unknown group/command → targeted error listing valid names); build the request; send via `HttpInvoker`; return the response. A corrupt stored model surfaces as `InvalidStoredModel`, never `NotFound`.
- **Acceptance Criteria:**
  - [x] Fake-port unit tests: successful invoke; unknown group/command errors name valid alternatives.
  - [x] Unknown API name → `NotFound` with the `~/.clining/` path.
  - [x] Corrupt model propagates `InvalidStoredModel` un-masked.

### 5. Build the dynamic clap tree and wire invocation

- **Prerequisites / Dependencies:** Task 4.
- **Affected Files:**
  - `src/infra/cli/clap_cli.rs`
  - `src/main.rs`
- **Affected Symbols:** `build_api_command(model)`, `run_invoke`, `Action`
- **Description:** The top level dispatches on the first positional: `install` → static path; otherwise load the model and build a clap tree (`<group>` → `<command>` with per-parameter `--long` args, `ArgAction::Append` for repeats, path params required). Read stdin body before invoking. On success: write body bytes to stdout (byte-exact, no trailing-newline mangling); write `HTTP/1.1 <status>` line + headers to stderr; exit `0` on 2xx else `1`.
- **Acceptance Criteria:**
  - [x] `clining pets pets get-pets --status available < body.json` hits the correct URL with query and body; body → stdout; status line + headers → stderr.
  - [x] 200 → exit 0; 404 → exit 1 with status on stderr.
  - [x] Binary-safe: arbitrary response body bytes reach stdout unmodified.
  - [x] End-to-end test against a local mock server (install fixture → invoke → assert stdout/stderr/exit-code split).

## Verification Plan

1. `cargo fmt --check`; `cargo clippy --all-targets -- -D warnings`; `cargo test`.
2. Manual E2E: install a fixture spec pointing at a local mock server; invoke commands; verify stdout body, stderr status + headers, exit codes for 2xx and 4xx.

## Phase Definition of Done

- [x] Invocation resolves, builds, sends, and streams correctly.
- [x] Exit codes: 0 on 2xx, 1 on non-2xx.
- [x] `NotFound` vs `InvalidStoredModel` correct in invoke paths.
- [x] Clippy-clean, formatted, all tests pass.
