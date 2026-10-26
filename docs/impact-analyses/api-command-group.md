# Software Impact Analysis: Reserve a top-level `api` command group and move `install` into it (`clining api install`)

## Metadata

- **Author:** GLM-5.3-Flash Free (OpenCode agent)
- **Date:** 2026-10-26
- **Target Branch / Environment:** main
- **Overall Risk Level:** Medium
  - The change is functionally small (CLI surface restructuring, no storage or invocation-logic changes), but it breaks the public CLI contract and touches the dispatch layer that every invocation flows through, so a routing regression would affect all commands.

---

## Executive Summary

Clining v0 exposes API management as a single top-level `clining install` command. To make room for future management commands (`list`, `edit`, `remove`) and settings, this change reserves a top-level `api` command group and moves install into it: `clining api install <name> <spec> [...]`.

The recommended approach is a **management-dispatch layer** inside the `api` group rather than a one-off reroute: the group matches its second positional against the (currently single-entry) set of management verbs and falls back to help otherwise, so each future management command is one table entry instead of another special case in the top-level dispatch. Alongside the move, the name `api` must be **reserved** in `validate_name` — otherwise `clining api ...` becomes ambiguous when an installed API is named `api`.

Scope confirmed with the user: hard break of top-level `clining install` (no alias), reserve the `api` name, and move `install` only — `list`/`edit`/`remove` are designed-for but implemented later.

Critical findings:

- The top-level dispatch keys on the first positional argument (`src/infra/cli/clap_cli.rs:219-225`); `api` needs its own branch, and `clining api --help` must not fall into the invoke path (which would try to load a model named `api`).
- The persisted model format is untouched: **no data migration is needed**, and rollback is a plain revert.
- Most integration-test churn is centralized in one harness helper (`tests/common/mod.rs:86`); only `tests/auth.rs` builds `install` args inline.

---

## 1. Requirement & Problem Space Analysis

- **Objectives:**
  - Reserve a top-level `api` command group as the home for all API-management functionality.
  - Move the existing `install` command into the group: `clining api install <name> <spec> [--base-url <url>] [--token-env <VAR> | --token-file <path>]`.
  - Keep the structure ready for future management commands (`list`, `edit`, `remove`) and configuration settings without further dispatch surgery.
- **Success Criteria:**
  - `clining api install ...` parses, validates auth flags, and persists a model exactly as `clining install ...` does today (same flags, same error contract).
  - `clining --help` shows the `api` group; `clining api --help` documents the `install` subcommand.
  - Installing an API named `api` is rejected with a clear error; existing (non-`api`) installed models keep working with zero migration.
  - Top-level `clining install` no longer parses (hard break, per user decision).
- **Key Constraints:**
  - The per-API dynamic command tree (`clining <name> <group> <command>`) is the product's core surface and must not change shape.
  - The stderr/exit-code contract (`error: ...`, exit 1 for runtime failures, exit 2 for clap usage errors) is documented in `docs/specs/spec.md` and must be preserved.
  - Help output is agent-facing and covered by tests at every level; help text drift is a regression.

---

## 2. Context & Findings

**How the CLI is structured today.** `build_cli_command` (`src/infra/cli/clap_cli.rs:29-40`) assembles a static tree containing the top-level `install` subcommand plus either the dynamic per-API tree (when a model is loaded) or a placeholder `<api>` entry. There is no clap-level routing: `dispatch` (`src/infra/cli/clap_cli.rs:210-225`) inspects `argv[1]` and routes to one of three actions — `Install` (first positional is literally `install`), `Invoke` (any other non-flag first positional, treated as an API name), or `Static` (flags, help, zero args).

**The `Action` enum's names describe routing, not outcomes — and mislead.** Each variant selects which command tree the argv is parsed against and which handler owns it; the actual execution outcome (run a command vs. render help) is decided by clap during parsing, inside every handler. This is not hypothetical: `clining pets --help` takes the `Invoke` branch (`clap_cli.rs:222`, non-flag first positional), loads the model, and clap renders the API help during parse — no HTTP request is made, and an end-to-end test pins exactly this (`tests/help.rs:9-27`, exit 0 with help on stdout). Likewise `clining pets pets get-pet --help` renders command help through the same path. The variant names `Install`/`Invoke` therefore overstate what happens, which will get worse once the `api` group adds verbs whose help views must route somewhere. The rename is folded into this change rather than left as follow-up: `Action` → `Route`, with variants named after the parse tree each arm owns — `Management` (the `api` group's verb table), `Invocation` (the model-loaded dynamic tree), `Static` (the no-model tree). Route names intentionally say nothing about the outcome; help rendering happens through clap on any route.

