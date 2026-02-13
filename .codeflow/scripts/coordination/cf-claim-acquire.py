#!/usr/bin/env python3
"""
cf-claim-acquire.py - Acquire a resource claim via CRDT.

Usage:
    cf-claim-acquire.py --work-id TSK-xxx --pattern "src/models/**" --owner-id agent-xxx
    cf-claim-acquire.py --work-id TSK-xxx --pattern "src/auth/**" --owner-id agent-xxx --mode shared

Exit Codes:
    0: Success
    1: Conflict (resource already claimed)
    2: Error
"""

import argparse
import json
import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path

# Add parent scripts dir to path (NOT codeflow_py_lib directly, to avoid shadowing stdlib)
sys.path.insert(0, str(Path(__file__).parent.parent))

from codeflow_py_lib import (
    append_jsonl,
    generate_ulid,
    get_logger,
    get_state_dir,
    load_coordination,
    save_coordination,
)

logger = get_logger(__name__)

DEFAULT_TTL = 600  # 10 minutes (canonical)


def _format_utc(dt: datetime) -> str:
    """Format a UTC datetime as ISO 8601 with Z suffix."""
    return dt.strftime("%Y-%m-%dT%H:%M:%SZ")


def _parse_expiry(expires_str: str) -> datetime:
    """Parse an expiry timestamp string to a timezone-aware datetime."""
    normalized = expires_str.replace("Z", "+00:00")
    return datetime.fromisoformat(normalized)


def main() -> int:
    parser = argparse.ArgumentParser(description="Acquire a resource claim")
    parser.add_argument(
        "--work-id",
        required=True,
        help="Work ULID PK (e.g., task-{ulid} or epic-{ulid}) this claim belongs to",
    )
    parser.add_argument(
        "--pattern",
        required=True,
        help="Resource pattern (e.g., 'src/models/**', 'src/auth/**')",
    )
    parser.add_argument(
        "--owner-id",
        required=True,
        help="Owner ID (session or agent ID)",
    )
    parser.add_argument(
        "--mode",
        default="exclusive",
        choices=("exclusive", "shared"),
        help="Claim mode (default: exclusive)",
    )
    parser.add_argument(
        "--ttl",
        type=int,
        default=DEFAULT_TTL,
        help=f"Time to live in seconds (default: {DEFAULT_TTL})",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="Output in JSON format",
    )
    args = parser.parse_args()

    try:
        # Load CRDT coordination state
        doc = load_coordination()

        # Check for conflicts
        now = datetime.now(timezone.utc)
        for claim_id, claim in doc.claims.items():
            if claim["pattern"] == args.pattern and claim["status"] == "active":
                expires_at = claim.get("expires_at")
                if expires_at:
                    try:
                        exp_time = _parse_expiry(expires_at)
                        if exp_time > now:
                            # Active conflict: block if either side is exclusive
                            if claim["mode"] == "exclusive" or args.mode == "exclusive":
                                result = {
                                    "success": False,
                                    "error": "conflict",
                                    "conflicting_claim": claim,
                                }
                                if args.json:
                                    print(json.dumps(result))
                                else:
                                    print(
                                        f"Conflict: Resource claimed by {claim['owner_id']}"
                                    )
                                return 1
                    except ValueError:
                        pass

        # Create new claim
        claim_id = f"claim_{generate_ulid()}"
        expires = now + timedelta(seconds=args.ttl)
        fencing_token = doc.get_next_fencing_token()
        expires_str = _format_utc(expires)
        created_str = _format_utc(now)

        claim = {
            "id": claim_id,
            "work_id": args.work_id,
            "pattern": args.pattern,
            "mode": args.mode,
            "owner_id": args.owner_id,
            "fencing_token": fencing_token,
            "expires_at": expires_str,
            "status": "active",
            "created_at": created_str,
        }

        # Write to CRDT (primary coordination state)
        doc.claims[claim_id] = claim
        save_coordination(doc)

        # Write to JSONL (rebuild authority)
        ledger_path = get_state_dir() / "ledger" / "sessions.jsonl"
        append_jsonl(ledger_path, {"type": "claim_created", **claim})

        result = {
            "success": True,
            "claim_id": claim_id,
            "pattern": args.pattern,
            "mode": args.mode,
            "expires_at": expires_str,
            "fencing_token": fencing_token,
        }

        if args.json:
            print(json.dumps(result))
        else:
            print(f"Claim acquired: {claim_id}")
            print(f"Fencing token: {fencing_token}")
            print(f"Expires at: {expires_str}")

        return 0

    except Exception as e:
        logger.error(f"Claim acquisition error: {e}")
        if args.json:
            print(json.dumps({"success": False, "error": str(e)}))
        else:
            print(f"Error: {e}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
