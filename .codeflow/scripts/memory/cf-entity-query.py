#!/usr/bin/env python3
"""
cf-entity-query.py - Query entities extracted from memory.

Usage:
    cf-entity-query.py --name "Authentication"
    cf-entity-query.py --entity-type function --limit 20

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
    parser = argparse.ArgumentParser(description="Query extracted entities")
    parser.add_argument(
        "--name",
        help="Filter by entity name (partial match)",
    )
    parser.add_argument(
        "--entity-type",
        help="Filter by entity type (e.g., function, class, file, concept)",
    )
    parser.add_argument(
        "--source-id",
        help="Filter by source memory event ID",
    )
    parser.add_argument(
        "--category-id",
        help="Filter by category ID",
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

        # Check if entities table exists
        if not db.table_exists("entities"):
            print(
                json.dumps(
                    {
                        "success": True,
                        "message": "Entities table not yet populated",
                        "entities": [],
                    }
                )
            )
            return 0

        # Build query dynamically
        conditions = ["1=1"]
        params = {}

        if args.name:
            conditions.append("name LIKE :name")
            params["name"] = f"%{args.name}%"

        if args.entity_type:
            conditions.append("entity_type = :entity_type")
            params["entity_type"] = args.entity_type

        if args.source_id:
            conditions.append("source_event_id = :source_id")
            params["source_id"] = args.source_id

        if args.category_id:
            conditions.append("category_id = :category_id")
            params["category_id"] = args.category_id

        query = f"""
            SELECT id, name, entity_type, description, source_event_id,
                   category_id, properties, created_at
            FROM entities
            WHERE {" AND ".join(conditions)}
            ORDER BY created_at DESC
            LIMIT {args.limit}
        """

        results = db.execute_query(query, params)

        # Parse JSON properties field
        for row in results:
            if row.get("properties"):
                try:
                    row["properties"] = json.loads(row["properties"])
                except json.JSONDecodeError:
                    pass

        if args.format == "json":
            print(json.dumps(results, indent=2))
        else:
            # Table format
            if not results:
                print("No entities found.")
            else:
                print(f"{'ID':<35} {'Name':<25} {'Type':<15}")
                print("-" * 75)
                for row in results:
                    name = (row["name"] or "")[:25]
                    print(f"{row['id']:<35} {name:<25} {row['entity_type'] or '':<15}")
                print(f"\nTotal: {len(results)} entities")

        return 0

    except Exception as e:
        logger.error(f"Query error: {e}")
        print(json.dumps({"success": False, "error": str(e)}))
        return 2


if __name__ == "__main__":
    sys.exit(main())
