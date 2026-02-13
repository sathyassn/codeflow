"""
test_cf_claim_check.py - Tests for cf-claim-check.py script.

Tests cover: file-path and pattern checks, conflict matrix (BLOCK/WARN/ALLOW),
cross-type pattern matching, expiration handling, owner detection, and V4 output.
"""

import json
import os
import subprocess
import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path

import pytest

SCRIPTS_PATH = Path(__file__).parent.parent.parent.parent / "scripts"
SCRIPT = SCRIPTS_PATH / "coordination" / "cf-claim-check.py"
sys.path.insert(0, str(SCRIPTS_PATH))

from codeflow_py_lib.crdt import CoordinationDoc


@pytest.fixture
def check_env(tmp_path):
    """Create isolated CRDT state for check tests."""
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

    coord_dir = state_dir / "coordination"
    coord_dir.mkdir(parents=True, exist_ok=True)
    data = doc.to_bytes()
    (coord_dir / "state.loro").write_bytes(data)
    json_data = json.dumps(
        {"claims": doc.claims, "token_counter": doc._token_counter}, indent=2
    )
    (coord_dir / "state.json").write_text(json_data, encoding="utf-8")


def _run_check(env, *args):
    """Run cf-claim-check.py and return (stdout, stderr, exit_code)."""
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


def _active_claim(
    claim_id,
    pattern="file:src/*.ts",
    mode="exclusive",
    owner_id="agent-001",
    ttl_seconds=600,
):
    """Create an active claim dict."""
    now = datetime.now(timezone.utc)
    expires = now + timedelta(seconds=ttl_seconds)
    return {
        "id": claim_id,
        "work_id": "task-01ARZ3NDEKTSV4RRFFQ69G5FAV",
        "pattern": pattern,
        "mode": mode,
        "owner_id": owner_id,
        "fencing_token": 1,
        "expires_at": expires.isoformat().replace("+00:00", "Z"),
        "status": "active",
        "created_at": now.isoformat().replace("+00:00", "Z"),
    }


def _expired_claim(claim_id, pattern="file:src/*.ts", mode="exclusive", owner_id="agent-001"):
    """Create a claim that has already expired."""
    now = datetime.now(timezone.utc)
    expired = now - timedelta(seconds=60)
    return {
        "id": claim_id,
        "work_id": "task-01ARZ3NDEKTSV4RRFFQ69G5FAV",
        "pattern": pattern,
        "mode": mode,
        "owner_id": owner_id,
        "fencing_token": 1,
        "expires_at": expired.isoformat().replace("+00:00", "Z"),
        "status": "active",
        "created_at": (now - timedelta(seconds=660)).isoformat().replace("+00:00", "Z"),
    }


class TestCheckFilePath:
    """Tests for --file-path argument."""

    def test_file_matching_glob_pattern(self, check_env):
        _, state_dir, env = check_env
        _create_state(state_dir, {"c1": _active_claim("c1", "file:src/*.ts")})

        stdout, _, exit_code = _run_check(env, "--file-path", "file:src/app.ts")

        assert exit_code == 0
        result = json.loads(stdout)
        assert result["claimed"] is True
        assert len(result["claims"]) == 1

    def test_file_not_matching(self, check_env):
        _, state_dir, env = check_env
        _create_state(state_dir, {"c1": _active_claim("c1", "file:src/*.ts")})

        stdout, _, exit_code = _run_check(env, "--file-path", "file:lib/utils.py")

        assert exit_code == 0
        result = json.loads(stdout)
        assert result["claimed"] is False
        assert result["claims"] == []

    def test_file_inside_dir_claim(self, check_env):
        _, state_dir, env = check_env
        _create_state(state_dir, {"c1": _active_claim("c1", "dir:src/auth")})

        stdout, _, exit_code = _run_check(env, "--file-path", "file:src/auth/jwt.ts")

        assert exit_code == 0
        result = json.loads(stdout)
        assert result["claimed"] is True

    def test_file_outside_dir_claim(self, check_env):
        _, state_dir, env = check_env
        _create_state(state_dir, {"c1": _active_claim("c1", "dir:src/auth")})

        stdout, _, exit_code = _run_check(env, "--file-path", "file:src/api/routes.ts")

        assert exit_code == 0
        result = json.loads(stdout)
        assert result["claimed"] is False

    def test_bare_file_path_treated_as_file(self, check_env):
        """File path without type prefix defaults to 'file' type."""
        _, state_dir, env = check_env
        _create_state(state_dir, {"c1": _active_claim("c1", "file:src/*.ts")})

        stdout, _, exit_code = _run_check(env, "--file-path", "src/app.ts")

        assert exit_code == 0
        result = json.loads(stdout)
        assert result["claimed"] is True


