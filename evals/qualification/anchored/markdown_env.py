"""The anchored-document grader's pinned parser, without a venv (TSK-445).

`check.py` parses narrative Markdown with markdown-it-py and its dependency
mdurl, both pure Python, pinned by version and wheel SHA-256 in
`requirements.txt` beside this file. There is no venv and no interpreter of
ours: the helper downloads each pinned wheel, checks its SHA-256, unpacks it
with `zipfile` into a content-named directory, and puts that directory on
`sys.path` of a grader that runs isolated.

- The grader runs as `python3 -I -S -B check.py ...`. `-I` ignores every
  `PYTHON*` variable, the user site and the script's own directory; `-S`
  skips `site`, so no `.pth` file, `sitecustomize` or `usercustomize` is
  read. `-B` is optional: `ensure()` turns bytecode writing off before the
  verified packages are imported, so no `.pyc` lands beside them.
  `ensure()` refuses to run under any other start, so no path through
  this code runs with the caller's Python environment. Its only imports
  outside the standard library are the two pinned packages, from the
  directory it verified.
- The directory is `<repository>/target/anchored/<requirements hash>/lib`.
  The root is fixed; `CODEFLOW_ANCHORED_VENV`, which once moved it, is
  refused if set. Changed pins give another name.
- A missing directory is built in a fresh private directory
  (`tempfile.mkdtemp`, mode 0700) beside its name: each wheel is fetched
  from the package index by its pinned SHA-256, checked against it, and
  unpacked with entries confined to the directory. A stamp records the
  SHA-256 of every unpacked file. The build is moved into place with one
  `os.rename`.
- Nothing is ever deleted. A stale directory keeps its old name and goes
  unused; a path at the managed name that is not a valid build is refused
  and left as it is. Old builds are removed by hand.

Threat model. This helper is a developer and evaluator convenience, not a
security boundary.

- Trusted bootstrap: the interpreter that runs `check.py` and its standard
  library, like any system tool, and this repository's own source. Only the
  downloaded code is the helper's to verify.
- In scope: other users, accidental collisions at the managed name, stale or
  mismatched builds, and tampering with the unpacked files.
- The chain rule, checked before the directory is put on `sys.path`: every
  path from the repository root to the `lib` directory, and every directory
  and file under it, must be owned by the current user, not writable by
  group or others, and not a symlink; on POSIX each is opened without
  following a link and checked on the open file. On Windows, which has no
  equivalent owner and mode check here, each path's resolved real path must
  lie inside the repository and none may be a reparse point. Every file
  must also match the SHA-256 the stamp recorded from the verified wheel,
  and no file may be added.
- Checked cheaply during a build: before each step the staging directory
  must still be the directory the helper created (same device and inode,
  read without following symlinks).
- Out of scope: a concurrent process running as the same user, which can
  already write these files directly.
- Residual: `os.rename` replaces an empty directory created at the managed
  name in the instant before the move; a directory with content, a file or
  a symlink there makes the rename fail, and nothing is removed.
"""

from __future__ import annotations

import hashlib
import io
import json
import os
import re
import stat
import sys
import tempfile
import urllib.request
import zipfile
from pathlib import Path, PurePosixPath, PureWindowsPath
from typing import Callable

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
REQUIREMENTS = HERE / "requirements.txt"
STAMP = "codeflow-anchored-lib.json"
INDEX = "https://pypi.org/pypi/{name}/{version}/json"
RETIRED = "CODEFLOW_ANCHORED_VENV"
START = "python3 -I -S -B evals/qualification/anchored/check.py <case> <document.json>"
REMEDY = ("run the grader again (it builds what it needs), or remove the path by hand after checking what it "
          "holds")
WINDOWS_REPARSE_POINT = 0x400  # FILE_ATTRIBUTE_REPARSE_POINT


def requirements() -> list[tuple[str, str, str]]:
    """The pinned wheels, as (name, version, sha256)."""
    text = REQUIREMENTS.read_text(encoding="utf-8")
    return re.findall(r"^([A-Za-z0-9_.-]+)==([^\s\\]+)\s*\\\s*--hash=sha256:([0-9a-f]{64})", text, re.M)


def pins() -> dict[str, str]:
    return {name: version for name, version, _ in requirements()}


