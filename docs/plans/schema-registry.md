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

**Status:** In Progress

## Overview

`clining install` currently stores the request body schema verbatim: a `$ref` like `{"$ref": "#/components/schemas/Order"}` is persisted as just that string, response bodies are dropped entirely, and `--help` shows the raw `$ref`. This change replaces raw-JSON schema storage with a **typed `SchemaSpec` tree** plus a **schema registry** built from `components.schemas`, captures **response body schemas** per operation (status code + content type), and renders a fully-resolved JSON schema tree in the command footer.

Agreed design decisions:

- **Typed schemas, not raw JSON.** `SchemaSpec` is a tagged enum covering the data types (object, array, number/integer, string, boolean), plus `Ref`, `Composite`, and `Unknown`. Registry values and operation body schemas use the same type. No raw JSON is kept in the registry.
- **Refs are referenced, not inlined.** Local `#/components/schemas/...` refs become `Ref { ref_id }` pointing at the registry; nested refs stay as `Ref` (cycle-safe at learn time by construction). Full expansion happens only at help-render time, cycle-guarded.
- **Footer shows the resolved tree.** `--help` prints a compact JSON-schema representation of the request body schema tree with refs fully expanded (cycle points render as `{"$ref": ...}` markers). No responses line in the footer.
- **`Composite` collapses allOf/oneOf/anyOf** into a single variant (the specific keyword is dropped; a `kind` field can be added later). Anything else (nullable, untyped, external refs) falls back to `Unknown { raw_json }`, which preserves the original JSON for display.
- v0: no backward-compatibility or migration concern; the stored model shape may change freely. `ModelVersion` stays `V1`.

## Task Details

### 1. Add typed schema types to the domain model

- **Prerequisites / Dependencies:** none.
- **Affected Files:**
  - `src/domain/model.rs`
- **Affected Symbols:** `SchemaSpec`, `SchemaProperty`, `ResponseSpec`, `BodySpec`, `ApiOperation`, `ApiModel`, `ApiModel::schema_by_ref_id`
- **Description:**
  - `SchemaSpec` (serde `tag = "kind"`, `rename_all = "snake_case"`):

    ```rust
    pub enum SchemaSpec {
        Ref { ref_id: String },
        Object { properties: BTreeMap<String, SchemaProperty> },
        Array { items: Option<Box<SchemaSpec>> },
        Integer,
        Number,
        String,
        Boolean,
        Composite { schemas: Vec<SchemaSpec> },
        Unknown { raw_json: String },
    }
    ```

  - `SchemaProperty { schema: SchemaSpec, required: bool, description: Option<String> }` (`required` derived from the object's `required` name list during parsing; `description` from the property's own `description`).
  - `BodySpec`: replace `schema_json: Option<String>` with `schema: Option<SchemaSpec>` — inline schemas use the same typed type as registry values.
  - `ResponseSpec { status_code: String, content_type: String, schema: Option<SchemaSpec> }`.
  - `ApiOperation`: add `responses: Vec<ResponseSpec>`.
  - `ApiModel`: add `schema_registry: BTreeMap<String, SchemaSpec>`; add `impl ApiModel { pub fn schema_by_ref_id(&self, ref_id: &str) -> Option<&SchemaSpec> }`.
- **Acceptance Criteria:**
  - [ ] All new entities serialize/deserialize losslessly via serde_json.
  - [ ] `schema_by_ref_id` returns the matching registry entry or `None`.
  - [ ] Registry and `responses` deserialize as empty when absent (`#[serde(default)]` where needed).

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
  - [ ] A spec with `components.schemas` and operation `responses` deserializes into the subset structs.
  - [ ] Existing specs without `components` / `responses` still parse.

### 3. Convert schemas and build the registry in the parser

- **Prerequisites / Dependencies:** Tasks 1–2.
- **Affected Files:**
  - `src/infra/openapi/parser.rs`
