---
type: workflow
title: "Issue #7 — CI release pipeline publishing GitHub releases with executable binaries"
description: "Adds a tag/dispatch-triggered GitHub Actions release workflow, a reproducible Makefile packaging target, and release documentation so clining binaries can be downloaded without a Rust toolchain."
tags:
  - "clining"
  - "implementation-plan"
  - "release"
  - "ci"
  - "github-actions"
timestamp: "2026-10-20T21:55:58Z"
related:
  - "[Issue #7](https://github.com/ehpalumbo/clining/issues/7)"
  - "[Phased Implementation Plan](implementation-plan.md)"
  - "[Requirements Specification](../specs/spec.md)"
  - "[Architecture Overview](../architecture/overview.md)"
---

# Issue #7 — CI release pipeline publishing GitHub releases with executable binaries

**Status:** Planned

## References

- [Issue #7: CI release pipeline publishing GitHub releases with executable binaries](https://github.com/ehpalumbo/clining/issues/7)
- [Issue #12: Windows readiness — runtime fixes](https://github.com/ehpalumbo/clining/issues/12)
- [Issue #13: Windows x64 release matrix](https://github.com/ehpalumbo/clining/issues/13)
- [Requirements Specification](../specs/spec.md)
- [Architecture Overview](../architecture/overview.md)
- [Existing CI workflow](../../.github/workflows/main.yml)
- [Makefile](../../Makefile)
- [Cargo manifest](../../Cargo.toml)
- [README build & install section](../../README.md)

## Executive Summary

Cut a release entirely from CI: a new `.github/workflows/release.yml` runs the existing quality gate (`make check`), produces the release binary (`make build`), packages it with a versioned asset name, and publishes a GitHub Release with generated notes. The tag is either pushed by a maintainer (`v*` tag) or created by the workflow from a manually dispatched version. A Release is only created after every gate passes, and an existing tag or Release aborts the run loudly.

### Architecture

The change is confined to the delivery edge — GitHub Actions configuration, a Makefile packaging target, and documentation. No domain, application, or infrastructure source changes are needed, and the onion dependency rule is unaffected. The pipeline stages are:

```text
resolve version ──> refuse existing tag/release ──> make check ──> make build
                                                                    │
                          GitHub Release <── gh release create <── make dist
                          (tag + notes + assets)
```

The release version has two representations that must agree: the git tag `vX.Y.Z` and `Cargo.toml`'s `package.version`. The workflow enforces equality before building so asset names, the tag, and `clining --version` (which clap renders from `CARGO_PKG_VERSION` at `src/infra/cli/clap_cli.rs:23`) can never disagree. Packaging is implemented once in the `Makefile` and reused by the workflow, so local verification and CI produce the same asset layout.

### Assumptions

- Both triggers from the issue are adopted: pushing a `v*` tag accepts the existing tag, while `workflow_dispatch` takes a `version` input and creates the tag at the dispatched commit.
- The version in the tag must equal `Cargo.toml`'s `package.version`. This is stricter than the issue but prevents a release whose binary reports a different version than its asset name.
- `SHA256SUMS` is included (the issue marks it a nice-to-have) because GNU `sha256sum` is already available on the runner and makes downloads verifiable.
- Only the runner's host target is built (`x86_64-unknown-linux-gnu` on `ubuntu-latest`); multi-platform matrices, crates.io publishing, signing, and package-manager taps remain out of scope. Windows x64 work is tracked separately: runtime readiness in [#12](https://github.com/ehpalumbo/clining/issues/12) and the release matrix in [#13](https://github.com/ehpalumbo/clining/issues/13).
- The `gh` CLI (preinstalled on GitHub-hosted runners) is used instead of a third-party release action, keeping the workflow vendor-free and small.
- Releases are published (not drafts). A version with a pre-release suffix (for example `v0.1.0-rc.1`) is published as a GitHub pre-release and is not marked latest; build metadata stays a normal release.
- On manual dispatch, an existing tag is a hard failure (strict reading of issue criterion 5). Recovery is a deliberate manual tag deletion, documented in the release docs page.

---

## Configuration & Environment Updates

- **Environment Variables:** No new application environment variables. The workflow consumes GitHub-provided `GITHUB_TOKEN` (as `GH_TOKEN`), `GITHUB_SHA`, `GITHUB_REF_NAME`, and the `workflow_dispatch` `version` input.
- **External Dependencies:** None added. The release job relies on the runner's preinstalled stable Rust toolchain, `gh`, GNU `tar`, and `sha256sum` — the same implicit toolchain assumption the current CI job already makes in `.github/workflows/main.yml:15-22`.

---

## Task Details

### 1. Add a reproducible release packaging target to the Makefile

- **Prerequisites / Dependencies:** None.
- **Affected Files:**
  - [Makefile](../../Makefile) - MODIFY
  - [.gitignore](../../.gitignore) - MODIFY
- **Affected Symbols:**
  - Make variables: `VERSION`, `TARGET_TRIPLE`, `DIST_DIR`, `DIST_ASSET`
  - Make targets: `dist`, `print-dist-asset`; update `.PHONY`
- **Description:**
  - Derive the version from the crate metadata so `Cargo.toml` stays the single source of truth: `VERSION ?= $(shell cargo pkgid | sed 's/.*@//')` (yields `0.1.0` today). Derive the target triple from the active toolchain: `TARGET_TRIPLE ?= $(shell rustc -vV | sed -n 's/^host: //p')` (yields `x86_64-unknown-linux-gnu` today).
  - Define `DIST_DIR = dist` and `DIST_ASSET = $(DIST_DIR)/clining-v$(VERSION)-$(TARGET_TRIPLE).tar.gz`.
  - `dist: build` — create `$(DIST_DIR)`, pack the already-built binary at the archive root with `tar -czf $(DIST_ASSET) -C target/release clining`, then write `$(DIST_DIR)/SHA256SUMS` from inside `dist/` so the checksum line names only the tarball (`sha256sum $(notdir $(DIST_ASSET)) > SHA256SUMS`). Do not put the binary or a directory prefix inside the archive: extracting yields `./clining`.
  - `print-dist-asset` — a silent target that echoes `$(DIST_ASSET)` so the workflow can consume the asset path without duplicating the naming scheme.
  - Add `dist` and `print-dist-asset` to `.PHONY`. Append `dist/` to `.gitignore`.
  - `make check` must remain unchanged and must not build the release package.
- **Acceptance Criteria:**
  - [ ] `make -s print-dist-asset` prints `dist/clining-v0.1.0-x86_64-unknown-linux-gnu.tar.gz` on the CI host.
  - [ ] `make dist` creates both `dist/clining-v0.1.0-x86_64-unknown-linux-gnu.tar.gz` and `dist/SHA256SUMS`.
  - [ ] `tar -tzf dist/clining-v0.1.0-x86_64-unknown-linux-gnu.tar.gz` lists exactly one entry, `clining`.
  - [ ] The extracted binary is executable and `./clining --version` prints `clining 0.1.0`.
  - [ ] `cd dist && sha256sum -c SHA256SUMS` reports `OK`.
  - [ ] `VERSION=1.2.3 make -s print-dist-asset` prints `dist/clining-v1.2.3-x86_64-unknown-linux-gnu.tar.gz`, proving the override works.
  - [ ] `dist/` is ignored by git (`git status --porcelain` stays clean after `make dist`).
  - [ ] `make check` still passes unchanged.

### 2. Add the tag/dispatch release workflow

- **Prerequisites / Dependencies:** Task 1 (`make dist`, `make -s print-dist-asset`).
- **Affected Files:**
  - [.github/workflows/release.yml](../../.github/workflows/release.yml) - NEW
- **Affected Symbols:**
  - Workflow: `name: Release`; triggers `on.push.tags`, `on.workflow_dispatch.inputs.version`
  - `permissions`, `concurrency`, `env`
  - Job `release` and its steps: checkout, resolve version, refuse existing release, check, build, package, publish
- **Description:**
  - Triggers and scope. The workflow must run only for `push` of `v*` tags and `workflow_dispatch`; no branch pushes, no `pull_request`:
    ```yaml
    on:
      push:
        tags: ["v*"]
      workflow_dispatch:
        inputs:
          version:
            description: "Version to release (tag v<version> is created)"
            required: true
            type: string
    ```
  - Permissions and serialization. `permissions: { contents: write }` only; no `secrets.*` anywhere. `concurrency: { group: release, cancel-in-progress: false }` prevents overlapping releases.
  - Job environment. Pass `${{ inputs.version }}`, `${{ github.event_name }}`, and `${{ github.ref_name }}` through `env` (not interpolated directly into shell) to avoid expression injection; set `GH_TOKEN: ${{ github.token }}` so `gh` authenticates without extra secrets.
  - Step 1 — checkout with `actions/checkout@v4` (`fetch-depth: 0` so tag context is complete).
  - Step 2 — resolve and validate version, with `set -euo pipefail`:
    - On `workflow_dispatch`: `VERSION="$INPUT_VERSION"`, `TAG="v$VERSION"`, and fail with `::error::` if `git ls-remote --exit-code --tags origin "refs/tags/$TAG"` finds the tag.
    - On tag push: `TAG="$REF_NAME"`, `VERSION="${TAG#v}"`.
    - Reject versions not matching `^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?$`.
    - Read `CARGO_VERSION="$(cargo pkgid | sed 's/.*@//')"` and fail with `::error::` unless it equals `$VERSION`.
    - Export `VERSION` and `TAG` via `$GITHUB_ENV`.
  - Step 3 — refuse to overwrite: `gh release view "$TAG" --json tagName` succeeds when a Release exists; print an `::error::` naming the tag and exit 1. This runs before any build or upload.
  - Step 4 — `make check` (identical gate to `main.yml`).
  - Step 5 — `make build`.
  - Step 6 — `make dist` (version is derived from `Cargo.toml`, already validated against the tag).
  - Step 7 — publish with `gh release create`, using the asset path from `make -s print-dist-asset` plus `dist/SHA256SUMS`, `--title "clining $TAG"`, and `--generate-notes`. Add `--target "$GITHUB_SHA"` on dispatch (this creates the tag at the dispatched commit) or `--verify-tag` on tag push (the tag already exists). Add `--prerelease` when the version carries a pre-release suffix. Build the extra flags as a bash array to keep quoting correct.
  - Ordering matters: on manual dispatch the tag and Release are created in the final step, so a failed check/build/package leaves no tag and no Release behind; on a tag push the tag already exists and only the Release is withheld.
- **Acceptance Criteria:**
  - [ ] `release.yml` declares only `push.tags: ["v*"]` and `workflow_dispatch`; it contains no `pull_request` or branch trigger.
  - [ ] `permissions` contains only `contents: write`; the file contains no `write-all` and no `secrets.` reference.
  - [ ] Ordinary pushes to `main` and PRs do not start the Release workflow; the existing CI workflow still runs on them.
  - [ ] Dispatch with a malformed version stops before `make check` with an `::error::` line.
  - [ ] Dispatch (or tag push) with a version that differs from `Cargo.toml` stops with an error naming both versions.
  - [ ] Dispatch with an already-existing tag stops at the resolve step; a pre-existing Release stops at the guard step.
  - [ ] The steps execute in the order resolve → guard → `make check` → `make build` → `make dist` → `gh release create`.
  - [ ] A dispatch run creates tag `v<version>` at the dispatched commit and a published Release titled `clining v<version>` with generated notes.
  - [ ] The Release carries `clining-v<version>-<target-triple>.tar.gz` and `SHA256SUMS`.
  - [ ] The publish step uses `--generate-notes`, `--target "$GITHUB_SHA"` for dispatch, and `--verify-tag` for tag pushes.
  - [ ] `concurrency.cancel-in-progress` is `false`.

### 3. Add a regression test for the release workflow contract

- **Prerequisites / Dependencies:** Task 2 (the workflow file must exist).
- **Affected Files:**
  - [tests/release_workflow.rs](../../tests/release_workflow.rs) - NEW
- **Affected Symbols:**
  - Test functions: `release_workflow_triggers_only_on_tags_and_dispatch`, `release_workflow_scopes_permissions_and_uses_no_secrets`, `release_workflow_runs_gates_before_publishing`, `release_workflow_refuses_existing_tag_or_release`, `release_workflow_publishes_notes_and_packaged_assets`
  - Constant: `WORKFLOW` via `include_str!("../.github/workflows/release.yml")`
- **Description:**
  - Add a std-only integration test that loads the workflow at compile time and asserts the release contract that is easy to regress when editing YAML:
    - Triggers: contains `workflow_dispatch:` and a `push` `tags:` filter; contains neither `pull_request` nor a branch-only trigger.
    - Permissions: contains `contents: write`, and contains neither `write-all` nor `secrets.`.
    - Gate ordering: the byte offsets of `make check`, `make build`, `make dist`, and `gh release create` are strictly increasing.
    - Overwrite protection: contains both the `git ls-remote` tag guard and the `gh release view` guard, and both exit non-zero (`::error::`).
    - Release contents: contains `--generate-notes`, `print-dist-asset`, and `dist/SHA256SUMS`.
    - Serialization: contains `cancel-in-progress: false`.
  - Keep assertions on stable, intentional substrings rather than full YAML structure; no new dev-dependency is introduced.
- **Acceptance Criteria:**
  - [ ] `cargo test --test release_workflow` passes.
  - [ ] Removing `--generate-notes`, either overwrite guard, or the `contents: write` permission makes the suite fail.
  - [ ] The test file is formatted and clippy-clean under `make check`.

### 4. Document binary installation and the release process

- **Prerequisites / Dependencies:** Tasks 1–3 (document the final commands and asset names).
- **Affected Files:**
  - [README.md](../../README.md) - MODIFY
  - [docs/workflows/releases.md](../workflows/releases.md) - NEW
  - [docs/index.md](../index.md) - MODIFY
- **Affected Symbols:**
  - README sections: `Build & Install` (add prebuilt binaries), new `Releasing`
  - Docs page metadata: `type: workflow`, `title`, `description`, tags, `timestamp`, `related`
  - `docs/index.md`: new `Workflows` section and an entry for this plan under `Plans`
- **Description:**
  - README `Build & Install`: state that building from source requires a Rust toolchain, then add a "Prebuilt binaries" subsection showing `gh release download v<version> -p 'clining-v*.tar.gz' -p SHA256SUMS`, `sha256sum -c SHA256SUMS`, `tar -xzf …`, and installing the extracted `clining` (e.g. `install -m 0755 clining ~/.local/bin/clining`).
  - README: add a short "Releasing" section stating that `Cargo.toml` must be bumped (and `Cargo.lock` refreshed by the next `cargo` command) and committed first, then either run the `Release` workflow with the version or push a matching `v<version>` tag; link to `docs/workflows/releases.md` for details.
  - `docs/workflows/releases.md` (OKF `type: workflow`): explain the two triggers, why the quality gate precedes packaging, the asset contract (`clining-v<version>-<target-triple>.tar.gz` containing `clining`, plus `SHA256SUMS`), the version/tag equality rule, minimal permissions (`contents: write`, `GITHUB_TOKEN` only), and failure/recovery semantics — existing tag or Release aborts; to retry, delete the failed Release/tag deliberately, fix the cause, and re-run. Do not duplicate the workflow YAML.
  - `docs/index.md`: add a `Workflows` section linking `workflows/releases.md` with a one-sentence summary, and list this plan under `Plans`.
- **Acceptance Criteria:**
  - [ ] README's install instructions let a user with no Rust toolchain download, verify, extract, and install the binary.
  - [ ] README documents the tag/version bump prerequisite (including `Cargo.lock`) and both release trigger paths, with a link to the release docs page.
  - [ ] `docs/workflows/releases.md` starts with valid OKF frontmatter (`type`, `title`, `description`, 2–5 tags, `timestamp`, `related`) and covers triggers, gates, asset naming, permissions, and failure/recovery.
  - [ ] `docs/index.md` links to `workflows/releases.md` and `plans/issue-7-release-pipeline.md`, and both links resolve on disk.
  - [ ] No documentation claims installing always requires compiling from source.

---

## Verification Plan (Whole Feature)

### Automated Tests

| Check | Command | Verifies |
| --- | --- | --- |
| Format, lint, unit + integration tests | `make check` | Existing suite plus Task 3; no regressions |
| Release workflow contract | `cargo test --test release_workflow` | Triggers, permissions, gate order, overwrite guards, asset publishing |
| Workflow YAML parses (optional, requires PyYAML) | `python3 -c 'import yaml; yaml.safe_load(open(".github/workflows/release.yml"))'` | No malformed YAML |
| Workflow static analysis (optional, requires Docker/network) | `docker run --rm -v "$PWD:/repo" -w /repo rhysd/actionlint:latest -color` | Action/expression/shell issues beyond parsing |
| Asset name single source | `make -s print-dist-asset` | Expected `dist/clining-v<version>-<target-triple>.tar.gz` |
| Archive contents | `tar -tzf "$(make -s print-dist-asset)"` | Exactly `clining` at the archive root |
| Checksum | `cd dist && sha256sum -c SHA256SUMS` | Checksum matches the tarball |
| Binary works from package | `tmp=$(mktemp -d); tar -xzf "$(make -s print-dist-asset)" -C "$tmp"; "$tmp/clining" --version` | Extracted binary runs and reports `clining <version>` |

### Manual Verification Steps

1. Merge the change to `main` (keep `Cargo.toml` at `0.1.0` for the first release).
2. Run **Actions → Release → Run workflow** on `main` with `version = 0.1.0`.
3. Confirm the run executes resolve → guard → `make check` → `make build` → `make dist` → publish, and is green.
4. Confirm tag `v0.1.0` points at the dispatched commit and Release `clining v0.1.0` is published with generated notes.
5. Confirm assets `clining-v0.1.0-x86_64-unknown-linux-gnu.tar.gz` and `SHA256SUMS` are attached.
6. Download and verify the asset with `gh release download v0.1.0`, `sha256sum -c SHA256SUMS`, extract, and run `./clining --version`; expect `clining 0.1.0`.
7. Re-run the Release workflow with `version = 0.1.0`; confirm it fails at the tag guard and that the Release and its assets are unchanged.
8. Dispatch with `version = 0.2.0` (without bumping `Cargo.toml`); confirm it fails the version-equality check before `make check`.
9. Push an ordinary commit to `main` and open a PR; confirm the Release workflow does not run while CI still does.

### Definition of Done (DoD)

- [ ] A Release can be produced entirely from CI by tag push or manual dispatch, with no local build.
- [ ] `make check` and `make build` run before any tag or Release is created.
- [ ] Release assets are named `clining-v<version>-<target-triple>.tar.gz`, contain the `clining` binary, and are accompanied by `SHA256SUMS`.
- [ ] Release notes are generated and the Release is published.
- [ ] An existing tag (dispatch) or Release (either trigger) aborts loudly without overwriting assets.
- [ ] Workflow permissions are limited to `contents: write`; no secrets beyond `GITHUB_TOKEN`.
- [ ] Release workflow triggers only on `v*` tags and `workflow_dispatch`; ordinary pushes/PRs are unaffected.
- [ ] `make check` passes (fmt, clippy with `-D warnings`, all tests) including the new release workflow test.
- [ ] README and `docs/workflows/releases.md` document installation and the release process; `docs/index.md` links resolve.
- [ ] No application source or runtime dependency changes; onion layering is untouched.
