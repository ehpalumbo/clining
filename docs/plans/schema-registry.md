---
type: workflow
title: "Issue #1 — Resolve OpenAPI $ref references; store request + response body schemas"
description: "Replaces raw-JSON schema storage with a typed SchemaSpec tree and a schema registry, captures response bodies per operation, and renders a fully-resolved JSON schema tree in the CLI command footer."
tags:
  - "clining"
  - "implementation-plan"
  - "schema-registry"
timestamp: "2026-08-20T22:36:32Z"
related:
  - "[Phased Implementation Plan](implementation-plan.md)"
  - "[Requirements Specification](../specs/spec.md)"
  - "[Architecture Overview](../architecture/overview.md)"
---

# Issue #1 — OpenAPI `$ref` resolution + request/response body schemas

**Status:** Implemented — ready for review (draft PR)

## Overview

`clining install` currently stores the request body schema verbatim: a `$ref` like `{"$ref": "#/components/schemas/Order"}` is persisted as just that string, response bodies are dropped entirely, and `--help` shows the raw `$ref`. This change replaces raw-JSON schema storage with a **typed `SchemaSpec` tree** plus a **schema registry** built from `components.schemas`, captures **response body schemas** per operation (status code + content type), and renders a fully-resolved JSON schema tree in the command footer.

Agreed design decisions:

- **Typed schema tree without child duplication.** `SchemaSpec` is a tagged enum with structural variants (`Object`, `Array`, `Composite`, `Ref`) enabling tree navigation and `$ref` expansion, plus leaf variants (`Primitive`, `Unknown`). Leaf nodes preserve their complete `raw_json` payload, while compound nodes store child schemas structurally and preserve only unmodeled metadata in `extra_json` (e.g. `required` lists, `minItems`, `description`) to eliminate payload duplication in the stored model.
- **Refs are referenced, not inlined.** Local `#/components/schemas/...` refs become `Ref { ref_id }` pointing at the registry; nested refs stay as `Ref` (cycle-safe at learn time by construction). Full expansion happens only at help-render time, cycle-guarded.
- **Footer shows the resolved tree.** `--help` prints a compact JSON-schema representation of the request body schema tree with refs fully expanded (cycle points render as `{"$ref": ...}` markers). No responses line in the footer.
- **Structural navigation merges node payloads.** `Object` merges rendered child property schemas into its `extra_json`; `Array` merges rendered item schemas into its `extra_json`; `Composite` merges rendered branches into its keyword array (`oneOf`/`allOf`/`anyOf`). Primitives and unknown nodes render their raw JSON directly.
- v0: no backward-compatibility or migration concern; the stored model shape may change freely. `ModelVersion` stays `V1`.

## Task Details

### 1. Add typed schema types to the domain model

- **Prerequisites / Dependencies:** none.
- **Affected Files:**
  - `src/domain/model.rs`
- **Affected Symbols:** `SchemaSpec`, `CompositeKind`, `ResponseSpec`, `BodySpec`, `ApiOperation`, `ApiModel`, `ApiModel::schema_by_ref_id`
- **Description:**
  - `CompositeKind`: `AllOf`, `OneOf`, `AnyOf`.
  - `SchemaSpec` (serde `tag = "kind"`, `rename_all = "snake_case"`):

    ```rust
    pub enum SchemaSpec {
        Ref { ref_id: String },
        Object { properties: BTreeMap<String, SchemaSpec>, extra_json: Option<String> },
        Array { items: Option<Box<SchemaSpec>>, extra_json: Option<String> },
        Primitive { raw_json: String },
        Composite { composite_kind: CompositeKind, schemas: Vec<SchemaSpec>, extra_json: Option<String> },
        Unknown { raw_json: String },
    }
    ```

  - `BodySpec`: replace `schema_json: Option<String>` with `schema: Option<SchemaSpec>` — inline schemas use the same typed type as registry values.
  - `ResponseSpec { status_code: String, content_type: String, schema: Option<SchemaSpec> }`.
  - `ApiOperation`: add `responses: Vec<ResponseSpec>`.
  - `ApiModel`: add `schema_registry: BTreeMap<String, SchemaSpec>`; add `impl ApiModel { pub fn schema_by_ref_id(&self, ref_id: &str) -> Option<&SchemaSpec> }`.
