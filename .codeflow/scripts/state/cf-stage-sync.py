#!/usr/bin/env python3
"""
cf-stage-sync.py - Two-tier stage transition sync.

Records stage transitions across two tiers of the CodeFlow data model:
  Tier 0: Appends stage_transition event to memory-events.jsonl
  Tier 2: Updates task markdown file with stage info and history section

Tier 1 (SQLite) is handled by the Go CLI via ``codeflow db sync``.

Usage:
    # Record a stage transition
    cf-stage-sync.py --task-id FRT-TSK-FEAT-AUTH-001 --stage review --stage-status pending
    cf-stage-sync.py --task-id FRT-TSK-FEAT-AUTH-001 --stage review --stage-status complete \\
        --agent cf-reviewer --verdict approved --notes "Code looks good"

    # Sync Tier 2 markdown from JSONL ledger data
    cf-stage-sync.py --task-id FRT-TSK-FEAT-AUTH-001 --sync-markdown
    cf-stage-sync.py --sync-all
    cf-stage-sync.py --from-ledger

Exit Codes:
    0: Success
    1: Invalid arguments
    2: Unexpected error
"""

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any, Dict, List, Optional

# Add codeflow_py_lib to path
sys.path.insert(0, str(Path(__file__).parent.parent))

from codeflow_py_lib import (  # noqa: E402
    ValidationError,
    append_jsonl,
    generate_ulid,
    get_logger,
    get_repo_root,
    get_state_dir,
    is_pathflow_active,
    read_jsonl,
)

logger = get_logger(__name__)

# Valid values for stage pipeline fields
VALID_STAGES = ("dev", "work", "review", "qa", "done")
VALID_STAGE_STATUSES = ("pending", "in_progress", "complete", "failed")
VALID_VERDICTS = ("pass", "approved", "changes_requested", "fail")

# Stage display names for markdown rendering
STAGE_DISPLAY = {
    "dev": "Development",
    "work": "Work",
    "review": "Review",
    "qa": "QA",
    "done": "Done",
}

STAGE_STATUS_DISPLAY = {
    "pending": "Pending",
    "in_progress": "In Progress",
    "complete": "Complete",
    "failed": "Failed",
}

VERDICT_DISPLAY = {
    "pass": "Passed",
    "approved": "Approved",
    "changes_requested": "Changes Requested",
    "fail": "Failed",
}

# Domain mapping for stage events (from V4 spec)
STAGE_DOMAIN_MAP = {
    "dev": "development",
    "work": "development",  # overridden by work_type for DOCS/SPKE
    "review": "review",
    "qa": "qa",
    "done": "development",
}


# =============================================================================
# TIER 0: JSONL LEDGER
# =============================================================================


def append_stage_event_to_ledger(
    task_id: str,
    work_id: Optional[str],
    from_stage: Optional[str],
    from_status: Optional[str],
    to_stage: str,
    to_status: str,
    agent: Optional[str] = None,
    verdict: Optional[str] = None,
    notes: Optional[str] = None,
    rework: bool = False,
    iteration: int = 1,
) -> Dict[str, Any]:
    """Append a stage_transition event to the memory-events JSONL ledger.

    Returns:
        The complete event dict that was written.
    """
    ledger_path = get_state_dir() / "ledger" / "memory-events.jsonl"
    domain = STAGE_DOMAIN_MAP.get(to_stage, "development")

    event_data = {
        "task_id": task_id,
        "from_stage": from_stage,
        "from_status": from_status,
        "to_stage": to_stage,
        "to_status": to_status,
    }
    if agent:
        event_data["agent"] = agent
    if verdict:
        event_data["verdict"] = verdict
    if notes:
        event_data["notes"] = notes
    if rework:
        event_data["rework"] = True
        event_data["iteration"] = iteration

    event = {
        "type": "stage_transition",
        "id": f"memory-{generate_ulid()}",
        "event_type": "stage_transition",
        "domain": domain,
        "work_id": work_id,
        "data": event_data,
        "memory_type": "episodic",
    }

    written = append_jsonl(ledger_path, event)
    logger.info("Tier 0: Appended stage_transition to ledger: %s", written.get("id"))
    return written


