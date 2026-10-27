"""Run the anchored-document grader in a content-named, locked venv (TSK-445).

`check.py` parses narrative Markdown with markdown-it-py, pinned with its
dependency mdurl by version and wheel SHA-256 in `requirements.txt` beside
this file. `ensure()` never accepts a parser that merely imports: the grader
always runs inside a managed venv that holds exactly the pins.

- The managed root is fixed: `<repository>/target/anchored`. There is no
  override; `CODEFLOW_ANCHORED_VENV`, which once named another root, is
  refused if set.
- The venv's directory is named by its content: the SHA-256 of
  `requirements.txt`, the Python implementation and version, and the
  machine. Changed pins or another Python give another name.
- A missing venv is built with copied interpreters (`venv --copies`, so the
  interpreter is a real file, not a link) in a fresh private directory
  (`tempfile.mkdtemp`, mode 0700) beside its name, installed with
  `--only-binary=:all: --require-hashes --no-deps`, verified, then moved into
  place with one `os.rename`.
- An existing venv of that name is used only when its whole chain is
  trusted (below), its stamp names the same requirements and Python, and
  its installed versions are the pins.
- Nothing is ever deleted. A stale venv keeps its old name and goes unused.
  A path of the managed name that is not a valid managed venv is refused and
  left as it is. If the name appears while a build runs, the build is left
  in its own directory and the run stops. Old venvs and builds are removed
  by hand (README "Grading anchored present documents").

Threat model. This helper is a developer and evaluator convenience, not a
security boundary.

- In scope: other users, accidental collisions at the managed name, and
  stale or mismatched environments.
- The chain rule, checked before anything under the root runs: every path
  from the repository root down to the interpreter file (the repository,
  `target/`, `target/anchored/`, the venv, `bin/` or `Scripts/`, the
  interpreter), and the paths Python loads from at start (`pyvenv.cfg` and
  `lib/`, `lib/pythonX.Y/`, `site-packages/`, or `Lib/` and
  `Lib/site-packages/` on Windows), must each be owned by the current user,
  not writable by group or others, and not a symlink. On POSIX each is
  opened without following a link and checked on the open file. On
  Windows, which has no equivalent owner and mode check here, each path's
  resolved real path must lie inside the repository and no path may be a
  reparse point (a symlink or junction). Anything else is refused before
  its interpreter runs.
- Checked cheaply during a build: before each step the staging directory
  must still be the directory the helper created (same device and inode,
  read without following symlinks); otherwise the build is refused.
- Out of scope: a concurrent process running as the same user. It can
  already write these files directly, so the checks catch interference but
  do not claim to defeat it.
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
from typing import Callable

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
REQUIREMENTS = HERE / "requirements.txt"
STAMP = "codeflow-anchored-venv.json"
INSTALL = ["--only-binary=:all:", "--require-hashes", "--no-deps"]
RETIRED = "CODEFLOW_ANCHORED_VENV"
REMEDY = ("run the grader again (it builds the venv itself), or remove the path by hand after checking what it "
          "holds")
WINDOWS_REPARSE_POINT = 0x400  # FILE_ATTRIBUTE_REPARSE_POINT


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
    """The managed root: always `<repository>/target/anchored`."""
    return ROOT / "target" / "anchored"


def venv_path() -> Path:
    """The managed venv for these pins and this Python."""
    return root() / f"anchored-venv-{digest()[:16]}-{python_tag()}"


def interpreter(venv: Path, windows: bool | None = None) -> Path:
    windows = os.name == "nt" if windows is None else windows
    return venv / ("Scripts/python.exe" if windows else "bin/python")


def startup_paths(venv: Path, windows: bool | None = None) -> list[Path]:
    """The paths Python reads at start from a venv: its configuration and
    the directories its site-packages sit in."""
    if os.name == "nt" if windows is None else windows:
        return [venv / "pyvenv.cfg", venv / "Lib", venv / "Lib" / "site-packages"]
    lib = venv / "lib" / f"python{sys.version_info.major}.{sys.version_info.minor}"
    return [venv / "pyvenv.cfg", venv / "lib", lib, lib / "site-packages"]


def chain(venv: Path) -> list[Path]:
    """Every path the chain rule covers, from the repository root down: the
    directories to the interpreter, the interpreter, then the startup paths."""
    python = interpreter(venv)
    paths = [ROOT]
    for part in python.relative_to(ROOT).parts:
        paths.append(paths[-1] / part)
    return paths + startup_paths(venv)


def posix_problem(path: Path) -> str | None:
    """Why one path fails the rule on POSIX, or None: opened without
    following a link, it must be owned by the current user and not writable
    by group or others."""
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
    try:
        handle = os.open(path, flags)
    except OSError as error:
        return f"{path} cannot be opened without following a link ({error.strerror})"
    try:
        info = os.fstat(handle)
    finally:
        os.close(handle)
    if info.st_uid != os.getuid():
        return f"{path} is owned by uid {info.st_uid}, not the current user"
    if info.st_mode & (stat.S_IWGRP | stat.S_IWOTH):
        return f"{path} is writable by group or others (mode {stat.S_IMODE(info.st_mode):o})"
    return None


def windows_problem(path: Path, repository: Path, realpath: Callable[[str], str] = os.path.realpath,
                    attributes: Callable[[Path], int] | None = None) -> str | None:
    """Why one path fails the rule on Windows, or None: no reparse point, and
    its resolved real path inside the repository. The resolvers are
    parameters so the policy can be tested on any system."""
    read = attributes or (lambda p: getattr(os.lstat(p), "st_file_attributes", 0))
    try:
        if read(path) & WINDOWS_REPARSE_POINT:
            return f"{path} is a reparse point (a symlink or junction)"
        real = Path(realpath(str(path)))
    except OSError as error:
        return f"{path} cannot be read ({error.strerror})"
    base = Path(realpath(str(repository)))
    if real != base and base not in real.parents:
        return f"{path} resolves to {real}, outside the repository {base}"
    return None


def chain_problem(venv: Path) -> str | None:
    """The first path of the venv's chain that fails the rule, as a reason."""
    return directories_problem(chain(venv))


