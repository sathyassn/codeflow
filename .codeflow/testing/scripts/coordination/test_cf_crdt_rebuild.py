"""
test_cf_crdt_rebuild.py - Tests for cf-crdt-rebuild.py script.
"""

import json
import os
import subprocess
import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path

import codeflow_py_lib.crdt as crdt_module
from codeflow_py_lib.crdt import CoordinationDoc, save_coordination

# Path to the script under test
SCRIPT_PATH = (
    Path(__file__).parent.parent.parent.parent
    / "scripts"
    / "coordination"
    / "cf-crdt-rebuild.py"
)


def _run_rebuild_script(*extra_args, env_override=None):
    """Run cf-crdt-rebuild.py as subprocess, returning (returncode, stdout, stderr)."""
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


def _create_jsonl_ledger(ledger_path, events):
    """Write events to a JSONL ledger file."""
    ledger_path.parent.mkdir(parents=True, exist_ok=True)
    with open(ledger_path, "w") as f:
        for event in events:
            f.write(json.dumps(event) + "\n")


class TestCrdtRebuildBasic:
    """Tests for basic rebuild operations."""

    def test_rebuild_from_jsonl(self, temp_dir):
        """Should rebuild CRDT state from JSONL events."""
        ledger_dir = temp_dir / ".state" / "ledger"
        ledger_dir.mkdir(parents=True)
        ledger_path = ledger_dir / "sessions.jsonl"

        now = datetime.now(timezone.utc)
        expires = (now + timedelta(seconds=600)).isoformat() + "Z"

        _create_jsonl_ledger(ledger_path, [
            {
                "type": "claim_created",
                "id": "claim-001",
                "work_id": "TSK-001",
                "pattern": "file:src/*.ts",
                "mode": "exclusive",
                "owner_id": "agent-001",
                "fencing_token": 1,
                "expires_at": expires,
                "created_at": now.isoformat() + "Z",
            },
        ])

        returncode, stdout, stderr = _run_rebuild_script(
            "--json",
            "--source", str(ledger_path),
            env_override={"CODEFLOW_REPO_ROOT": str(temp_dir)},
        )

        assert returncode == 0
        output = json.loads(stdout)
        assert output["success"] is True
        assert output["events_replayed"] == 1
        assert output["final_state"]["active_claims"] == 1

    def test_rebuild_with_release_events(self, temp_dir):
        """Should apply release events during rebuild."""
        ledger_dir = temp_dir / ".state" / "ledger"
        ledger_dir.mkdir(parents=True)
        ledger_path = ledger_dir / "sessions.jsonl"

        now = datetime.now(timezone.utc)
        expires = (now + timedelta(seconds=600)).isoformat() + "Z"

        _create_jsonl_ledger(ledger_path, [
            {
                "type": "claim_created",
                "id": "claim-001",
                "work_id": "TSK-001",
                "pattern": "file:src/*.ts",
                "owner_id": "agent-001",
                "fencing_token": 1,
                "expires_at": expires,
                "created_at": now.isoformat() + "Z",
            },
            {
                "type": "claim_released",
                "claim_id": "claim-001",
                "released_at": now.isoformat() + "Z",
            },
        ])

        returncode, stdout, stderr = _run_rebuild_script(
            "--json",
            "--source", str(ledger_path),
            env_override={"CODEFLOW_REPO_ROOT": str(temp_dir)},
        )

        assert returncode == 0
        output = json.loads(stdout)
        assert output["events_replayed"] == 2
        assert output["final_state"]["active_claims"] == 0
        assert output["final_state"]["released_claims"] == 1

    def test_rebuild_with_renew_events(self, temp_dir):
        """Should apply renew events during rebuild."""
        ledger_dir = temp_dir / ".state" / "ledger"
        ledger_dir.mkdir(parents=True)
        ledger_path = ledger_dir / "sessions.jsonl"

        now = datetime.now(timezone.utc)
        old_expires = (now + timedelta(seconds=300)).isoformat() + "Z"
        new_expires = (now + timedelta(seconds=900)).isoformat() + "Z"

        _create_jsonl_ledger(ledger_path, [
            {
                "type": "claim_created",
                "id": "claim-001",
                "work_id": "TSK-001",
                "pattern": "file:src/*.ts",
                "owner_id": "agent-001",
                "fencing_token": 1,
                "expires_at": old_expires,
                "created_at": now.isoformat() + "Z",
            },
            {
                "type": "claim_renewed",
                "claim_id": "claim-001",
                "expires_at": new_expires,
            },
        ])

        returncode, stdout, stderr = _run_rebuild_script(
            "--json",
            "--source", str(ledger_path),
            env_override={"CODEFLOW_REPO_ROOT": str(temp_dir)},
        )

        assert returncode == 0
        output = json.loads(stdout)
        assert output["events_replayed"] == 2
        assert output["final_state"]["active_claims"] == 1


