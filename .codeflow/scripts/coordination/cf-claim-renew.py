#!/usr/bin/env python3
"""
cf-claim-renew.py - Renew a resource claim's TTL.

Usage:
    cf-claim-renew.py --claim-id claim_01JXYZ...
    cf-claim-renew.py --claim-id claim_01JXYZ... --ttl 1200
    cf-claim-renew.py --all --ttl 600

Notes:
    claim-id is the claim's own ULID key (claim_{ulid}), not the work ULID PK.

Exit Codes:
    0: Success
    1: Claim not found or expired
    2: Error
"""

import argparse
import json
import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path

# Add codeflow_py_lib to path
sys.path.insert(0, str(Path(__file__).parent.parent))

from codeflow_py_lib import (
    append_jsonl,
    get_logger,
    get_state_dir,
    load_coordination,
    save_coordination,
)

logger = get_logger(__name__)

DEFAULT_TTL = 600  # 10 minutes


def _format_ts(dt: datetime) -> str:
    """Format datetime as ISO 8601 with Z suffix."""
    return dt.isoformat().replace("+00:00", "Z")


def _renew_single(doc, claim_id: str, ttl: int, now: datetime):
    """Renew a single claim. Returns (result_dict, exit_code)."""
    if claim_id not in doc.claims:
        return {"success": False, "error": "claim_not_found"}, 1

    claim = doc.claims[claim_id]

    if claim["status"] != "active":
        return {
            "success": False,
            "error": "claim_not_active",
            "status": claim["status"],
        }, 1

    # Check if claim has expired
    expires_at = claim.get("expires_at")
    if expires_at:
        try:
            exp_time = datetime.fromisoformat(expires_at.replace("Z", "+00:00"))
            if exp_time <= now:
                return {
                    "success": False,
                    "error": "claim_expired",
                    "message": f"Claim expired at {expires_at}, cannot renew",
                    "claim_id": claim_id,
                    "expired_at": expires_at,
                    "suggestion": "Create a new claim with cf-claim-acquire.py",
                }, 1
        except ValueError:
            pass

    previous_expires = claim.get("expires_at")
    new_expires = now + timedelta(seconds=ttl)
    new_expires_str = _format_ts(new_expires)
    claim["expires_at"] = new_expires_str
    now_str = _format_ts(now)

    # Write to JSONL (rebuild authority)
    ledger_path = get_state_dir() / "ledger" / "sessions.jsonl"
    append_jsonl(
        ledger_path,
        {
            "type": "claim_renewed",
            "claim_id": claim_id,
            "expires_at": new_expires_str,
            "renewed_at": now_str,
            "ttl": ttl,
        },
    )

    return {
        "success": True,
        "claim_id": claim_id,
        "pattern": claim.get("pattern"),
        "previous_expires_at": previous_expires,
        "new_expires_at": new_expires_str,
        "extended_by_seconds": ttl,
    }, 0


def main():
    parser = argparse.ArgumentParser(description="Renew a resource claim's TTL")
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument(
        "--claim-id",
        help="Claim ID to renew",
    )
    group.add_argument(
        "--all",
        action="store_true",
        dest="renew_all",
        help="Renew all own active claims",
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
        doc = load_coordination()
        now = datetime.now(timezone.utc)

        if args.renew_all:
            # Renew all active, non-expired claims
            renewed = []
            for cid, claim in doc.claims.items():
                if claim["status"] != "active":
                    continue
                expires_at = claim.get("expires_at")
                if expires_at:
                    try:
                        exp_time = datetime.fromisoformat(
                            expires_at.replace("Z", "+00:00")
                        )
                        if exp_time <= now:
                            continue
                    except ValueError:
                        pass
                result, _ = _renew_single(doc, cid, args.ttl, now)
                if result.get("success"):
                    renewed.append(
                        {
                            "claim_id": cid,
                            "new_expires_at": result["new_expires_at"],
                        }
                    )

            save_coordination(doc)

            batch_result = {
                "success": True,
                "renewed_count": len(renewed),
                "claims": renewed,
            }

            if args.json:
                print(json.dumps(batch_result))
            else:
                print(f"Renewed {len(renewed)} claim(s)")
                for r in renewed:
                    print(f"  {r['claim_id']} -> {r['new_expires_at']}")
            return 0

        # Single claim renewal
        result, exit_code = _renew_single(doc, args.claim_id, args.ttl, now)

        if result.get("success"):
            save_coordination(doc)

        if args.json:
            print(json.dumps(result))
        else:
            if result.get("success"):
                print(f"Claim renewed: {args.claim_id}")
                print(f"New expiration: {result['new_expires_at']}")
            else:
                error = result.get("error", "unknown")
                msg = result.get("message", error)
                print(msg)

        return exit_code

    except Exception as e:
        logger.error(f"Claim renewal error: {e}")
        if args.json:
            print(json.dumps({"success": False, "error": str(e)}))
        else:
            print(f"Error: {e}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
