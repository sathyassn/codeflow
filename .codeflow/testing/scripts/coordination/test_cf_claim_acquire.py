"""
test_cf_claim_acquire.py - Tests for cf-claim-acquire.py coordination script.

Tests cover:
- Successful claim acquisition (exclusive and shared)
- Conflict detection (exclusive vs exclusive, exclusive vs shared)
- JSON and text output modes
- V4 output format compliance (claim_id, pattern, mode, expires_at, fencing_token)
- Dual-write: CRDT + JSONL
- Exit codes: 0=success, 1=conflict, 2=error
- TTL handling
- Monotonic fencing tokens
"""

import json
import os
import subprocess
import sys
from datetime import datetime
from pathlib import Path

import pytest

SCRIPTS_DIR = Path(__file__).parent.parent.parent.parent / "scripts"
ACQUIRE_SCRIPT = SCRIPTS_DIR / "coordination" / "cf-claim-acquire.py"


def run_acquire(state_dir: Path, *extra_args: str) -> subprocess.CompletedProcess:
    """Run cf-claim-acquire.py with proper environment."""
    env = os.environ.copy()
    env["CODEFLOW_REPO_ROOT"] = str(state_dir.parent)
    env["PYTHONPATH"] = str(SCRIPTS_DIR)
    cmd = [sys.executable, str(ACQUIRE_SCRIPT)] + list(extra_args)
    return subprocess.run(cmd, env=env, capture_output=True, text=True, timeout=15)


@pytest.fixture
def state_dir(tmp_path: Path) -> Path:
    """Create state directory structure for testing."""
    state = tmp_path / ".state"
    (state / "ledger").mkdir(parents=True)
    (state / "coordination").mkdir(parents=True)
    return state


