#!/usr/bin/env python3
"""
cf-claim-release.py - Release a resource claim.

Usage:
    cf-claim-release.py --claim-id claim_01JXYZ... --reason "Work completed"
    cf-claim-release.py --pattern "src/models/**" --reason "Work completed"

Notes:
    claim-id is the claim's own ULID key (claim_{ulid}), not the work ULID PK.

Exit Codes:
    0: Success
    1: Claim not found
    2: Error
"""

import argparse
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

# Add parent scripts dir to path (NOT codeflow_py_lib directly, to avoid shadowing stdlib)
sys.path.insert(0, str(Path(__file__).parent.parent))

from codeflow_py_lib import (
    append_jsonl,
    get_logger,
    get_state_dir,
    load_coordination,
    save_coordination,
)

logger = get_logger(__name__)


def _format_utc(dt: datetime) -> str:
    """Format a UTC datetime as ISO 8601 with Z suffix."""
    return dt.strftime("%Y-%m-%dT%H:%M:%SZ")


def _parse_timestamp(ts_str: str) -> datetime:
    """Parse an ISO timestamp string to a timezone-aware datetime."""
    normalized = ts_str.replace("Z", "+00:00")
    return datetime.fromisoformat(normalized)


def _find_claim(doc, claim_id=None, pattern=None):
    """Find a claim by ID or by pattern (first active match).

    Returns (claim_id, claim_dict) or (None, None).
    """
    if claim_id:
        if claim_id in doc.claims:
            return claim_id, doc.claims[claim_id]
        return None, None

    if pattern:
        for cid, claim in doc.claims.items():
            if claim["pattern"] == pattern and claim["status"] == "active":
                return cid, claim
    return None, None


def main() -> int:
    parser = argparse.ArgumentParser(description="Release a resource claim")
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument(
        "--claim-id",
        help="Claim ID to release",
    )
    group.add_argument(
        "--pattern",
        help="Release active claim matching this pattern",
    )
    parser.add_argument(
        "--reason",
        default="",
        help="Reason for releasing the claim",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="Output in JSON format",
    )
    args = parser.parse_args()

    try:
        doc = load_coordination()

        # Find the claim by ID or pattern
        found_id, claim = _find_claim(doc, claim_id=args.claim_id, pattern=args.pattern)

        if found_id is None:
            lookup = args.claim_id or args.pattern
            result = {
                "success": False,
                "error": "claim_not_found",
                "message": f"No matching claim found for: {lookup}",
            }
            if args.json:
                print(json.dumps(result))
            else:
                print(f"Claim not found: {lookup}")
            return 1

        # Check if already released
        if claim["status"] == "released":
            released_at = claim.get("released_at", "unknown")
            result = {
                "success": False,
                "error": "already_released",
                "message": f"Claim was already released at {released_at}",
                "claim_id": found_id,
                "released_at": released_at,
            }
            if args.json:
                print(json.dumps(result))
            else:
                print(f"Already released: {found_id} at {released_at}")
            return 1

        # Compute held duration
        now = datetime.now(timezone.utc)
        released_str = _format_utc(now)
        held_seconds = 0
        created_at = claim.get("created_at")
        if created_at:
            try:
                created_dt = _parse_timestamp(created_at)
                held_seconds = int((now - created_dt).total_seconds())
            except ValueError:
                pass

        # Release the claim in CRDT
        claim["status"] = "released"
        claim["released_at"] = released_str
        save_coordination(doc)

        # Write to JSONL (rebuild authority)
        ledger_path = get_state_dir() / "ledger" / "sessions.jsonl"
        ledger_event = {
            "type": "claim_released",
            "claim_id": found_id,
            "released_at": released_str,
        }
        if args.reason:
            ledger_event["reason"] = args.reason
        append_jsonl(ledger_path, ledger_event)

        result = {
            "success": True,
            "claim_id": found_id,
            "pattern": claim["pattern"],
            "released_at": released_str,
            "reason": args.reason,
            "held_duration_seconds": held_seconds,
        }

        if args.json:
            print(json.dumps(result))
        else:
            print(f"Claim released: {found_id}")

        return 0

    except Exception as e:
        logger.error(f"Claim release error: {e}")
        if args.json:
            print(json.dumps({"success": False, "error": str(e)}))
        else:
            print(f"Error: {e}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
