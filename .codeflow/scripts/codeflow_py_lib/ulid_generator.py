#!/usr/bin/env python3
"""
Purpose: CLI wrapper for ULID generation.
Usage: ulid_generator.py [--count N]

Generates Universally Unique Lexicographically Sortable Identifiers (ULIDs).
Wraps the core ulid module for command-line use and re-exports generate_ulid
for programmatic import.
"""
from __future__ import annotations

import argparse
import os
import sys
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from argparse import Namespace

# Ensure parent dir is on sys.path so codeflow_py_lib is importable when
# this script is run directly (python3 ulid_generator.py).
_PARENT_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
if _PARENT_DIR not in sys.path:
    sys.path.insert(0, _PARENT_DIR)

# Import generate_ulid from the sibling ulid module. Use importlib to load
# the file directly so we bypass __init__.py (which pulls in modules that
# shadow stdlib logging when run as a script).
import importlib.util as _ilu  # noqa: E402

_spec = _ilu.spec_from_file_location(
    "_codeflow_ulid",
    os.path.join(os.path.dirname(os.path.abspath(__file__)), "ulid.py"),
)
_ulid_mod = _ilu.module_from_spec(_spec)
_spec.loader.exec_module(_ulid_mod)
generate_ulid = _ulid_mod.generate_ulid

__all__ = ["generate_ulid"]


def main(args: Namespace) -> int:
    """Generate and print ULIDs."""
    for _ in range(args.count):
        print(generate_ulid())
    return 0


def parse_args() -> Namespace:
    """Parse command line arguments."""
    parser = argparse.ArgumentParser(
        description="Generate ULIDs (Universally Unique Lexicographically Sortable Identifiers)"
    )
    parser.add_argument(
        "--count",
        type=int,
        default=1,
        help="Number of ULIDs to generate (default: 1)",
    )
    return parser.parse_args()


if __name__ == "__main__":
    sys.exit(main(parse_args()))
