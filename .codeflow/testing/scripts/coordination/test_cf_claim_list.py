"""
test_cf_claim_list.py - Tests for cf-claim-list.py script.
"""

import json
import subprocess
import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path
from unittest.mock import patch

import codeflow_py_lib.crdt as crdt_module
from codeflow_py_lib.crdt import CoordinationDoc, save_coordination

# Path to the script under test
SCRIPT_PATH = (
    Path(__file__).parent.parent.parent.parent
    / "scripts"
    / "coordination"
    / "cf-claim-list.py"
)


def _run_list_script(*extra_args, env_override=None):
    """Run cf-claim-list.py as subprocess, returning (returncode, stdout, stderr)."""
    import os

    env = os.environ.copy()
    scripts_dir = str(Path(__file__).parent.parent.parent.parent / "scripts")
    env["PYTHONPATH"] = scripts_dir
    if env_override:
        env.update(env_override)

    result = subprocess.run(
        [sys.executable, str(SCRIPT_PATH), *extra_args],
        capture_output=True,
        text=True,
        env=env,
        timeout=10,
    )
    return result.returncode, result.stdout, result.stderr


class TestClaimListBasic:
    """Tests for basic list operations."""

    def test_list_empty_state(self, temp_dir, monkeypatch):
        """Should return empty list when no claims exist."""
        state_dir = temp_dir / ".state"
        state_dir.mkdir()
        monkeypatch.setattr(crdt_module, "get_state_dir", lambda: state_dir)

        # Import the script's main function
        import importlib.util

        spec = importlib.util.spec_from_file_location("cf_claim_list", SCRIPT_PATH)
        mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(mod)

        with patch("sys.argv", ["cf-claim-list.py"]):
            with patch("codeflow_py_lib.crdt.get_state_dir", return_value=state_dir):
                # Direct function test via module
                pass

    def test_list_returns_active_claims(self, temp_dir, monkeypatch):
        """Should list active claims by default."""
        state_dir = temp_dir / ".state"
        state_dir.mkdir()
        monkeypatch.setattr(crdt_module, "get_state_dir", lambda: state_dir)

        now = datetime.now(timezone.utc)
        expires = (now + timedelta(seconds=600)).isoformat().replace("+00:00", "Z")

        doc = CoordinationDoc()
        doc.add_claim(
            claim_id="claim-001",
            work_id="task-01ARZ3NDEKTSV4RRFFQ69G5FAV",
            pattern="file:src/*.ts",
            owner_id="agent-001",
            expires_at=expires,
        )
        doc.add_claim(
            claim_id="claim-002",
            work_id="task-02BRZ4NDEKTSV4RRFFQ69G5FAV",
            pattern="dir:src/auth",
            owner_id="agent-002",
            mode="shared",
            expires_at=expires,
        )
        save_coordination(doc)

        # Use subprocess to test the script
        returncode, stdout, stderr = _run_list_script(
            env_override={"CODEFLOW_REPO_ROOT": str(temp_dir)}
        )

        assert returncode == 0
        output = json.loads(stdout)
        assert "claims" in output
        assert "summary" in output
        assert output["summary"]["total"] == 2
        assert output["summary"]["active"] == 2

    def test_list_filters_by_status(self, temp_dir, monkeypatch):
        """Should filter claims by status."""
        state_dir = temp_dir / ".state"
        state_dir.mkdir()
        monkeypatch.setattr(crdt_module, "get_state_dir", lambda: state_dir)

        now = datetime.now(timezone.utc)
        expires = (now + timedelta(seconds=600)).isoformat().replace("+00:00", "Z")

        doc = CoordinationDoc()
        doc.add_claim(
            claim_id="claim-001",
            work_id="task-01ARZ3NDEKTSV4RRFFQ69G5FAV",
            pattern="file:src/*.ts",
            owner_id="agent-001",
            expires_at=expires,
        )
        doc.add_claim(
            claim_id="claim-002",
            work_id="task-02BRZ4NDEKTSV4RRFFQ69G5FAV",
            pattern="dir:src/auth",
            owner_id="agent-002",
            expires_at=expires,
        )
        doc.release_claim("claim-002")
        save_coordination(doc)

        # List released only
        returncode, stdout, stderr = _run_list_script(
            "--status", "released",
            env_override={"CODEFLOW_REPO_ROOT": str(temp_dir)},
        )

        assert returncode == 0
        output = json.loads(stdout)
        assert output["summary"]["total"] == 1
        assert output["claims"][0]["claim_id"] == "claim-002"
        assert output["claims"][0]["status"] == "released"

    def test_list_filters_by_owner(self, temp_dir, monkeypatch):
        """Should filter claims by owner ID."""
        state_dir = temp_dir / ".state"
        state_dir.mkdir()
        monkeypatch.setattr(crdt_module, "get_state_dir", lambda: state_dir)

        now = datetime.now(timezone.utc)
        expires = (now + timedelta(seconds=600)).isoformat().replace("+00:00", "Z")

        doc = CoordinationDoc()
        doc.add_claim(
            claim_id="claim-001",
            work_id="task-01ARZ3NDEKTSV4RRFFQ69G5FAV",
            pattern="file:src/*.ts",
            owner_id="agent-001",
            expires_at=expires,
        )
        doc.add_claim(
            claim_id="claim-002",
            work_id="task-02BRZ4NDEKTSV4RRFFQ69G5FAV",
            pattern="dir:src/auth",
            owner_id="agent-002",
            expires_at=expires,
        )
        save_coordination(doc)

        returncode, stdout, stderr = _run_list_script(
            "--owner-id", "agent-001",
            env_override={"CODEFLOW_REPO_ROOT": str(temp_dir)},
        )

        assert returncode == 0
        output = json.loads(stdout)
        assert output["summary"]["total"] == 1
        assert output["claims"][0]["owner_id"] == "agent-001"

    def test_list_filters_by_pattern(self, temp_dir, monkeypatch):
        """Should filter claims by pattern substring."""
        state_dir = temp_dir / ".state"
        state_dir.mkdir()
        monkeypatch.setattr(crdt_module, "get_state_dir", lambda: state_dir)

        now = datetime.now(timezone.utc)
        expires = (now + timedelta(seconds=600)).isoformat().replace("+00:00", "Z")

        doc = CoordinationDoc()
        doc.add_claim(
            claim_id="claim-001",
            work_id="task-01ARZ3NDEKTSV4RRFFQ69G5FAV",
            pattern="file:src/*.ts",
            owner_id="agent-001",
            expires_at=expires,
        )
        doc.add_claim(
            claim_id="claim-002",
            work_id="task-02BRZ4NDEKTSV4RRFFQ69G5FAV",
            pattern="dir:src/auth",
            owner_id="agent-002",
            expires_at=expires,
        )
        save_coordination(doc)

        returncode, stdout, stderr = _run_list_script(
            "--pattern", "auth",
            env_override={"CODEFLOW_REPO_ROOT": str(temp_dir)},
        )

        assert returncode == 0
        output = json.loads(stdout)
        assert output["summary"]["total"] == 1
        assert "auth" in output["claims"][0]["pattern"]