# =============================================================================
# TIER 2: MARKDOWN FILES
# =============================================================================


def find_work_agreement(work_id: str) -> Optional[Path]:
    """Find existing work agreement file for a given work ID.

    Searches .claude/memory/ directories for a file containing the work ID.
    """
    repo_root = get_repo_root()
    memory_dir = repo_root / ".claude" / "memory"

    if not memory_dir.exists():
        return None

    for domain_dir in memory_dir.iterdir():
        if not domain_dir.is_dir():
            continue
        for md_file in domain_dir.glob("work-agreement-*.md"):
            try:
                content = md_file.read_text(encoding="utf-8")
                if work_id in content:
                    return md_file
            except (OSError, UnicodeDecodeError):
                continue

    return None


def find_task_markdown(task_id: str) -> Optional[Path]:
    """Find an epic/task markdown file that references the given task ID.

    Searches project-management/epics/ directory for markdown files containing the task ID.
    """
    repo_root = get_repo_root()
    epics_dir = repo_root / "project-management" / "epics"

    if not epics_dir.exists():
        return None

    for md_file in epics_dir.rglob("*.md"):
        try:
            content = md_file.read_text(encoding="utf-8")
            if task_id in content:
                return md_file
        except (OSError, UnicodeDecodeError):
            continue

    return None


def build_stage_section(
    stage: Optional[str],
    stage_status: Optional[str],
    stage_history: List[Dict[str, Any]],
    pathflow_active: bool,
) -> str:
    """Build the ## Stage Tracking section for markdown."""
    lines: List[str] = []
    lines.append("## Stage Tracking")
    lines.append("")

    if stage:
        stage_name = STAGE_DISPLAY.get(stage, stage)
        status_name = STAGE_STATUS_DISPLAY.get(stage_status or "", stage_status or "unknown")
        lines.append(f"**Current Stage:** {stage_name} ({status_name})")
    else:
        lines.append("**Current Stage:** Not in pipeline")

    if pathflow_active:
        lines.append("**Mode:** PathFlow (full pipeline)")
    else:
        lines.append("**Mode:** Standalone")

    lines.append("")

    if stage_history:
        lines.append("### Stage History")
        lines.append("")
        lines.append("| # | Stage | Status | Agent | Verdict | Time |")
        lines.append("|---|-------|--------|-------|---------|------|")

        for i, entry in enumerate(stage_history, 1):
            entry_stage = STAGE_DISPLAY.get(entry.get("stage", ""), entry.get("stage", "?"))
            entry_status = entry.get("status", "?")
            agent = entry.get("agent", "-")
            verdict = VERDICT_DISPLAY.get(entry.get("verdict", ""), entry.get("verdict", "-"))
            completed = entry.get("completed_at", entry.get("started_at", ""))
            if completed:
                completed = completed[:19].replace("T", " ")

            rework_marker = ""
            if entry.get("rework"):
                iteration = entry.get("iteration", "?")
                rework_marker = f" (rework #{iteration})"

            lines.append(
                f"| {i} | {entry_stage}{rework_marker} | {entry_status} "
                f"| {agent} | {verdict} | {completed} |"
            )

        lines.append("")

    return "\n".join(lines)


