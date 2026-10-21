//! Regression tests for the release workflow contract (issue #7): triggers,
//! permissions, gate ordering, overwrite guards, and published assets. These
//! assert stable, intentional substrings so YAML edits that weaken the
//! contract fail the suite.

const WORKFLOW: &str = include_str!("../.github/workflows/release.yml");

fn offset(needle: &str) -> usize {
    WORKFLOW
        .find(needle)
        .unwrap_or_else(|| panic!("release workflow must contain {needle:?}"))
}

#[test]
fn release_workflow_triggers_only_on_tags_and_dispatch() {
    assert!(WORKFLOW.contains("workflow_dispatch:"));
    assert!(WORKFLOW.contains("tags: [\"v*\"]"));
    assert!(!WORKFLOW.contains("pull_request"));
    assert!(
        !WORKFLOW.contains("branches:"),
        "release workflow must not run on branch pushes"
    );
}

#[test]
fn release_workflow_scopes_permissions_and_uses_no_secrets() {
    assert!(WORKFLOW.contains("contents: write"));
    assert!(!WORKFLOW.contains("write-all"));
    assert!(!WORKFLOW.contains("secrets."));
}

#[test]
fn release_workflow_runs_gates_before_publishing() {
    let check = offset("make check");
    let build = offset("make build");
    let dist = offset("make dist");
    let publish = offset("gh release create");

    assert!(check < build, "`make check` must run before `make build`");
    assert!(build < dist, "`make build` must run before `make dist`");
    assert!(dist < publish, "`make dist` must run before publishing");
}

fn between(start: &str, end: &str) -> &'static str {
    let start = offset(start);
    let end = offset(end);
    assert!(start < end, "{start:?} must precede {end:?}");
    &WORKFLOW[start..end]
}

#[test]
fn release_workflow_refuses_existing_tag_or_release() {
    let tag_guard = offset("git ls-remote --exit-code --tags");
    let release_guard = offset("gh release view");
    assert!(
        tag_guard < release_guard,
        "the existing-tag guard must run before the existing-release guard"
    );

    let tag_block = between("if git ls-remote --exit-code --tags", "else");
    assert!(
        tag_block.contains("::error::Tag $TAG already exists"),
        "the existing-tag guard must report a tag-specific error"
    );
    assert!(
        tag_block.contains("exit 1"),
        "the existing-tag guard must fail the run"
    );

    let release_block = between("if gh release view", "\n      - name: Check\n");
    assert!(
        release_block.contains("::error::Release $TAG already exists"),
        "the existing-release guard must report a release-specific error"
    );
    assert!(
        release_block.contains("exit 1"),
        "the existing-release guard must fail the run"
    );
}

#[test]
fn release_workflow_publishes_notes_and_packaged_assets() {
    assert!(WORKFLOW.contains("--generate-notes"));
    assert!(WORKFLOW.contains("--prerelease"));
    assert!(WORKFLOW.contains("--title \"clining $TAG\""));
    assert!(WORKFLOW.contains("make -s print-dist-asset"));
    assert!(WORKFLOW.contains("dist/SHA256SUMS"));
    assert!(WORKFLOW.contains("--target \"$GITHUB_SHA\""));
    assert!(WORKFLOW.contains("--verify-tag"));
    assert!(WORKFLOW.contains("cancel-in-progress: false"));

    let dist = offset("make dist");
    let asset = offset("make -s print-dist-asset");
    let publish = offset("gh release create");
    assert!(
        dist < asset && asset < publish,
        "packaging must precede publishing the asset path"
    );
}
