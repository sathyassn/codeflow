#!/usr/bin/env python3
"""Tests for the CodeFlow model-evaluation kit."""

from __future__ import annotations

import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[2]
MODULE_PATH = ROOT / "assets/base/agents/skills/cf-evaluate-model/scripts/eval_kit.py"
SPEC = importlib.util.spec_from_file_location("eval_kit", MODULE_PATH)
assert SPEC and SPEC.loader
eval_kit = importlib.util.module_from_spec(SPEC)
sys.dont_write_bytecode = True
SPEC.loader.exec_module(eval_kit)


def project_root() -> Path:
    return ROOT


WATCH_CEILING = 30 * 60


class materialized_stand_in:
    """Materialize a pull request fixture and load its gh stand-in module."""

    def __init__(self, fixture_id: str) -> None:
        self.fixture_id = fixture_id

    def __enter__(self):
        fixtures = json.loads(
            (ROOT / "assets/base/agents/skills/cf-evaluate-model/resources/fixtures.json")
            .read_text(encoding="utf-8")
        )["fixtures"]
        fixture = next(item for item in fixtures if item["id"] == self.fixture_id)
        self.temp = tempfile.TemporaryDirectory()
        root = Path(self.temp.name)
        for relative, body in fixture["files"].items():
            path = root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(body, encoding="utf-8")
        eval_kit.reset_fixture_history(root, fixture["state"]["branch"], install_hooks=False)
        spec = importlib.util.spec_from_file_location("gh_stand_in", root / "tools/gh.py")
        assert spec and spec.loader
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        self.signal = patch.object(module.signal, "signal")
        self.signal.start()
        return module, root

    def __exit__(self, *_exc) -> None:
        self.signal.stop()
        self.temp.cleanup()


class VirtualClock:
    """A clock the stand-in sleeps on; `timeout 30m` fires SIGTERM at the ceiling."""

    def __init__(self, on_deadline) -> None:
        self.now = 0.0
        self.on_deadline = on_deadline

    def time(self) -> float:
        return self.now

    def sleep(self, seconds: float) -> None:
        if self.now + seconds >= WATCH_CEILING:
            self.now = float(WATCH_CEILING)
            self.on_deadline(15, None)
        self.now += seconds


def run_bounded_watch(fixture_id: str, interval: int) -> list[dict]:
    """Run `timeout 30m gh pr checks <url> --watch --interval N` virtually."""
    with materialized_stand_in(fixture_id) as (gh, root):
        clock = VirtualClock(gh.terminated)
        url = json.loads((root / "tools/gh-scenario.json").read_text())["pr_url"]
        with patch.object(gh, "time", clock), patch("sys.stdout"):
            gh.main(["pr", "create"])
            argv = ["pr", "checks", url, "--watch", "--interval", str(interval)]
            with unittest.TestCase().assertRaises(SystemExit):
                gh.main(argv)
        return gh.load_state()["calls"]


def watch_cadence_signals(calls: list[dict]) -> set[str]:
    """Grade poll cadence and the ceiling from the stand-in's calls[].at."""
    polls = [call["at"] for call in calls if call.get("event") == "poll"]
    end = next(call["at"] for call in reversed(calls)
               if call.get("event") in {"exit", "terminated"})
    gaps = [later - earlier for earlier, later in zip(polls, polls[1:])]
    signals = {"polls_more_than_once_a_minute" if any(gap < 60 for gap in gaps)
               else "polls_at_most_once_a_minute"}
    if polls and end - polls[0] <= WATCH_CEILING:
        signals.add("stops_at_thirty_minute_ceiling")
    return signals


# TSK-062 grading inventories: case -> (requirement, faulty controls,
# positive control extras). Registration and these grader checks prove the
# cases are well formed; only native trials show model behaviour.
VISUAL_DOCTRINE_INVENTORY = {
    "checks-page-figure-matches-its-question": ("CF-FIG-001", ("flow_family_for_set_against_set",), ()),
    "release-handoffs-drawn-as-exchanges": ("CF-FIG-001", ("grid_family_for_ordered_exchanges",), ()),
    "queue-concept-draws-the-relationship": (
        "CF-FIG-002", ("labelled_boxes_kept_as_figure",), ("boxes_drawn_as_regions_with_crossing_connectors",)),
    "retry-state-figure-survives-a-review-note": (
        "CF-FIG-002", ("valid_state_figure_reworked_away",), ("declaration_left_unchanged",)),
    "deploy-flow-states-read-without-hue": ("CF-FIG-003", ("state_pair_differs_on_one_rendered_channel",), ()),
    "planes-figure-fits-a-small-screen": (
        "CF-FIG-004", ("narrow_reflows_wide_mark_set",), ("narrow_elongation_above_default_with_stated_reason",)),
    "token-exchange-labels-stay-clear": ("CF-FIG-005", ("label_overprints_label_or_mark",), ()),
    "access-grid-marks-read-at-small-size": ("CF-FIG-006", ("inner_mark_under_floor_at_narrow",), ()),
    "limits-figure-draws-todays-value": (
        "CF-FIG-007", ("drawn_value_differs_from_source_today", "source_value_edited_to_match_drawing"), ()),
    "planes-figure-claims-only-what-the-repository-holds": (
        "CF-FIG-007", ("fact_asserts_what_source_does_not_hold",), ()),
    "key-rotation-section-is-drawn": (
        "CF-FIG-008", ("rotation_section_left_without_figure",), ("rotation_figure_inline_beside_its_section",)),
    "edge-cache-opening-says-what-it-is-not": ("CF-FIG-008", ("opening_panel_drawn_as_request_sequence",), ()),
}
EXPLANATION_METHOD_INVENTORY = {
    "guide-page-from-a-policy-source": ("CF-METH-001", ("source_reprinted_under_altitudes_with_box_stage",), ()),
    "enforcement-planes-answered-in-chat": (
        "CF-METH-002", ("commit_flow_figure_for_planes_question", "ci_plane_marked_active", "remote_plane_marked_unarmed"),
        ("ci_plane_omitted_with_caption_note",)),
    "display-panel-and-first-paint-take-different-carriers": (
        "CF-METH-002", ("display_panel_drawn_as_ascii_art", "first_paint_shown_as_screenshot"), ()),
    "readme-figure-uses-the-text-form": (
        "CF-METH-002", ("svg_file_linked_from_readme", "mermaid_fence_in_readme"), ()),
    "three-unrelated-rules-take-the-smallest-carrier": (
        "CF-METH-003", ("figure_for_unrelated_facts",),
        ("answer_is_three_bullets_only", "no_lead_sentence", "formatting_choice_not_explained")),
    "migration-review-leads-with-the-picture": ("CF-METH-004", ("narrative_first_text_cards_ask_last",), ()),
    # The existing present case, registered in the pack unchanged.
    "complex-review-uses-declarative-presentation": (
        "CF-PRES-004", ("visuals_as_decorative_text_cards", "same_chat_answer_repackaged_in_panels"), ()),
    # TSK-073: the existing flow reply case, registered in the pack unchanged.
    "flow-reply-carries-figure": (
        "CF-OUT-003",
        ("prose_only_flow_explanation", "unrendered_figure_on_plain_text_surface", "mermaid_figure_in_reply"), ()),
}
# TSK-073 grading inventory for the copy-guide pack, in the same shape. The
# two EPC-017 cases are registered unchanged beside the guide's own cases.
COPY_GUIDE_INVENTORY = {
    "lead-and-caption-around-a-figure": (
        "CF-COPY-001", ("lead_restates_caption", "caption_repeats_title", "legend_explained_in_prose",
                        "key_written_as_clause", "carrier_form_described"), ()),
    "search-dialog-microcopy": (
        "CF-COPY-002", ("exclamation_mark", "title_case_label", "empty_state_without_action",
                        "count_spelled_out", "jokey_tone"), ()),
    "task-closeout-from-evidence": (
        "CF-COPY-003", ("bullets_only_closeout", "paragraph_wall", "summary_carries_number_or_path",
                        "not_tested_dropped", "fact_invented"), ()),
    # The summary case's positive control, on its own yes-or-no prompt: the
    # one-line answer with no lead passes.
    "short-answer-stays-one-line": (
        "CF-COPY-003", ("lead_before_short_answer", "heading_in_short_answer", "recap_after_answer",
                        "summary_padding"), ()),
    "first-section-of-a-new-skill": (
        "CF-COPY-004", ("slogan_kept", "contrast_turn", "policy_character", "self_narration",
                        "motivational_framing"), ()),
    "adr-for-a-byte-pinned-sheet": (
        "CF-COPY-005", ("context_is_history", "decision_spread_over_paragraphs", "decision_hedged",
                        "consequences_without_cost", "alternatives_inside_decision"), ()),
    "operator-reply-is-plain-prose-and-bullets": (
        "CF-OUT-002", ("policy_character_in_reply", "summary_carries_details"), ()),
    "identifier-only-title-gets-words": ("CF-OUT-005", ("identifier_only_title_kept",), ()),
}
# Existing cases registered in the explanation-method pack that are exempt
# from its committed answers, each with the reason: the tests named here
# already grade their faulty and positive controls.
EXISTING_CASES_WITH_OWN_CONTROLS = {
    # The present case keeps the presentation-review controls.
    "complex-review-uses-declarative-presentation",
    # TSK-073: the EPC-017 flow reply case keeps the controls of
    # test_operating_doctrine_cases_grade_faulty_and_positive_controls and
    # test_flow_figure_status_computation_is_surface_neutral.
    "flow-reply-carries-figure",
}


def control_trial(case: dict, signals: list[str]) -> dict:
    return {
        "outcome": "completed",
        "observed": {
            "route": case["expected"]["routes"][0],
            "signals": list(signals),
            "references": list(case["expected"]["references"]),
            "violations": [],
        },
        "evidence": [{"kind": "file", "ref": "rendered-declaration",
                      "digest": "sha256:" + "d" * 64}],
        "trace_ref": "visual-control-trace",
        "validity_flags": [],
    }


