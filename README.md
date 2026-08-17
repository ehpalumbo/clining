# Clining: Expose OpenAPI Specs as Local CLIs

## Scope & Purpose

`clining` is a command line utility that makes OpenAPI-documented HTTP APIs available to agents as a local CLI. Given an OpenAPI 3.0 spec, it learns the API's shape, stores a model locally under `~/.clining/`, and exposes endpoints as command groups and commands.

> **Note:** This is a work-in-progress project. The current implementation is a proof-of-concept and may not be fully functional or stable. Please refer to the [Phased Implementation Plan](docs/plans/implementation-plan.md) for details on the current state and future plans.

## Usage & Integration

Implemented in Rust, targeting Linux for the time being. Intended usage:

```bash
# setup an API, one-off
clining install <my-api-name> <path-or-uri-to-openapi-json-file>

# API calls as CLI commands
clining <my-api-name> <command-group> <command> --<param> <value> < <body-file-via-stdin>

# discover commands and how to invoke them
clining <my-api-name> --help
clining <my-api-name> <command-group> --help
clining <my-api-name> <command-group> <command> --help
```

Environment: `CLINING_DIR` overrides the default model store root (`~/.clining/`).

## Documentation

Please refer to the [Repository Docs Index](docs/index.md) for further details.