**Install execution path.** `run_install` (`src/infra/cli/clap_cli.rs:249-291`) re-parses argv against the static tree and extracts the `install` subcommand matches, resolves `--token-file` to an absolute path (`resolve_token_file_path`, lines 93-107), builds the `AuthConfig`, and calls `CliApp::install` (lines 451-470), which delegates to `ApiModelService::learn` and prints the success line. The service layer (`src/application/api_model.rs:34-58`) and storage layer are command-agnostic — they care about names and models, not about where in the CLI tree the command sits.

**Namespace collision is real, not hypothetical.** `validate_name` (`src/domain/model.rs:238-257`) only rejects empty names, path separators, and `.`/`..`. Today `clining install api spec.json` is accepted and would persist a model named `api`. Once `api` becomes a reserved group, dispatch cannot tell `clining api install <spec>` (management) apart from invoking an API named `api` with a group named `install`. Because dispatch keys on `argv[1]`, an installed model named `api` would also make `clining api ...` fall into the invoke path and fail with a confusing "missing command group or command" error instead of the group's help. Reserving the name in `validate_name` closes both problems deterministically; the same check already guards the store defensively at `src/infra/storage/json_file_store.rs:45`.

**Error-copy coupling.** The invoke path's `NotFound` hint hard-codes the old command shape: `"...install it first with 'clining install <name> <spec>'"` (`src/infra/cli/clap_cli.rs:312-316`). Two integration tests assert on this copy (`tests/errors.rs:23`) and on top-level usage listing `install` (`tests/errors.rs:36`).

**Test blast radius is concentrated.** The shared harness helper `install()` builds the command argv in exactly one place (`tests/common/mod.rs:83-95`), and all five integration suites (`invoke.rs`, `help.rs`, `errors.rs`, `headers.rs`, and most of `auth.rs`) go through it. `tests/auth.rs` additionally builds `"install"` argv inline at five sites (lines 30, 53, 201, 225, 342). Unit tests in the `clap_cli.rs` test module parse the static tree directly and expect `install` at the top level (`install_args_parse_*`, lines 680-800; `static_help_lists_install_and_api_placeholder`, line 842; `dispatch_routes_by_first_positional`, line 1092).

**Docs describe the current surface.** The command contract is spelled out in `docs/specs/spec.md:29,39-44` (FR-1), the README quick-start (`README.md:22,25`), and the architecture overview's CLI description (`docs/architecture/overview.md:47`). Historical/archived plans (`docs/plans/archive/*`) describe the old shape but are records, not living docs — out of scope.

**No prior churn warning.** Git history shows `clap_cli.rs` is touched in essentially every feature commit (dispatch, help tree, headers, auth), confirming it is the hotspot — but the storage, parser, and invocation layers have stable interfaces that this change does not alter.

---

## 3. Proposed Solution Approaches

### Approach A: Wrap-and-reroute (minimal special-casing)

- **Description:** In `build_cli_command`, create a `Command::new("api")` group and attach `install_command()` under it. In `dispatch`, add a branch: `argv[1] == "api"` and `argv[2] == "install"` → `Install`; `argv[1] == "api"` otherwise → `Static` (renders group help / clap usage errors). `run_install` navigates `api` → `install` when extracting matches. Reserve `api` in `validate_name`. Update hint text, tests, docs.
- **Pros:**
  - Smallest diff; the management verbs stay as ad-hoc match arms.
  - Zero abstraction added for a single command.
- **Cons:**
  - Each future management command (`list`, `edit`, `remove`) adds another `argv[2] == "<verb>"` arm in `dispatch` and another hand-wired parse path — the pattern that produced today's three-arm `Action` enum grows linearly with commands.
  - Dispatch (manual argv inspection) and the static clap tree (help) can drift apart silently; a new verb present in one but not the other surfaces only at runtime.
