---
type: architecture
title: "Architecture Overview"
description: "The concentric-layered architecture of clining: domain entities and ports, application use cases, and infrastructure adapters, with inward-only dependencies."
tags:
  - "clining"
  - "architecture"
  - "onion"
  - "rust"
timestamp: "2026-08-17T19:47:50Z"
related:
  - "[Requirements Specification](../specs/spec.md)"
  - "[Phased Implementation Plan](../plans/implementation-plan.md)"
---

# Architecture Overview

## Rationale

clining is small today, but its two halves — learning an API versus invoking endpoints — grow in opposite directions, and parsing, storage, HTTP, and CLI presentation are all replaceable. The onion layout keeps that flexibility by making the core logic (what an API model *is* and how a command becomes an HTTP request) independent of clap, reqwest, serde, and the filesystem. Dependencies point inward only: infrastructure may depend on use cases and the domain, use cases only on the domain, and the domain on nothing outside itself.

## Layers

### Domain (core)

The innermost layer, written in pure Rust with no external dependencies. It owns:

- **Entities** — `ApiModel`, `CommandGroup`, `Command`, `Param`, `HttpMethod`, `BodySpec`, plus request/response value types. Plain data with serde derives for persistence.
- **Rules** — pure functions for command naming (operationId kebab-casing, method+path fallback, collision disambiguation) and kebab-casing of parameter CLI names.
- **Ports** — traits the outer layers implement: `ApiStore` (load/save models), `SpecLoader` (fetch spec bytes), `OpenApiParser` (parse spec into a model), `HttpInvoker` (send a request, return a response).
- **Errors** — the domain error vocabulary (not found, invalid stored model, invalid spec, parameter/body errors, network, I/O).

### Application (use cases)

Depends only on the domain. Orchestrates ports into user-facing flows:

- **Learn API** — `SpecLoader` → `OpenApiParser` → `ApiStore.save`, with base-URL override handling.
- **Invoke command** — `ApiStore.load` → resolve group/command → build request from params and body → `HttpInvoker.send`.
- **Describe** — produce structured help data (groups, commands, parameters, body hints) from a model.

### Infrastructure (adapters)

Implements the ports and presentation; may depend on any inner layer:

- **CLI** — clap: a static `install` subcommand plus a runtime-built dynamic command tree (per installed API) that resolves to install/invoke actions; reads stdin and writes stdout/stderr.
- **Storage** — JSON file store under `~/.clining/` (override `CLINING_DIR`), atomic writes, distinct not-found vs. invalid-stored-model errors.
- **OpenAPI parsing** — serde structs for the OpenAPI 3.0 subset mapped into the domain model.
- **Source loading** — local file path or `http(s)://` URI → bytes.
- **HTTP** — blocking reqwest client implementing `HttpInvoker`.

### Composition root

`main.rs` wires concrete adapters into the use cases. The CLI layer is the only place that knows about argument parsing and stdout/stderr conventions.

## Dependency Rule

```text
infrastructure  →  application  →  domain
       └──────────────┴──────────────┘
              (nothing depends outward)
```

## Ports & Adapters

| Port (domain trait) | Adapter (infra) | Purpose |
| --- | --- | --- |
| `ApiStore` | JSON file store | Persist/load API models |
| `SpecLoader` | Source loader | Fetch spec bytes from path or URI |
| `OpenApiParser` | OpenAPI 3.0 serde parser | Spec JSON → `ApiModel` |
| `HttpInvoker` | Blocking reqwest client | Send built requests, return responses |

## Key Domain Rules

- Endpoints group by OpenAPI tag; untagged endpoints land in `default`.
- Command name = kebab-cased `operationId`, else `method-path-segments` joined by `-` and skipping path variables; collisions get `-2`, `-3`, … suffixes.
- Parameters expose a kebab-cased CLI name (`cli_name`) while retaining the original name for path/query substitution.
- Base URL comes from `servers[0].url` unless overridden at install.

## Storage Layout

- Root: `$CLINING_DIR` if set, else `~/.clining/`.
- One plain JSON file per API: `<name>.json`.
- Atomic replace on save; corrupt files surface as `InvalidStoredModel`, never as not-found.

## Data Flows

### Learn

`cli args → LearnApi use case → SpecLoader → OpenApiParser → ApiModel → ApiStore.save → ~/.clining/<name>.json`

### Invoke

`cli args + stdin body → InvokeCommand use case → ApiStore.load → resolve → build request → HttpInvoker.send → response → body:stdout, status+headers:stderr, exit code`