class TestCrdtRebuildMissingSource:
    """Tests for missing JSONL source file."""

    def test_missing_source_returns_exit_1(self, temp_dir):
        """Should return exit code 1 when source file is missing."""
        returncode, stdout, stderr = _run_rebuild_script(
            "--json",
            "--source", str(temp_dir / "nonexistent.jsonl"),
            env_override={"CODEFLOW_REPO_ROOT": str(temp_dir)},
        )

        assert returncode == 1
        output = json.loads(stdout)
        assert output["success"] is False
        assert output["error"] == "source_not_found"

    def test_missing_source_text_output(self, temp_dir):
        """Should show text message when source file is missing."""
        returncode, stdout, stderr = _run_rebuild_script(
            "--source", str(temp_dir / "nonexistent.jsonl"),
            env_override={"CODEFLOW_REPO_ROOT": str(temp_dir)},
        )

        assert returncode == 1
        assert "not found" in stdout


class TestCrdtRebuildDryRun:
    """Tests for --dry-run flag."""

    def test_dry_run_does_not_save(self, temp_dir):
        """Should not save state during dry run."""
        ledger_dir = temp_dir / ".state" / "ledger"
        ledger_dir.mkdir(parents=True)
        ledger_path = ledger_dir / "sessions.jsonl"

        now = datetime.now(timezone.utc)
        expires = (now + timedelta(seconds=600)).isoformat() + "Z"

        _create_jsonl_ledger(ledger_path, [
            {
                "type": "claim_created",
                "id": "claim-001",
                "work_id": "TSK-001",
                "pattern": "file:src/*.ts",
                "owner_id": "agent-001",
                "fencing_token": 1,
                "expires_at": expires,
                "created_at": now.isoformat() + "Z",
            },
        ])

        returncode, stdout, stderr = _run_rebuild_script(
            "--dry-run", "--json",
            "--source", str(ledger_path),
            env_override={"CODEFLOW_REPO_ROOT": str(temp_dir)},
        )

        assert returncode == 0
        output = json.loads(stdout)
        assert output["dry_run"] is True
        assert output["events_replayed"] == 1
        # State file should NOT exist
        coord_dir = temp_dir / ".state" / "coordination"
        assert not (coord_dir / "state.json").exists()

    def test_dry_run_text_output(self, temp_dir):
        """Should show text summary during dry run."""
        ledger_dir = temp_dir / ".state" / "ledger"
        ledger_dir.mkdir(parents=True)
        ledger_path = ledger_dir / "sessions.jsonl"

        _create_jsonl_ledger(ledger_path, [])

        returncode, stdout, stderr = _run_rebuild_script(
            "--dry-run",
            "--source", str(ledger_path),
            env_override={"CODEFLOW_REPO_ROOT": str(temp_dir)},
        )

        assert returncode == 0
        assert "DRY RUN" in stdout


