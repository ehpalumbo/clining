# Repository Docs

> Cross-cutting knowledge base index. Read individual pages for full context.

## Specs

The product specification for clining: goals, usage contract, functional and non-functional requirements, error model, and scope boundaries.

- [Requirements Specification](specs/spec.md) - Defines clining v0's functional and non-functional requirements, usage contract, error model, and explicitly out-of-scope items.

## Architecture

Repository-wide architectural guidance for the clining codebase.

- [Architecture Overview](architecture/overview.md) - The concentric domain/application/infra layering with inward-only dependencies, ports, key domain rules, and the learn/invoke data flows.

## Plans

Phased implementation plan and per-phase working documents with progress status.

- [Phased Implementation Plan](plans/implementation-plan.md) - The four-phase, one-commit-per-phase build plan for clining v0 with per-phase status and the whole-feature definition of done.
- [Issue #1: Schema `$ref` Resolution & Body Schemas](plans/schema-registry.md) - Resolves OpenAPI `$ref` references into a typed schema registry and stores request + response body schemas.
- [Phase 1: Toolchain & Onion Scaffolding](plans/phases/phase-1-scaffolding.md) - Installs the Rust toolchain and lays down the crate module skeleton with a lint-clean, test-passing baseline.
- [Phase 2: "Learn" Vertical Slice](plans/phases/phase-2-learn.md) - Implements the domain model, OpenAPI 3.0 parser, JSON store, source loader, and the `clining install` flow.
- [Phase 3: "Invoke" Vertical Slice](plans/phases/phase-3-invoke.md) - Implements request building, the reqwest adapter, the dynamic clap tree, and response streaming with exit-code semantics.
- [Phase 4: Discovery Polish & Hardening](plans/phases/phase-4-polish.md) - Enriches help/descriptions, hardens error UX, adds end-to-end tests, and adds the README and Makefile.
