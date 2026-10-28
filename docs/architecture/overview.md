---
type: architecture
title: "Architecture Overview"
description: "The concentric-layered architecture of clining: domain entities, application services owning their ports, and infrastructure adapters, with inward-only dependencies."
tags:
  - "clining"
  - "architecture"
  - "onion"
  - "rust"
timestamp: "2026-08-22T23:11:43Z"
related:
  - "[Requirements Specification](../specs/spec.md)"
  - "[Phased Implementation Plan](../plans/implementation-plan.md)"
  - "[ADR-001: Application-Owned Ports and Owned Service Dependencies](../decisions/adr-001-application-ports-and-owned-services.md)"
---

# Architecture Overview

## Rationale

clining is small today, but its two halves — learning an API versus invoking endpoints — grow in opposite directions, and parsing, storage, HTTP, and CLI presentation are all replaceable. The onion layout keeps that flexibility by making the core logic (what an API model *is* and how a command becomes an HTTP request) independent of clap, reqwest, serde, and the filesystem. Dependencies point inward only: infrastructure may depend on services and the domain, services only on the domain, and the domain on nothing outside itself.

## Layers

### Domain (core)

The innermost layer, written in pure Rust with no external dependencies. It owns:

- **Entities** — `ApiModel` (including optional `AuthConfig` token reference), `ApiOperationGroup`, `ApiOperation`, `Param`, `HttpMethod`, `BodySpec`, plus request/response value types. Plain data with serde derives for persistence. Runtime-only `ApiInvocationRequest` carries parsed `CustomHeader`s but is never serialized.
- **Rules** — pure functions for command naming (operationId kebab-casing, method+path fallback, collision disambiguation), kebab-casing of parameter CLI names, and custom-header parsing/validation (`CustomHeader::parse`, including the framing/hop-by-hop denylist).
- **Errors** — the domain error vocabulary (not found, invalid stored model, invalid spec, parameter/body/header errors, auth errors, network, I/O).

### Application (services)

Depends only on the domain, and owns the port traits its services consume (see [ADR-001](../decisions/adr-001-application-ports-and-owned-services.md)). Services own their dependencies as trait objects, keeping signatures free of generics and lifetimes:

- **API model management** — install from spec (`SpecLoader` → `OpenApiParser` → `ApiStore.save`, with base-URL override and optional `AuthConfig` handling, including fail-fast token resolvability check) and load an installed model by name.
- **API invocation** — resolve group/operation on a loaded model → resolve Bearer token from the stored auth reference (unless a custom `Authorization` header is present) → build request from params, body, token, and custom headers → `HttpInvoker.send`. Request construction merges custom headers case-insensitively over generated headers and preserves repeated header lines.
- **Auth resolution** — `application::auth` resolves `AuthConfig::Env`/`File` to a token value at install (validation) and invoke time; the secret never reaches the store, logs, or help.

The clap tree builder reads the domain model directly (groups, operations, parameters, body schemas, and response schemas) to render help at every level.

### Infrastructure (adapters)

Implements the application ports and presentation; may depend on any inner layer:

- **CLI** — clap: a static `api` management command group containing `install`, built from a shared verb table, plus a runtime-built dynamic command tree (per installed API) that resolves to management/invoke routes; reads stdin and writes stdout/stderr. The `api` install name is reserved (exact match) so dispatch on the first positional stays unambiguous. Every endpoint command registers a repeatable `-H`/`--clining-header` flag (short-only when an API parameter claims the long name); values are parsed and validated before stdin is read. A presentation-only help renderer turns the domain model into plain-text parameter shapes and recursive request/response schema walkthroughs, attached to each command through clap's `after_help`; clap's `help_template` only arranges the static sections.
- **Storage** — JSON file store under `~/.clining/` (override `CLINING_DIR`), atomic writes, distinct not-found vs. invalid-stored-model errors.
- **OpenAPI parsing** — serde structs for the OpenAPI 3.0 subset mapped into the domain model.
- **Source loading** — local file path or `http(s)://` URI → bytes.
- **HTTP** — blocking reqwest client implementing `HttpInvoker`.

### Composition root

`main.rs` wires concrete adapters into the application services, which own them. The CLI layer is the only place that knows about argument parsing and stdout/stderr conventions.

## Dependency Rule

```text
infrastructure  →  application  →  domain
       └──────────────┴──────────────┘
              (nothing depends outward)
```

## Ports & Adapters

| Port (application trait) | Adapter (infra) | Purpose |
| --- | --- | --- |
| `ApiStore` | JSON file store | Persist/load API models |
| `SpecLoader` | Source loader | Fetch spec bytes from path or URI |
| `OpenApiParser` | OpenAPI 3.0 serde parser | Spec JSON → `ApiModel` |
| `HttpInvoker` | Blocking reqwest client | Send built requests, return responses |

## Key Domain Rules

- Endpoints group by OpenAPI tag; untagged endpoints land in `default`.
- Command name = kebab-cased `operationId`, else `method-path-segments` joined by `-` and skipping path variables; collisions get `-2`, `-3`, … suffixes.
- Parameters expose a kebab-cased CLI name (`cli_name`) while retaining the original name for path/query substitution.
- Custom headers are parsed as `Name: value` pairs in the domain; framing/hop-by-hop names and non-ASCII/control values are rejected, generated headers are overridden case-insensitively, and repeated names remain separate lines.
- Base URL comes from `servers[0].url` unless overridden at install.

## Storage Layout

- Root: `$CLINING_DIR` if set, else `~/.clining/`.
- One plain JSON file per API: `<name>.json`.
- Atomic replace on save; corrupt files surface as `InvalidStoredModel`, never as not-found.

## Data Flows

### Install (learn)

`cli args → API model service → SpecLoader → OpenApiParser → ApiModel (+ AuthConfig reference, validated by resolving once) → ApiStore.save → ~/.clining/<name>.json`

### Invoke

`cli args + stdin body → API model service (load model) → API invocation service → resolve auth token unless a custom Authorization header is present → build request (merge custom headers over generated ones, + Authorization header) → HttpInvoker.send → response → body:stdout, status+headers:stderr, exit code`
