#!/usr/bin/env python3
"""
cf-crdt-sync.py - Synchronize CRDT state with SQLite.

Usage:
    cf-crdt-sync.py
    cf-crdt-sync.py --direction crdt-to-db
    cf-crdt-sync.py --direction db-to-crdt

Exit Codes:
    0: Success
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
    get_db,
    get_logger,
    load_coordination,
    save_coordination,
)

logger = get_logger(__name__)


def sync_crdt_to_db(doc, db) -> dict:
    """Sync CRDT state to SQLite."""
    stats = {"inserted": 0, "updated": 0, "errors": 0}

    for claim_id, claim in doc.claims.items():
        try:
            # Check if claim exists in DB
            existing = db.execute_query(
                "SELECT id FROM work_claims WHERE id = :id",
                {"id": claim_id},
            )

            if existing:
                # Update existing
                db.execute_write(
                    """
                    UPDATE work_claims
                    SET status = :status, expires_at = :expires_at
                    WHERE id = :id
                    """,
                    {
                        "id": claim_id,
                        "status": claim["status"],
                        "expires_at": claim.get("expires_at"),
                    },
                )
                stats["updated"] += 1
            else:
                # Insert new
                db.execute_write(
                    """
                    INSERT INTO work_claims
                    (id, work_id, pattern, mode, owner_id, fencing_token, expires_at, status, created_at)
                    VALUES (:id, :work_id, :pattern, :mode, :owner_id, :fencing_token, :expires_at, :status, :created_at)
                    """,
                    {
                        "id": claim_id,
                        "work_id": claim.get("work_id"),
                        "pattern": claim.get("pattern"),
                        "mode": claim.get("mode", "exclusive"),
                        "owner_id": claim.get("owner_id"),
                        "fencing_token": claim.get("fencing_token", 0),
                        "expires_at": claim.get("expires_at"),
                        "status": claim.get("status", "active"),
                        "created_at": claim.get("created_at"),
                    },
                )
                stats["inserted"] += 1

        except Exception as e:
            logger.error(f"Error syncing claim {claim_id}: {e}")
            stats["errors"] += 1

    return stats


def sync_db_to_crdt(doc, db) -> dict:
    """Sync SQLite state to CRDT."""
    stats = {"added": 0, "updated": 0, "errors": 0}

    claims = db.execute_query("SELECT * FROM work_claims ORDER BY created_at")

    for claim in claims:
        try:
            claim_id = claim["id"]
            crdt_claim = {
                "id": claim_id,
                "work_id": claim.get("work_id"),
                "pattern": claim.get("pattern"),
                "mode": claim.get("mode", "exclusive"),
                "owner_id": claim.get("owner_id"),
                "fencing_token": claim.get("fencing_token", 0),
                "expires_at": claim.get("expires_at"),
                "status": claim.get("status", "active"),
                "created_at": claim.get("created_at"),
            }

            if claim_id in doc.claims:
                # Update only if DB has newer status
                if claim.get("status") != doc.claims[claim_id].get("status"):
                    doc.claims[claim_id].update(crdt_claim)
                    stats["updated"] += 1
            else:
                doc.claims[claim_id] = crdt_claim
                stats["added"] += 1

                # Update token counter
                token = claim.get("fencing_token", 0)
                if token > doc._token_counter:
                    doc._token_counter = token

        except Exception as e:
            logger.error(f"Error syncing claim from DB: {e}")
            stats["errors"] += 1

    return stats


def main():
    parser = argparse.ArgumentParser(description="Synchronize CRDT state with SQLite")
    parser.add_argument(
        "--direction",
        choices=("crdt-to-db", "db-to-crdt", "bidirectional"),
        default="crdt-to-db",
        help="Sync direction (default: crdt-to-db)",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="Output in JSON format",
    )
    args = parser.parse_args()

    try:
        doc = load_coordination()
        db = get_db()

        stats = {"direction": args.direction}

        if args.direction == "crdt-to-db":
            sync_stats = sync_crdt_to_db(doc, db)
            stats["crdt_to_db"] = sync_stats

        elif args.direction == "db-to-crdt":
            sync_stats = sync_db_to_crdt(doc, db)
            save_coordination(doc)
            stats["db_to_crdt"] = sync_stats

        else:  # bidirectional
            # First DB to CRDT, then CRDT to DB
            db_stats = sync_db_to_crdt(doc, db)
            save_coordination(doc)
            crdt_stats = sync_crdt_to_db(doc, db)
            stats["db_to_crdt"] = db_stats
            stats["crdt_to_db"] = crdt_stats

        stats["success"] = True
        stats["timestamp"] = datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")

        if args.json:
            print(json.dumps(stats, indent=2))
        else:
            print(f"Sync completed ({args.direction})")
            for key, value in stats.items():
                if isinstance(value, dict):
                    print(f"  {key}:")
                    for k, v in value.items():
                        print(f"    {k}: {v}")

        return 0

    except Exception as e:
        logger.error(f"Sync error: {e}")
        if args.json:
            print(json.dumps({"success": False, "error": str(e)}))
        else:
            print(f"Error: {e}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
