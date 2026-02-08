#!/usr/bin/env python3
"""
cf-crdt-rebuild.py - Rebuild CRDT state from JSONL ledger.

Usage:
    cf-crdt-rebuild.py
    cf-crdt-rebuild.py --dry-run
    cf-crdt-rebuild.py --source /path/to/sessions.jsonl

Exit Codes:
    0: Success
    1: JSONL file not found
    2: Error
"""

import argparse
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

# Add codeflow_py_lib to path
sys.path.insert(0, str(Path(__file__).parent.parent / "codeflow_py_lib"))

from codeflow_py_lib import (
    get_logger,
    get_state_dir,
    rebuild_from_jsonl,
    save_coordination,
)

logger = get_logger(__name__)


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

        # Rebuild from JSONL
        doc = rebuild_from_jsonl(jsonl_path)

        stats = {
            "success": True,
            "source": str(jsonl_path),
            "claims_rebuilt": len(doc.claims),
            "token_counter": doc._token_counter,
            "timestamp": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        }

        # Count by status
        status_counts = {}
        for claim in doc.claims.values():
            status = claim.get("status", "unknown")
            status_counts[status] = status_counts.get(status, 0) + 1
        stats["by_status"] = status_counts

        if args.dry_run:
            stats["dry_run"] = True
            if args.json:
                print(json.dumps(stats, indent=2))
            else:
                print("DRY RUN - would rebuild:")
                print(f"  Claims: {stats['claims_rebuilt']}")
                print(f"  Token counter: {stats['token_counter']}")
                print(f"  By status: {status_counts}")
        else:
            # Save the rebuilt state
            save_coordination(doc)
            stats["saved"] = True

            if args.json:
                print(json.dumps(stats, indent=2))
            else:
                print("CRDT state rebuilt successfully")
                print(f"  Claims: {stats['claims_rebuilt']}")
                print(f"  Token counter: {stats['token_counter']}")
                print(f"  By status: {status_counts}")

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
