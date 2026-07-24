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
const ADAPTER: &str = "assets/base/claude/skills/cf-delegate/resources/claude-turn-completion.md";
const CONSULT: &str = "assets/base/agents/skills/cf-consult/SKILL.md";
const CUSTOMIZE: &str = "assets/base/agents/skills/cf-customize/SKILL.md";
const ORCHESTRATOR: &str = "assets/base/agents/skills/cf-model-orchestrator/SKILL.md";
const ROUTING: &str =
    "assets/base/agents/skills/cf-model-orchestrator/resources/capability-routing.md";
const AGENTS_TMPL: &str = "assets/base/AGENTS.md.tmpl";
const AGENTS_MINIMAL_TMPL: &str = "assets/base/AGENTS.minimal.md.tmpl";
const SPEC: &str = "project-management/specs/SPC-002.md";

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

const LIFECYCLE_ARROW: &str =
    "`delegate init` → wait-ready → `arm` → canonical UTF-8/internal-LF exact-byte delivery → wait-accepted → wait-terminal";

#[test]
fn lifecycle_sequence_is_ordered_across_delegate_assets() {
    for asset in [DELEGATE_SKILL, ADAPTER] {
        assert_ordered(
            asset,
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
    }
    for asset in [CONSULT, CUSTOMIZE, ORCHESTRATOR] {
        assert_contains(asset, &[LIFECYCLE_ARROW, "bounded cleanup"]);
    }
    // The lifecycle replaced the legacy signal protocol: no shipped skill may
    // reintroduce `tmux wait-for` as the work protocol.
    for asset in [DELEGATE_SKILL, ADAPTER, CONSULT, ORCHESTRATOR] {
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
    assert_contains(
        DELEGATE_SKILL,
        &[
            "settings file is **immutable**",
            "one outstanding armed turn per run",
            "Pane access is diagnosis-only.",
            "stable exit states",
        ],
    );
}

#[test]
fn lifecycle_pins_canonical_prompt_and_bounded_submission_retry() {
    for asset in [DELEGATE_SKILL, ADAPTER] {
        assert_contains(
            asset,
            &[
                "UTF-8",
                "internal LF line endings",
                "no terminal line break",
                "no NUL bytes",
                "300 ms",
                "send Enter once more",
                "Never",
                "repeated Enter",
            ],
        );
    }
    for asset in [DELEGATE_SKILL, ADAPTER, ORCHESTRATOR, CONSULT] {
        assert_contains(asset, &["user scope"]);
        assert!(
            !read(asset).contains("user or CLI scope"),
            "{asset} incorrectly promises lifecycle settings composition through repeated CLI flags"
        );
    }
}

#[test]
fn every_cross_harness_dispatch_declares_a_bounded_role() {
    for asset in [DELEGATE_SKILL, ORCHESTRATOR, CONSULT, ROUTING] {
        assert_contains(asset, &["ROLE: peer", "top-level", "host lineage"]);
    }
    for asset in [DELEGATE_SKILL, ORCHESTRATOR, ROUTING] {
        assert_contains(asset, &["generic", "subagent"]);
    }
}

#[test]
fn forward_lane_requires_native_recheckable_provenance_and_honest_effort() {
    assert_contains(
        DELEGATE_SKILL,
        &[
            "native Codex thread behind it",
            "native thread ID, recheckable",
            "never silently upgrade requested to observed",
            "grade it explicitly as inferred",
        ],
    );
    assert_contains(
        ORCHESTRATOR,
        &[
            "native Codex thread ID, recheckable",
            "otherwise label them requested",
            "never silently upgraded to observed",
            "grade inferred completion explicitly as inferred",
        ],
    );
    assert_contains(
        ROUTING,
        &[
            "native Codex thread ID",
            "never silently upgrade requested to observed",
            "Grade inferred completion explicitly as inferred.",
        ],
    );
}

#[test]
fn generic_claude_relay_never_counts_as_codex() {
    assert_contains(
        DELEGATE_SKILL,
        &["any surface that cannot show that thread never counts as Codex"],
    );
    assert_contains(
        ORCHESTRATOR,
        &["a generic Claude subagent or an unverified relay never counts as Codex"],
    );
    assert_contains(
        ROUTING,
        &[
            "A relay — plugin, adapter, relay subagent, or transport session — is transport, not author.",
            "a relay answering in the other vendor's name is evidence fabrication",
        ],
    );
}

#[test]
fn sibling_preflight_rejects_unknown_stop_hooks() {
    assert_contains(
        DELEGATE_SKILL,
        &[
            "Reject any sibling Stop hook whose nonblocking behavior you do not deterministically know.",
            "unverified sibling fails the preflight",
        ],
    );
    assert_contains(
        ADAPTER,
        &[
            "Reject any sibling Stop hook you do not deterministically know to be nonblocking",
            "an unknown or unverified sibling fails the preflight",
        ],
    );
    for asset in [CONSULT, CUSTOMIZE] {
        assert_contains(asset, &["sibling Stop-hook preflight"]);
    }
}

#[test]
fn sibling_preflight_permits_only_the_exact_known_safe_nonblocking_hook() {
    for asset in [DELEGATE_SKILL, ADAPTER] {
        assert_contains(
            asset,
            &[
                "stop-review-gate-hook.mjs",
                "`stopReviewGate` is off",
                "plugin's own surface",
            ],
        );
    }
    // The check is the operator's, against the plugin's own configuration —
    // CodeFlow ships no code that reads or infers plugin-private state.
    assert_contains(
        DELEGATE_SKILL,
        &["CodeFlow never reads or infers plugin-private state"],
    );
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
        &["Never use `codex exec`, `claude -p` / `--print`, or another headless peer invocation."],
    );
    assert_contains(CONSULT, &["never headless (`codex exec`, `claude -p`)"]);
    assert_contains(CUSTOMIZE, &["Never use `claude -p`"]);
}

#[test]
fn legacy_result_mode_is_compatibility_only_and_mutually_exclusive() {
    assert_contains(
        ADAPTER,
        &[
            "legacy one-shot record-and-signal mode",
            "until a later major release",
            "mutually exclusive and never fall back to one another",
            "New work always uses the schema-v2 lifecycle",
        ],
    );
    assert_contains(
        DELEGATE_SKILL,
        &[
            "byte-compatible compatibility for existing callers until a later major release",
            "mutually exclusive and never fall back",
        ],
    );
    assert_contains(
        SPEC,
        &["The two hook modes are mutually exclusive and never fall back to one another."],
    );
}

#[test]
fn five_obligation_evidence_contract_is_shared_across_both_adapters() {
    assert_contains(
        DELEGATE_SKILL,
        &[
            "five obligations, both lanes",
            "**Launch.**",
            "**Provenance.**",
            "**Return.**",
            "**Failure.**",
            "**Recheck.**",
        ],
    );
    assert_contains(
        ROUTING,
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
        &["five-obligation evidence contract — launch, provenance, return, failure, recheck"],
    );
    assert_contains(
        CONSULT,
        &["five-obligation evidence contract (launch/provenance/return/failure/recheck)"],
    );
    assert_contains(
        AGENTS_TMPL,
        &[
            "durable delegate lifecycle over the interactive Claude CLI",
            "launch, provenance, return, failure, recheck",
        ],
    );
    // Both templates carry the identical tier-neutral provenance sentence.
    let provenance = "Work attributed to another model or harness counts only with native, \
                      recheckable provenance";
    for template in [AGENTS_TMPL, AGENTS_MINIMAL_TMPL] {
        assert_contains(template, &[provenance]);
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
