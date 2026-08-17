---
type: workflow
title: "clining v0 — Phased Implementation Plan"
description: "The four-phase, one-commit-per-phase build plan for clining v0, with per-phase status and the whole-feature definition of done."
tags:
  - "clining"
  - "implementation-plan"
  - "phases"
  - "workflow"
timestamp: "2026-08-17T19:47:50Z"
related:
  - "[Requirements Specification](../specs/spec.md)"
  - "[Architecture Overview](../architecture/overview.md)"
---

# Phased Implementation Plan — clining v0

## Executive Summary

clining v0 is built in four phases, each delivered as a single reviewable commit. Phases 2 and 3 are vertical slices — each delivers an end-to-end flow (learn and invoke) — while Phase 1 establishes the skeleton and Phase 4 polishes discovery and hardening. Requirements live in the [Requirements Specification](../specs/spec.md) and the layering in the [Architecture Overview](../architecture/overview.md); every phase links back to the requirements it satisfies.

## Commit Strategy

One commit per phase, plus an initial documentation commit containing this plan. Phase statuses below are updated as work progresses.

## Phase Index

| Phase | Scope | Status | Plan |
| --- | --- | --- | --- |
| 1 | Toolchain & onion scaffolding — lint-clean crate skeleton | **Completed** | [phase-1-scaffolding.md](phases/phase-1-scaffolding.md) |
| 2 | "Learn" slice — model, OpenAPI parser, store, source loader, `install` | Planned | [phase-2-learn.md](phases/phase-2-learn.md) |
| 3 | "Invoke" slice — request builder, HTTP adapter, dynamic CLI, streaming | Planned | [phase-3-invoke.md](phases/phase-3-invoke.md) |
| 4 | Discovery polish & hardening — help, error UX, E2E tests, README/Makefile | Planned | [phase-4-polish.md](phases/phase-4-polish.md) |

Status values: **Planned** → **In Progress** → **Completed**.

## Configuration & Environment Updates

- **Environment variables:** `HOME` (resolves `~/.clining/`); `CLINING_DIR` overrides the model store root (used by tests to isolate storage, useful for users).
- **Feature flags:** None.
- **External dependencies:** `rustup` toolchain; Rust crates `clap` 4, `reqwest` 0.12 (blocking), `serde` + `serde_json`, `anyhow`, `thiserror`. Integration tests use `std::net::TcpListener` (no extra dev dependencies).

## Definition of Done (whole feature)

- [ ] `clining install` persists a correct `ApiModel` from file or URI, honoring `--base-url`.
- [ ] `clining <name> <group> <command> --param v < body` executes the request; body → stdout byte-exact, status line + headers → stderr; exit 0/1 per status.
- [ ] Help renders at every level with parameters and body schema hints.
- [ ] All unit + integration tests pass; `cargo clippy -- -D warnings` clean; formatted.
- [ ] README + Makefile present; onion layering (domain ← application ← infra) enforced.
- [ ] Requirements FR-1..FR-5 satisfied per the traceability table in the spec.
