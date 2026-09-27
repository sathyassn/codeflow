#!/usr/bin/env python3
"""Offline checks for the scripted multi-turn kind and the retention pack.

These prove the schema, the materializer's arm handling, the deterministic
session check and the retention bar against synthetic transcripts. They say
nothing about live model behaviour; the live sessions are a separate run.
"""

from __future__ import annotations

import copy
import importlib.util
import json
from pathlib import Path
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

PACK = "guidance-retention"
# Words that would name the rule under test in a probe.
RULE_WORDS = {
    "estimate", "estimates", "cf-estimate", "agentic", "scenario", "scenarios",
    "week", "weeks", "sprint", "sprints", "person-days", "outcome", "outcomes",
    "id", "ids", "identifier", "identifiers", "figure", "diagram", "visual",
    "visually", "present", "cf-present", "page", "chat", "simple", "rule",
}


def documents() -> tuple[dict, dict]:
    _, cases_doc, fixtures_doc = eval_kit.suite_documents()
    cases = {case["id"]: case for case in cases_doc["cases"]}
    fixtures = {fixture["id"]: fixture for fixture in fixtures_doc["fixtures"]}
    return cases, fixtures


def pack_cases() -> dict[str, dict]:
    cases, _ = documents()
    return {case_id: cases[case_id] for case_id in eval_kit.resolve_pack(PACK)}


def case_for(probe: str, arm: str) -> dict:
    return next(
        case
        for case in pack_cases().values()
        if case["session"]["probe"] == probe and case["session"]["arm"] == arm
    )


class Transcript:
    """Build a synthetic Claude Code session transcript (JSON Lines)."""

    def __init__(self, session_id: str = "session-a") -> None:
        self.session_id = session_id
        self.entries: list[dict] = []
        self.hook("SessionStart:startup")

    def add(self, entry: dict) -> "Transcript":
        entry.setdefault("sessionId", self.session_id)
        entry.setdefault("isSidechain", False)
        self.entries.append(entry)
        return self

    def hook(self, name: str, command: str = "codeflow hook session-orient") -> "Transcript":
        return self.add(
            {
                "type": "attachment",
                "attachment": {
                    "type": "hook_success",
                    "hookName": name,
                    "hookEvent": name.split(":")[0],
                    "command": command,
                    "exitCode": 0,
                },
            }
        )

    def typed(self, text: str) -> "Transcript":
        self.add({"type": "user", "message": {"role": "user", "content": text}})
        return self.hook("UserPromptSubmit")

    def reply(self, text: str, *tools: dict) -> "Transcript":
        content = [{"type": "tool_use", "id": f"t{len(self.entries)}", **tool} for tool in tools]
        content.append({"type": "text", "text": text})
        self.add({"type": "assistant", "message": {"role": "assistant", "content": content}})
        for block in content[:-1]:
            self.add(
                {
                    "type": "user",
                    "message": {
                        "role": "user",
                        "content": [{"type": "tool_result", "tool_use_id": block["id"], "content": "ok"}],
                    },
                }
            )
        return self

    def compact(self, trigger: str = "auto", hook: bool = True) -> "Transcript":
        self.add(
            {
                "type": "system",
                "subtype": "compact_boundary",
                "compactMetadata": {"trigger": trigger, "preTokens": 101234},
            }
        )
        self.add(
            {
                "type": "user",
                "isCompactSummary": True,
                "message": {"role": "user", "content": "Summary of the earlier conversation."},
            }
        )
        return self.hook("SessionStart:compact") if hook else self

    def write(self, directory: Path) -> Path:
        path = directory / "session.jsonl"
        path.write_text(
            "".join(json.dumps(entry) + "\n" for entry in self.entries), encoding="utf-8"
        )
        return path


def plan_for(case: dict) -> dict:
    _, fixtures = documents()
    return eval_kit.session_plan(case, fixtures[case["fixture"]])


def record_for(case: dict) -> dict:
    return {"case_id": case["id"], "trial": 1, "session": plan_for(case)}