class TestCrdtRebuildVerify:
    """Tests for --verify flag."""

    def test_verify_matching_state(self, temp_dir, monkeypatch):
        """Should report OK when rebuild matches existing state."""
        state_dir = temp_dir / ".state"
        state_dir.mkdir()
        monkeypatch.setattr(crdt_module, "get_state_dir", lambda: state_dir)

        now = datetime.now(timezone.utc)
        expires = (now + timedelta(seconds=600)).isoformat() + "Z"

        # Create existing state
        doc = CoordinationDoc()
        doc._claims["claim-001"] = {
            "id": "claim-001",
            "work_id": "TSK-001",
            "pattern": "file:src/*.ts",
            "mode": "exclusive",
            "owner_id": "agent-001",
            "fencing_token": 1,
            "expires_at": expires,
            "status": "active",
            "created_at": now.isoformat() + "Z",
        }
        doc._token_counter = 1
        save_coordination(doc)

        # Create matching JSONL
        ledger_dir = state_dir / "ledger"
        ledger_dir.mkdir(parents=True, exist_ok=True)
        ledger_path = ledger_dir / "sessions.jsonl"

        _create_jsonl_ledger(ledger_path, [
            {
                "type": "claim_created",
                "id": "claim-001",
                "work_id": "TSK-001",
                "pattern": "file:src/*.ts",
                "mode": "exclusive",
                "owner_id": "agent-001",
                "fencing_token": 1,
                "expires_at": expires,
                "created_at": now.isoformat() + "Z",
            },
        ])

        returncode, stdout, stderr = _run_rebuild_script(
            "--verify", "--json",
            "--source", str(ledger_path),
            env_override={"CODEFLOW_REPO_ROOT": str(temp_dir)},
        )

        assert returncode == 0
        output = json.loads(stdout)
        assert output["verify"] is True
        assert output["verified_ok"] is True
        assert len(output["mismatches"]) == 0

    def test_verify_detects_mismatch(self, temp_dir, monkeypatch):
        """Should detect mismatches between rebuild and existing state."""
        state_dir = temp_dir / ".state"
        state_dir.mkdir()
        monkeypatch.setattr(crdt_module, "get_state_dir", lambda: state_dir)

        now = datetime.now(timezone.utc)
        expires = (now + timedelta(seconds=600)).isoformat() + "Z"

        # Create existing state with extra claim
        doc = CoordinationDoc()
        doc._claims["claim-001"] = {
            "id": "claim-001",
            "work_id": "TSK-001",
            "pattern": "file:src/*.ts",
            "mode": "exclusive",
            "owner_id": "agent-001",
            "fencing_token": 1,
            "expires_at": expires,
            "status": "active",
            "created_at": now.isoformat() + "Z",
        }
        doc._claims["claim-extra"] = {
            "id": "claim-extra",
            "work_id": "TSK-999",
            "pattern": "dir:extra",
            "mode": "exclusive",
            "owner_id": "agent-999",
            "fencing_token": 99,
            "expires_at": expires,
            "status": "active",
            "created_at": now.isoformat() + "Z",
        }
        doc._token_counter = 99
        save_coordination(doc)

        # JSONL only has claim-001
        ledger_dir = state_dir / "ledger"
        ledger_dir.mkdir(parents=True, exist_ok=True)
        ledger_path = ledger_dir / "sessions.jsonl"

        _create_jsonl_ledger(ledger_path, [
            {
                "type": "claim_created",
                "id": "claim-001",
                "work_id": "TSK-001",
                "pattern": "file:src/*.ts",
                "owner_id": "agent-001",
                "fencing_token": 1,
                "expires_at": expires,
                "created_at": now.isoformat() + "Z",
            },
        ])

        returncode, stdout, stderr = _run_rebuild_script(
            "--verify", "--json",
            "--source", str(ledger_path),
            env_override={"CODEFLOW_REPO_ROOT": str(temp_dir)},
        )

        assert returncode == 0
        output = json.loads(stdout)
        assert output["verified_ok"] is False
        assert len(output["mismatches"]) > 0


