//! Deterministic guards for the shipped delegate-lifecycle doctrine
//! (SPC-002 Plan v5, ADR-0036, TSK-002-003).
//!
//! These tests pin the *shipped contract text* of the canonical assets: the
//! lifecycle sequence, the five-obligation evidence contract, forward-lane
//! native provenance honesty, the sibling Stop-hook preflight, the headless
//! ban, and legacy-mode compatibility framing. Behavioral lifecycle coverage
//! lives in `delegate.rs` unit tests and `codeflow-cli/tests/delegate_cli.rs`
//! (including the `--result`/`--state-dir` never-fall-back rejection); broad
//! manifest and mirror checks live in `manifest_consistency.rs`. This file
//! only adds the delegate-doctrine pins those suites do not carry.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const DELEGATE_SKILL: &str = "assets/base/claude/skills/cf-delegate/SKILL.md";
// TSK-129 split cf-delegate into a common core plus one file per lane; each
// lane pin reads the lane file that now holds the duty.
const DELEGATE_PLUGIN_LANE: &str = "assets/base/claude/skills/cf-delegate/resources/lane-plugin.md";
const DELEGATE_LIFECYCLE_LANE: &str =
    "assets/base/claude/skills/cf-delegate/resources/lane-lifecycle.md";
const ADAPTER: &str = "assets/base/claude/skills/cf-delegate/resources/claude-turn-completion.md";
const CONSULT: &str = "assets/base/agents/skills/cf-consult/SKILL.md";
const CUSTOMIZE: &str = "assets/base/agents/skills/cf-customize/SKILL.md";
const DEVELOP: &str = "assets/base/agents/skills/cf-develop/SKILL.md";
const HERDR: &str = "assets/base/agents/skills/cf-herdr/SKILL.md";
const ORCHESTRATOR: &str = "assets/base/agents/skills/cf-model-orchestrator/SKILL.md";
const ROUTING: &str =
    "assets/base/agents/skills/cf-model-orchestrator/resources/capability-routing.md";
// TSK-127 moved the always-loaded doctrine one hop away: the workflow
// reference is installed at every tier.
const WORKFLOW_DISCIPLINE: &str = "assets/base/rules/workflow-discipline.md";
const SPEC: &str = "project-management/specs/SPC-002.md";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(relative: &str) -> String {
    let text = std::fs::read_to_string(root().join(relative))
        .unwrap_or_else(|error| panic!("read {relative}: {error}"));
    // TSK-129: capability-routing loads by section from an index, so a pin on
    // it reads the index together with every section file. TSK-184: the
    // session-level routing read is the orchestrator's seat section and the
    // assignment line lives in the plan section, so a pin on routing reads
    // those homes too.
    if relative == ROUTING {
        let mut text = with_sections(
            text,
            "assets/base/agents/skills/cf-model-orchestrator/resources/routing",
        );
        for home in [
            ORCHESTRATOR,
            "assets/base/agents/skills/cf-model-orchestrator/resources/quality/plan.md",
        ] {
            text.push('\n');
            text.push_str(&read(home));
        }
        return text;
    }
    text
}

fn with_sections(mut text: String, dir: &str) -> String {
    let mut sections: Vec<_> = std::fs::read_dir(root().join(dir))
        .unwrap_or_else(|error| panic!("read {dir}: {error}"))
        .map(|entry| entry.expect("section entry").path())
        .collect();
    sections.sort();
    for section in sections {
        text.push('\n');
        text.push_str(&std::fs::read_to_string(&section).expect("read section"));
    }
    text
}

