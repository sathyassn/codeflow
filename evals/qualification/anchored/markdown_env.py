"""Run the anchored-document grader in a content-named, locked venv (TSK-445).

`check.py` parses narrative Markdown with markdown-it-py, pinned with its
dependency mdurl by version and wheel SHA-256 in `requirements.txt` beside
this file. `ensure()` never accepts a parser that merely imports: the grader
always runs inside a managed venv that holds exactly the pins.

- The venv lives under a managed root, `target/` (or `$CODEFLOW_ANCHORED_VENV`,
  the root to build under), in a directory named by its content: the
  SHA-256 of `requirements.txt`, the Python implementation and version, and
  the machine. Changed pins or another Python give another name.
- A missing venv is built in a fresh private directory beside it
  (`tempfile.mkdtemp`), installed with `--only-binary=:all: --require-hashes
  --no-deps`, verified, then moved into place with one `os.rename`.
- An existing venv of that name is used when it is valid: a real directory,
  not a symlink, whose stamp names the same requirements and Python and
  whose installed versions are the pins.
- Nothing is ever deleted. A stale venv keeps its old name and goes unused.
  A path of the managed name that is not a valid managed venv is refused and
  left as it is. If the name appears while a build runs, the build is left
  in its own directory and the run stops. Old venvs and builds are removed
  by hand (README "Grading anchored present documents").

Threat model. This helper is a developer and evaluator convenience, not a
security boundary.

- In scope: other users (the build directory is made by `tempfile.mkdtemp`,
  mode 0700, and the venv is built inside it), accidental collisions at the
  managed name, and stale or mismatched environments.
- Root requirement, checked the way ssh checks its directories: before the
  helper trusts or runs anything under the root, the root and the managed
  venv directory must each be a real directory (opened without following a
  symlink) owned by the current user and not writable by group or others.
  Anything else is refused before its interpreter runs. The default root,
  `target/` in the user's own checkout, passes. On Windows, which has no
  equivalent mode check here, a configured root outside the repository is
  refused.
- Checked cheaply: before each build step the staging directory must still
  be the directory the helper created (same device and inode, read without
  following symlinks); otherwise the build is refused.
- Out of scope: a concurrent process running as the same user. It can
  already write these files directly, so the identity check catches
  interference but does not claim to defeat it.
- Residual: `os.rename` replaces an empty directory created at the managed
  name in the instant between the last check and the move. A directory with
  content, a file or a symlink there makes the rename fail, and nothing is
  removed.
"""

from __future__ import annotations

import hashlib
import json
import os
import platform
import stat
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
REQUIREMENTS = HERE / "requirements.txt"
STAMP = "codeflow-anchored-venv.json"
INSTALL = ["--only-binary=:all:", "--require-hashes", "--no-deps"]
REMEDY = ("run the grader again (it builds the venv itself), set CODEFLOW_ANCHORED_VENV to another root "
          "to build under, or remove the path by hand after checking what it holds")


def pins() -> dict[str, str]:
    """The pinned distributions, as {name: version}."""
    import re
    return dict(re.findall(r"^([A-Za-z0-9_.-]+)==([^\s\\]+)", REQUIREMENTS.read_text(encoding="utf-8"), re.M))


def digest() -> str:
    return hashlib.sha256(REQUIREMENTS.read_bytes()).hexdigest()


def python_tag() -> str:
    return f"{sys.implementation.name}{sys.version_info.major}.{sys.version_info.minor}-{platform.machine() or 'any'}"


def stamp_data() -> dict[str, str]:
    return {"requirements_sha256": digest(), "python": python_tag(), "pins": json.dumps(pins(), sort_keys=True)}


def root() -> Path:
    """The managed root to build under, absolute and not resolved."""
    return Path(os.path.abspath(os.environ.get("CODEFLOW_ANCHORED_VENV", ROOT / "target")))


def venv_path() -> Path:
    """The managed venv for these pins and this Python."""
    return root() / f"anchored-venv-{digest()[:16]}-{python_tag()}"


def interpreter(venv: Path) -> Path:
    return venv / ("Scripts/python.exe" if os.name == "nt" else "bin/python")


def installed(python: Path) -> dict[str, str | None]:
    """The pinned distributions' versions as a venv's interpreter sees them."""
    probe = ("import importlib.metadata as m, json, sys\n"
             "out = {}\n"
             "for name in sys.argv[1:]:\n"
             "    try: out[name] = m.version(name)\n"
             "    except m.PackageNotFoundError: out[name] = None\n"
             "print(json.dumps(out))")
    result = subprocess.run([str(python), "-I", "-c", probe, *pins()], capture_output=True, text=True, check=False)
    return json.loads(result.stdout) if result.returncode == 0 else {}