- **Affected Symbols:** `Parser::build_model`, `Parser::build_operation`, `Parser::to_body_spec`, `Parser::to_response_specs`, `Parser::parse_schema`, `Parser::local_schema_ref_name`
- **Description:**
  - `local_schema_ref_name(ref_str) -> Option<String>`: trailing name for `#/components/schemas/<name>`, else `None`.
  - `parse_schema(value) -> SchemaSpec`:
    - `$ref`: local schema ref → `Ref { ref_id }`; any other ref → `Unknown { raw_json: value.to_string() }`.
    - `type: "object"` (or no `type` but `properties` present) → `Object`; each property → `SchemaProperty { schema: parse_schema(prop), required: name ∈ "required" list, description: prop.description }`.
    - `type: "array"` → `Array { items: Option<Box<SchemaSpec>> }`.
    - `type` string/integer/number/boolean → unit variants.
    - `allOf` / `oneOf` / `anyOf` → `Composite { schemas }` (branches parsed recursively; keyword dropped).
    - everything else → `Unknown { raw_json: value.to_string() }`.
  - Registry: convert each `components.schemas` entry with `parse_schema`; **refs preserved as `Ref`, never inlined** (cycle-safe at learn time by construction).
  - `to_body_spec`: use `parse_schema` for `media.schema`.
  - `to_response_specs(responses) -> Vec<ResponseSpec>`: one per (status code, content type), schema via `parse_schema`.
  - Thread `schema_registry` and `responses` through `build_model` / `build_operation`.
- **Acceptance Criteria:**
  - [ ] Request-body `$ref` → `BodySpec.schema == Ref { ref_id }`; registry entry typed.
  - [ ] Nested refs preserved as `Ref` (not inlined).
  - [ ] Object properties map to `SchemaProperty` with required flag + description.
  - [ ] Array/scalar conversions; `Composite` from allOf/oneOf/anyOf; `Unknown { raw_json }` for untyped and non-local refs.
  - [ ] Cycle-safe: a self-referencing schema parses with `Ref` preserved (no recursion).
  - [ ] Response schemas captured per (status, content-type) for both ref and inline schemas.

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
    - `Object` → `{"type":"object","properties":{ name: render(prop.schema) (+ "description" when present) }}` plus `"required":[...]` reconstructed from `required == true` properties (only when non-empty).
    - `Array` → `{"type":"array"}` (+ `"items"` when present).
    - `Integer`/`Number`/`String`/`Boolean` → `{"type":"integer"}` etc.
    - `Composite` → `{"composite":[...]}`.
    - `Unknown { raw_json }` → the wrapped JSON embedded as-is (fallback `{}` if unparseable).
- **Acceptance Criteria:**
  - [ ] A `Ref` body renders the fully-expanded registry tree (no raw `$ref` at the top level).
  - [ ] Inline `{"type":"object"}` still renders `schema: {"type":"object"}`.
  - [ ] Cyclic schemas terminate with a `$ref` marker at the cycle point.
  - [ ] `Unknown` renders its wrapped raw JSON.

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
  - [ ] All crates compile with the new model fields.

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
  - [ ] E2E help shows the resolved JSON tree for the `$ref` request body.
  - [ ] Inline body help output unchanged.

## Verification Plan

1. `cargo fmt --check`; `cargo clippy --all-targets -- -D warnings`; `cargo test`.
2. Manual: `clining install pets <fixture>`; inspect stored JSON for `schema_registry`, typed request schema, and `responses`.
3. Manual: `clining pets store place-order --help` shows the resolved schema tree, no raw `$ref`.

## Definition of Done

- [ ] Parser unit tests plus the end-to-end fixture exercise `$ref` request and response bodies.
- [ ] Stored `ApiModel` carries a schema registry separate from operations; operation request/response body specs reference it by ref ID.
- [ ] Response body schemas stored per operation (status code + content type).
- [ ] `--help` shows the resolved schema tree instead of the raw `$ref`.
- [ ] Non-reference schemas stored typed/inline (not raw strings).
- [ ] Clippy-clean, formatted, all tests pass.