def update_work_agreement_stage(
    file_path: Path,
    stage: Optional[str],
    stage_status: Optional[str],
    stage_history: List[Dict[str, Any]],
    pathflow_active: bool,
) -> bool:
    """Update ## Stage Tracking section in a work agreement markdown file.

    If the file has an existing section, it is replaced. Otherwise the section
    is inserted before ## Progress or appended at the end.

    Returns True if file was updated.
    """
    if not file_path.exists():
        logger.warning("Work agreement not found: %s", file_path)
        return False

    try:
        content = file_path.read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError) as e:
        logger.error("Failed to read %s: %s", file_path, e)
        return False

    stage_section = build_stage_section(stage, stage_status, stage_history, pathflow_active)

    # Replace existing section or insert
    stage_pattern = re.compile(r"## Stage Tracking\n.*?(?=\n## |\Z)", re.DOTALL)
    match = stage_pattern.search(content)

    if match:
        new_content = content[:match.start()] + stage_section + content[match.end():]
    else:
        progress_match = re.search(r"\n## Progress\b", content)
        if progress_match:
            insert_pos = progress_match.start()
            new_content = content[:insert_pos] + "\n" + stage_section + "\n" + content[insert_pos:]
        else:
            new_content = content.rstrip() + "\n\n" + stage_section + "\n"

    if new_content == content:
        return False

    try:
        file_path.parent.mkdir(parents=True, exist_ok=True)
        file_path.write_text(new_content, encoding="utf-8")
        logger.info("Tier 2: Updated stage tracking in %s", file_path)
        return True
    except OSError as e:
        logger.error("Failed to write %s: %s", file_path, e)
        return False


def update_epic_markdown_stage(
    file_path: Path, task_id: str, stage: Optional[str], stage_status: Optional[str]
) -> bool:
    """Update task stage annotation within an epic markdown file.

    Adds or updates [stage: dev/in_progress] next to the task ID reference.

    Returns True if file was updated.
    """
    if not file_path.exists():
        return False

    try:
        content = file_path.read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError) as e:
        logger.error("Failed to read %s: %s", file_path, e)
        return False

    task_pattern = re.compile(
        rf"({re.escape(task_id)})" r"(\s*\[stage:\s*[^\]]*\])?"
    )

    stage_label = ""
    if stage:
        status_str = f"/{stage_status}" if stage_status else ""
        stage_label = f" [stage: {stage}{status_str}]"

    new_content, count = task_pattern.subn(rf"\g<1>{stage_label}", content)

    if count == 0 or new_content == content:
        return False

    try:
        file_path.write_text(new_content, encoding="utf-8")
        logger.info("Tier 2: Updated epic markdown %s for task %s", file_path, task_id)
        return True
    except OSError as e:
        logger.error("Failed to write %s: %s", file_path, e)
        return False


def _read_stage_from_ledger(task_id: str) -> Dict[str, Any]:
    """Read the latest stage info for a task from the JSONL ledger.

    Returns dict with stage, stage_status, stage_history, work_id.
    """
    ledger_path = get_state_dir() / "ledger" / "memory-events.jsonl"
    if not ledger_path.exists():
        return {}

    stage = None
    stage_status = None
    stage_history: List[Dict[str, Any]] = []
    work_id = None

    for event in read_jsonl(ledger_path, skip_errors=True):
        event_type = event.get("event_type") or event.get("e")
        if event_type != "stage_transition":
            continue
        data = event.get("data") or event.get("d", {})
        if isinstance(data, str):
            try:
                data = json.loads(data)
            except json.JSONDecodeError:
                continue
        if not isinstance(data, dict) or data.get("task_id") != task_id:
            continue

        stage = data.get("to_stage", stage)
        stage_status = data.get("to_status", stage_status)
        if not work_id:
            work_id = event.get("work_id")

        history_entry: Dict[str, Any] = {
            "stage": data.get("to_stage", ""),
            "status": data.get("to_status", ""),
            "started_at": event.get("ts", ""),
        }
        if data.get("agent"):
            history_entry["agent"] = data["agent"]
        if data.get("verdict"):
            history_entry["verdict"] = data["verdict"]
        if data.get("rework"):
            history_entry["rework"] = True
            history_entry["iteration"] = data.get("iteration", 1)
        stage_history.append(history_entry)

    return {
        "stage": stage,
        "stage_status": stage_status,
        "stage_history": stage_history,
        "work_id": work_id,
    }


