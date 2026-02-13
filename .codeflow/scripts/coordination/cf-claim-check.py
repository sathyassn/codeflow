#!/usr/bin/env python3
"""
cf-claim-check.py - Check if a resource path or pattern is claimed.

Usage:
    cf-claim-check.py --file-path "src/models/user.ts"
    cf-claim-check.py --pattern "src/**"
    cf-claim-check.py --file-path "src/auth/jwt.ts" --owner-id agent-001

Exit Codes:
    0: Success (check result in output JSON)
    2: Error
"""

import argparse
import fnmatch
import json
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


def patterns_conflict(check_value: str, claim_pattern: str) -> bool:
    """Check if a file path or pattern conflicts with a claim pattern.

    Handles: exact match, glob matching, directory containment,
    and cross-type file-in-directory checks.
    """
    if check_value == claim_pattern:
        return True

    def parse_pattern(p):
        if ":" in p:
            ptype, path = p.split(":", 1)
            return ptype, path
        return "file", p

    check_type, check_path = parse_pattern(check_value)
    claim_type, claim_path = parse_pattern(claim_pattern)

    # Glob matching (either direction)
    if fnmatch.fnmatch(check_path, claim_path) or fnmatch.fnmatch(
        claim_path, check_path
    ):
        return True

    # Directory containment: file inside dir claim
    if check_type == "file" and claim_type == "dir":
        if check_path.startswith(claim_path.rstrip("/") + "/"):
            return True

    # Directory containment: dir claim contains file
    if check_type == "dir" and claim_type == "file":
        if claim_path.startswith(check_path.rstrip("/") + "/"):
            return True

    # Same-type directory containment
    if check_type == "dir" and claim_type == "dir":
        cp = check_path.rstrip("/")
        clp = claim_path.rstrip("/")
        if cp.startswith(clp + "/") or clp.startswith(cp + "/"):
            return True

    return False


def _classify_conflict(claim: dict, owner_id: str | None, is_expired: bool) -> str:
    """Classify conflict per V4 conflict matrix.

    Returns: "BLOCK", "WARN", or "ALLOW".
    """
    if owner_id and claim.get("owner_id") == owner_id:
        return "ALLOW"
    if is_expired:
        return "WARN"
    if claim.get("mode") == "exclusive":
        return "BLOCK"
    return "WARN"


def main():
    parser = argparse.ArgumentParser(
        description="Check if a path or pattern is claimed"
    )
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument(
        "--file-path",
        help="File path to check against claims",
    )
    group.add_argument(
        "--pattern",
        help="Resource pattern to check",
    )
    parser.add_argument(
        "--owner-id",
        help="Current owner ID — session or agent identifier (to determine own vs other claims)",
    )
    parser.add_argument(
        "--include-expired",
        action="store_true",
        help="Include expired claims in check",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        default=True,
        help="Output in JSON format (default)",
    )
    args = parser.parse_args()

    try:
        doc = load_coordination()
        now = datetime.now(timezone.utc)

        check_value = args.file_path if args.file_path else args.pattern

        matching_claims = []
        can_proceed = True

        for claim_id, claim in doc.claims.items():
            if claim["status"] != "active":
                continue

            # Check expiration
            is_expired = False
            expires_at = claim.get("expires_at")
            if expires_at:
                try:
                    exp_time = datetime.fromisoformat(
                        expires_at.replace("Z", "+00:00")
                    )
                    if exp_time <= now:
                        is_expired = True
                        if not args.include_expired:
                            continue
                except ValueError:
                    pass

            if not patterns_conflict(check_value, claim["pattern"]):
                continue

            is_own = bool(
                args.owner_id and claim.get("owner_id") == args.owner_id
            )
            action = _classify_conflict(claim, args.owner_id, is_expired)

            if action == "BLOCK":
                can_proceed = False

            matching_claims.append(
                {
                    "claim_id": claim_id,
                    "pattern": claim.get("pattern"),
                    "mode": claim.get("mode"),
                    "owner_id": claim.get("owner_id"),
                    "is_own": is_own,
                    "expires_at": claim.get("expires_at"),
                    "action": action,
                }
            )

        result = {
            "claimed": len(matching_claims) > 0,
            "claims": matching_claims,
            "can_proceed": can_proceed,
        }

        print(json.dumps(result, indent=2))
        return 0

    except Exception as e:
        logger.error(f"Check error: {e}")
        print(json.dumps({"success": False, "error": str(e)}))
        return 2


if __name__ == "__main__":
    sys.exit(main())
