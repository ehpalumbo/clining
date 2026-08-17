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

**Status:** Planned

## Overview

Makes the tool discoverable for agents: meaningful help at every level, schema hints for request bodies, clear error UX, and an end-to-end test suite plus README and Makefile. No new architectural surface. Satisfies FR-4 and the FR-5 hardening items.

## Task Details

### 1. Enrich help with descriptions and body schema hints

- **Prerequisites / Dependencies:** Phase 3.
- **Affected Files:**
  - `src/infra/cli/clap_cli.rs`
  - `src/application/describe.rs`
  - `src/domain/model.rs`
- **Affected Symbols:** `DescribeService::describe(model, group?, command?)`, `build_api_command`
- **Description:** Add a pure `describe` use case producing structured help data (groups, command summaries, per-parameter descriptions/required flags, body summary). The clap tree builder consumes it to set `about`/`long_about`, parameter help, and a help-visible body-schema line (content type, requiredness, schema summary). All three help levels must render useful text.
- **Acceptance Criteria:**
  - [ ] `--help` at API, group, and command level shows expected groups/commands/params (snapshot tests).
  - [ ] Command help shows body content type, requiredness, and schema summary.

### 2. Harden error UX and edge cases

- **Prerequisites / Dependencies:** Phase 3.
- **Affected Files:**
  - `src/main.rs`
  - `src/infra/cli/clap_cli.rs`
  - `src/application/*`
- **Affected Symbols:** `run()`, error formatting
- **Description:** Consistent stderr format (`error: <message>`), non-zero exit on all failures, and clean handling of: empty `~/.clining/`, name collisions at install, absent stdin, missing args (delegated to clap), and SIGPIPE-safe stdout writes.
- **Acceptance Criteria:**
  - [ ] Every error path exits non-zero with a one-line stderr message.
  - [ ] Invocation with zero args prints help and exits non-zero.

### 3. Write end-to-end integration tests and fixture specs

- **Prerequisites / Dependencies:** Phases 2–3.
- **Affected Files:**
  - `tests/integration/*.rs`
  - `tests/fixtures/*.json`
- **Affected Symbols:** N/A
- **Description:** Spin a `std::net::TcpListener` mock server in-process; drive the compiled binary (`CARGO_BIN_EXE_clining`) through install → help → invoke → exit-code scenarios, isolating storage with `CLINING_DIR` temp dirs. Fixture specs cover: tags + untagged (`default`), operationId present/absent, path + query params, required body, array params.
- **Acceptance Criteria:**
  - [ ] Happy path, non-2xx path, and unknown-command path covered by passing tests.
  - [ ] Fixture specs assert naming/grouping edge cases end-to-end.

### 4. Write README and Makefile

- **Prerequisites / Dependencies:** None beyond prior tasks.
- **Affected Files:**
  - `README.md`
  - `Makefile`
- **Description:** README: build/install, both usage patterns, storage layout, exit-code/stream contract, link to the docs index. Makefile targets: `check` (fmt + clippy + test), `build`, `install`.
- **Acceptance Criteria:**
  - [ ] `make check` passes from a clean checkout.
  - [ ] README documents the stream/exit-code contract and storage location.

## Verification Plan

1. `cargo fmt --check`; `cargo clippy --all-targets -- -D warnings`; `cargo test` (unit + integration).
2. `make check` from a clean checkout.
3. Manual smoke: install a fixture against a running mock server; run `--help` at each level; confirm stdout/stderr/exit codes.

## Phase Definition of Done

- [ ] Help informative at every level with body schema hints.
- [ ] All error paths non-zero with a one-line stderr.
- [ ] E2E suite green; README + Makefile present.
- [ ] Clippy-clean, formatted.
