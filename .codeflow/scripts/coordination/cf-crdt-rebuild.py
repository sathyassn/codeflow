#!/usr/bin/env python3
"""
cf-crdt-rebuild.py - Rebuild CRDT state from JSONL ledger.

Usage:
    cf-crdt-rebuild.py
    cf-crdt-rebuild.py --verify
    cf-crdt-rebuild.py --until "2026-01-25T10:00:00Z"
    cf-crdt-rebuild.py --dry-run
    cf-crdt-rebuild.py --source /path/to/sessions.jsonl

Exit Codes:
    0: Success
    1: JSONL file not found
    2: Error
"""

import argparse
import json
import shutil
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

# Add codeflow_py_lib to path
sys.path.insert(0, str(Path(__file__).parent.parent))

from codeflow_py_lib import (
    get_logger,
    get_state_dir,
    load_coordination,
    read_jsonl,
    save_coordination,
)
from codeflow_py_lib.crdt import CRDT_JSON_FALLBACK, CRDT_STATE_FILE, CoordinationDoc

logger = get_logger(__name__)


def _rebuild_from_jsonl_with_stats(
    jsonl_path: Path,
    until: str | None = None,
) -> tuple[CoordinationDoc, int]:
    """Rebuild CRDT state from JSONL, returning doc and event count.

    Args:
        jsonl_path: Path to the sessions.jsonl file.
        until: Optional ISO timestamp cutoff for point-in-time rebuild.

    Returns:
        Tuple of (rebuilt CoordinationDoc, events_replayed count).
    """
    doc = CoordinationDoc()
    events_replayed = 0

    until_dt = None
    if until:
        until_dt = datetime.fromisoformat(until.replace("Z", "+00:00"))

    for event in read_jsonl(jsonl_path):
        event_type = event.get("type") or event.get("e")

        # Check time cutoff
        if until_dt:
            ts = event.get("ts") or event.get("created_at")
            if ts:
                try:
                    event_dt = datetime.fromisoformat(ts.replace("Z", "+00:00"))
                    if event_dt > until_dt:
                        continue
                except ValueError:
                    pass

        if event_type == "claim_created":
            claim_id = event.get("id") or event.get("claim_id")
            if claim_id:
                doc._claims[claim_id] = {
                    "id": claim_id,
                    "work_id": event.get("work_id"),
                    "pattern": event.get("pattern"),
                    "mode": event.get("mode", "exclusive"),
                    "owner_id": event.get("owner_id"),
                    "fencing_token": event.get("fencing_token", 0),
                    "expires_at": event.get("expires_at"),
                    "status": "active",
                    "created_at": event.get("created_at") or event.get("ts"),
                }
                if event.get("fencing_token", 0) > doc._token_counter:
                    doc._token_counter = event["fencing_token"]
                events_replayed += 1

        elif event_type == "claim_released":
            claim_id = event.get("claim_id")
            if claim_id and claim_id in doc._claims:
                doc._claims[claim_id]["status"] = "released"
                events_replayed += 1

        elif event_type == "claim_renewed":
            claim_id = event.get("claim_id")
            if claim_id and claim_id in doc._claims:
                doc._claims[claim_id]["expires_at"] = event.get("expires_at")
                events_replayed += 1

    return doc, events_replayed


def _backup_existing_state(state_dir: Path) -> str | None:
    """Back up existing CRDT state files if they exist.

    Returns:
        Backup path string if backup was created, None otherwise.
    """
    now_str = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")

    for state_file in [CRDT_STATE_FILE, CRDT_JSON_FALLBACK]:
        src = state_dir / state_file
        if src.exists():
            backup = src.with_suffix(f"{src.suffix}.bak.{now_str}")
            shutil.copy2(str(src), str(backup))
            return str(backup)

    return None


