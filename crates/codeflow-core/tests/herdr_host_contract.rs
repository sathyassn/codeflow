//! Pins the Herdr-primary TTY overlay: naming, anti-hijack, and lifecycle
//! remaining the Claude completion protocol.

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(relative: &str) -> String {
    std::fs::read_to_string(root().join(relative))
        .unwrap_or_else(|error| panic!("read {relative}: {error}"))
}

fn normalized(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn assert_contains(relative: &str, markers: &[&str]) {
    let content = normalized(&read(relative));
    for marker in markers {
        assert!(
            content.contains(&normalized(marker)),
            "{relative} lost marker: {marker}"
        );
    }
}

const HERDR: &str = "assets/base/agents/skills/cf-herdr/SKILL.md";
const CONSULT: &str = "assets/base/agents/skills/cf-consult/SKILL.md";
const DELEGATE: &str = "assets/base/claude/skills/cf-delegate/SKILL.md";

#[test]
fn herdr_skill_names_tabs_anti_hijack_and_lifecycle_boundary() {
    assert_contains(
        HERDR,
        &[
            "HERDR_ENV",
            "named tab",
            "Never send keys to `$HERDR_PANE_ID`",
            "cf-<repo>-<work>-<k><nn>",
            "`idle` or `done` is **not**",
            "schema-v2",
            "label that path **degraded**",
            "Herdr wins",
            "intended worktree",
            "bypassPermissions",
            "herdr pane send-text",
            "name namespace",
        ],
    );
}

#[test]
fn consult_and_delegate_route_through_herdr_when_inside_herdr() {
    assert_contains(
        CONSULT,
        &[
            "cf-herdr",
            "HERDR_ENV=1",
            "axis: standards",
            "herdr pane send-text",
        ],
    );
    assert_contains(
        DELEGATE,
        &[
            "cf-herdr",
            "tmux is the degraded TTY host",
            "idle`/`done` is not turn completion",
            "herdr pane send-text",
        ],
    );
}
