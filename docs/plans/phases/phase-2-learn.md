---
type: workflow
title: "Phase 2 — \"Learn\" Vertical Slice"
description: "Implements the full learn flow: fetch a spec (file or URI), parse OpenAPI 3.0 into the domain model, and persist it under ~/.clining."
tags:
  - "clining"
  - "implementation-plan"
  - "phase-2"
timestamp: "2026-08-17T19:47:50Z"
related:
  - "[Phased Implementation Plan](../implementation-plan.md)"
---

# Phase 2 — "Learn" Vertical Slice

**Status:** Completed

## Overview

Implements the complete learn flow: fetch a spec (file or URI), parse OpenAPI 3.0.x into the domain model, and persist it. This is the foundation everything else consumes — the model shape and the command/parameter naming rules frozen here are load-bearing for Phases 3–4. Satisfies FR-1, FR-2, FR-5 (store), and the error-model baseline.

## Task Details

### 1. Define domain model entities

- **Prerequisites / Dependencies:** Phase 1.
- **Affected Files:**
  - `src/domain/model.rs`
- **Affected Symbols:** `ApiModel`, `ApiOperationGroup`, `ApiOperation`, `Param`, `ParamLocation`, `HttpMethod`, `BodySpec`, `ModelVersion`
- **Description:** Define the persisted `ApiModel` (`name`, `base_url`, `version`, `operation_groups`) with serde derives. `ApiOperation` carries `name`, `summary`, `method`, `path` template, `path_params`, `query_params`, and an optional `request_body` (`BodySpec { required, content_type, schema_json }`). `Param` stores both the original `name` (from the spec) and the kebab-cased `cli_name` used on the command line. `BodySpec.schema_json` stores the raw schema for help display only (no validation in v0).
- **Acceptance Criteria:**
  - [x] All entities serialize/deserialize losslessly via serde_json.

### 2. Implement command and parameter naming rules

- **Prerequisites / Dependencies:** Task 1.
- **Affected Files:**
  - `src/domain/command_name.rs`
- **Affected Symbols:** `command_name(operation_id, method, path)`, `cli_name(param_name)`, `disambiguate(names)`
- **Description:** Pure functions: kebab-case conversion of `operationId`; fallback `method-path-segments` joined by `-` and skipping `{path vars}`; kebab-casing of parameter names for the CLI; collision suffixing (`-2`, `-3`, …) within a group.
- **Acceptance Criteria:**
  - [x] `operationId "getPetById"` → `get-pet-by-id`.
  - [x] `GET /pets/{petId}` (no operationId) → `get-pets`.
  - [x] Param `petStatus` → CLI `pet-status`.
  - [x] Two identical fallback names in one group → second becomes `...-2`.

### 3. Define domain ports

- **Prerequisites / Dependencies:** Task 1.
- **Affected Files:**
  - `src/domain/ports.rs`
- **Affected Symbols:** `ApiStore`, `SpecLoader`, `OpenApiParser`
- **Description:** `ApiStore::load_by_name` / `save`; `SpecLoader::load(source) -> bytes`; `OpenApiParser::parse(bytes) -> ApiModel`. The domain error enum covers not-found, invalid-stored-model, invalid-spec, I/O, and network.
- **Acceptance Criteria:**
  - [x] Ports compile as traits with no references to infrastructure types.

### 4. Implement the OpenAPI 3.0 parser adapter

- **Prerequisites / Dependencies:** Tasks 1–2.
- **Affected Files:**
  - `src/infra/openapi/{spec.rs, parser.rs}`
