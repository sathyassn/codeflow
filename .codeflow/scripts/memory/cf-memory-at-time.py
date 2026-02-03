#!/usr/bin/env python3
"""
cf-memory-at-time.py - Get memory state at a specific point in time.

Usage:
    cf-memory-at-time.py --timestamp "2026-01-25T10:00:00Z"
    cf-memory-at-time.py --timestamp "2026-01-25" --domain development

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

VALID_DOMAINS = ("planning", "development", "review", "qa", "ops", "documentation")


def main():
    parser = argparse.ArgumentParser(description="Get memory state at a point in time")
    parser.add_argument(
        "--timestamp",
        "-t",
        required=True,
        help="Point in time (ISO format: YYYY-MM-DD or YYYY-MM-DDTHH:MM:SSZ)",
    )
    parser.add_argument(
        "--domain",
        choices=VALID_DOMAINS,
        help="Filter by domain",
    )
    parser.add_argument(
        "--work-id",
        help="Filter by work ID",
    )
    parser.add_argument(
        "--window",
        type=int,
        default=0,
        help="Time window in hours before timestamp (default: 0 = exact time and before)",
    )
    parser.add_argument(
        "--limit",
        type=int,
        default=50,
        help="Maximum number of results (default: 50)",
    )
    parser.add_argument(
        "--format",
        choices=("json", "summary"),
        default="json",
        help="Output format (default: json)",
    )
    args = parser.parse_args()

    try:
        # Parse timestamp
        timestamp = args.timestamp
        if len(timestamp) == 10:  # Just date
            timestamp += "T23:59:59Z"
        elif not timestamp.endswith("Z"):
            timestamp += "Z"

        db = get_db()

        # Build query
        conditions = ["created_at <= :timestamp"]
        params = {"timestamp": timestamp}

        if args.window > 0:
            conditions.append(
                f"datetime(created_at) >= datetime(:timestamp, '-{args.window} hours')"
            )

        if args.domain:
            conditions.append("domain = :domain")
            params["domain"] = args.domain

        if args.work_id:
            conditions.append("work_id = :work_id")
            params["work_id"] = args.work_id

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

        output = {
            "query_timestamp": timestamp,
            "window_hours": args.window,
            "count": len(results),
            "memories": results,
        }

        if args.format == "json":
            print(json.dumps(output, indent=2))
        else:
            # Summary format
            print(f"Memory State at: {timestamp}")
            if args.window > 0:
                print(f"Window: {args.window} hours before")
            print("=" * 50)

            # Group by domain
            by_domain = {}
            for mem in results:
                domain = mem.get("domain", "unknown")
                if domain not in by_domain:
                    by_domain[domain] = []
                by_domain[domain].append(mem)

            for domain, mems in sorted(by_domain.items()):
                print(f"\n{domain.upper()} ({len(mems)} events):")
                for mem in mems[:5]:  # Show first 5
                    event_type = mem.get("event_type", "?")
                    content = ""
                    if isinstance(mem.get("data"), dict):
                        content = mem["data"].get("content", "")[:50]
                    print(f"  [{event_type}] {content}...")

            print(f"\nTotal: {len(results)} memories")

        return 0

    except ValueError as e:
        logger.error(f"Invalid timestamp: {e}")
        print(json.dumps({"success": False, "error": f"Invalid timestamp: {e}"}))
        return 1

    except Exception as e:
        logger.error(f"Query error: {e}")
        print(json.dumps({"success": False, "error": str(e)}))
        return 2


if __name__ == "__main__":
    sys.exit(main())
