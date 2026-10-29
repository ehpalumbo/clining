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

Platform target: **Linux and Windows (x86_64)**, implemented in **Rust**, organized with **onion architecture** (inward-only dependencies).

## Usage Contract

| Command | Meaning |
| --- | --- |
| `clining api install <name> <path-or-uri> [--base-url <url>] [--token-env <VAR> \| --token-file <path>]` | One-off setup: fetch the spec, parse it, persist the API model with an optional Bearer token reference. |
| `clining <name> <group> <command> [--param value]... [-H <header>]... [< body.json]` | Invoke an endpoint, optionally adding custom request headers. |
| `clining <name> --help` | List command groups for the API. |
| `clining <name> <group> --help` | List commands in the group. |
| `clining <name> <group> <command> --help` | Show full input/output documentation: request line, parameter shapes, request body walkthrough, and response schemas. |

## Functional Requirements

### FR-1 Install

- FR-1.1 `clining api install` accepts a spec **source** that is a local file path or an `http(s)://` URI. Install lives only under the `api` management group; there is no top-level install subcommand.
- FR-1.2 The spec is parsed as **OpenAPI 3.0.x** JSON. Unsupported versions are rejected with a clear error.
- FR-1.3 The parsed model is persisted locally (see FR-5). Re-installing an existing name overwrites it.
- FR-1.4 `--base-url` overrides the base URL taken from `servers[0].url` and is stored in the model.
- FR-1.5 Success prints a confirmation with the number of command groups and commands.
- FR-1.6 `--token-env <VAR>` or `--token-file <path>` (mutually exclusive) records where the Bearer token lives. Only the reference is persisted, never the secret. Relative `--token-file` paths are stored as absolute at install time. The source must be resolvable at install (fail-fast, nothing persisted) and is re-resolved at every invocation. Re-installing without either flag clears auth.

### FR-2 Model & Command Naming

- FR-2.1 Endpoints are grouped by OpenAPI **tag**; endpoints without a tag go to a `default` group.
- FR-2.2 Command name = `operationId` converted to **kebab-case** when present.
- FR-2.3 Fallback command name = HTTP method + path segments (skipping `{path variables}`) joined by `-` (e.g. `GET /pets/{petId}` → `get-pets`).
- FR-2.4 Duplicate command names within a group are disambiguated with numeric suffixes (`-2`, `-3`, …).
- FR-2.5 Parameter CLI names are **kebab-cased** for the command line (e.g. `petStatus` → `--pet-status`); the original name is retained in the model so path/query substitution stays correct.
- FR-2.6 The stored model captures base URL, groups, commands, HTTP method, path template, path/query parameters (original name, CLI name, required flag, and value schema where the spec provides one), the request body spec (content type, required flag, typed schema), and response specs (status code, content type, typed schema).
- FR-2.7 The install name must not be the reserved static command keyword `api` (exact match); `API`, `apix`, and `api-2` remain valid names.

### FR-3 Invocation

- FR-3.1 Path and query parameters are passed as named arguments: `--<param> <value>`. Path parameters are required.
- FR-3.2 Repeating a parameter is allowed (e.g. `--tag a --tag b`) to represent repeated/array query values.
- FR-3.3 The request body is read from **stdin**; the response body is written to **stdout** byte-exact (binary-safe, no trailing-newline mangling).
- FR-3.4 The HTTP status line and response headers are written to **stderr**.
- FR-3.5 Exit code is `0` for 2xx responses and `1` for any non-2xx response.
- FR-3.6 Validation is **presence-only**: required path/query params and a required body must be supplied; a body supplied for an endpoint that declares none is rejected. Schemas are stored and shown in help but not enforced in v0.
- FR-3.7 When the model carries an auth reference, the token is resolved at invocation time (env var verbatim; file with trailing newline trimmed) and sent as `Authorization: Bearer <token>` on every request. Unresolvable sources fail with `error:` on stderr and exit non-zero without sending the request. The token value never appears in the model file, logs, help, or stderr.
- FR-3.8 Every endpoint command accepts repeatable custom request headers as `-H <header>` / `--clining-header <header>`. Each occurrence is a `Name: value` pair split at the first colon; optional HTTP whitespace (spaces and tabs) is trimmed from both ends. Headers override generated `User-Agent`, `Accept`, `Content-Type`, and `Authorization` headers case-insensitively and are sent as separate header lines in input order, so repeating a name yields repeated lines. A custom `Authorization` suppresses generated Bearer auth and skips token resolution entirely. Header names must be RFC 9110 tokens; values accept only tab and printable ASCII (`0x20`–`0x7E`); framing and hop-by-hop names (`Content-Length`, `Transfer-Encoding`, `Connection`, `Keep-Alive`, `Proxy-Connection`, `TE`, `Trailer`, `Upgrade`, `Proxy-Authenticate`, `Proxy-Authorization`) are rejected. Custom headers are command-scoped. If an API path or query parameter canonicalizes to `clining-header`, the parameter keeps the long form and the custom-header flag is available as `-H` only.