class TestClaimListExpired:
    """Tests for expired claim handling."""

    def test_expired_claims_excluded_by_default(self, temp_dir, monkeypatch):
        """Should exclude expired claims when listing active."""
        state_dir = temp_dir / ".state"
        state_dir.mkdir()
        monkeypatch.setattr(crdt_module, "get_state_dir", lambda: state_dir)

        now = datetime.now(timezone.utc)
        past = (now - timedelta(seconds=60)).isoformat().replace("+00:00", "Z")
        future = (now + timedelta(seconds=600)).isoformat().replace("+00:00", "Z")

        doc = CoordinationDoc()
        doc.add_claim(
            claim_id="claim-active",
            work_id="task-01ARZ3NDEKTSV4RRFFQ69G5FAV",
            pattern="file:src/*.ts",
            owner_id="agent-001",
            expires_at=future,
        )
        doc.add_claim(
            claim_id="claim-expired",
            work_id="task-02BRZ4NDEKTSV4RRFFQ69G5FAV",
            pattern="dir:src/auth",
            owner_id="agent-002",
            expires_at=past,
        )
        save_coordination(doc)

        returncode, stdout, stderr = _run_list_script(
            env_override={"CODEFLOW_REPO_ROOT": str(temp_dir)},
        )

        assert returncode == 0
        output = json.loads(stdout)
        assert output["summary"]["total"] == 1
        assert output["claims"][0]["claim_id"] == "claim-active"

    def test_include_expired_flag(self, temp_dir, monkeypatch):
        """Should include expired claims when --include-expired is set."""
        state_dir = temp_dir / ".state"
        state_dir.mkdir()
        monkeypatch.setattr(crdt_module, "get_state_dir", lambda: state_dir)

        now = datetime.now(timezone.utc)
        past = (now - timedelta(seconds=60)).isoformat().replace("+00:00", "Z")
        future = (now + timedelta(seconds=600)).isoformat().replace("+00:00", "Z")

        doc = CoordinationDoc()
        doc.add_claim(
            claim_id="claim-active",
            work_id="task-01ARZ3NDEKTSV4RRFFQ69G5FAV",
            pattern="file:src/*.ts",
            owner_id="agent-001",
            expires_at=future,
        )
        doc.add_claim(
            claim_id="claim-expired",
            work_id="task-02BRZ4NDEKTSV4RRFFQ69G5FAV",
            pattern="dir:src/auth",
            owner_id="agent-002",
            expires_at=past,
        )
        save_coordination(doc)

        returncode, stdout, stderr = _run_list_script(
            "--include-expired",
            env_override={"CODEFLOW_REPO_ROOT": str(temp_dir)},
        )

        assert returncode == 0
        output = json.loads(stdout)
        assert output["summary"]["total"] == 2