def main():
    parser = argparse.ArgumentParser(description="Rebuild CRDT state from JSONL")
    parser.add_argument(
        "--source",
        help="Path to sessions.jsonl file (default: .state/ledger/sessions.jsonl)",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Show what would be rebuilt without saving",
    )
    parser.add_argument(
        "--verify",
        action="store_true",
        help="Verify rebuild against existing state without replacing",
    )
    parser.add_argument(
        "--until",
        help="Point-in-time rebuild cutoff (ISO 8601 timestamp)",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="Output in JSON format",
    )
    args = parser.parse_args()

    try:
        # Determine source file
        if args.source:
            jsonl_path = Path(args.source)
        else:
            jsonl_path = get_state_dir() / "ledger" / "sessions.jsonl"

        if not jsonl_path.exists():
            result = {
                "success": False,
                "error": "source_not_found",
                "path": str(jsonl_path),
            }
            if args.json:
                print(json.dumps(result))
            else:
                print(f"JSONL file not found: {jsonl_path}")
            return 1

        start_time = time.monotonic()

        # Rebuild from JSONL
        doc, events_replayed = _rebuild_from_jsonl_with_stats(
            jsonl_path, until=args.until
        )

        duration_ms = int((time.monotonic() - start_time) * 1000)

        # Count by status
        status_counts = {"active": 0, "released": 0, "expired": 0}
        for claim in doc.claims.values():
            status = claim.get("status", "unknown")
            status_counts[status] = status_counts.get(status, 0) + 1

        stats = {
            "success": True,
            "rebuild_type": "point_in_time" if args.until else "full",
            "source": str(jsonl_path),
            "events_replayed": events_replayed,
            "final_state": {
                "active_claims": status_counts.get("active", 0),
                "released_claims": status_counts.get("released", 0),
                "expired_claims": status_counts.get("expired", 0),
                "max_fencing_token": doc._token_counter,
            },
            "duration_ms": duration_ms,
        }

        if args.until:
            stats["until"] = args.until

        if args.verify:
            # Compare with existing state
            existing = load_coordination()
            mismatches = []
            for cid, claim in doc.claims.items():
                existing_claim = existing.claims.get(cid)
                if not existing_claim:
                    mismatches.append({"claim_id": cid, "issue": "missing_in_existing"})
                elif existing_claim.get("status") != claim.get("status"):
                    mismatches.append({
                        "claim_id": cid,
                        "issue": "status_mismatch",
                        "rebuilt": claim.get("status"),
                        "existing": existing_claim.get("status"),
                    })
            for cid in existing.claims:
                if cid not in doc.claims:
                    mismatches.append(
                        {"claim_id": cid, "issue": "missing_in_rebuild"}
                    )

            stats["verify"] = True
            stats["mismatches"] = mismatches
            stats["verified_ok"] = len(mismatches) == 0

            if args.json:
                print(json.dumps(stats, indent=2))
            else:
                if mismatches:
                    print(f"VERIFY: {len(mismatches)} mismatches found")
                    for m in mismatches:
                        print(f"  {m['claim_id']}: {m['issue']}")
                else:
                    print("VERIFY: State matches rebuild (OK)")
                print(f"  Events replayed: {events_replayed}")
                print(f"  Duration: {duration_ms}ms")

        elif args.dry_run:
            stats["dry_run"] = True
            if args.json:
                print(json.dumps(stats, indent=2))
            else:
                print("DRY RUN - would rebuild:")
                print(f"  Events replayed: {events_replayed}")
                print(f"  Active claims: {status_counts.get('active', 0)}")
                print(f"  Released claims: {status_counts.get('released', 0)}")
                print(f"  Max fencing token: {doc._token_counter}")
                print(f"  Duration: {duration_ms}ms")
        else:
            # Backup existing state
            state_dir = get_state_dir()
            backup_path = _backup_existing_state(state_dir)
            if backup_path:
                stats["backup_created"] = backup_path

            # Save the rebuilt state
            save_coordination(doc)
            stats["saved"] = True

            if args.json:
                print(json.dumps(stats, indent=2))
            else:
                print("CRDT state rebuilt successfully")
                print(f"  Events replayed: {events_replayed}")
                print(f"  Active claims: {status_counts.get('active', 0)}")
                print(f"  Released claims: {status_counts.get('released', 0)}")
                print(f"  Max fencing token: {doc._token_counter}")
                print(f"  Duration: {duration_ms}ms")
                if backup_path:
                    print(f"  Backup: {backup_path}")

        return 0

    except Exception as e:
        logger.error(f"Rebuild error: {e}")
        if args.json:
            print(json.dumps({"success": False, "error": str(e)}))
        else:
            print(f"Error: {e}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
