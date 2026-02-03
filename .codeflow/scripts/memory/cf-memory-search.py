#!/usr/bin/env python3
"""
cf-memory-search.py - Full-text search for memory events.

Usage:
    cf-memory-search.py "search query"
    cf-memory-search.py --query "authentication" --limit 20

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


def main():
    parser = argparse.ArgumentParser(description="Full-text search memory events")
    parser.add_argument(
        "query",
        nargs="?",
        help="Search query (FTS5 syntax)",
    )
    parser.add_argument(
        "--query",
        "-q",
        dest="query_opt",
        help="Search query (alternative to positional)",
    )
    parser.add_argument(
        "--domain",
        help="Filter by domain",
    )
    parser.add_argument(
        "--limit",
        type=int,
        default=50,
        help="Maximum number of results (default: 50)",
    )
    parser.add_argument(
        "--format",
        choices=("json", "table"),
        default="json",
        help="Output format (default: json)",
    )
    args = parser.parse_args()

    # Get query from either positional or optional argument
    search_query = args.query or args.query_opt
    if not search_query:
        parser.error("Search query is required")

    try:
        db = get_db()

        # Build FTS5 search query
        # Uses TEXT primary key, so we join on id
        base_query = """
            SELECT m.id, m.event_type, m.domain, m.work_id, m.data,
                   m.memory_type, m.created_at,
                   bm25(memory_fts) as score
            FROM memory_fts f
            JOIN memory_events m ON f.id = m.id
            WHERE memory_fts MATCH :query
        """

        params = {"query": search_query}

        if args.domain:
            base_query += " AND m.domain = :domain"
            params["domain"] = args.domain

        base_query += f" ORDER BY score LIMIT {args.limit}"

        results = db.execute_query(base_query, params)

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
                print(f"No results found for: {search_query}")
            else:
                print(f"Search results for: {search_query}")
                print(f"{'ID':<35} {'Score':<8} {'Type':<12} {'Domain':<12}")
                print("-" * 70)
                for row in results:
                    score = f"{row.get('score', 0):.2f}"
                    print(
                        f"{row['id']:<35} {score:<8} "
                        f"{row['event_type']:<12} {row['domain']:<12}"
                    )
                print(f"\nTotal: {len(results)} matches")

        return 0

    except Exception as e:
        logger.error(f"Search error: {e}")
        print(json.dumps({"success": False, "error": str(e)}))
        return 2


if __name__ == "__main__":
    sys.exit(main())
