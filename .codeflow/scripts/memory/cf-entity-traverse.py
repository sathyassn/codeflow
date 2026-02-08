#!/usr/bin/env python3
"""
cf-entity-traverse.py - Traverse entity relationship graph.

Usage:
    cf-entity-traverse.py --entity-id entity-xxx
    cf-entity-traverse.py --entity-id entity-xxx --depth 2 --relation-types "references,implements"

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


def traverse_relationships(
    db,
    entity_id: str,
    depth: int = 1,
    relation_types: list = None,
    direction: str = "both",
) -> dict:
    """Traverse entity relationships."""
    visited = set()
    results = {
        "root_entity_id": entity_id,
        "depth": depth,
        "entity": None,
        "relationships": [],
        "connected_entities": [],
    }

    # Get the root entity
    root = db.execute_query(
        "SELECT * FROM entities WHERE id = :id",
        {"id": entity_id},
    )

    if not root:
        return {"error": f"Entity not found: {entity_id}"}

    root = root[0]
    if root.get("properties"):
        try:
            root["properties"] = json.loads(root["properties"])
        except json.JSONDecodeError:
            pass
    results["entity"] = root
    visited.add(entity_id)

    # Build relationship query
    conditions = []
    params = {"entity_id": entity_id}

    if direction in ("both", "outgoing"):
        conditions.append("source_entity_id = :entity_id")
    if direction in ("both", "incoming"):
        conditions.append("target_entity_id = :entity_id")

    if relation_types:
        type_placeholders = ",".join(f":rtype_{i}" for i in range(len(relation_types)))
        for i, rtype in enumerate(relation_types):
            params[f"rtype_{i}"] = rtype
        conditions = [f"({' OR '.join(conditions)}) AND relation_type IN ({type_placeholders})"]

    where_clause = " OR ".join(conditions) if len(conditions) > 1 else conditions[0]

    # Get relationships
    relationships = db.execute_query(
        f"""
        SELECT r.id, r.source_entity_id, r.target_entity_id, r.relation_type,
               r.weight, r.properties, r.created_at
        FROM relationships r
        WHERE {where_clause}
        ORDER BY r.weight DESC
        """,
        params,
    )

    for rel in relationships:
        if rel.get("properties"):
            try:
                rel["properties"] = json.loads(rel["properties"])
            except json.JSONDecodeError:
                pass
        results["relationships"].append(rel)

        # Get connected entity
        connected_id = (
            rel["target_entity_id"]
            if rel["source_entity_id"] == entity_id
            else rel["source_entity_id"]
        )

        if connected_id not in visited:
            visited.add(connected_id)
            connected = db.execute_query(
                "SELECT * FROM entities WHERE id = :id",
                {"id": connected_id},
            )
            if connected:
                entity = connected[0]
                if entity.get("properties"):
                    try:
                        entity["properties"] = json.loads(entity["properties"])
                    except json.JSONDecodeError:
                        pass
                entity["relation_type"] = rel["relation_type"]
                entity["relation_direction"] = (
                    "outgoing" if rel["source_entity_id"] == entity_id else "incoming"
                )
                results["connected_entities"].append(entity)

    return results


def main():
    parser = argparse.ArgumentParser(description="Traverse entity relationships")
    parser.add_argument(
        "--entity-id",
        required=True,
        help="Entity ID to start traversal from",
    )
    parser.add_argument(
        "--depth",
        type=int,
        default=1,
        help="Traversal depth (default: 1)",
    )
    parser.add_argument(
        "--relation-types",
        help="Comma-separated list of relation types to follow",
    )
    parser.add_argument(
        "--direction",
        choices=("both", "outgoing", "incoming"),
        default="both",
        help="Direction of relationships to traverse (default: both)",
    )
    parser.add_argument(
        "--format",
        choices=("json", "tree"),
        default="json",
        help="Output format (default: json)",
    )
    args = parser.parse_args()

    try:
        db = get_db()

        # Check if tables exist
        if not db.table_exists("entities") or not db.table_exists("relationships"):
            print(
                json.dumps(
                    {
                        "success": True,
                        "message": "Entity tables not yet populated",
                        "entity": None,
                        "relationships": [],
                    }
                )
            )
            return 0

        relation_types = None
        if args.relation_types:
            relation_types = [t.strip() for t in args.relation_types.split(",")]

        results = traverse_relationships(
            db,
            args.entity_id,
            depth=args.depth,
            relation_types=relation_types,
            direction=args.direction,
        )

        if "error" in results:
            print(json.dumps({"success": False, "error": results["error"]}))
            return 1

        if args.format == "json":
            print(json.dumps(results, indent=2))
        else:
            # Tree format
            entity = results.get("entity", {})
            print(
                f"Entity: {entity.get('name', 'Unknown')} ({entity.get('entity_type', 'unknown')})"
            )
            print(f"ID: {args.entity_id}")
            print("=" * 50)
            print(f"\nRelationships ({len(results.get('relationships', []))}):")

            for conn in results.get("connected_entities", []):
                direction = conn.get("relation_direction", "?")
                rel_type = conn.get("relation_type", "?")
                arrow = "->" if direction == "outgoing" else "<-"
                print(
                    f"  {arrow} [{rel_type}] {conn.get('name', 'Unknown')} ({conn.get('entity_type', '?')})"
                )

        return 0

    except Exception as e:
        logger.error(f"Traversal error: {e}")
        print(json.dumps({"success": False, "error": str(e)}))
        return 2


if __name__ == "__main__":
    sys.exit(main())