class TestCheckPattern:
    """Tests for --pattern argument."""

    def test_exact_pattern_match(self, check_env):
        _, state_dir, env = check_env
        _create_state(state_dir, {"c1": _active_claim("c1", "file:src/*.ts")})

        stdout, _, _ = _run_check(env, "--pattern", "file:src/*.ts")

        result = json.loads(stdout)
        assert result["claimed"] is True

    def test_overlapping_patterns(self, check_env):
        _, state_dir, env = check_env
        _create_state(state_dir, {"c1": _active_claim("c1", "file:src/**")})

        stdout, _, _ = _run_check(env, "--pattern", "file:src/auth/jwt.ts")

        result = json.loads(stdout)
        assert result["claimed"] is True

    def test_no_overlap(self, check_env):
        _, state_dir, env = check_env
        _create_state(state_dir, {"c1": _active_claim("c1", "file:src/auth/**")})

        stdout, _, _ = _run_check(env, "--pattern", "file:lib/**")

        result = json.loads(stdout)
        assert result["claimed"] is False


class TestConflictMatrix:
    """Tests for V4 conflict matrix classification."""

    def test_exclusive_other_owner_blocks(self, check_env):
        _, state_dir, env = check_env
        _create_state(state_dir, {
            "c1": _active_claim("c1", "file:src/*.ts", "exclusive", "agent-001"),
        })

        stdout, _, _ = _run_check(
            env, "--file-path", "file:src/app.ts", "--owner-id", "agent-999"
        )

        result = json.loads(stdout)
        assert result["can_proceed"] is False
        assert result["claims"][0]["action"] == "BLOCK"
        assert result["claims"][0]["is_own"] is False

    def test_exclusive_same_owner_allows(self, check_env):
        _, state_dir, env = check_env
        _create_state(state_dir, {
            "c1": _active_claim("c1", "file:src/*.ts", "exclusive", "agent-001"),
        })

        stdout, _, _ = _run_check(
            env, "--file-path", "file:src/app.ts", "--owner-id", "agent-001"
        )

        result = json.loads(stdout)
        assert result["can_proceed"] is True
        assert result["claims"][0]["action"] == "ALLOW"
        assert result["claims"][0]["is_own"] is True

    def test_shared_other_owner_warns(self, check_env):
        _, state_dir, env = check_env
        _create_state(state_dir, {
            "c1": _active_claim("c1", "dir:src/auth", "shared", "agent-001"),
        })

        stdout, _, _ = _run_check(
            env, "--file-path", "file:src/auth/jwt.ts", "--owner-id", "agent-999"
        )

        result = json.loads(stdout)
        assert result["can_proceed"] is True
        assert result["claims"][0]["action"] == "WARN"

    def test_shared_same_owner_allows(self, check_env):
        _, state_dir, env = check_env
        _create_state(state_dir, {
            "c1": _active_claim("c1", "dir:src/auth", "shared", "agent-001"),
        })

        stdout, _, _ = _run_check(
            env, "--file-path", "file:src/auth/jwt.ts", "--owner-id", "agent-001"
        )

        result = json.loads(stdout)
        assert result["can_proceed"] is True
        assert result["claims"][0]["action"] == "ALLOW"

    def test_no_owner_id_exclusive_blocks(self, check_env):
        """Without --owner-id, exclusive claims should BLOCK."""
        _, state_dir, env = check_env
        _create_state(state_dir, {
            "c1": _active_claim("c1", "file:src/*.ts", "exclusive", "agent-001"),
        })

        stdout, _, _ = _run_check(env, "--file-path", "file:src/app.ts")

        result = json.loads(stdout)
        assert result["can_proceed"] is False
        assert result["claims"][0]["action"] == "BLOCK"

    def test_no_claims_allows(self, check_env):
        _, state_dir, env = check_env
        _create_state(state_dir, {})

        stdout, _, _ = _run_check(env, "--file-path", "file:src/app.ts")

        result = json.loads(stdout)
        assert result["claimed"] is False
        assert result["can_proceed"] is True