class TestCrdtRebuildUntil:
    """Tests for --until point-in-time rebuild."""

    def test_until_filters_events(self, temp_dir):
        """Should only replay events up to the --until timestamp."""
        ledger_dir = temp_dir / ".state" / "ledger"
        ledger_dir.mkdir(parents=True)
        ledger_path = ledger_dir / "sessions.jsonl"

        base = datetime(2026, 1, 25, 10, 0, 0, tzinfo=timezone.utc)
        expires = (base + timedelta(hours=1)).isoformat() + "Z"

        _create_jsonl_ledger(ledger_path, [
            {
                "type": "claim_created",
                "id": "claim-001",
                "work_id": "TSK-001",
                "pattern": "file:src/*.ts",
                "owner_id": "agent-001",
                "fencing_token": 1,
                "expires_at": expires,
                "created_at": "2026-01-25T10:00:00Z",
                "ts": "2026-01-25T10:00:00Z",
            },
            {
                "type": "claim_created",
                "id": "claim-002",
                "work_id": "TSK-002",
                "pattern": "dir:src/auth",
                "owner_id": "agent-002",
                "fencing_token": 2,
                "expires_at": expires,
                "created_at": "2026-01-25T11:00:00Z",
                "ts": "2026-01-25T11:00:00Z",
            },
        ])

        # Rebuild until 10:30 - should only include first event
        returncode, stdout, stderr = _run_rebuild_script(
            "--json",
            "--source", str(ledger_path),
            "--until", "2026-01-25T10:30:00Z",
            env_override={"CODEFLOW_REPO_ROOT": str(temp_dir)},
        )

        assert returncode == 0
        output = json.loads(stdout)
        assert output["events_replayed"] == 1
        assert output["final_state"]["active_claims"] == 1
        assert output["rebuild_type"] == "point_in_time"


class TestCrdtRebuildBackup:
    """Tests for backup of existing state."""

    def test_creates_backup_before_rebuild(self, temp_dir, monkeypatch):
        """Should back up existing state before overwriting."""
        state_dir = temp_dir / ".state"
        state_dir.mkdir()
        monkeypatch.setattr(crdt_module, "get_state_dir", lambda: state_dir)

        # Create existing state
        doc = CoordinationDoc()
        doc.add_claim(
            claim_id="old-claim",
            work_id="TSK-OLD",
            pattern="file:old.ts",
            owner_id="agent-old",
        )
        save_coordination(doc)

        # Verify state file exists
        coord_dir = state_dir / "coordination"
        assert (coord_dir / "state.json").exists()

        # Create JSONL
        ledger_dir = state_dir / "ledger"
        ledger_dir.mkdir(parents=True, exist_ok=True)
        ledger_path = ledger_dir / "sessions.jsonl"

        now = datetime.now(timezone.utc)
        expires = (now + timedelta(seconds=600)).isoformat() + "Z"

        _create_jsonl_ledger(ledger_path, [
            {
                "type": "claim_created",
                "id": "claim-new",
                "work_id": "TSK-NEW",
                "pattern": "file:new.ts",
                "owner_id": "agent-new",
                "fencing_token": 1,
                "expires_at": expires,
                "created_at": now.isoformat() + "Z",
            },
        ])

        returncode, stdout, stderr = _run_rebuild_script(
            "--json",
            "--source", str(ledger_path),
            env_override={"CODEFLOW_REPO_ROOT": str(temp_dir)},
        )

        assert returncode == 0
        output = json.loads(stdout)
        assert "backup_created" in output

        # Verify backup file exists
        backup_path = output["backup_created"]
        assert Path(backup_path).exists()


