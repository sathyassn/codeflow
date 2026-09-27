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
        self.last: str | None = None
        self.hook("SessionStart:startup")

    def add(self, entry: dict) -> "Transcript":
        """Append an entry chained to the one before, as Claude Code writes them:
        a compact boundary starts a new chain and keeps a logical parent."""
        entry.setdefault("sessionId", self.session_id)
        entry.setdefault("isSidechain", False)
        entry.setdefault("uuid", f"u{len(self.entries)}")
        if entry.get("subtype") == "compact_boundary":
            entry.setdefault("parentUuid", None)
            entry.setdefault("logicalParentUuid", self.last)
        else:
            entry.setdefault("parentUuid", self.last)
        self.entries.append(entry)
        self.last = entry["uuid"]
        return self

    def prompt_uuid(self, text: str) -> str:
        return next(
            entry["uuid"] for entry in reversed(self.entries)
            if entry.get("type") == "user" and entry["message"]["content"] == text
        )

    def hook(self, name: str, command: str = "codeflow hook session-orient",
             stdout: str = "") -> "Transcript":
        return self.add(
            {
                "type": "attachment",
                "attachment": {
                    "type": "hook_success",
                    "hookName": name,
                    "hookEvent": name.split(":")[0],
                    "command": command,
                    "exitCode": 0,
                    "stdout": stdout,
                },
            }
        )

    def typed(self, text: str, reminder: str = "") -> "Transcript":
        self.add({"type": "user", "message": {"role": "user", "content": text}})
        return self.hook("UserPromptSubmit", "codeflow hook rule-reminder", reminder)

    def reply(self, text: str, *tools: dict, parent: str | None = None) -> "Transcript":
        content = [{"type": "tool_use", "id": f"t{len(self.entries)}", **tool} for tool in tools]
        content.append({"type": "text", "text": text})
        entry = {"type": "assistant", "message": {"role": "assistant", "content": content}}
        if parent is not None:
            entry["parentUuid"] = parent
        self.add(entry)
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
    plan = plan_for(case)
    return {
        "case_id": case["id"],
        "trial": 1,
        "session": plan,
        "session_digest": eval_kit.canonical_digest(plan),
    }


def warmup(case: dict, *, compact_at: int = 7, hook: bool = True,
           answered: int | None = None) -> Transcript:
    """The warm-up of a faithful after-compaction session, with the first
    `answered` turns answered (all of them by default)."""
    plan = plan_for(case)
    transcript = Transcript()
    for number, turn in enumerate(plan["warmup"], 1):
        transcript.typed(turn)
        if answered is not None and number > answered:
            continue
        # Warm-up work that must never reach the grader.
        transcript.reply(
            "Onboarding and notifications led this week; see the cf-present notes.",
            {"name": "Read", "input": {"file_path": f"archive/week-{number:02d}.md"}},
        )
        if number == compact_at:
            transcript.compact(hook=hook)
    return transcript


def full_session(case: dict, probe_reply: str, *probe_tools: dict, compact_at: int = 7,
                 hook: bool = True) -> Transcript:
    """A faithful after-compaction session: warm-up, auto compaction, probe."""
    transcript = warmup(case, compact_at=compact_at, hook=hook)
    transcript.typed(plan_for(case)["probe_prompt"])
    return transcript.reply(probe_reply, *probe_tools)


def session_check(case: dict, number: int, reminders: tuple[str, ...] = ()) -> dict:
    """A check-session output for one trial, bound to the case's own plan."""
    plan = plan_for(case)
    excerpt = {
        "case_id": case["id"],
        "arm": plan["arm"],
        "probe_prompt": plan["probe_prompt"],
        "text": f"reply {number}",
        "tool_uses": [],
        "prompt_hooks": ["UserPromptSubmit"],
        "prompt_reminders": [{"hook": "UserPromptSubmit", "stdout": text} for text in reminders],
    }
    return {
        "valid": True,
        "validity_flags": [],
        "session_digest": eval_kit.canonical_digest(plan),
        "probe_excerpt": excerpt,
        "probe_excerpt_digest": eval_kit.canonical_digest(excerpt),
    }


