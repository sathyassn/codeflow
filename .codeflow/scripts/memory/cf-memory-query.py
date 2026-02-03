#!/usr/bin/env python3
"""
cf-memory-query.py - Query memory events.

Usage:
    cf-memory-query.py --domain development
    cf-memory-query.py --event-type decision --limit 50
    cf-memory-query.py --work-id TSK-xxx

Exit Codes:
    0: Success
    1: Invalid arguments
    2: Database error
"""

import argparse
import json
import sys
from pathlib import Path

# Add codeflow_py_lib to path
sys.path.insert(0, str(Path(__file__).parent.parent))

from codeflow_py_lib import get_db, get_logger

logger = get_logger(__name__)

VALID_EVENT_TYPES = ("progress", "decision", "milestone", "blocker")
VALID_DOMAINS = ("planning", "development", "review", "qa", "ops", "documentation")


def main():
    parser = argparse.ArgumentParser(description="Query memory events")
    parser.add_argument(
        "--domain",
        choices=VALID_DOMAINS,
        help="Filter by domain",
    )
    parser.add_argument(
        "--event-type",
        choices=VALID_EVENT_TYPES,
        help="Filter by event type",
    )
    parser.add_argument(
        "--work-id",
        help="Filter by work ID",
    )
    parser.add_argument(
        "--memory-type",
        choices=("episodic", "semantic", "procedural"),
        help="Filter by memory type",
    )
    parser.add_argument(
        "--since",
        help="Filter events since timestamp (ISO format)",
    )
    parser.add_argument(
        "--limit",
        type=int,
        default=100,
        help="Maximum number of results (default: 100)",
    )
    parser.add_argument(
        "--format",
        choices=("json", "table"),
        default="json",
        help="Output format (default: json)",
    )
    args = parser.parse_args()

    try:
        db = get_db()

        # Build query dynamically
        conditions = ["1=1"]
        params = {}

        if args.domain:
            conditions.append("domain = :domain")
            params["domain"] = args.domain

        if args.event_type:
            conditions.append("event_type = :event_type")
            params["event_type"] = args.event_type

        if args.work_id:
            conditions.append("work_id = :work_id")
            params["work_id"] = args.work_id

        if args.memory_type:
            conditions.append("memory_type = :memory_type")
            params["memory_type"] = args.memory_type

        if args.since:
            conditions.append("created_at >= :since")
            params["since"] = args.since

        query = f"""
            SELECT id, event_type, domain, work_id, data, memory_type, created_at
            FROM memory_events
            WHERE {" AND ".join(conditions)}
            ORDER BY created_at DESC
            LIMIT {args.limit}
        """

        results = db.execute_query(query, params)

        # Parse JSON data field
        for row in results:
            if row.get("data"):
                try:
                    row["data"] = json.loads(row["data"])
                except json.JSONDecodeError:
                    pass

        if args.format == "json":
            print(json.dumps(results, indent=2))
        else:
            # Table format
            if not results:
                print("No memory events found.")
            else:
                print(f"{'ID':<35} {'Type':<12} {'Domain':<15} {'Created':<20}")
                print("-" * 85)
                for row in results:
                    print(
                        f"{row['id']:<35} {row['event_type']:<12} "
                        f"{row['domain']:<15} {row['created_at']:<20}"
                    )
                print(f"\nTotal: {len(results)} events")

        return 0

    except Exception as e:
        logger.error(f"Query error: {e}")
        print(json.dumps({"success": False, "error": str(e)}))
        return 2


if __name__ == "__main__":
    sys.exit(main())