def _sync_tier2_with_info(
    task_id: str,
    stage: Optional[str],
    stage_status: Optional[str],
    stage_history: List[Dict[str, Any]],
    work_id: Optional[str],
) -> Dict[str, bool]:
    """Sync Tier 2 markdown using pre-loaded stage info (no ledger read)."""
    pathflow_active = is_pathflow_active()
    updates = {"work_agreement": False, "epic_markdown": False}

    if not stage:
        logger.warning("No stage data for task: %s", task_id)
        return updates

    if work_id:
        agreement_path = find_work_agreement(work_id)
        if agreement_path:
            updates["work_agreement"] = update_work_agreement_stage(
                agreement_path, stage, stage_status, stage_history, pathflow_active
            )

    epic_md = find_task_markdown(task_id)
    if epic_md:
        updates["epic_markdown"] = update_epic_markdown_stage(
            epic_md, task_id, stage, stage_status
        )

    return updates


def sync_tier2_for_task(task_id: str) -> Dict[str, bool]:
    """Read stage state from JSONL ledger and sync Tier 2 markdown for a task."""
    info = _read_stage_from_ledger(task_id)
    return _sync_tier2_with_info(
        task_id,
        info.get("stage"),
        info.get("stage_status"),
        info.get("stage_history", []),
        info.get("work_id"),
    )


# =============================================================================
# ORCHESTRATION: TWO-TIER STAGE TRANSITION
# =============================================================================


def record_stage_transition(
    task_id: str,
    stage: str,
    stage_status: str,
    agent: Optional[str] = None,
    verdict: Optional[str] = None,
    notes: Optional[str] = None,
    rework: bool = False,
    iteration: int = 1,
) -> Dict[str, Any]:
    """Record a stage transition across Tier 0 and Tier 2.

    This is the primary entry point for stage changes. It writes:
      Tier 0: JSONL ledger event
      Tier 2: Markdown file updates

    In PathFlow mode, only cf-knowledge-layer should call this.

    Args:
        task_id: Task ID (e.g., FRT-TSK-FEAT-AUTH-001)
        stage: New stage (dev, work, review, qa, done)
        stage_status: New stage status (pending, in_progress, complete, failed)
        agent: Agent performing the stage (e.g., cf-developer)
        verdict: Stage outcome (pass, approved, changes_requested, fail)
        notes: Optional notes about the transition
        rework: Whether this is a rework iteration
        iteration: Rework iteration number (1 = first pass)

    Returns:
        Result dict with success status and tier details
    """
    # Validate inputs
    if stage not in VALID_STAGES:
        raise ValidationError(f"Invalid stage: {stage}. Must be one of {VALID_STAGES}")
    if stage_status not in VALID_STAGE_STATUSES:
        raise ValidationError(
            f"Invalid stage_status: {stage_status}. Must be one of {VALID_STAGE_STATUSES}"
        )
    if verdict and verdict not in VALID_VERDICTS:
        raise ValidationError(f"Invalid verdict: {verdict}. Must be one of {VALID_VERDICTS}")

    result: Dict[str, Any] = {
        "success": False,
        "task_id": task_id,
        "stage": stage,
        "stage_status": stage_status,
        "tier0": None,
        "tier2": None,
    }

    # Read previous stage from ledger for the transition event
    prev_info = _read_stage_from_ledger(task_id)
    prev_stage = prev_info.get("stage")
    prev_status = prev_info.get("stage_status")
    prev_history = prev_info.get("stage_history", [])
    work_id = prev_info.get("work_id")

    # Tier 0: Append to JSONL ledger (must succeed before Tier 2)
    try:
        ledger_event = append_stage_event_to_ledger(
            task_id=task_id,
            work_id=work_id,
            from_stage=prev_stage,
            from_status=prev_status,
            to_stage=stage,
            to_status=stage_status,
            agent=agent,
            verdict=verdict,
            notes=notes,
            rework=rework,
            iteration=iteration,
        )
        result["tier0"] = {"event_id": ledger_event.get("id")}
    except Exception as e:
        logger.error("Tier 0 failed: %s", e)
        result["tier0"] = {"error": str(e)}
        return result

    # Build updated history in-memory (avoid re-reading ledger)
    new_entry: Dict[str, Any] = {
        "stage": stage,
        "status": stage_status,
        "started_at": ledger_event.get("ts", ""),
    }
    if agent:
        new_entry["agent"] = agent
    if verdict:
        new_entry["verdict"] = verdict
    if rework:
        new_entry["rework"] = True
        new_entry["iteration"] = iteration
    updated_history = prev_history + [new_entry]

    # Tier 2: Update markdown using in-memory state
    try:
        tier2_result = _sync_tier2_with_info(
            task_id, stage, stage_status, updated_history, work_id
        )
        result["tier2"] = tier2_result
    except Exception as e:
        logger.error("Tier 2 failed: %s", e)
        result["tier2"] = {"error": str(e)}

    result["success"] = True
    return result


