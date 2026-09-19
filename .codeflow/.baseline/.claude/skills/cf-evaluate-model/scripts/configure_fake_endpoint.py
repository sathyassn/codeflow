#!/usr/bin/env python3
"""Bind one materialized diagnostic fixture to an owned loopback simulator."""

import argparse
import json
from pathlib import Path
from urllib.parse import urlsplit


def validated_loopback_url(ready_path: Path) -> str:
    payload = json.loads(ready_path.read_text(encoding="utf-8"))
    endpoint = payload.get("url")
    parsed = urlsplit(endpoint) if isinstance(endpoint, str) else None
    if (
        parsed is None
        or parsed.scheme != "http"
        or parsed.hostname != "127.0.0.1"
        or not parsed.port
        or parsed.path
        or parsed.username is not None
        or parsed.password is not None
        or parsed.query
        or parsed.fragment
        or payload.get("fake") is not True
    ):
        raise SystemExit("ready file must identify an owned fake loopback endpoint")
    return endpoint


def configure(actor_root: Path, ready_path: Path) -> None:
    actor_root = actor_root.resolve(strict=True)
    endpoint_path = actor_root / "tools" / "fake-endpoint.json"
    if endpoint_path.is_symlink() or not endpoint_path.is_file():
        raise SystemExit("materialized tools/fake-endpoint.json is required")
    current = json.loads(endpoint_path.read_text(encoding="utf-8"))
    if current.get("url") is not None or "setup_required" not in current:
        raise SystemExit("refusing to replace an already configured endpoint")
    endpoint = validated_loopback_url(ready_path.resolve(strict=True))
    endpoint_path.write_text(json.dumps({"url": endpoint}, indent=2) + "\n", encoding="utf-8")
    print(endpoint)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--actor-root", type=Path, required=True)
    parser.add_argument("--ready", type=Path, required=True)
    arguments = parser.parse_args()
    configure(arguments.actor_root, arguments.ready)
