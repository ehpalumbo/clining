---
type: workflow
title: "Phase 1 — Toolchain & Onion Scaffolding"
description: "Installs the Rust toolchain and lays down the clining crate with its onion module skeleton and a lint-clean, test-passing baseline."
tags:
  - "clining"
  - "implementation-plan"
  - "phase-1"
timestamp: "2026-08-17T19:47:50Z"
related:
  - "[Phased Implementation Plan](../implementation-plan.md)"
---

# Phase 1 — Toolchain & Onion Scaffolding

**Status:** Completed

## Overview

This phase establishes the project skeleton and toolchain so every later phase lands in a consistent, lint-clean crate with the onion module layout already in place. No feature code yet: `main.rs` prints a usage banner and the module tree is empty but compiles. Satisfies NFR-1..NFR-3 (Linux, Rust 2024, onion layering).

## Task Details

### 1. Install the Rust toolchain

- **Prerequisites / Dependencies:** None (Rust is currently absent from the machine).
- **Affected Files:** N/A (system toolchain).
- **Description:** Install via rustup (stable channel) on Linux.
- **Acceptance Criteria:**
  - [x] `cargo --version`, `rustc --version`, `rustup --version` all succeed.

### 2. Initialize the crate and add dependencies

- **Prerequisites / Dependencies:** Task 1.
- **Affected Files:**
  - `Cargo.toml`
- **Description:** `cargo init --name clining --bin`; set `edition = "2024"`. Add `clap` (derive), `serde` + `serde_json`, `anyhow`, `thiserror`, and `reqwest` (blocking feature), pinned to current versions.
- **Acceptance Criteria:**
  - [x] `cargo build` succeeds with zero warnings.
  - [x] `cargo clippy --all-targets -- -D warnings` is clean.

### 3. Create the onion module skeleton

- **Prerequisites / Dependencies:** Task 2.
- **Affected Files:**
  - `src/main.rs`
  - `src/domain/{mod.rs, model.rs, command_name.rs, ports.rs, errors.rs}`
  - `src/application/{mod.rs, learn_api.rs, invoke_command.rs, request_builder.rs, describe.rs}`
  - `src/infra/{mod.rs, cli/, storage/, openapi/, source/, http/}`
- **Affected Symbols:** placeholder `main()`
- **Description:** Create the empty module tree per the [Architecture Overview](../../architecture/overview.md). Placeholder `main.rs` prints a one-line usage banner. Add `#![deny(unsafe_code)]`.
- **Acceptance Criteria:**
  - [x] `cargo build` and `cargo test` pass; clippy clean with `-D warnings`.
  - [x] `cargo run` prints the usage banner and exits 0.

## Verification Plan

1. `cargo fmt --check`
2. `cargo clippy --all-targets -- -D warnings`
3. `cargo test`
4. `cargo run` → usage banner, exit 0

## Phase Definition of Done

- [x] Toolchain installed and pinned.
- [x] Crate compiles, tests pass, lint-clean.
- [x] Onion module layout present and documented.
