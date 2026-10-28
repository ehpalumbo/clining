# Implementation Plan: Issue #18 — Reserve a top-level `api` command group; move install to `clining api install`

## Metadata

- **Author:** OpenCode agent (GLM-5.3-Flash Free)
- **Date:** 2026-10-28
- **Structure:** Single increment
- **Target branch / environment:** `api-command-group` (off `main`); Linux, Rust `edition 2024`, clap 4
- **Status:** Ready for implementation

---

## References

- [Software Impact Analysis: Reserve a top-level `api` command group and move `install` into it](../impact-analyses/api-command-group.md) — lives on branch `api-command-group`; land it together with this plan
- [Requirements Specification — clining v0](../specs/spec.md)
- [Architecture Overview](../architecture/overview.md)
- [GitHub Issue #18](https://github.com/ehpalumbo/clining/issues/18)

---

## Executive Summary

Restructure the static CLI surface around a management group. The top-level `install`
subcommand moves under a new `api` group (`clining api install <name> <spec> [...flags]`),
top-level `clining install` is removed with no alias (hard break), the name `api` is
reserved in `validate_name`, and the dispatch enum `Action` is renamed to `Route` with
parse-tree-oriented variants (`Management` / `Invocation` / `Static`).

The management group is routed from a **single verb table** that both builds the static
clap tree (help) and feeds the dispatch membership check, so help and routing cannot
drift. Today the table has one entry: `install`. Future management commands
(`list`, `edit`, `remove`) and settings become table entries. All flags, the dynamic
per-API tree (`clining <name> <group> <command>`), the stderr/exit-code contract, and
the persisted model format are unchanged — **no data migration**; rollback is a plain
`git revert` of the single commit.

### Architecture

- **Domain (`src/domain/model.rs`):** `validate_name` gains one reserved word (`api`); no new types, no outward dependencies.
- **Infrastructure/CLI (`src/infra/cli/clap_cli.rs`):** a management-verb table shared by tree building and dispatch; the `Route` enum replaces `Action`; the `NotFound` hint copy points at the new command shape.
- **Application / storage / HTTP:** untouched. `ApiModelService::learn`, the JSON store, and the invoker are command-agnostic.
- **Tests:** the shared harness helper absorbs most argv churn; new unit + e2e tests pin routing, help shapes, the reserved name, and the hard break.

### Assumptions

Adopted from the approved impact analysis without re-negotiation:

1. **Hard break** — top-level `clining install` is removed, no alias. It surfaces as a clap usage error (exit 2): the dispatch arm for the root word `install` routes to `Static`, where clap reports an unrecognized subcommand. This matches the analysis's manual verification step 5.
2. **Reserved name is exact-match** — `validate_name` rejects exactly `api` (lowercase); `API`, `apix`, `api-2` remain valid, since dispatch is also an exact string compare on `argv[1]`.
3. **Scope is install only** — `list`/`edit`/`remove` are enabled by the structure, not implemented now. The `NotFound` hint mentions only `clining api install` (no future-verb advertising).
4. **Legacy stores** containing a pre-reservation model named `api` are treated as practically nonexistent (v0); a graceful runtime error for that case is out of scope.
5. **Mechanics** (implementation detail, identical observable outcomes to Approach B): the dispatch arm `argv[1] == "api"` consults the verb table for `argv[2]` — known verb → `Route::Management`, anything else (including `--help`, unknown verbs, bare `api`) → `Route::Static`, where the static tree renders group help (exit 0) or a clap usage error (exit 2). This keeps the `§6` dispatch unit tests of the analysis verbatim.

---

## Configuration & Environment Updates

- **Environment Variables:** None.
- **Feature Flags:** None.
- **External Dependencies:** None.

---

## Task Details

### 1. Reserve the name `api` in domain name validation

- **Prerequisites / Dependencies:** None.
- **Affected Files:**
  - [model.rs](src/domain/model.rs) - MODIFY
- **Affected Symbols:**
  - `validate_name()` (`src/domain/model.rs:238`)
- **Description:** Add an exact-match branch rejecting `api` with a dedicated reason string (e.g. `"name must not be the reserved command group name 'api'"`) rendered through `DomainError::InvalidName`. Keep it a pure string check — no external types.
- **Acceptance Criteria:**
  - [ ] `validate_name("api")` returns `DomainError::InvalidName` whose message names the reserved word
  - [ ] `validate_name` still passes for `"pets"`, `"apix"`, `"api-2"`, `"API"` and still fails for `""`, `"a/b"`, `"a\\b"`, `"."`, `".."`
  - [ ] `cargo test -p clining --lib -- domain::` passes including the existing validate-name unit tests

### 2. Build the `api` management group from a shared verb table

- **Prerequisites / Dependencies:** Task 1.
- **Affected Files:**
  - [clap_cli.rs](src/infra/cli/clap_cli.rs) - MODIFY
- **Affected Symbols:**
  - `build_cli_command()` (`src/infra/cli/clap_cli.rs:29`), new management-verb table, `install_command()` (`src/infra/cli/clap_cli.rs:43`)
- **Description:** Introduce a single management-verb table enumerating the group's
  subcommands (today: `install_command()` only — its args are unchanged). Share it so
  both consumers derive from it:
  - `build_cli_command(None)` / `build_cli_command(Some(m))` attach `Command::new("api")` (with a clear `about`, e.g. referencing API model management, `subcommand_required(true)`, `disable_help_subcommand(true)` consistent with the tree) containing the table's verb commands.
  - A membership helper (e.g. `management_verb(argv2: Option<&str>) -> bool`) reads the same table for dispatch (Task 3).
  Keep the `<api>` placeholder for the no-model case; top-level help will show both `api` (management) and `<api>` (installed invocation).
- **Acceptance Criteria:**
  - [ ] `build_cli_command(None).render_help()` contains the `api` group with `install` listed beneath it and still contains the `<api>` placeholder
  - [ ] `api install --help` still documents `name`, `spec`, `--base-url`, `--token-env`, `--token-file` semantics (existing install help tests adapt their navigation only)
  - [ ] Adding a verb to the table requires changing only the table (one entry) — reviewed for AST/help consistency

### 3. Rename the dispatch enum to `Route` and rework routing

- **Prerequisites / Dependencies:** Task 2 (table membership helper).
- **Affected Files:**
  - [clap_cli.rs](src/infra/cli/clap_cli.rs) - MODIFY
- **Affected Symbols:**
  - `Action` enum + `dispatch()` (`src/infra/cli/clap_cli.rs:210-225`), renamed to `Route` with variants `Management(Vec<String>)` / `Invocation(Vec<String>)` / `Static(Vec<String>)`; `CliApp::run()` (`src/infra/cli/clap_cli.rs:239`) arm names only
- **Description:** Rework `dispatch` (keys on `argv[1]`, exact string compares):
  - `Some("api")` with the verb table matching `argv[2]` → `Route::Management`
  - `Some("api")` otherwise, the root word `Some("install")`, flags, and zero args → `Route::Static` (hard-break: clap usage error for `install`; group help for `api`)
  - any other non-flag first positional → `Route::Invocation` (unchanged behavior)
  The variants must keep saying "which parse tree argv goes to", not "what executes" — help rendering can happen through clap on any route (`clining pets --help` already renders help via Invocation).
- **Acceptance Criteria:**
  - [ ] Dispatch unit test (renamed from `dispatch_routes_by_first_positional`, e.g. `dispatch_routes_by_parse_tree`): `api install ...` → `Management`; `api --help`, `api <unknown-verb>`, bare `clining`, `clining install ...` → `Static`; `pets ...` → `Invocation`; `clining --help` → `Static`
  - [ ] No variant named after an execution outcome remains; doc comments updated

### 4. Route `run_install` through the `api` group; update the hint copy

- **Prerequisites / Dependencies:** Tasks 2 and 3.
- **Affected Files:**
  - [clap_cli.rs](src/infra/cli/clap_cli.rs) - MODIFY
- **Affected Symbols:**
  - `run_install()` (`src/infra/cli/clap_cli.rs:249`): match extraction becomes `api` → `install`; `NotFound` hint (`src/infra/cli/clap_cli.rs:312-316`): `'clining install <name> <spec>'` → `'clining api install <name> <spec>'`
- **Description:** The Management arm keeps calling `run_install`; only the `matches.subcommand_matches` navigation changes. Everything after extraction (token-file resolution, auth construction, `CliApp::install`, success line) is byte-identical. Update the doc comment on `run_install`. Verify the storage-layer defensive call (`src/infra/storage/json_file_store.rs:45`) and `src/application/api_model.rs:42` inherit Task 1's reservation without code change.
- **Acceptance Criteria:**
  - [ ] Unit: `clining api install pets spec.json --base-url u` parses name/spec/`--base-url`; `--token-env`/`--token-file` both given still exits the clap conflict as exit code 2
  - [ ] Unit: install help navigation resolves under `api`; the success output line (`Installed <name> (N commands, M groups)`) is unchanged
  - [ ] Existing auth validation behavior (unset env var at install fails, persists nothing) is preserved — covered by unchanged `tests/auth.rs` logic once the harness is updated (Task 5)

### 5. Update the test harness and add e2e coverage for the new surface

- **Prerequisites / Dependencies:** Tasks 1–4 (e2e needs the full change; unit tests from Tasks 1–3 run earlier).
- **Affected Files:**
  - [common/mod.rs](tests/common/mod.rs) - MODIFY (`install()` helper at `tests/common/mod.rs:83-95`: argv becomes `["api", "install", name, spec, ...];`)
  - [auth.rs](tests/auth.rs) - MODIFY (inline argv sites at lines 30, 53, 201, 225, 342 become `["api", "install", ...]`)
  - [errors.rs](tests/errors.rs) - MODIFY (hint assertion at line 23 → `clining api install`; zero-args usage assertion at line 36 → `api` group wording; new hard-break case) 
  - [help.rs](tests/help.rs) / [invoke.rs](tests/invoke.rs) / [headers.rs](tests/headers.rs) - VERIFY ONLY (go through the harness helper; must pass unchanged)
- **Affected Symbols:**
  - `install()` (harness), `run_cli` call sites with inline argv
- **Description:** Update the harness and inline argv first; run the full suite to
  expose any remaining `clining install` strings. Then add e2e tests (asserting the
  stream/exit-code contract per `docs/specs/spec.md`):
  - reserved name: `clining api install api <spec>` → exit 1, one `error: ...` line on stderr mentioning the reserved word, zero bytes on stdout, no model file persisted
  - group help: `clining api --help` → exit 0, help on stdout, `install` listed
  - unknown verb: `clining api frobnicate` → exit 2, clap usage error on stderr
  - hard break: `clining install pets <spec>` → exit 2, clap usage error on stderr
  - invocation unaffected: an installed API still invokes through `run_cli` with token/header flags (existing suites cover; add only if a gap appears)
- **Acceptance Criteria:**
  - [ ] All five integration suites pass with the updated helper
  - [ ] New e2e tests assert exit code and stream split exactly as specified above
  - [ ] No test or helper builds top-level `install` argv anymore (grep `tests/` for it — only `api install` remains)

### 6. Update living docs to the new command shape

- **Prerequisites / Dependencies:** Tasks 1–5 (document the landed behavior).
- **Affected Files:**
  - [README.md](README.md) - MODIFY (quick-start line 45 → `clining api install <my-api-name> ...`; surrounding prose if it references the shape)
  - [spec.md](docs/specs/spec.md) - MODIFY (FR-1 command row at line 29; FR-1.1 at line 39; the name rules/errors section gains the reserved word `api`)
  - [overview.md](docs/architecture/overview.md) - MODIFY (Infrastructure/CLI bullet ~line 47: "a static `api` management command group containing `install`, built from a shared verb table, plus a runtime-built dynamic per-API tree")
  - [index.md](docs/index.md) - MODIFY (phase-2 summary line 41 mentions `clining install`; reword to reflect the group without touching archive entries)
- **Affected Symbols:** None (prose).
- **Description:** Update every living doc that spells the old command shape; skip
  `docs/plans/archive/*` and past impact analyses (historical records). Grepping
  `'clining install' README.md docs/specs docs/architecture docs/index.md` must return
  zero hits when done (phrase `clining api install` is fine since it doesn't match
  `clining install ` word-boundary-at-position-zero; use targeted grep per file).
- **Acceptance Criteria:**
  - [ ] No living doc instructs users to run top-level `clining install`
  - [ ] `docs/specs/spec.md` documents both the new command shape and the reserved name `api`
  - [ ] Archived documents remain untouched (`git diff --stat docs/plans/archive docs/impact-analyses/issue-8.md` is empty)

---

## Verification Plan (Whole Feature)
<!-- This plan covers the whole feature. Every individual task is also verified against its own acceptance criteria. -->

### Automated Tests

- Task 1 ↔ new `validate_name` unit cases (reserved `api`; adjacent strings still valid).
- Task 2 ↔ help-shape unit tests: top-level help lists `api` with `install`; `install` help unchanged beneath the group; `<api>` placeholder preserved (adapt `static_help_lists_install_and_api_placeholder` at `src/infra/cli/clap_cli.rs:842`).
- Task 3 ↔ routing unit test `dispatch_routes_by_parse_tree` (adapted from `dispatch_routes_by_first_positional`, `src/infra/cli/clap_cli.rs:1092`).
- Task 4 ↔ parse-navigation unit tests (`install_args_parse_*` at `src/infra/cli/clap_cli.rs:681-794` navigate `api` → `install`).
- Task 5 ↔ e2e: updated all-suite run via harness; new reserved-name, group-help, unknown-verb, and hard-break cases in `tests/errors.rs` (or a sibling suite).
- Whole feature: `make check` (fmt + clippy `-D warnings` + full test suite).

### Manual Verification Steps

1. `cargo run -- api install pets tests/fixtures/petstore.json --base-url http://localhost:8080` → `Installed pets (N commands, M groups)` on stdout; the stored JSON format is identical to a same-config model installed by the pre-change binary.
2. `cargo run -- --help`, `cargo run -- api --help`, `cargo run -- api install --help` → group appears at top level; install help intact with token flags.
3. Against a mock server (MockServer pattern): `cargo run -- pets pets get-pet --pet-id 42` → unchanged invocation path (status line + headers on stderr, body on stdout).
4. `cargo run -- api nope pets get-pets` → `error: ... install it first with 'clining api install <name> <spec>'`, exit 1.
5. `cargo run -- install pets tests/fixtures/petstore.json` → clap usage error on stderr, exit 2 (hard break effective).
6. `git revert` dry-run sanity: the change is one commit; revert restores top-level `install` and every model installed via `clining api install` still loads.

### Definition of Done (DoD)

- [ ] Code is formatted and linted (no warnings/errors); `make check` passes
- [ ] All automated unit & integration tests pass successfully
- [ ] No regression: dynamic per-API tree, `-H`/`--clining-header` flag, token-reference persistence, stderr/exit-code contract unchanged
- [ ] Reserved-name contract live end to end (rejected at install; existing non-`api` models load with zero migration)
- [ ] Living docs updated; archived docs untouched