- **Risk Level & Complexity:** Low-Medium. Correct for today's scope, but it seeds the exact special-case accumulation this change is meant to prevent.

### Approach B: Management-dispatch layer inside the `api` group (recommended)

- **Description:** Same tree change as A, but the group's routing is centralized: a new management arm routes `argv[1] == "api"` to a `run_api_group` method, which matches `argv[2]` against a single table of management verbs (today: `install`) and delegates to the matching handler; anything else (including `--help` and unknown verbs) falls back to the static tree for group-level help with a clap usage error. The static `api` group command in `build_cli_command` is built from the same verb table so help and dispatch cannot drift. `run_install`'s match extraction changes to `api` → `install`. As part of the same edit, the `Action` enum is renamed (to `Route`, with variants `Management` / `Invocation` / `Static` — see §4) so the dispatch types describe routing, not execution outcomes. Reserve `api` in `validate_name`; update hint text, tests, docs.
- **Pros:**
  - Adding `list`/`edit`/`remove` later is one table entry (verb + handler + help text) — no new dispatch arms, no new routing logic.
  - Single source of truth for "what does `clining api <verb>` accept"; help output and routing are derived together.
  - `clining api --help` and unknown-verb errors come from clap's static tree for free, consistent with the existing Static path.
- **Cons:**
  - Slightly more restructuring now (a new action arm and handler indirection) for a single command.
- **Risk Level & Complexity:** Low-Medium. Same touched files as A; the extra indirection is small and testable in isolation.

---

## 4. Recommended Way Forward & Design Decisions

- **Chosen Approach:** Approach B. The stated motivation for this change is precisely to make future management commands easy; A optimizes the current diff at the cost of reproducing the special-case pattern in `dispatch` for every future verb. B costs a few dozen extra lines now and makes each future command mechanical. The `Action` → `Route` rename rides along in the same edit: since this change already rewrites the enum's variants (`Install` disappears into `Management`), leaving a misleading name in freshly rewritten code would be the worst moment to fix it later.
- **Rationale:** All functional behavior (flags, auth resolution, persistence, error contract) is unchanged — the entire delta is presentation and routing, which lives entirely in `clap_cli.rs` plus name validation. B keeps that delta contained while making the group extensible, and the reserved-name check gives dispatch a deterministic guarantee it can lean on.
- **Effort Estimate:** M — multiple files, but one subsystem (CLI presentation); storage/invocation logic untouched; existing test coverage is dense and the harness concentrates most test churn in one helper. New tests needed for group routing, reserved name, and help shapes.
- **Assumptions:**
  - Hard break is acceptable: no user has scripted around `clining install` in a way that must keep working (v0, per user decision).
  - Pre-existing stores that already contain a model named `api` (installed before the reservation existed) are practically nonexistent; if one is found, the fix is manual — reinstall under a different name. A graceful runtime error for this case is a nice-to-have, not required.
  - Archived plan docs and past impact analyses are historical records and are not updated.
- **Security & Performance Impact Evaluation:**
  - **Security:** None. Auth flag semantics (`--token-env`/`--token-file`, mutual exclusion, reference-only storage, fail-fast validation) carry over verbatim; `validate_name` gains one reserved word. No new dependencies, no data exposure changes.
  - **Performance:** None. One extra string comparison in dispatch; no storage or network changes.
- **Clarifying Questions Resolved:**
  - Compat policy: **hard break** — top-level `clining install` is removed, no alias (user choice).
  - Collision policy: **reserve the name** — `validate_name` rejects installing an API named `api` (user choice).
  - Scope: **move `install` only** — `list`/`edit`/`remove` are enabled by the structure but implemented later (user choice).
  - Review feedback: rename the dispatch enum — the user observed that `Action` doesn't reflect the kind of CLI execution and that the `Invoke` arm can also terminate in help-only output. Verified in code (`clining pets --help` renders help through the invoke path, `tests/help.rs:9-27`) and adopted: rename `Action` → `Route` with outcome-agnostic variant names (`Management` / `Invocation` / `Static`).
- **Remaining Open Questions:**
  - None blocking. (Nice-to-have decided during implementation: exact wording of the reserved-name error and whether the empty-store `NotFound` hint should also mention `clining api list` as a future affordance — recommendation: keep the hint to `clining api install` only, matching what exists.)

