# Clining: Expose OpenAPI Specs as Local CLIs

## Scope & Purpose

`clining` is a command line utility that makes OpenAPI-documented HTTP APIs available to agents as a local CLI. Given an OpenAPI 3.0 spec, it learns the API's shape, stores a model locally under `~/.clining/`, and exposes endpoints as command groups and commands.

## Build & Install

Requires a Rust toolchain (`rustup`; `edition 2024`). From a checkout:

```bash
make check          # fmt --check + clippy -D warnings + tests
make build          # release build into target/release/clining
make install        # copies the binary to ~/.local/bin/clining (PREFIX overridable)
```

## Usage

### Install an API (one-off setup)

```bash
clining install <my-api-name> <path-or-uri-to-openapi-json-file> [--base-url <url>]
```

The spec source may be a local file path or an `http(s)://` URI. `--base-url` overrides the URL taken from `servers[0].url`. Re-installing an existing name overwrites the stored model.

### Invoke endpoints as commands

```bash
clining <my-api-name> <command-group> <command> --<param> <value> [--<param> <value>]... [< body-file]
```

- Path parameters are required; query parameters are optional unless marked required.
- Repeating a parameter is allowed and produces repeated query keys (`--tag a --tag b` → `?tag=a&tag=b`).
- The request body is read from stdin when the operation declares one.
- The response body is written to stdout byte-exact; the HTTP status line and response headers go to stderr.
- Exit code is `0` for 2xx responses and `1` for any non-2xx response. All errors print a one-line `error: ...` to stderr and exit non-zero.

### Discover commands

```bash
clining <my-api-name> --help                      # list command groups
clining <my-api-name> <command-group> --help      # list commands in the group
clining <my-api-name> <command-group> <command> --help  # params + body schema hint
```

## Storage Layout

Models are plain JSON files, one per API:

- Root: `$CLINING_DIR` if set, otherwise `~/.clining/`.
- File: `<name>.json`.
- Writes are atomic (temp file + rename); a corrupt file surfaces as an "invalid stored model" error, never as not-found.

## Stream & Exit-Code Contract

| Response / outcome | stdout | stderr | exit |
| --- | --- | --- | --- |
| 2xx | response body (byte-exact) | status line + headers | 0 |
| non-2xx | response body (byte-exact) | status line + headers | 1 |
| CLI/spec/model/network error | — | `error: <message>` | 1 |

## Documentation

Please refer to the [Repository Docs Index](docs/index.md) for the [requirements specification](docs/specs/spec.md), [architecture overview](docs/architecture/overview.md), and the [phased implementation plan](docs/plans/implementation-plan.md).