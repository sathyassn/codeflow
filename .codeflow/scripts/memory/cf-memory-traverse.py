#!/usr/bin/env python3
"""
cf-memory-traverse.py - Traverse memory graph for related memories.

Usage:
    cf-memory-traverse.py --start-id memory-xxx
    cf-memory-traverse.py --work-id TSK-xxx --depth 2

Exit Codes:
    0: Success
    1: Invalid arguments
    2: Database error
"""

import argparse
import json
import re
import sys
from pathlib import Path

# Add codeflow_py_lib to path
sys.path.insert(0, str(Path(__file__).parent.parent))

from codeflow_py_lib import get_db, get_logger

logger = get_logger(__name__)


def traverse_by_work_id(db, work_id: str, depth: int = 1) -> dict:
    """Traverse memories by work ID, finding related work."""
    visited = set()
    results = {
        "root_work_id": work_id,
        "depth": depth,
        "memories": [],
        "related_work": [],
    }

    work_ids_to_process = [work_id]
    current_depth = 0

    while work_ids_to_process and current_depth <= depth:
        next_work_ids = []

        for wid in work_ids_to_process:
            if wid in visited:
                continue
            visited.add(wid)

            # Get memories for this work ID
            memories = db.execute_query(
                """
                SELECT id, event_type, domain, work_id, data, memory_type, created_at
                FROM memory_events
                WHERE work_id = :work_id
                ORDER BY created_at DESC
                """,
                {"work_id": wid},
            )

            for mem in memories:
                mem["traverse_depth"] = current_depth
                if mem.get("data"):
                    try:
                        mem["data"] = json.loads(mem["data"])
                    except json.JSONDecodeError:
                        pass
                results["memories"].append(mem)

            # Find related work IDs in the data
            for mem in memories:
                if isinstance(mem.get("data"), dict):
                    # Look for references to other work IDs
                    data_str = json.dumps(mem["data"])
                    # Simple pattern matching for work IDs
                    related_ids = re.findall(r"(TSK-[A-Z0-9]+|EPC-[A-Z0-9]+)", data_str)
                    for rid in related_ids:
                        if rid not in visited:
                            next_work_ids.append(rid)
                            if rid not in results["related_work"]:
                                results["related_work"].append(rid)

        work_ids_to_process = next_work_ids
        current_depth += 1

    return results


def traverse_by_memory_id(db, memory_id: str, depth: int = 1) -> dict:
    """Traverse from a specific memory, finding related memories."""
    visited = set()
    results = {
        "root_memory_id": memory_id,
        "depth": depth,
        "memories": [],
        "related_memories": [],
    }

    # Get the starting memory
    start_mem = db.execute_query(
        "SELECT * FROM memory_events WHERE id = :id",
        {"id": memory_id},
    )

    if not start_mem:
        return {"error": f"Memory not found: {memory_id}"}

    start_mem = start_mem[0]
    if start_mem.get("data"):
        try:
            start_mem["data"] = json.loads(start_mem["data"])
        except json.JSONDecodeError:
            pass

    results["memories"].append(start_mem)
    visited.add(memory_id)

    # Find related by work_id
    if start_mem.get("work_id"):
        related = db.execute_query(
            """
            SELECT id, event_type, domain, work_id, data, memory_type, created_at
            FROM memory_events
            WHERE work_id = :work_id AND id != :id
            ORDER BY created_at DESC
            LIMIT 20
            """,
            {"work_id": start_mem["work_id"], "id": memory_id},
        )

        for mem in related:
            if mem["id"] not in visited:
                visited.add(mem["id"])
                if mem.get("data"):
                    try:
                        mem["data"] = json.loads(mem["data"])
                    except json.JSONDecodeError:
                        pass
                results["related_memories"].append(mem)

    # Find related by domain and time proximity
    related_by_domain = db.execute_query(
        """
        SELECT id, event_type, domain, work_id, data, memory_type, created_at
        FROM memory_events
        WHERE domain = :domain
          AND id != :id
          AND datetime(created_at) BETWEEN datetime(:created_at, '-1 hour')
                                       AND datetime(:created_at, '+1 hour')
        ORDER BY created_at DESC
        LIMIT 10
        """,
        {
            "domain": start_mem["domain"],
            "id": memory_id,
            "created_at": start_mem["created_at"],
        },
    )

    for mem in related_by_domain:
        if mem["id"] not in visited:
            visited.add(mem["id"])
            if mem.get("data"):
                try:
                    mem["data"] = json.loads(mem["data"])
                except json.JSONDecodeError:
                    pass
            mem["relation_type"] = "temporal_domain"
            results["related_memories"].append(mem)

    return results


def main():
    parser = argparse.ArgumentParser(description="Traverse memory graph")
    parser.add_argument(
        "--start-id",
        help="Starting memory ID to traverse from",
    )
    parser.add_argument(
        "--work-id",
        help="Work ID to find all related memories",
    )
    parser.add_argument(
        "--depth",
        type=int,
        default=1,
        help="Traversal depth (default: 1)",
    )
    parser.add_argument(
        "--format",
        choices=("json", "summary"),
        default="json",
        help="Output format (default: json)",
    )
    args = parser.parse_args()

    if not args.start_id and not args.work_id:
        parser.error("Either --start-id or --work-id is required")

    try:
        db = get_db()

        if args.start_id:
            results = traverse_by_memory_id(db, args.start_id, args.depth)
        else:
            results = traverse_by_work_id(db, args.work_id, args.depth)

        if args.format == "json":
            print(json.dumps(results, indent=2))
        else:
            # Summary format
            print(f"Traversal Results (depth={args.depth})")
            print("=" * 50)
            print(f"Total memories found: {len(results.get('memories', []))}")
            print(f"Related memories: {len(results.get('related_memories', []))}")
            if results.get("related_work"):
                print(f"Related work IDs: {', '.join(results['related_work'])}")

        return 0

    except Exception as e:
        logger.error(f"Traversal error: {e}")
        print(json.dumps({"success": False, "error": str(e)}))
        return 2


if __name__ == "__main__":
    sys.exit(main())
