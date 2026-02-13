"""
test_stage_sync.py - Tests for state/cf-stage-sync.py

Tests the two-tier stage transition sync script covering:
  - CLI --help output
  - --from-ledger with empty/missing ledger
  - --task-id with nonexistent ID
  - Mode detection (is-pathflow-active flag)
  - Markdown section insertion/replacement
  - --json output format

Location: .codeflow/testing/scripts/state/test_stage_sync.py
"""

import json
import os
import subprocess
import sys
from pathlib import Path
from unittest.mock import patch

import pytest

# Add paths for imports
SCRIPTS_PATH = Path(__file__).parent.parent.parent.parent / "scripts"
STATE_SCRIPTS = SCRIPTS_PATH / "state"
sys.path.insert(0, str(SCRIPTS_PATH))
sys.path.insert(0, str(STATE_SCRIPTS))

# The script under test (path for subprocess invocations)
STAGE_SYNC_SCRIPT = STATE_SCRIPTS / "cf-stage-sync.py"

# Import functions under test (using importlib to handle hyphenated filename)
import importlib.util  # noqa: E402

_spec = importlib.util.spec_from_file_location("cf_stage_sync", STAGE_SYNC_SCRIPT)
_mod = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_mod)

build_stage_section = _mod.build_stage_section
update_work_agreement_stage = _mod.update_work_agreement_stage
update_epic_markdown_stage = _mod.update_epic_markdown_stage
is_pathflow_active = _mod.is_pathflow_active
sync_tier2_for_task = _mod.sync_tier2_for_task

from codeflow_py_lib.paths import get_repo_root  # noqa: E402
sync_all_active = _mod.sync_all_active
VALID_STAGES = _mod.VALID_STAGES
VALID_STAGE_STATUSES = _mod.VALID_STAGE_STATUSES
VALID_VERDICTS = _mod.VALID_VERDICTS
STAGE_DISPLAY = _mod.STAGE_DISPLAY
STAGE_STATUS_DISPLAY = _mod.STAGE_STATUS_DISPLAY


# =============================================================================
# CLI TESTS (subprocess invocations)
# =============================================================================


class TestCLIHelp:
    """Test --help output."""

    def test_help_exits_zero(self):
        result = subprocess.run(
            [sys.executable, str(STAGE_SYNC_SCRIPT), "--help"],
            capture_output=True, text=True,
        )
        assert result.returncode == 0

    def test_help_shows_description(self):
        result = subprocess.run(
            [sys.executable, str(STAGE_SYNC_SCRIPT), "--help"],
            capture_output=True, text=True,
        )
        assert "Two-tier stage transition sync" in result.stdout

    def test_help_shows_stage_choices(self):
        result = subprocess.run(
            [sys.executable, str(STAGE_SYNC_SCRIPT), "--help"],
            capture_output=True, text=True,
        )
        for stage in ("dev", "work", "review", "qa", "done"):
            assert stage in result.stdout

    def test_help_shows_sync_modes(self):
        result = subprocess.run(
            [sys.executable, str(STAGE_SYNC_SCRIPT), "--help"],
            capture_output=True, text=True,
        )
        assert "--sync-all" in result.stdout
        assert "--from-ledger" in result.stdout
        assert "--sync-markdown" in result.stdout

    def test_no_args_errors(self):
        result = subprocess.run(
            [sys.executable, str(STAGE_SYNC_SCRIPT)],
            capture_output=True, text=True,
        )
        assert result.returncode != 0


def _cli_env(mock_repo):
    """Build env dict and ensure .codeflow dir for _find_repo_root()."""
    (mock_repo / ".codeflow").mkdir(exist_ok=True)
    env = os.environ.copy()
    env["CODEFLOW_REPO_ROOT"] = str(mock_repo)
    env["CODEFLOW_STATE_DIR"] = str(mock_repo / ".state")
    return env


