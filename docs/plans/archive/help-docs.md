---
type: workflow
title: "Issue #5 — Command help as agent-readable input/output documentation"
description: "Captures parameter schemas during learn and renders pretty parameter, request-body, and response walkthroughs from command --help, without a separate --docs mode or a templating engine."
tags:
  - "clining"
  - "implementation-plan"
  - "help"
  - "schemas"
  - "agents"
timestamp: "2026-10-20T00:00:00Z"
related:
  - "[Requirements Specification](../../specs/spec.md)"
  - "[Architecture Overview](../../architecture/overview.md)"
  - "[Phased Implementation Plan](../implementation-plan.md)"
---

# Issue #5 — Command help as agent-readable input/output documentation

**Status:** Implemented — ready for review

## Overview

clining is agent-oriented: many target APIs have no native CLI, so an agent cannot
guess request or response shapes from the endpoint alone. The command-level
`--help` is therefore the documentation surface. This change captures parameter
schemas during learn and renders, in plain text:

1. the summary and `METHOD path` request line;
2. each parameter's CLI flag, required/optional status, description, type, enum,
   examples, and constraints;
3. a field-by-field request-body walkthrough with resolved `$ref` component names,
   required fields, nested objects, arrays, and composites;
4. response status codes, content types, and the same schema walkthrough.

There is no separate `--docs` mode. `--help` is the single, read-only
documentation surface and never sends a request.

## Rendering decision: clap template vs. template engine

The issue originally suggested a templating engine such as Askama. After
evaluating the alternatives:

- **clap `help_template` is not sufficient for the dynamic document.** Its
  placeholders (`{about}`, `{usage}`, `{all-args}`, `{after-help}`, …) substitute
  fixed sections; they cannot loop over parameters, responses, or nested schema
  fields, or conditionally render required markers. It can arrange the static
  skeleton but cannot express a recursive schema walkthrough.
- **`Command::override_help` would work but is counterproductive.** It would move
  usage/option formatting into application code, duplicating clap's rendering and
  losing its standard behavior.
- **Askama would only template already-flattened lines.** Recursive schema
  rendering still needs cycle-guarded recursion in Rust; the template would add
  a dependency and compile-time cost without making the tree declarative.
- **Chosen:** keep clap's default help template and attach the dynamic content
  through `after_help`. The renderer lives in the presentation layer
  (`src/infra/cli/help.rs`), is pure and directly unit-testable, and keeps the
  domain/application layers free of presentation concerns.

## Task Details

### 1. Capture parameter schemas

- **Affected files:** `src/domain/model.rs`, `src/infra/openapi/spec.rs`,
  `src/infra/openapi/parser.rs`.
- **Description:**
  - Add `schema: Option<SchemaSpec>` to `Param`, with `#[serde(default)]` so
    models persisted before schema capture still load.
  - Deserialize `Parameter.schema` in the OpenAPI subset and convert it with the
    existing `parse_schema`, preserving local refs and cycle safety.
- **Acceptance criteria:**
  - [x] Parameter type, enum, examples, format, default, and constraints are
    persisted in the model when present.
  - [x] A `$ref` parameter schema is stored as `Ref`, not inlined.
  - [x] Older stored models without parameter schemas load unchanged.

### 2. Preserve contentless response status codes

- **Affected files:** `src/infra/openapi/parser.rs`.
- **Description:** Emit a `ResponseSpec` with an empty content type and no schema
  for responses such as `204` that declare no content, so help can list them.
- **Acceptance criteria:**
  - [x] Responses with content remain one spec per `(status, content type)`.
  - [x] Contentless responses appear in the response list with their status code.

### 3. Render pretty input/output documentation

- **Affected files:** `src/infra/cli/help.rs`, `src/infra/cli/clap_cli.rs`,
  `src/infra/cli/mod.rs`.
- **Description:**
  - `command_after_help(model, operation)` renders request line, request body,
    and responses.
  - `param_help_text(model, param)` appends `[required]`/`[optional]` plus
    `[type: …; enum: …; …; example: …]` to the clap argument help.
  - Schema walkthroughs render `name (type[, required]): description [metadata]`
    with stable indentation. Local refs render as `name (Component, type)`;
    recursive refs render as `name (Component, recursive)`; unresolved refs say
    `unresolved reference`. Raw `$ref` JSON and unlabeled JSON blobs are never
    emitted.
  - Arrays render `array<item>` and expand structured items as `items (…)`;
    composites label branches `option N` (`oneOf`/`anyOf`) or `part N` (`allOf`).
- **Acceptance criteria:**
  - [x] Object, array, enum, `$ref`, nested, composite, recursive, and unknown
    schemas render as readable text.
  - [x] Request and response walkthroughs share the same renderer and formatting.
  - [x] Unit tests cover the renderer independently of the CLI.
  - [x] `--help` exits without sending an HTTP request.

### 4. Update documentation and fixtures

- **Affected files:** `README.md`, `docs/specs/spec.md`,
  `docs/architecture/overview.md`, `tests/fixtures/petstore.json`,
  `tests/help.rs`.
- **Description:** Document the new help contract (FR-4), explicitly exclude a
  separate `--docs` mode, and enrich the fixture with parameter schemas, nested
  refs, array responses, error responses, and a contentless `204`.
- **Acceptance criteria:**
  - [x] End-to-end help tests assert parameter shapes, body walkthroughs,
    response schemas, resolved ref names, and contentless status codes.
  - [x] Existing help assertions that were strictly less detailed are updated.

## Verification Plan

1. `make check` (fmt, clippy `-D warnings`, all tests).
2. Manual: install the petstore fixture and inspect command help:
   - `clining pets pets get-pets --help` — parameter enum/example and array
     response walkthrough;
   - `clining pets store place-order --help` — resolved `Order` body and
     `Order`/`Error` responses;
   - `clining pets default get-ping --help` — contentless `204`.
3. Confirm no help invocation contacts the base URL and no `$ref` text appears.

## Definition of Done

- [x] Parameter schemas captured, persisted, and shown.
- [x] Request-body and response schemas rendered field by field.
- [x] Local refs shown by resolved component name and shape; cycles marked.
- [x] `--help` is the only documentation mode and performs no request.
- [x] Renderer unit tests plus updated CLI/E2E help tests pass.
- [x] Spec, README, and architecture docs updated.
- [x] `make check` green.
