#!/usr/bin/env python3
"""Controls for scripts/gate-parity.py (TSK-134).

Each control mutates a copy of the real test config and checks the guard
against the real CI workflow. The coverage and doctest pair stands for CI's
`cargo test --workspace` only when both halves run in CI and run the whole
suite, so every way of running less must read as drift.
"""

import copy
import importlib.util
import json
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("gate_parity", ROOT / "scripts" / "gate-parity.py")
parity = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(parity)

CONFIG = json.loads(parity.CONFIG.read_text())
CI = parity.ci_rust_job_commands(parity.WORKFLOW.read_text())


def target(cfg: dict, name: str) -> dict:
    return next(t for t in cfg["targets"] if t["name"] == name)


def full_command(cfg: dict, name: str) -> str:
    return target(cfg, name).get("modes", {}).get("full", {}).get("command", "")


def mutated(change) -> dict:
    cfg = copy.deepcopy(CONFIG)
    change(cfg)
    return cfg


def coverage_name() -> str:
    return next(t["name"] for t in CONFIG["targets"]
                if full_command(CONFIG, t["name"]).startswith("cargo llvm-cov "))


def doctest_name() -> str:
    return next(t["name"] for t in CONFIG["targets"]
                if full_command(CONFIG, t["name"]) == parity.DOCTESTS)


class GateParityControls(unittest.TestCase):
    def assert_parity(self, cfg: dict) -> None:
        self.assertEqual(parity.local_rust_commands(cfg), CI)

    def assert_drift(self, cfg: dict) -> list[str]:
        notes: list[str] = []
        local = parity.local_rust_commands(cfg, notes)
        self.assertNotEqual(local, CI)
        self.assertNotIn(parity.SUITE, local)
        return notes

    def test_the_committed_config_is_at_parity(self):
        self.assert_parity(CONFIG)

    def test_removing_either_half_is_drift(self):
        for name in (coverage_name(), doctest_name()):
            with self.subTest(name=name):
                self.assert_drift(mutated(lambda c, n=name: c["targets"].remove(target(c, n))))

    def test_disabling_either_half_is_drift(self):
        for name in (coverage_name(), doctest_name()):
            with self.subTest(name=name):
                self.assert_drift(mutated(lambda c, n=name: target(c, n).update(enabled=False)))

    def test_skipping_either_half_in_ci_is_drift(self):
        for name in (coverage_name(), doctest_name()):
            with self.subTest(name=name):
                self.assert_drift(mutated(lambda c, n=name: target(c, n).update(
                    ci_skip=True, ci_skip_reason="control")))

    def test_a_narrowed_coverage_selection_is_drift(self):
        for suffix in (" --lib", " -- --skip git_guard", " --features extra",
                       " --exclude codeflow-cli", " --no-default-features"):
            with self.subTest(suffix=suffix):
                def narrow(c, s=suffix):
                    target(c, coverage_name())["modes"]["full"]["command"] += s
                notes = self.assert_drift(mutated(narrow))
                self.assertTrue(any(parity.COVERAGE_FORM in n for n in notes), notes)

    def test_a_cwd_or_env_on_either_half_is_drift(self):
        for name in (coverage_name(), doctest_name()):
            for field, value in (("cwd", "crates/codeflow-core"),
                                 ("env", {"RUSTFLAGS": "--cfg skip"})):
                with self.subTest(name=name, field=field):
                    self.assert_drift(mutated(
                        lambda c, n=name, f=field, v=value: target(c, n).update({f: v})))

    def test_a_changed_line_threshold_keeps_parity(self):
        def threshold(c):
            target(c, coverage_name())["modes"]["full"]["command"] = (
                "cargo llvm-cov --workspace --summary-only --fail-under-lines 80")
        self.assert_parity(mutated(threshold))

    def test_any_disabled_rust_check_is_drift(self):
        cfg = mutated(lambda c: target(c, "rust-clippy").update(enabled=False))
        self.assertNotEqual(parity.local_rust_commands(cfg), CI)


if __name__ == "__main__":
    unittest.main()