- **Acceptance Criteria:**
  - [x] All new entities serialize/deserialize losslessly via serde_json.
  - [x] `schema_by_ref_id` returns the matching registry entry or `None`.
  - [x] Registry and `responses` deserialize as empty when absent (`#[serde(default)]` where needed).

### 2. Extend the OpenAPI 3.0 spec subset

- **Prerequisites / Dependencies:** Task 1.
- **Affected Files:**
  - `src/infra/openapi/spec.rs`
- **Affected Symbols:** `OpenApi30Spec`, `Components`, `Operation`, `Response`
- **Description:**
  - `OpenApi30Spec.components: Option<Components>`.
  - `Components { schemas: BTreeMap<String, serde_json::Value> }` (raw JSON; converted to typed form in the parser).
  - `Operation.responses: Option<BTreeMap<String, Response>>` (Option fields deserialize as `None` when missing).
  - `Response { content: Option<BTreeMap<String, MediaType>> }` (reuses existing `MediaType`; `description` not needed).
- **Acceptance Criteria:**
  - [x] A spec with `components.schemas` and operation `responses` deserializes into the subset structs.
  - [x] Existing specs without `components` / `responses` still parse.

### 3. Convert schemas and build the registry in the parser

- **Prerequisites / Dependencies:** Tasks 1–2.
- **Affected Files:**
  - `src/infra/openapi/parser.rs`
- **Affected Symbols:** `Parser::build_model`, `Parser::build_operation`, `Parser::to_body_spec`, `Parser::to_response_specs`, `Parser::parse_schema`, `Parser::local_schema_ref_name`
- **Description:**
  - `local_schema_ref_name(ref_str) -> Option<String>`: trailing name for `#/components/schemas/<name>`, else `None`.
  - `parse_schema(value) -> SchemaSpec`:
    - `$ref`: local schema ref → `Ref { ref_id }`; any other ref → `Unknown { raw_json: value.to_string() }`.
    - `type: "object"` (or no `type` but `properties` present) → `Object { properties, extra_json }` (properties removed from extra metadata).
    - `type: "array"` → `Array { items, extra_json }` (items removed from extra metadata).
    - `type` string/integer/number/boolean → `Primitive { raw_json: value.to_string() }`.
    - `allOf` / `oneOf` / `anyOf` → `Composite { composite_kind, schemas, extra_json }` (composite keyword array removed from extra metadata).
    - everything else → `Unknown { raw_json: value.to_string() }`.
  - Registry: convert each `components.schemas` entry with `parse_schema`; **refs preserved as `Ref`, never inlined** (cycle-safe at learn time by construction).
  - `to_body_spec`: use `parse_schema` for `media.schema`.
  - `to_response_specs(responses) -> Vec<ResponseSpec>`: one per (status code, content type), schema via `parse_schema`.
  - Thread `schema_registry` and `responses` through `build_model` / `build_operation`.
- **Acceptance Criteria:**
  - [x] Request-body `$ref` → `BodySpec.schema == Ref { ref_id }`; registry entry typed.
  - [x] Nested refs preserved as `Ref` (not inlined).
  - [x] Object properties, array items, and composite branches navigate recursively without duplicating payloads.
  - [x] Node constraints (min, max, format, default, description) preserved in `raw_json` / `extra_json`.
  - [x] Cycle-safe: a self-referencing schema parses with `Ref` preserved (no recursion).
  - [x] Response schemas captured per (status, content-type) for both ref and inline schemas.

### 4. Render the resolved schema tree in the command footer

- **Prerequisites / Dependencies:** Tasks 1–3.
- **Affected Files:**
  - `src/infra/cli/clap_cli.rs`
