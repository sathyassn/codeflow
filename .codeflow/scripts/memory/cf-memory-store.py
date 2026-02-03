#!/usr/bin/env python3
"""
cf-memory-store.py - Store a memory event.

Usage:
    cf-memory-store.py --event-type progress --domain development --content "Message"
    cf-memory-store.py --event-type decision --domain planning --content "Decision" --work-id TSK-xxx

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

from codeflow_py_lib import (
    ValidationError,
    append_jsonl,
    generate_ulid,
    get_db,
    get_logger,
    get_state_dir,
)

logger = get_logger(__name__)

VALID_EVENT_TYPES = ("progress", "decision", "milestone", "blocker")
VALID_DOMAINS = ("planning", "development", "review", "qa", "ops", "documentation")
VALID_MEMORY_TYPES = ("episodic", "semantic", "procedural")


def main():
    parser = argparse.ArgumentParser(description="Store a memory event")
    parser.add_argument(
        "--event-type",
        required=True,
        choices=VALID_EVENT_TYPES,
        help="Type of memory event",
    )
    parser.add_argument(
        "--domain",
        required=True,
        choices=VALID_DOMAINS,
        help="Domain of the event",
    )
    parser.add_argument(
        "--content",
        required=True,
        help="Content/summary of the memory event",
    )
    parser.add_argument(
        "--work-id",
        help="Optional work ID (task/epic) this event relates to",
    )
    parser.add_argument(
        "--memory-type",
        choices=VALID_MEMORY_TYPES,
        help="Optional memory classification type",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="Output in JSON format",
    )
    args = parser.parse_args()

    try:
        # Generate event ID
        event_id = f"memory-{generate_ulid()}"

        # Prepare data payload
        data = {"content": args.content}

        # Get database operations
        db = get_db()

        # Write to SQLite (Tier 1 - operational state)
        db.execute_write(
            """
            INSERT INTO memory_events (id, event_type, domain, work_id, data, memory_type)
            VALUES (:id, :event_type, :domain, :work_id, :data, :memory_type)
            """,
            {
                "id": event_id,
                "event_type": args.event_type,
                "domain": args.domain,
                "work_id": args.work_id,
                "data": json.dumps(data),
                "memory_type": args.memory_type,
            },
        )

        # Write to JSONL (Tier 0 - rebuild authority)
        ledger_path = get_state_dir() / "ledger" / "memory-events.jsonl"
        append_jsonl(
            ledger_path,
            {
                "type": "memory_stored",
                "id": event_id,
                "event_type": args.event_type,
                "domain": args.domain,
                "work_id": args.work_id,
                "data": data,
                "memory_type": args.memory_type,
            },
        )

        result = {"success": True, "id": event_id}

        if args.json:
            print(json.dumps(result))
        else:
            print(f"Memory stored: {event_id}")

        return 0

    except ValidationError as e:
        logger.error(f"Validation error: {e}")
        if args.json:
            print(json.dumps({"success": False, "error": str(e)}))
        else:
            print(f"Error: {e}", file=sys.stderr)
        return 1

    except Exception as e:
        logger.error(f"Database error: {e}")
        if args.json:
            print(json.dumps({"success": False, "error": str(e)}))
        else:
            print(f"Error: {e}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
