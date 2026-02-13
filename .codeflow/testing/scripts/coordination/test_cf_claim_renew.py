"""
test_cf_claim_renew.py - Tests for cf-claim-renew.py script.

Tests cover: single renewal, --all batch renewal, expired claim handling,
CRDT + JSONL dual-write, V4 output format, and error cases.
"""

import json
import os
import subprocess
import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path

import pytest

SCRIPTS_PATH = Path(__file__).parent.parent.parent.parent / "scripts"
SCRIPT = SCRIPTS_PATH / "coordination" / "cf-claim-renew.py"
sys.path.insert(0, str(SCRIPTS_PATH))

from codeflow_py_lib import CoordinationDoc
from codeflow_py_lib.jsonl import read_jsonl


@pytest.fixture
def renew_env(tmp_path):
    """Create isolated CRDT state for renew tests."""
    state_dir = tmp_path / ".state"
    (state_dir / "coordination").mkdir(parents=True)
    (state_dir / "ledger").mkdir(parents=True)

    env = os.environ.copy()
    env["CODEFLOW_REPO_ROOT"] = str(tmp_path)
    return tmp_path, state_dir, env


def _create_state(state_dir, claims_dict, token_counter=0):
    """Helper to create CRDT state on disk."""
    doc = CoordinationDoc()
    doc._claims = claims_dict
    doc._token_counter = token_counter

    # Save using the library
    coord_dir = state_dir / "coordination"
    coord_dir.mkdir(parents=True, exist_ok=True)
    data = doc.to_bytes()
    (coord_dir / "state.loro").write_bytes(data)
    json_data = json.dumps(
        {"claims": doc.claims, "token_counter": doc._token_counter}, indent=2
    )
    (coord_dir / "state.json").write_text(json_data, encoding="utf-8")


def _run_renew(env, *args):
    """Run cf-claim-renew.py and return (stdout, stderr, exit_code)."""
    venv_python = (
        Path(__file__).parent.parent.parent / ".venv" / "bin" / "python"
    )
    result = subprocess.run(
        [str(venv_python), str(SCRIPT), *args],
        env=env,
        capture_output=True,
        text=True,
        timeout=10,
    )
    return result.stdout.strip(), result.stderr, result.returncode


def _active_claim(claim_id, ttl_seconds=600, owner_id="agent-001", pattern="file:src/*.ts"):
    """Create an active claim dict with future expiration."""
    now = datetime.now(timezone.utc)
    expires = now + timedelta(seconds=ttl_seconds)
    return {
        "id": claim_id,
        "work_id": "task-01ARZ3NDEKTSV4RRFFQ69G5FAV",
        "pattern": pattern,
        "mode": "exclusive",
        "owner_id": owner_id,
        "fencing_token": 1,
        "expires_at": expires.isoformat().replace("+00:00", "Z"),
        "status": "active",
        "created_at": now.isoformat().replace("+00:00", "Z"),
    }


def _expired_claim(claim_id, owner_id="agent-001"):
    """Create a claim that has already expired (status still active)."""
    now = datetime.now(timezone.utc)
    expired = now - timedelta(seconds=60)
    return {
        "id": claim_id,
        "work_id": "task-01ARZ3NDEKTSV4RRFFQ69G5FAV",
        "pattern": "file:src/*.ts",
        "mode": "exclusive",
        "owner_id": owner_id,
        "fencing_token": 1,
        "expires_at": expired.isoformat().replace("+00:00", "Z"),
        "status": "active",
        "created_at": (now - timedelta(seconds=660)).isoformat().replace("+00:00", "Z"),
    }


