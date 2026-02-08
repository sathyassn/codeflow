#!/usr/bin/env python3
"""
cf-claim-check.py - Check if a resource pattern is claimed.

Usage:
    cf-claim-check.py --pattern "file:src/*.ts"
    cf-claim-check.py --pattern "dir:src/auth" --include-expired

Exit Codes:
    0: Success (claimed=true or claimed=false in output)
    2: Error
"""

import argparse
import fnmatch
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

# Add codeflow_py_lib to path
sys.path.insert(0, str(Path(__file__).parent.parent / "codeflow_py_lib"))

from codeflow_py_lib import (
    get_logger,
    load_coordination,
)

logger = get_logger(__name__)


def patterns_conflict(pattern1: str, pattern2: str) -> bool:
    """Check if two patterns conflict (overlap)."""
    # Exact match
    if pattern1 == pattern2:
        return True

    # Extract type and path
    def parse_pattern(p):
        if ":" in p:
            ptype, path = p.split(":", 1)
            return ptype, path
        return "file", p

    type1, path1 = parse_pattern(pattern1)
    type2, path2 = parse_pattern(pattern2)

    # Different types don't conflict
    if type1 != type2:
        return False

    # Check glob matching
    if fnmatch.fnmatch(path1, path2) or fnmatch.fnmatch(path2, path1):
        return True

    # Check directory containment
    if type1 == "dir":
        if path1.startswith(path2 + "/") or path2.startswith(path1 + "/"):
            return True

    return False


def main():
    parser = argparse.ArgumentParser(description="Check if a pattern is claimed")
    parser.add_argument(
        "--pattern",
        required=True,
        help="Resource pattern to check",
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

        # Find conflicting claims
        conflicting_claims = []
        for claim_id, claim in doc.claims.items():
            # Skip non-active claims
            if claim["status"] != "active":
                continue

            # Check expiration unless --include-expired
            if not args.include_expired:
                expires_at = claim.get("expires_at")
                if expires_at:
                    try:
                        exp_time = datetime.fromisoformat(expires_at.replace("Z", ""))
                        if exp_time <= now:
                            continue
                    except ValueError:
                        pass

            # Check if patterns conflict
            if patterns_conflict(args.pattern, claim["pattern"]):
                conflicting_claims.append(claim)

        if conflicting_claims:
            result = {
                "claimed": True,
                "pattern": args.pattern,
                "conflicts": conflicting_claims,
            }
        else:
            result = {
                "claimed": False,
                "pattern": args.pattern,
            }

        print(json.dumps(result, indent=2))
        return 0

    except Exception as e:
        logger.error(f"Check error: {e}")
        print(json.dumps({"success": False, "error": str(e)}))
        return 2


if __name__ == "__main__":
    sys.exit(main())