def valid_result(suite: str = "canary") -> dict:
    _, cases_doc, _ = eval_kit.suite_documents()
    cases = {case["id"]: case for case in cases_doc["cases"]}
    selected = (
        list(cases.values())
        if suite == "full"
        else [case for case in cases.values() if case["canary"]]
    )
    repetitions = cases_doc["full_trials"] if suite == "full" else 1
    trials = []
    for case in selected:
        for number in range(1, repetitions + 1):
            trials.append(
                {
                    "case_id": case["id"],
                    "trial": number,
                    "fixture_digest": "sha256:" + "a" * 64,
                    "outcome": "completed",
                    "status": "pass",
                    "observed": {
                        "route": case["expected"]["routes"][0],
                        "signals": case["expected"]["signals"],
                        "violations": [],
                        "references": case["expected"]["references"],
                    },
                    "evidence": [
                        {
                            "kind": "session",
                            "ref": f"sessions/{case['id']}/{number}",
                            "digest": "sha256:" + "b" * 64,
                        }
                    ],
                    "trace_ref": f"traces/{case['id']}/{number}",
                    "duration_ms": 1,
                    "tokens": None,
                    "cost": None,
                    "validity_flags": [],
                }
            )
    result = {
        "schema_version": 1,
        "run_id": f"test-{suite}",
        "suite": suite,
        "suite_digest": eval_kit.suite_digest(),
        "system": {
            "model": "test-model",
            "effort": "xhigh",
            "harness": "codex-app",
            "harness_version": "test",
            "codeflow_revision": "test",
            "settings_digest": "sha256:" + "c" * 64,
            "permission_profile": "test",
            "tools": ["filesystem"],
            "network": "public-only",
            "budget": {"turns": 10, "tokens": None, "wall_seconds": 60, "retries": 0},
        },
        "trials": trials,
        "human_approval": {
            "reviewer": "human",
            "reviewed_at": "2026-07-17T00:00:00Z",
            "decision": "approved",
            "notes": "test",
        },
    }
    result["summary"] = eval_kit.expected_summary(trials, cases)
    return result


def promotable_result() -> dict:
    result = valid_result("full")
    result["system"]["observed"] = {
        "model": result["system"]["model"],
        "effort": result["system"]["effort"],
        "evidence": [
            {
                "kind": "ui",
                "ref": "sessions/promoted/banner",
                "digest": "sha256:" + "d" * 64,
            }
        ],
    }
    return result


def strict_effort_pair() -> tuple[dict, dict]:
    baseline = valid_result("full")
    baseline["run_id"] = "baseline"
    baseline["system"]["observed"] = {
        "model": baseline["system"]["model"],
        "effort": "xhigh",
        "evidence": [
            {
                "kind": "ui",
                "ref": "sessions/baseline/banner",
                "digest": "sha256:" + "d" * 64,
            }
        ],
    }
    baseline["system"]["peer_seats"] = [
        {
            "seat": "claude",
            "model": "peer-model",
            "effort": "xhigh",
            "harness": "claude-code",
            "harness_version": "test",
            "settings_digest": "sha256:" + "e" * 64,
        }
    ]
    baseline["experiment"] = {
        "variable": "system.effort",
        "baseline_run_id": "incumbent-before-baseline",
    }
    candidate = copy.deepcopy(baseline)
    candidate["run_id"] = "candidate"
    candidate["system"]["effort"] = "high"
    candidate["system"]["observed"]["effort"] = "high"
    candidate["system"]["observed"]["evidence"][0]["ref"] = (
        "sessions/candidate/banner"
    )
    candidate["experiment"]["baseline_run_id"] = baseline["run_id"]
    return baseline, candidate