# =============================================================================
# SYNC-ONLY MODES (Tier 2 from JSONL ledger data)
# =============================================================================


def sync_all_active() -> Dict[str, Any]:
    """Sync Tier 2 markdown for all tasks with stage data in the ledger."""
    ledger_path = get_state_dir() / "ledger" / "memory-events.jsonl"

    if not ledger_path.exists():
        return {"success": True, "total": 0, "synced": 0, "errors": 0}

    # Collect unique task IDs from stage_transition events
    task_ids: set = set()
    for event in read_jsonl(ledger_path, skip_errors=True):
        event_type = event.get("event_type") or event.get("e")
        if event_type == "stage_transition":
            data = event.get("data") or event.get("d", {})
            if isinstance(data, str):
                try:
                    data = json.loads(data)
                except json.JSONDecodeError:
                    data = {}
            task_id = data.get("task_id") if isinstance(data, dict) else None
            if task_id:
                task_ids.add(task_id)

    results = {"success": True, "total": len(task_ids), "synced": 0, "errors": 0}

    for task_id in task_ids:
        try:
            updates = sync_tier2_for_task(task_id)
            if any(updates.values()):
                results["synced"] += 1
        except Exception as e:
            logger.error("Sync failed for %s: %s", task_id, e)
            results["errors"] += 1

    return results


def sync_from_ledger() -> Dict[str, Any]:
    """Process stage events from JSONL ledger and sync Tier 2 for each."""
    ledger_path = get_state_dir() / "ledger" / "memory-events.jsonl"

    if not ledger_path.exists():
        return {"success": True, "message": "No ledger file found", "events_processed": 0}

    stage_event_types = {"stage_transition", "stage_complete", "rework_limit"}
    task_ids_to_sync: set = set()

    for event in read_jsonl(ledger_path, skip_errors=True):
        event_type = event.get("event_type") or event.get("e")
        if event_type in stage_event_types:
            data = event.get("data") or event.get("d", {})
            if isinstance(data, str):
                try:
                    data = json.loads(data)
                except json.JSONDecodeError:
                    data = {}
            task_id = data.get("task_id") if isinstance(data, dict) else None
            if task_id:
                task_ids_to_sync.add(task_id)

    results = {
        "success": True,
        "tasks_found": len(task_ids_to_sync),
        "synced": 0,
        "errors": 0,
    }

    for task_id in task_ids_to_sync:
        try:
            updates = sync_tier2_for_task(task_id)
            if any(updates.values()):
                results["synced"] += 1
        except Exception as e:
            logger.error("Sync failed for %s: %s", task_id, e)
            results["errors"] += 1

    return results


