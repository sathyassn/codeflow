"""
test_cf_claim_release.py - Tests for cf-claim-release.py coordination script.

Tests cover:
- Release by claim-id
- Release by pattern
- Already-released error
- Claim-not-found error
- JSON and text output modes
- V4 output format (claim_id, pattern, released_at, reason, held_duration_seconds)
- Dual-write: CRDT + JSONL
- Exit codes: 0=success, 1=not found/already released, 2=error
- Reason parameter
"""

import json
import os
import subprocess
import sys
from pathlib import Path

import pytest

SCRIPTS_DIR = Path(__file__).parent.parent.parent.parent / "scripts"
ACQUIRE_SCRIPT = SCRIPTS_DIR / "coordination" / "cf-claim-acquire.py"
RELEASE_SCRIPT = SCRIPTS_DIR / "coordination" / "cf-claim-release.py"


def _run_script(script: Path, state_dir: Path, *extra_args: str) -> subprocess.CompletedProcess:
    """Run a coordination script with proper environment."""
    env = os.environ.copy()
    env["CODEFLOW_REPO_ROOT"] = str(state_dir.parent)
    env["PYTHONPATH"] = str(SCRIPTS_DIR)
    cmd = [sys.executable, str(script)] + list(extra_args)
    return subprocess.run(cmd, env=env, capture_output=True, text=True, timeout=15)


def run_acquire(state_dir: Path, *extra_args: str) -> subprocess.CompletedProcess:
    return _run_script(ACQUIRE_SCRIPT, state_dir, *extra_args)


def run_release(state_dir: Path, *extra_args: str) -> subprocess.CompletedProcess:
    return _run_script(RELEASE_SCRIPT, state_dir, *extra_args)


def acquire_claim(state_dir: Path, pattern: str = "src/models/**") -> str:
    """Acquire a claim and return the claim_id."""
    r = run_acquire(
        state_dir,
        "--work-id", "task-01ARZ3NDEKTSV4RRFFQ69G5FAV",
        "--pattern", pattern,
        "--owner-id", "agent-001",
        "--json",
    )
    assert r.returncode == 0, f"Acquire failed: {r.stderr}"
    data = json.loads(r.stdout.strip().split("\n")[-1])
    return data["claim_id"]


@pytest.fixture
def state_dir(tmp_path: Path) -> Path:
    """Create state directory structure for testing."""
    state = tmp_path / ".state"
    (state / "ledger").mkdir(parents=True)
    (state / "coordination").mkdir(parents=True)
    return state


class TestReleaseByClaimId:
    """Test releasing claims by claim ID."""

    def test_release_success_json(self, state_dir: Path) -> None:
        claim_id = acquire_claim(state_dir)
        r = run_release(state_dir, "--claim-id", claim_id, "--reason", "Work completed", "--json")
        assert r.returncode == 0
        data = json.loads(r.stdout.strip().split("\n")[-1])
        assert data["success"] is True
        assert data["claim_id"] == claim_id
        assert data["pattern"] == "src/models/**"
        assert data["released_at"].endswith("Z")
        assert data["reason"] == "Work completed"
        assert isinstance(data["held_duration_seconds"], int)
        assert data["held_duration_seconds"] >= 0

    def test_release_success_text(self, state_dir: Path) -> None:
        claim_id = acquire_claim(state_dir)
        r = run_release(state_dir, "--claim-id", claim_id)
        assert r.returncode == 0
        assert f"Claim released: {claim_id}" in r.stdout

    def test_release_not_found(self, state_dir: Path) -> None:
        r = run_release(state_dir, "--claim-id", "claim_nonexistent", "--json")
        assert r.returncode == 1
        data = json.loads(r.stdout.strip().split("\n")[-1])
        assert data["success"] is False
        assert data["error"] == "claim_not_found"

    def test_release_already_released(self, state_dir: Path) -> None:
        claim_id = acquire_claim(state_dir)
        # First release succeeds
        r1 = run_release(state_dir, "--claim-id", claim_id, "--json")
        assert r1.returncode == 0
        # Second release returns already_released
        r2 = run_release(state_dir, "--claim-id", claim_id, "--json")
        assert r2.returncode == 1
        data = json.loads(r2.stdout.strip().split("\n")[-1])
        assert data["error"] == "already_released"
        assert "released_at" in data


