"""
JSONL (JSON Lines) operations for CodeFlow scripts.

Handles reading, writing, and appending to JSONL ledger files.
"""

import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Dict, Generator, List, Optional

from .errors import JSONLError
from .ulid import generate_ulid


def read_jsonl(
    path: Path,
    validate: bool = True,
    skip_errors: bool = False,
) -> Generator[Dict[str, Any], None, None]:
    """
    Read JSONL file line by line.

    Yields dictionaries for each valid line.
    """
    if not path.exists():
        return

    with open(path, "r", encoding="utf-8") as f:
        for line_num, line in enumerate(f, start=1):
            line = line.strip()
            if not line:
                continue

            try:
                data = json.loads(line)
                if validate and not isinstance(data, dict):
                    raise JSONLError(
                        f"Expected object, got {type(data).__name__}",
                        line_number=line_num,
                    )
                yield data
            except json.JSONDecodeError as e:
                if skip_errors:
                    continue
                raise JSONLError(
                    f"Invalid JSON: {e}",
                    line_number=line_num,
                )


def write_jsonl(
    path: Path,
    events: List[Dict[str, Any]],
    overwrite: bool = False,
) -> int:
    """
    Write events to JSONL file.

    Returns number of events written.
    """
    mode = "w" if overwrite else "a"
    path.parent.mkdir(parents=True, exist_ok=True)

    count = 0
    with open(path, mode, encoding="utf-8") as f:
        for event in events:
            json_line = json.dumps(event, separators=(",", ":"))
            f.write(json_line + "\n")
            count += 1

    return count


def append_jsonl(
    path: Path,
    event_type: str,
    data: Dict[str, Any],
    add_timestamp: bool = True,
    add_id: bool = True,
) -> Dict[str, Any]:
    """
    Append single event to JSONL file.

    Returns the complete event that was written.
    """
    event = {"e": event_type, **data}

    if add_timestamp and "ts" not in event:
        event["ts"] = datetime.now(timezone.utc).isoformat()

    if add_id and "id" not in event:
        event["id"] = generate_ulid()

    write_jsonl(path, [event])
    return event


def count_events(path: Path, event_type: Optional[str] = None) -> int:
    """Count events in JSONL file, optionally filtered by type."""
    count = 0
    for event in read_jsonl(path, validate=False, skip_errors=True):
        if event_type is None or event.get("e") == event_type:
            count += 1
    return count


def filter_events(
    path: Path,
    event_type: Optional[str] = None,
    since: Optional[datetime] = None,
    until: Optional[datetime] = None,
) -> Generator[Dict[str, Any], None, None]:
    """Filter events from JSONL file."""
    for event in read_jsonl(path):
        if event_type and event.get("e") != event_type:
            continue

        if since or until:
            ts_str = event.get("ts")
            if ts_str:
                ts = datetime.fromisoformat(ts_str.replace("Z", "+00:00"))
                if since and ts < since:
                    continue
                if until and ts > until:
                    continue

        yield event