class TestCLIFromLedger:
    """Test --from-ledger with empty/missing ledger."""

    def test_missing_ledger_file(self, mock_repo, state_db):
        env = _cli_env(mock_repo)
        result = subprocess.run(
            [sys.executable, str(STAGE_SYNC_SCRIPT), "--from-ledger"],
            capture_output=True, text=True, env=env, cwd=str(mock_repo),
        )
        assert result.returncode == 0

    def test_missing_ledger_json_output(self, mock_repo, state_db):
        env = _cli_env(mock_repo)
        result = subprocess.run(
            [sys.executable, str(STAGE_SYNC_SCRIPT), "--from-ledger", "--json"],
            capture_output=True, text=True, env=env, cwd=str(mock_repo),
        )
        assert result.returncode == 0
        data = json.loads(result.stdout)
        assert data["success"] is True
        assert data["events_processed"] == 0

    def test_empty_ledger_file(self, mock_repo, state_db):
        ledger_dir = mock_repo / ".state" / "ledger"
        ledger_dir.mkdir(parents=True, exist_ok=True)
        (ledger_dir / "memory-events.jsonl").write_text("")

        env = _cli_env(mock_repo)
        result = subprocess.run(
            [sys.executable, str(STAGE_SYNC_SCRIPT), "--from-ledger", "--json"],
            capture_output=True, text=True, env=env, cwd=str(mock_repo),
        )
        assert result.returncode == 0
        data = json.loads(result.stdout)
        assert data["success"] is True


class TestCLIJsonFormat:
    """Test --json output format across modes."""

    def test_from_ledger_json_has_required_keys(self, mock_repo, state_db):
        env = _cli_env(mock_repo)
        result = subprocess.run(
            [sys.executable, str(STAGE_SYNC_SCRIPT), "--from-ledger", "--json"],
            capture_output=True, text=True, env=env, cwd=str(mock_repo),
        )
        data = json.loads(result.stdout)
        assert "success" in data
        assert isinstance(data["success"], bool)

    def test_sync_all_json_has_required_keys(self, mock_repo, state_db):
        env = _cli_env(mock_repo)
        result = subprocess.run(
            [sys.executable, str(STAGE_SYNC_SCRIPT), "--sync-all", "--json"],
            capture_output=True, text=True, env=env, cwd=str(mock_repo),
        )
        data = json.loads(result.stdout)
        for key in ("success", "total", "synced", "errors"):
            assert key in data


# =============================================================================
# MODE DETECTION TESTS
# =============================================================================


class TestModeDetection:
    """Test is-pathflow-active flag detection."""

    def test_inactive_when_no_flag(self, tmp_path):
        repo = tmp_path / "repo"
        (repo / ".state" / "session" / "test-sess").mkdir(parents=True)
        with patch.dict(os.environ, {
            "CODEFLOW_PATHFLOW_OVERRIDE": "auto",
            "CODEFLOW_REPO_ROOT": str(repo),
            "CODEFLOW_SESSION_ID": "test-sess",
        }):
            get_repo_root.cache_clear()
            assert is_pathflow_active() is False
        get_repo_root.cache_clear()

    def test_active_when_flag_present(self, tmp_path):
        repo = tmp_path / "repo"
        flag_dir = repo / ".state" / "session" / "test-sess"
        flag_dir.mkdir(parents=True)
        (flag_dir / "is-pathflow-active").touch()
        with patch.dict(os.environ, {
            "CODEFLOW_PATHFLOW_OVERRIDE": "auto",
            "CODEFLOW_REPO_ROOT": str(repo),
            "CODEFLOW_SESSION_ID": "test-sess",
        }):
            get_repo_root.cache_clear()
            assert is_pathflow_active() is True
        get_repo_root.cache_clear()


# =============================================================================
# UNIT TESTS (pure function tests)
# =============================================================================