def full_session(case: dict, probe_reply: str, *probe_tools: dict, compact_at: int = 7,
                 hook: bool = True) -> Transcript:
    """A faithful after-compaction session: warm-up, auto compaction, probe."""
    plan = plan_for(case)
    transcript = Transcript()
    for number, turn in enumerate(plan["warmup"], 1):
        transcript.typed(turn)
        # Warm-up work that must never reach the grader.
        transcript.reply(
            "Onboarding and notifications led this week; see the cf-present notes.",
            {"name": "Read", "input": {"file_path": f"archive/week-{number:02d}.md"}},
        )
        if number == compact_at:
            transcript.compact(hook=hook)
    transcript.typed(plan["probe_prompt"])
    return transcript.reply(probe_reply, *probe_tools)


def scripted_trial(case: dict, signals: list[str], number: int = 1, **changes) -> dict:
    trial = {
        "case_id": case["id"],
        "trial": number,
        "fixture_digest": "sha256:" + "a" * 64,
        "outcome": "completed",
        "observed": {
            "route": case["expected"]["routes"][0],
            "signals": signals,
            "violations": [],
            "references": case["expected"]["references"],
        },
        "evidence": [{"kind": "session", "ref": f"s/{case['id']}/{number}", "digest": "sha256:" + "b" * 64}],
        "trace_ref": f"t/{case['id']}/{number}",
        "duration_ms": 1,
        "tokens": None,
        "cost": None,
        "validity_flags": [],
    }
    trial.update(changes)
    return trial


def passing_trials() -> list[dict]:
    trials = []
    for case in pack_cases().values():
        repetitions = 3 if case["session"]["gate"] == "hard" else 1
        for number in range(1, repetitions + 1):
            trials.append(scripted_trial(case, case["expected"]["signals"], number))
    return trials