fn normalized(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn assert_contains(relative: &str, markers: &[&str]) {
    let content = normalized(&read(relative));
    for marker in markers {
        assert!(
            content.contains(&normalized(marker)),
            "{relative} lost doctrine marker: {marker}"
        );
    }
}

/// Assert the markers appear in `relative` in the given order.
fn assert_ordered(relative: &str, markers: &[&str]) {
    let content = normalized(&read(relative));
    let mut position = 0;
    for marker in markers {
        let needle = normalized(marker);
        let found = content[position..].find(&needle).unwrap_or_else(|| {
            panic!("{relative}: lifecycle marker missing or out of order: {marker}")
        });
        position += found + needle.len();
    }
}

#[test]
fn lifecycle_sequence_is_ordered_across_delegate_assets() {
    // TSK-163: the sequence has one home, the adapter; the lane points there
    // and `each_delegated_turn_rule_is_stated_once_in_the_adapter` fails when
    // the lane states it again.
    assert_ordered(
        ADAPTER,
        &[
            "codeflow delegate init",
            "--until ready",
            "codeflow delegate arm",
            "paste-buffer -p",
            "Enter",
            "--until accepted",
            "--until terminal",
        ],
    );
    assert_contains(
        CONSULT,
        &["Launch through `cf-delegate`: it owns the lanes, the launch and delivery sequence"],
    );
    assert_contains(CUSTOMIZE, &["as `cf-delegate` and its lane files set out"]);
    assert_contains(
        ORCHESTRATOR,
        &[
            "Use `cf-delegate` for the seats and fallbacks of cross-family transport",
            "exact-byte delivery",
            "bounded cleanup",
        ],
    );
    // The lifecycle replaced the legacy signal protocol: no shipped skill may
    // reintroduce `tmux wait-for` as the work protocol.
    for asset in [
        DELEGATE_SKILL,
        DELEGATE_LIFECYCLE_LANE,
        ADAPTER,
        CONSULT,
        ORCHESTRATOR,
    ] {
        assert!(
            !read(asset).contains("tmux wait-for"),
            "{asset} reintroduced the legacy tmux wait-for protocol"
        );
    }
}

#[test]
fn lifecycle_documents_stable_exits_immutability_turns_and_pane_discipline() {
    assert_contains(
        ADAPTER,
        &[
            "`10` for a failed",
            "`11` for poison",
            "`124` for timeout",
            "`130` for interruption",
            "The generated settings file is immutable.",
            "one outstanding armed turn",
            "a terminal turn id can never be re-armed",
            "only for bounded diagnosis",
            "recovery is a new run id in a fresh state directory",
        ],
    );
    // TSK-163: immutability, sequential turns and the exit states moved with
    // turn detection to the adapter; the lane keeps its pane rule.
    assert_contains(
        ADAPTER,
        &[
            "The generated settings file is immutable.",
            "Each run permits one outstanding armed turn.",
            "## Stable exit states",
        ],
    );
    assert_contains(DELEGATE_LIFECYCLE_LANE, &["Pane access is diagnosis-only."]);
}

#[test]
fn lifecycle_pins_canonical_prompt_and_bounded_submission_retry() {
    // TSK-163: delivery and its bounded retry are stated once, in the adapter.
    assert_contains(
        ADAPTER,
        &[
            "UTF-8",
            "internal LF line endings",
            "no terminal line break",
            "no other control characters",
            "empty",
            "300 ms",
            "Carry out the pasted instructions.",
            "send Enter once more",
            "Never",
            "repeated Enter",
        ],
    );
    for asset in [DELEGATE_LIFECYCLE_LANE, ADAPTER, CONSULT] {
        assert!(
            !read(asset).contains("user or CLI scope"),
            "{asset} incorrectly promises lifecycle settings composition through repeated CLI flags"
        );
    }
    assert_contains(
        ORCHESTRATOR,
        &[
            "Use `cf-delegate` for the seats and fallbacks of cross-family transport",
            "foreground-return contract",
        ],
    );
}

#[test]
fn every_cross_harness_dispatch_declares_a_bounded_role() {
    for asset in [DELEGATE_PLUGIN_LANE, ORCHESTRATOR, CONSULT, ROUTING] {
        assert_contains(asset, &["ROLE: peer", "top-level", "host lineage"]);
    }
    for asset in [DELEGATE_PLUGIN_LANE, ORCHESTRATOR, ROUTING] {
        assert_contains(asset, &["generic", "subagent"]);
    }
}

#[test]
fn forward_lane_requires_native_recheckable_provenance_and_honest_effort() {
    assert_contains(
        DELEGATE_PLUGIN_LANE,
        &[
            "native Codex thread behind it",
            "native thread ID, recheckable",
            "never silently upgrade requested to observed",
            "grade it explicitly as inferred",
        ],
    );
    // TSK-129: the plugin-exchange detail moved to the plugin lane (pinned
    // above); the orchestrator's provenance invariant points at the routing
    // evidence section. TSK-184: the recheck and labelling rules live with
    // the five obligations in the cf-delegate evidence contract.
    assert_contains(ORCHESTRATOR, &["routing/evidence.md"]);
    assert_contains(
        DELEGATE_SKILL,
        &[
            "native Codex thread ID",
            "the resumable Codex thread or Grok session",
            "otherwise label them requested",
            "never silently upgrade requested to observed",
            "Grade inferred completion explicitly as inferred.",
        ],
    );
}

#[test]
fn generic_claude_relay_never_counts_as_codex() {
    assert_contains(
        DELEGATE_PLUGIN_LANE,
        &["any surface that cannot show that thread never counts as Codex"],
    );
    // TSK-129: the plugin-lane sentence lives in the plugin lane (pinned
    // above); the relay rule lives in the routing evidence section (pinned
    // below), and the orchestrator keeps its generic-subagent rule.
    assert_contains(
        ORCHESTRATOR,
        &["generic same-lineage subagent never satisfies"],
    );
    assert_contains(
        ROUTING,
        &[
            "A relay (plugin, adapter, relay subagent, or transport session) is transport, not author.",
            "a relay answering in the other vendor's name is evidence fabrication",
        ],
    );
}

/// One rule of the delegated Claude turn that TSK-163 states once, in the
/// adapter, as a step before launch.
struct TurnRule {
    name: &'static str,
    /// The adapter's statement of the rule, in reading order.
    adapter: &'static [&'static str],
    /// Text that, found in the lifecycle lane, states the rule again.
    restated_by: &'static [&'static str],
}