---

## 5. Affected Components

| File / Component Path | Evidence (path:line) | Action | Description of Change |
| :--- | :--- | :--- | :--- |
| `src/infra/cli/clap_cli.rs` | `src/infra/cli/clap_cli.rs:29-40` | MODIFY | `build_cli_command`: attach an `api` group command containing `install` instead of the bare top-level `install`; group built from the management-verb table (Approach B). |
| `src/infra/cli/clap_cli.rs` | `src/infra/cli/clap_cli.rs:43-82` | MODIFY | `install_command()`: args unchanged; moves under the `api` group (parent changes only). |
| `src/infra/cli/clap_cli.rs` | `src/infra/cli/clap_cli.rs:210-225` | MODIFY | `Action` enum + `dispatch`: rename the enum to `Route` (variants `Management` / `Invocation` / `Static`) to reflect that it selects the parse tree and handler, not the execution outcome; route `argv[1] == "api"` to the new `Management` arm; remove the top-level `install` arm; `Invocation` and `Static` arms unchanged. |
| `src/infra/cli/clap_cli.rs` | `src/infra/cli/clap_cli.rs:239-291` | MODIFY | `run`/new `run_api_group`/`run_install`: match extraction becomes `api` → `install`; management-verb matching lives here. |
| `src/infra/cli/clap_cli.rs` | `src/infra/cli/clap_cli.rs:312-316` | MODIFY | `NotFound` hint copy: `clining install <name> <spec>` → `clining api install <name> <spec>`. |
| `src/infra/cli/clap_cli.rs` | `src/infra/cli/clap_cli.rs:112-114` | REVIEW | Top-level help will now show `api` alongside the `<api>` placeholder; keep the placeholder but confirm the dual listing is acceptable/clear in help tests. |
| `src/infra/cli/clap_cli.rs` (test mod) | `src/infra/cli/clap_cli.rs:680-800`, `842-849`, `1092-1106` | MODIFY | Unit tests: parse paths navigate `api` → `install`; `static_help_lists_install_and_api_placeholder` asserts group help; `dispatch_routes_by_first_positional` gains `api` cases (install verb, other verb → static). |
| `src/domain/model.rs` | `src/domain/model.rs:238-257` | MODIFY | `validate_name`: reject the reserved name `api` with a dedicated reason string. |
| `src/application/api_model.rs` | `src/application/api_model.rs:42` | VERIFY ONLY | Inherits the reservation via `validate_name`; no code change; unit tests at lines 166, 212, 237 remain valid. |
| `src/infra/storage/json_file_store.rs` | `src/infra/storage/json_file_store.rs:45` | VERIFY ONLY | Defensive `validate_name` call inherits the reservation; no code change. |
| `tests/common/mod.rs` | `tests/common/mod.rs:83-95` | MODIFY | Harness `install()`: argv becomes `["api", "install", name, spec, ...]`; one change covers most of all five integration suites. |
| `tests/auth.rs` | `tests/auth.rs:30`, `53`, `201`, `225`, `342` | MODIFY | Inline `"install"` argv sites become `["api", "install", ...]`. |
| `tests/errors.rs` | `tests/errors.rs:23`, `36` | MODIFY | Hint-text assertion updated to the new command shape; top-level usage assertion still matches `install` (now listed under the `api` group) — verify wording. |
| `tests/help.rs`, `tests/invoke.rs`, `tests/headers.rs` | `tests/help.rs:12` (representative), `tests/invoke.rs:18` | VERIFY ONLY | All go through the harness helper; should pass unchanged once the helper is updated. |
| `README.md` | `README.md:22`, `25` | MODIFY | Quick-start command becomes `clining api install ...`. |
| `docs/specs/spec.md` | `docs/specs/spec.md:29`, `39-44` | MODIFY | FR-1 command surface, FR-1.1 wording, and error-table references move to the `api` group shape. |
| `docs/architecture/overview.md` | `docs/architecture/overview.md:47` | MODIFY | CLI description: static `api` command group containing management subcommands (install today), plus the dynamic per-API tree. |
| `docs/index.md` | `docs/index.md:35` | MODIFY | Phase-2 summary mentions `clining install` — update the living-docs phrasing (archive docs untouched). |
| `docs/plans/archive/*`, `docs/impact-analyses/issue-8.md` | e.g. `docs/plans/archive/phase-2-learn.md:102-112` | SKIP | Historical records of past work; not updated. |

