# Implementation Plan: Issue #17 — Native support for multipart/form-data request bodies (file uploads)

- **Author:** OpenCode agent (GLM-5.3-Flash Free)
- **Date:** 2026-11-02
- **Structure:** Single increment
- **Target branch / environment:** `multipart-bodies` (off `main`); Linux, Rust `edition 2024`, clap 4, blocking reqwest — **no new crates**
- **Status:** Ready for implementation

---

## References

- [Requirements Specification — clining v0](../specs/spec.md)
- [Architecture Overview](../architecture/overview.md)
- [AGENTS.md](../../AGENTS.md) — build gate (`make check`), onion layering, e2e contract
- [GitHub Issue #17](https://github.com/ehpalumbo/clining/issues/17) — enhancement, milestone v0.1.0
- Direction aligned with the maintainer in a plan session; per maintainer instruction there is **no accompanying impact analysis** for this issue. This revision incorporates maintainer feedback on stdin/`Content-Type` override behavior and spec-derived per-part content types.

---

## Executive Summary

Commands whose request body declares `multipart/form-data` gain native form-part input:
a single generic, repeatable flag `-F, --clining-form <field>=<value>` (text part) and
`-F, --clining-form <field>=@<path>` (file part), optionally with a curl-compatible
`;type=<content-type>` postfix on file inputs. clining owns the wire format end to end:
it generates a per-invocation boundary, frames the parts (CRLF framing,
`Content-Disposition`, per-part `Content-Type`), and transmits file bytes exactly as
read. File streaming is out of scope (tracked as issue #4); whole files are read into
memory at invocation.

The invocation chooses **one** body source, in this precedence:

1. **`-F` form parts and no custom `Content-Type`** → native multipart framing.
2. **A custom `Content-Type` header (any value, via `-H`)** → the caller owns the
   body: a piped stdin body is allowed and sent raw with the manual header (this
   preserves the manual-boundary workaround and any content-type override); supplying
   `-F` parts alongside it is rejected as ambiguous.
3. **Only piped stdin, no override** → rejected on multipart commands with a
   one-line error pointing at `-F`.
4. **Nothing supplied** → the existing required-body check (hinting `-F`); optional
   bodies send nothing.

Per-part content types are resolved from the spec when it documents them, matched
**by part name**: an explicit OpenAPI `encoding.<part>.contentType` wins, otherwise the
type is inferred from the part's schema `type` field (`string` + `format: binary` →
`application/octet-stream`, `object` → `application/json`, other types → no explicit
header, which servers read as `text/plain`). A caller `;type=` always wins. This
requires capturing part metadata at install time: `BodySpec` gains an optional
`parts` list (serde-default) and `MediaType` gains the previously-dropped `encoding`
map. The stored-format change is additive: old model files load unchanged, parts
metadata is simply absent, and the default content-type rules apply — re-install is
optional and only needed to gain spec-derived per-part content types.

Discovery stays in `--help`: the schema walkthrough gains a `-F` usage line and a
`(file part)` annotation on parts classified as file uploads.

### Architecture

- **Domain, new `src/domain/multipart.rs`:** all multipart logic is pure: `FormInput`
  parsing (mirrors `CustomHeader::parse`), the body-source decision helpers, part
  materialization, the byte-exact framing encoder (given an explicit boundary), a
  bounded boundary generator seeded from time+pid, a `multipart/form-data`
  content-type classifier, and the per-part content-type resolution chain. Zero
  external crates; hetero helpers are pure so the resolver/encoder are unit-testable.
- **Domain model (`src/domain/model.rs`):** `BodySpec` gains
  `parts: Option<Vec<PartSpec>>` (serde default — old models deserialize unchanged);
  `ApiInvocationRequest` (runtime struct, not serialized) gains
  `form_parts: Vec<FormPart>`.
- **Application (`src/application/request_builder.rs`):** `build_request` branches on
  the body content type and the presence of a custom `Content-Type` header: multipart
  operations take `form_parts`, reject bare stdin bodies, delegate framing to the
  domain encoder, and resolve per-part content types; every non-multipart path and the
  custom-header override step are byte-for-byte unchanged. `ApiInvocationService`
  passes the field through.
- **Infrastructure — parser (`src/infra/openapi/`):** `spec.rs` extends `MediaType`
  with `encoding`; `parser.rs` derives `BoundaryPartSpec`-style part metadata for
  multipart bodies only.
- **Infrastructure/CLI (`src/infra/cli/clap_cli.rs`):** registers the `-F` argument
  only for multipart-body commands, parses and resolves inputs (file bytes read in
  infra, like stdin today), and keeps the help-only path from ever touching stdin.
- **Help (`src/infra/cli/help.rs`):** `-F` usage guidance and file-part annotations.
- **Docs/tests:** new e2e suite with a dedicated fixture; spec + README updates.

### Design Decisions (aligned with the maintainer; adopted without re-negotiation)

1. **Generic curl-style flag under the `clining-` prefix** — `-F/--clining-form
   name=value | name=@path`, mirroring `-H/--clining-header` from issue #8. The
   underscore arg ID `clining_form` cannot collide with kebab-cased parameter IDs. No
   per-part flags are generated.
2. **Stdin policy conditioned on manual `Content-Type`** — a custom `-H Content-Type`
   (any value) opts the invocation out of native multipart framing entirely:
   stdin-as-raw-body is allowed and sent with the manual header; mixing `-F` inputs
   with it is a one-line error (framing would be ambiguous). With no override, piped
   stdin on a multipart command is rejected with a pointer to `-F`. This keeps the
   pre-native manual-boundary workaround functional in the new version as well.
3. **Per-part content-type resolution chain** — caller `;type=` (verbatim) >
   `encoding.<field>.contentType` captured at install > inference from the part's
   schema `type` (`string`+`format: binary` → `application/octet-stream`; `object` →
   `application/json`; others → no per-part `Content-Type` line) > input-syntax
   default (file inputs → `application/octet-stream`, text inputs → none). Field
   matching is exact, case-sensitive, on the original spec property name. "Other"
   text types deliberately get **no** header line (servers read untyped parts as
   `text/plain` per RFC 7578 §4.5, so the wire result is equivalent); an explicitly
   declared `text/plain` (via `encoding` or `;type=`) is still sent verbatim.
4. **Framing kind follows the input syntax, not the spec kind** — `@path` always
   frames as a file part (with `filename`), a plain value always frames as text. The
   spec-derived kind refines only the per-part `Content-Type` and the help annotation;
   it is never enforced against the input (presence-only philosophy, NFR-4).
5. **Filename = basename of the supplied path**; explicit filename override syntax is
   not built in v0.
6. **Part validation is presence-only** — `-F` field names are not constrained to the
   schema; spec-documented `required` parts are documented in help but not enforced.
   The existing required-body check still applies when nothing at all is supplied.
7. **Loose-spec acceptance** — a multipart body without a parsable object schema still
   enables `-F` (the content type alone is sufficient); parts metadata is absent and
   only the input-syntax content-type defaults apply; help annotations are best-effort.
8. **`format: binary` only** is treated as a file-upload marker (`format: base64`
   stays a text part; noted as a documented limitation).
9. **Additive model change, no migration** — `BodySpec.parts` deserializes as `None`
   when absent; previously installed models behave identically except that per-part
   content types fall back to the input-syntax defaults until re-installed.
10. **Hand-rolled encoding, no new dependencies** — reqwest's `multipart` feature
    stays off; framing lives in domain/application, keeping the builder pure and
    byte-testable per ADR-001.
11. **Multipart classifier** — exact subtype match, ASCII case-insensitive,
    parameters after `;` ignored (`multipart/form-data; charset=utf-8` matches;
    `multipart/mixed` does not). Only the first captured request media type
    participates (the existing `to_body_spec` choice); specs listing
    `application/json` before `multipart/form-data` keep today's behavior.

### Error contract (all one-line `error:`, exit 1, request not sent)

| Trigger | Message shape |
| --- | --- |
| Required multipart body, no `-F`, no stdin | `body error: command 'x' requires a request body: multipart/form-data; supply parts with -F <field>=<value> or <field>=@<path>` |
| Piped stdin onto a multipart command, no custom `Content-Type` | `body error: command 'x' takes multipart form fields; pass -F <field>=@<path> parts, or override the body Content-Type with -H to send raw` |
| `-F` parts together with a custom `Content-Type` header | `body error: command 'x' cannot combine -F form parts with a custom Content-Type header (framing would be ambiguous)` |
| `-F` on a command that declares no body | existing `declares no request body` error (unchanged, fires first) |
| `-F` on a JSON/binary-bodied command | `body error: command 'x' takes its body on stdin; -F form parts support multipart/form-data commands only` |
| Malformed `-F` value / invalid field name or filename chars | `invalid form part: ...` (new `DomainError::Form`) |
| File path unreadable/empty | `I/O error: failed to read form file '<path>' for part '<field>': <reason>` |

---

## Configuration & Environment Updates

- **Environment Variables:** None.
- **Feature Flags:** None.
- **External Dependencies:** None (no crates added; reqwest `multipart` feature deliberately not enabled).

---

## Task Details

### 1. Domain: pure multipart module (`FormInput`, encoder, resolution, boundary, classifier)

- **Prerequisites / Dependencies:** None.
- **Affected Files:**
  - [multipart.rs](../../src/domain/multipart.rs) - NEW
  - [mod.rs](../../src/domain/mod.rs) - MODIFY (add `pub mod multipart;`)
  - [errors.rs](../../src/domain/errors.rs) - MODIFY (new variant)
- **Affected Symbols:**
  - `FormInput`, `FormPart`, `FormPartValue` (new types)
  - `FormInput::parse()` — `"field=value"` → text part; `"field=@path"` → file part;
    `"field=@path;type=<ct>"` → file part with caller content type. Errors:
    `DomainError::Form { message: "invalid form part: ..." }` for a missing `=`, empty
    field name, missing path after `@` (including `@` alone), `;type=` on a text
    value, or field names/filenames containing `"`, `\`, or CRLF characters (quoting /
    framing safety). Everything after the first `;type=` is the caller content type,
    validated non-empty and free of quotes/CRLF. Values are otherwise unconstrained
    (empty values frame empty parts).
  - `encode(boundary: &str, parts: &[FormPart]) -> Vec<u8>` — exact framing:
    `--<b>\r\n` `Content-Disposition: form-data; name="<field>"` plus
    `; filename="<filename>"` for file inputs, `[Content-Type: <ct>]` header line
    (resolved per the decision-3 chain), blank line, part bytes, trailing `\r\n`;
    final `--<b>--\r\n`. Part order = input order; repeated field names produce
    repeated parts. Values are bytes: text via UTF-8, files verbatim (binary-safe,
    no re-encoding).
  - `resolve_part_content_type(input_kind, ByNamePartSpec lookup) -> Option<String>` —
    the decision-3 chain as a pure function: caller `;type=` → captured
    `encoding.contentType` → `type`-field inference (`File` →
    `application/octet-stream`; `Object` → `application/json`; `Text` → `None`) →
    input-syntax default (`@file` without spec metadata → `application/octet-stream`;
    text → `None`).
  - `boundary() -> String` — `clining-` + 32 ASCII hex chars from a splitmix64 mix of
    `SystemTime` nanos + pid (std only); regenerated per request.
  - `is_multipart_form_data(content_type: &str) -> bool` — case-insensitive exact
    subtype match, parameters after the first `;` stripped.
- **Out of scope, documented in code comments:** boundary-vs-content collision
  renegotiation; streaming; `encoding.headers` (e.g. `Content-ID` per part); escape
  implementation for field/filename (rejected instead).
- **Acceptance Criteria:**
  - [ ] Framing unit tests with a fixed boundary: declared order preserved, repeated field names → two separate parts, binary-safe (part data containing the boundary string, `0x00`/`0xFF`, and CRLF survives verbatim), exact CRLF framing, final terminator present
  - [ ] Content-type chain unit tests: caller `;type=` wins over encoding; encoding wins over inference; inference `binary` → `octet-stream`, `object` → `application/json`, plain string → no header; unknown field with `@input` → `octet-stream`, unknown text field → no header
  - [ ] Text part → no `Content-Type` line unless explicitly declared (encoding or `;type=`)
  - [ ] `FormInput::parse` matrix: `k=v`, `k=` (empty value frames an empty part), `k=@f`, `k=@f;type=x/y`, missing `=`, empty name, `k=@`, `;type=` on text rejected, invalid characters rejected — every failure is `DomainError::Form` with a single-line message
  - [ ] `boundary()`: starts `clining-`, hex-only, length ≥ 24; two calls (tiny sleep apart) differ
  - [ ] `is_multipart_form_data("multipart/form-data")`, `("Multipart/Form-Data; charset=utf-8")` → true; `("multipart/mixed")`, `("application/json")` → false
  - [ ] `cargo test --lib domain::multipart` green; domain keeps zero external crates

### 2. Model + spec capture (part metadata at install time)

- **Prerequisites / Dependencies:** Task 1 (kinds/classifier).
- **Affected Files:**
  - [model.rs](../../src/domain/model.rs) - MODIFY
  - [spec.rs](../../src/infra/openapi/spec.rs) - MODIFY
  - [parser.rs](../../src/infra/openapi/parser.rs) - MODIFY
- **Affected Symbols:**
  - `BodySpec` ([src/domain/model.rs:86](../../src/domain/model.rs)) — gains
    `parts: Option<Vec<PartSpec>>` with serde defaulting so pre-existing model files
    deserialize with `parts = None` (decision 9; skip serialization when `None`).
  - New `PartSpec { name, required, kind: PartKind, repeated, content_type }` and
    `PartKind::{Text, File, Object}` — captured, display/resolution metadata only
    (framing never consumes the kind: decision 4). `required` comes from the body
    object schema's `required` array; `repeated` from array-typed properties (kind
    taken from the items schema); `content_type` is the raw
    `encoding.<name>.contentType` when declared (`None` = infer later).
  - `MediaType` ([src/infra/openapi/spec.rs:89](../../src/infra/openapi/spec.rs)) —
    gains `encoding: Option<BTreeMap<String, MediaTypeEncoding>>` and
    `MediaTypeEncoding { content_type: Option<String> }` (capturing `contentType`).
  - `to_body_spec` ([src/infra/openapi/parser.rs:239](../../src/infra/openapi/parser.rs)) —
    after selecting the first media type: when it classifies as
    `multipart/form-data`, resolve the schema (inline object or local `$ref` into the
    components map built earlier in `build_model`) into part specs; a schema that is
    not a resolvable object (or lacks properties) yields `parts: None` (decision 7).
- **Acceptance Criteria:**
  - [ ] Unit: a multipart body with an object schema (properties `name` string, `photo` string/`format: binary`, array-of-binary property, object property, and `encoding.contentType` on some fields) produces the correct `PartSpec` list incl. `required` flags, `repeated` flags, kinds, and raw `content_type`s
  - [ ] Unit: schema as local `$ref` resolves; unresolvable `Unknown` schema → `parts: None`
  - [ ] Unit: JSON-body operations produce `parts: None`; `encoding` on a JSON body is still ignored (out of scope)
  - [ ] Old-model compatibility: a model JSON without `parts` deserializes (`parts = None`) — covered by a serde round-trip test
  - [ ] `cargo test --lib domain::model infra::openapi` green

### 3. Invocation plumbing + request-builder branch

- **Prerequisites / Dependencies:** Tasks 1 and 2.
- **Affected Files:**
  - [model.rs](../../src/domain/model.rs) - MODIFY (`ApiInvocationRequest` gains `form_parts: Vec<FormPart>`; `new()` gains the parameter; doc comment notes infra materializes file bytes)
  - [request_builder.rs](../../src/application/request_builder.rs) - MODIFY
- **Affected Symbols:**
  - `build_request()` ([src/application/request_builder.rs:21](../../src/application/request_builder.rs)) — gains `form_parts: &[FormPart]`
  - body-selection block ([src/application/request_builder.rs:43-65](../../src/application/request_builder.rs)) — gains the multipart branch
- **Description:** The branch implements the Executive-Summary precedence list, using
  the existing `custom_headers` input (already available in `build_request`):
  - detect a custom `Content-Type` via ASCII case-insensitive name match on
    `custom_headers` (fired before `merge_headers`, which already expects to drop the
    generated header):
    - custom `Content-Type` present → caller-controlled body: stdin `body` sent as-is
      (with the stdin-empty filter preserved for the stdin path); `form_parts`
      non-empty → conflict error (contract table row 3);
    - no custom `Content-Type`:
      - `form_parts` non-empty → `multipart::boundary()`, encode, set
        `Content-Type: multipart/form-data; boundary=<b>` (generated after header
        construction so `merge_headers` can still be overridden by a later custom
        header — tested);
      - stdin body present → rejection error (row 2);
      - nothing → required check with the row-1 message hinting `-F`; optional → send
        nothing (unchanged empty-body semantics).
  - non-multipart operations with `form_parts` → rows 4/5 (no-body check keeps firing
    first).
  `merge_headers` logic itself is untouched. Update `ApiInvocationRequest::new` call
  sites (clap CLI + unit tests).
- **Acceptance Criteria:**
  - [ ] Multipart success: generated `Content-Type` carries the boundary; body parses back to the input parts (test recovers the boundary from the header and re-frames); part content types follow the decision-3 chain
  - [ ] Precedence matrix unit-tested: `-F`+stdin+no override → row 2; stdin+custom CT → raw passthrough (request carries manual header and untouched body); `-F`+custom CT → row 3; `-F`+no CT on non-multipart → rows 4/5
  - [ ] Every row of the error contract table is unit-tested; each mentions the command name; no request is built for error paths
  - [ ] Non-multipart behavior byte-for-byte unchanged: existing unit tests pass with only signature-plumbing updates
  - [ ] Custom `-H Content-Type: …` still overrides the generated boundary `Content-Type` (the override precedes generation and both are exercised)

### 4. Infrastructure/CLI: `-F` flag, input resolution

- **Prerequisites / Dependencies:** Task 1 (domain); Task 3 (plumbing).
- **Affected Files:**
  - [clap_cli.rs](../../src/infra/cli/clap_cli.rs) - MODIFY
- **Affected Symbols:**
  - Constants `CUSTOM_FORM_ARG_ID = "clining_form"`, `CUSTOM_FORM_LONG = "clining-form"`, `CUSTOM_FORM_HELP` (next to the header flag constants at [src/infra/cli/clap_cli.rs:21-24](../../src/infra/cli/clap_cli.rs); help text explains both value forms and `;type=`). The arg ID avoids kebab collisions the same way the header ID does.
  - `api_command()` ([src/infra/cli/clap_cli.rs:152](../../src/infra/cli/clap_cli.rs)) — register `Arg::new(CUSTOM_FORM_ARG_ID).short('F').value_name("FORM")` with `ArgAction::Append`, `num_args(1)` only when the operation's body content type classifies as multipart (loose-spec rule: gate on content type, not on the schema). Skip `.long(CUSTOM_FORM_LONG)` when an API parameter already canonicalizes to `clining-form` — extend the carve-out helper `operation_claims_clining_header()` ([src/infra/cli/clap_cli.rs:212](../../src/infra/cli/clap_cli.rs)) to also consider that long name (mirrors FR-3.8: the param keeps the long form, the form flag stays short-only).
  - `run_invoke()` ([src/infra/cli/clap_cli.rs:357](../../src/infra/cli/clap_cli.rs)) — after header parsing (fail-fast before reads), collect `-F` strings in occurrence order: `FormInput::parse` each (printing `error:` and returning `ExitCode::FAILURE` on first failure), then `fs::read` file parts (basename via `Path::file_name`, falling back to the raw path when it has no basename) into materialized `FormPartValue::File`; I/O failures print the contract-table message with exit 1. Materialize strictly before reading stdin so nothing is consumed on failure.
  - `read_stdin_body()` ([src/infra/cli/clap_cli.rs:567](../../src/infra/cli/clap_cli.rs)) — unchanged; all stdin policy lives in `build_request` (single policy point). Help rendering keeps never reading stdin (FR-4.5).
- **Acceptance Criteria:**
  - [ ] Unit: a model with a multipart-body op parses `-F name=x -F file=@/tmp/p;type=image/png` into ordered `FormInput`s; a JSON-body op exposes no `-F` flag
  - [ ] Unit: parse/file failures exit 1 with single-line `error:` and precede any stdin read; mixed repeatable order (text ↔ file, interleaved with `-H`) is preserved
  - [ ] Unit: carve-out — a param canonicalizing to `clining-form` leaves the form flag short-only (`-F`), mirroring the `api_parameter_named_clining_header` test ([src/infra/cli/clap_cli.rs:1329](../../src/infra/cli/clap_cli.rs))
  - [ ] Unit: help-only paths never touch stdin (no regression; the arg is registered before any read)

### 5. Command help: `-F` usage line and `(file part)` annotations

- **Prerequisites / Dependencies:** Tasks 1, 2 (part metadata); Task 4 (flag text).
- **Affected Files:**
  - [help.rs](../../src/infra/cli/help.rs) - MODIFY
- **Affected Symbols:**
  - `command_after_help()` ([src/infra/cli/help.rs:19](../../src/infra/cli/help.rs)) and `render_request_body()` ([src/infra/cli/help.rs:110](../../src/infra/cli/help.rs))
- **Description:** When the body content type classifies as multipart, after the
  content-type/required header line add one guidance line, e.g.
  `  Parts: -F '<field>=<value>' for text, -F '<field>=@<path>' for files (optional ';type=<content-type>')`
  plus, per the captured `PartSpec`s, one part line each stating name, kind
  (text / file upload / JSON field), required/optional status, repeatable-ness, and
  the resolved per-part content type (`;type=`-style). This walkthrough
  (`render_schema_entry` machinery) is otherwise unchanged for non-multipart. When
  part metadata is absent (loose spec or models installed before this change),
  annotate property entries whose schema is a string primitive with
  `format: binary` with a `(file part)` suffix via best-effort raw-JSON detection
  (decision 7).
- **Acceptance Criteria:**
  - [ ] Multipart guidance line renders whether or not the spec documents an object schema
  - [ ] With part metadata: per-part lines reflect name/kind/required/repeat/resolved content type incl. `;`-default `octet-stream` for files
  - [ ] Without part metadata: `format: binary` properties are annotated `(file part)`; plain string properties are not
  - [ ] Non-multipart help output is unchanged (existing help unit tests pass untouched)

### 6. E2E suite and fixture

- **Prerequisites / Dependencies:** Tasks 1–5.
- **Affected Files:**
  - [multipart-upload.json](../../tests/fixtures/multipart-upload.json) - NEW (spec: `POST /uploads` with `multipart/form-data` body documenting `name` (string), `photo` (string, `format: binary`) with `encoding.contentType: image/jpeg` for photo, and a repeated field; 201 response; kept separate from `tests/fixtures/petstore.json` so shared suites stay stable)
  - [common/mod.rs](../../tests/common/mod.rs) - MODIFY (MockServer route table at [tests/common/mod.rs:158](../../tests/common/mod.rs): answer `POST /uploads` with `201`)
  - [multipart.rs](../../tests/multipart.rs) - NEW e2e suite
- **Description:** e2e tests asserting the stream/exit-code contract per
  `docs/specs/spec.md`:
  - happy path: install the fixture, invoke `<api> <group> <command> -F name=fluffy -F photo=<path>` against the mock server; 201 → exit 0; status line + headers on stderr; body on stdout.
  - **Wire assertions** (MockServer records full request bytes): recover the boundary
    from the `Content-Type` header; split frames on the boundary; assert field order,
    the exact text part bytes, byte-exact file content (reuse `BINARY_BODY` from
    [tests/common/mod.rs:13](../../tests/common/mod.rs) to prove binary safety),
    `filename=<basename>`, per-part `Content-Type` (capture backward-compatible
    defaults and the spec-derived `image/jpeg`; `;type=` override honored), and the
    final terminator.
  - repeated fields: `-F name=a -F name=b` produces two parts in order.
  - override matrix (wire-level): stdin + custom `-H Content-Type: application/json` → raw stdin sent, manual header honored, exit 0; `-F` + custom `Content-Type` → one-line error, no connection; bare stdin → one-line error mentioning `-F`, no connection.
  - error paths (each: one `error:` line, exit 1, **no connection opened**): required
    body with nothing supplied; unreadable file; malformed `-F`.
  - discovery: `--help` shows the `-F` flag, the guidance line, and per-part metadata
    with the `(file part)` annotation.
  - non-multipart regression: existing suites (`invoke.rs`, `headers.rs`, `errors.rs`)
    pass unchanged — explicit signal that JSON/raw-binary/empty-body behavior is intact.
- **Acceptance Criteria:**
  - [ ] All e2e cases above pass; `tests/fixtures/petstore.json` is unmodified (`git diff` empty)
  - [ ] Every new e2e error test asserts both the exit code and the stream split (stdout empty / stderr one line)
  - [ ] `make check` (fmt + clippy `-D warnings` + full test suite) passes

### 7. Living docs

- **Prerequisites / Dependencies:** Tasks 1–6 (document the landed behavior).
- **Affected Files:**
  - [spec.md](../specs/spec.md) - MODIFY
    - Usage contract table (~line 27): invoke row gains `-F <field>=<value> | <field>=@<path>` alongside `[< body]`.
    - New **FR-3.10** (numbered after FR-3.9 at line 66): multipart bodies —
      enablement by content type; `-F` syntax incl. `;type=`; boundary + matching
      `Content-Type` generated per invocation; byte-exact file transmission; filename
      from path basename; per-part content-type resolution chain (caller `;type=` >
      captured `encoding.contentType` > `type`-field inference —
      `application/octet-stream` for files, `application/json` for objects, no header
      for text — > input-syntax defaults); part order/repeated names; the
      stdin/override precedence matrix (stdin rejected without a custom
      `Content-Type`; allowed as raw passthrough with one; `-F`+custom rejected);
      `-F` only accepted on multipart commands; field-name and filename validation
      (quotes/backslash/CRLF rejected); presence-only validation of spec-documented
      required parts (documented, not enforced); the `clining-form` carve-out
      mirroring FR-3.8's wording; loose-spec enablement without a parsable schema.
    - FR-2.6 (~line 53): extend "request body spec" to mention captured multipart part metadata (name, kind, required/repeated flags, declared `encoding` content type).
    - Error model table (~line 91): new row `Form part errors | Malformed -F value, invalid field/file name characters | stderr error: invalid form part: ...; exit 1; request not sent` (stdin/file-I/O rows already covered by body/I-O rows — keep the table honest); usage-errors row gains the `-F`+custom-`Content-Type` conflict (exit 1 via `error:` rather than clap's exit 2, since it is a semantic conflict).
    - Out of Scope (~line 115): add "Streaming large uploads/requests without buffering (issue #4)". Note in FR-3.10 that installed models captured before part metadata gain native `-F` handling, with per-part content types defaulting until re-install.
    - FR-3.8 (~line 65): extend the carve-out sentence, or add to FR-3.10, for the `clining-form` long-name reservation.
  - [README.md](../../README.md) - MODIFY (request-body section: file-upload walkthrough with a `-F` example, the `;type=` postfix, the stdin-rejection note, and the custom-`Content-Type` override escape hatch)
  - [index.md](../index.md), [overview.md](../architecture/overview.md) - VERIFY ONLY (expected: no change — no command-shape or layering change)
- **Acceptance Criteria:**
  - [ ] `docs/specs/spec.md` describes the full multipart contract incl. the precedence matrix, resolution chain, errors, and scope boundary
  - [ ] README demonstrates an upload end to end
  - [ ] No architectural docs require edits; archived docs untouched

---

## Verification Plan (Whole Feature)
<!-- Every individual task is also verified against its own acceptance criteria. -->

### Automated Tests

- Task 1 ↔ framing/parse/resolution/boundary/classifier unit tests in `src/domain/multipart.rs` (fixed-boundary framing, binary-safety, resolution chain, parse matrix, boundary shape, classifier truth table).
- Task 2 ↔ parser/model unit tests: part derivation (kinds, repeated, required, encoding content types, ref resolution, loose-spec `None`), serde round-trip for old models.
- Task 3 ↔ request-builder unit tests: multipart success (boundary recovered from header), the full precedence matrix, every error-contract row, custom `Content-Type` override precedence, non-multipart suite untouched.
- Task 4 ↔ clap-tree unit tests: `-F` registration gating, repeatable occurrence order, carve-out, fail-fast erroring.
- Task 5 ↔ help unit tests: multipart guidance line with/without part metadata, `(file part)` annotation, non-multipart help unchanged.
- Task 6 ↔ e2e: full wire assertions + override matrix + error paths + discovery + non-multipart regression suites.
- Whole feature: `make check` (`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`); CI runs `make check` + `make build`.

### Manual Verification Steps

1. Install the new fixture against a local echo server; invoke with a text and a file
   part; verify the echoed wire: `Content-Type: multipart/form-data; boundary=clining-…`,
   frame order, filename, per-part headers (incl. spec-derived `image/jpeg`), byte-exact file tail.
2. `<command> --help` shows the `-F` flag, the guidance line, and part metadata.
3. `cat anything | <api> <group> <multipart-command> ...` → one-line error, exit 1, no HTTP traffic.
4. `cat body | ... -H "Content-Type: application/json"` on the same command → stdin sent raw with the manual header (override escape hatch works).
5. Regression: the petstore JSON flow still reads stdin byte-exact (`invoke.rs` behavior) — same binary, same shapes.
6. Upgrade path: hand-load a model JSON installed by the pre-change binary (with
   `CLINING_DIR` pointing at a copy) → `-F` works without re-install; re-installing
   additionally activates spec-derived per-part content types.

### Definition of Done (DoD)

- [ ] Code is formatted and linted (no warnings/errors — clippy runs with `-D warnings`); `make check` passes
- [ ] All automated unit & integration tests pass
- [ ] multipart/form-data commands accept `-F` parts and transmit a spec-valid, server-parsable request (verified byte-exactly in e2e)
- [ ] The stdin/override precedence matrix behaves exactly as specified (raw passthrough works with manual `Content-Type`; ambiguity is rejected)
- [ ] Existing behavior byte-for-byte unchanged for non-multipart bodies, custom headers, auth, and the stdout/stderr/exit-code contract
- [ ] No new external dependencies; onion layering intact (domain/application free of I/O and infra types)
- [ ] Model change additive: pre-existing installed models load unchanged and work with native `-F` (re-install optional)
- [ ] Living docs updated; archived docs untouched
