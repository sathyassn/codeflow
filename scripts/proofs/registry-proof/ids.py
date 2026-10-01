#!/usr/bin/env python3
"""Registry stand-in for the TSK-100 host proof (SPC-013).

This is proof tooling, not product code. It implements just enough of the
future `codeflow ids` to exercise the protocol on a real host:

  check  the registry check of R-109: history rule (R-8, root commit
         included), tip damage (R-9) and the uid binding of records (R-14).
  next   allocation from history (R-7); --tip-only shows the unsafe rule
         that history allocation replaces.
  bound  read back which uid the fetched registry binds to an id (R-13).

Typed restore (R-108) is out of scope: any commit that is not a pure
addition of new `ids/` files is reported.

The check is read only: it runs git plumbing on refs that are already
fetched and never writes a ref or contacts a remote.
"""

import argparse
import re
import subprocess
import sys
import tomllib

KINDS = ("EPC", "SPC", "TSK", "ADR")
REG_PATH = re.compile(r"^ids/(EPC|SPC|TSK|ADR)/(\d+(?:-\d+)?)\.toml$")
RECORD_PATH = re.compile(
    r"^project-management/(?:tasks|epics|specs)/((EPC|SPC|TSK)-(\d+(?:-\d+)?))\.md$"
)
REQUIRED_FIELDS = (
    "id", "uid", "kind", "title", "issuer", "created", "target",
    "introduced", "landed", "mapped", "mapped_by",
)
UID = re.compile(r"^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$")


def git(*args):
    proc = subprocess.run(["git", *args], capture_output=True, text=True)
    if proc.returncode != 0:
        sys.exit(f"error: git {' '.join(args)}: {proc.stderr.strip()}")
    return proc.stdout


def registry_history(registry):
    """Walk the registry oldest first; return (first additions, findings)."""
    first = {}  # path -> (commit sha, blob id)
    findings = []
    listing = git("rev-list", "--reverse", "--topo-order", "--parents", registry)
    for line in listing.splitlines():
        sha, *parents = line.split()
        short = sha[:12]
        if len(parents) > 1:
            findings.append(f"{short}: merge commit on the registry (R-8)")
            continue
        raw = git("diff-tree", "-r", "--root", "--no-renames", "--no-commit-id", sha)
        if not raw.strip():
            findings.append(f"{short}: commit changes nothing (R-8)")
        for entry in raw.splitlines():
            meta, path = entry.split("\t", 1)
            _old_mode, mode, _old_blob, blob_id, status = meta.lstrip(":").split()
            if status == "D":
                findings.append(f"{short}: deletes {path} (R-8)")
                continue
            if status != "A":
                findings.append(f"{short}: modifies {path} (status {status}) (R-8)")
                continue
            if not REG_PATH.match(path):
                findings.append(f"{short}: adds {path}, which is not an ids/ file (R-8, R-109)")
                continue
            if path in first:
                findings.append(f"{short}: re-adds {path}, first added in {first[path][0][:12]} (R-8)")
                continue
            if mode != "100644":
                findings.append(f"{short}: adds {path} with mode {mode} (R-8)")
            first[path] = (sha, blob_id)
            findings.extend(f"{short}: {p}" for p in validate_entry(path, blob_id))
    return first, findings


def validate_entry(path, blob_id):
    kind, number = REG_PATH.match(path).groups()
    try:
        data = tomllib.loads(git("cat-file", "blob", blob_id))
    except tomllib.TOMLDecodeError as err:
        return [f"{path} is not valid TOML: {err}"]
    problems = [f"{path} lacks field {f}" for f in REQUIRED_FIELDS if f not in data]
    if data.get("id") != f"{kind}-{number}":
        problems.append(f"{path} has id {data.get('id')!r}, expected {kind}-{number}")
    if data.get("kind") != kind:
        problems.append(f"{path} has kind {data.get('kind')!r}, expected {kind}")
    if not UID.match(str(data.get("uid", ""))):
        problems.append(f"{path} has a uid that is not a lower-case UUIDv4")
    return problems


def tip_damage(registry, first):
    tip = {}
    for entry in git("ls-tree", "-r", registry).splitlines():
        meta, path = entry.split("\t", 1)
        tip[path] = meta.split()[2]
    damage = []
    for path, (sha, blob_id) in sorted(first.items()):
        if path not in tip:
            damage.append(f"damaged: {path} (added in {sha[:12]}) is absent from the tip (R-9)")
        elif tip[path] != blob_id:
            damage.append(f"damaged: {path} differs from its first addition in {sha[:12]} (R-9)")
    return damage


def bound_uid(registry, record_id):
    kind, number = record_id.split("-", 1)
    path = f"ids/{kind}/{number}.toml"
    proc = subprocess.run(
        ["git", "show", f"{registry}:{path}"], capture_output=True, text=True
    )
    if proc.returncode != 0:
        return None
    return tomllib.loads(proc.stdout).get("uid")