class TestRenewSingleClaim:
    """Tests for renewing a single claim by ID."""

    def test_renew_active_claim_success(self, renew_env):
        tmp_path, state_dir, env = renew_env
        _create_state(state_dir, {"claim-001": _active_claim("claim-001")})

        stdout, _, exit_code = _run_renew(env, "--claim-id", "claim-001", "--json")

        assert exit_code == 0
        result = json.loads(stdout)
        assert result["success"] is True
        assert result["claim_id"] == "claim-001"
        assert "new_expires_at" in result
        assert "previous_expires_at" in result
        assert "extended_by_seconds" in result
        assert result["extended_by_seconds"] == 600

    def test_renew_with_custom_ttl(self, renew_env):
        tmp_path, state_dir, env = renew_env
        _create_state(state_dir, {"claim-001": _active_claim("claim-001")})

        stdout, _, exit_code = _run_renew(
            env, "--claim-id", "claim-001", "--ttl", "1200", "--json"
        )

        assert exit_code == 0
        result = json.loads(stdout)
        assert result["success"] is True
        assert result["extended_by_seconds"] == 1200

    def test_renew_includes_pattern_in_output(self, renew_env):
        tmp_path, state_dir, env = renew_env
        _create_state(state_dir, {"claim-001": _active_claim("claim-001")})

        stdout, _, exit_code = _run_renew(env, "--claim-id", "claim-001", "--json")

        result = json.loads(stdout)
        assert result["pattern"] == "file:src/*.ts"

    def test_renew_nonexistent_claim(self, renew_env):
        tmp_path, state_dir, env = renew_env
        _create_state(state_dir, {"claim-001": _active_claim("claim-001")})

        stdout, _, exit_code = _run_renew(env, "--claim-id", "claim-999", "--json")

        assert exit_code == 1
        result = json.loads(stdout)
        assert result["success"] is False
        assert result["error"] == "claim_not_found"

    def test_renew_released_claim(self, renew_env):
        tmp_path, state_dir, env = renew_env
        claim = _active_claim("claim-001")
        claim["status"] = "released"
        _create_state(state_dir, {"claim-001": claim})

        stdout, _, exit_code = _run_renew(env, "--claim-id", "claim-001", "--json")

        assert exit_code == 1
        result = json.loads(stdout)
        assert result["success"] is False
        assert result["error"] == "claim_not_active"

    def test_renew_expired_claim_returns_error(self, renew_env):
        tmp_path, state_dir, env = renew_env
        _create_state(state_dir, {"claim-001": _expired_claim("claim-001")})

        stdout, _, exit_code = _run_renew(env, "--claim-id", "claim-001", "--json")

        assert exit_code == 1
        result = json.loads(stdout)
        assert result["success"] is False
        assert result["error"] == "claim_expired"
        assert "suggestion" in result

    def test_renew_plain_text_output(self, renew_env):
        tmp_path, state_dir, env = renew_env
        _create_state(state_dir, {"claim-001": _active_claim("claim-001")})

        stdout, _, exit_code = _run_renew(env, "--claim-id", "claim-001")

        assert exit_code == 0
        assert "Claim renewed: claim-001" in stdout
        assert "New expiration:" in stdout


class TestRenewAll:
    """Tests for --all batch renewal."""

    def test_renew_all_active_claims(self, renew_env):
        tmp_path, state_dir, env = renew_env
        claims = {
            "claim-001": _active_claim("claim-001", pattern="file:src/*.ts"),
            "claim-002": _active_claim("claim-002", pattern="dir:src/auth"),
        }
        _create_state(state_dir, claims, token_counter=2)

        stdout, _, exit_code = _run_renew(env, "--all", "--json")

        assert exit_code == 0
        result = json.loads(stdout)
        assert result["success"] is True
        assert result["renewed_count"] == 2
        assert len(result["claims"]) == 2

    def test_renew_all_skips_expired(self, renew_env):
        tmp_path, state_dir, env = renew_env
        claims = {
            "claim-001": _active_claim("claim-001"),
            "claim-002": _expired_claim("claim-002"),
        }
        _create_state(state_dir, claims, token_counter=2)

        stdout, _, exit_code = _run_renew(env, "--all", "--json")

        assert exit_code == 0
        result = json.loads(stdout)
        assert result["renewed_count"] == 1

    def test_renew_all_skips_released(self, renew_env):
        tmp_path, state_dir, env = renew_env
        claim = _active_claim("claim-002")
        claim["status"] = "released"
        claims = {
            "claim-001": _active_claim("claim-001"),
            "claim-002": claim,
        }
        _create_state(state_dir, claims, token_counter=2)

        stdout, _, exit_code = _run_renew(env, "--all", "--json")

        result = json.loads(stdout)
        assert result["renewed_count"] == 1

    def test_renew_all_empty_state(self, renew_env):
        tmp_path, state_dir, env = renew_env
        _create_state(state_dir, {})

        stdout, _, exit_code = _run_renew(env, "--all", "--json")

        assert exit_code == 0
        result = json.loads(stdout)
        assert result["renewed_count"] == 0
        assert result["claims"] == []

    def test_renew_all_plain_text(self, renew_env):
        tmp_path, state_dir, env = renew_env
        claims = {
            "claim-001": _active_claim("claim-001"),
        }
        _create_state(state_dir, claims)

        stdout, _, exit_code = _run_renew(env, "--all")

        assert exit_code == 0
        assert "Renewed 1 claim(s)" in stdout


