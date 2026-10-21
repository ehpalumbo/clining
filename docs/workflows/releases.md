---
type: workflow
title: "Release Process"
description: "How clining releases are produced entirely in CI: triggers, quality gates, asset naming, version/tag equality, minimal permissions, and failure recovery."
tags:
  - "clining"
  - "release"
  - "ci"
  - "github-actions"
timestamp: "2026-10-21T00:00:00Z"
related:
  - "[Issue #7: CI release pipeline](https://github.com/ehpalumbo/clining/issues/7)"
  - "[Release Workflow](../../.github/workflows/release.yml)"
  - "[Makefile](../../Makefile)"
  - "[README Build & Install](../../README.md)"
---

# Release Process

Releases are produced only by CI. A maintainer never builds, tags, or uploads a release by hand; the [release workflow](../../.github/workflows/release.yml) runs the quality gate, packages the binary, and publishes a GitHub Release with generated notes.

## Triggers

The workflow runs in exactly two cases and never for ordinary branch pushes or pull requests:

- **Tag push** — pushing an existing `v<version>` tag accepts that tag. This is the release path once the version bump is merged.
- **Manual dispatch** — run **Actions → Release → Run workflow** with a `version` input. The workflow creates the tag `v<version>` at the dispatched commit.

Do not mix the paths for the same version. The workflow does not move or recreate tags.

## Version and tag equality

The crate version in `Cargo.toml` is the single source of truth. Before anything is built, the workflow requires the tag (or dispatch input) `v<version>` to equal `package.version` exactly; otherwise it fails with an error naming both values. This keeps the tag, the asset name, and `clining --version` from disagreeing.

To release:

1. Bump `package.version` in `Cargo.toml`, let the next `cargo` command refresh `Cargo.lock`, and commit both.
2. Either push the matching `v<version>` tag or dispatch the workflow with that version.

## Pipeline order

The stages run in this order. On manual dispatch, every stage must pass before the tag and Release exist; on a tag push, the tag already exists and every stage must pass before the Release is published.

```text
resolve version ──> refuse existing tag/release ──> make check ──> make build
                                                                    │
                          GitHub Release <── gh release create <── make dist
                          (tag + notes + assets)
```

- `make check` is the same gate as CI: formatting, clippy with `-D warnings`, and all tests.
- `make build` produces the release binary.
- `make dist` packages the binary and checksums. The workflow reuses the Makefile target rather than duplicating the layout, so local verification matches CI.

A failure in any stage leaves no Release behind. On manual dispatch it also leaves no tag; on a tag push the tag remains, so delete and recreate it deliberately if you need to retry (see Failure and recovery).

## Asset contract

Each Release carries:

- `clining-v<version>-<target-triple>.tar.gz` — a gzip tarball whose only entry is the `clining` binary at the archive root. Extracting it yields `./clining`.
- `SHA256SUMS` — a checksum line naming just the tarball, verifiable with `sha256sum -c SHA256SUMS`.

Only the runner's host target is built (`x86_64-unknown-linux-gnu` on `ubuntu-latest`). Multi-platform matrices, crates.io publishing, signing, and package-manager taps are out of scope.

## Permissions

The workflow requests only `contents: write`, which is what `gh release create` needs. It authenticates with the repository-provided `GITHUB_TOKEN` (exposed as `GH_TOKEN`) and uses no other secrets. Releases publish immediately; there is no draft mode. A version with a pre-release suffix (for example `v0.1.0-rc.1`) is published as a GitHub pre-release and is not marked latest, while build metadata such as `v0.1.0+build.1` publishes as a normal release.

## Failure and recovery

The workflow refuses to overwrite existing artifacts:

- On manual dispatch, an existing tag stops the run at version resolution.
- On either trigger, an existing Release stops the run before the quality gate.

To retry, first fix the cause. If the previous attempt actually created the tag or Release, delete it deliberately in GitHub, then re-run the workflow (or push the tag again). Never delete a published Release merely to retag a different commit; bump the version instead.

## Verifying a release locally

```bash
gh release download v<version> -p 'clining-v*.tar.gz' -p SHA256SUMS
sha256sum -c SHA256SUMS
tar -xzf clining-v<version>-x86_64-unknown-linux-gnu.tar.gz
./clining --version
```

The reported version must match the tag and asset name.
