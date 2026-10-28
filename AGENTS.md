# AGENTS.md

## Commands

- `make check` — the full gate: `cargo fmt --check`, then `cargo clippy --all-targets -- -D warnings`, then `cargo test`. Run this before declaring work done; CI (`main` branch) runs `make check` + `make build`.
- Clippy uses `-D warnings`: any warning fails the build, not just errors. Fix or `#[allow]` deliberately.
- `cargo test <test_name>` runs a single test; integration binaries spawn the real CLI, so their output may be slow on first compile.
- `make install` — release build then copy to `$DESTDIR$PREFIX/bin/clining` (`PREFIX` defaults to `~/.local`).

## Architecture (onion, dependencies point inward only)

- `src/domain/` — pure Rust, zero external crates. Entities (`ApiModel`, `ApiOperation`, `Param`, ...), naming/kebab-case/header validation rules, and the domain error vocabulary.
- `src/application/` — services depend only on domain; they **own the port traits** in `application/ports.rs` (`ApiStore`, `SpecLoader`, `OpenApiParser`, `HttpInvoker`) and hold dependencies as `Box<dyn ...>` trait objects, not generics (per `docs/decisions/adr-001-*.md`).
- `src/infra/` — adapters implementing the ports: `cli/` (clap, dynamic command tree + help renderer), `http/` (blocking reqwest), `openapi/`, `source/`, `storage/` (JSON file store).
- `src/main.rs` — composition root; the only place wiring concrete adapters.
- Do not introduce outward dependencies: never add serde/clap/reqwest types to domain, or clap types to application.

## Verification & behavior contract

- The user-facing contract is specified in `docs/specs/spec.md`; docs index is `docs/index.md`. Honor it when changing CLI behavior: byte-exact response body on stdout, status line + headers on stderr, exit `0` for 2xx / `1` for anything else (clap usage errors are `2`), one-line `error: ...` on stderr for all failures.
- `docs/plans/archive/` predates ADR-001; file/symbol names there may not match current source. Trust the code over the archive docs.

## Tests

- `tests/*.rs` are end-to-end: they exec the built binary via `env!("CARGO_BIN_EXE_clining")` with `CLINING_DIR` pointed at a temp dir (`tests/common/mod.rs` provides `run_cli`, `temp_dir`, `MockServer`).
- HTTP behavior is tested against an in-process mock server (`MockServer` in `common`); request bytes are asserted via `request_text`. Fixtures live in `tests/fixtures/` (petstore.json).
- New CLI behavior needs an e2e test asserting the stream/exit-code contract, not just a unit test.

## Runtime layout

- Stored models: one JSON per API at `$CLINING_DIR` (typical in tests) or `~/.clining/<name>.json`; writes are atomic temp-file+rename. Bearer tokens are never stored — only a reference (`--token-env`/`--token-file`) resolved at invoke time.