const TURN_RULES: &[TurnRule] = &[
    TurnRule {
        name: "launch sequence",
        adapter: &[
            "codeflow delegate init",
            LAUNCH,
            "--until ready",
            "codeflow delegate arm",
            "paste-buffer -p",
            "--until accepted",
            "--until terminal",
        ],
        restated_by: &[
            "codeflow delegate init",
            "codeflow delegate arm",
            "codeflow delegate wait",
            "tmux new-session",
            "paste-buffer",
            "send Enter once more",
        ],
    },
    TurnRule {
        name: "turn detection",
        adapter: &[
            "wires `SessionStart`, `UserPromptSubmit`, `Stop`, and `StopFailure`",
            "The generated settings file is immutable.",
            "Acceptance requires a `UserPromptSubmit` whose prompt matches the digest",
            "Each run permits one outstanding armed turn.",
        ],
        restated_by: &[
            "StopFailure",
            "UserPromptSubmit",
            "SHA-256",
            "immutable",
            "internal LF line endings",
            "one outstanding armed turn",
            "arm the next turn",
        ],
    },
    TurnRule {
        name: "sibling Stop-hook preflight",
        adapter: &[
            "Before launching, the operator enumerates the effective Stop-hook set",
            "Reject any sibling Stop hook you do not deterministically know to be nonblocking",
            "stop-review-gate-hook.mjs",
            "an unknown or unverified sibling fails the preflight",
        ],
        restated_by: &[
            "Stop-hook set",
            "Reject any sibling Stop hook",
            "stop-review-gate-hook.mjs",
            "stopReviewGate",
        ],
    },
];

/// The launch the preflight must come before: the adapter's first `claude`
/// session start.
const LAUNCH: &str = "tmux new-session";

/// Every way `lane` and `adapter` break the one-home rule, each naming the
/// rule: a rule the adapter lost or reordered, a rule the lane states again,
/// and a preflight the adapter places after launch.
fn turn_rule_faults(lane: &str, adapter: &str) -> Vec<String> {
    let lane = normalized(lane);
    let adapter = normalized(adapter);
    let mut faults = Vec::new();
    for rule in TURN_RULES {
        let mut position = 0;
        for marker in rule.adapter {
            let needle = normalized(marker);
            match adapter[position..].find(&needle) {
                Some(found) => position += found + needle.len(),
                None => faults.push(format!(
                    "{}: the adapter lost it or states it out of order: {marker}",
                    rule.name
                )),
            }
        }
        for marker in rule.restated_by {
            if lane.contains(&normalized(marker)) {
                faults.push(format!(
                    "{}: the lifecycle lane states it again: {marker}",
                    rule.name
                ));
            }
        }
    }
    // A missing preflight or launch is already reported above.
    let preflight = adapter.find("Reject any sibling Stop hook");
    let launch = adapter.find(LAUNCH);
    if let (Some(preflight), Some(launch)) = (preflight, launch) {
        if preflight > launch {
            faults.push(
                "sibling Stop-hook preflight: the adapter places it after launch".to_string(),
            );
        }
    }
    faults
}

