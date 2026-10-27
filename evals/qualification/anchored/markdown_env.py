"""Run the anchored-document grader in its managed, locked venv (TSK-445).

`check.py` parses narrative Markdown with markdown-it-py, pinned with its
dependency mdurl by version and wheel SHA-256 in `requirements.txt` beside
this file. `ensure()` never accepts a parser that happens to import: the
grader always runs inside its managed venv, `target/anchored-venv` (or
`$CODEFLOW_ANCHORED_VENV`), which holds exactly the pinned versions.

- Outside that venv, it prepares the venv and runs the current script again
  there.
- The venv is rebuilt when it is missing, when it was built from another
  requirements file (a digest stamp records which), or when its installed
  versions differ from the pins.
- Only a directory this helper created is ever removed: it writes an
  ownership marker naming the directory before it builds the venv, and a
  rebuild deletes only a directory, not a symlink, whose marker names it.
  An existing directory without that marker, or a symlinked target, is
  refused and left untouched.
- Installs use `--only-binary=:all: --require-hashes --no-deps`, so no
  source package is built. A failed install, or a version that still differs
  afterwards, stops the run; it never falls back to another parser.
"""

from __future__ import annotations

import hashlib
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
REQUIREMENTS = HERE / "requirements.txt"
STAMP = "codeflow-anchored-requirements.sha256"
OWNER = "codeflow-anchored-venv.owner"
INSTALL = ["--only-binary=:all:", "--require-hashes", "--no-deps"]
SETUP = (f"python3 -m venv target/anchored-venv && target/anchored-venv/bin/pip install "
         f"{' '.join(INSTALL)} -r {REQUIREMENTS.relative_to(ROOT)}")


def pins() -> dict[str, str]:
    """The pinned distributions, as {name: version}."""
    return dict(re.findall(r"^([A-Za-z0-9_.-]+)==([^\s\\]+)", REQUIREMENTS.read_text(encoding="utf-8"), re.M))


def digest() -> str:
    return hashlib.sha256(REQUIREMENTS.read_bytes()).hexdigest()


def venv_path() -> Path:
    """The managed venv's path as configured, made absolute but not resolved,
    so a symlinked target is seen as one."""
    return Path(os.path.abspath(os.environ.get("CODEFLOW_ANCHORED_VENV", ROOT / "target" / "anchored-venv")))


def owner_text(venv: Path) -> str:
    return f"codeflow anchored venv (evals/qualification/anchored/markdown_env.py)\n{venv}\n"


def owned(venv: Path) -> bool:
    """Whether this helper created the directory: a real directory, not a
    symlink, holding a marker that names it."""
    marker = venv / OWNER
    return (venv.is_dir() and not venv.is_symlink() and marker.is_file() and not marker.is_symlink()
            and marker.read_text(encoding="utf-8") == owner_text(venv))


def refuse_unowned(venv: Path) -> None:
    if venv.is_symlink():
        sys.exit(f"markdown_env: {venv} is a symlink; the managed venv must be a directory this helper created. "
                 "Nothing was changed.")
    if venv.exists() and not owned(venv):
        sys.exit(f"markdown_env: {venv} exists and was not created by this helper (no {OWNER} marker naming it); "
                 "it is left untouched. Point CODEFLOW_ANCHORED_VENV at a new path or remove it yourself.")


def interpreter(venv: Path) -> Path:
    return venv / ("Scripts/python.exe" if os.name == "nt" else "bin/python")


def installed(python: Path) -> dict[str, str]:
    """The pinned distributions' versions as the venv's interpreter sees them."""
    probe = ("import importlib.metadata as m, json, sys\n"
             "out = {}\n"
             "for name in sys.argv[1:]:\n"
             "    try: out[name] = m.version(name)\n"
             "    except m.PackageNotFoundError: out[name] = None\n"
             "print(json.dumps(out))")
    result = subprocess.run([str(python), "-I", "-c", probe, *pins()], capture_output=True, text=True, check=False)
    if result.returncode != 0:
        return {}
    import json
    return json.loads(result.stdout)


def current(venv: Path) -> bool:
    if not owned(venv):
        return False
    python = interpreter(venv)
    stamp = venv / STAMP
    return (python.exists() and stamp.exists() and stamp.read_text(encoding="utf-8").strip() == digest()
            and installed(python) == pins())


def prepare(venv: Path) -> Path:
    """Build or rebuild the managed venv; returns its interpreter."""
    refuse_unowned(venv)
    if current(venv):
        return interpreter(venv)
    # The base interpreter, resolved before a stale venv is removed: the
    # caller may be running from inside that venv.
    base = os.path.realpath(getattr(sys, "_base_executable", sys.executable))
    if venv.exists():
        shutil.rmtree(venv)  # owned: refuse_unowned() returned
    venv.mkdir(parents=True)
    (venv / OWNER).write_text(owner_text(venv), encoding="utf-8")
    subprocess.run([base, "-m", "venv", str(venv)], check=True)
    python = interpreter(venv)
    subprocess.run([str(python), "-m", "pip", "install", "--quiet", "--disable-pip-version-check", *INSTALL,
                    "-r", str(REQUIREMENTS)], check=True)
    found = installed(python)
    if found != pins():
        sys.exit(f"markdown_env: the managed venv holds {found}, not the pins {pins()}; run `{SETUP}`")
    (venv / STAMP).write_text(digest() + "\n", encoding="utf-8")
    return python


def ensure() -> None:
    venv = venv_path()
    if Path(sys.prefix).resolve() == venv.resolve() and current(venv):
        return
    python = prepare(venv)
    os.execv(str(python), [str(python), "-B", *sys.argv])


def versions() -> dict[str, str]:
    """The parser versions this process runs with."""
    import importlib.metadata as metadata
    return {name: metadata.version(name) for name in pins()}