class TestReleaseByPattern:
    """Test releasing claims by pattern."""

    def test_release_by_pattern_json(self, state_dir: Path) -> None:
        claim_id = acquire_claim(state_dir, "src/services/**")
        r = run_release(
            state_dir,
            "--pattern", "src/services/**",
            "--reason", "Scope changed",
            "--json",
        )
        assert r.returncode == 0
        data = json.loads(r.stdout.strip().split("\n")[-1])
        assert data["success"] is True
        assert data["claim_id"] == claim_id
        assert data["pattern"] == "src/services/**"

    def test_release_by_pattern_not_found(self, state_dir: Path) -> None:
        r = run_release(state_dir, "--pattern", "nonexistent/**", "--json")
        assert r.returncode == 1
        data = json.loads(r.stdout.strip().split("\n")[-1])
        assert data["error"] == "claim_not_found"


class TestReleaseMutualExclusion:
    """Test that --claim-id and --pattern are mutually exclusive."""

    def test_requires_one_of_claim_id_or_pattern(self, state_dir: Path) -> None:
        r = run_release(state_dir, "--reason", "test")
        assert r.returncode == 2

    def test_rejects_both_claim_id_and_pattern(self, state_dir: Path) -> None:
        r = run_release(
            state_dir,
            "--claim-id", "claim_xxx",
            "--pattern", "src/**",
            "--json",
        )
        assert r.returncode == 2


class TestReleaseDualWrite:
    """Test CRDT + JSONL dual-write on release."""

    def test_jsonl_release_event_written(self, state_dir: Path) -> None:
        claim_id = acquire_claim(state_dir)
        run_release(state_dir, "--claim-id", claim_id, "--reason", "Done", "--json")

        ledger = state_dir / "ledger" / "sessions.jsonl"
        events = [json.loads(line) for line in ledger.read_text().strip().split("\n")]
        release_events = [e for e in events if e.get("type") == "claim_released"]
        assert len(release_events) == 1
        assert release_events[0]["claim_id"] == claim_id
        assert release_events[0]["reason"] == "Done"

    def test_crdt_state_updated(self, state_dir: Path) -> None:
        claim_id = acquire_claim(state_dir)
        run_release(state_dir, "--claim-id", claim_id, "--json")

        json_state = state_dir / "coordination" / "state.json"
        data = json.loads(json_state.read_text())
        assert data["claims"][claim_id]["status"] == "released"


class TestReleaseExitCodes:
    """Test exit codes per V4 spec."""

    def test_success_exits_0(self, state_dir: Path) -> None:
        claim_id = acquire_claim(state_dir)
        r = run_release(state_dir, "--claim-id", claim_id)
        assert r.returncode == 0

    def test_not_found_exits_1(self, state_dir: Path) -> None:
        r = run_release(state_dir, "--claim-id", "claim_missing")
        assert r.returncode == 1

    def test_missing_args_exits_2(self, state_dir: Path) -> None:
        r = run_release(state_dir)
        assert r.returncode == 2


class TestReleaseV4Format:
    """Test V4 spec JSON output format compliance."""

    def test_output_has_all_v4_fields(self, state_dir: Path) -> None:
        claim_id = acquire_claim(state_dir)
        r = run_release(
            state_dir,
            "--claim-id", claim_id,
            "--reason", "Work completed",
            "--json",
        )
        data = json.loads(r.stdout.strip().split("\n")[-1])
        expected_keys = {"success", "claim_id", "pattern", "released_at", "reason", "held_duration_seconds"}
        assert expected_keys.issubset(data.keys())

    def test_released_at_is_valid_iso_with_z(self, state_dir: Path) -> None:
        claim_id = acquire_claim(state_dir)
        r = run_release(state_dir, "--claim-id", claim_id, "--json")
        data = json.loads(r.stdout.strip().split("\n")[-1])
        assert data["released_at"].endswith("Z")
        assert "+00:00" not in data["released_at"]