def directories_problem(paths: list[Path]) -> str | None:
    """The first of these paths that fails the rule, as a reason."""
    for path in paths:
        problem = windows_problem(path, ROOT) if os.name == "nt" else posix_problem(path)
        if problem:
            return problem
    return None


def root_chain() -> list[Path]:
    paths = [ROOT]
    for part in root().relative_to(ROOT).parts:
        paths.append(paths[-1] / part)
    return paths


def installed(python: Path) -> dict[str, str | None]:
    """The pinned distributions' versions as a venv's interpreter sees them.
    Run only after the chain rule passed for that venv."""
    probe = ("import importlib.metadata as m, json, sys\n"
             "out = {}\n"
             "for name in sys.argv[1:]:\n"
             "    try: out[name] = m.version(name)\n"
             "    except m.PackageNotFoundError: out[name] = None\n"
             "print(json.dumps(out))")
    result = subprocess.run([str(python), "-I", "-c", probe, *pins()], capture_output=True, text=True, check=False)
    return json.loads(result.stdout) if result.returncode == 0 else {}


def valid(venv: Path) -> bool:
    """The chain rule holds, the stamp names these pins and this Python, and
    the interpreter sees exactly the pinned versions. The chain rule comes
    first, so an untrusted interpreter never runs."""
    if chain_problem(venv):
        return False
    stamp = venv / STAMP
    if stamp.is_symlink() or not stamp.is_file():
        return False
    try:
        if json.loads(stamp.read_text(encoding="utf-8")) != stamp_data():
            return False
    except (OSError, ValueError):
        return False
    return installed(interpreter(venv)) == pins()


def create(build: Path) -> None:
    """Create a venv with copied interpreters in the empty staging directory
    from the base interpreter."""
    base = os.path.realpath(getattr(sys, "_base_executable", sys.executable))
    subprocess.run([base, "-m", "venv", "--copies", str(build)], check=True)


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


def refuse(message: str) -> None:
    sys.exit(f"markdown_env: {message}. Nothing under the managed root was run or removed; {REMEDY}.")


def build(venv: Path) -> None:
    """Build the venv in a private directory and move it into place. Never
    deletes: a failed or raced build stays in its own directory."""
    problem = directories_problem([p for p in root_chain() if os.path.lexists(p)])
    if problem:
        refuse(f"the managed root is not trusted: {problem}")
    venv.parent.mkdir(parents=True, exist_ok=True)
    problem = directories_problem(root_chain())
    if problem:
        refuse(f"the managed root is not trusted: {problem}")
    parent = Path(tempfile.mkdtemp(prefix=f".{venv.name}.build-", dir=venv.parent))
    staging = parent / "venv"
    os.mkdir(staging, 0o700)
    created = identity(staging)

    def still_ours(step: str) -> None:
        pause(step)
        if identity(staging) != created:
            refuse(f"{staging} is no longer the directory this helper created (before {step})")

    still_ours("create")
    create(staging)
    still_ours("install")
    problem = chain_problem(staging)
    if problem:
        refuse(f"the build's chain is not trusted: {problem}")
    install(staging)
    still_ours("verify")
    found = installed(interpreter(staging))
    if found != pins():
        refuse(f"the build at {staging} holds {found}, not the pins {pins()}; nothing was moved into place")
    still_ours("stamp")
    (staging / STAMP).write_text(json.dumps(stamp_data(), sort_keys=True) + "\n", encoding="utf-8")
    still_ours("rename")
    if os.path.lexists(venv):
        refuse(f"{venv} appeared while the venv was built; it is left as it is and the build stays at {staging}")
    try:
        os.rename(staging, venv)
    except OSError as error:
        refuse(f"could not move the build at {staging} to {venv} ({error})")


def ensure() -> None:
    if os.environ.get(RETIRED):
        refuse(f"{RETIRED} is set, but the managed root is fixed at {root()} and can no longer be moved; "
               f"unset {RETIRED}")
    venv = venv_path()
    if Path(sys.prefix).resolve() == venv.resolve() and valid(venv):
        return
    problem = directories_problem([p for p in root_chain() if os.path.lexists(p)])
    if problem:
        refuse(f"the managed root is not trusted: {problem}")
    if os.path.lexists(venv):
        problem = chain_problem(venv)
        if problem:
            refuse(f"the managed venv is not trusted: {problem}")
        if not valid(venv):
            refuse(f"{venv} exists but is not a valid managed venv for these pins; it is left as it is")
    else:
        build(venv)
        if not valid(venv):
            refuse(f"{venv} is not a valid managed venv after the build; it is left as it is")
    python = interpreter(venv)
    os.execv(str(python), [str(python), "-B", *sys.argv])


def versions() -> dict[str, str]:
    """The parser versions this process runs with."""
    import importlib.metadata as metadata
    return {name: metadata.version(name) for name in pins()}