### Database & Schema Changes

- None. The persisted `ApiModel` JSON format and the store layout (`~/.clining/<name>.json` / `CLINING_DIR`) are untouched.

### API & Contract Changes

- **Public CLI contract (breaking):** `clining install ...` → `clining api install ...`. Flags (`--base-url`, `--token-env`, `--token-file`), their mutual exclusion, and all error/exit-code semantics are unchanged. `clining api --help` and `clining --help` output shapes change.
- **New validation contract:** installing an API named `api` now fails with `DomainError::InvalidName` (exit 1, `error: ...` on stderr).
- The dynamic per-API tree (`clining <name> <group> <command>`), stdin/stdout/stderr split, and SIGPIPE behavior are unchanged.

### Backward Compatibility & Migration Strategy

- **Stored data:** Fully compatible. Model files load without reinstall; only the command used to create/replace them changes.
- **CLI users/scripts:** Breaking by decision (v0, hard break). The `NotFound` hint and docs steer users to the new shape.
- **Edge case:** A store containing a pre-reservation model named `api` would shadow the group in the `Invoke` dispatch arm's assumptions; with the reserved name enforced going forward this can only exist as a legacy artifact. Documented as an assumption; optional graceful error if detected.

### Rollback Plan

- Plain `git revert` of the change. No data migration to undo; models installed via `clining api install` are byte-identical in format to those installed via the old command, so a reverted binary continues to load them. Top-level `clining install` works again after revert.

---

## 6. Verification & Testing Plan

### Automated Tests

- **Existing Tests to Run:** `make check` (fmt + clippy + full test suite: unit tests in `src/` plus the five integration suites).
- **New Tests to Add:**
  - Unit (clap_cli test mod): `clining api install ...` parses name/spec/`--base-url`/`--token-env`/`--token-file` through the `api` → `install` path; mutual-exclusion conflict still exits 2; `install` help still documents token flags; top-level help lists `api` (not bare `install`).
  - Unit (dispatch): the routing test (renamed with the enum, e.g. `dispatch_routes_by_first_positional` → `dispatch_routes_by_parse_tree` or similar): `api install ...` routes to `Management`; `api --help` and `api <unknown-verb>` route to `Static`; bare `api` does **not** route to `Invocation`; `pets --help` still routes to `Invocation` (help-through-invocation regression pin).
  - Unit (domain): `validate_name("api")` fails; `validate_name("apix")`, `"api-2"` still pass.
  - Integration: reserved-name install attempt prints the reason and persists nothing; unknown API error hint contains `clining api install`; zero-args usage output names the `api` group.

### Manual Verification Steps

1. `cargo run -- api install pets tests/fixtures/petstore.json --base-url http://localhost:8080`; confirm `Installed pets (N commands, M groups)` and inspect the stored JSON.
2. `cargo run -- --help`, `cargo run -- api --help`, `cargo run -- api install --help` — confirm the group appears at top level and install help is intact.
3. `cargo run -- api pets pets get-pet --pet-id 42` against a mock server — confirm the dynamic invoke path is unaffected.
4. `cargo run -- api nope store get-pets` — confirm the updated `install it first with 'clining api install ...'` hint.
5. `cargo run -- install pets spec.json` — confirm it now fails with a clap usage error (exit 2), i.e. the hard break is effective.

### Potential Regression Risks

- **Dispatch misrouting:** the highest-risk line in the change. If `api` handling accidentally lets `clining api <group> <cmd>` (an API literally named `api`) through to the `Invocation` route, or sends `clining api --help` into the model-loading path, every management command or help view breaks. Mitigated by dedicated dispatch unit tests and by reserving the name.
- **Help-text drift** between the static tree and the management-verb table (Approach B mitigates by deriving both from one table).
- **Stale user-facing copy:** any remaining `clining install` string in error hints or docs would point users at a broken command. Mitigated by grepping for `install` in `src/` and living docs before merge (archived plans excluded) and by the `tests/errors.rs` assertions.
- **Integration-test blind spots:** because the harness helper hides the argv change, a typo in it would fail ~every suite loudly — which is protective, not a risk.
