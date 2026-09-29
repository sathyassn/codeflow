#!/usr/bin/env python3
"""Run a gate target's command on the Node version its version file pins.

usage: python3 -B scripts/with-node.py <version-file> "<shell command>"

One `codeflow test --mode full` runs targets that pin different Node
versions (TSK-142 AC-1): `docs-portal` reads `docs-portal/.node-version` and
the present renderer reads `crates/codeflow-present/web/.node-version`. CI
installs each with `actions/setup-node`; locally one PATH cannot serve both.
This launcher finds the pinned version, puts its directory first on PATH and
runs the command through the shell, so `node`, `npm` and `npx` all come from
that install. When the version is not installed it fails, naming the version
and the file that pins it.

Where it looks, in order: the GitHub Actions tool cache (where setup-node
installs), asdf, nvm, then every `node` on PATH. Version-manager installs
come before PATH because they hold the official distribution, while a
`node` on PATH may be a rebuild against system libraries (Homebrew's Node
26 fails the present renderer's zlib pin). Each candidate is confirmed by
running it with `--version`, never trusted by its directory name.
"""

import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

VERSION = re.compile(r"^v?(\d+\.\d+\.\d+)$")
EXE = "node.exe" if os.name == "nt" else "node"


def pinned_version(version_file: Path) -> str:
    """The exact version a version file pins, or exit naming the problem."""
    try:
        text = version_file.read_text(encoding="utf-8").strip()
    except OSError as error:
        fail(f"cannot read the Node version file {version_file}: {error.strerror}")
    match = VERSION.match(text)
    if not match:
        fail(f"{version_file} must hold one exact Node version such as 24.18.0; "
             f"it holds {text!r}")
    return match.group(1)


def candidates(version: str) -> list[Path]:
    """Places the version may be installed, most trusted first."""
    home = Path.home()
    found: list[Path] = []
    tool_cache = os.environ.get("RUNNER_TOOL_CACHE")
    if tool_cache:
        for arch in sorted((Path(tool_cache) / "node" / version).glob("*")):
            found += [arch / "bin" / EXE, arch / EXE]
    asdf = Path(os.environ.get("ASDF_DATA_DIR") or home / ".asdf")
    found.append(asdf / "installs" / "nodejs" / version / "bin" / EXE)
    nvm = Path(os.environ.get("NVM_DIR") or home / ".nvm")
    found.append(nvm / "versions" / "node" / f"v{version}" / "bin" / EXE)
    for entry in os.environ.get("PATH", "").split(os.pathsep):
        if entry:
            found.append(Path(entry) / EXE)
    return found


def reports(node: Path, version: str) -> bool:
    """Whether `node` runs and reports exactly `version`."""
    if not (node.is_file() and os.access(node, os.X_OK)):
        return False
    try:
        run = subprocess.run([str(node), "--version"], capture_output=True,
                             text=True, timeout=30, check=False)
    except (OSError, subprocess.SubprocessError):
        return False
    return run.returncode == 0 and run.stdout.strip() == f"v{version}"


def find_node(version: str) -> Path | None:
    seen: set[Path] = set()
    for node in candidates(version):
        if node in seen:
            continue
        seen.add(node)
        if reports(node, version):
            return node
    return None


def fail(message: str) -> None:
    print(f"with-node: {message}", file=sys.stderr)
    sys.exit(1)


def run_inner(command: str, env: dict[str, str], *, windows: bool) -> int:
    """Run the chain with POSIX syntax even when the gate uses cmd.exe."""
    if windows:
        shell = shutil.which("bash")
        if shell is None:
            fail("bash is required to run the pinned Node target on Windows")
        return subprocess.run([shell, "-c", command], env=env, check=False).returncode
    shell = shutil.which("sh") or "/bin/sh"
    os.execve(shell, [shell, "-c", command], env)
    return 1  # unreachable: execve replaces this process or raises


def main(argv: list[str]) -> int:
    if len(argv) != 3:
        fail("usage: with-node.py <version-file> '<shell command>'")
    version_file, command = Path(argv[1]), argv[2]
    version = pinned_version(version_file)
    node = find_node(version)
    if node is None:
        fail(f"Node {version} is required by {version_file} and is not installed "
             f"(looked in the Actions tool cache, asdf, nvm and PATH). Install "
             f"Node {version}, for example `asdf install nodejs {version}`, "
             f"then run the gate again.")
    print(f"with-node: Node {version} from {node.parent}", file=sys.stderr)
    env = dict(os.environ)
    env["PATH"] = f"{node.parent}{os.pathsep}{env.get('PATH', '')}"
    sys.stdout.flush()
    sys.stderr.flush()
    return run_inner(command, env, windows=os.name == "nt")


if __name__ == "__main__":
    sys.exit(main(sys.argv))
