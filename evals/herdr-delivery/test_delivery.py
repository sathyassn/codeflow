"""Prove the Herdr armed-prompt recipe is executable, not documentation-only.

The stub cases drive cf-herdr's delivery script against a stand-in `herdr`
whose replies follow the bundled API schema (`herdr api schema`, herdr
0.9.0): `pane get` answers `.result.pane`, `agent get` answers
`.result.agent` with `agent_status` and `state_change_seq`. Point
`CF_HERDR_SKILL_DIR` at an installed `cf-herdr` to test that copy.
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
import unittest
import uuid
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SKILL_DIR = Path(
    os.environ.get("CF_HERDR_SKILL_DIR") or ROOT / "assets/base/agents/skills/cf-herdr"
)
HERDR_SKILL = SKILL_DIR / "SKILL.md"
DELIVER = SKILL_DIR / "scripts/deliver.py"
PANE = "w1:p7"


def herdr(*args: str) -> dict:
    raw = subprocess.check_output(["herdr", *args], text=True)
    return json.loads(raw)


def words(text: str) -> str:
    return " ".join(text.split())


STUB = r"""
import json, os, sys

path = os.environ["HERDR_STUB_STATE"]
with open(path, encoding="utf-8") as handle:
    state = json.load(handle)
args = sys.argv[1:]
state["calls"].append(args)
pane = state["pane"]
code = 0


def reply(result):
    print(json.dumps({"id": "stub", "result": result}))


def screen():
    # A live pane: what was submitted stays on screen above the input.
    return state["scrollback"] + state["input"]


verb = tuple(args[:2])
if verb == ("pane", "get"):
    reply({"type": "pane_info", "pane": pane})
elif verb == ("agent", "get"):
    reply({"type": "agent_info",
           "agent": dict(pane, state_change_seq=state["seq"])})
elif verb == ("pane", "send-text"):
    text = args[3]
    if state["fold_paste"] and text.count("\n") > 1:
        text = "[Pasted text #1 +%d lines]" % text.count("\n")
    state["input"] += text
    reply({"type": "ok"})
elif verb == ("pane", "send-keys"):
    if "Enter" in args[3:]:
        state["enters"] += 1
        start = state["start_on_enter"]
        if start is not None and state["enters"] >= start:
            pane["agent_status"] = state["started_status"]
            state["seq"] += 1
            state["scrollback"] += "> " + state["input"] + "\n"
            state["input"] = ""
        elif state["submit_on_enter"]:
            # Submitted, but the seat's status has not moved yet.
            state["scrollback"] += "> " + state["input"] + "\n"
            state["input"] = ""
        elif state["clear_on_enter"]:
            state["input"] = ""
    reply({"type": "ok"})
elif verb == ("pane", "read"):
    print(screen())
elif verb == ("pane", "wait-output"):
    needle = args[args.index("--match") + 1]
    if needle in screen():
        reply({"type": "wait_matched", "event": {}})
    else:
        sys.stderr.write(json.dumps({"error": {"code": "timeout"}}))
        code = 1
else:
    sys.stderr.write("stub herdr: unsupported " + " ".join(args))
    code = 2
with open(path, "w", encoding="utf-8") as handle:
    json.dump(state, handle)