class TestRenewDualWrite:
    """Tests for CRDT + JSONL dual-write on renewal."""

    def test_jsonl_event_written(self, renew_env):
        tmp_path, state_dir, env = renew_env
        _create_state(state_dir, {"claim-001": _active_claim("claim-001")})

        _run_renew(env, "--claim-id", "claim-001", "--json")

        ledger = state_dir / "ledger" / "sessions.jsonl"
        assert ledger.exists()
        events = list(read_jsonl(ledger))
        renewed = [e for e in events if e.get("type") == "claim_renewed"]
        assert len(renewed) == 1
        assert renewed[0]["claim_id"] == "claim-001"
        assert "expires_at" in renewed[0]
        assert "ttl" in renewed[0]
        assert renewed[0]["ttl"] == 600

    def test_crdt_state_updated(self, renew_env):
        tmp_path, state_dir, env = renew_env
        original_claim = _active_claim("claim-001")
        original_expires = original_claim["expires_at"]
        _create_state(state_dir, {"claim-001": original_claim})

        _run_renew(env, "--claim-id", "claim-001", "--json")

        # Re-read CRDT state
        json_path = state_dir / "coordination" / "state.json"
        state = json.loads(json_path.read_text())
        new_expires = state["claims"]["claim-001"]["expires_at"]
        assert new_expires != original_expires

    def test_jsonl_batch_events_for_all(self, renew_env):
        tmp_path, state_dir, env = renew_env
        claims = {
            "claim-001": _active_claim("claim-001", pattern="file:src/*.ts"),
            "claim-002": _active_claim("claim-002", pattern="dir:src/auth"),
        }
        _create_state(state_dir, claims, token_counter=2)

        _run_renew(env, "--all", "--json")

        ledger = state_dir / "ledger" / "sessions.jsonl"
        events = list(read_jsonl(ledger))
        renewed = [e for e in events if e.get("type") == "claim_renewed"]
        assert len(renewed) == 2

    def test_no_jsonl_on_failed_renewal(self, renew_env):
        tmp_path, state_dir, env = renew_env
        _create_state(state_dir, {"claim-001": _active_claim("claim-001")})

        _run_renew(env, "--claim-id", "claim-999", "--json")

        ledger = state_dir / "ledger" / "sessions.jsonl"
        if ledger.exists():
            events = list(read_jsonl(ledger))
            renewed = [e for e in events if e.get("type") == "claim_renewed"]
            assert len(renewed) == 0


class TestRenewArgValidation:
    """Tests for argument parsing and validation."""

    def test_claim_id_and_all_mutually_exclusive(self, renew_env):
        _, _, env = renew_env
        _, stderr, exit_code = _run_renew(
            env, "--claim-id", "claim-001", "--all", "--json"
        )
        assert exit_code == 2

    def test_requires_claim_id_or_all(self, renew_env):
        _, _, env = renew_env
        _, stderr, exit_code = _run_renew(env, "--json")
        assert exit_code == 2

    def test_default_ttl_is_600(self, renew_env):
        tmp_path, state_dir, env = renew_env
        _create_state(state_dir, {"claim-001": _active_claim("claim-001")})

        stdout, _, exit_code = _run_renew(env, "--claim-id", "claim-001", "--json")

        result = json.loads(stdout)
        assert result["extended_by_seconds"] == 600


class TestRenewExitCodes:
    """Tests for V4-aligned exit codes."""

    def test_exit_0_on_success(self, renew_env):
        tmp_path, state_dir, env = renew_env
        _create_state(state_dir, {"claim-001": _active_claim("claim-001")})
        _, _, exit_code = _run_renew(env, "--claim-id", "claim-001", "--json")
        assert exit_code == 0

    def test_exit_1_on_not_found(self, renew_env):
        tmp_path, state_dir, env = renew_env
        _create_state(state_dir, {})
        _, _, exit_code = _run_renew(env, "--claim-id", "claim-999", "--json")
        assert exit_code == 1

    def test_exit_1_on_expired(self, renew_env):
        tmp_path, state_dir, env = renew_env
        _create_state(state_dir, {"claim-001": _expired_claim("claim-001")})
        _, _, exit_code = _run_renew(env, "--claim-id", "claim-001", "--json")
        assert exit_code == 1