- **Affected Symbols:** `api_command`, `command_footer`, `render_body_schema`, `render_schema`
- **Description:**
  - `command_footer(model: &ApiModel, command: &ApiOperation)`; `api_command` passes the model.
  - Footer body line: `Body: <content_type> (<required|optional>), schema: <json>\n`. No responses line.
  - `render_body_schema(model, &body.schema) -> String`: `None` → `"unknown"`; else compact-serialize `render_schema`.
  - `render_schema(spec, model, seen: &mut Vec<String>) -> serde_json::Value` (path-stack cycle guard):
    - `Ref` → if `ref_id ∈ seen` or registry lookup misses, emit `{"$ref":"#/components/schemas/<id>"}`; else push id, render target, pop.
    - `Object` → parsed `extra_json` with `"properties"` updated to recursively rendered properties map.
    - `Array` → parsed `extra_json` with `"items"` updated to recursively rendered item schema.
    - `Primitive` / `Unknown` → parsed `raw_json` as-is.
    - `Composite` → parsed `extra_json` with `"oneOf"`/`"allOf"`/`"anyOf"` updated with recursively rendered branch schemas.
  - Note: rendered JSON keys are alphabetically sorted (serde_json's default `BTreeMap`-backed `Map`).
- **Acceptance Criteria:**
  - [x] A `Ref` body renders the fully-expanded registry tree (no raw `$ref` at the top level).
  - [x] Inline `{"type":"object"}` still renders `schema: {"type":"object"}`.
  - [x] Cyclic schemas terminate with a `$ref` marker at the cycle point.
  - [x] Common constraints and hints (min, max, format, default, required list) are rendered in the schema JSON.
  - [x] `Unknown` renders its wrapped raw JSON.

### 5. Update model-construction touchpoints

- **Prerequisites / Dependencies:** Task 1.
- **Affected Files:**
  - `src/application/learn_api.rs`
  - `src/application/invoke_operation.rs`
  - `src/application/request_builder.rs`
  - `src/infra/storage/json_file_store.rs`
  - `src/infra/cli/clap_cli.rs`
- **Affected Symbols:** `FakeParser`, `sample_model`, `operation_with`, `BodySpec`
- **Description:**
  - Add `schema_registry: BTreeMap::new()` (and `responses: vec![]` where an `ApiOperation` is constructed) to test `ApiModel`/`ApiOperation` literals in `learn_api.rs`, `invoke_operation.rs`, `json_file_store.rs`, `clap_cli.rs`.
  - `request_builder.rs` `BodySpec` literals: `schema: None` (3 sites).
  - `clap_cli.rs` `sample_model` body uses `SchemaSpec::Object { properties: BTreeMap::new() }` so it still renders `schema: {"type":"object"}`.
- **Acceptance Criteria:**
  - [x] All crates compile with the new model fields.

### 6. Update fixture and tests

- **Prerequisites / Dependencies:** Tasks 1–5.
- **Affected Files:**
  - `tests/fixtures/petstore.json`
  - `tests/help.rs`
- **Affected Symbols:** `components`, `responses`, `command_help_shows_reference_schema_summary`
- **Description:**
  - Fixture: add `components.schemas.Order` (object, `id`/`petId`/`quantity` integer properties, `required: ["id"]`, a `description` on one property); add `responses`: `placeOrder` `200` → `application/json` `$ref Order`; `createPet` `200` → inline `{"type":"object"}`.
  - `tests/help.rs`: replace `command_help_shows_reference_schema_summary` with an assertion that `place-order --help` shows the expanded tree (including `"required"` / `"description"`) and does **not** contain `"$ref"`. Keep the inline-object assertion (`get-pets` renders `schema: {"type":"object"}`).
- **Acceptance Criteria:**
  - [x] E2E help shows the resolved JSON tree for the `$ref` request body.
  - [x] Inline body help output unchanged.

## Verification Plan

1. `cargo fmt --check`; `cargo clippy --all-targets -- -D warnings`; `cargo test`.
2. Manual: `clining install pets <fixture>`; inspect stored JSON for `schema_registry`, typed request schema, and `responses`.
3. Manual: `clining pets store place-order --help` shows the resolved schema tree, no raw `$ref`.

## Definition of Done

- [x] Parser unit tests plus the end-to-end fixture exercise `$ref` request and response bodies.
- [x] Stored `ApiModel` carries a schema registry separate from operations; operation request/response body specs reference it by ref ID.
- [x] Response body schemas stored per operation (status code + content type).
- [x] `--help` shows the resolved schema tree instead of the raw `$ref`.
- [x] Non-reference schemas stored typed/inline (not raw strings).
- [x] Clippy-clean, formatted, all tests pass.