def digest() -> str:
    return hashlib.sha256(REQUIREMENTS.read_bytes()).hexdigest()


def root() -> Path:
    """The managed root: always `<repository>/target/anchored`."""
    return ROOT / "target" / "anchored"


def build_path() -> Path:
    """The content-named build for these pins."""
    return root() / digest()[:16]


def lib_path(build: Path | None = None) -> Path:
    return (build or build_path()) / "lib"


def root_chain() -> list[Path]:
    paths = [ROOT]
    for part in root().relative_to(ROOT).parts:
        paths.append(paths[-1] / part)
    return paths


# The chain rule.

def posix_problem(path: Path) -> str | None:
    """Why one path fails the rule on POSIX, or None."""
    try:
        handle = os.open(path, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
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
    its resolved real path inside the repository."""
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


def problem(path: Path) -> str | None:
    return windows_problem(path, ROOT) if os.name == "nt" else posix_problem(path)


def paths_problem(paths: list[Path]) -> str | None:
    """The first of these paths that fails the rule, as a reason."""
    for path in paths:
        reason = problem(path)
        if reason:
            return reason
    return None


def tree(directory: Path) -> list[Path]:
    """Every entry under a directory, links included and not followed."""
    out = []
    for entry in sorted(directory.iterdir()):
        out.append(entry)
        if entry.is_dir() and not entry.is_symlink():
            out.extend(tree(entry))
    return out


def chain(build: Path) -> list[Path]:
    """Every path the rule covers for a build: the repository down to `lib`,
    the stamp, then every entry under `lib`."""
    paths = [ROOT]
    for part in lib_path(build).relative_to(ROOT).parts:
        paths.append(paths[-1] / part)
    return paths + [build / STAMP] + tree(lib_path(build))


# Integrity.

def file_digests(lib: Path) -> dict[str, str]:
    return {str(path.relative_to(lib).as_posix()): hashlib.sha256(path.read_bytes()).hexdigest()
            for path in tree(lib) if path.is_file() and not path.is_symlink()}


def stamp_data(files: dict[str, str]) -> dict[str, object]:
    return {"requirements_sha256": digest(), "pins": pins(), "files": files}


def build_problem(build: Path) -> str | None:
    """Why a build may not be used, or None: the chain rule, then the stamp,
    then every file's SHA-256 against the stamp."""
    if not os.path.isdir(build) or os.path.islink(build):
        return f"{build} is not a directory"
    stamp = build / STAMP
    if not os.path.isfile(stamp) or os.path.islink(stamp) or not os.path.isdir(lib_path(build)):
        return f"{build} has no stamp or no lib directory"
    reason = paths_problem(chain(build))
    if reason:
        return reason
    try:
        recorded = json.loads(stamp.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return f"{stamp} cannot be read"
    if recorded.get("requirements_sha256") != digest() or recorded.get("pins") != pins():
        return f"{stamp} names other requirements"
    if file_digests(lib_path(build)) != recorded.get("files"):
        return f"a file under {lib_path(build)} differs from its recorded SHA-256, or was added or removed"
    return None


# Building.

def fetch(name: str, version: str, sha256: str) -> bytes:
    """The pinned wheel, found on the package index by its SHA-256 and
    checked against it."""
    with urllib.request.urlopen(INDEX.format(name=name, version=version), timeout=60) as response:
        release = json.load(response)
    wheels = [entry for entry in release["urls"]
              if entry["packagetype"] == "bdist_wheel" and entry["digests"]["sha256"] == sha256]
    if len(wheels) != 1:
        raise SystemExit(f"markdown_env: no wheel of {name} {version} has the pinned SHA-256 {sha256}")
    with urllib.request.urlopen(wheels[0]["url"], timeout=120) as response:
        data = response.read()
    if hashlib.sha256(data).hexdigest() != sha256:
        raise SystemExit(f"markdown_env: the downloaded {name} {version} wheel does not match its pinned SHA-256")
    return data


def entry_problem(name: str) -> str | None:
    """Why a wheel entry name may not be unpacked, read under both POSIX and
    Windows rules, or None."""
    if not name or "\\" in name or ":" in name:
        return "has a backslash, a colon or no name"
    for flavour in (PurePosixPath, PureWindowsPath):
        path = flavour(name)
        if path.is_absolute() or path.drive or path.root or ".." in path.parts:
            return "is absolute, rooted, drive-qualified or climbs out"
    return None


def unpack(wheel: bytes, lib: Path) -> None:
    """Unpack a verified wheel into lib, every entry confined to it, files
    0644 and directories 0755, no links."""
    base = lib.resolve()
    seen: set[str] = set()
    with zipfile.ZipFile(io.BytesIO(wheel)) as archive:
        for info in archive.infolist():
            if entry_problem(info.filename):
                raise SystemExit(f"markdown_env: wheel entry {info.filename!r} {entry_problem(info.filename)}")
            if info.filename in seen:
                raise SystemExit(f"markdown_env: wheel entry {info.filename!r} appears twice")
            seen.add(info.filename)
            if stat.S_ISLNK(info.external_attr >> 16):
                raise SystemExit(f"markdown_env: wheel entry {info.filename!r} is a link")
            name = PurePosixPath(info.filename)
            target = lib.joinpath(*name.parts)
            resolved = target.resolve()
            if resolved != base and base not in resolved.parents:
                raise SystemExit(f"markdown_env: wheel entry {info.filename!r} resolves outside {lib}")
            if info.is_dir():
                target.mkdir(parents=True, exist_ok=True)
                continue
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(archive.read(info))
    for path in tree(lib):
        path.chmod(0o755 if path.is_dir() else 0o644)
    lib.chmod(0o755)


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


def build(target: Path) -> None:
    """Build in a private directory and move it into place. Never deletes:
    a failed or raced build stays in its own directory."""
    reason = paths_problem([p for p in root_chain() if os.path.lexists(p)])
    if reason:
        refuse(f"the managed root is not trusted: {reason}")
    target.parent.mkdir(parents=True, exist_ok=True)
    reason = paths_problem(root_chain())
    if reason:
        refuse(f"the managed root is not trusted: {reason}")
    staging = Path(tempfile.mkdtemp(prefix=f".{target.name}.build-", dir=target.parent))
    created = identity(staging)

    def still_ours(step: str) -> None:
        pause(step)
        if identity(staging) != created:
            refuse(f"{staging} is no longer the directory this helper created (before {step})")

    still_ours("unpack")
    lib = lib_path(staging)
    lib.mkdir(mode=0o755)
    for name, version, sha256 in requirements():
        unpack(fetch(name, version, sha256), lib)
    still_ours("stamp")
    (staging / STAMP).write_text(json.dumps(stamp_data(file_digests(lib)), sort_keys=True) + "\n",
                                 encoding="utf-8")
    (staging / STAMP).chmod(0o644)
    still_ours("rename")
    if os.path.lexists(target):
        refuse(f"{target} appeared while it was built; it is left as it is and the build stays at {staging}")
    try:
        os.rename(staging, target)
    except OSError as error:
        refuse(f"could not move the build at {staging} to {target} ({error})")


def isolated() -> bool:
    """Started as `python3 -I -S`: no PYTHON* variables, no user site, no
    site module, no script directory on sys.path."""
    return bool(sys.flags.isolated and sys.flags.no_site and sys.flags.ignore_environment)


def ensure() -> None:
    """Verify (building first if needed) the pinned parser and put it first
    on sys.path. Refuses unless the process started isolated."""
    if not isolated():
        refuse(f"the grader must start isolated, without site or the caller's Python environment: run `{START}`")
    if os.environ.get(RETIRED):
        refuse(f"{RETIRED} is set, but the managed root is fixed at {root()} and can no longer be moved; "
               f"unset {RETIRED}")
    target = build_path()
    reason = paths_problem([p for p in root_chain() if os.path.lexists(p)])
    if reason:
        refuse(f"the managed root is not trusted: {reason}")
    if not os.path.lexists(target):
        build(target)
    reason = build_problem(target)
    if reason:
        refuse(f"{target} is not a valid build of the pinned parser: {reason}; it is left as it is")
    # No bytecode is written beside the verified files, however the grader
    # was started: a .pyc there would make the next run refuse the build.
    sys.dont_write_bytecode = True
    sys.path.insert(0, str(lib_path(target)))


def versions() -> dict[str, str]:
    """The parser versions this process imports, read from their metadata."""
    import importlib.metadata as metadata
    return {name: metadata.version(name) for name in pins()}