class TestExpirationHandling:
    """Tests for expired claim handling."""

    def test_expired_claims_skipped_by_default(self, check_env):
        _, state_dir, env = check_env
        _create_state(state_dir, {"c1": _expired_claim("c1", "file:src/*.ts")})

        stdout, _, _ = _run_check(env, "--file-path", "file:src/app.ts")

        result = json.loads(stdout)
        assert result["claimed"] is False

    def test_include_expired_shows_expired(self, check_env):
        _, state_dir, env = check_env
        _create_state(state_dir, {"c1": _expired_claim("c1", "file:src/*.ts")})

        stdout, _, _ = _run_check(
            env, "--file-path", "file:src/app.ts", "--include-expired"
        )

        result = json.loads(stdout)
        assert result["claimed"] is True
        assert result["claims"][0]["action"] == "WARN"

    def test_expired_claim_warns_not_blocks(self, check_env):
        """Expired claims should WARN even if exclusive."""
        _, state_dir, env = check_env
        _create_state(state_dir, {
            "c1": _expired_claim("c1", "file:src/*.ts", "exclusive", "agent-other"),
        })

        stdout, _, _ = _run_check(
            env, "--file-path", "file:src/app.ts", "--include-expired", "--owner-id", "agent-me"
        )

        result = json.loads(stdout)
        assert result["can_proceed"] is True
        assert result["claims"][0]["action"] == "WARN"

    def test_released_claims_always_skipped(self, check_env):
        _, state_dir, env = check_env
        claim = _active_claim("c1", "file:src/*.ts")
        claim["status"] = "released"
        _create_state(state_dir, {"c1": claim})

        stdout, _, _ = _run_check(
            env, "--file-path", "file:src/app.ts", "--include-expired"
        )

        result = json.loads(stdout)
        assert result["claimed"] is False


class TestPatternConflictDetection:
    """Tests for the patterns_conflict logic."""

    def test_exact_match(self, check_env):
        _, state_dir, env = check_env
        _create_state(state_dir, {"c1": _active_claim("c1", "file:src/app.ts")})

        stdout, _, _ = _run_check(env, "--file-path", "file:src/app.ts")

        result = json.loads(stdout)
        assert result["claimed"] is True

    def test_glob_star_matches_file(self, check_env):
        _, state_dir, env = check_env
        _create_state(state_dir, {"c1": _active_claim("c1", "file:src/*.ts")})

        stdout, _, _ = _run_check(env, "--file-path", "file:src/index.ts")

        result = json.loads(stdout)
        assert result["claimed"] is True

    def test_double_star_matches_nested(self, check_env):
        _, state_dir, env = check_env
        _create_state(state_dir, {"c1": _active_claim("c1", "file:src/**")})

        stdout, _, _ = _run_check(env, "--file-path", "file:src/auth/jwt.ts")

        result = json.loads(stdout)
        assert result["claimed"] is True

    def test_dir_containment(self, check_env):
        _, state_dir, env = check_env
        _create_state(state_dir, {"c1": _active_claim("c1", "dir:src")})

        stdout, _, _ = _run_check(env, "--pattern", "dir:src/auth")

        result = json.loads(stdout)
        assert result["claimed"] is True

    def test_dir_containment_reverse(self, check_env):
        _, state_dir, env = check_env
        _create_state(state_dir, {"c1": _active_claim("c1", "dir:src/auth")})

        stdout, _, _ = _run_check(env, "--pattern", "dir:src")

        result = json.loads(stdout)
        assert result["claimed"] is True

    def test_disjoint_dirs(self, check_env):
        _, state_dir, env = check_env
        _create_state(state_dir, {"c1": _active_claim("c1", "dir:src/auth")})

        stdout, _, _ = _run_check(env, "--pattern", "dir:src/api")

        result = json.loads(stdout)
        assert result["claimed"] is False