class TestCrdtRebuildEmptyFile:
    """Tests for empty JSONL files."""

    def test_empty_jsonl_rebuilds_empty_state(self, temp_dir):
        """Should handle empty JSONL file gracefully."""
        ledger_dir = temp_dir / ".state" / "ledger"
        ledger_dir.mkdir(parents=True)
        ledger_path = ledger_dir / "sessions.jsonl"
        ledger_path.write_text("")

        returncode, stdout, stderr = _run_rebuild_script(
            "--json",
            "--source", str(ledger_path),
            env_override={"CODEFLOW_REPO_ROOT": str(temp_dir)},
        )

        assert returncode == 0
        output = json.loads(stdout)
        assert output["events_replayed"] == 0
        assert output["final_state"]["active_claims"] == 0


class TestCrdtRebuildDuration:
    """Tests for duration tracking."""

    def test_output_includes_duration_ms(self, temp_dir):
        """Should include duration_ms in output."""
        ledger_dir = temp_dir / ".state" / "ledger"
        ledger_dir.mkdir(parents=True)
        ledger_path = ledger_dir / "sessions.jsonl"
        ledger_path.write_text("")

        returncode, stdout, stderr = _run_rebuild_script(
            "--json",
            "--source", str(ledger_path),
            env_override={"CODEFLOW_REPO_ROOT": str(temp_dir)},
        )

        assert returncode == 0
        output = json.loads(stdout)
        assert "duration_ms" in output
        assert isinstance(output["duration_ms"], int)
        assert output["duration_ms"] >= 0


class TestCrdtRebuildFencingToken:
    """Tests for fencing token tracking."""

    def test_max_fencing_token_tracked(self, temp_dir):
        """Should track the maximum fencing token during rebuild."""
        ledger_dir = temp_dir / ".state" / "ledger"
        ledger_dir.mkdir(parents=True)
        ledger_path = ledger_dir / "sessions.jsonl"

        now = datetime.now(timezone.utc)
        expires = (now + timedelta(seconds=600)).isoformat() + "Z"

        _create_jsonl_ledger(ledger_path, [
            {
                "type": "claim_created",
                "id": "claim-001",
                "work_id": "TSK-001",
                "pattern": "file:a.ts",
                "owner_id": "agent-001",
                "fencing_token": 5,
                "expires_at": expires,
                "created_at": now.isoformat() + "Z",
            },
            {
                "type": "claim_created",
                "id": "claim-002",
                "work_id": "TSK-002",
                "pattern": "file:b.ts",
                "owner_id": "agent-002",
                "fencing_token": 42,
                "expires_at": expires,
                "created_at": now.isoformat() + "Z",
            },
        ])

        returncode, stdout, stderr = _run_rebuild_script(
            "--json",
            "--source", str(ledger_path),
            env_override={"CODEFLOW_REPO_ROOT": str(temp_dir)},
        )

        assert returncode == 0
        output = json.loads(stdout)
        assert output["final_state"]["max_fencing_token"] == 42


class TestCrdtRebuildTextOutput:
    """Tests for text output mode."""

    def test_text_output_on_success(self, temp_dir):
        """Should show text summary on success."""
        ledger_dir = temp_dir / ".state" / "ledger"
        ledger_dir.mkdir(parents=True)
        ledger_path = ledger_dir / "sessions.jsonl"

        now = datetime.now(timezone.utc)
        expires = (now + timedelta(seconds=600)).isoformat() + "Z"

        _create_jsonl_ledger(ledger_path, [
            {
                "type": "claim_created",
                "id": "claim-001",
                "work_id": "TSK-001",
                "pattern": "file:src/*.ts",
                "owner_id": "agent-001",
                "fencing_token": 1,
                "expires_at": expires,
                "created_at": now.isoformat() + "Z",
            },
        ])

        returncode, stdout, stderr = _run_rebuild_script(
            "--source", str(ledger_path),
            env_override={"CODEFLOW_REPO_ROOT": str(temp_dir)},
        )

        assert returncode == 0
        assert "rebuilt successfully" in stdout
        assert "Events replayed:" in stdout
        assert "Duration:" in stdout
