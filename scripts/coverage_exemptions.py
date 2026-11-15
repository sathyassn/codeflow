"""Validate the single changed-line coverage exemption list."""

import json
import re
from pathlib import PurePosixPath


class Problem(ValueError):
    """An input cannot establish changed-line coverage."""


def path_name(value, source_root=""):
    if not isinstance(value, str) or not value or "\x00" in value:
        raise Problem("coverage path must be a nonempty string without NUL")
    value = value.replace("\\", "/")
    root = source_root.replace("\\", "/").rstrip("/")
    if root and value.startswith(root + "/"):
        value = value[len(root) + 1:]
    if value.startswith("/") or re.match(r"^[A-Za-z]:", value) or ".." in value.split("/"):
        raise Problem(f"coverage path is outside the source root: {value!r}")
    result = str(PurePosixPath(value))
    if result == ".":
        raise Problem("coverage path is empty after normalization")
    return result


def exemptions(path):
    data = json.loads(path.read_text(encoding="utf-8"))
    if (not isinstance(data, dict) or set(data) != {"version", "exemptions"}
            or type(data["version"]) is not int or data["version"] != 1):
        raise Problem("exemption list requires version 1 and exemptions")
    if not isinstance(data["exemptions"], list):
        raise Problem("exemptions must be a list")
    entries = {}
    for entry in data["exemptions"]:
        if not isinstance(entry, dict) or set(entry) != {"path", "owner", "reason", "remove_when"}:
            raise Problem("each exemption requires path, owner, reason and remove_when")
        for field in ("path", "owner", "reason", "remove_when"):
            if not isinstance(entry[field], str) or not entry[field].strip():
                raise Problem(f"exemption {field} must be nonempty")
        name = path_name(entry["path"])
        if name != entry["path"] or name in entries:
            raise Problem(f"exemption path must be normalized and unique: {name!r}")
        entries[name] = entry
    return entries
