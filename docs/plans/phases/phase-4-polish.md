---
type: workflow
title: "Phase 4 — Discovery Polish & Hardening"
description: "Makes clining discoverable and pleasant for agents: enriched help, hardened error UX, end-to-end tests, and the README and Makefile."
tags:
  - "clining"
  - "implementation-plan"
  - "phase-4"
timestamp: "2026-08-17T19:47:50Z"
related:
  - "[Phased Implementation Plan](../implementation-plan.md)"
---

# Phase 4 — Discovery Polish & Hardening

**Status:** Completed

## Overview

Makes the tool discoverable for agents: meaningful help at every level, schema hints for request bodies, clear error UX, and an end-to-end test suite plus README and Makefile. No new architectural surface. Satisfies FR-4 and the FR-5 hardening items.

Implementation notes: `DescribeService` (application layer) produces structured help data (`ModelHelp`/`GroupHelp`/`CommandHelp`/`ParamHelp`/`BodyHelp`) consumed by the clap tree builder for `about`, parameter help, and a command-level footer (`Request: <METHOD> <path>` plus body content type, requiredness, and a schema summary derived from the stored raw schema). `ApiOperationGroup` and `Param` now carry optional descriptions captured by the parser from OpenAPI tags and parameter descriptions. Error UX hardening: `NotFound` errors now carry an "install it first" hint, the invoke-path debug line was removed so stderr carries only the contract output, and stdout writes treat a closed pipe (`SIGPIPE`) as success while other write failures exit non-zero. Integration tests share a `tests/common` harness (in-process `TcpListener` mock server, `CARGO_BIN_EXE_clining` driver, fixture loader) and cover help at all levels, happy/non-2xx/unknown paths, repeated query values, required/unexpected bodies, reinstall overwrite, empty-store guidance, and zero-arg help.

## Task Details

### 1. Enrich help with descriptions and body schema hints

- **Prerequisites / Dependencies:** Phase 3.
- **Affected Files:**
  - `src/infra/cli/clap_cli.rs`
  - `src/application/describe.rs`
  - `src/domain/model.rs`
- **Affected Symbols:** `DescribeService::describe`, `build_api_command`
- **Description:** A pure `describe` use case producing structured help data (groups, command summaries, per-parameter descriptions/required flags, body summary). The clap tree builder consumes it to set `about`/`long_about`, parameter help, and a help-visible body-schema line (content type, requiredness, schema summary). All three help levels render useful text. `ApiOperationGroup`/`Param` gained optional `description` fields (serde-defaulted) populated by the parser from OpenAPI tag and parameter descriptions.
- **Acceptance Criteria:**
  - [x] `--help` at API, group, and command level shows expected groups/commands/params (snapshot tests).
  - [x] Command help shows body content type, requiredness, and schema summary.

### 2. Harden error UX and edge cases

- **Prerequisites / Dependencies:** Phase 3.
- **Affected Files:**
  - `src/main.rs`
  - `src/infra/cli/clap_cli.rs`
  - `src/application/*`
- **Affected Symbols:** `run()`, error formatting
- **Description:** Consistent stderr format (`error: <message>`), non-zero exit on all failures, and clean handling of: empty `~/.clining/` (NotFound errors hint at `install`), name collisions at install (re-install overwrites per FR-1.3), absent stdin (required-body error before any network), missing args (delegated to clap, exit 2), and SIGPIPE-safe stdout writes (broken pipe treated as success).
- **Acceptance Criteria:**
  - [x] Every error path exits non-zero with a one-line stderr message.
  - [x] Invocation with zero args prints help and exits non-zero.

### 3. Write end-to-end integration tests and fixture specs

- **Prerequisites / Dependencies:** Phases 2–3.
- **Affected Files:**
  - `tests/common/mod.rs`
  - `tests/invoke.rs`
  - `tests/help.rs`
  - `tests/errors.rs`
  - `tests/fixtures/petstore.json`
- **Affected Symbols:** N/A
- **Description:** A shared harness spins a `std::net::TcpListener` mock server in-process and drives the compiled binary (`CARGO_BIN_EXE_clining`) through install → help → invoke → exit-code scenarios, isolating storage with `CLINING_DIR` temp dirs. The `petstore.json` fixture covers tags + untagged (`default`), operationId present/absent, path + query params, required bodies, and array/repeated query params.
- **Acceptance Criteria:**
  - [x] Happy path, non-2xx path, and unknown-command path covered by passing tests.
  - [x] Fixture specs assert naming/grouping edge cases end-to-end.

### 4. Write README and Makefile

- **Prerequisites / Dependencies:** None beyond prior tasks.
- **Affected Files:**
  - `README.md`
  - `Makefile`
- **Description:** README: build/install, both usage patterns, storage layout, exit-code/stream contract, link to the docs index. Makefile targets: `check` (fmt + clippy + test), `build`, `install`.
- **Acceptance Criteria:**
  - [x] `make check` passes from a clean checkout.
  - [x] README documents the stream/exit-code contract and storage location.

## Verification Plan

1. `cargo fmt --check`; `cargo clippy --all-targets -- -D warnings`; `cargo test` (unit + integration).
2. `make check` from a clean checkout.
3. Manual smoke: install the petstore fixture against a running mock server; run `--help` at each level; confirm stdout/stderr/exit codes; confirm `get-binary | head -c 2` exits 0 (SIGPIPE-safe).

## Phase Definition of Done

- [x] Help informative at every level with body schema hints.
- [x] All error paths non-zero with a one-line stderr.
- [x] E2E suite green; README + Makefile present.
- [x] Clippy-clean, formatted.