class RetentionPackSchemaTests(unittest.TestCase):
    def test_shipped_suite_accepts_the_scripted_kind(self) -> None:
        self.assertEqual([], eval_kit.validate_suite(ROOT))
        cases, fixtures = documents()
        self.assertEqual([], eval_kit.scripted_case_errors(cases, fixtures))

    def test_pack_holds_three_hard_probes_each_with_a_paired_negative_in_both_arms(self) -> None:
        by_probe: dict[str, dict[str, dict]] = {}
        for case in pack_cases().values():
            session = case["session"]
            self.assertEqual("scripted-multi-turn", session["kind"])
            by_probe.setdefault(session["probe"], {})[session["arm"]] = case
        self.assertEqual(
            {
                "estimate-volunteered-in-plan": "hard",
                "status-outcomes-first": "hard",
                "complex-explanation-spontaneous-present": "hard",
                "milestone-features-stay-a-list": "paired-negative",
                "ticket-number-answered-directly": "paired-negative",
                "simple-question-stays-in-chat": "paired-negative",
            },
            {probe: arms["fresh"]["session"]["gate"] for probe, arms in by_probe.items()},
        )
        for probe, arms in by_probe.items():
            self.assertEqual({"fresh", "after-compaction"}, set(arms), probe)
        pairs = {
            arms["fresh"]["session"]["pairs_with"]
            for arms in by_probe.values()
            if arms["fresh"]["session"]["gate"] == "paired-negative"
        }
        self.assertEqual(
            {"estimate-volunteered-in-plan", "status-outcomes-first",
             "complex-explanation-spontaneous-present"},
            pairs,
        )

    def test_no_probe_names_the_rule_under_test(self) -> None:
        for case_id, case in pack_cases().items():
            words = {
                word.strip("?.,'\"`").lower()
                for word in case["prompt"].replace("'s", " ").split()
            }
            self.assertFalse(words & RULE_WORDS, f"{case_id}: {sorted(words & RULE_WORDS)}")

    def test_compaction_is_a_fixture_local_claude_setting(self) -> None:
        _, fixtures = documents()
        for case in pack_cases().values():
            self.assertEqual(["claude"], case["hosts"])
            script = fixtures[case["fixture"]]["script"]
            self.assertEqual(
                {"trigger": "auto", "env": {"CLAUDE_CODE_AUTO_COMPACT_WINDOW": "100000"}},
                script["compaction"],
            )
            self.assertGreaterEqual(len(script["warmup"]), 8)
            self.assertNotIn(".claude/settings.local.json", fixtures[case["fixture"]]["files"])

    def test_warmup_generator_is_one_shared_copy(self) -> None:
        _, fixtures = documents()
        copies = {
            fixtures[case["fixture"]]["files"]["tools/build_support_archive.py"]
            for case in pack_cases().values()
        }
        self.assertEqual(1, len(copies))

    def test_schema_rejects_malformed_scripted_cases(self) -> None:
        cases, fixtures = documents()
        hard = "retention-status-outcomes-first-after-compaction"
        negative = "retention-ticket-number-answered-directly-fresh"
        fixture_id = cases[hard]["fixture"]

        def errors(mutate) -> str:
            mutated_cases, mutated_fixtures = copy.deepcopy(cases), copy.deepcopy(fixtures)
            mutate(mutated_cases, mutated_fixtures)
            return "\n".join(eval_kit.scripted_case_errors(mutated_cases, mutated_fixtures))

        checks = [
            (lambda c, f: c[hard]["session"].update(kind="scripted"), "session.kind"),
            (lambda c, f: c[hard]["session"].update(arm="deep"), "session.arm"),
            (lambda c, f: c[hard]["session"].update(gate="soft"), "session.gate"),
            (lambda c, f: c[hard]["session"].update(extra=True), "unknown session fields"),
            (lambda c, f: c[hard].update(prompt="How are things?"), "arms differ in prompt"),
            (lambda c, f: c[hard].update(hosts=["claude", "codex"]), "hosts"),
            (lambda c, f: c[negative]["session"].pop("pairs_with"), "pairs_with"),
            (lambda c, f: c[hard]["session"].update(detectors={"x": {"text": "("}}), "not a valid pattern"),
            (lambda c, f: c[hard]["session"].update(detectors={"x": {"regex": "a"}}), "uses keys"),
            (lambda c, f: c.pop(hard), "lacks arms: after-compaction"),
            (lambda c, f: f[fixture_id].pop("script"), "script.warmup and script.compaction"),
            (lambda c, f: f[fixture_id]["script"].update(warmup=[]), "script.warmup"),
            (
                lambda c, f: f[fixture_id]["script"]["compaction"]["env"].update(
                    CLAUDE_CODE_AUTO_COMPACT_WINDOW="50000"
                ),
                "100000 to 1000000",
            ),
            (
                lambda c, f: f[fixture_id]["script"]["compaction"]["env"].update(
                    CLAUDE_CODE_AUTO_COMPACT_WINDOW="100k"
                ),
                "plain integer",
            ),
            (
                lambda c, f: f[fixture_id]["script"]["compaction"]["env"].update(ANTHROPIC_MODEL="x"),
                "must set exactly",
            ),
            (lambda c, f: f[fixture_id]["script"]["compaction"].update(trigger="manual"), "trigger must be auto"),
            (
                lambda c, f: f[fixture_id]["files"].update({".claude/settings.local.json": "{}"}),
                "materializer owns",
            ),
            (
                lambda c, f: f[fixture_id]["script"]["warmup"].append(c[hard]["prompt"]),
                "probe repeats a warm-up turn",
            ),
        ]
        for mutate, expected in checks:
            with self.subTest(expected=expected):
                self.assertIn(expected, errors(mutate))