# =============================================================================
# CLI
# =============================================================================


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Two-tier stage transition sync for CodeFlow (JSONL + Markdown)"
    )

    # Primary: record a stage transition
    parser.add_argument(
        "--task-id",
        help="Task ID (e.g., FRT-TSK-FEAT-AUTH-001)",
    )
    parser.add_argument(
        "--stage",
        choices=VALID_STAGES,
        help="New stage (dev, work, review, qa, done)",
    )
    parser.add_argument(
        "--stage-status",
        choices=VALID_STAGE_STATUSES,
        help="New stage status (pending, in_progress, complete, failed)",
    )
    parser.add_argument("--agent", help="Agent performing the stage")
    parser.add_argument(
        "--verdict",
        choices=VALID_VERDICTS,
        help="Stage verdict (pass, approved, changes_requested, fail)",
    )
    parser.add_argument("--notes", help="Notes about the transition")
    parser.add_argument(
        "--rework", action="store_true", help="Mark as rework iteration"
    )
    parser.add_argument(
        "--iteration", type=int, default=1, help="Rework iteration number (default: 1)"
    )

    # Sync-only modes
    parser.add_argument(
        "--sync-markdown",
        action="store_true",
        help="Only sync Tier 2 markdown from JSONL ledger data",
    )
    parser.add_argument(
        "--sync-all",
        action="store_true",
        help="Sync Tier 2 markdown for all active work with stage data",
    )
    parser.add_argument(
        "--from-ledger",
        action="store_true",
        help="Process stage events from JSONL ledger and sync Tier 2",
    )

    # Output format
    parser.add_argument(
        "--json", action="store_true", help="Output in JSON format"
    )

    args = parser.parse_args()

    # Determine mode
    try:
        if args.sync_all:
            result = sync_all_active()
        elif args.from_ledger:
            result = sync_from_ledger()
        elif args.sync_markdown and args.task_id:
            updates = sync_tier2_for_task(args.task_id)
            result = {"success": True, "task_id": args.task_id, "updates": updates}
        elif args.task_id and args.stage and args.stage_status:
            # Two-tier stage transition (JSONL + Markdown)
            result = record_stage_transition(
                task_id=args.task_id,
                stage=args.stage,
                stage_status=args.stage_status,
                agent=args.agent,
                verdict=args.verdict,
                notes=args.notes,
                rework=args.rework,
                iteration=args.iteration,
            )
        else:
            parser.error(
                "Provide --task-id with --stage and --stage-status for a transition, "
                "or use --sync-markdown/--sync-all/--from-ledger for sync-only mode"
            )
            return 1

        # Output
        if args.json:
            print(json.dumps(result, indent=2))
        else:
            if not result.get("success"):
                print(f"Error: {result.get('error', 'Unknown error')}", file=sys.stderr)
                return 1

            if args.sync_all:
                print(
                    f"Synced {result.get('synced', 0)}/{result.get('total', 0)} "
                    f"work items ({result.get('errors', 0)} errors)"
                )
            elif args.from_ledger:
                print(
                    f"Processed {result.get('tasks_found', 0)} tasks "
                    f"from ledger ({result.get('synced', 0)} synced, "
                    f"{result.get('errors', 0)} errors)"
                )
            elif args.sync_markdown:
                updates = result.get("updates", {})
                updated = [k for k, v in updates.items() if v]
                print(f"Updated: {', '.join(updated)}" if updated else "No files needed updating")
            else:
                new = result.get("stage", "?")
                print(f"Stage transition: -> {new}/{result.get('stage_status', '?')}")
                tier2 = result.get("tier2", {})
                updated = [k for k, v in tier2.items() if v] if isinstance(tier2, dict) else []
                if updated:
                    print(f"Markdown updated: {', '.join(updated)}")

        return 0

    except ValidationError as e:
        logger.error("Validation error: %s", e)
        if args.json:
            print(json.dumps({"success": False, "error": str(e)}))
        else:
            print(f"Error: {e}", file=sys.stderr)
        return 1

    except Exception as e:
        logger.error("Unexpected error: %s", e)
        if args.json:
            print(json.dumps({"success": False, "error": str(e)}))
        else:
            print(f"Error: {e}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
