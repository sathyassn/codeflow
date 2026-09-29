#!/usr/bin/env python3
"""Print the binary built by the current gate, including a custom target dir."""

import os
from pathlib import Path

print((Path(os.environ.get("CARGO_TARGET_DIR", "target")) / "debug" / "codeflow").resolve())
