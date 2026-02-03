#!/usr/bin/env python3
"""
cf-claim-release.py - Release a resource claim.

Usage:
    cf-claim-release.py --claim-id claim-xxx

Exit Codes:
    0: Success
    1: Claim not found
    2: Error
"""

import argparse
import json
import sys
from datetime import datetime
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


def main():
    parser = argparse.ArgumentParser(description="Release a resource claim")
    parser.add_argument(
        "--claim-id",
        required=True,
        help="Claim ID to release",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="Output in JSON format",
    )
    args = parser.parse_args()

    try:
        # Update CRDT (primary coordination state)
        doc = load_coordination()

        if args.claim_id not in doc.claims:
            result = {"success": False, "error": "claim_not_found"}
            if args.json:
                print(json.dumps(result))
            else:
                print(f"Claim not found: {args.claim_id}")
            return 1

        # Release the claim
        doc.claims[args.claim_id]["status"] = "released"
        save_coordination(doc)

        # Write to JSONL (rebuild authority)
        now = datetime.utcnow()
        ledger_path = get_state_dir() / "ledger" / "sessions.jsonl"
        append_jsonl(
            ledger_path,
            {
                "type": "claim_released",
                "claim_id": args.claim_id,
                "released_at": now.isoformat() + "Z",
            },
        )

        # Update SQLite (query cache)
        db = get_db()
        db.execute_write(
            "UPDATE work_claims SET status = 'released' WHERE id = :id",
            {"id": args.claim_id},
        )

        result = {"success": True, "claim_id": args.claim_id}

        if args.json:
            print(json.dumps(result))
        else:
            print(f"Claim released: {args.claim_id}")

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
