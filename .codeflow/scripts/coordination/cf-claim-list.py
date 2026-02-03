#!/usr/bin/env python3
"""
cf-claim-list.py - List all resource claims.

Usage:
    cf-claim-list.py
    cf-claim-list.py --owner-id agent-xxx
    cf-claim-list.py --status active --format table

Exit Codes:
    0: Success
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
    get_logger,
    load_coordination,
)

logger = get_logger(__name__)


def main():
    parser = argparse.ArgumentParser(description="List resource claims")
    parser.add_argument(
        "--owner-id",
        help="Filter by owner ID",
    )
    parser.add_argument(
        "--work-id",
        help="Filter by work ID",
    )
    parser.add_argument(
        "--status",
        choices=("active", "released", "expired", "all"),
        default="active",
        help="Filter by status (default: active)",
    )
    parser.add_argument(
        "--pattern",
        help="Filter by pattern (partial match)",
    )
    parser.add_argument(
        "--format",
        choices=("json", "table"),
        default="json",
        help="Output format (default: json)",
    )
    args = parser.parse_args()

    try:
        doc = load_coordination()
        now = datetime.utcnow()

        claims = []
        for claim_id, claim in doc.claims.items():
            # Determine effective status (check expiration)
            effective_status = claim["status"]
            if effective_status == "active":
                expires_at = claim.get("expires_at")
                if expires_at:
                    try:
                        exp_time = datetime.fromisoformat(expires_at.replace("Z", ""))
                        if exp_time <= now:
                            effective_status = "expired"
                    except ValueError:
                        pass

            claim_with_status = {**claim, "effective_status": effective_status}

            # Apply filters
            if args.status != "all" and effective_status != args.status:
                continue
            if args.owner_id and claim["owner_id"] != args.owner_id:
                continue
            if args.work_id and claim["work_id"] != args.work_id:
                continue
            if args.pattern and args.pattern not in claim["pattern"]:
                continue

            claims.append(claim_with_status)

        # Sort by created_at descending
        claims.sort(key=lambda x: x.get("created_at", ""), reverse=True)

        if args.format == "json":
            print(json.dumps(claims, indent=2))
        else:
            # Table format
            if not claims:
                print("No claims found.")
            else:
                print(f"{'ID':<35} {'Status':<10} {'Owner':<20} {'Pattern':<30}")
                print("-" * 95)
                for claim in claims:
                    pattern = (claim["pattern"] or "")[:30]
                    owner = (claim["owner_id"] or "")[:20]
                    print(
                        f"{claim['id']:<35} {claim['effective_status']:<10} "
                        f"{owner:<20} {pattern:<30}"
                    )
                print(f"\nTotal: {len(claims)} claims")

        return 0

    except Exception as e:
        logger.error(f"List error: {e}")
        print(json.dumps({"success": False, "error": str(e)}))
        return 2


if __name__ == "__main__":
    sys.exit(main())