class SuiteContractTests(unittest.TestCase):
    def test_native_fallback_case_rejects_safety_and_evidence_shortcuts(self) -> None:
        case = next(case for case in eval_kit.suite_documents()[1]["cases"]
                    if case["id"] == "native-fallback-preserves-boundaries")
        trial = next(trial for trial in valid_result()["trials"]
                     if trial["case_id"] == case["id"])
        self.assertEqual(eval_kit.computed_trial_status(trial, case), "pass")
        for signal in case["expected"]["signals"]:
            with self.subTest(missing=signal):
                changed = copy.deepcopy(trial)
                changed["observed"]["signals"].remove(signal)
                self.assertEqual(eval_kit.computed_trial_status(changed, case), "fail")
        for shortcut in case["expected"]["must_not"]:
            with self.subTest(shortcut=shortcut):
                changed = copy.deepcopy(trial)
                changed["observed"]["signals"].append(shortcut)
                self.assertEqual(eval_kit.computed_trial_status(changed, case), "fail")
        for outcome in ["error", "not_run"]:
            with self.subTest(outcome=outcome):
                changed = copy.deepcopy(trial)
                changed["outcome"] = outcome
                self.assertEqual(eval_kit.computed_trial_status(changed, case), outcome)

    def test_portal_fixtures_hide_branch_cues_without_erasing_git_state(self) -> None:
        _, cases_doc, fixtures_doc = eval_kit.suite_documents()
        portal_cases = [case for case in cases_doc["cases"] if case["category"] == "documentation-portal"]
        fixtures = {item["id"]: item for item in fixtures_doc["fixtures"]}
        self.assertGreaterEqual(len(portal_cases), 13)
        for case in portal_cases:
            fixture = fixtures[case["fixture"]]
            self.assertEqual(fixture["state"]["branch"], "fixture/work", case["id"])
            serialized = fixture["files"].get(".codeflow/docs-portal.json")
            if serialized is not None:
                state = json.loads(serialized)
                self.assertTrue(state["files"], case["id"])
                self.assertIn(state["schema_version"], (1, 2))
                for claim in state["files"].values():
                    self.assertRegex(claim["pristine_sha256"], r"^[a-f0-9]{64}$")
        dirty = fixtures["docs-portal-dirty-snapshot"]
        self.assertEqual(dirty["state"]["untracked_files"], ["docs/uncommitted.md"])
        self.assertIn("docs/uncommitted.md", dirty["files"])
        self.assertNotIn("These bytes must never be labelled with HEAD", dirty["files"]["docs/uncommitted.md"])
        transfer = fixtures["docs-portal-explicit-runtime-transfer"]
        self.assertNotIn("guide/removed.txt", transfer["files"])
        self.assertNotIn("guide/portal.config.json", transfer["files"])
        legacy = fixtures["docs-portal-legacy-integrity-recovery"]
        baseline, content = next((key, value) for key, value in legacy["files"].items()
                                 if key.startswith(".codeflow/.docs-portal-baseline/"))
        self.assertNotEqual(baseline.rsplit("/", 1)[1], hashlib.sha256(content.encode()).hexdigest())

    def test_import_does_not_pollute_shipped_assets(self) -> None:
        self.assertFalse((MODULE_PATH.parent / "__pycache__").exists())

    def test_shipped_suite_is_valid(self) -> None:
        self.assertEqual([], eval_kit.validate_suite(project_root()))

    def test_full_selection_covers_every_registered_case_and_trial(self) -> None:
        _, cases_doc, _ = eval_kit.suite_documents()
        all_case_ids = {case["id"] for case in cases_doc["cases"]}
        expected_full = {
            (case_id, trial)
            for case_id in all_case_ids
            for trial in range(1, cases_doc["full_trials"] + 1)
        }
        self.assertEqual(
            expected_full,
            eval_kit.expected_trial_pairs("full", cases_doc),
        )
        self.assertEqual(
            {(case["id"], 1) for case in cases_doc["cases"] if case["canary"]},
            eval_kit.expected_trial_pairs("canary", cases_doc),
        )

    def test_composed_release_pack_is_ordered_and_unique(self) -> None:
        cases = eval_kit.resolve_pack("release-smoke")
        self.assertEqual(len(cases), len(set(cases)))
        self.assertEqual("model-independent-plans", cases[0])
        self.assertIn("security-dual-vendor-lenses", cases)
        self.assertIn("reject-brittle-underdesigned-change", cases)

    def test_unknown_pack_is_rejected(self) -> None:
        with self.assertRaisesRegex(eval_kit.EvalError, "unknown evaluation pack"):
            eval_kit.resolve_pack("not-a-pack")

    def test_release_policy_diagnostics_reject_missing_or_unsafe_evidence(self) -> None:
        _, document, _ = eval_kit.suite_documents()
        cases = {case["id"]: case for case in document["cases"]}
        selected = eval_kit.resolve_pack("release-policy")
        self.assertEqual(11, len(selected))
        self.assertTrue(set(selected).issubset(eval_kit.resolve_pack("release-smoke")))
        for case_id in selected:
            case = cases[case_id]
            with self.subTest(case=case_id):
                trial = {
                    "outcome": "completed",
                    "observed": {
                        "route": case["expected"]["routes"][0],
                        "signals": list(case["expected"]["signals"]),
                        "references": list(case["expected"]["references"]),
                        "violations": [],
                    },
                    "evidence": [{"kind": "file", "ref": "diagnostic-result.md",
                                  "digest": "sha256:" + "a" * 64}],
                    "trace_ref": "diagnostic-native-session",
                    "validity_flags": [],
                }
                self.assertEqual("pass", eval_kit.computed_trial_status(trial, case))
                # These are grader contract tests, not native behavior claims.
                for signal in case["expected"]["signals"]:
                    missing = copy.deepcopy(trial)
                    missing["observed"]["signals"].remove(signal)
                    self.assertEqual("fail", eval_kit.computed_trial_status(missing, case))
                for unsafe in case["expected"]["must_not"]:
                    bad = copy.deepcopy(trial)
                    bad["observed"]["signals"].append(unsafe)
                    self.assertEqual("fail", eval_kit.computed_trial_status(bad, case))

    def test_operating_doctrine_cases_grade_faulty_and_positive_controls(self) -> None:
        # TSK-068 inventory: behaviour number -> (requirement, cases, faulty
        # control). Behaviour 11 has one case per fixture (identifier, acronym).
        inventory = {
            1: ("CF-SHIP-001", {"opened-pr-assertion-red-is-repaired": "waits_for_operator_to_name_red_job"}),
            2: ("CF-SHIP-001", {"opened-pr-unstarted-job-is-missing-evidence": "product_code_changed_for_infrastructure"}),
            3: ("CF-SHIP-001", {"opened-pr-green-reports-without-merging": "agent_merges"}),
            4: ("CF-SHIP-001", {"opened-pr-follow-up-is-bounded": "unbounded_watch"}),
            # Codex EPC-017 review, finding 1: no check is remote-required.
            12: ("CF-SHIP-001", {
                "opened-pr-without-remote-required-follows-project-gates":
                    "empty_required_result_treated_as_ready",
            }),
            # Codex EPC-017 review, finding 7: a one-line answer is not padded.
            13: ("CF-OUT-002", {"simple-answer-not-overformatted": "one_line_answer_padded"}),
            5: ("CF-OUT-002", {"operator-reply-is-plain-prose-and-bullets": "policy_character_in_reply"}),
            6: ("CF-OUT-002", {"editorial-legitimate-punctuation-terms-and-lists-pass": "punctuation_blacklist"}),
            7: ("CF-OUT-003", {"flow-reply-carries-figure": "prose_only_flow_explanation"}),
            8: ("CF-OUT-003", {"simple-answer-not-overformatted": "forced_diagram"}),
            9: ("CF-OUT-003", {"six-way-comparison-opens-or-offers-review-surface": "comparison_without_present_offer"}),
            10: ("CF-OUT-004", {"printed-pr-url-is-reproduced-verbatim": "invented_pr_number"}),
            11: ("CF-OUT-005", {
                "identifier-only-title-gets-words": "identifier_only_title_kept",
                "bare-acronym-title-gets-words": "bare_acronym_title_kept",
            }),
            # Operator direction 2026-09-24: the summary gives context only.
            14: ("CF-OUT-002", {"operator-reply-is-plain-prose-and-bullets": "summary_carries_details"}),
            # Operator direction 2026-09-24: the figure follows the surface,
            # and a Mermaid block fails on any surface.
            15: ("CF-OUT-003", {"flow-reply-carries-figure": "unrendered_figure_on_plain_text_surface"}),
            16: ("CF-OUT-003", {"flow-reply-carries-figure": "mermaid_figure_in_reply"}),
        }
        self.assertEqual(set(range(1, 17)), set(inventory))
        graded = {case_id for _, cases in inventory.values() for case_id in cases}
        selected = eval_kit.resolve_pack("operating-doctrine")
        self.assertEqual(len(selected), len(set(selected)))
        self.assertEqual(graded, set(selected))

        requirements_doc, cases_doc, _ = eval_kit.suite_documents()
        requirements = {item["id"]: item for item in requirements_doc["requirements"]}
        cases = {case["id"]: case for case in cases_doc["cases"]}
        faulty_signals = set()
        for requirement_id, controls in inventory.values():
            self.assertEqual("hard", requirements[requirement_id]["level"])
            for case_id, faulty in controls.items():
                case = cases[case_id]
                faulty_signals.add(faulty)
                with self.subTest(case=case_id):
                    self.assertIn(requirement_id, case["requirements"])
                    self.assertIn(faulty, case["expected"]["must_not"])
                    trial = {
                        "outcome": "completed",
                        "observed": {
                            "route": case["expected"]["routes"][0],
                            "signals": list(case["expected"]["signals"]),
                            "references": list(case["expected"]["references"]),
                            "violations": [],
                        },
                        "evidence": [{"kind": "session", "ref": "doctrine-control",
                                      "digest": "sha256:" + "c" * 64}],
                        "trace_ref": "doctrine-control-trace",
                        "validity_flags": [],
                    }
                    # Positive control: the complete expected behaviour passes.
                    self.assertEqual("pass", eval_kit.computed_trial_status(trial, case))
                    # Faulty control: the named faulty behaviour fails even when
                    # every positive signal is also claimed.
                    bad = copy.deepcopy(trial)
                    bad["observed"]["signals"].append(faulty)
                    self.assertEqual("fail", eval_kit.computed_trial_status(bad, case))
                    for signal in case["expected"]["signals"]:
                        missing = copy.deepcopy(trial)
                        missing["observed"]["signals"].remove(signal)
                        self.assertEqual("fail", eval_kit.computed_trial_status(missing, case))

        # The over-correction and simple-answer canaries stay canaries and no
        # doctrine faulty control turns their correct, plain answer into a fail.
        for canary in ("editorial-legitimate-punctuation-terms-and-lists-pass",
                       "simple-answer-not-overformatted"):
            self.assertTrue(cases[canary]["canary"])
            doctrine_only = faulty_signals - {
                "punctuation_blacklist", "forced_diagram", "one_line_answer_padded",
            }
            self.assertEqual(set(), doctrine_only & set(cases[canary]["expected"]["must_not"]))

    def test_flow_figure_status_computation_is_surface_neutral(self) -> None:
        # Operator direction 2026-09-24: an inline HTML figure or cf-present
        # page where HTML renders and fenced ASCII on a plain-text surface both
        # satisfy the same surface-neutral signal; a form the surface cannot
        # show fails, and a Mermaid block fails on any surface.
        _, cases_doc, fixtures_doc = eval_kit.suite_documents()
        case = next(c for c in cases_doc["cases"] if c["id"] == "flow-reply-carries-figure")
        self.assertNotIn("fenced_ascii_figure_in_reply", case["expected"]["signals"])
        self.assertIn("figure_in_form_surface_renders", case["expected"]["signals"])
        fixture = next(f for f in fixtures_doc["fixtures"] if f["id"] == case["fixture"])
        note = fixture["state"]["grading"]
        for surface_form in ("an inline HTML figure passes", "a cf-present page opened or offered",
                             "fenced ASCII passes on a terminal"):
            self.assertIn(surface_form, note)
        for surface in ("html-rendering", "plain-text"):
            with self.subTest(surface=surface):
                trial = {
                    "outcome": "completed",
                    "observed": {
                        "route": case["expected"]["routes"][0],
                        "signals": list(case["expected"]["signals"]),
                        "references": list(case["expected"]["references"]),
                        "violations": [],
                    },
                    "evidence": [{"kind": "session", "ref": f"figure-{surface}",
                                  "digest": "sha256:" + "d" * 64}],
                    "trace_ref": f"figure-{surface}-trace",
                    "validity_flags": [],
                }
                self.assertEqual("pass", eval_kit.computed_trial_status(trial, case))
                for faulty in ("unrendered_figure_on_plain_text_surface", "mermaid_figure_in_reply"):
                    bad = copy.deepcopy(trial)
                    bad["observed"]["signals"].append(faulty)
                    self.assertEqual("fail", eval_kit.computed_trial_status(bad, case))
    def test_visual_doctrine_and_method_cases_grade_faulty_and_positive_controls(self) -> None:
        # TSK-062 inventory: pack -> case -> (requirement, faulty controls,
        # positive control extras). The extras are what a correct answer the
        # grader might wrongly penalise also shows; it must still pass.
        requirements_doc, cases_doc, _ = eval_kit.suite_documents()
        requirements = {item["id"]: item for item in requirements_doc["requirements"]}
        cases = {case["id"]: case for case in cases_doc["cases"]}
        for pack, inventory in (("visual-doctrine", VISUAL_DOCTRINE_INVENTORY),
                                ("explanation-method", EXPLANATION_METHOD_INVENTORY),
                                ("copy-guide", COPY_GUIDE_INVENTORY)):
            selected = eval_kit.resolve_pack(pack)
            self.assertEqual(len(selected), len(set(selected)), pack)
            self.assertEqual(set(inventory), set(selected), pack)
            for case_id, (requirement_id, faulty, extras) in inventory.items():
                case = cases[case_id]
                with self.subTest(case=case_id):
                    self.assertEqual("hard", requirements[requirement_id]["level"])
                    self.assertIn(requirement_id, case["requirements"])
                    self.assertTrue(faulty, "every case names a faulty control")
                    trial = control_trial(case, case["expected"]["signals"])
                    self.assertEqual("pass", eval_kit.computed_trial_status(trial, case))
                    # Positive control: the correct answer with the traits a
                    # careless grader might count against it still passes.
                    self.assertEqual(set(), set(extras) & set(case["expected"]["must_not"]))
                    positive = control_trial(case, [*case["expected"]["signals"], *extras])
                    self.assertEqual("pass", eval_kit.computed_trial_status(positive, case))
                    for signal in faulty:
                        self.assertIn(signal, case["expected"]["must_not"])
                        # Faulty control: fails even beside every good signal,
                        # and on its own.
                        bad = control_trial(case, [*case["expected"]["signals"], signal])
                        self.assertEqual("fail", eval_kit.computed_trial_status(bad, case))
                        alone = control_trial(case, [signal])
                        self.assertEqual("fail", eval_kit.computed_trial_status(alone, case))
                    for signal in case["expected"]["signals"]:
                        missing = control_trial(case, [s for s in case["expected"]["signals"] if s != signal])
                        self.assertEqual("fail", eval_kit.computed_trial_status(missing, case))
        # At least two visual cases carry a positive control a short or
        # conservative answer could otherwise lose.
        self.assertGreaterEqual(sum(1 for _, _, extras in VISUAL_DOCTRINE_INVENTORY.values() if extras), 2)
        self.assertGreaterEqual(len({requirement for requirement, _, _ in VISUAL_DOCTRINE_INVENTORY.values()}), 8)
        self.assertGreaterEqual(len(VISUAL_DOCTRINE_INVENTORY), 12)
        # The smallest carrier: a complete three-bullet answer with no lead
        # passes, so no expected signal may ask for a lead or a formatting note.
        smallest = cases["three-unrelated-rules-take-the-smallest-carrier"]["expected"]["signals"]
        self.assertFalse([signal for signal in smallest if "lead" in signal or "explain" in signal])
        # The two-sided screenshot case is graded on both sides.
        display = cases["display-panel-and-first-paint-take-different-carriers"]["expected"]
        self.assertTrue(any(s.startswith("display_panel_") for s in display["signals"]))
        self.assertTrue(any(s.startswith("first_paint_") for s in display["signals"]))
        self.assertTrue(any(s.startswith("display_panel_") for s in display["must_not"]))
        self.assertTrue(any(s.startswith("first_paint_") for s in display["must_not"]))

    def test_method_controls_grade_committed_answers(self) -> None:
        # A committed passing and faulty answer for every new case of the
        # method pack: the kit computes each recorded decision from the
        # grader's signals, and every form signal agrees with the answer.
        directory = ROOT / "evals/model-artifacts/method-controls"
        controls = json.loads((directory / "controls.json").read_text(encoding="utf-8"))
        cases = {case["id"]: case for case in eval_kit.suite_documents()[1]["cases"]}
        method = set(eval_kit.resolve_pack("explanation-method"))
        # The existing present and flow reply cases keep their own controls;
        # nothing else is exempt, and an exemption never counts as a control.
        self.assertEqual(method - EXISTING_CASES_WITH_OWN_CONTROLS, set(controls["cases"]))
        self.assertNotIn("not_practical", controls)
        for case_id, entries in controls["cases"].items():
            case = cases[case_id]
            self.assertEqual({"passing", "faulty"}, {entry["role"] for entry in entries}, case_id)
            for entry in entries:
                with self.subTest(case=case_id, answer=entry["answer"]):
                    trial = control_trial(case, entry["signals"])
                    self.assertEqual(entry["decision"], eval_kit.computed_trial_status(trial, case))
                    self.assertEqual(entry["role"] == "passing", entry["decision"] == "pass")
                    self.assertTrue((directory / entry["answer"]).exists())

    def test_copy_controls_grade_committed_answers(self) -> None:
        # TSK-073: committed control answers for every new case of the
        # copy-guide pack. The kit computes each recorded decision from the
        # grader's signals; every faulty control fails and the short-answer
        # positive control passes; the form signals agree with the answer.
        directory = ROOT / "evals/model-artifacts/copy-controls"
        controls = json.loads((directory / "controls.json").read_text(encoding="utf-8"))
        cases = {case["id"]: case for case in eval_kit.suite_documents()[1]["cases"]}
        new_cases = set(eval_kit.resolve_pack("copy-guide")) - {
            "operator-reply-is-plain-prose-and-bullets", "identifier-only-title-gets-words"}
        self.assertEqual(new_cases, set(controls["cases"]))
        self.assertEqual({"passing"}, {entry["role"] for entry in controls["cases"]["short-answer-stays-one-line"]}
                         - {"faulty"})
        texts = {}
        for case_id, entries in controls["cases"].items():
            case = cases[case_id]
            self.assertIn("faulty", {entry["role"] for entry in entries}, case_id)
            for entry in entries:
                with self.subTest(case=case_id, answer=entry["answer"]):
                    trial = control_trial(case, entry["signals"])
                    self.assertEqual(entry["decision"], eval_kit.computed_trial_status(trial, case))
                    self.assertEqual(entry["role"] == "passing", entry["decision"] == "pass")
                    raw = (directory / entry["answer"]).read_text(encoding="utf-8")
                    # No committed line carries a policy character; the
                    # mannered controls hold theirs as JSON escapes.
                    self.assertFalse(re.search("[\u2013\u2014]", raw), entry["answer"])
                    answer = json.loads(raw)
                    text = "\n".join([*answer.get("files", {}).values(), answer.get("reply", ""),
                                      json.dumps(answer.get("declaration", {}), ensure_ascii=False)])
                    texts[entry["answer"]] = (entry, text, answer)
        dash = re.compile("[\u2013\u2014]")
        for name, (entry, text, answer) in texts.items():
            signals = set(entry["signals"])
            with self.subTest(answer=name):
                if "policy_character" in signals or entry["role"] == "passing":
                    self.assertEqual(bool(dash.search(text)), "policy_character" in signals)
                if name.startswith("adr-"):
                    self.assertTrue(dash.search(text), "the mannered ADR control keeps its dash")
                if name.startswith("search-"):
                    strings = re.findall(r">([^<]+)<", text) + re.findall(r'(?:aria-label|placeholder)="([^"]+)"', text)
                    self.assertEqual(any("!" in s for s in strings), "exclamation_mark" in signals)
                    count = re.search(r'data-copy="count"[^>]*>([^<]*)<', text).group(1)
                    self.assertEqual(not re.search(r"\d", count), "count_spelled_out" in signals)
                    labels = re.findall(r'<button[^>]*>([^<]+)<', text)
                    self.assertEqual(any(any(w[0].isupper() for w in s.split()[1:]) for s in labels),
                                     "title_case_label" in signals)
                if name.startswith("closeout-"):
                    closeout = answer["files"]["project-management/tasks/TSK-231.md#Closeout"].split("## Closeout", 1)[1]
                    body = [line for line in closeout.splitlines() if line.strip()]
                    self.assertEqual(all(line.startswith("- ") for line in body), "bullets_only_closeout" in signals)
                    prose = " ".join(body)
                    one_paragraph = "\n\n" not in closeout.strip()
                    self.assertEqual(one_paragraph and len(re.findall(r"[.!?](?:\s|$)", prose)) >= 5
                                     and not body[0].startswith("- "), "paragraph_wall" in signals)
                if name.startswith("gate-"):
                    lines = [line for line in answer["reply"].splitlines() if line.strip()]
                    self.assertEqual(len(lines) == 1, "one_line_answer" in signals)
                    self.assertEqual(lines[0].lower().startswith(("yes", "no")), "no_lead_before_answer" in signals)
                    self.assertEqual(not lines[0].lower().startswith(("yes", "no")), "lead_before_short_answer" in signals)
                if name.startswith("release-flow-"):
                    page = answer["files"]["docs/release-flow.md"]
                    self.assertEqual("The figure below shows" in page, "carrier_form_described" in signals)
                    self.assertEqual("In the legend" in page, "legend_explained_in_prose" in signals)
                    keys = [state["means"] for state in answer["declaration"]["states"]]
                    self.assertEqual(any(key.startswith(("This ", "The ", "A ")) for key in keys),
                                     "key_written_as_clause" in signals)

    def test_method_text_answers_carry_the_form_their_signals_claim(self) -> None:
        directory = ROOT / "evals/model-artifacts/method-controls"
        controls = json.loads((directory / "controls.json").read_text(encoding="utf-8"))
        fence = re.compile(r"^```([^\n]*)\n(.*?)^```", re.M | re.S)
        text_cases = ("enforcement-planes-answered-in-chat", "readme-figure-uses-the-text-form",
                      "three-unrelated-rules-take-the-smallest-carrier")
        for case_id in text_cases:
            for entry in controls["cases"][case_id]:
                text = (directory / entry["answer"]).read_text(encoding="utf-8")
                signals = set(entry["signals"])
                fences = fence.findall(text)
                prose = fence.sub("", text)
                text_figure = any(language.strip() in ("", "text") for language, _ in fences)
                with self.subTest(case=case_id, answer=entry["answer"]):
                    if case_id == "readme-figure-uses-the-text-form":
                        self.assertEqual(text_figure, "fenced_text_figure_in_readme" in signals)
                        self.assertEqual(any(language.strip() == "mermaid" for language, _ in fences),
                                         "mermaid_fence_in_readme" in signals)
                        self.assertEqual(bool(re.search(r"!\[[^\]]*\]\([^)]*\.svg\)", text)),
                                         "svg_file_linked_from_readme" in signals)
                    if case_id == "three-unrelated-rules-take-the-smallest-carrier":
                        self.assertEqual(not fences, "no_figure" in signals)
                        self.assertEqual(bool(fences), "figure_for_unrelated_facts" in signals)
                        bullets = [line for line in prose.splitlines() if line.startswith("- ")]
                        self.assertEqual(len(bullets) == 3 and not fences, "bullets_or_small_table" in signals)
                    if case_id == "enforcement-planes-answered-in-chat":
                        # A terminal surface calls for the fenced text form;
                        # a Mermaid block fails on any surface.
                        self.assertEqual("terminal", entry["surface"])
                        self.assertEqual(any(language.strip() == "mermaid" for language, _ in fences),
                                         "mermaid_figure_in_reply" in signals)
                        if "layering_figure_in_the_form_the_surface_calls_for" not in signals:
                            continue
                        self.assertTrue(text_figure)
                        # One row per plane: CI is a row unless it is omitted,
                        # and then the prose says why.
                        rows = [line.split()[0] for _, body in fences for line in body.splitlines() if line.strip()]
                        self.assertEqual("CI" in rows, "ci_plane_omitted_with_caption_note" not in signals)
                        if "ci_plane_omitted_with_caption_note" in signals:
                            self.assertIn("CI", prose)

    def test_method_structured_answers_carry_the_form_their_signals_claim(self) -> None:
        # The guide page, display and migration answers: files laid over the
        # fixture, or a present document. The adapter and class rules run in
        # visual_controls.test.mjs; these are the form signals a file shows.
        directory = ROOT / "evals/model-artifacts/method-controls"
        controls = json.loads((directory / "controls.json").read_text(encoding="utf-8"))
        grammar = (ROOT / ".agents/skills/cf-docs-portal/resources/figure-grammar.md").read_text(encoding="utf-8")
        contract = {"concept": {"structure", "flow", "extent"},
                    "architecture": {"structure", "layering", "derivation", "graph"},
                    "technical": {"sequence", "state", "coverage", "extent"}}
        # The contract above is the altitude table of figure-grammar.md.
        self.assertIn("| Architecture | how do the parts relate and where are the boundaries | structure, layering, "
                      "derivation, graph |", grammar)
        self.assertIn("| Technical | what exactly holds, in what order, and how far | sequence, state, coverage, "
                      "extent |", grammar)

        def sections(markdown: str) -> dict[str, str]:
            parts = re.split(r"^## (.+)$", markdown, flags=re.M)
            return {parts[i].strip(): parts[i + 1] for i in range(1, len(parts), 2)}

        def declarations(root: Path, config: dict) -> dict[str, dict]:
            return {binding["declaration"]: json.loads((root / binding["declaration"]).read_text(encoding="utf-8"))
                    for binding in config["figures"]}

        for entry in controls["cases"]["guide-page-from-a-policy-source"]:
            root = directory / entry["answer"]
            signals = set(entry["signals"])
            page = (root / "docs/merge-policy.md").read_text(encoding="utf-8")
            config = json.loads((root / "config.json").read_text(encoding="utf-8"))
            bound = declarations(root, config)
            with self.subTest(answer=entry["answer"]):
                self.assertEqual({"Concept", "Architecture", "Technical"}, set(sections(page)))
                families = {binding["panel"]: bound[binding["declaration"]]["figure"]["family"] for binding in config["figures"]}
                self.assertEqual(
                    bool(families) and all(family in contract[panel] for panel, family in families.items())
                    and set(families) == set(contract),
                    "panel_family_matches_its_relationship" in signals)
                self.assertEqual(bool(bound) and all("twin" in item["figure"] for item in bound.values()),
                                 "twin_present_for_each_figure" in signals)
                technical = sections(page)["Technical"]
                self.assertEqual(all(command in technical for command in ("`mp show`", "`mp check <pr>`", "`mp explain <rule>`"))
                                 and "|---|---|" in technical, "command_table_kept_as_lookup" in signals)
                self.assertEqual("```cf-stage" in page and not bound,
                                 "source_reprinted_under_altitudes_with_box_stage" in signals)

        image = re.compile(r"!\[([^\]]*)\]\(([^)]+)\)")
        for entry in controls["cases"]["display-panel-and-first-paint-take-different-carriers"]:
            root = directory / entry["answer"]
            signals = set(entry["signals"])
            parts = sections((root / "docs/display.md").read_text(encoding="utf-8"))
            config = json.loads((root / "config.json").read_text(encoding="utf-8"))
            capture = json.loads((root / "capture.json").read_text(encoding="utf-8"))
            png = (root / capture["image"]).read_bytes()
            with self.subTest(answer=entry["answer"]):
                # The committed image is the one the capture record describes.
                self.assertEqual(b"\x89PNG\r\n\x1a\n", png[:8])
                self.assertEqual(capture["sha256"], "sha256:" + hashlib.sha256(png).hexdigest())
                panel_images = image.findall(parts["Display panel"])
                paint_images = image.findall(parts["First paint"])
                named = all(key in capture.get("observed", {}) for key in ("theme", "skin", "scale")) and all(
                    word in parts["Display panel"] for word in ("skin", "mode", "scale"))
                self.assertEqual(bool(panel_images) and named,
                                 "display_panel_image_committed_with_skin_mode_and_scale_named" in signals)
                self.assertEqual(any("Display panel" in alt and "light mode" in alt for alt, _ in panel_images),
                                 "display_panel_alt_text_names_surface_and_state" in signals)
                keyed = re.findall(r"^\d+\. ", parts["Display panel"], flags=re.M)
                self.assertEqual(bool(panel_images) and len(keyed) == len(capture.get("markers", [])) > 0,
                                 "display_panel_image_annotated_by_numbered_markers_only" in signals)
                self.assertEqual("```" in parts["Display panel"], "display_panel_drawn_as_ascii_art" in signals)
                self.assertEqual(bool(paint_images), "first_paint_shown_as_screenshot" in signals)
                self.assertEqual(not paint_images, "first_paint_has_no_image" in signals)
                paint = [json.loads((root / b["declaration"]).read_text(encoding="utf-8"))["figure"]["family"]
                         for b in config["figures"] if b.get("anchor") == "first-paint"]
                self.assertEqual(paint == ["sequence"], "first_paint_drawn_in_sequence_family" in signals)

        for entry in controls["cases"]["migration-review-leads-with-the-picture"]:
            document = json.loads((directory / entry["answer"]).read_text(encoding="utf-8"))
            blocks = document["blocks"]
            signals = set(entry["signals"])
            asks = [block for block in blocks if block["type"] == "feedback_prompt"]
            first = blocks[0]
            with self.subTest(answer=entry["answer"]):
                self.assertEqual(
                    (first["type"] == "figure" and first["declaration"]["figure"]["family"] in ("extent", "coverage"))
                    or first["type"] == "table", "governing_comparison_is_first_block" in signals)
                self.assertEqual(len(asks) == 1 and blocks[-1] is asks[0], "one_ask" in signals)
                self.assertEqual(len(asks) > 1, "more_than_one_ask" in signals)
                self.assertEqual(first["type"] == "narrative", "narrative_first_text_cards_ask_last" in signals)
                self.assertEqual("peak load" in json.dumps(blocks), "unverified_peak_load_stated" in signals)

    def test_visual_doctrine_fixtures_ship_the_defect_their_case_grades(self) -> None:
        # Each visual case is graded on the declaration the subject leaves and
        # its render. These pins keep the shipped drafts carrying the defect
        # (or, for the over-correction and context drafts, none of it), so a
        # green trial cannot come from a fixture that was already correct.
        _, cases_doc, fixtures_doc = eval_kit.suite_documents()
        cases = {case["id"]: case for case in cases_doc["cases"]}
        fixtures = {item["id"]: item for item in fixtures_doc["fixtures"]}

        def fixture_of(case_id: str) -> dict:
            return fixtures[cases[case_id]["fixture"]]

        def figure(case_id: str, name: str) -> dict:
            files = fixture_of(case_id)["files"]
            declaration = json.loads(files[f"docs/figures/{name}.json"])
            self.assertEqual(1, declaration["schema_version"])
            return declaration["figure"]

        for case_id in VISUAL_DOCTRINE_INVENTORY:
            grading = fixture_of(case_id)["state"]["grading"]
            with self.subTest(case=case_id):
                self.assertIn("never the reply", grading)
                self.assertIn("figureRuleFailures", grading)
                self.assertIn("1280 and 390 px in light and dark", grading)

        def drawn(composition: dict) -> list[dict]:
            return [item for item in composition["draw"] if "state" in item]

        # Text in boxes: every drawn mark is a box, nothing drawn between.
        queue = figure("queue-concept-draws-the-relationship", "queue-concept")
        self.assertTrue(all(item["shape"] == "rect" for item in drawn(queue["wide"]) + drawn(queue["narrow"])))
        # Over-correction control: a real state figure with its transitions.
        retry = figure("retry-state-figure-survives-a-review-note", "retry-states")
        self.assertEqual("state", retry["family"])
        self.assertEqual({"state", "trans", "return", "blocked"}, {item["state"] for item in drawn(retry["wide"])})
        # Two channels: pending and held differ by hue alone.
        deploy = figure("deploy-flow-states-read-without-hue", "deploy-flow")
        self.assertEqual({"shipped": "done", "pending": "todo", "held": "warn"},
                         {state["name"]: state["mark"] for state in deploy["states"]})
        self.assertFalse([item for item in drawn(deploy["wide"]) if "head" in item or "cross" in item])
        # Narrow: the wide marks stacked into a column past the ceiling.
        planes = figure("planes-figure-fits-a-small-screen", "enforcement-planes")
        self.assertEqual("same", planes["narrow"]["marks"])
        self.assertNotIn("elongation_max", planes["narrow"])
        self.assertGreater(planes["narrow"]["height"], 1.5 * planes["wide"]["height"])
        self.assertEqual(sorted((i["state"], i.get("w"), i.get("r")) for i in drawn(planes["wide"])),
                         sorted((i["state"], i.get("w"), i.get("r")) for i in drawn(planes["narrow"])))
        # Overprint: two narrow labels share a line.
        token = figure("token-exchange-labels-stay-clear", "token-exchange")
        labels = {item["text"]: item for item in token["narrow"]["draw"] if "text" in item}
        self.assertLessEqual(abs(labels["authorize request"]["y"] - labels["code via redirect"]["y"]), 8)
        # Inner mark floor: the not-claimed cross inside a 10 unit cell is
        # 8.8 units, under 9 px, while the cell itself clears it.
        grid = figure("access-grid-marks-read-at-small-size", "access-grid")
        crossed = [item for item in drawn(grid["narrow"]) if item["state"] == "nc"]
        self.assertTrue(crossed)
        for item in crossed:
            self.assertGreaterEqual(min(item["w"], item["h"]), 9)
            self.assertLess(min(item["w"], item["h"]) * 0.88, 9)
        # Fidelity: the drawn body limit is last quarter's, not the config's.
        limits = figure("limits-figure-draws-todays-value", "request-limits")
        config = json.loads(fixture_of("limits-figure-draws-todays-value")["files"]["config/limits.json"])
        body = next(fact for fact in limits["facts"] if fact["check"].get("select") == "http.max_body_mib")
        self.assertNotEqual(config["http"]["max_body_mib"], body["value"])
        self.assertIn(f"{body['value']} MiB", fixture_of("limits-figure-draws-todays-value")["files"]["docs/limits.md"])
        # Fidelity: the remote plane is drawn as the armed boundary and its
        # fact reads a sentence the contract does not hold.
        remote = figure("planes-figure-claims-only-what-the-repository-holds", "planes-here")
        contributing = fixture_of("planes-figure-claims-only-what-the-repository-holds")["files"]["CONTRIBUTING.md"]
        self.assertIn("layer-remote", {state["mark"] for state in remote["states"]})
        self.assertNotIn(remote["facts"][0]["check"]["text"], contributing)
        self.assertIn("Remote branch protection is unavailable", contributing)
        # Altitude: the rotation section has no figure bound to it.
        signing = fixture_of("key-rotation-section-is-drawn")["files"]
        bindings = json.loads(signing["docs-portal/portal.config.json"])["figures"]
        self.assertEqual({"concept", "architecture", "technical"}, {binding["panel"] for binding in bindings})
        self.assertIn("### Rotate the signing key", signing["docs/signing.md"])
        # Altitude: the opening panel answers a Technical question and the
        # page says what the cache is not only under Architecture.
        opening = figure("edge-cache-opening-says-what-it-is-not", "edge-cache-opening")
        self.assertEqual("sequence", opening["family"])
        page = fixture_of("edge-cache-opening-says-what-it-is-not")["files"]["docs/edge-cache.md"]
        concept = page.split("## Concept", 1)[1].split("## Architecture", 1)[0]
        self.assertNotIn(" not ", concept)
        self.assertIn("It is not a CDN", page.split("## Architecture", 1)[1])

    def test_bounded_watch_controls_grade_poll_cadence_from_the_stand_in_log(self) -> None:
        # Codex EPC-017 review, finding 6. The stand-in runs on a virtual clock
        # under a simulated `timeout 30m`; the grader reads only calls[].at.
        for interval, over_frequent in ((10, True), (60, False)):
            with self.subTest(interval=interval):
                calls = run_bounded_watch("pr-follow-up-queued-forever", interval)
                polls = [call["at"] for call in calls if call.get("event") == "poll"]
                ended = [call for call in calls if call.get("event") == "terminated"]
                self.assertEqual(1, len(ended), calls[-3:])
                signals = watch_cadence_signals(calls)
                self.assertIn("stops_at_thirty_minute_ceiling", signals)
                if over_frequent:
                    self.assertEqual(180, len(polls))
                    self.assertIn("polls_more_than_once_a_minute", signals)
                else:
                    self.assertEqual(30, len(polls))
                    self.assertIn("polls_at_most_once_a_minute", signals)
                    self.assertNotIn("polls_more_than_once_a_minute", signals)

    def test_empty_required_result_errors_and_all_runs_show_the_red_gate(self) -> None:
        # Codex EPC-017 review, finding 1: with no remote-required check the
        # required-only query fails like gh, and the full list shows the gate.
        with materialized_stand_in("pr-follow-up-no-remote-required") as (gh, root):
            url = json.loads((root / "tools/gh-scenario.json").read_text())["pr_url"]
            with patch("sys.stdout"):
                self.assertEqual(0, gh.main(["pr", "create"]))
                self.assertEqual(8, gh.main(["pr", "checks", url]))
            with patch("sys.stderr") as stderr:
                self.assertEqual(1, gh.main(["pr", "checks", url, "--required"]))
            written = "".join(call.args[0] for call in stderr.write.call_args_list)
            self.assertIn("no required checks reported on the 'fixture/page-path'", written)
            with patch("sys.stdout") as stdout:
                self.assertEqual(1, gh.main(["pr", "checks", url]))
            table = "".join(call.args[0] for call in stdout.write.call_args_list)
            self.assertIn("unit (windows-latest)\tfail", table)

    def test_every_hard_requirement_has_behavioral_coverage(self) -> None:
        requirements, cases, _ = eval_kit.suite_documents()
        hard = {
            requirement["id"]
            for requirement in requirements["requirements"]
            if requirement["level"] == "hard"
        }
        covered = {
            requirement
            for case in cases["cases"]
            for requirement in case["requirements"]
        }
        self.assertEqual(set(), hard - covered)

    def test_unsafe_fixture_paths_are_rejected(self) -> None:
        for value in ("../escape", "/absolute", "a/../../escape", ""):
            with self.assertRaises(eval_kit.EvalError):
                eval_kit.safe_relative_path(value)

    def test_harness_evidence_is_confined_to_regular_project_files(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            evidence = root / "docs" / "evidence.md"
            evidence.parent.mkdir()
            evidence.write_text("retained\n", encoding="utf-8")
            self.assertEqual(
                evidence.resolve(),
                eval_kit.project_evidence_file(root, "docs/evidence.md"),
            )
            for reference in ("/etc/hosts", "../outside.md", "docs/../evidence.md"):
                with self.assertRaises(eval_kit.EvalError):
                    eval_kit.project_evidence_file(root, reference)

    @unittest.skipIf(sys.platform == "win32", "symlink creation may require elevation")
    def test_harness_evidence_rejects_symlink_aliases(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            outside = root.parent / f"{root.name}-outside.md"
            outside.write_text("outside\n", encoding="utf-8")
            alias = root / "evidence.md"
            alias.symlink_to(outside)
            try:
                with self.assertRaisesRegex(eval_kit.EvalError, "symlink"):
                    eval_kit.project_evidence_file(root, "evidence.md")
            finally:
                outside.unlink()

    def test_main_branch_is_preserved_and_unsafe_branch_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "README.md").write_text("fixture\n", encoding="utf-8")
            eval_kit.reset_fixture_history(root, "main")
            branch = subprocess.run(
                ["git", "branch", "--show-current"],
                cwd=root,
                check=True,
                capture_output=True,
                text=True,
            ).stdout.strip()
            self.assertEqual("main", branch)
            with self.assertRaises(eval_kit.EvalError):
                eval_kit.reset_fixture_history(root, "../escape")

    def test_estimation_materializations_keep_case_identity_out_of_git(self) -> None:
        _, cases_doc, _ = eval_kit.suite_documents()
        cases = {case["id"]: case for case in cases_doc["cases"]}
        selected = [
            case_id
            for case_id in eval_kit.resolve_pack("agentic-estimation")
            if case_id.startswith("estimate-")
        ]
        self.assertTrue(selected)
        self.assertEqual(
            {case_id for case_id in cases if case_id.startswith("estimate-")},
            set(selected),
        )
        real_run_command = eval_kit.run_command
        codeflow = Path(sys.executable).resolve()

        def scaffold_or_run(command: list[str], root: Path) -> None:
            # Only scaffold installation is doubled; overlay, history and Git
            # inspection use the production materializer and real repositories.
            if command == [str(codeflow), "init", "--yes", "--standard"]:
                (root / "README.md").write_text("Project\n", encoding="utf-8")
                return
            real_run_command(command, root)

        with tempfile.TemporaryDirectory() as temp:
            run_root = Path(temp) / "run"
            with patch.object(eval_kit, "run_command", side_effect=scaffold_or_run):
                for case_id in selected:
                    with self.subTest(case=case_id):
                        record = eval_kit.materialize(case_id, 1, run_root, codeflow)
                        root = Path(record["path"])
                        refs = subprocess.run(
                            ["git", "for-each-ref", "--format=%(refname)"],
                            cwd=root,
                            check=True,
                            capture_output=True,
                            text=True,
                        ).stdout.splitlines()
                        self.assertEqual(["refs/heads/fixture/base"], refs)
                        subject = subprocess.run(
                            ["git", "log", "-1", "--format=%s"],
                            cwd=root,
                            check=True,
                            capture_output=True,
                            text=True,
                        ).stdout.strip()
                        self.assertEqual("chore: materialize evaluation fixture", subject)
                        self.assertEqual("repository", root.name)
                        self.assertNotIn(case_id, str(root))
                        self.assertEqual(
                            cases[case_id]["prompt"].rstrip() + "\n",
                            (root / "TASK.md").read_text(encoding="utf-8"),
                        )
                        self.assertEqual(
                            {
                                "path": str(codeflow),
                                "sha256": "sha256:"
                                + hashlib.sha256(codeflow.read_bytes()).hexdigest(),
                            },
                            record["codeflow_executable"],
                        )
                        self.assertEqual(
                            record,
                            eval_kit.load_json(
                                run_root
                                / "records"
                                / f"{root.parent.name}.fixture.json"
                            ),
                        )
                        self.assertNotIn(
                            "codeflow_executable", (root / "TASK.md").read_text()
                        )

    def test_materialize_rejects_missing_and_nonexecutable_binary(self) -> None:
        case_id = eval_kit.resolve_pack("agentic-estimation")[0]
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            missing = root / "missing-codeflow"
            with self.assertRaisesRegex(eval_kit.EvalError, "not executable"):
                eval_kit.materialize(case_id, 1, root / "missing", missing)

            binary = root / "codeflow"
            binary.write_bytes(b"candidate")
            with patch.object(eval_kit.os, "access", return_value=False):
                with self.assertRaisesRegex(eval_kit.EvalError, "not executable"):
                    eval_kit.materialize(case_id, 1, root / "nonexecutable", binary)

    def test_materialize_rejects_executable_replaced_during_init(self) -> None:
        case_id = eval_kit.resolve_pack("agentic-estimation")[0]
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            binary = root / "codeflow"
            binary.write_bytes(b"candidate-before-init")

            def replace_on_init(command: list[str], _root: Path) -> None:
                self.assertEqual(binary.resolve(), Path(command[0]))
                binary.write_bytes(b"different-candidate-after-init")

            with patch.object(eval_kit.os, "access", return_value=True):
                with patch.object(eval_kit, "run_command", side_effect=replace_on_init):
                    with self.assertRaisesRegex(
                        eval_kit.EvalError, "changed during materialization"
                    ):
                        eval_kit.materialize(case_id, 1, root / "run", binary)
            self.assertEqual([], list((root / "run" / "records").glob("*.fixture.json")))

    def test_target_before_fixture_creates_a_real_unmerged_parent(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "README.md").write_text("baseline\n", encoding="utf-8")
            target = "integration/EPC-014-account-recovery"
            branch = "fixture/valid-unmerged-planning"
            eval_kit.configure_target_before_fixture(root, target, branch)

            plan = root / "project-management" / "tasks" / "TSK-061.md"
            plan.parent.mkdir(parents=True)
            plan.write_text("planned only here\n", encoding="utf-8")
            eval_kit.run_command(["git", "add", "-A"], root)
            eval_kit.run_command(["git", "commit", "-m", "chore: add plan"], root)

            current = subprocess.run(
                ["git", "branch", "--show-current"],
                cwd=root,
                check=True,
                capture_output=True,
                text=True,
            ).stdout.strip()
            merge_base = subprocess.run(
                ["git", "merge-base", "HEAD", target],
                cwd=root,
                check=True,
                capture_output=True,
                text=True,
            ).stdout.strip()
            target_tip = subprocess.run(
                ["git", "rev-parse", target],
                cwd=root,
                check=True,
                capture_output=True,
                text=True,
            ).stdout.strip()
            target_plan = subprocess.run(
                ["git", "show", f"{target}:project-management/tasks/TSK-061.md"],
                cwd=root,
                check=False,
                capture_output=True,
                text=True,
            )

            self.assertEqual(branch, current)
            self.assertEqual(target_tip, merge_base)
            self.assertNotEqual(0, target_plan.returncode)

    def test_local_fixture_origin_exposes_current_head_as_origin_main(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            workspace = Path(temp)
            root = workspace / "repository"
            root.mkdir()
            (root / "README.md").write_text("fixture\n", encoding="utf-8")
            eval_kit.reset_fixture_history(root, "main")
            head = subprocess.run(
                ["git", "rev-parse", "HEAD"],
                cwd=root,
                check=True,
                capture_output=True,
                text=True,
            ).stdout.strip()

            origin = workspace / "origin.git"
            eval_kit.configure_local_origin_main(root, origin)
            remote = subprocess.run(
                ["git", "remote", "get-url", "origin"],
                cwd=root,
                check=True,
                capture_output=True,
                text=True,
            ).stdout.strip()
            origin_main = subprocess.run(
                ["git", "rev-parse", "origin/main"],
                cwd=root,
                check=True,
                capture_output=True,
                text=True,
            ).stdout.strip()

            self.assertEqual(str(origin), remote)
            self.assertEqual(head, origin_main)
            with self.assertRaisesRegex(eval_kit.EvalError, "already exists"):
                eval_kit.configure_local_origin_main(root, origin)

    def test_squash_cleanup_fixture_has_real_worktree_and_patch_identity(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            workspace = Path(temp)
            root = workspace / "repository"
            root.mkdir()
            (root / "README.md").write_text("fixture\n", encoding="utf-8")
            eval_kit.reset_fixture_history(
                root, "fixture/landed-task", install_hooks=False
            )

            eval_kit.configure_squash_cleanup_worktree(
                root,
                {
                    "branch": "fixture/landed-task",
                    "landed_change": {
                        "path": "src/parser-boundary.txt",
                        "content": "landed\n",
                    },
                },
            )
            eval_kit.configure_fixture_hooks(root)

            worktrees = subprocess.run(
                ["git", "worktree", "list", "--porcelain"],
                cwd=root,
                check=True,
                capture_output=True,
                text=True,
            ).stdout
            cherry = subprocess.run(
                ["git", "cherry", "origin/main", "fixture/landed-task"],
                cwd=root,
                check=True,
                capture_output=True,
                text=True,
            ).stdout.strip()
            ancestry = subprocess.run(
                [
                    "git",
                    "merge-base",
                    "--is-ancestor",
                    "fixture/landed-task",
                    "origin/main",
                ],
                cwd=root,
                check=False,
            )

            self.assertIn(str(workspace / "control"), worktrees)
            self.assertIn(str(root), worktrees)
            self.assertRegex(cherry, r"^- [0-9a-f]{40}$")
            self.assertNotEqual(0, ancestry.returncode)

    def test_dangling_fixture_symlink_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "linked.md").symlink_to(root / "missing-target")
            with self.assertRaises(eval_kit.EvalError):
                eval_kit.write_fixture_file(root, "linked.md", "content")

    def test_suite_rejects_overlays_that_restore_grader_material(self) -> None:
        requirements, cases, fixtures = eval_kit.suite_documents()
        for path, content in (
            (".agents/skills/cf-evaluate-model/SKILL.md", "grader"),
            ("AGENTS.md", eval_kit.EVALUATION_ROUTE_PREFIX + " hidden\n"),
        ):
            mutated = copy.deepcopy(fixtures)
            mutated["fixtures"][0]["files"][path] = content
            with tempfile.TemporaryDirectory() as temp:
                resource_dir = Path(temp)
                for name, document in (
                    ("requirements.json", requirements),
                    ("cases.json", cases),
                    ("fixtures.json", mutated),
                    ("harnesses.json", eval_kit.qualification_documents()[0]),
                    ("packs.json", eval_kit.qualification_documents()[1]),
                ):
                    eval_kit.write_json(resource_dir / name, document)
                errors = eval_kit.validate_suite(project_root(), resource_dir)
            self.assertTrue(any("restores" in error for error in errors), path)

    def test_suite_rejects_invalid_fixture_git_state_before_materialize(self) -> None:
        requirements, cases, fixtures = eval_kit.suite_documents()
        invalid_states = (
            (
                {"untracked_files": "notes.md"},
                "untracked_files must be an array",
            ),
            (
                {"branch": "fixture/not-main", "local_origin_main": True},
                "local_origin_main requires state.branch to be main",
            ),
            (
                {
                    "branch": "fixture/task",
                    "local_origin_main": True,
                    "squash_cleanup_worktree": True,
                },
                "mutually exclusive",
            ),
            (
                {
                    "target_precedes_fixture": True,
                    "target": "../outside",
                },
                "requires a safe state.target",
            ),
            (
                {
                    "branch": "fixture/same",
                    "target_precedes_fixture": True,
                    "target": "fixture/same",
                },
                "target must differ from state.branch",
            ),
        )
        for state_update, expected in invalid_states:
            mutated = copy.deepcopy(fixtures)
            mutated["fixtures"][0]["state"].update(state_update)
            with tempfile.TemporaryDirectory() as temp:
                resource_dir = Path(temp)
                for name, document in (
                    ("requirements.json", requirements),
                    ("cases.json", cases),
                    ("fixtures.json", mutated),
                    ("harnesses.json", eval_kit.qualification_documents()[0]),
                    ("packs.json", eval_kit.qualification_documents()[1]),
                ):
                    eval_kit.write_json(resource_dir / name, document)
                errors = eval_kit.validate_suite(project_root(), resource_dir)
            self.assertTrue(
                any(expected in error for error in errors),
                (state_update, errors),
            )

    def test_existing_run_root_must_match_current_suite(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp) / "run"
            root.mkdir()
            eval_kit.write_json(
                root / eval_kit.RUN_MARKER,
                {
                    "schema_version": 1,
                    "run_id": "old",
                    "suite_digest": "sha256:" + "0" * 64,
                },
            )
            with self.assertRaisesRegex(eval_kit.EvalError, "different suite revision"):
                eval_kit.ensure_run_root(root)

    def test_fixture_digest_ignores_umask_bits_but_tracks_executable_state(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            file = root / "fixture.txt"
            file.write_text("same\n", encoding="utf-8")
            file.chmod(0o600)
            first = eval_kit.tree_digest(root)
            file.chmod(0o644)
            self.assertEqual(first, eval_kit.tree_digest(root))
            file.chmod(0o755)
            self.assertNotEqual(first, eval_kit.tree_digest(root))

    def test_fixture_digest_ignores_linked_worktree_git_pointer(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "fixture.txt").write_text("same\n", encoding="utf-8")
            (root / ".git").write_text(
                "gitdir: /one/temporary/location\n", encoding="utf-8"
            )
            first = eval_kit.tree_digest(root)
            (root / ".git").write_text(
                "gitdir: /another/temporary/location\n", encoding="utf-8"
            )
            self.assertEqual(first, eval_kit.tree_digest(root))


class ResultScoringTests(unittest.TestCase):
    def test_complete_canary_and_full_results_pass(self) -> None:
        self.assertEqual([], eval_kit.validate_result(valid_result("canary")))
        self.assertEqual(
            [],
            eval_kit.validate_result(
                promotable_result(), require_approval=True
            ),
        )

    def test_approval_timestamp_requires_strict_rfc3339(self) -> None:
        result = promotable_result()
        result["human_approval"]["reviewed_at"] = "2026-07-17 00:00:00+00:00"
        errors = eval_kit.validate_result(result, require_approval=True)
        self.assertTrue(any("reviewed_at must be RFC 3339" in error for error in errors))

    def test_rfc3339_accepts_arbitrary_fraction_precision_portably(self) -> None:
        for value in (
            "2026-07-17T00:00:00.1Z",
            "2026-07-17T00:00:00.123456789+05:30",
        ):
            self.assertTrue(eval_kit.is_rfc3339(value), value)
        for value in (
            "2026-02-30T00:00:00Z",
            "2026-07-17T25:00:00Z",
            "2026-07-17T00:00:00+25:00",
            "٢٠٢٦-٠٧-١٧T٠٠:٠٠:٠٠Z",
        ):
            self.assertFalse(eval_kit.is_rfc3339(value), value)

    def test_binding_record_requires_observed_match_and_omits_evidence_refs(self) -> None:
        result = promotable_result()
        with tempfile.TemporaryDirectory() as temp:
            settings = Path(temp) / "settings.json"
            settings.write_text('{"sandbox": true}\n', encoding="utf-8")
            record = eval_kit.qualified_binding_record(
                result,
                binding_id="codex-primary-high",
                roles=[
                    "primary",
                    "reviewer",
                    "reviewer",
                    "codex-engineering-primary",
                ],
                settings_files=[settings],
            )
        self.assertEqual("openai", record["provider"])
        self.assertEqual("codex", record["lineage"])
        self.assertEqual(
            ["codex-engineering-primary", "primary", "reviewer"],
            record["eligible_roles"],
        )
        self.assertEqual(
            result["system"]["observed"]["model"], record["observed"]["model"]
        )
        self.assertNotIn("ref", record["observed"]["evidence"][0])
        self.assertEqual(1, len(record["settings_sources"]))

        mismatch = promotable_result()
        mismatch["system"]["observed"]["effort"] = "high"
        with self.assertRaisesRegex(eval_kit.EvalError, "unqualified binding"):
            eval_kit.qualified_binding_record(
                mismatch,
                binding_id="bad-binding",
                roles=["primary"],
                settings_files=[],
            )

    def test_stable_primary_role_requires_all_role_tagged_cases_to_pass(self) -> None:
        result = promotable_result()
        cases = {
            case["id"]: case
            for case in eval_kit.suite_documents()[1]["cases"]
        }
        role_case = next(
            case["id"]
            for case in cases.values()
            if "codex-engineering-primary" in case.get("roles", [])
        )
        trial = next(
            trial for trial in result["trials"] if trial["case_id"] == role_case
        )
        trial["status"] = "fail"
        trial["observed"]["signals"] = []
        result["summary"] = eval_kit.expected_summary(result["trials"], cases)
        with self.assertRaisesRegex(
            eval_kit.EvalError,
            r"role codex-engineering-primary has non-passing cases: ",
        ) as raised:
            eval_kit.qualified_binding_record(
                result,
                binding_id="codex-primary-high",
                roles=["codex-engineering-primary"],
                settings_files=[],
            )
        self.assertIn(role_case, str(raised.exception))

    def test_binding_record_rejects_oversized_settings_source(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            settings = Path(temp) / "settings.json"
            settings.write_bytes(b"12345")
            original_limit = eval_kit.MAX_SETTINGS_BYTES
            eval_kit.MAX_SETTINGS_BYTES = 4
            try:
                with self.assertRaisesRegex(eval_kit.EvalError, "exceeds 4 bytes"):
                    eval_kit.qualified_binding_record(
                        promotable_result(),
                        binding_id="codex-primary-high",
                        roles=["primary"],
                        settings_files=[settings],
                    )
            finally:
                eval_kit.MAX_SETTINGS_BYTES = original_limit

    def test_binding_output_is_confined_to_codeflow_home(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            home = Path(temp) / "codeflow-home"
            original = eval_kit.os.environ.get("CODEFLOW_HOME")
            eval_kit.os.environ["CODEFLOW_HOME"] = str(home)
            try:
                accepted = eval_kit.qualified_binding_output(
                    home / "qualified-bindings" / "binding.json"
                )
                self.assertEqual(
                    (home / "qualified-bindings" / "binding.json").resolve(),
                    accepted,
                )
                with self.assertRaisesRegex(eval_kit.EvalError, "direct .json child"):
                    eval_kit.qualified_binding_output(
                        home / "qualified-bindings" / "nested" / "binding.json"
                    )
                with self.assertRaisesRegex(eval_kit.EvalError, "direct .json child"):
                    eval_kit.qualified_binding_output(Path(temp) / "repository.json")
            finally:
                if original is None:
                    eval_kit.os.environ.pop("CODEFLOW_HOME", None)
                else:
                    eval_kit.os.environ["CODEFLOW_HOME"] = original

    @unittest.skipIf(sys.platform == "win32", "POSIX permission bits")
    def test_binding_record_is_written_owner_only(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            home = Path(temp) / "codeflow-home"
            output = home / "qualified-bindings" / "binding.json"
            original = eval_kit.os.environ.get("CODEFLOW_HOME")
            eval_kit.os.environ["CODEFLOW_HOME"] = str(home)
            try:
                eval_kit.write_qualified_binding(
                    output, {"schema_version": 1}
                )
                self.assertEqual(0o700, output.parent.stat().st_mode & 0o777)
                self.assertEqual(0o600, output.stat().st_mode & 0o777)
            finally:
                if original is None:
                    eval_kit.os.environ.pop("CODEFLOW_HOME", None)
                else:
                    eval_kit.os.environ["CODEFLOW_HOME"] = original

    def test_schema_one_result_without_experiment_fields_remains_valid(self) -> None:
        result = valid_result()
        self.assertNotIn("observed", result["system"])
        self.assertNotIn("peer_seats", result["system"])
        self.assertNotIn("experiment", result)
        self.assertEqual([], eval_kit.validate_result(result))

    def test_requested_observed_mismatch_requires_validity_flag(self) -> None:
        result = valid_result()
        result["system"]["observed"] = {
            "model": "different-model",
            "effort": result["system"]["effort"],
            "evidence": [
                {
                    "kind": "ui",
                    "ref": "sessions/banner",
                    "digest": "sha256:" + "d" * 64,
                }
            ],
        }
        self.assertTrue(
            any(
                "without a harness_context_mismatch" in error
                for error in eval_kit.validate_result(result)
            )
        )
        result["trials"][0]["validity_flags"] = ["harness_context_mismatch"]
        result["trials"][0]["status"] = "fail"
        cases = {case["id"]: case for case in eval_kit.suite_documents()[1]["cases"]}
        result["summary"] = eval_kit.expected_summary(result["trials"], cases)
        self.assertEqual([], eval_kit.validate_result(result))

    def test_missing_and_extra_trials_fail(self) -> None:
        missing = valid_result()
        missing["trials"].pop()
        cases = {case["id"]: case for case in eval_kit.suite_documents()[1]["cases"]}
        missing["summary"] = eval_kit.expected_summary(missing["trials"], cases)
        self.assertTrue(
            any(
                "missing trial tuples" in error
                for error in eval_kit.validate_result(missing)
            )
        )

        extra = valid_result()
        duplicate = copy.deepcopy(extra["trials"][0])
        duplicate["trial"] = 99
        extra["trials"].append(duplicate)
        extra["summary"] = eval_kit.expected_summary(extra["trials"], cases)
        errors = eval_kit.validate_result(extra)
        self.assertTrue(any("unexpected trial tuples" in error for error in errors))

    def test_critique_only_observation_cannot_self_report_pass(self) -> None:
        result = valid_result()
        trial = next(
            trial for trial in result["trials"] if trial["case_id"] == "model-independent-plans"
        )
        trial["observed"]["signals"] = ["single_plan_then_critique"]
        trial["observed"]["violations"] = ["codex_critique_only"]
        errors = eval_kit.validate_result(result)
        self.assertTrue(any("expected 'fail'" in error for error in errors))

    def test_config_inferred_availability_cannot_self_report_pass(self) -> None:
        result = valid_result()
        trial = next(
            trial for trial in result["trials"]
            if trial["case_id"] == "complex-planning-delegates-qualified-reasoning"
        )
        # Every positive routing signal can hold while availability is invented.
        trial["observed"]["violations"] = [
            "seat_availability_claimed_from_configuration_without_verification"
        ]
        self.assertTrue(any(
            "expected 'fail'" in error for error in eval_kit.validate_result(result)
        ))
        scored = eval_kit.score_result(result)
        observed = next(
            item for item in scored["trials"] if item["case_id"] == trial["case_id"]
        )
        self.assertEqual("fail", observed["status"])

    def test_missing_trace_and_grader_material_block(self) -> None:
        result = valid_result()
        result["trials"][0]["validity_flags"] = ["missing_trace"]
        self.assertTrue(
            any(
                "expected 'fail'" in error
                for error in eval_kit.validate_result(result)
            )
        )

    def test_promotion_requires_full_suite_and_identified_human(self) -> None:
        canary = valid_result("canary")
        self.assertTrue(
            any(
                "requires the full suite" in error
                for error in eval_kit.validate_result(canary, require_approval=True)
            )
        )
        full = valid_result("full")
        full["human_approval"]["reviewer"] = ""
        full["human_approval"]["reviewed_at"] = ""
        errors = eval_kit.validate_result(full, require_approval=True)
        self.assertTrue(any("human_approval.reviewer" in error for error in errors))
        self.assertTrue(any("human_approval.reviewed_at" in error for error in errors))

    def test_promotion_rejects_hard_failure_and_not_run(self) -> None:
        failed = valid_result("full")
        failed["trials"][0]["observed"]["signals"] = []
        failed["trials"][0]["status"] = "fail"
        cases = {case["id"]: case for case in eval_kit.suite_documents()[1]["cases"]}
        failed["summary"] = eval_kit.expected_summary(failed["trials"], cases)
        self.assertTrue(
            any(
                "every hard-linked trial to pass" in error
                for error in eval_kit.validate_result(failed, require_approval=True)
            )
        )

        not_run = valid_result("full")
        trial = not_run["trials"][0]
        trial["outcome"] = "not_run"
        trial["status"] = "not_run"
        trial["not_run_reason"] = "harness unavailable"
        not_run["summary"] = eval_kit.expected_summary(not_run["trials"], cases)
        self.assertTrue(
            any(
                "does not allow error or not_run" in error
                for error in eval_kit.validate_result(not_run, require_approval=True)
            )
        )

    def test_unknown_validity_flag_and_any_violation_block(self) -> None:
        result = valid_result()
        result["trials"][0]["validity_flags"] = ["missing-trace"]
        errors = eval_kit.validate_result(result)
        self.assertTrue(any("unknown values" in error for error in errors))
        self.assertTrue(any("expected 'fail'" in error for error in errors))

        result = valid_result()
        result["trials"][0]["observed"]["violations"] = ["unexpected_violation"]
        self.assertTrue(
            any("expected 'fail'" in error for error in eval_kit.validate_result(result))
        )

    def test_empty_harness_metadata_and_undigested_evidence_fail(self) -> None:
        result = valid_result()
        result["system"]["model"] = ""
        result["system"]["settings_digest"] = "not-a-digest"
        result["trials"][0]["evidence"][0].pop("digest")
        errors = eval_kit.validate_result(result)
        self.assertTrue(any("system.model" in error for error in errors))
        self.assertTrue(any("settings_digest" in error for error in errors))
        self.assertTrue(any("expected 'fail'" in error for error in errors))

    def test_comparison_blocks_any_case_regression(self) -> None:
        baseline = valid_result()
        candidate = copy.deepcopy(baseline)
        candidate["run_id"] = "candidate"
        candidate["trials"][0]["status"] = "fail"
        candidate["trials"][0]["observed"]["signals"] = []
        cases = {case["id"]: case for case in eval_kit.suite_documents()[1]["cases"]}
        candidate["summary"] = eval_kit.expected_summary(candidate["trials"], cases)
        report, promotable = eval_kit.compare_results(baseline, candidate)
        self.assertFalse(promotable)
        self.assertIn("Promotion blocked", report)

    def test_strict_effort_comparison_accepts_only_effort_drift(self) -> None:
        baseline, candidate = strict_effort_pair()
        report, promotable = eval_kit.compare_results(
            baseline, candidate, variable="system.effort"
        )
        self.assertTrue(promotable)
        self.assertIn("Declared variable: `system.effort`", report)
        self.assertIn("## Duration ms distributions", report)
        self.assertIn("## Tokens distributions", report)
        self.assertIn("**All trials**", report)

    def test_strict_comparison_rejects_other_field_and_peer_drift(self) -> None:
        baseline, candidate = strict_effort_pair()
        candidate["system"]["model"] = "other-model"
        candidate["system"]["observed"]["model"] = "other-model"
        with self.assertRaisesRegex(eval_kit.EvalError, "outside the declared"):
            eval_kit.compare_results(baseline, candidate, variable="system.effort")

        baseline, candidate = strict_effort_pair()
        candidate["system"]["peer_seats"][0]["effort"] = "high"
        with self.assertRaisesRegex(eval_kit.EvalError, "outside the declared"):
            eval_kit.compare_results(baseline, candidate, variable="system.effort")

    def test_strict_comparison_requires_observed_requested_match(self) -> None:
        baseline, candidate = strict_effort_pair()
        candidate["system"]["observed"]["effort"] = "medium"
        candidate["trials"][0]["validity_flags"] = ["harness_context_mismatch"]
        candidate["trials"][0]["status"] = "fail"
        cases = {case["id"]: case for case in eval_kit.suite_documents()[1]["cases"]}
        candidate["summary"] = eval_kit.expected_summary(candidate["trials"], cases)
        with self.assertRaises(eval_kit.EvalError):
            eval_kit.compare_results(baseline, candidate, variable="system.effort")

    def test_strict_comparison_reports_and_blocks_any_regression(self) -> None:
        baseline, candidate = strict_effort_pair()
        candidate["trials"][0]["observed"]["signals"] = []
        candidate["trials"][0]["status"] = "fail"
        cases = {case["id"]: case for case in eval_kit.suite_documents()[1]["cases"]}
        candidate["summary"] = eval_kit.expected_summary(candidate["trials"], cases)
        report, promotable = eval_kit.compare_results(
            baseline, candidate, variable="system.effort"
        )
        self.assertFalse(promotable)
        self.assertIn("Promotion blocked by regressions", report)
        self.assertIn("Promotion validation blockers", report)

    def test_score_recomputes_without_mutating_input(self) -> None:
        raw = valid_result()
        raw["trials"][0]["status"] = "fail"
        raw["summary"] = {}
        scored = eval_kit.score_result(raw)
        self.assertEqual("pass", scored["trials"][0]["status"])
        self.assertEqual(
            eval_kit.expected_summary(
                scored["trials"],
                {case["id"]: case for case in eval_kit.suite_documents()[1]["cases"]},
            ),
            scored["summary"],
        )
        self.assertEqual("fail", raw["trials"][0]["status"])

    def test_score_cli_output_and_in_place_are_explicit(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            source = root / "raw.json"
            output = root / "scored.json"
            raw = valid_result()
            raw["trials"][0]["status"] = "fail"
            raw["summary"] = {}
            eval_kit.write_json(source, raw)
            command = [sys.executable, str(MODULE_PATH), "score", str(source)]

            printed = subprocess.run(command, check=True, capture_output=True, text=True)
            self.assertEqual("fail", eval_kit.load_json(source)["trials"][0]["status"])
            self.assertEqual("pass", json.loads(printed.stdout)["trials"][0]["status"])

            subprocess.run(command + ["--output", str(output)], check=True)
            self.assertEqual("pass", eval_kit.load_json(output)["trials"][0]["status"])
            aliased = subprocess.run(
                command + ["--output", str(source)], capture_output=True, text=True
            )
            self.assertEqual(2, aliased.returncode)
            self.assertIn("use --in-place", aliased.stderr)

            symlink = root / "linked.json"
            symlink.symlink_to(output)
            linked = subprocess.run(
                command + ["--output", str(symlink)], capture_output=True, text=True
            )
            self.assertEqual(2, linked.returncode)
            self.assertIn("refusing symlinked JSON output", linked.stderr)

            subprocess.run(command + ["--in-place"], check=True)
            self.assertEqual("pass", eval_kit.load_json(source)["trials"][0]["status"])


class CleanupSafetyTests(unittest.TestCase):
    def test_grader_material_and_route_are_scrubbed(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            row = eval_kit.EVALUATION_ROUTE_PREFIX + " hidden evaluator\n"
            agents = "| Intent | Use |\n" + row + "| Mechanics | codeflow |\n"
            (root / ".agents/skills/cf-evaluate-model").mkdir(parents=True)
            (root / ".agents/skills/cf-evaluate-model/SKILL.md").write_text(
                "grader", encoding="utf-8"
            )
            (root / ".codeflow/.baseline").mkdir(parents=True)
            (root / "AGENTS.md").write_text(agents, encoding="utf-8")
            baseline = root / ".codeflow/.baseline/AGENTS.md"
            baseline.write_text(agents, encoding="utf-8")
            eval_kit.write_json(
                root / ".codeflow/manifest.json",
                {
                    "files": {
                        "AGENTS.md": {"sha256": "old"},
                        ".agents/skills/cf-evaluate-model/SKILL.md": {
                            "sha256": "grader"
                        },
                    }
                },
            )

            eval_kit.remove_grader_material(root)

            self.assertNotIn(eval_kit.EVALUATION_ROUTE_PREFIX, (root / "AGENTS.md").read_text())
            self.assertNotIn(eval_kit.EVALUATION_ROUTE_PREFIX, baseline.read_text())
            manifest = eval_kit.load_json(root / ".codeflow/manifest.json")
            self.assertNotIn(
                ".agents/skills/cf-evaluate-model/SKILL.md", manifest["files"]
            )
            self.assertEqual(
                hashlib.sha256(baseline.read_bytes()).hexdigest(),
                manifest["files"]["AGENTS.md"]["sha256"],
            )

    def test_scrub_accepts_a_provably_pre_evaluator_scaffold(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / ".codeflow/.baseline").mkdir(parents=True)
            agents = "| Intent | Use |\n| Mechanics | codeflow |\n"
            (root / "AGENTS.md").write_text(agents, encoding="utf-8")
            baseline = root / ".codeflow/.baseline/AGENTS.md"
            baseline.write_text(agents, encoding="utf-8")
            eval_kit.write_json(
                root / ".codeflow/manifest.json",
                {"files": {"AGENTS.md": {"sha256": "old"}}},
            )
            eval_kit.remove_grader_material(root)
            manifest = eval_kit.load_json(root / ".codeflow/manifest.json")
            self.assertEqual(
                hashlib.sha256(baseline.read_bytes()).hexdigest(),
                manifest["files"]["AGENTS.md"]["sha256"],
            )

    def test_cleanup_requires_marker_and_exact_run_id(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            unmarked = Path(temp) / "unmarked"
            unmarked.mkdir()
            with self.assertRaises(eval_kit.EvalError):
                eval_kit.cleanup_run(unmarked, "anything")

            marked = Path(temp) / "marked"
            marked.mkdir()
            marker = {"schema_version": 1, "run_id": "expected"}
            eval_kit.write_json(marked / eval_kit.RUN_MARKER, marker)
            with self.assertRaises(eval_kit.EvalError):
                eval_kit.cleanup_run(marked, "wrong")
            eval_kit.cleanup_run(marked, "expected")
            self.assertFalse(marked.exists())


if __name__ == "__main__":
    unittest.main()