def untrusted(path: Path) -> str | None:
    """Why a root or venv directory may not be trusted, or None. The
    directory is opened without following a symlink and checked on that
    open file: a directory owned by the current user and not writable by
    group or others."""
    if os.name == "nt":
        configured = os.environ.get("CODEFLOW_ANCHORED_VENV")
        if configured and not Path(os.path.abspath(configured)).is_relative_to(ROOT):
            return "a configured root outside the repository is refused on Windows, which has no mode check here"
        return None
    flags = os.O_RDONLY | getattr(os, "O_DIRECTORY", 0) | getattr(os, "O_NOFOLLOW", 0)
    try:
        handle = os.open(path, flags)
    except OSError as error:
        return f"it cannot be opened as a directory without following a symlink ({error.strerror})"
    try:
        info = os.fstat(handle)
    finally:
        os.close(handle)
    if not stat.S_ISDIR(info.st_mode):
        return "it is not a directory"
    if info.st_uid != os.getuid():
        return f"it is owned by uid {info.st_uid}, not the current user"
    if info.st_mode & (stat.S_IWGRP | stat.S_IWOTH):
        return f"it is writable by group or others (mode {stat.S_IMODE(info.st_mode):o})"
    return None


def require_trusted(path: Path, what: str) -> None:
    reason = untrusted(path)
    if reason:
        sys.exit(f"markdown_env: the {what} {path} is not trusted: {reason}. Nothing under it was run or changed. "
                 f"{REMEDY}.")


def valid(venv: Path) -> bool:
    """A real directory, not a symlink, owned by the current user and not
    writable by others under a root of the same kind, stamped for these pins
    and this Python, whose interpreter sees exactly the pinned versions. The
    ownership checks come first, so an untrusted interpreter never runs."""
    if untrusted(venv.parent) or untrusted(venv):
        return False
    stamp = venv / STAMP
    if venv.is_symlink() or not venv.is_dir() or stamp.is_symlink() or not stamp.is_file():
        return False
    try:
        if json.loads(stamp.read_text(encoding="utf-8")) != stamp_data():
            return False
    except (OSError, ValueError):
        return False
    return interpreter(venv).exists() and installed(interpreter(venv)) == pins()


def create(build: Path) -> None:
    """Create a venv in the empty staging directory from the base interpreter."""
    base = os.path.realpath(getattr(sys, "_base_executable", sys.executable))
    subprocess.run([base, "-m", "venv", str(build)], check=True)


def install(build: Path) -> None:
    subprocess.run([str(interpreter(build)), "-m", "pip", "install", "--quiet", "--disable-pip-version-check",
                    *INSTALL, "-r", str(REQUIREMENTS)], check=True)


def pause(step: str) -> None:
    """A no-op before each identity check; tests replace it to interfere."""


def identity(path: Path) -> tuple[int, int] | None:
    try:
        info = os.stat(path, follow_symlinks=False)
    except OSError:
        return None
    return (info.st_dev, info.st_ino) if stat.S_ISDIR(info.st_mode) else None


def build(venv: Path) -> None:
    """Build the venv in a private directory and move it into place. Never
    deletes: a failed or raced build stays in its own directory."""
    venv.parent.mkdir(parents=True, exist_ok=True)
    require_trusted(venv.parent, "managed root")
    parent = Path(tempfile.mkdtemp(prefix=f".{venv.name}.build-", dir=venv.parent))
    staging = parent / "venv"
    os.mkdir(staging, 0o700)
    created = identity(staging)

    def still_ours(step: str) -> None:
        pause(step)
        if identity(staging) != created:
            sys.exit(f"markdown_env: {staging} is no longer the directory this helper created (before {step}); "
                     f"the build stops and nothing was removed. {REMEDY}.")

    still_ours("create")
    create(staging)
    still_ours("install")
    install(staging)
    still_ours("verify")
    found = installed(interpreter(staging))
    if found != pins():
        sys.exit(f"markdown_env: the build at {staging} holds {found}, not the pins {pins()}; nothing was "
                 f"moved into place. {REMEDY}.")
    still_ours("stamp")
    (staging / STAMP).write_text(json.dumps(stamp_data(), sort_keys=True) + "\n", encoding="utf-8")
    still_ours("rename")
    if os.path.lexists(venv):
        sys.exit(f"markdown_env: {venv} appeared while the venv was built; it is left as it is and the build "
                 f"stays at {staging}. {REMEDY}.")
    try:
        os.rename(staging, venv)
    except OSError as error:
        sys.exit(f"markdown_env: could not move the build at {staging} to {venv} ({error}); nothing was "
                 f"removed. {REMEDY}.")


def ensure() -> None:
    venv = venv_path()
    if os.path.lexists(venv.parent):
        require_trusted(venv.parent, "managed root")
    if os.path.lexists(venv):
        require_trusted(venv, "managed venv")
    if Path(sys.prefix).resolve() == venv.resolve() and valid(venv):
        return
    if os.path.lexists(venv) and not valid(venv):
        sys.exit(f"markdown_env: {venv} exists but is not a valid managed venv for these pins; it is left as "
                 f"it is. {REMEDY}.")
    if not os.path.lexists(venv):
        build(venv)
    if not valid(venv):
        sys.exit(f"markdown_env: {venv} is not a valid managed venv after the build; it is left as it is. "
                 f"{REMEDY}.")
    python = interpreter(venv)
    os.execv(str(python), [str(python), "-B", *sys.argv])


def versions() -> dict[str, str]:
    """The parser versions this process runs with."""
    import importlib.metadata as metadata
    return {name: metadata.version(name) for name in pins()}