#[test]
fn each_delegated_turn_rule_is_stated_once_in_the_adapter() {
    let faults = turn_rule_faults(&read(DELEGATE_LIFECYCLE_LANE), &read(ADAPTER));
    assert!(faults.is_empty(), "{}", faults.join("\n"));
    // The lane keeps its pointer, read before launch.
    assert_contains(
        DELEGATE_LIFECYCLE_LANE,
        &["before launching Claude, read and follow the shipped [turn lifecycle adapter](claude-turn-completion.md)"],
    );
}

#[test]
fn a_restated_lost_or_late_turn_rule_fails_naming_it() {
    let lane = read(DELEGATE_LIFECYCLE_LANE);
    let adapter = read(ADAPTER);
    // The lane stating each rule again, as it did before TSK-163.
    for (rule, restatement) in [
        (
            "launch sequence",
            "tmux new-session -d -s cf-run-42 -x 220 -y 50 -c /path/to/worktree",
        ),
        (
            "turn detection",
            "The generated settings file is **immutable** and bound to run id and state-dir spelling.",
        ),
        (
            "sibling Stop-hook preflight",
            "Before delivery, enumerate the effective Stop-hook set from every source the session loads.",
        ),
    ] {
        let faults = turn_rule_faults(&format!("{lane}\n{restatement}\n"), &adapter);
        assert!(
            !faults.is_empty() && faults.iter().all(|f| f.starts_with(rule)),
            "{rule}: {faults:?}"
        );
        assert!(faults.iter().all(|f| f.contains("states it again")));
    }
    // The adapter losing a rule.
    let lost = adapter.replace("codeflow delegate arm", "codeflow delegate prime");
    let faults = turn_rule_faults(&lane, &lost);
    assert!(
        faults
            .iter()
            .any(|f| f.starts_with("launch sequence: the adapter lost it")),
        "{faults:?}"
    );
    // The adapter moving the preflight after launch.
    let start = adapter
        .find("## Sibling Stop-hook preflight")
        .expect("preflight section");
    let end = adapter
        .find("## Launch and drive one turn")
        .expect("launch section");
    let moved = format!(
        "{}{}\n{}",
        &adapter[..start],
        &adapter[end..],
        &adapter[start..end]
    );
    let faults = turn_rule_faults(&lane, &moved);
    assert!(
        faults.contains(
            &"sibling Stop-hook preflight: the adapter places it after launch".to_string()
        ),
        "{faults:?}"
    );
}

#[test]
fn sibling_preflight_rejects_unknown_stop_hooks() {
    // TSK-163: the lane and adapter pins became the one guard above; the
    // consult and customize pointers (TSK-184 wording) stay.
    assert_contains(CUSTOMIZE, &["sibling Stop-hook preflight"]);
    assert_contains(CONSULT, &["the preflight and the evidence contract"]);
}

#[test]
fn sibling_preflight_permits_only_the_exact_known_safe_nonblocking_hook() {
    // TSK-163: stated once, in the adapter.
    assert_contains(
        ADAPTER,
        &[
            "stop-review-gate-hook.mjs",
            "`stopReviewGate` is off",
            "plugin's own surface",
        ],
    );
    // The check is the operator's, against the plugin's own configuration;
    // CodeFlow ships no code that reads or infers plugin-private state.
    assert_contains(
        ADAPTER,
        &["CodeFlow does not read or infer plugin-private state"],
    );
    assert_contains(
        SPEC,
        &[
            "only when its exact command is recognized",
            "CodeFlow does not read or infer plugin-private state",
        ],
    );
}

#[test]
fn no_headless_peer_execution_anywhere_in_doctrine() {
    assert_contains(
        DELEGATE_SKILL,
        &[
            "Prohibited at all times",
            "`codex exec`",
            "`claude -p` / `--print`",
        ],
    );
    assert_contains(
        ORCHESTRATOR,
        &[
            "Never use `codex exec`",
            "`claude -p` / `--print`",
            "`grok -p` / `--single`",
            "or another headless peer invocation.",
        ],
    );
    assert_contains(CONSULT, &["never headless (`codex exec`, `claude -p`)"]);
    assert_contains(
        CUSTOMIZE,
        &["Never use headless `codex exec` or `claude -p`"],
    );
}