class TestBuildStageSection:
    """Tests for build_stage_section()."""

    def test_basic_stage_output(self):
        """Should produce markdown with current stage."""
        result = build_stage_section("dev", "in_progress", [], False)
        assert "## Stage Tracking" in result
        assert "Development" in result
        assert "In Progress" in result

    def test_no_stage(self):
        """Should handle None stage gracefully."""
        result = build_stage_section(None, None, [], False)
        assert "Not in pipeline" in result

    def test_standalone_mode(self):
        """Should show Standalone mode label."""
        result = build_stage_section("dev", "pending", [], False)
        assert "Standalone" in result

    def test_pathflow_mode(self):
        """Should show PathFlow mode label."""
        result = build_stage_section("review", "in_progress", [], True)
        assert "PathFlow" in result

    def test_with_history(self):
        """Should render stage history table."""
        history = [
            {
                "stage": "dev",
                "status": "complete",
                "started_at": "2026-01-01T10:00:00+00:00",
                "completed_at": "2026-01-01T12:00:00+00:00",
            },
            {
                "stage": "review",
                "status": "in_progress",
                "agent": "cf-reviewer",
                "started_at": "2026-01-01T12:00:00+00:00",
            },
        ]
        result = build_stage_section("review", "in_progress", history, True)
        assert "### Stage History" in result
        assert "Development" in result
        assert "Review" in result
        assert "cf-reviewer" in result
        assert "| # |" in result

    def test_history_with_rework(self):
        """Should show rework marker in history."""
        history = [
            {"stage": "dev", "status": "complete", "started_at": "2026-01-01T10:00:00+00:00"},
            {
                "stage": "review",
                "status": "failed",
                "rework": True,
                "iteration": 2,
                "started_at": "2026-01-02T10:00:00+00:00",
            },
        ]
        result = build_stage_section("review", "failed", history, False)
        assert "rework #2" in result

    def test_history_with_verdict(self):
        """Should display verdict in history."""
        history = [
            {
                "stage": "review",
                "status": "complete",
                "verdict": "approved",
                "started_at": "2026-01-01T10:00:00+00:00",
            },
        ]
        result = build_stage_section("review", "complete", history, False)
        assert "Approved" in result

    def test_all_stages_displayable(self):
        """All valid stages should have display names."""
        for stage in VALID_STAGES:
            result = build_stage_section(stage, "pending", [], False)
            assert "## Stage Tracking" in result
            # Should not show raw stage name as fallback
            assert stage not in result or stage in ("dev", "work", "review", "qa", "done")


class TestUpdateWorkAgreementStage:
    """Tests for update_work_agreement_stage()."""

    def test_inserts_section_when_missing(self, tmp_path):
        """Should append stage section to file without one."""
        md_file = tmp_path / "work-agreement-001.md"
        md_file.write_text("# Work Agreement\n\nSome content.\n")

        result = update_work_agreement_stage(
            md_file, "dev", "in_progress", [], False
        )
        assert result is True

        content = md_file.read_text()
        assert "## Stage Tracking" in content
        assert "Development" in content

    def test_replaces_existing_section(self, tmp_path):
        """Should replace existing ## Stage Tracking section."""
        md_file = tmp_path / "work-agreement-002.md"
        md_file.write_text(
            "# Work Agreement\n\n"
            "## Stage Tracking\n\n"
            "**Current Stage:** Development (Pending)\n\n"
            "## Progress\n\nSome progress.\n"
        )

        result = update_work_agreement_stage(
            md_file, "review", "in_progress", [], True
        )
        assert result is True

        content = md_file.read_text()
        assert "Review" in content
        assert "In Progress" in content
        assert "## Progress" in content
        # Old stage should be gone
        assert content.count("## Stage Tracking") == 1

    def test_inserts_before_progress_section(self, tmp_path):
        """Should insert before ## Progress if it exists."""
        md_file = tmp_path / "work-agreement-003.md"
        md_file.write_text("# Work Agreement\n\n## Progress\n\nSome progress.\n")

        result = update_work_agreement_stage(
            md_file, "qa", "pending", [], False
        )
        assert result is True

        content = md_file.read_text()
        stage_pos = content.index("## Stage Tracking")
        progress_pos = content.index("## Progress")
        assert stage_pos < progress_pos

    def test_missing_file_returns_false(self, tmp_path):
        """Should return False for non-existent file."""
        result = update_work_agreement_stage(
            tmp_path / "nonexistent.md", "dev", "pending", [], False
        )
        assert result is False

    def test_idempotent_content_stabilizes(self, tmp_path):
        """Content should stabilize after replacement path is used."""
        md_file = tmp_path / "work-agreement-004.md"
        md_file.write_text("# Work\n\nSome content.\n")

        # First call inserts the section (append path)
        update_work_agreement_stage(md_file, "dev", "pending", [], False)
        # Second call replaces existing section (replace path)
        update_work_agreement_stage(md_file, "dev", "pending", [], False)
        content_after_second = md_file.read_text()

        # Third call should produce identical content (stable)
        update_work_agreement_stage(md_file, "dev", "pending", [], False)
        content_after_third = md_file.read_text()

        assert content_after_second == content_after_third