def record_uid(ref, path):
    text = git("show", f"{ref}:{path}")
    parts = text.split("---", 2)
    if len(parts) < 3:
        return None
    for line in parts[1].splitlines():
        key, _, value = line.partition(":")
        if key.strip() == "uid":
            return value.split("#", 1)[0].strip().strip("\"'") or None
    return None


def binding_findings(registry, ref, paths):
    findings = []
    for path in paths:
        match = RECORD_PATH.match(path)
        if not match:
            continue
        record_id = match.group(1)
        uid = record_uid(ref, path)
        bound = bound_uid(registry, record_id)
        if uid is None:
            findings.append(f"{record_id}: record has no uid (R-2, R-14)")
        elif bound is None:
            findings.append(
                f"{record_id}: not reserved in the registry; a maintainer must admit uid {uid} (R-14, R-17)"
            )
        elif bound != uid:
            findings.append(f"{record_id}: registry binds uid {bound}, record has {uid}; retarget (R-14)")
        else:
            print(f"bound: {record_id} -> {uid}")
    return findings


def cmd_check(args):
    registry = args.registry
    tip = git("rev-parse", registry).strip()
    first, findings = registry_history(registry)
    count = len(git("rev-list", registry).split())
    print(f"registry: {registry} tip {tip[:12]}, {count} commits, {len(first)} ids ever added")
    findings += tip_damage(registry, first)
    if args.pr_base and args.pr_head:
        base = git("merge-base", args.pr_base, args.pr_head).strip()
        added = git("diff", "--name-only", "--diff-filter=A", base, args.pr_head,
                    "--", "project-management").split()
        print(f"mode: pull request, {len(added)} added paths under project-management/")
        findings += binding_findings(registry, args.pr_head, added)
    elif args.target:
        paths = git("ls-tree", "-r", "--name-only", args.target, "--", "project-management").split()
        print(f"mode: target {args.target}, {len(paths)} paths under project-management/")
        findings += binding_findings(registry, args.target, paths)
    else:
        print("mode: registry history only")
    for finding in findings:
        print(f"FINDING {finding}")
    if findings:
        print(f"registry check: FAIL ({len(findings)} findings)")
        return 1
    print("registry check: pass")
    return 0


def number_key(number):
    return tuple(int(p) for p in number.split("-"))


def cmd_next(args):
    kind = args.kind
    numbers = []
    if args.tip_only:
        listing = git("ls-tree", "-r", "--name-only", args.registry, "--", f"ids/{kind}/")
        source = "registry tip only (unsafe)"
    else:
        listing = git("log", "--format=", "--name-only", "--diff-filter=A",
                      "--no-renames", args.registry, "--", f"ids/{kind}/")
        source = "registry history"
    for path in listing.split():
        match = REG_PATH.match(path)
        if match and match.group(1) == kind:
            numbers.append(match.group(2))
    history_max = max(numbers, key=number_key) if numbers else "0"
    ref_max = "0"
    if not args.tip_only:
        refs = git("for-each-ref", "--format=%(refname)", "refs/heads", "refs/remotes").split()
        for ref in refs:
            if ref.endswith("/codeflow/registry"):
                continue
            for path in git("ls-tree", "-r", "--name-only", ref, "--", "project-management").split():
                match = RECORD_PATH.match(path)
                if match and match.group(2) == kind and number_key(match.group(3)) > number_key(ref_max):
                    ref_max = match.group(3)
    top = max(history_max, ref_max, key=number_key)
    nxt = number_key(top)[0] + 1
    width = 4 if kind == "ADR" else 3
    print(f"{nxt:0{width}d}")
    print(f"next {kind}: {source} max {history_max}, refs max {ref_max}", file=sys.stderr)
    return 0


def cmd_bound(args):
    uid = bound_uid(args.registry, args.id)
    print(uid if uid else "unbound")
    return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = parser.add_subparsers(dest="command", required=True)
    check = sub.add_parser("check")
    check.add_argument("--registry", required=True)
    check.add_argument("--target")
    check.add_argument("--pr-base")
    check.add_argument("--pr-head")
    check.set_defaults(func=cmd_check)
    nxt = sub.add_parser("next")
    nxt.add_argument("--registry", required=True)
    nxt.add_argument("--kind", required=True, choices=KINDS)
    nxt.add_argument("--tip-only", action="store_true")
    nxt.set_defaults(func=cmd_next)
    bound = sub.add_parser("bound")
    bound.add_argument("--registry", required=True)
    bound.add_argument("--id", required=True)
    bound.set_defaults(func=cmd_bound)
    args = parser.parse_args()
    return args.func(args)


if __name__ == "__main__":
    sys.exit(main())