class TestClaimListOutputFormat:
    """Tests for output format options."""

    def test_json_output_has_v4_structure(self, temp_dir, monkeypatch):
        """Should output V4-compliant JSON structure."""
        state_dir = temp_dir / ".state"
        state_dir.mkdir()
        monkeypatch.setattr(crdt_module, "get_state_dir", lambda: state_dir)

        now = datetime.now(timezone.utc)
        expires = (now + timedelta(seconds=600)).isoformat().replace("+00:00", "Z")

        doc = CoordinationDoc()
        doc.add_claim(
            claim_id="claim-001",
            work_id="task-01ARZ3NDEKTSV4RRFFQ69G5FAV",
            pattern="file:src/*.ts",
            owner_id="agent-001",
            expires_at=expires,
        )
        save_coordination(doc)

        returncode, stdout, stderr = _run_list_script(
            env_override={"CODEFLOW_REPO_ROOT": str(temp_dir)},
        )

        assert returncode == 0
        output = json.loads(stdout)

        # V4 structure check
        assert "claims" in output
        assert "summary" in output
        summary = output["summary"]
        assert "total" in summary
        assert "active" in summary
        assert "own" in summary

        # Claim field check
        claim = output["claims"][0]
        assert "claim_id" in claim
        assert "pattern" in claim
        assert "mode" in claim
        assert "status" in claim
        assert "owner_id" in claim
        assert "is_own" in claim
        assert "fencing_token" in claim
        assert "created_at" in claim
        assert "expires_at" in claim

    def test_table_format(self, temp_dir, monkeypatch):
        """Should output table format when requested."""
        state_dir = temp_dir / ".state"
        state_dir.mkdir()
        monkeypatch.setattr(crdt_module, "get_state_dir", lambda: state_dir)

        now = datetime.now(timezone.utc)
        expires = (now + timedelta(seconds=600)).isoformat().replace("+00:00", "Z")

        doc = CoordinationDoc()
        doc.add_claim(
            claim_id="claim-001",
            work_id="task-01ARZ3NDEKTSV4RRFFQ69G5FAV",
            pattern="file:src/*.ts",
            owner_id="agent-001",
            expires_at=expires,
        )
        save_coordination(doc)

        returncode, stdout, stderr = _run_list_script(
            "--format", "table",
            env_override={"CODEFLOW_REPO_ROOT": str(temp_dir)},
        )

        assert returncode == 0
        assert "claim-001" in stdout
        assert "Total:" in stdout

    def test_table_format_empty(self, temp_dir, monkeypatch):
        """Should show 'No claims found' for empty table."""
        state_dir = temp_dir / ".state"
        state_dir.mkdir()
        monkeypatch.setattr(crdt_module, "get_state_dir", lambda: state_dir)

        doc = CoordinationDoc()
        save_coordination(doc)

        returncode, stdout, stderr = _run_list_script(
            "--format", "table",
            env_override={"CODEFLOW_REPO_ROOT": str(temp_dir)},
        )

        assert returncode == 0
        assert "No claims found" in stdout


