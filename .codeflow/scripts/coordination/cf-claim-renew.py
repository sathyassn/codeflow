#!/usr/bin/env python3
"""
cf-claim-renew.py - Renew a resource claim's TTL.

Usage:
    cf-claim-renew.py --claim-id claim-xxx
    cf-claim-renew.py --claim-id claim-xxx --ttl 1200

Exit Codes:
    0: Success
    1: Claim not found or expired
    2: Error
"""

import argparse
import json
import sys
from datetime import datetime, timedelta
from pathlib import Path

# Add codeflow_py_lib to path
sys.path.insert(0, str(Path(__file__).parent.parent / "codeflow_py_lib"))

from codeflow_py_lib import (
    append_jsonl,
    get_db,
    get_logger,
    get_state_dir,
    load_coordination,
    save_coordination,
)

logger = get_logger(__name__)

DEFAULT_TTL = 600  # 10 minutes


def main():
    parser = argparse.ArgumentParser(description="Renew a resource claim's TTL")
    parser.add_argument(
        "--claim-id",
        required=True,
        help="Claim ID to renew",
    )
    parser.add_argument(
        "--ttl",
        type=int,
        default=DEFAULT_TTL,
        help=f"New time to live in seconds (default: {DEFAULT_TTL})",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="Output in JSON format",
    )
    args = parser.parse_args()

    try:
        # Load CRDT state
        doc = load_coordination()

        if args.claim_id not in doc.claims:
            result = {"success": False, "error": "claim_not_found"}
            if args.json:
                print(json.dumps(result))
            else:
                print(f"Claim not found: {args.claim_id}")
            return 1

        claim = doc.claims[args.claim_id]

        # Check if claim is still active
        if claim["status"] != "active":
            result = {
                "success": False,
                "error": "claim_not_active",
                "status": claim["status"],
            }
            if args.json:
                print(json.dumps(result))
            else:
                print(f"Claim not active: {claim['status']}")
            return 1

        # Renew the claim
        now = datetime.utcnow()
        new_expires = now + timedelta(seconds=args.ttl)
        claim["expires_at"] = new_expires.isoformat() + "Z"
        save_coordination(doc)

        # Write to JSONL (rebuild authority)
        ledger_path = get_state_dir() / "ledger" / "sessions.jsonl"
        append_jsonl(
            ledger_path,
            {
                "type": "claim_renewed",
                "claim_id": args.claim_id,
                "expires_at": new_expires.isoformat() + "Z",
                "renewed_at": now.isoformat() + "Z",
            },
        )

        # Update SQLite (query cache)
        db = get_db()
        db.execute_write(
            "UPDATE work_claims SET expires_at = :expires_at WHERE id = :id",
            {"id": args.claim_id, "expires_at": new_expires.isoformat() + "Z"},
        )

        result = {
            "success": True,
            "claim_id": args.claim_id,
            "new_expires_at": new_expires.isoformat() + "Z",
        }

        if args.json:
            print(json.dumps(result))
        else:
            print(f"Claim renewed: {args.claim_id}")
            print(f"New expiration: {new_expires.isoformat()}Z")

        return 0

    except Exception as e:
        logger.error(f"Claim renewal error: {e}")
        if args.json:
            print(json.dumps({"success": False, "error": str(e)}))
        else:
            print(f"Error: {e}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