def scripted_trial(case: dict, signals: list[str], number: int = 1,
                   reminders: tuple[str, ...] = (), **changes) -> dict:
    check = session_check(case, number, reminders)
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
        "evidence": [{"kind": "session", "ref": f"s/{case['id']}/{number}",
                      "digest": check["probe_excerpt_digest"]}],
        "session_check": check,
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

    def test_the_explanation_probe_needs_cf_present_on_the_claude_host(self) -> None:
        # Fable 1: no escape clause that no trial field backs.
        _, fixtures = documents()
        for arm in eval_kit.SCRIPTED_ARMS:
            case = case_for("complex-explanation-spontaneous-present", arm)
            self.assertEqual(["claude"], case["hosts"])
            self.assertIn("cf_present_opened_or_offered_with_reason", case["expected"]["signals"])
        note = fixtures[case["fixture"]]["state"]["grading"]
        self.assertIn("a figure or prose in chat alone fails it", note)
        self.assertNotIn("cannot show a page", note)

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
            ("It lands in about a week.", (), ["human_calendar_duration"]),
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
             "no record of the re-injection hook"),
            (fresh, compacted_fresh, "broken_fixture", "the fresh arm compacted"),
            (case, Transcript("s").add({"type": "noise"}), "retry_contamination", "0 typed"),
        ]
        for subject, transcript, flag, problem in cases:
            with self.subTest(flag=flag, problem=problem):
                checked = self.check(subject, transcript)
                self.assertFalse(checked["valid"])
                self.assertIn(flag, checked["validity_flags"])
                self.assertTrue(any(problem in item for item in checked["problems"]), checked["problems"])

    def test_a_record_cannot_redefine_its_case(self) -> None:
        # R130-2: the checker grades the pack's own plan, bound by digest.
        case = case_for("estimate-volunteered-in-plan", "after-compaction")
        fresh = case_for("estimate-volunteered-in-plan", "fresh")
        as_fresh = record_for(case)
        as_fresh["session"] = plan_for(fresh)
        as_fresh["session_digest"] = eval_kit.canonical_digest(as_fresh["session"])
        warmup_as_probe = record_for(case)
        warmup_as_probe["session"]["probe_prompt"] = warmup_as_probe["session"]["warmup"][0]
        undigested = record_for(case)
        del undigested["session_digest"]
        stale = record_for(case)
        stale["session_digest"] = "sha256:" + "0" * 64
        transcript = Transcript().typed(fresh["prompt"]).reply("ok")
        for label, record in [("fresh plan", as_fresh), ("warm-up as probe", warmup_as_probe),
                              ("no digest", undigested), ("stale digest", stale)]:
            with self.subTest(label), tempfile.TemporaryDirectory() as temp:
                with self.assertRaisesRegex(eval_kit.EvalError, "not the case's own plan"):
                    eval_kit.check_session(record, transcript.write(Path(temp)))

    def test_a_reply_is_bound_to_its_own_turn(self) -> None:
        # R130-3: a late warm-up answer is not the probe's reply.
        case = case_for("complex-explanation-spontaneous-present", "after-compaction")
        plan = plan_for(case)
        late = warmup(case, answered=len(plan["warmup"]) - 1)
        last_warmup = late.prompt_uuid(plan["warmup"][-1])
        late.typed(plan["probe_prompt"]).reply(
            "Here is the page.", {"name": "Skill", "input": {"skill": "cf-present"}},
            parent=last_warmup,
        )
        checked = self.check(case, late)
        self.assertFalse(checked["valid"])
        self.assertIn("retry_contamination", checked["validity_flags"])
        self.assertTrue(any("still running" in item for item in checked["problems"]), checked["problems"])
        self.assertEqual([], checked["probe_excerpt"]["tool_uses"])
        self.assertEqual([], checked["detected_signals"])

        unanswered_warmup = warmup(case, answered=len(plan["warmup"]) - 1)
        unanswered_warmup.typed(plan["probe_prompt"]).reply("ok")
        checked = self.check(case, unanswered_warmup)
        self.assertIn("retry_contamination", checked["validity_flags"])
        self.assertTrue(any("had not finished" in item for item in checked["problems"]), checked["problems"])

        no_reply = warmup(case).typed(plan["probe_prompt"])
        checked = self.check(case, no_reply)
        self.assertFalse(checked["valid"])
        self.assertIn("missing_trace", checked["validity_flags"])
        self.assertTrue(any("probe turn has no finished reply" in item for item in checked["problems"]))

        unbound = full_session(case, "ok")
        unbound.entries[-1]["parentUuid"] = None
        checked = self.check(case, unbound)
        self.assertIn("missing_trace", checked["validity_flags"])

    def test_one_native_session_identity_is_required(self) -> None:
        # R130-3: no session identity is not one session.
        case = case_for("status-outcomes-first", "fresh")
        anonymous = Transcript().typed(case["prompt"]).reply("Refunds no longer charge twice.")
        for entry in anonymous.entries:
            del entry["sessionId"]
        checked = self.check(case, anonymous)
        self.assertFalse(checked["valid"])
        self.assertIn("missing_trace", checked["validity_flags"])
        self.assertTrue(any("native session identity" in item for item in checked["problems"]))
        unnamed = Transcript().typed(case["prompt"]).reply("Refunds no longer charge twice.")
        del unnamed.entries[-1]["uuid"]
        self.assertIn("missing_trace", self.check(case, unnamed)["validity_flags"])

    def test_every_compaction_needs_its_reinjection(self) -> None:
        # R130-4: an earlier hook does not cover a later compaction.
        case = case_for("estimate-volunteered-in-plan", "after-compaction")
        plan = plan_for(case)
        for hook, valid in [(False, False), (True, True)]:
            with self.subTest(second_hook=hook):
                transcript = warmup(case)
                transcript.compact(hook=hook)
                transcript.typed(plan["probe_prompt"]).reply("ok")
                checked = self.check(case, transcript)
                self.assertEqual(valid, checked["valid"], checked["problems"])
                self.assertEqual(2, checked["compaction"]["boundaries"])
                if not valid:
                    self.assertEqual(["harness_context_mismatch"], checked["validity_flags"])
                    self.assertIn("no record of the re-injection hook after compaction 2 of 2",
                                  checked["problems"])

    def test_the_probe_turn_reminder_is_recorded(self) -> None:
        # Fable 2: guidance a prompt hook adds on the probe turn is evidence.
        case = case_for("status-outcomes-first", "fresh")
        reminder = "Status: lead each item with its outcome in words."
        checked = self.check(case, Transcript().typed(case["prompt"], reminder).reply("ok"))
        self.assertTrue(checked["valid"], checked["problems"])
        self.assertEqual([{"hook": "UserPromptSubmit", "stdout": reminder}],
                         checked["probe_excerpt"]["prompt_reminders"])
        quiet = self.check(case, Transcript().typed(case["prompt"]).reply("ok"))
        self.assertEqual([], quiet["probe_excerpt"]["prompt_reminders"])

    def test_the_ticket_negative_catches_an_over_applied_status_rule(self) -> None:
        # Fable 6: the negative asks for identifiers, so over-applying fails it.
        case = case_for("ticket-number-answered-directly", "fresh")
        self.assertNotIn("PAY-", case["prompt"])
        for reply, expected in [
            ("- PAY-219: blocked on the provider's sandbox secret\n- PAY-220: depends on PAY-216",
             ["asked_identifiers_given"]),
            ("Forged webhooks stay unverified until the provider rotates its secret (PAY-219).", []),
        ]:
            with self.subTest(reply=reply):
                checked = self.check(case, Transcript().typed(case["prompt"]).reply(reply))
                self.assertEqual(expected, checked["detected_signals"])
        self.assertIn("identifiers_behind_outcome_prose", case["expected"]["must_not"])

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

    def test_a_retry_never_replaces_an_original_trial(self) -> None:
        # R130-1: every started trial keeps its grade; retries only add.
        target = case_for("estimate-volunteered-in-plan", "after-compaction")
        trials = passing_trials()
        original = next(t for t in trials if t["case_id"] == target["id"] and t["trial"] == 1)
        original["observed"]["signals"] = []
        retry = scripted_trial(target, target["expected"]["signals"], 4)
        dropped = [trial for trial in trials if trial is not original] + [retry]
        report, met = eval_kit.retention_report({"trials": dropped}, PACK)
        self.assertFalse(met, report)
        self.assertIn(f"- {target['id']}: original trial 1 is missing", report)
        report, met = eval_kit.retention_report({"trials": trials + [retry]}, PACK)
        self.assertFalse(met, report)
        self.assertIn("estimate-volunteered-in-plan (hard), after-compaction: 3 of 4 passed", report)
        self.assertIn(f"- {target['id']} trial 1: fail", report)

    def test_one_native_session_counts_as_one_trial(self) -> None:
        # R130-1: repetitions need separate native sessions.
        trials = passing_trials()
        target = case_for("status-outcomes-first", "fresh")["id"]
        for trial in trials:
            if trial["case_id"] == target:
                trial["trace_ref"] = "t/one-session"
        with self.assertRaisesRegex(eval_kit.EvalError, "reuses the native session"):
            eval_kit.retention_report({"trials": trials}, PACK)
        trials = passing_trials()
        shared = [trial for trial in trials if trial["case_id"] == target]
        shared[1]["evidence"] = copy.deepcopy(shared[0]["evidence"])
        with self.assertRaisesRegex(eval_kit.EvalError, "reuses the native session"):
            eval_kit.retention_report({"trials": trials}, PACK)

    def test_a_scored_trial_carries_its_check_session_output(self) -> None:
        # Fable 4 and R130-2: the grade is bound to the deterministic check.
        target = case_for("status-outcomes-first", "after-compaction")

        def refused(mutate, pattern: str) -> None:
            trials = passing_trials()
            mutate(next(trial for trial in trials if trial["case_id"] == target["id"]))
            with self.assertRaisesRegex(eval_kit.EvalError, pattern):
                eval_kit.retention_report({"trials": trials}, PACK)

        refused(lambda trial: trial.pop("session_check"), "carries no check-session output")
        refused(lambda trial: trial["session_check"]["probe_excerpt"].update(text="edited"),
                "does not match its digest")
        refused(lambda trial: trial.update(session_check=session_check(
            case_for("status-outcomes-first", "fresh"), 1)), "another case or plan")
        refused(lambda trial: trial["evidence"][0].update(digest="sha256:" + "c" * 64),
                "no session evidence cites")
        refused(lambda trial: trial["session_check"].update(validity_flags=["broken_fixture"]),
                "drops the check-session validity flags: broken_fixture")
        errored = passing_trials()
        errored[0].update(outcome="error", error_message="seat unavailable")
        errored[0].pop("session_check")
        _, met = eval_kit.retention_report({"trials": errored}, PACK)
        self.assertFalse(met)

    def test_a_real_check_session_output_binds_its_trial(self) -> None:
        case = case_for("status-outcomes-first", "fresh")
        reply = "- Refunds no longer charge twice (PAY-211)."
        with tempfile.TemporaryDirectory() as temp:
            checked = eval_kit.check_session(
                record_for(case), Transcript().typed(case["prompt"]).reply(reply).write(Path(temp))
            )
        trials = [trial for trial in passing_trials() if trial["case_id"] != case["id"] or trial["trial"] != 1]
        real = scripted_trial(case, case["expected"]["signals"], 1)
        real["session_check"] = checked
        real["evidence"][0]["digest"] = checked["probe_excerpt_digest"]
        report, met = eval_kit.retention_report({"trials": trials + [real]}, PACK)
        self.assertTrue(met, report)

    def test_reminder_assisted_lines_are_labelled(self) -> None:
        # Fable 2: a pass helped by a prompt reminder is not shown as retention.
        target = case_for("status-outcomes-first", "after-compaction")
        trials = [
            scripted_trial(target, target["expected"]["signals"], trial["trial"],
                           reminders=("Status: outcomes first.",))
            if trial["case_id"] == target["id"] else trial
            for trial in passing_trials()
        ]
        report, met = eval_kit.retention_report({"trials": trials}, PACK)
        self.assertTrue(met, report)
        self.assertIn("status-outcomes-first (hard), after-compaction: 3 of 3 passed; reminder-assisted",
                      report)
        self.assertIn("estimate-volunteered-in-plan (hard), after-compaction: 3 of 3 passed\n", report)
        self.assertIn("Reminder-assisted: a prompt hook added guidance on the probe turn itself", report)

    def test_trial_numbers_are_positive_integers(self) -> None:
        for number in ["1", 0, True, 1.0]:
            trials = passing_trials()
            trials[0]["trial"] = number
            with self.subTest(number=number), self.assertRaisesRegex(
                eval_kit.EvalError, "trial must be a positive integer"
            ):
                eval_kit.retention_report({"trials": trials}, PACK)

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