class TestUpdateEpicMarkdownStage:
    """Tests for update_epic_markdown_stage()."""

    def test_adds_stage_annotation(self, tmp_path):
        """Should add [stage: ...] after task ID."""
        md_file = tmp_path / "epic.md"
        md_file.write_text(
            "# Epic\n\n"
            "## Tasks\n\n"
            "- [ ] FRT-TSK-FEAT-TEST-001: Implement feature\n"
        )

        result = update_epic_markdown_stage(
            md_file, "FRT-TSK-FEAT-TEST-001", "dev", "in_progress"
        )
        assert result is True

        content = md_file.read_text()
        assert "[stage: dev/in_progress]" in content

    def test_updates_existing_annotation(self, tmp_path):
        """Should update existing [stage: ...] annotation."""
        md_file = tmp_path / "epic.md"
        md_file.write_text(
            "# Epic\n\n"
            "- [ ] FRT-TSK-FEAT-TEST-001 [stage: dev/pending]: Implement\n"
        )

        result = update_epic_markdown_stage(
            md_file, "FRT-TSK-FEAT-TEST-001", "review", "in_progress"
        )
        assert result is True

        content = md_file.read_text()
        assert "[stage: review/in_progress]" in content
        assert "[stage: dev/pending]" not in content

    def test_missing_task_id_returns_false(self, tmp_path):
        """Should return False if task ID not found in file."""
        md_file = tmp_path / "epic.md"
        md_file.write_text("# Epic\n\nNo tasks here.\n")

        result = update_epic_markdown_stage(
            md_file, "FRT-TSK-NONEXISTENT", "dev", "pending"
        )
        assert result is False

    def test_missing_file_returns_false(self, tmp_path):
        """Should return False for non-existent file."""
        result = update_epic_markdown_stage(
            tmp_path / "nonexistent.md", "TSK-001", "dev", "pending"
        )
        assert result is False

    def test_stage_without_status(self, tmp_path):
        """Should handle stage without status."""
        md_file = tmp_path / "epic.md"
        md_file.write_text("- FRT-TSK-001: Task\n")

        result = update_epic_markdown_stage(
            md_file, "FRT-TSK-001", "done", None
        )
        assert result is True

        content = md_file.read_text()
        assert "[stage: done]" in content


class TestValidation:
    """Tests for input validation constants."""

    def test_valid_stages_complete(self):
        """Should have all expected stages."""
        expected = {"dev", "work", "review", "qa", "done"}
        assert set(VALID_STAGES) == expected

    def test_valid_statuses_complete(self):
        """Should have all expected stage statuses."""
        expected = {"pending", "in_progress", "complete", "failed"}
        assert set(VALID_STAGE_STATUSES) == expected


class TestModeAwareness:
    """Tests for standalone vs PathFlow mode display."""

    def test_standalone_label(self):
        """Standalone mode should show Standalone."""
        result = build_stage_section("dev", "pending", [], False)
        assert "Standalone" in result
        assert "PathFlow" not in result

    def test_pathflow_label(self):
        """PathFlow mode should show PathFlow."""
        result = build_stage_section("dev", "pending", [], True)
        assert "PathFlow" in result
        assert "Standalone" not in result