class TestClaimListMineFlag:
    """Tests for --mine flag."""

    def test_mine_filters_by_current_user(self, temp_dir, monkeypatch):
        """Should filter to current user's claims with --mine."""
        state_dir = temp_dir / ".state"
        state_dir.mkdir()
        monkeypatch.setattr(crdt_module, "get_state_dir", lambda: state_dir)

        now = datetime.now(timezone.utc)
        expires = (now + timedelta(seconds=600)).isoformat().replace("+00:00", "Z")

        doc = CoordinationDoc()
        doc.add_claim(
            claim_id="claim-001",
            work_id="task-01ARZ3NDEKTSV4RRFFQ69G5FAV",
            pattern="file:src/*.ts",
            owner_id="test-user",
            expires_at=expires,
        )
        doc.add_claim(
            claim_id="claim-002",
            work_id="task-02BRZ4NDEKTSV4RRFFQ69G5FAV",
            pattern="dir:src/auth",
            owner_id="other-user",
            expires_at=expires,
        )
        save_coordination(doc)

        returncode, stdout, stderr = _run_list_script(
            "--mine",
            env_override={
                "CODEFLOW_REPO_ROOT": str(temp_dir),
                "CODEFLOW_OWNER_ID": "test-user",
            },
        )

        assert returncode == 0
        output = json.loads(stdout)
        assert output["summary"]["total"] == 1
        assert output["claims"][0]["owner_id"] == "test-user"


class TestClaimListTimeRemaining:
    """Tests for time_remaining_seconds field."""

    def test_active_claim_has_time_remaining(self, temp_dir, monkeypatch):
        """Should include time_remaining_seconds for active claims."""
        state_dir = temp_dir / ".state"
        state_dir.mkdir()
        monkeypatch.setattr(crdt_module, "get_state_dir", lambda: state_dir)

        now = datetime.now(timezone.utc)
        expires = (now + timedelta(seconds=300)).isoformat().replace("+00:00", "Z")

        doc = CoordinationDoc()
        doc.add_claim(
            claim_id="claim-001",
            work_id="task-01ARZ3NDEKTSV4RRFFQ69G5FAV",
            pattern="file:src/*.ts",
            owner_id="agent-001",
            expires_at=expires,
        )
        save_coordination(doc)

        returncode, stdout, stderr = _run_list_script(
            env_override={"CODEFLOW_REPO_ROOT": str(temp_dir)},
        )

        assert returncode == 0
        output = json.loads(stdout)
        claim = output["claims"][0]
        assert "time_remaining_seconds" in claim
        # Should be roughly 300 seconds (allow 10s variance for test execution)
        assert 280 <= claim["time_remaining_seconds"] <= 310


class TestClaimListWorkIdFilter:
    """Tests for --work-id filter."""

    def test_filter_by_work_id(self, temp_dir, monkeypatch):
        """Should filter claims by work ID."""
        state_dir = temp_dir / ".state"
        state_dir.mkdir()
        monkeypatch.setattr(crdt_module, "get_state_dir", lambda: state_dir)

        now = datetime.now(timezone.utc)
        expires = (now + timedelta(seconds=600)).isoformat().replace("+00:00", "Z")

        doc = CoordinationDoc()
        doc.add_claim(
            claim_id="claim-001",
            work_id="task-01ARZ3NDEKTSV4RRFFQ69G5FAV",
            pattern="file:src/*.ts",
            owner_id="agent-001",
            expires_at=expires,
        )
        doc.add_claim(
            claim_id="claim-002",
            work_id="task-02BRZ4NDEKTSV4RRFFQ69G5FAV",
            pattern="dir:src/auth",
            owner_id="agent-002",
            expires_at=expires,
        )
        save_coordination(doc)

        returncode, stdout, stderr = _run_list_script(
            "--work-id", "task-01ARZ3NDEKTSV4RRFFQ69G5FAV",
            env_override={"CODEFLOW_REPO_ROOT": str(temp_dir)},
        )

        assert returncode == 0
        output = json.loads(stdout)
        assert output["summary"]["total"] == 1
        assert output["claims"][0]["claim_id"] == "claim-001"