class TestOutputFormat:
    """Tests for V4-aligned output structure."""

    def test_output_has_claimed_field(self, check_env):
        _, state_dir, env = check_env
        _create_state(state_dir, {})

        stdout, _, _ = _run_check(env, "--file-path", "file:src/app.ts")

        result = json.loads(stdout)
        assert "claimed" in result

    def test_output_has_can_proceed_field(self, check_env):
        _, state_dir, env = check_env
        _create_state(state_dir, {})

        stdout, _, _ = _run_check(env, "--file-path", "file:src/app.ts")

        result = json.loads(stdout)
        assert "can_proceed" in result

    def test_output_has_claims_array(self, check_env):
        _, state_dir, env = check_env
        _create_state(state_dir, {"c1": _active_claim("c1")})

        stdout, _, _ = _run_check(env, "--file-path", "file:src/app.ts")

        result = json.loads(stdout)
        assert "claims" in result
        assert isinstance(result["claims"], list)

    def test_claim_entry_has_required_fields(self, check_env):
        _, state_dir, env = check_env
        _create_state(state_dir, {"c1": _active_claim("c1")})

        stdout, _, _ = _run_check(
            env, "--file-path", "file:src/app.ts", "--owner-id", "agent-999"
        )

        result = json.loads(stdout)
        claim = result["claims"][0]
        assert "claim_id" in claim
        assert "pattern" in claim
        assert "mode" in claim
        assert "owner_id" in claim
        assert "is_own" in claim
        assert "expires_at" in claim
        assert "action" in claim

    def test_multiple_conflicts_returned(self, check_env):
        _, state_dir, env = check_env
        _create_state(state_dir, {
            "c1": _active_claim("c1", "file:src/*.ts", "exclusive", "agent-001"),
            "c2": _active_claim("c2", "file:src/**", "shared", "agent-002"),
        }, token_counter=2)

        stdout, _, _ = _run_check(env, "--file-path", "file:src/app.ts")

        result = json.loads(stdout)
        assert len(result["claims"]) == 2


class TestReadOnly:
    """Tests that check is truly READ-ONLY (no CRDT writes)."""

    def test_no_crdt_modification(self, check_env):
        _, state_dir, env = check_env
        _create_state(state_dir, {"c1": _active_claim("c1")})

        loro_path = state_dir / "coordination" / "state.loro"
        before = loro_path.read_bytes()

        _run_check(env, "--file-path", "file:src/app.ts")

        after = loro_path.read_bytes()
        assert before == after

    def test_no_jsonl_written(self, check_env):
        _, state_dir, env = check_env
        _create_state(state_dir, {"c1": _active_claim("c1")})

        _run_check(env, "--file-path", "file:src/app.ts")

        ledger = state_dir / "ledger" / "sessions.jsonl"
        assert not ledger.exists()


class TestExitCodes:
    """Tests for V4-aligned exit codes."""

    def test_exit_0_on_claimed(self, check_env):
        _, state_dir, env = check_env
        _create_state(state_dir, {"c1": _active_claim("c1")})
        _, _, exit_code = _run_check(env, "--file-path", "file:src/app.ts")
        assert exit_code == 0

    def test_exit_0_on_not_claimed(self, check_env):
        _, state_dir, env = check_env
        _create_state(state_dir, {})
        _, _, exit_code = _run_check(env, "--file-path", "file:src/app.ts")
        assert exit_code == 0

    def test_file_path_and_pattern_mutually_exclusive(self, check_env):
        _, _, env = check_env
        _, _, exit_code = _run_check(
            env, "--file-path", "x", "--pattern", "y"
        )
        assert exit_code == 2

    def test_requires_file_path_or_pattern(self, check_env):
        _, _, env = check_env
        _, _, exit_code = _run_check(env)
        assert exit_code == 2