class TestAcquireSuccess:
    """Test successful claim acquisition."""

    def test_acquire_exclusive_json(self, state_dir: Path) -> None:
        result = run_acquire(
            state_dir,
            "--work-id", "task-01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "--pattern", "src/models/**",
            "--owner-id", "agent-001",
            "--mode", "exclusive",
            "--ttl", "600",
            "--json",
        )
        assert result.returncode == 0
        data = json.loads(result.stdout.strip().split("\n")[-1])
        assert data["success"] is True
        assert data["claim_id"].startswith("claim_")
        assert data["pattern"] == "src/models/**"
        assert data["mode"] == "exclusive"
        assert data["fencing_token"] == 1
        assert data["expires_at"].endswith("Z")
        assert "+00:00" not in data["expires_at"]

    def test_acquire_shared_json(self, state_dir: Path) -> None:
        result = run_acquire(
            state_dir,
            "--work-id", "task-01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "--pattern", "docs/**",
            "--owner-id", "agent-001",
            "--mode", "shared",
            "--ttl", "300",
            "--json",
        )
        assert result.returncode == 0
        data = json.loads(result.stdout.strip().split("\n")[-1])
        assert data["success"] is True
        assert data["mode"] == "shared"

    def test_acquire_text_output(self, state_dir: Path) -> None:
        result = run_acquire(
            state_dir,
            "--work-id", "task-01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "--pattern", "src/**",
            "--owner-id", "agent-001",
        )
        assert result.returncode == 0
        assert "Claim acquired:" in result.stdout
        assert "Fencing token:" in result.stdout
        assert "Expires at:" in result.stdout

    def test_acquire_default_mode_is_exclusive(self, state_dir: Path) -> None:
        result = run_acquire(
            state_dir,
            "--work-id", "task-01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "--pattern", "src/**",
            "--owner-id", "agent-001",
            "--json",
        )
        assert result.returncode == 0
        data = json.loads(result.stdout.strip().split("\n")[-1])
        assert data["mode"] == "exclusive"


class TestAcquireConflict:
    """Test conflict detection."""

    def test_exclusive_vs_exclusive_conflict(self, state_dir: Path) -> None:
        # First acquire succeeds
        r1 = run_acquire(
            state_dir,
            "--work-id", "task-01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "--pattern", "src/models/**",
            "--owner-id", "agent-001",
            "--json",
        )
        assert r1.returncode == 0

        # Second acquire on same pattern conflicts
        r2 = run_acquire(
            state_dir,
            "--work-id", "task-02BRZ4NDEKTSV4RRFFQ69G5FAV",
            "--pattern", "src/models/**",
            "--owner-id", "agent-002",
            "--json",
        )
        assert r2.returncode == 1
        data = json.loads(r2.stdout.strip().split("\n")[-1])
        assert data["success"] is False
        assert data["error"] == "conflict"
        assert "conflicting_claim" in data

    def test_exclusive_blocks_shared(self, state_dir: Path) -> None:
        # Exclusive claim first
        r1 = run_acquire(
            state_dir,
            "--work-id", "task-01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "--pattern", "src/**",
            "--owner-id", "agent-001",
            "--mode", "exclusive",
            "--json",
        )
        assert r1.returncode == 0

        # Shared claim on same pattern blocked
        r2 = run_acquire(
            state_dir,
            "--work-id", "task-02BRZ4NDEKTSV4RRFFQ69G5FAV",
            "--pattern", "src/**",
            "--owner-id", "agent-002",
            "--mode", "shared",
            "--json",
        )
        assert r2.returncode == 1

    def test_shared_blocks_exclusive(self, state_dir: Path) -> None:
        # Shared claim first
        r1 = run_acquire(
            state_dir,
            "--work-id", "task-01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "--pattern", "src/**",
            "--owner-id", "agent-001",
            "--mode", "shared",
            "--json",
        )
        assert r1.returncode == 0

        # Exclusive claim on same pattern blocked
        r2 = run_acquire(
            state_dir,
            "--work-id", "task-02BRZ4NDEKTSV4RRFFQ69G5FAV",
            "--pattern", "src/**",
            "--owner-id", "agent-002",
            "--mode", "exclusive",
            "--json",
        )
        assert r2.returncode == 1

    def test_shared_allows_shared(self, state_dir: Path) -> None:
        # Two shared claims on same pattern should both succeed
        r1 = run_acquire(
            state_dir,
            "--work-id", "task-01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "--pattern", "docs/**",
            "--owner-id", "agent-001",
            "--mode", "shared",
            "--json",
        )
        assert r1.returncode == 0

        r2 = run_acquire(
            state_dir,
            "--work-id", "task-02BRZ4NDEKTSV4RRFFQ69G5FAV",
            "--pattern", "docs/**",
            "--owner-id", "agent-002",
            "--mode", "shared",
            "--json",
        )
        assert r2.returncode == 0

    def test_different_patterns_no_conflict(self, state_dir: Path) -> None:
        r1 = run_acquire(
            state_dir,
            "--work-id", "task-01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "--pattern", "src/auth/**",
            "--owner-id", "agent-001",
            "--json",
        )
        assert r1.returncode == 0

        r2 = run_acquire(
            state_dir,
            "--work-id", "task-02BRZ4NDEKTSV4RRFFQ69G5FAV",
            "--pattern", "src/api/**",
            "--owner-id", "agent-002",
            "--json",
        )
        assert r2.returncode == 0

    def test_conflict_text_output(self, state_dir: Path) -> None:
        run_acquire(
            state_dir,
            "--work-id", "task-01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "--pattern", "src/**",
            "--owner-id", "agent-001",
        )
        r2 = run_acquire(
            state_dir,
            "--work-id", "task-02BRZ4NDEKTSV4RRFFQ69G5FAV",
            "--pattern", "src/**",
            "--owner-id", "agent-002",
        )
        assert r2.returncode == 1
        assert "Conflict:" in r2.stdout


class TestAcquireFencingTokens:
    """Test monotonic fencing token behavior."""

    def test_tokens_increment(self, state_dir: Path) -> None:
        r1 = run_acquire(
            state_dir,
            "--work-id", "task-01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "--pattern", "src/a/**",
            "--owner-id", "agent-001",
            "--json",
        )
        r2 = run_acquire(
            state_dir,
            "--work-id", "task-02BRZ4NDEKTSV4RRFFQ69G5FAV",
            "--pattern", "src/b/**",
            "--owner-id", "agent-002",
            "--json",
        )
        d1 = json.loads(r1.stdout.strip().split("\n")[-1])
        d2 = json.loads(r2.stdout.strip().split("\n")[-1])
        assert d1["fencing_token"] == 1
        assert d2["fencing_token"] == 2


class TestAcquireDualWrite:
    """Test CRDT + JSONL dual-write."""

    def test_jsonl_event_written(self, state_dir: Path) -> None:
        run_acquire(
            state_dir,
            "--work-id", "task-01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "--pattern", "src/**",
            "--owner-id", "agent-001",
            "--json",
        )
        ledger = state_dir / "ledger" / "sessions.jsonl"
        assert ledger.exists()
        events = [json.loads(line) for line in ledger.read_text().strip().split("\n")]
        claim_events = [e for e in events if e.get("type") == "claim_created"]
        assert len(claim_events) == 1
        assert claim_events[0]["pattern"] == "src/**"
        assert claim_events[0]["mode"] == "exclusive"

    def test_crdt_state_written(self, state_dir: Path) -> None:
        run_acquire(
            state_dir,
            "--work-id", "task-01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "--pattern", "src/**",
            "--owner-id", "agent-001",
            "--json",
        )
        # Check JSON fallback (always written alongside Loro)
        json_state = state_dir / "coordination" / "state.json"
        assert json_state.exists()
        data = json.loads(json_state.read_text())
        assert len(data["claims"]) == 1


class TestAcquireExitCodes:
    """Test exit codes per V4 spec."""

    def test_success_exits_0(self, state_dir: Path) -> None:
        r = run_acquire(
            state_dir,
            "--work-id", "task-01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "--pattern", "src/**",
            "--owner-id", "agent-001",
        )
        assert r.returncode == 0

    def test_conflict_exits_1(self, state_dir: Path) -> None:
        run_acquire(
            state_dir,
            "--work-id", "task-01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "--pattern", "src/**",
            "--owner-id", "agent-001",
        )
        r = run_acquire(
            state_dir,
            "--work-id", "task-02BRZ4NDEKTSV4RRFFQ69G5FAV",
            "--pattern", "src/**",
            "--owner-id", "agent-002",
        )
        assert r.returncode == 1

    def test_missing_args_exits_2(self, state_dir: Path) -> None:
        """Missing required args should produce non-zero exit."""
        r = run_acquire(state_dir, "--work-id", "TSK-001")
        assert r.returncode == 2


class TestAcquireV4Format:
    """Test V4 spec JSON output format compliance."""

    def test_output_has_all_v4_fields(self, state_dir: Path) -> None:
        r = run_acquire(
            state_dir,
            "--work-id", "task-01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "--pattern", "src/models/**",
            "--owner-id", "agent-001",
            "--mode", "exclusive",
            "--ttl", "600",
            "--json",
        )
        data = json.loads(r.stdout.strip().split("\n")[-1])
        expected_keys = {"success", "claim_id", "pattern", "mode", "expires_at", "fencing_token"}
        assert expected_keys.issubset(data.keys())

    def test_claim_id_uses_underscore_prefix(self, state_dir: Path) -> None:
        r = run_acquire(
            state_dir,
            "--work-id", "task-01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "--pattern", "src/**",
            "--owner-id", "agent-001",
            "--json",
        )
        data = json.loads(r.stdout.strip().split("\n")[-1])
        assert data["claim_id"].startswith("claim_")

    def test_expires_at_is_valid_iso_with_z(self, state_dir: Path) -> None:
        r = run_acquire(
            state_dir,
            "--work-id", "task-01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "--pattern", "src/**",
            "--owner-id", "agent-001",
            "--json",
        )
        data = json.loads(r.stdout.strip().split("\n")[-1])
        exp = data["expires_at"]
        assert exp.endswith("Z")
        # Should be parseable
        dt = datetime.fromisoformat(exp.replace("Z", "+00:00"))
        assert dt.tzinfo is not None