#[test]
fn legacy_result_mode_is_compatibility_only_and_mutually_exclusive() {
    // TSK-184 removed the legacy-mode section from the shipped lanes (change
    // list WP5, cf-delegate row); the lanes offer only the schema-v2 lifecycle,
    // `delegate_cli.rs` keeps the never-fall-back rejection, and SPC-002 keeps
    // the contract sentence. TSK-163: the one launch sequence lives in the
    // adapter, and the lane points there.
    assert_contains(ADAPTER, &["## Launch and drive one turn"]);
    assert_contains(
        DELEGATE_LIFECYCLE_LANE,
        &["read and follow the shipped [turn lifecycle adapter](claude-turn-completion.md)"],
    );
    for asset in [DELEGATE_SKILL, DELEGATE_LIFECYCLE_LANE, ADAPTER] {
        assert!(
            !read(asset).contains("--result"),
            "{asset} offers the legacy --result mode as a route"
        );
    }
    assert_contains(
        SPEC,
        &["The two hook modes are mutually exclusive and never fall back to one another."],
    );
}

#[test]
fn five_obligation_evidence_contract_is_shared_across_both_adapters() {
    // TSK-184: the five obligations have one home in the cf-delegate core,
    // which points at the routing evidence section for admissibility; that
    // section points back for the obligations. Each lane states its
    // specifics.
    assert_contains(
        DELEGATE_SKILL,
        &[
            "Evidence contract, every lane",
            "one five-obligation evidence contract",
            "routing/evidence.md",
        ],
    );
    assert_contains(DELEGATE_PLUGIN_LANE, &["## Evidence on this lane"]);
    assert_contains(
        DELEGATE_LIFECYCLE_LANE,
        &[
            "## Evidence on this lane",
            "the terminal `wait` result already carries `provenance`",
        ],
    );
    assert_contains(
        ROUTING,
        &["five-obligation evidence contract (launch, provenance, return, failure, recheck) stated once in"],
    );
    assert_contains(
        DELEGATE_SKILL,
        &[
            "one five-obligation evidence contract",
            "**Launch**",
            "**Provenance**",
            "**Return**",
            "**Failure**",
            "**Recheck**",
        ],
    );
    assert_contains(
        ORCHESTRATOR,
        &["five-obligation evidence contract: launch, provenance, return, failure, recheck"],
    );
    assert_contains(
        CONSULT,
        &["five-obligation evidence contract (launch/provenance/return/failure/recheck)"],
    );
    // The entry-point cell's host routes live in the owning skills.
    // TSK-129: that route's text moved into the cf-delegate lifecycle lane.
    assert_contains(
        DELEGATE_LIFECYCLE_LANE,
        &["the durable lifecycle over the interactive claude CLI"],
    );
    assert_contains(
        ORCHESTRATOR,
        &["evidence contract: launch, provenance, return, failure, recheck"],
    );
    // Every tier installs the one tier-neutral provenance sentence.
    let provenance = "Work attributed to another model or harness counts only with native, \
                      recheckable provenance";
    assert_contains(WORKFLOW_DISCIPLINE, &[provenance]);
}

#[test]
fn consult_exhausts_qualified_fallback_without_weakening_auth_or_lineage() {
    assert_contains(
        CONSULT,
        &[
            "missing or incompatible preferred CLI, Herdr server or plugin is a lane failure",
            "Exhaust the qualified native alternatives",
            "authentication failure still stops for operator action",
            "After every qualified other-vendor route is unavailable",
            "Never substitute the host's own vendor",
        ],
    );
}

#[test]
fn worker_dispatch_propagates_unavailability_and_requires_foreground_return() {
    assert_contains(
        ROUTING,
        &[
            "Propagate current observed unavailability into every later worker choice",
            "do not infer that sibling models or another account are unavailable",
            // TSK-184: the worker effort preflight joined the orchestrator's
            // preflight, whose adapter sentence names every Claude launch.
            "On a Codex, Grok or other non-Claude host, before every Claude worker or same-session reviewer launch through the delegated lifecycle, load the",
            "claude-turn-completion.md",
            "a Claude host does not load it.",
            "collect the worker result before the primary returns",
            "It does not govern an in-session Agent launch",
            "the verified return is the task notification from this session's own launch",
            "Do not report the unit complete before it arrives",
            "preserve the existing Stop-hook and lifecycle safety policy unchanged",
        ],
    );
    assert_contains(
        ADAPTER,
        &[
            "## Sequential turns",
            "This section governs the delegated Claude lifecycle",
            "Collect delegated worker results before the primary returns",
            "never through Claude Bash `run_in_background` watchers",
            "work is still running is incomplete",
            "a worker that resumes the primary after its terminal result without an admitted notice poisons the run",
        ],
    );
}

