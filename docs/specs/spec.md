---
type: specification
title: "clining v0 — Requirements Specification"
description: "Functional and non-functional requirements for clining v0: installing OpenAPI-described HTTP APIs as discoverable, agent-friendly local CLI commands."
tags:
  - "clining"
  - "requirements"
  - "openapi"
  - "cli"
  - "agents"
timestamp: "2026-08-17T19:47:50Z"
related:
  - "[Architecture Overview](../architecture/overview.md)"
  - "[Phased Implementation Plan](../plans/implementation-plan.md)"
---

# Requirements Specification — clining v0

## Goal

`clining` makes an OpenAPI-documented HTTP API available to agents as a local CLI. Given an OpenAPI spec, it "learns" the API shape, persists a model locally, and exposes the API's endpoints as commands. A caller invokes commands with named parameters and a body on stdin; the API's response body goes to stdout and its status line plus headers go to stderr. The CLI must be self-discoverable through `--help` at every level.

Platform target: **Linux only**, implemented in **Rust**, organized with **onion architecture** (inward-only dependencies).

## Usage Contract

| Command | Meaning |
| --- | --- |
| `clining install <name> <path-or-uri> [--base-url <url>]` | One-off setup: fetch the spec, parse it, persist the API model. |
| `clining <name> <group> <command> [--param value]... [< body.json]` | Invoke an endpoint. |
| `clining <name> --help` | List command groups for the API. |
| `clining <name> <group> --help` | List commands in the group. |
| `clining <name> <group> <command> --help` | Show parameters and the expected request body. |

## Functional Requirements

### FR-1 Install

- FR-1.1 `clining install` accepts a spec **source** that is a local file path or an `http(s)://` URI.
- FR-1.2 The spec is parsed as **OpenAPI 3.0.x** JSON. Unsupported versions are rejected with a clear error.
- FR-1.3 The parsed model is persisted locally (see FR-5). Re-installing an existing name overwrites it.
- FR-1.4 `--base-url` overrides the base URL taken from `servers[0].url` and is stored in the model.
- FR-1.5 Success prints a confirmation with the number of command groups and commands.

### FR-2 Model & Command Naming

- FR-2.1 Endpoints are grouped by OpenAPI **tag**; endpoints without a tag go to a `default` group.
- FR-2.2 Command name = `operationId` converted to **kebab-case** when present.
- FR-2.3 Fallback command name = HTTP method + path segments (skipping `{path variables}`) joined by `-` (e.g. `GET /pets/{petId}` → `get-pets`).
- FR-2.4 Duplicate command names within a group are disambiguated with numeric suffixes (`-2`, `-3`, …).
- FR-2.5 Parameter CLI names are **kebab-cased** for the command line (e.g. `petStatus` → `--pet-status`); the original name is retained in the model so path/query substitution stays correct.
- FR-2.6 The stored model captures base URL, groups, commands, HTTP method, path template, path/query parameters (original name, CLI name, required flag), and the request body spec (content type, required flag, raw JSON schema).

### FR-3 Invocation

- FR-3.1 Path and query parameters are passed as named arguments: `--<param> <value>`. Path parameters are required.
- FR-3.2 Repeating a parameter is allowed (e.g. `--tag a --tag b`) to represent repeated/array query values.
- FR-3.3 The request body is read from **stdin**; the response body is written to **stdout** byte-exact (binary-safe, no trailing-newline mangling).
- FR-3.4 The HTTP status line and response headers are written to **stderr**.
- FR-3.5 Exit code is `0` for 2xx responses and `1` for any non-2xx response.
- FR-3.6 Validation is **presence-only**: required path/query params and a required body must be supplied; a body supplied for an endpoint that declares none is rejected. Schemas are stored and shown in help but not enforced in v0.

### FR-4 Discovery

- FR-4.1 `--help` renders at every level: API (groups), group (commands), command (parameters + body schema hint).
- FR-4.2 Help shows command summaries, parameter CLI names with required flags, and the request body content type, requiredness, and a schema summary.

### FR-5 Local Storage

- FR-5.1 Model files are plain JSON under `~/.clining/<name>.json`; the root is overridable via the `CLINING_DIR` environment variable.
- FR-5.2 Writes are atomic (temp file + rename) so no partial file is left behind.
- FR-5.3 A missing model is reported as **not found**; a present but unparseable/corrupt model is reported as a distinct **invalid stored model** error (with file path and parse reason) — never masked as not found.

## Non-Functional Requirements

- NFR-1 Target platform: Linux only.
- NFR-2 Language: Rust, `edition 2024`.
- NFR-3 Onion architecture: concentric layers with inward-only dependencies; CLI, storage, HTTP, and spec parsing are infrastructure adapters; domain and use cases stay free of infrastructure concerns.
- NFR-4 Presence-only input validation in v0 (see FR-3.6).
- NFR-5 One-shot execution: a single request per invocation, no long-running server.

## Error Model

| Error | Meaning | User-visible behavior |
| --- | --- | --- |
| Not found | No installed API model under the given name | stderr `error: ...`; exit 1 |
| Invalid stored model | Model file exists but is unparseable/corrupt | stderr with file path + reason; exit 1 |
| Invalid spec | Spec source missing/unreadable or not OpenAPI 3.0.x | stderr; exit 1; nothing persisted |
| Parameter/body errors | Required param/body missing, or body supplied where none declared | stderr; exit 1; request not sent |
| Network/I-O errors | Fetch, HTTP, or filesystem failure | stderr with context; exit 1 |

All errors go to stderr as a one-line `error: <message>`; all failure paths exit non-zero.

## Exit Codes & Streams Contract

| Response / outcome | stdout | stderr | exit |
| --- | --- | --- | --- |
| 2xx | response body (byte-exact) | status line + headers | 0 |
| non-2xx | response body (byte-exact) | status line + headers | 1 |
| CLI/spec/model/network error | — | `error: <message>` | 1 |

## Out of Scope (v0)

- JSON Schema validation of request bodies and parameter values (presence-only only).
- Authentication/authorization handling (API keys, OAuth, etc.).
- OpenAPI 3.1 and 2.0 (Swagger) support.
- Async/concurrent execution.
- Convenience features such as `clining list`, command aliases, and shell tab-completion.

## Traceability to Phases

| Requirement | Phase |
| --- | --- |
| FR-1 Install | Phase 2 |
| FR-2 Model & Command Naming | Phase 2 |
| FR-3 Invocation | Phase 3 |
| FR-4 Discovery | Phase 4 |
| FR-5 Local Storage | Phase 2 (store), Phase 4 (hardening) |
| NFR-1..3 Toolchain & architecture | Phase 1 |
| Error model / exit codes | Phases 2–4 |
