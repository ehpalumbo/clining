# Repository Docs

> Cross-cutting knowledge base index. Read individual pages for full context.

## Specs

The product specification for clining: goals, usage contract, functional and non-functional requirements, error model, and scope boundaries.

- [Requirements Specification](specs/spec.md) - Defines clining v0's functional and non-functional requirements, usage contract, error model, and explicitly out-of-scope items.

## Architecture

Repository-wide architectural guidance for the clining codebase.

- [Architecture Overview](architecture/overview.md) - The concentric domain/application/infra layering with inward-only dependencies, application-owned ports, key domain rules, and the install/invoke data flows.

## Workflows

Operational processes that keep the repository shippable.

- [Release Process](workflows/releases.md) - How tag pushes and manual dispatches run the quality gate, package the binary, and publish GitHub Releases.

## Decision Records

Architecture decision records capturing the context, choices, and trade-offs behind structural changes to the codebase.

- [ADR-001: Application-Owned Ports and Owned Service Dependencies](decisions/adr-001-application-ports-and-owned-services.md) - Moves port traits from domain to application and has services own their dependencies as trait objects, eliminating generics and lifetimes from service signatures.

## Plans

The master phased implementation plan for clining; completed phase and issue working documents live in the archive below.

- [Phased Implementation Plan](plans/implementation-plan.md) - The four-phase, one-commit-per-phase build plan for clining v0 with per-phase status and the whole-feature definition of done.

## Plans Archive

Completed implementation plans and per-phase working documents, kept as historical records of how the codebase was built. They predate ADR-001, so file and symbol names may no longer match current source.

- [Issue #1: Schema `$ref` Resolution & Body Schemas](plans/archive/schema-registry.md) - Resolves OpenAPI `$ref` references into a typed schema registry and stores request + response body schemas.
- [Phase 1: Toolchain & Onion Scaffolding](plans/archive/phase-1-scaffolding.md) - Installs the Rust toolchain and lays down the crate module skeleton with a lint-clean, test-passing baseline.
- [Phase 2: "Learn" Vertical Slice](plans/archive/phase-2-learn.md) - Implements the domain model, OpenAPI 3.0 parser, JSON store, source loader, and the API install flow.
- [Phase 3: "Invoke" Vertical Slice](plans/archive/phase-3-invoke.md) - Implements request building, the reqwest adapter, the dynamic clap tree, and response streaming with exit-code semantics.
- [Phase 4: Discovery Polish & Hardening](plans/archive/phase-4-polish.md) - Enriches help/descriptions, hardens error UX, adds end-to-end tests, and adds the README and Makefile.