#[test]
fn tracked_claude_launch_uses_scoped_synchronous_task_mode() {
    // TSK-163: the launch lines live in the adapter; the lane keeps the
    // Herdr variant's launch-local environment.
    assert_contains(DELEGATE_LIFECYCLE_LANE, &["launch-local task environment"]);
    assert_contains(
        ADAPTER,
        &[
            "CLAUDE_CODE_DISABLE_BACKGROUND_TASKS=1 claude",
            "For consult/no-edit, use the same launch with --permission-mode auto",
        ],
    );
    assert_contains(
        ADAPTER,
        &[
            "For every schema-v2 Claude process, set `CLAUDE_CODE_DISABLE_BACKGROUND_TASKS=1` after its login shell initializes and before Claude starts",
            "never add it to global settings, generic project presets, or the immutable generated task settings",
            "no `run_in_background` control on the Agent tool",
            "background Bash tasks, background subagents, and Ctrl+B are disabled",
            "servers or watchers in separate owned panes",
            "independent host-owned native sessions may still run in parallel",
            "bounded named-child canary",
            "reviewer result before that armed turn's `Stop`",
            "A task notification, `UserPromptSubmit`, backgrounded Agent, requested environment value, or launch string is not proof",
        ],
    );
    assert!(
        !read(ADAPTER).contains("run_in_background: false"),
        "the Agent tool does not expose a foreground launch flag"
    );
    assert_contains(
        HERDR,
        &[
            "Tracked Claude only: set after shell init in its dedicated pane",
            "herdr pane run",
            "export CLAUDE_CODE_DISABLE_BACKGROUND_TASKS=1",
            "verify `1`",
            "Close the dedicated tracked-Claude pane after its lifecycle ends",
        ],
    );
    assert!(
        !read(HERDR).contains("CLAUDE_CODE_DISABLE_BACKGROUND_TASKS=1 herdr agent start"),
        "the Herdr client environment is not guaranteed to reach its daemon-spawned child"
    );
}

#[test]
fn lifecycle_tracked_claude_reviewers_return_in_foreground() {
    assert_contains(
        ORCHESTRATOR,
        &[
            "Claude worker or same-session reviewer launch",
            "claude-turn-completion.md",
            "foreground-return contract",
        ],
    );
    assert_contains(
        DEVELOP,
        &[
            "For lifecycle-tracked Claude runs",
            "invoke `cf-reviewer` in the foreground",
            "`run_in_background: false` when offered",
            "collect its actual verdict before the primary turn ends",
            "never defer it to a later callback or bypass review",
        ],
    );
}

#[test]
fn delegate_prompts_narrow_authority_and_data_without_reasking_safe_handoffs() {
    assert_contains(
        DELEGATE_SKILL,
        &[
            "Every delegate prompt narrows authority and data",
            "actions/files/resources/data/processors/destinations/effects",
            "ambiguity blocks; never guess",
            "Send only necessary minimized data to an approved processor",
            "route qualification is not data authority",
            "already-authorized scoped handoff needs no new approval",
            "the lead verifies effects and claims",
        ],
    );
}

#[test]
fn native_fallback_keeps_authority_and_claim_boundaries() {
    let fallback = "assets/base/claude/skills/cf-delegate/resources/native-fallback.md";
    assert_contains(DELEGATE_SKILL, &["resources/native-fallback.md"]);
    assert_contains(
        fallback,
        &[
            "five obligations",
            "the child's sandbox does not protect its launcher",
            "an alternate route must preserve the rejected action's underlying boundary",
            "Cases whose expected behavior includes delegation still require a real peer",
            "retain old failures/unrun records, never relabel them as passes",
            "summaries that omit required actions are insufficient evidence",
        ],
    );
    for asset in [DELEGATE_SKILL, ORCHESTRATOR, CUSTOMIZE, CONSULT] {
        assert_contains(asset, &["native fallback", "cf-delegate"]);
        for stale in [
            "plugin, only",
            "no Herdr CLI third lane",
            "one lane per direction",
        ] {
            assert!(
                !normalized(&read(asset)).contains(stale),
                "{asset}: stale exclusive transport rule"
            );
        }
    }
}