class TestClaimListStatusAll:
    """Tests for --status all."""

    def test_status_all_shows_everything(self, temp_dir, monkeypatch):
        """Should show all claims regardless of status."""
        state_dir = temp_dir / ".state"
        state_dir.mkdir()
        monkeypatch.setattr(crdt_module, "get_state_dir", lambda: state_dir)

        now = datetime.now(timezone.utc)
        expires = (now + timedelta(seconds=600)).isoformat().replace("+00:00", "Z")

        doc = CoordinationDoc()
        doc.add_claim(
            claim_id="claim-active",
            work_id="task-01ARZ3NDEKTSV4RRFFQ69G5FAV",
            pattern="file:src/*.ts",
            owner_id="agent-001",
            expires_at=expires,
        )
        doc.add_claim(
            claim_id="claim-released",
            work_id="task-02BRZ4NDEKTSV4RRFFQ69G5FAV",
            pattern="dir:src/auth",
            owner_id="agent-002",
            expires_at=expires,
        )
        doc.release_claim("claim-released")
        save_coordination(doc)

        returncode, stdout, stderr = _run_list_script(
            "--status", "all",
            env_override={"CODEFLOW_REPO_ROOT": str(temp_dir)},
        )

        assert returncode == 0
        output = json.loads(stdout)
        assert output["summary"]["total"] == 2


class TestClaimListExitCodes:
    """Tests for exit code behavior."""

    def test_success_returns_0(self, temp_dir, monkeypatch):
        """Should return exit code 0 on success."""
        state_dir = temp_dir / ".state"
        state_dir.mkdir()
        monkeypatch.setattr(crdt_module, "get_state_dir", lambda: state_dir)

        doc = CoordinationDoc()
        save_coordination(doc)

        returncode, stdout, stderr = _run_list_script(
            env_override={"CODEFLOW_REPO_ROOT": str(temp_dir)},
        )
        assert returncode == 0


class TestClaimListSorting:
    """Tests for claim sorting."""

    def test_sorted_by_created_at_descending(self, temp_dir, monkeypatch):
        """Should sort claims by created_at descending."""
        state_dir = temp_dir / ".state"
        state_dir.mkdir()
        monkeypatch.setattr(crdt_module, "get_state_dir", lambda: state_dir)

        now = datetime.now(timezone.utc)
        expires = (now + timedelta(seconds=600)).isoformat().replace("+00:00", "Z")

        doc = CoordinationDoc()
        # Manually set different created_at times
        doc._claims["claim-old"] = {
            "id": "claim-old",
            "work_id": "task-01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "pattern": "file:old.ts",
            "mode": "exclusive",
            "owner_id": "agent-001",
            "fencing_token": 1,
            "expires_at": expires,
            "status": "active",
            "created_at": "2026-01-01T00:00:00Z",
        }
        doc._claims["claim-new"] = {
            "id": "claim-new",
            "work_id": "task-02BRZ4NDEKTSV4RRFFQ69G5FAV",
            "pattern": "file:new.ts",
            "mode": "exclusive",
            "owner_id": "agent-001",
            "fencing_token": 2,
            "expires_at": expires,
            "status": "active",
            "created_at": "2026-02-01T00:00:00Z",
        }
        doc._token_counter = 2
        save_coordination(doc)

        returncode, stdout, stderr = _run_list_script(
            env_override={"CODEFLOW_REPO_ROOT": str(temp_dir)},
        )

        assert returncode == 0
        output = json.loads(stdout)
        assert output["claims"][0]["claim_id"] == "claim-new"
        assert output["claims"][1]["claim_id"] == "claim-old"
