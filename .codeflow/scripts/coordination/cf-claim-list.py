#!/usr/bin/env python3
"""
cf-claim-list.py - List all resource claims.

Usage:
    cf-claim-list.py
    cf-claim-list.py --mine
    cf-claim-list.py --pattern "src/**"
    cf-claim-list.py --status active
    cf-claim-list.py --include-expired
    cf-claim-list.py --format table

Exit Codes:
    0: Success
    2: Error
"""

import argparse
import json
import os
import sys
from datetime import datetime, timezone
from pathlib import Path

# Add codeflow_py_lib to path
sys.path.insert(0, str(Path(__file__).parent.parent))

from codeflow_py_lib import (
    get_logger,
    load_coordination,
)

logger = get_logger(__name__)


def _get_current_owner_id() -> str:
    """Get current owner ID from environment or hostname."""
    return os.environ.get("CODEFLOW_OWNER_ID", os.environ.get("USER", "unknown"))


def _parse_expiry(expires_at: str) -> datetime:
    """Parse expiration timestamp, handling Z suffix."""
    return datetime.fromisoformat(expires_at.replace("Z", "+00:00"))


def main():
    parser = argparse.ArgumentParser(description="List resource claims")
    parser.add_argument(
        "--owner-id",
        help="Filter by owner ID",
    )
    parser.add_argument(
        "--mine",
        action="store_true",
        help="Show only own claims",
    )
    parser.add_argument(
        "--work-id",
        help="Filter by work ULID PK (e.g., task-{ulid} or epic-{ulid})",
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
        "--include-expired",
        action="store_true",
        help="Include expired claims in results",
    )
    parser.add_argument(
        "--format",
        choices=("json", "table"),
        default="json",
        help="Output format (default: json)",
    )
    args = parser.parse_args()

    # Resolve --mine to owner-id
    owner_filter = args.owner_id
    if args.mine:
        owner_filter = _get_current_owner_id()

    # If --include-expired, switch status filter to "all"
    if args.include_expired and args.status == "active":
        status_filter = "all"
    else:
        status_filter = args.status

    try:
        doc = load_coordination()
        now = datetime.now(timezone.utc)
        current_owner = _get_current_owner_id()

        claims = []
        status_counts = {"active": 0, "released": 0, "expired": 0}
        own_count = 0
        mode_counts = {"exclusive": 0, "shared": 0}

        for claim_id, claim in doc.claims.items():
            # Determine effective status (check expiration)
            effective_status = claim["status"]
            if effective_status == "active":
                expires_at = claim.get("expires_at")
                if expires_at:
                    try:
                        exp_time = _parse_expiry(expires_at)
                        if exp_time <= now:
                            effective_status = "expired"
                    except ValueError:
                        pass

            is_own = claim.get("owner_id") == current_owner

            # Compute time remaining
            time_remaining = None
            if effective_status == "active" and claim.get("expires_at"):
                try:
                    exp_time = _parse_expiry(claim["expires_at"])
                    time_remaining = int((exp_time - now).total_seconds())
                except ValueError:
                    pass

            # Apply filters
            if status_filter != "all" and effective_status != status_filter:
                continue
            if owner_filter and claim.get("owner_id") != owner_filter:
                continue
            if args.work_id and claim.get("work_id") != args.work_id:
                continue
            if args.pattern and args.pattern not in claim.get("pattern", ""):
                continue

            claim_output = {
                "claim_id": claim.get("id", claim_id),
                "pattern": claim.get("pattern"),
                "mode": claim.get("mode", "exclusive"),
                "status": effective_status,
                "owner_id": claim.get("owner_id"),
                "is_own": is_own,
                "fencing_token": claim.get("fencing_token"),
                "created_at": claim.get("created_at"),
                "expires_at": claim.get("expires_at"),
            }
            if time_remaining is not None:
                claim_output["time_remaining_seconds"] = time_remaining

            claims.append(claim_output)

            # Track summary counts
            status_counts[effective_status] = (
                status_counts.get(effective_status, 0) + 1
            )
            if is_own:
                own_count += 1
            mode = claim.get("mode", "exclusive")
            mode_counts[mode] = mode_counts.get(mode, 0) + 1

        # Sort by created_at descending
        claims.sort(key=lambda x: x.get("created_at") or "", reverse=True)

        if args.format == "json":
            output = {
                "claims": claims,
                "summary": {
                    "total": len(claims),
                    **status_counts,
                    "own": own_count,
                    **mode_counts,
                },
            }
            print(json.dumps(output, indent=2))
        else:
            # Table format
            if not claims:
                print("No claims found.")
            else:
                print(f"{'ID':<35} {'Status':<10} {'Owner':<20} {'Pattern':<30}")
                print("-" * 95)
                for c in claims:
                    pattern = (c.get("pattern") or "")[:30]
                    owner = (c.get("owner_id") or "")[:20]
                    print(
                        f"{c['claim_id']:<35} {c['status']:<10} "
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