/// The delegate skill is managed in the same five source/live/baseline
/// locations as the other pinned skills. Baseline drift is dangerous because
/// it corrupts future three-way updates without changing the active mirrors;
/// the broad mirror test compares only `.claude` vs `.agents`.
fn walk(dir: &Path, base: &Path, out: &mut BTreeSet<String>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, base, out);
            } else {
                out.insert(
                    path.strip_prefix(base)
                        .expect("path under base")
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
    }
}

#[test]
fn delegate_source_live_and_baseline_copies_are_byte_identical() {
    let root = root();
    let canonical = root.join("assets/base/claude/skills/cf-delegate");
    let copies = [
        root.join(".claude/skills/cf-delegate"),
        root.join(".agents/skills/cf-delegate"),
        root.join(".codeflow/.baseline/.claude/skills/cf-delegate"),
        root.join(".codeflow/.baseline/.agents/skills/cf-delegate"),
    ];

    let mut canonical_files = BTreeSet::new();
    walk(&canonical, &canonical, &mut canonical_files);
    assert!(
        !canonical_files.is_empty(),
        "delegate skill source is empty"
    );

    let mut problems = Vec::new();
    for copy in copies {
        let mut copy_files = BTreeSet::new();
        walk(&copy, &copy, &mut copy_files);
        for file in canonical_files.symmetric_difference(&copy_files) {
            problems.push(format!("{}: file-set drift at {file}", copy.display()));
        }
        for file in canonical_files.intersection(&copy_files) {
            let expected = std::fs::read(canonical.join(file)).expect("read canonical file");
            let actual = std::fs::read(copy.join(file)).expect("read mirrored file");
            if expected != actual {
                problems.push(format!("{}: byte drift at {file}", copy.display()));
            }
        }
    }
    problems.sort();
    assert!(
        problems.is_empty(),
        "delegate skill source/live/baseline copies drifted (regenerate with \
         `cargo run -p codeflow-cli -- update`):\n  {}",
        problems.join("\n  ")
    );
}

#[test]
fn process_round_guidance_names_the_evidence_before_action() {
    for path in [ORCHESTRATOR, "assets/base/agents/skills/cf-ship/SKILL.md"] {
        assert_contains(
            path,
            &[
                "Before choosing a landing route, read `PLAN.md`",
                "in the project README",
            ],
        );
    }
    assert_contains(
        "assets/base/rules/worktrees.md",
        &[
            "Before any removal, read `CODEFLOW_STATUS.txt`",
            "against current Git state",
            // TSK-194 first batch, case 15: a sandboxed removal stopped partway.
            "Where the effective sandbox denies those writes, make the proof first",
            "through the harness's sanctioned unsandboxed path",
            "Do not make a first attempt inside the sandbox",
            "keep the worktree and hand the proven removal to the operator",
            "never change sandbox or permission settings",
        ],
    );
    assert_contains(
        "assets/base/claude/skills/cf-method/references/project-organization.md",
        &[
            "A task branch is not an integration target",
            "real local or remote-tracking branch",
            "`codeflow work start`, `codeflow ci` and pre-commit enforce",
        ],
    );
    assert_contains(
        "assets/base/agents/skills/cf-ship/references/pr-checks.md",
        &[
            "timeout 30m gh pr checks <url> --required --watch --interval 60",
            "do not add a preliminary poll, a parallel poll or a background loop",
            "Wait for this command to finish before reporting readiness",
        ],
    );
    assert_contains(
        "assets/base/agents/skills/cf-model-orchestrator/resources/quality/irreversible.md",
        &[
            "refuse agent execution even after approval",
            "separate controlled operator channel",
            "checkpoint or backup identity, restore procedure",
            "do not execute the action to obtain it",
        ],
    );
}

// TSK-213 (ADR-0077): how one model family calls another is stated once.
const TRANSPORT: &str =
    "assets/base/agents/skills/cf-model-orchestrator/resources/routing/transport.md";

