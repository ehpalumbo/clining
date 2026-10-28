# Clining: Expose OpenAPI Specs as Local CLIs

## Scope & Purpose

`clining` is a command line utility that makes OpenAPI-documented HTTP APIs available to agents as a local CLI. Given an OpenAPI 3.0 spec, it learns the API's shape, stores a model locally under `~/.clining/`, and exposes endpoints as command groups and commands.

## Build & Install

### Prebuilt binaries

Each release publishes a Linux x86_64 tarball, so installing does not require a Rust toolchain. Replace `<version>` with the release you want (for example `0.1.0`):

```bash
gh release download v<version> --repo ehpalumbo/clining -p 'clining-v*.tar.gz'
tar -xzf clining-v<version>-x86_64-unknown-linux-gnu.tar.gz
install -Dm755 clining ~/.local/bin/clining
```

The archive contains only the `clining` binary at its root. GitHub publishes a SHA-256 digest for each release asset; view it with `gh release view v<version> --repo ehpalumbo/clining --json assets`.

### From source

Requires a Rust toolchain (`rustup`; `edition 2024`). From a checkout:

```bash
make check          # fmt --check + clippy -D warnings + tests
make build          # release build into target/release/clining
make install        # copies the binary to ~/.local/bin/clining (PREFIX overridable)
```

`make dist` additionally requires `jq`: the Makefile reads the crate version through `cargo metadata`.

## Releasing

1. Bump `package.version` in `Cargo.toml`, let the next `cargo` command refresh `Cargo.lock`, and commit both.
2. Run **Actions → Release → Run workflow** on the target commit with that version, or push a matching `v<version>` tag.

The tag and the `Cargo.toml` version must be identical; the workflow refuses to publish otherwise. A version with a pre-release suffix such as `0.1.0-rc.1` is published as a GitHub pre-release. See the [Release Process](docs/workflows/releases.md) for the pipeline, assets, and failure recovery.

## Usage

### Install an API (one-off setup)

```bash
clining install <my-api-name> <path-or-uri-to-openapi-json-file> [--base-url <url>] [--token-env <VAR> | --token-file <path>]
```

The spec source may be a local file path or an `http(s)://` URI. `--base-url` overrides the URL taken from `servers[0].url`. `--token-env` / `--token-file` (mutually exclusive) record where the Bearer token lives; only the reference is stored and the token is resolved at every invocation as `Authorization: Bearer <token>`. Relative `--token-file` paths are stored as absolute at install time. Re-installing an existing name overwrites the stored model (omitting both token flags clears auth).

### Invoke endpoints as commands

```bash
clining <my-api-name> <command-group> <command> --<param> <value> [--<param> <value>]... [-H "Name: value"]... [< body-file]
```

- Path parameters are required; query parameters are optional unless marked required.
- Repeating a parameter is allowed and produces repeated query keys (`--tag a --tag b` → `?tag=a&tag=b`).
- The request body is read from stdin when the operation declares one.
- The response body is written to stdout byte-exact; the HTTP status line and response headers go to stderr.
- Exit code is `0` for 2xx responses and `1` for any non-2xx response. All errors print a one-line `error: ...` to stderr and exit non-zero.

#### Custom request headers

Every endpoint command accepts `-H "Name: value"` or `--clining-header "Name: value"`, repeatable once per header:

```bash
clining pets pets get-pet --pet-id 42 \
  -H "X-Request-Id: abc-123" \
  -H "Accept: application/xml"
```

- The pair splits at the first `:`; surrounding spaces and tabs are trimmed. Empty values are allowed (`-H "X-Empty:"`).
- A custom header overrides the generated `User-Agent`, `Accept`, `Content-Type`, or `Authorization` header case-insensitively.
- Repeating the same name sends one header line per occurrence, in input order (no comma joining, no last-wins collapse).
- A custom `Authorization` replaces the stored Bearer token and skips token resolution, so it works even when the configured token source is unset or unreadable.
- Header names must be HTTP tokens. Values may contain only tab and printable ASCII; non-ASCII and control characters (including CR/LF) are rejected.
- Framing and hop-by-hop headers are rejected: `Content-Length`, `Transfer-Encoding`, `Connection`, `Keep-Alive`, `Proxy-Connection`, `TE`, `Trailer`, `Upgrade`, `Proxy-Authenticate`, and `Proxy-Authorization`.
- Invalid headers fail with one `error:` line and exit `1` before stdin is read or a request is sent. A missing flag value or unknown flag remains a clap usage error with exit `2`.
- Headers are command-scoped. There is no config file, header file, environment header, install-time header, or API/group inheritance in v0.
- If an API path or query parameter itself canonicalizes to `--clining-header`, that parameter keeps the long form and the custom-header flag falls back to `-H` only. An API parameter canonicalized to `--header` is unaffected.

Header values appear in the process argument list, so they may be visible in process listings and shell history. Prefer referencing secrets from the stored Bearer token source when possible; passing literal secrets with `-H` is at your own risk.

### Discover commands

```bash
clining <my-api-name> --help                      # list command groups
clining <my-api-name> <command-group> --help      # list commands in the group
clining <my-api-name> <command-group> <command> --help  # full input/output documentation
```

Command-level `--help` is the agent-facing documentation surface. It renders the
summary and request line, each parameter's value shape (type, enum values,
examples, and constraints), a field-by-field walkthrough of the request body,
and the documented response status codes, content types, and schemas. Local
`$ref`s are shown as their resolved component names and shapes; recursive
schemas are marked rather than expanded forever. Help never sends an HTTP
request.

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