class MaterializeScriptedTests(unittest.TestCase):
    def materialize(self, case_id: str, *, wired: bool = True) -> dict:
        codeflow = Path(sys.executable).resolve()
        real_run_command = eval_kit.run_command

        def scaffold_or_run(command: list[str], root: Path) -> None:
            # Only scaffold installation is doubled: a fresh standard scaffold
            # carries the rule map and the re-injection hooks when wired.
            if command == [str(codeflow), "init", "--yes", "--standard"]:
                agents = "# Project\n\n## Always rules\n\n## When you are about to\n"
                (root / "AGENTS.md").write_text(agents if wired else "# Project\n", encoding="utf-8")
                (root / ".gitignore").write_text(".claude/settings.local.json\n", encoding="utf-8")
                hook = [{"type": "command", "command": "codeflow hook session-orient"}]
                (root / ".claude").mkdir()
                (root / ".claude/settings.json").write_text(
                    json.dumps(
                        {
                            "hooks": {
                                "SessionStart": [
                                    {"matcher": "startup|resume|clear|compact|fork", "hooks": hook}
                                ],
                                "UserPromptSubmit": [{"hooks": hook}],
                            }
                        }
                    ),
                    encoding="utf-8",
                )
                return
            real_run_command(command, root)

        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        with patch.object(eval_kit, "run_command", side_effect=scaffold_or_run):
            return eval_kit.materialize(case_id, 1, Path(self.temp.name) / "run", codeflow)

    def test_arms_differ_only_in_the_fixture_local_window_and_the_plan(self) -> None:
        for case in pack_cases().values():
            with self.subTest(case=case["id"]):
                record = self.materialize(case["id"])
                root = Path(record["path"])
                plan = record["session"]
                self.assertFalse((root / "TASK.md").exists())
                self.assertEqual(case["prompt"], plan["probe_prompt"])
                self.assertEqual(
                    {"rule_map": True, "compact_reinjection": True, "prompt_reminder": True},
                    record["guidance_wiring"],
                )
                settings = root / ".claude/settings.local.json"
                subject_text = "\n".join(
                    path.read_text(encoding="utf-8", errors="replace")
                    for path in root.rglob("*")
                    if path.is_file() and ".git" not in path.parts
                )
                self.assertNotIn(case["prompt"], subject_text)
                if case["session"]["arm"] == "fresh":
                    self.assertEqual([], plan["warmup"])
                    self.assertIsNone(plan["compaction"])
                    self.assertFalse(settings.exists())
                else:
                    self.assertEqual(11, len(plan["warmup"]))
                    self.assertEqual(
                        {"env": {"CLAUDE_CODE_AUTO_COMPACT_WINDOW": "100000"}},
                        json.loads(settings.read_text(encoding="utf-8")),
                    )
                    for turn in plan["warmup"]:
                        self.assertNotIn(turn, subject_text)
                    tracked = eval_kit.subprocess.run(
                        ["git", "ls-files", ".claude/settings.local.json"],
                        cwd=root, check=True, capture_output=True, text=True,
                    ).stdout
                    self.assertEqual("", tracked, "the window stays out of fixture history")

    def test_scaffold_without_the_rule_map_is_refused(self) -> None:
        case_id = eval_kit.resolve_pack(PACK)[0]
        with self.assertRaisesRegex(eval_kit.EvalError, "lacks: rule_map"):
            self.materialize(case_id, wired=False)

    def test_single_turn_cases_still_get_task_md(self) -> None:
        case_id = eval_kit.resolve_pack("agentic-estimation")[0]
        record = self.materialize(case_id)
        self.assertTrue((Path(record["path"]) / "TASK.md").is_file())
        self.assertNotIn("session", record)