sys.exit(code)
"""


class StubSeat:
    """A stand-in `herdr` on PATH and the seat state it answers from."""

    def __init__(self, tmp: Path, **scenario: object) -> None:
        self.tmp = tmp
        folder = tmp / "worktree"
        folder.mkdir()
        self.folder = folder
        bin_dir = tmp / "bin"
        bin_dir.mkdir()
        stub = bin_dir / "herdr"
        stub.write_text(f"#!{sys.executable}\n{STUB}", encoding="utf-8")
        stub.chmod(0o755)
        self.bin_dir = bin_dir
        self.state_path = tmp / "state.json"
        state = {
            "pane": {
                "pane_id": PANE,
                "cwd": str(folder),
                "foreground_cwd": str(folder),
                "agent_status": scenario.pop("status", "idle"),
            },
            "seq": 4,
            "scrollback": scenario.pop("scrollback", ""),
            "input": "",
            "enters": 0,
            "calls": [],
            "start_on_enter": scenario.pop("start_on_enter", 1),
            "started_status": scenario.pop("started_status", "working"),
            "clear_on_enter": scenario.pop("clear_on_enter", False),
            "submit_on_enter": scenario.pop("submit_on_enter", False),
            "fold_paste": scenario.pop("fold_paste", False),
        }
        assert not scenario, scenario
        self.state_path.write_text(json.dumps(state), encoding="utf-8")
        self.prompt = tmp / "prompt.md"
        self.prompt.write_text(
            "Review the diff.\nReply with the single word ok.\n", encoding="utf-8"
        )

    def deliver(self, *extra: str) -> subprocess.CompletedProcess[str]:
        env = dict(os.environ)
        env["PATH"] = f"{self.bin_dir}{os.pathsep}{env.get('PATH', '')}"
        env["HERDR_STUB_STATE"] = str(self.state_path)
        env["PYTHONDONTWRITEBYTECODE"] = "1"
        return subprocess.run(
            [
                sys.executable,
                "-B",
                str(DELIVER),
                "--pane",
                PANE,
                "--file",
                str(self.prompt),
                "--settle",
                "0",
                "--start-timeout",
                "1",
                *extra,
            ],
            env=env,
            capture_output=True,
            text=True,
            timeout=60,
        )

    def state(self) -> dict:
        return json.loads(self.state_path.read_text(encoding="utf-8"))

    def sent(self, verb: str) -> list[list[str]]:
        return [call for call in self.state()["calls"] if call[:2] == ["pane", verb]]


class HerdrDeliveryTests(unittest.TestCase):
    def test_skill_names_send_text_recipe(self) -> None:
        text = HERDR_SKILL.read_text(encoding="utf-8")
        for marker in (
            "herdr pane send-text",
            "herdr pane send-keys",
            "Enter",
            "256 KiB",
            "codeflow delegate arm",
            "scripts/deliver.py",
        ):
            self.assertIn(marker, text, f"cf-herdr lost delivery marker {marker!r}")

    def test_herdr_cli_exposes_send_text_and_send_keys(self) -> None:
        if shutil.which("herdr") is None:
            self.skipTest("herdr not on PATH")
        help_text = subprocess.check_output(["herdr", "pane", "--help"], text=True)
        self.assertIn("send-text", help_text)
        self.assertIn("send-keys", help_text)

    def test_live_send_text_round_trip(self) -> None:
        if os.environ.get("CF_HERDR_LIVE_CANARY") != "1":
            self.skipTest("live canaries require explicit CF_HERDR_LIVE_CANARY=1")
        if os.environ.get("HERDR_ENV") != "1":
            self.skipTest("not inside Herdr")
        if shutil.which("herdr") is None:
            self.skipTest("herdr not on PATH")
        workspace = os.environ.get("HERDR_WORKSPACE_ID")
        caller = os.environ.get("HERDR_PANE_ID")
        if not workspace or not caller:
            self.skipTest("missing HERDR_WORKSPACE_ID or HERDR_PANE_ID")

        token = f"cf-herdr-canary-{uuid.uuid4().hex[:8]}"
        created = herdr(
            "tab",
            "create",
            "--workspace",
            workspace,
            "--label",
            f"cf/codeflow/skill-cat/canary/{token[-2:]}",
            "--cwd",
            str(ROOT),
            "--no-focus",
        )
        tab_id = created["result"]["tab"]["tab_id"]
        pane_id = created["result"]["root_pane"]["pane_id"]
        try:
            self.assertNotEqual(pane_id, caller)
            current = herdr("pane", "current", "--current")
            self.assertEqual(
                current["result"]["pane"]["pane_id"],
                caller,
                "tab create stole focus from the caller pane",
            )
            wait_for_shell(self, pane_id)
            subprocess.check_call(["herdr", "pane", "send-text", pane_id, f"echo {token}"])
            time.sleep(0.3)
            subprocess.check_call(["herdr", "pane", "send-keys", pane_id, "Enter"])
            subprocess.check_call(
                [
                    "herdr",
                    "pane",
                    "wait-output",
                    pane_id,
                    "--match",
                    token,
                    "--timeout",
                    "15000",
                    "--source",
                    "visible",
                    "--lines",
                    "40",
                ]
            )
        finally:
            subprocess.call(["herdr", "tab", "close", tab_id])

    def test_live_delivery_confirms_a_started_turn(self) -> None:
        """AC-3: deliver to a real seat and observe its confirmed start.

        The seat kind is `CF_HERDR_CANARY_KIND` (default codex); its native
        arguments after `--` come from `CF_HERDR_CANARY_ARGS`, split on
        spaces. The canary prints the tab, agent and pane it used.
        """
        if os.environ.get("CF_HERDR_LIVE_CANARY") != "1":
            self.skipTest("live canaries require explicit CF_HERDR_LIVE_CANARY=1")
        if os.environ.get("HERDR_ENV") != "1":
            self.skipTest("not inside Herdr (HERDR_ENV is not 1)")
        if shutil.which("herdr") is None:
            self.skipTest("herdr not on PATH")
        workspace = os.environ.get("HERDR_WORKSPACE_ID")
        if not workspace or not os.environ.get("HERDR_PANE_ID"):
            self.skipTest("missing HERDR_WORKSPACE_ID or HERDR_PANE_ID")
        kind = os.environ.get("CF_HERDR_CANARY_KIND", "codex")
        native = os.environ.get("CF_HERDR_CANARY_ARGS", "").split()
        nonce = uuid.uuid4().hex[:6]
        label = f"cf/codeflow/tsk144/canary/{nonce[:2]}"
        name = f"cf-codeflow-tsk144-canary-{nonce}"
        created = herdr(
            "tab", "create", "--workspace", workspace, "--label", label,
            "--cwd", str(ROOT), "--no-focus",
        )
        tab_id = created["result"]["tab"]["tab_id"]
        pane_id = created["result"]["root_pane"]["pane_id"]
        print(f"\ncanary: tab {label} ({tab_id}), agent {name}, pane {pane_id}")
        try:
            wait_for_shell(self, pane_id)
            start_agent(self, name, kind, pane_id, native)
            with tempfile.TemporaryDirectory() as tmp:
                prompt = Path(tmp) / "canary.md"
                prompt.write_text("Reply with the single word ok.\n", encoding="utf-8")
                done = subprocess.run(
                    [sys.executable, "-B", str(DELIVER), "--pane", pane_id,
                     "--file", str(prompt)],
                    capture_output=True, text=True, timeout=180,
                )
            print(done.stdout + done.stderr)
            self.assertEqual(done.returncode, 0, done.stdout + done.stderr)
            self.assertIn("started", done.stdout)
        finally:
            subprocess.call(["herdr", "tab", "close", tab_id])


SHELLS = {"zsh", "bash", "sh"}


def shell_is_idle(foreground: list[dict]) -> bool:
    """True when the pane's foreground group is only a shell: a startup child
    (a prompt plugin, `sleep`, an rc command) still running means the shell
    has not reached its prompt yet."""
    names = [proc.get("name", "") for proc in foreground]
    return bool(names) and all(name in SHELLS for name in names)


def wait_for_shell(case: unittest.TestCase, pane_id: str) -> None:
    """Wait until the pane shows an idle shell on two polls in a row."""
    deadline = time.time() + 20
    idle = 0
    while time.time() < deadline:
        info = herdr("pane", "process-info", "--pane", pane_id)
        foreground = info["result"]["process_info"].get("foreground_processes", [])
        idle = idle + 1 if shell_is_idle(foreground) else 0
        if idle >= 2:
            return
        time.sleep(0.2)
    case.fail("new pane never reached an idle shell prompt")


def start_agent(
    case: unittest.TestCase, name: str, kind: str, pane_id: str, native: list[str]
) -> None:
    """Start the seat. Only Herdr's pre-launch `agent_pane_busy` refusal is
    retried, and only while no agent of that name is registered; any other
    failure is final, and no prompt is ever resent."""
    deadline = time.time() + 20
    while True:
        run = subprocess.run(
            ["herdr", "agent", "start", name, "--kind", kind, "--pane", pane_id,
             "--", *native],
            capture_output=True, text=True,
        )
        if run.returncode == 0:
            return
        busy = "agent_pane_busy" in run.stdout + run.stderr
        registered = subprocess.run(
            ["herdr", "agent", "get", name], capture_output=True, text=True
        ).returncode == 0
        if not busy or registered or time.time() >= deadline:
            case.fail(f"herdr agent start failed: {run.stdout}{run.stderr}")
        time.sleep(0.5)


class StubDeliveryTests(unittest.TestCase):
    """AC-1 and AC-2 against a stand-in `herdr`."""

    def test_a_shell_still_running_startup_is_not_ready(self):
        """A pane whose shell still runs a startup child is not ready for a
        seat; only a foreground group of shells alone is."""
        self.assertFalse(shell_is_idle([{"name": "zsh"}, {"name": "sleep"}]))
        self.assertFalse(shell_is_idle([]))
        self.assertTrue(shell_is_idle([{"name": "zsh"}]))

    def seat(self, **scenario: object) -> StubSeat:
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        return StubSeat(Path(tmp.name), **scenario)

    def test_a_seat_that_starts_is_confirmed_after_one_enter(self) -> None:
        seat = self.seat(start_on_enter=1)
        done = seat.deliver()
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(seat.state()["enters"], 1)
        self.assertIn(f"pane {PANE} started", done.stdout)
        [text] = seat.sent("send-text")
        self.assertEqual(text[3], seat.prompt.read_text(encoding="utf-8"))

    def test_a_seat_that_has_not_started_gets_no_second_enter(self) -> None:
        # Herdr 0.9.0 exposes no input line or cursor state, so the script
        # cannot tell a prompt waiting in the input from the same words in
        # the scrollback, and never presses Enter twice (TSK-144 review 1).
        seat = self.seat(start_on_enter=2)
        done = seat.deliver()
        self.assertEqual(done.returncode, 4, done.stdout + done.stderr)
        self.assertEqual(seat.state()["enters"], 1)
        self.assertIn(f"turn not confirmed on pane {PANE}", done.stderr)
        self.assertIn("no second Enter", done.stderr)

    def test_a_seat_that_never_starts_is_reported_by_pane(self) -> None:
        seat = self.seat(start_on_enter=None)
        done = seat.deliver()
        self.assertEqual(done.returncode, 4, done.stdout + done.stderr)
        self.assertEqual(seat.state()["enters"], 1, "one Enter only")
        self.assertEqual(len(seat.sent("send-text")), 1, "the prompt is never resent")
        self.assertIn(f"turn not confirmed on pane {PANE}", done.stderr)
        self.assertIn("herdr agent read", done.stderr)

    def test_a_submitted_prompt_left_in_the_scrollback_gets_no_second_enter(self) -> None:
        # The first Enter submits; the seat's status has not moved inside
        # the bound, and the prompt's words stay on screen above the input.
        seat = self.seat(start_on_enter=None, submit_on_enter=True)
        done = seat.deliver()
        self.assertEqual(done.returncode, 4, done.stdout + done.stderr)
        self.assertEqual(seat.state()["enters"], 1, "a second Enter would submit twice")
        self.assertIn("Reply with the single word ok.", seat.state()["scrollback"])
        self.assertIn(f"turn not confirmed on pane {PANE}", done.stderr)

    def test_a_prompt_gone_from_the_input_gets_no_blind_enter(self) -> None:
        seat = self.seat(start_on_enter=None, clear_on_enter=True)
        done = seat.deliver()
        self.assertEqual(done.returncode, 4, done.stdout + done.stderr)
        self.assertEqual(seat.state()["enters"], 1)
        self.assertIn(f"pane {PANE}", done.stderr)
        self.assertIn("herdr agent read", done.stderr)

    def test_a_turn_that_already_finished_counts_as_started(self) -> None:
        seat = self.seat(start_on_enter=1, started_status="done")
        done = seat.deliver()
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(seat.state()["enters"], 1)

    def test_a_busy_or_unknown_seat_gets_nothing(self) -> None:
        # `unknown` may be a turn still running that herdr cannot classify.
        for status in ("working", "blocked", "unknown"):
            seat = self.seat(status=status)
            done = seat.deliver()
            self.assertEqual(done.returncode, 5, f"{status}: {done.stdout}{done.stderr}")
            self.assertEqual(seat.sent("send-text") + seat.sent("send-keys"), [], status)
            self.assertIn(f"pane {PANE} is {status}", done.stderr)

    def test_a_fold_already_on_screen_is_not_this_paste(self) -> None:
        # A folded paste from an earlier turn stays in the scrollback; only
        # a fold this paste added gets the fold sentence.
        seat = self.seat(start_on_enter=None, scrollback="> [Pasted text #1 +40 lines]\n")
        done = seat.deliver("--lifecycle")
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(len(seat.sent("send-text")), 1, "no fold sentence")

    def test_a_fold_this_paste_added_gets_the_fold_sentence(self) -> None:
        seat = self.seat(start_on_enter=None, fold_paste=True)
        done = seat.deliver("--lifecycle")
        self.assertEqual(done.returncode, 0, done.stderr)
        texts = [call[3] for call in seat.sent("send-text")]
        self.assertEqual(texts[1:], ["Carry out the pasted instructions."])
        self.assertEqual(seat.state()["enters"], 1)

    def test_the_lifecycle_lane_keeps_its_accepted_wait(self) -> None:
        seat = self.seat(start_on_enter=None)
        done = seat.deliver("--lifecycle")
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(seat.state()["enters"], 1)
        calls = seat.state()["calls"]
        self.assertFalse([c for c in calls if c[:2] == ["agent", "get"]])
        self.assertIn("--until accepted", done.stdout)
        skill = words(HERDR_SKILL.read_text(encoding="utf-8"))
        self.assertIn("--until accepted", skill)

    def test_a_seat_whose_folder_is_gone_gets_nothing(self) -> None:
        for lane in ((), ("--lifecycle",)):
            seat = self.seat()
            shutil.rmtree(seat.folder)
            done = seat.deliver(*lane)
            self.assertEqual(done.returncode, 3, f"{lane}: {done.stdout}{done.stderr}")
            self.assertEqual(seat.sent("send-text") + seat.sent("send-keys"), [], lane)
            self.assertIn(str(seat.folder), done.stderr)
            self.assertIn("nothing was sent", done.stderr)
            self.assertIn("relaunch", done.stderr)

    def test_an_oversized_prompt_takes_the_degraded_path(self) -> None:
        seat = self.seat()
        seat.prompt.write_text("x" * (256 * 1024 + 1), encoding="utf-8")
        done = seat.deliver()
        self.assertEqual(done.returncode, 2, done.stdout + done.stderr)
        self.assertEqual(seat.sent("send-text"), [])
        self.assertIn("256 KiB", done.stderr)

    def test_resume_and_cleanup_name_the_seat_folder(self) -> None:
        skill = words(HERDR_SKILL.read_text(encoding="utf-8"))
        for marker in (
            "Resume delivers through the same script",
            "keep a worktree that a live seat uses as its folder until that seat's tab is closed",
            "herdr agent list",
        ):
            self.assertIn(marker, skill, f"cf-herdr lost {marker!r}")



class QualificationDeliveryTests(unittest.TestCase):
    def setUp(self):
        import importlib.util
        spec = importlib.util.spec_from_file_location("qualification_runner", ROOT / "evals/qualification/runner.py")
        self.runner = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.runner)

    @staticmethod
    def screen(body):
        return "history\n────────────────\n❯ " + body + "\n────────────────\n  ⏸ manual mode on\n"

    def test_recognizer_rejects_history_dialogs_and_unknown_footers(self):
        editor = self.runner.editor
        self.assertEqual("", editor(self.screen("")))
        self.assertEqual("pending", editor(self.screen("pending")))
        self.assertEqual("pending", editor(self.screen("pending").replace("❯ ", "❯\u00a0")))
        for bad in ["", "❯ pending\n", self.screen("pending") + "Permission dialog\n",
                    self.screen("pending").replace("⏸ manual mode on", "unknown footer"),
                    self.screen("pending").replace("❯ ", "❯\t"), self.screen(" pending"),
                    self.screen(" "), self.screen("pending\n❯ second prompt")]:
            self.assertIsNone(editor(bad), bad)

    def test_live_idle_placeholders_allow_first_delivery(self):
        from unittest.mock import patch
        runner = self.runner
        for placeholder, footer in [
            ('Try "fix lint errors"', '  ⏸ manual mode on'),
            ('Try "create a util logging.py that..."', '  ⏵⏵ bypass permissions on (shift+tab to cycle)'),
            ('Try "refactor <filepath>"', '  codeflow-qualify\n  ⏸ manual mode on'),
        ]:
            initial = self.screen(placeholder).replace('  ⏸ manual mode on', footer)
            with self.subTest(placeholder=placeholder), \
                 patch.object(runner, "visible", side_effect=[runner.editor(initial), "prompt"]), \
                 patch.object(runner, "started", return_value=True), \
                 patch.object(runner, "herdr") as call, patch.object(runner.time, "sleep"):
                self.assertEqual(1, runner.deliver_claude("own-pane", "prompt", {}, 1))
                self.assertEqual(1, sum(c.args[:2] == ("pane", "send-text") for c in call.call_args_list))

    def test_second_enter_only_when_this_prompt_remains_in_verified_editor(self):
        from unittest.mock import patch
        runner = self.runner
        with patch.object(runner, "visible", side_effect=["", "prompt", "prompt"]), \
             patch.object(runner, "started", side_effect=[False, True]), \
             patch.object(runner, "herdr") as call, patch.object(runner.time, "sleep"):
            self.assertEqual(2, runner.deliver_claude("own-pane", "prompt", {}, 1))
            self.assertEqual(2, sum(c.args[:2] == ("pane", "send-keys") for c in call.call_args_list))
        for after in [None, "", "another prompt"]:
            with patch.object(runner, "visible", side_effect=["", "prompt", after]), \
                 patch.object(runner, "started", return_value=False), \
                 patch.object(runner, "herdr") as call, patch.object(runner.time, "sleep"):
                with self.assertRaises(runner.Refused):
                    runner.deliver_claude("own-pane", "prompt", {}, 1)
                self.assertEqual(1, sum(c.args[:2] == ("pane", "send-keys") for c in call.call_args_list))

    def test_unreadable_or_nonempty_initial_editor_gets_no_text(self):
        from unittest.mock import patch
        runner = self.runner
        for initial in [None, "previous input", 'Try "hint" then act', 'Try "hint"\nmore']:
            with patch.object(runner, "visible", return_value=initial), patch.object(runner, "herdr") as call:
                with self.assertRaises(runner.Refused):
                    runner.deliver_claude("own-pane", "prompt", {}, 1)
                call.assert_not_called()

    def test_fold_directive_must_be_visible_before_enter(self):
        from unittest.mock import patch
        runner = self.runner
        folded = "[Pasted text #1 +4 lines]"
        for confirmed in [False, True]:
            after = folded + runner.DIRECTIVE if confirmed else folded
            with patch.object(runner, "visible", side_effect=["", folded, after]), \
                 patch.object(runner, "started", return_value=True), \
                 patch.object(runner, "herdr") as call, patch.object(runner.time, "sleep"):
                if confirmed:
                    self.assertEqual(1, runner.deliver_claude("own-pane", "prompt", {}, 1))
                else:
                    with self.assertRaises(runner.Refused):
                        runner.deliver_claude("own-pane", "prompt", {}, 1)
                self.assertEqual(int(confirmed), sum(c.args[:2] == ("pane", "send-keys") for c in call.call_args_list))

    def test_waits_for_starting_seat_before_delivery(self):
        from unittest.mock import patch
        runner = self.runner
        with patch.object(runner, "state", side_effect=[{"agent_status": "starting"}, {"agent_status": "idle"}]) as state, \
             patch.object(runner.time, "sleep"):
            self.assertEqual("idle", runner.wait_ready("own-pane", 1)["agent_status"])
            self.assertEqual(2, state.call_count)

if __name__ == "__main__":
    unittest.main()