### FR-4 Discovery

- FR-4.1 `--help` renders at every level: API (groups), group (commands), and command (complete input/output documentation).
- FR-4.2 Command help shows the summary, the operation description when the spec provides one, the full request line (`METHOD path`), and every parameter's CLI flag, required/optional status, description, and value shape where the spec provides one (type, enum values, examples, and constraints).
- FR-4.3 Command help renders the request body as a field-by-field walkthrough: content type, required/optional status, then each field's name, type, required flag, description, enum, and examples, including nested objects, arrays, and composite schemas. Local `$ref`s are shown by their resolved component name and shape, never as raw references or an unlabeled JSON blob; recursive schemas are marked at the cycle point.
- FR-4.4 Command help documents responses per status code and content type, with the response description when the spec provides one and the same schema walkthrough so an agent can parse or validate the result. Responses without a body still list their status code.
- FR-4.5 Help is read-only: rendering it never resolves auth, reads stdin, or sends an HTTP request.
- FR-4.6 Command help is the single documentation surface; there is no separate long-form docs mode.

### FR-5 Local Storage

- FR-5.1 Model files are plain JSON under `~/.clining/<name>.json` (or `%USERPROFILE%\.clining\<name>.json` on Windows); the root is overridable via the `CLINING_DIR` environment variable.
- FR-5.2 Writes are atomic (temp file + rename) so no partial file is left behind.
- FR-5.3 A missing model is reported as **not found**; a present but unparseable/corrupt model is reported as a distinct **invalid stored model** error (with file path and parse reason) — never masked as not found.

## Non-Functional Requirements

- NFR-1 Target platforms: Linux and Windows (x86_64).
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
| Invalid header | Malformed `Name: value`, invalid/denylisted name, or non-ASCII/control value | stderr `error: invalid header: ...`; exit 1; request not sent |
| Auth errors | Token source unresolvable (var unset/empty, file missing/unreadable/empty) | stderr; exit 1; request not sent (install persists nothing) |
| Usage errors | Mutually exclusive flags supplied together (e.g. both `--token-env` and `--token-file`), a missing flag value, or an unknown flag | clap usage error on stderr; exit 2; request not sent (install persists nothing) |
| Network/I-O errors | Fetch, HTTP, or filesystem failure | stderr with context; exit 1 |

All errors go to stderr as a one-line `error: <message>`; all failure paths exit non-zero.

## Exit Codes & Streams Contract

| Response / outcome | stdout | stderr | exit |
| --- | --- | --- | --- |
| 2xx | response body (byte-exact) | status line + headers | 0 |
| non-2xx | response body (byte-exact) | status line + headers | 1 |
| CLI/spec/model/network error | — | `error: <message>` | 1 |
| CLI usage error (e.g. conflicting flags) | — | clap usage message | 2 |

## Out of Scope (v0)

- JSON Schema validation of request bodies and parameter values (presence-only only).
- Persistent non-Bearer authentication and credential management. Ad-hoc custom headers via `-H` are supported, but header files (`-H @file`), environment-provided headers, install-time headers, API/group header inheritance, and a `--config` file are not.
- Auth beyond Bearer tokens via env var or token file (no API keys, OAuth flows, etc.). A caller-supplied `Authorization` header can override the generated Bearer header per invocation, but it is not persisted.
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