class CheckSessionTests(unittest.TestCase):
    def check(self, case: dict, transcript: Transcript) -> dict:
        with tempfile.TemporaryDirectory() as temp:
            return eval_kit.check_session(record_for(case), transcript.write(Path(temp)))

    def test_faithful_after_compaction_session_grades_only_the_probe(self) -> None:
        case = case_for("complex-explanation-spontaneous-present", "after-compaction")
        checked = self.check(
            case,
            full_session(
                case,
                "I opened a review page for the release order and the three blockers.",
                {"name": "Skill", "input": {"skill": "cf-present"}},
            ),
        )
        self.assertTrue(checked["valid"], checked["problems"])
        self.assertEqual({"observed": True, "trigger": "auto", "boundaries": 1, "pre_tokens": 101234},
                         checked["compaction"])
        excerpt = checked["probe_excerpt"]
        self.assertEqual([{"name": "Skill", "input": {"skill": "cf-present"}}], excerpt["tool_uses"])
        self.assertNotIn("Onboarding", excerpt["text"])
        self.assertEqual(["UserPromptSubmit"], excerpt["prompt_hooks"])
        self.assertEqual(["cf_present_opened"], checked["detected_signals"])
        self.assertEqual(eval_kit.canonical_digest(excerpt), checked["probe_excerpt_digest"])

    def test_warmup_tool_events_never_reach_the_detectors(self) -> None:
        case = case_for("simple-question-stays-in-chat", "after-compaction")
        transcript = full_session(case, "Run `sh scripts/check-release-notes.sh`.")
        transcript.entries[3]["message"]["content"].insert(
            0, {"type": "tool_use", "id": "w", "name": "Skill", "input": {"skill": "cf-present"}}
        )
        checked = self.check(case, transcript)
        self.assertTrue(checked["valid"], checked["problems"])
        self.assertEqual(["check_command_named"], checked["detected_signals"])

    def test_fresh_arm_sends_the_probe_as_turn_one(self) -> None:
        case = case_for("status-outcomes-first", "fresh")
        reply = "- **PAY-211**: idempotency keys\n- Refunds no longer charge twice (PAY-211)."
        checked = self.check(case, Transcript().typed(case["prompt"]).reply(reply))
        self.assertTrue(checked["valid"], checked["problems"])
        self.assertEqual({"observed": False}, checked["compaction"])
        self.assertEqual(["item_led_by_identifier"], checked["detected_signals"])

    def test_detectors_follow_their_patterns(self) -> None:
        case = case_for("estimate-volunteered-in-plan", "fresh")
        for reply, tools, expected in [
            ("It lands in about 3 weeks.", (), ["human_calendar_duration"]),
            ("Two sprints, like photos.", (), ["human_calendar_duration"]),
            ("Roughly 5 person-days, 3x faster with AI.", (),
             ["ai_speed_multiplier", "human_calendar_duration"]),
            ("Likely 6 to 9 agent hours plus store review, based on the queue size.",
             ({"name": "Skill", "input": {"skill": "cf-estimate"}},), ["cf_estimate_consulted"]),
            ("I read the method first.",
             ({"name": "Read", "input": {"file_path": ".claude/skills/cf-estimate/SKILL.md"}},),
             ["cf_estimate_consulted"]),
            ("Never in human weeks; see the scenarios below.", (), []),
        ]:
            with self.subTest(reply=reply):
                checked = self.check(case, Transcript().typed(case["prompt"]).reply(reply, *tools))
                self.assertEqual(expected, checked["detected_signals"])

    def test_structural_failures_name_their_validity_flag(self) -> None:
        case = case_for("estimate-volunteered-in-plan", "after-compaction")
        fresh = case_for("estimate-volunteered-in-plan", "fresh")
        plan = plan_for(case)
        no_compaction = full_session(case, "ok", compact_at=0)
        manual_entries = Transcript()
        for turn in plan["warmup"]:
            manual_entries.typed(turn).reply("done")
        manual_entries.typed("/compact").compact(trigger="manual")
        manual_entries.typed(plan["probe_prompt"]).reply("ok")
        extra = full_session(case, "ok")
        extra.typed("Please answer again, more briefly.").reply("ok")
        two_sessions = full_session(case, "ok")
        two_sessions.entries[-1]["sessionId"] = "session-b"
        compacted_fresh = Transcript().compact().typed(fresh["prompt"]).reply("ok")
        cases = [
            (case, no_compaction, "broken_fixture", "no automatic compaction"),
            (case, manual_entries, "retry_contamination", "typed turns do not match"),
            (case, manual_entries, "broken_fixture", "no automatic compaction"),
            (case, extra, "retry_contamination", "typed turns do not match"),
            (case, two_sessions, "reused_session", "2 sessions"),
            (case, full_session(case, "ok", hook=False), "harness_context_mismatch",
             "re-injection hook did not run"),
            (fresh, compacted_fresh, "broken_fixture", "the fresh arm compacted"),
            (case, Transcript("s").add({"type": "noise"}), "retry_contamination", "0 typed"),
        ]
        for subject, transcript, flag, problem in cases:
            with self.subTest(flag=flag, problem=problem):
                checked = self.check(subject, transcript)
                self.assertFalse(checked["valid"])
                self.assertIn(flag, checked["validity_flags"])
                self.assertTrue(any(problem in item for item in checked["problems"]), checked["problems"])

    def test_missing_or_unreadable_transcript_is_missing_trace(self) -> None:
        case = case_for("status-outcomes-first", "fresh")
        with tempfile.TemporaryDirectory() as temp:
            checked = eval_kit.check_session(record_for(case), Path(temp) / "absent.jsonl")
            self.assertEqual(["missing_trace"], checked["validity_flags"])
            empty = Path(temp) / "empty.jsonl"
            empty.write_text("", encoding="utf-8")
            self.assertEqual(
                ["missing_trace"], eval_kit.check_session(record_for(case), empty)["validity_flags"]
            )

    def test_record_without_a_plan_is_refused(self) -> None:
        with self.assertRaisesRegex(eval_kit.EvalError, "no scripted session plan"):
            eval_kit.check_session({"case_id": "x"}, Path("unused.jsonl"))

    def test_check_session_cli_writes_the_result_outside_the_fixture(self) -> None:
        case = case_for("status-outcomes-first", "fresh")
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp)
            record = directory / "record.json"
            record.write_text(json.dumps(record_for(case)), encoding="utf-8")
            transcript = Transcript().typed(case["prompt"]).reply("Refunds no longer charge twice.")
            output = directory / "checked.json"
            completed = eval_kit.subprocess.run(
                [sys.executable, "-B", str(MODULE_PATH), "check-session", "--record", str(record),
                 "--transcript", str(transcript.write(directory)), "--output", str(output)],
                capture_output=True, text=True, check=False,
            )
            self.assertEqual(0, completed.returncode, completed.stderr)
            self.assertTrue(json.loads(output.read_text(encoding="utf-8"))["valid"])


