"""Run a script under merge admission's pinned verifier (TSK-1001).

`scripts/ci-reuse.py` verifies a run's OIDC token with PyJWT and
cryptography, pinned by version and SHA-256 in
`scripts/requirements-admission.txt`. A test of that verifier calls
`ensure()` first: when the libraries are importable it returns; otherwise it
installs them into `target/admission-venv` with `pip install
--require-hashes --no-deps` and runs the script again there. A failed
install stops the run; it never falls back to another verifier.
"""

from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
REQUIREMENTS = ROOT / "scripts" / "requirements-admission.txt"
MARK = "CODEFLOW_ADMISSION_ENV"


def ensure() -> None:
    try:
        import cryptography  # noqa: F401
        import jwt  # noqa: F401
        return
    except ImportError:
        pass
    if os.environ.get(MARK) == "1":
        sys.exit(f"admission_env: PyJWT and cryptography are still missing after installing {REQUIREMENTS}")
    venv = Path(os.environ.get("CODEFLOW_ADMISSION_VENV", ROOT / "target" / "admission-venv"))
    python = venv / ("Scripts/python.exe" if os.name == "nt" else "bin/python")
    if not python.exists():
        subprocess.run([sys.executable, "-m", "venv", str(venv)], check=True)
    subprocess.run([str(python), "-m", "pip", "install", "--quiet", "--disable-pip-version-check",
                    "--require-hashes", "--no-deps", "-r", str(REQUIREMENTS)], check=True)
    os.environ[MARK] = "1"
    os.execv(str(python), [str(python), "-B", *sys.argv])