/// Every passage of the issue 31 inventory that routes a seat; each cites
/// the transport rule.
const TRANSPORT_CITERS: &[&str] = &[
    ORCHESTRATOR,
    "assets/base/agents/skills/cf-model-orchestrator/resources/grok-host.md",
    "assets/base/agents/skills/cf-model-orchestrator/resources/routing/design.md",
    DELEGATE_SKILL,
    DELEGATE_PLUGIN_LANE,
    DELEGATE_LIFECYCLE_LANE,
    "assets/base/claude/skills/cf-delegate/resources/native-fallback.md",
    ADAPTER,
    CONSULT,
    HERDR,
    CUSTOMIZE,
    "assets/base/CLAUDE.md.tmpl",
    "assets/base/AGENTS.md.tmpl",
    "assets/base/AGENTS.full.md.tmpl",
];

/// Retired preferences, compared lower-case on normalized text.
const STALE_ROUTES: &[&str] = &[
    "codex-plugin-cc` preferred",
    "prefer the official",
    "official plugin (preferred)",
    "preferred plugin",
    "when `herdr_env=1`",
    "use when herdr_env=1",
    "do not use from outside herdr",
    "tmux degraded",
    "degraded tmux",
    "still uses the official plugin",
    "through the official codex plugin, or a herdr tab",
];

/// A seat's launch flags: stated in the transport table, and written out
/// only in the adapter's literal tmux launch line.
const POSTURE_FLAGS: &[&str] = &[
    "danger-full-access",
    "--always-approve",
    "bypassPermissions",
    "--ask-for-approval never",
];

fn transport_faults(path: &str, text: &str) -> Vec<String> {
    let normal = normalized(text);
    let lower = normal.to_lowercase();
    let mut faults = Vec::new();
    if !normal.contains("transport.md") {
        faults.push(format!("{path} does not cite the transport rule"));
    }
    for stale in STALE_ROUTES {
        if lower.contains(stale) {
            faults.push(format!("{path} restates a retired route: {stale}"));
        }
    }
    if path != ADAPTER {
        for flag in POSTURE_FLAGS {
            if normal.contains(flag) {
                faults.push(format!("{path} restates a launch flag: {flag}"));
            }
        }
    }
    faults
}

#[test]
fn cross_family_transport_is_stated_once_and_cited_everywhere() {
    assert_contains(
        TRANSPORT,
        &[
            "never runs as a separate CLI session or a Herdr tab",
            "The interactive Codex CLI on the local Codex app-server",
            "The interactive Claude Code CLI, its turns tracked by the `codeflow delegate` lifecycle",
            "The interactive Grok Build CLI",
            "Any host may drive any reachable Herdr server",
            "the official Codex plugin, an optional fallback",
            "tmux, the last fallback, only when no Herdr server is reachable",
            "Never `codex exec`, `claude -p` / `--print`, `grok -p` / `--single`",
            "| Claude | `--permission-mode bypassPermissions` | `--permission-mode auto` |",
            "| Codex | `--ask-for-approval never --sandbox danger-full-access` | `--ask-for-approval never`, no `--sandbox` flag, so the project's `cf-guard` profile applies |",
            "| Grok | `--always-approve` | `--permission-mode auto` |",
            "The Codex builder posture is ADR-0075 D1",
            "The caller answers folder trust only for the task's own folder",
            "It answers hook trust (ADR-0075, amendment of 2026-10-03) only",
            "is byte-identical to the one at the pull\nrequest's target tip",
            "The\ncaller never trusts a changed hook and never picks \"continue without\ntrusting\".",
            "Skip a self-update offer.",
        ],
    );
    let mut faults = Vec::new();
    for path in TRANSPORT_CITERS {
        faults.extend(transport_faults(path, &read(path)));
    }
    assert!(faults.is_empty(), "{}", faults.join("\n"));
}

#[test]
fn a_restated_route_or_flag_fails_naming_it() {
    let faults = transport_faults(
        HERDR,
        "Official `codex-plugin-cc` preferred. Use when HERDR_ENV=1. \
         Codex: --ask-for-approval never --sandbox danger-full-access.",
    );
    for expected in [
        "does not cite the transport rule",
        "retired route: codex-plugin-cc` preferred",
        "retired route: use when herdr_env=1",
        "launch flag: danger-full-access",
        "launch flag: --ask-for-approval never",
    ] {
        assert!(
            faults.iter().any(|fault| fault.contains(expected)),
            "{expected}: {faults:?}"
        );
    }
}
