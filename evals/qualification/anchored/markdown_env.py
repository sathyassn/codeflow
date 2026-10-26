"""Run the anchored-document grader under its pinned Markdown parser (TSK-445).

`check.py` parses narrative Markdown with markdown-it-py, pinned with its
dependency mdurl by version and SHA-256 in `requirements.txt` beside this
file. `ensure()` returns when both import; otherwise it installs them into
`target/anchored-venv` (or `$CODEFLOW_ANCHORED_VENV`) with `pip install
--require-hashes --no-deps` and runs the current script again there. A failed
install stops the run; it never falls back to an unpinned parser.
"""

from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
REQUIREMENTS = HERE / "requirements.txt"
MARK = "CODEFLOW_ANCHORED_ENV"
SETUP = f"python3 -m pip install --require-hashes --no-deps -r {REQUIREMENTS.relative_to(ROOT)}"


def ensure() -> None:
    try:
        import markdown_it  # noqa: F401
        import mdurl  # noqa: F401
        return
    except ImportError:
        pass
    if os.environ.get(MARK) == "1":
        sys.exit(f"markdown_env: markdown-it-py is still missing after installing {REQUIREMENTS}; run `{SETUP}`")
    venv = Path(os.environ.get("CODEFLOW_ANCHORED_VENV", ROOT / "target" / "anchored-venv"))
    python = venv / ("Scripts/python.exe" if os.name == "nt" else "bin/python")
    if not python.exists():
        subprocess.run([sys.executable, "-m", "venv", str(venv)], check=True)
    subprocess.run([str(python), "-m", "pip", "install", "--quiet", "--disable-pip-version-check",
                    "--require-hashes", "--no-deps", "-r", str(REQUIREMENTS)], check=True)
    os.environ[MARK] = "1"
    os.execv(str(python), [str(python), "-B", *sys.argv])
