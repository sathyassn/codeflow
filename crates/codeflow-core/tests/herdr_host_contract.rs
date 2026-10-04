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
const DELEGATE_LIFECYCLE_LANE: &str =
    "assets/base/claude/skills/cf-delegate/resources/lane-lifecycle.md";

#[test]
fn herdr_skill_names_tabs_anti_hijack_and_lifecycle_boundary() {
    assert_contains(
        HERDR,
        &[
            "HERDR_ENV",
            "herdr tab create",
            "--label \"cf/<repo>/<work>/<kind>/<nn>\"",
            "Never send keys to `$HERDR_PANE_ID`",
            "cf-<repo>-<work>-<k><nn>",
            "`idle` or `done` is **not**",
            "schema-v2",
            "label that path **degraded**",
            // TSK-184 removed the Herdr run cache (change list WP5, cf-herdr
            // row), so "Herdr wins" over the cache has nothing to rule; the
            // live cwd check on resume stays.
            "pane `cwd` is `$PWD`",
            "intended worktree",
            // TSK-213 (ADR-0077): any host drives a reachable Herdr server,
            // and the launch flags live in the transport rule's posture
            // table, which the native args cite.
            "herdr status server",
            "from any host, inside a Herdr pane or outside one",
            "routing/transport.md",
            "--reasoning-effort <effort> <posture flags>",
            "same topic",
            "herdr pane send-text",
            "name namespace",
        ],
    );
}

fn yaml_description(relative: &str) -> String {
    let text = read(relative);
    let front = text
        .split("---")
        .nth(1)
        .unwrap_or_else(|| panic!("{relative} missing YAML frontmatter"));
    for line in front.lines() {
        if let Some(rest) = line.strip_prefix("description:") {
            return rest.trim().to_ascii_lowercase();
        }
    }
    panic!("{relative} missing description:");
}

#[test]
fn consult_and_delegate_descriptions_do_not_scent_on_herdr() {
    for relative in [CONSULT, DELEGATE] {
        let description = yaml_description(relative);
        assert!(
            !description.contains("herdr"),
            "{relative} YAML description is a load trigger, not a TTY-host dump"
        );
    }
}

#[test]
fn consult_and_delegate_route_through_herdr_whenever_a_server_is_reachable() {
    assert_contains(
        CONSULT,
        &[
            "cf-herdr",
            "From any host with a reachable Herdr server",
            "axis: standards",
            // TSK-184: consult points at cf-delegate for the launch; the
            // lifecycle lane (below) owns the Herdr delivery command.
            "`cf-herdr` hosts the seat in a named tab and never takes over the caller pane",
        ],
    );
    // TSK-129: the Herdr host rules and delivery moved with the lifecycle into
    // its lane file; the core names that lane.
    assert_contains(DELEGATE, &["resources/lane-lifecycle.md"]);
    assert_contains(
        DELEGATE_LIFECYCLE_LANE,
        &[
            "cf-herdr",
            "tmux only as the last fallback",
            "idle`/`done` is not turn completion",
            "herdr pane send-text",
        ],
    );
}

const HERDR_DELIVER: &str = "assets/base/agents/skills/cf-herdr/scripts/deliver.py";

/// TSK-144 AC-1 and AC-2: delivery goes through the script that confirms a
/// started turn, sends one Enter only and refuses a busy or unknown seat or
/// one whose folder is gone; resume uses the same
/// script; cleanup keeps a live seat's worktree; the Claude lane keeps its
/// `accepted` wait. The behaviour itself is proven against a stub `herdr` in
/// `evals/herdr-delivery/test_delivery.py`.
#[test]
fn herdr_delivery_confirms_a_started_turn_and_checks_the_seat_folder() {
    assert_contains(
        HERDR,
        &[
            "python3 \"$D\" --pane \"$pane_id\" --file \"$P\"",
            "--lifecycle",
            "--until accepted",
            "confirms within 20 s",
            "sends one Enter only: Herdr cannot tell the input from the scrollback",
            "`unknown` (exit 5)",
            "else exits 4 naming the pane",
            "never resend blindly",
            "sends nothing when the seat's folder is gone",
            "Resume delivers through the same script",
            "keep a worktree that a live seat uses as its folder until that seat's tab is closed",
        ],
    );
    assert_contains(
        HERDR_DELIVER,
        &[
            "\"agent\", \"get\"",
            "state_change_seq",
            "turn not confirmed on pane",
            "no second Enter was sent",
            "BUSY = STARTED | {\"unknown\"}",
            "nothing was sent. To relaunch",
            "LIMIT = 256 * 1024",
        ],
    );
}