- **Affected Symbols:** `OpenApi30Spec`, `PathItem`, `Operation`, `Parameter`, `RequestBody`, `Parser`
- **Description:** Serde structs for the OpenAPI 3.0.x subset (`openapi`, `info`, `servers`, `paths`, `tags`, operation `parameters`, `requestBody`, `responses`). `parse` maps spec → `ApiModel`: groups by tag (`default` for untagged), applies naming rules, merges path-item-level and operation-level parameters, and sets the base URL from `servers[0].url`. Rejects non-3.0 specs with a clear error.
- **Acceptance Criteria:**
  - [x] Fixture 3.0 spec maps to the expected `ApiModel` (groups, command names, params, base URL) in unit tests.
  - [x] 3.1 / 2.0 spec → clear "unsupported version" error.
  - [x] Malformed JSON → clear parse error.

### 5. Implement the JSON file store adapter

- **Prerequisites / Dependencies:** Task 1.
- **Affected Files:**
  - `src/infra/storage/json_file_store.rs`
- **Affected Symbols:** `JsonFileStore`
- **Description:** Store root = `$CLINING_DIR` or `~/.clining/`; one file per API (`<name>.json`). Atomic write (temp file + rename). A missing file returns `NotFound`; a present-but-unparseable file returns the distinct `InvalidStoredModel` error (with file path + parse reason) — never masked as not-found.
- **Acceptance Criteria:**
  - [x] Save-then-load roundtrips an `ApiModel`; JSON is canonical.
  - [x] Missing file → `NotFound`; corrupt file → `InvalidStoredModel` (never `NotFound`).
  - [x] Writes are atomic (no partial file left on failure).

### 6. Implement the source loader adapter

- **Prerequisites / Dependencies:** Phase 1.
- **Affected Files:**
  - `src/infra/source/loader.rs`
- **Affected Symbols:** `SourceLoader`
- **Description:** Loads bytes from a local file path or an `http(s)://` URI (via reqwest blocking). Descriptive errors for missing files and failed fetches.
- **Acceptance Criteria:**
  - [x] File path → file bytes; `https://` URI → response bytes.
  - [x] Nonexistent path → descriptive error.

### 7. Implement the LearnApiService use case

- **Prerequisites / Dependencies:** Tasks 3–6.
- **Affected Files:**
  - `src/application/learn_api.rs`
- **Affected Symbols:** `LearnApiService::learn(name, source, base_url_override)`
- **Description:** Orchestrates `SpecLoader` → `OpenApiParser` → `ApiStore.save`. Validates that the name is non-empty and filename-safe; `base_url_override` replaces the server URL. Re-install overwrites the existing model.
- **Acceptance Criteria:**
  - [x] Unit tests with fake ports: install persists the model; invalid spec propagates; base-URL override lands in the stored model.

### 8. Wire `clining install` in the CLI

- **Prerequisites / Dependencies:** Task 7.
- **Affected Files:**
  - `src/main.rs`
  - `src/infra/cli/clap_cli.rs`
- **Affected Symbols:** `InstallArgs`, `build_static_command()`, `run()`
- **Description:** Top-level clap command with the static `install <name> <spec-source> [--base-url]` subcommand. `main.rs` composes the real adapters into `LearnApiService`. Success prints `Installed <name> (N commands, M groups)`; errors to stderr with exit 1.
- **Acceptance Criteria:**
  - [x] `clining install pets /path/spec.json` creates `~/.clining/pets.json`.
  - [x] `clining install pets --base-url http://localhost:8080 ...` stores the overridden base URL.
  - [x] `clining install pets https://.../openapi.json` fetches over the network.
  - [x] Invalid spec → stderr error, exit 1, nothing persisted.

## Verification Plan

1. `cargo fmt --check`; `cargo clippy --all-targets -- -D warnings`; `cargo test`.
2. Manual: `clining install` against a fixture spec; inspect the stored JSON; confirm naming, grouping, params, base URL.
3. Manual: invalid spec and missing file → exit 1 with clear stderr.

## Phase Definition of Done

- [x] `clining install` persists a correct model from file or URI, honoring `--base-url`.
- [x] Naming/grouping rules verified by unit tests.
- [x] `NotFound` vs `InvalidStoredModel` distinguished.
- [x] Clippy-clean, formatted, all tests pass.