class RetentionReportTests(unittest.TestCase):
    def test_every_trial_passing_in_both_arms_meets_the_bar(self) -> None:
        report, met = eval_kit.retention_report({"trials": passing_trials()}, PACK)
        self.assertTrue(met, report)
        self.assertIn("estimate-volunteered-in-plan (hard), after-compaction: 3 of 3 passed", report)
        self.assertIn("simple-question-stays-in-chat (paired-negative), fresh: 1 of 1 passed", report)
        self.assertTrue(report.endswith("Retention bar: met\n"))

    def test_a_hard_failure_after_compaction_fails_the_bar_and_is_listed(self) -> None:
        trials = passing_trials()
        target = case_for("status-outcomes-first", "after-compaction")["id"]
        failing = next(trial for trial in trials if trial["case_id"] == target)
        failing["observed"]["signals"] = ["item_led_by_identifier", "items_led_by_outcome_in_words"]
        report, met = eval_kit.retention_report({"trials": trials}, PACK)
        self.assertFalse(met)
        self.assertIn("status-outcomes-first: adherence after compaction is lower than fresh", report)
        self.assertIn(f"- {target} trial 1: fail", report)

    def test_statuses_are_recomputed_and_too_few_trials_fail(self) -> None:
        trials = passing_trials()
        hard = case_for("estimate-volunteered-in-plan", "fresh")["id"]
        claimed = next(trial for trial in trials if trial["case_id"] == hard)
        claimed["status"] = "pass"
        claimed["observed"]["signals"] = []
        negative = case_for("ticket-number-answered-directly", "after-compaction")["id"]
        trials = [trial for trial in trials if trial["case_id"] != negative]
        report, met = eval_kit.retention_report({"trials": trials}, PACK)
        self.assertFalse(met)
        self.assertIn("estimate-volunteered-in-plan (hard), fresh: 2 of 3 passed", report)
        self.assertIn(
            "ticket-number-answered-directly (paired-negative), after-compaction: 0 of 0 passed",
            report,
        )

    def test_a_flagged_trial_never_counts_as_a_pass(self) -> None:
        trials = passing_trials()
        trials[0]["validity_flags"] = ["broken_fixture"]
        _, met = eval_kit.retention_report({"trials": trials}, PACK)
        self.assertFalse(met)

    def test_trials_outside_the_pack_are_refused(self) -> None:
        with self.assertRaisesRegex(eval_kit.EvalError, "is not a case of"):
            eval_kit.retention_report(
                {"trials": [{"case_id": "model-independent-plans", "trial": 1}]}, PACK
            )
        with self.assertRaisesRegex(eval_kit.EvalError, "repeats"):
            eval_kit.retention_report({"trials": passing_trials() + passing_trials()[:1]}, PACK)
        with self.assertRaisesRegex(eval_kit.EvalError, "is not a scripted case"):
            eval_kit.retention_report({"trials": []}, "orchestration-core")


if __name__ == "__main__":
    unittest.main()
