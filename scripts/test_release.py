#!/usr/bin/env python3
"""Behavior tests for CodeFlow's same-PR release state."""
from __future__ import annotations

import argparse
import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import tomllib
import unittest
from unittest import mock

SCRIPT = Path(__file__).with_name("release.py")
SPEC = importlib.util.spec_from_file_location("codeflow_release", SCRIPT)
assert SPEC and SPEC.loader
release = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = release
SPEC.loader.exec_module(release)


# The published source archive whose digest the fixture config records.
BOOTSTRAP_ARCHIVE = {"name": "source.tar.gz", "digest": "sha256:" + "a" * 64, "size": 1}


def command(root: Path, *args: str) -> str:
    result = subprocess.run(
        list(args), cwd=root, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=True
    )
    return result.stdout.strip()


class Repository:
    def __init__(self) -> None:
        self.temp = tempfile.TemporaryDirectory(prefix="codeflow-release-")
        self.root = Path(self.temp.name)
        command(self.root, "git", "init", "-q", "-b", "main")
        command(self.root, "git", "config", "user.name", "Release Test")
        command(self.root, "git", "config", "user.email", "release@example.invalid")
        self.version = "2.0.0"
        self.write_stamps(self.version)
        self.write("CHANGELOG.md", "# Changelog\n\n## [2.0.0] - 2026-01-01\n\n- public\n")
        self.write("docs/releasing.md", "policy\n")
        self.commit("chore: baseline")
        self.baseline = command(self.root, "git", "rev-parse", "HEAD")
        command(self.root, "git", "tag", "v2.0.0")
        self.config = self.root / ".release/config.json"
        self.host = self.root / "host.json"
        self.write_config()
        self.write_host()
        self.commit("chore: release config")
        self.target = command(self.root, "git", "rev-parse", "HEAD")

    def cleanup(self) -> None:
        self.temp.cleanup()

    def load(self) -> dict[str, object]:
        return release.load_config(self.config)

    def write(self, path: str, value: str) -> None:
        destination = self.root / path
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text(value, encoding="utf-8")

    def write_stamps(self, version: str) -> None:
        self.write("Cargo.toml", f'[workspace]\nmembers = []\n\n[workspace.package]\nversion = "{version}"\n')
        packages = "".join(
            f'[[package]]\nname = "{name}"\nversion = "{version}"\n'
            for name in ["codeflow-cli", "codeflow-core", "codeflow-present"]
        )
        self.write("Cargo.lock", f"version = 4\n\n{packages}")
        self.write(".codeflow/project.toml", f'scaffold_version = "{version}"\n')
        self.write(".codeflow/manifest.json", json.dumps({"scaffold_version": version}) + "\n")
        marker = f"<!-- codeflow:managed:begin scaffold={version} -->\n"
        self.write("AGENTS.md", marker)
        self.write("CLAUDE.md", marker)

    def write_config(self) -> None:
        value = {
            "schema_version": 2,
            "release_unit": "codeflow",
            "main_branch": "main",
            "bootstrap": {
                "comparison": {
                    "tag": "v2.0.0",
                    "commit": self.baseline,
                    "tree": command(self.root, "git", "rev-parse", "HEAD^{tree}"),
                },
                "published": {
                    "changelog_sha256": release.hashlib.sha256(
                        release.published_snapshot(
                            (self.root / "CHANGELOG.md").read_text(), "2.0.0"
                        ).encode()
                    ).hexdigest(),
                    "version": "2.0.0",
                    "source_commit": self.baseline,
                    "release_target_commit": self.baseline,
                    "source_archive_sha256": "a" * 64,
                },
            },
            "legacy_pending_group": {
                "version": "3.0.0",
                "impact": "major",
                "id": "pre-policy-v3",
            },
            "required_publication_checks": [
                "release state",
                "codeflow gates",
                "rust (format + test + clippy)",
                "windows (build + test + clippy)",
                "secret scan",
                "security review",
            ],
            "watched_contract_paths": ["docs/releasing.md", "assets/**"],
        }
        self.write(".release/config.json", json.dumps(value, indent=2) + "\n")

    def write_host(
        self,
        *,
        tags: dict[str, str] | None = None,
        releases: list[dict[str, object]] | None = None,
    ) -> None:
        bootstrap_release = {
            "tag": "v2.0.0",
            "draft": False,
            "prerelease": False,
            "target": self.baseline,
            "body": "historical bootstrap",
            "assets": [BOOTSTRAP_ARCHIVE],
        }
        self.write(
            "host.json",
            json.dumps(
                {
                    "schema_version": 1,
                    "drafts_visible": True,
                    "tags": tags or {"v2.0.0": self.baseline},
                    "releases": releases if releases is not None else [bootstrap_release],
                }
            ),
        )

    def commit(self, message: str) -> str:
        command(self.root, "git", "add", ".")
        command(self.root, "git", "commit", "-q", "-m", message)
        return command(self.root, "git", "rev-parse", "HEAD")

    def pending(self, version: str, entries: list[tuple[str, ...]]) -> None:
        """Pending entries as (impact, label) or (impact, label, body): each
        is written `- **Label.** body`, identified by its label (R-92)."""
        body = "\n".join(
            f"<!-- codeflow:release-impact {entry[0]} -->\n- **{entry[1]}.**"
            + (f" {entry[2]}" if len(entry) > 2 else "")
            + "\n"
            for entry in entries
        )
        self.write(
            "CHANGELOG.md",
            f"# Changelog\n\n## [{version}]\n\n{body}\n## [2.0.0] - 2026-01-01\n\n- public\n",
        )
        self.write_stamps(version)

    def args(self, **values: object) -> argparse.Namespace:
        defaults = {
            "root": self.root,
            "config": self.config,
            "host_state": self.host,
            "repository": "",
        }
        defaults.update(values)
        return argparse.Namespace(**defaults)

    @staticmethod
    def body(
        impact: str,
        *,
        breaking: str | None = None,
        contract: str | None = None,
        migration: str | None = "none",
        withdrawal: str = "",
    ) -> str:
        """A Release impact block. With neither Breaking nor the legacy
        Contract given, Breaking follows Impact; None omits a field."""
        if breaking is None and contract is None:
            breaking = "yes" if impact == "major" else "no"
        lines = [
            "## Release impact\n\n",
            "- Unit: codeflow\n",
            f"- Impact: {impact}\n",
            "- Rationale: reviewed fixture.\n",
            "- Evidence: CHANGELOG.md pending entry.\n",
        ]
        if breaking is not None:
            lines.append(f"- Breaking: {breaking}\n")
        if contract is not None:
            lines.append(f"- Contract: {contract}\n")
        if migration is not None:
            lines.append(f"- Migration: {migration}\n")
        lines.append(f"- Withdrawal: {withdrawal}\n")
        return "".join(lines)


class PendingVersionTests(unittest.TestCase):
    def setUp(self) -> None:
        self.repo = Repository()
        self.config = release.load_config(self.repo.config)
        self.baseline = release.Baseline("2.0.0", "v2.0.0", self.repo.baseline, self.repo.baseline)

    def tearDown(self) -> None:
        self.repo.cleanup()

    def expected(self, impacts: list[str]) -> str:
        entries = [(impact, f"{impact} change {index}") for index, impact in enumerate(impacts)]
        highest = max(impacts, key=release.IMPACT_ORDER.get)
        version = release.bump("2.0.0", highest)
        self.repo.pending(version, entries)
        actual, _, _ = release.expected_pending(
            (self.repo.root / "CHANGELOG.md").read_text(), self.baseline, self.config
        )
        return actual

    def test_cumulative_order_independence_and_all_impacts(self) -> None:
        self.assertEqual(self.expected(["patch", "patch"]), "2.0.1")
        self.assertEqual(self.expected(["patch", "minor"]), "2.1.0")
        self.assertEqual(self.expected(["minor", "patch"]), "2.1.0")
        self.assertEqual(self.expected(["major", "patch"]), "3.0.0")
        self.assertEqual(self.expected(["patch", "major"]), "3.0.0")

    def test_no_all_none_requires(self) -> None:
        text = "# Changelog\n\n## [2.0.0] - 2026-01-01\n\n- public\n"
        version, pending, impacts = release.expected_pending(text, self.baseline, self.config)
        self.assertEqual((version, pending, impacts), ("2.0.0", None, []))
        self.repo.pending("2.0.0", [("none", "internal")])
        with self.assertRaisesRegex(release.ReleaseError, "newest-first|none-only"):
            release.expected_pending(
                (self.repo.root / "CHANGELOG.md").read_text(), self.baseline, self.config
            )

    def test_markers_are_adjacent_and_legacy_group_is_bounded(self) -> None:
        backlog = "- backlog"
        digest = release.hashlib.sha256(backlog.encode()).hexdigest()
        self.config["legacy_pending_group"]["sha256"] = digest
        text = (
            "# Changelog\n\n## [3.0.0]\n\n"
            "<!-- codeflow:release-impact minor -->\n- **New guidance.**\n\n"
            f"<!-- codeflow:release-impact major legacy-group=pre-policy-v3 sha256={digest} -->\n"
            f"{backlog}\n<!-- codeflow:legacy-group-end -->\n\n"
            "## [2.0.0] - 2026-01-01\n\n- public\n"
        )
        self.assertEqual(release.expected_pending(text, self.baseline, self.config)[0], "3.0.0")
        with self.assertRaisesRegex(release.ReleaseError, "bounded"):
            release.expected_pending(text.replace("pre-policy-v3", "unbounded"), self.baseline, self.config)

    def test_undated_single_pending_section_only(self) -> None:
        self.repo.pending("2.0.1", [("patch", "fix")])
        text = (self.repo.root / "CHANGELOG.md").read_text().replace("## [2.0.1]", "## [2.0.1] - 2026-02-02")
        with self.assertRaisesRegex(release.ReleaseError, "undated"):
            release.expected_pending(text, self.baseline, self.config)
        text = text.replace("## [2.0.1] - 2026-02-02", "## [2.1.0]\n\n<!-- codeflow:release-impact minor -->\n- **Feature.**\n\n## [2.0.1]")
        with self.assertRaisesRegex(release.ReleaseError, "more than one"):
            release.expected_pending(text, self.baseline, self.config)

    def test_stamp_drift_is_rejected(self) -> None:
        self.repo.pending("2.0.1", [("patch", "fix")])
        self.repo.commit("fix: pending")
        files = {
            path: release.file_at_ref("HEAD", path, cwd=self.repo.root)
            for path in release.VERSION_STAMP_PATHS
        }
        files["Cargo.lock"] = files["Cargo.lock"].replace(b'2.0.1', b'9.9.9', 1)
        with self.assertRaisesRegex(release.ReleaseError, "version stamps"):
            release.validate_version_stamps(files)

    def test_sync_recovers_partial_stamp_write_through_build_and_update(self) -> None:
        self.repo.pending("2.0.1", [("patch", "recover partial update")])
        lock = self.repo.root / "Cargo.lock"
        lock.write_text(lock.read_text().replace('version = "2.0.1"', 'version = "9.9.9"', 1))
        log = self.repo.root / "cargo-calls.log"
        cargo = self.repo.root / "fake-cargo"
        self.repo.write(
            "fake-cargo",
            "#!/bin/sh\n"
            f"printf '%s\\n' \"$*\" >> {log}\n"
            "exit 0\n",
        )
        cargo.chmod(0o755)
        binary = self.repo.root / "target/debug/codeflow"
        binary.parent.mkdir(parents=True)
        self.repo.write(
            "target/debug/codeflow",
            "#!/usr/bin/env python3\n"
            "from pathlib import Path\n"
            "import sys\n"
            "if sys.argv[1:] == ['--version']:\n"
            "    print('codeflow 2.0.1')\n"
            "elif sys.argv[1:] == ['update']:\n"
            "    path = Path('Cargo.lock')\n"
            "    path.write_text(path.read_text().replace('9.9.9', '2.0.1'))\n"
            "else:\n"
            "    raise SystemExit(2)\n",
        )
        binary.chmod(0o755)
        release.sync(self.repo.args(cargo=str(cargo)))
        self.assertEqual(
            release.validate_version_stamps(
                {path: (self.repo.root / path).read_bytes() for path in release.VERSION_STAMP_PATHS}
            ),
            "2.0.1",
        )
        self.assertEqual(
            log.read_text().splitlines(),
            ["check --workspace", "build --locked -p codeflow-cli"],
        )

    def test_sync_retries_after_an_interrupted_build_without_committing_state(self) -> None:
        self.repo.pending("2.0.1", [("patch", "recover interrupted build")])
        lock = self.repo.root / "Cargo.lock"
        lock.write_text(lock.read_text().replace('version = "2.0.1"', 'version = "9.9.9"', 1))
        before_head = command(self.repo.root, "git", "rev-parse", "HEAD")
        before_changelog = (self.repo.root / "CHANGELOG.md").read_bytes()
        calls = self.repo.root / "cargo-call-count"
        cargo = self.repo.root / "fake-cargo"
        self.repo.write(
            "fake-cargo",
            "#!/bin/sh\n"
            f"count_file={calls}\n"
            "count=0\n"
            "test ! -f \"$count_file\" || count=$(sed -n '1p' \"$count_file\")\n"
            "count=$((count + 1))\n"
            "printf '%s\\n' \"$count\" > \"$count_file\"\n"
            "test \"$count\" -ne 2\n",
        )
        cargo.chmod(0o755)
        binary = self.repo.root / "target/debug/codeflow"
        binary.parent.mkdir(parents=True)
        self.repo.write(
            "target/debug/codeflow",
            "#!/usr/bin/env python3\n"
            "from pathlib import Path\n"
            "import sys\n"
            "if sys.argv[1:] == ['--version']:\n"
            "    print('codeflow 2.0.1')\n"
            "elif sys.argv[1:] == ['update']:\n"
            "    path = Path('Cargo.lock')\n"
            "    path.write_text(path.read_text().replace('9.9.9', '2.0.1'))\n"
            "else:\n"
            "    raise SystemExit(2)\n",
        )
        binary.chmod(0o755)
        with self.assertRaisesRegex(release.ReleaseError, "command failed"):
            release.sync(self.repo.args(cargo=str(cargo)))
        self.assertEqual(command(self.repo.root, "git", "rev-parse", "HEAD"), before_head)
        self.assertEqual((self.repo.root / "CHANGELOG.md").read_bytes(), before_changelog)
        self.assertNotEqual(len(set(release.version_stamp_values({
            path: (self.repo.root / path).read_bytes() for path in release.VERSION_STAMP_PATHS
        }).values())), 1)
        release.sync(self.repo.args(cargo=str(cargo)))
        self.assertEqual(
            release.validate_version_stamps(
                {path: (self.repo.root / path).read_bytes() for path in release.VERSION_STAMP_PATHS}
            ),
            "2.0.1",
        )

    def test_published_section_must_match_exact_public_source(self) -> None:
        self.repo.write(
            "CHANGELOG.md",
            "# Changelog\n\n## [2.0.0] - 2026-01-01\n\n- altered after publication\n",
        )
        head = self.repo.commit("docs: stale published edit")
        with self.assertRaisesRegex(release.ReleaseError, "historical bootstrap"):
            release.validate_release_tree(
                head,
                self.config,
                json.loads(self.repo.host.read_text()),
                cwd=self.repo.root,
            )

    def test_sync_validates_before_write_and_leaves_target_untouched(self) -> None:
        self.repo.write("CHANGELOG.md", "# Changelog\n\n## [Unreleased]\n\n- invalid\n")
        before = {path: (self.repo.root / path).read_bytes() for path in release.VERSION_STAMP_PATHS}
        with self.assertRaisesRegex(release.ReleaseError, "Unreleased"):
            release.sync(self.repo.args(cargo="missing-cargo"))
        after = {path: (self.repo.root / path).read_bytes() for path in release.VERSION_STAMP_PATHS}
        self.assertEqual(before, after)

    def test_sync_rejects_metadata_symlink_escape_without_touching_target(self) -> None:
        outside = self.repo.root.parent / f"{self.repo.root.name}-outside-changelog"
        outside.write_text("outside stays unchanged\n", encoding="utf-8")
        changelog = self.repo.root / "CHANGELOG.md"
        changelog.unlink()
        changelog.symlink_to(outside)
        try:
            with self.assertRaisesRegex(release.ReleaseError, "escapes repository"):
                release.sync(self.repo.args(cargo="missing-cargo"))
            self.assertEqual(outside.read_text(encoding="utf-8"), "outside stays unchanged\n")
        finally:
            outside.unlink()

    def test_sync_recomputes_stale_heading_before_stamp_work(self) -> None:
        self.repo.pending("2.0.1", [("major", "breaking change")])
        self.repo.write_stamps("3.0.0")
        release.sync(self.repo.args(cargo="missing-cargo"))
        self.assertIn("## [3.0.0]", (self.repo.root / "CHANGELOG.md").read_text())


class BaselineTests(unittest.TestCase):
    def setUp(self) -> None:
        self.repo = Repository()
        self.config = release.load_config(self.repo.config)

    def tearDown(self) -> None:
        self.repo.cleanup()

    def published(self, version: str, source: str, *, prerelease: bool = False) -> dict[str, object]:
        return {
            "tag": f"v{version}",
            "draft": False,
            "prerelease": prerelease,
            "target": source,
            "body": release.SOURCE_MARKER.format(source=source),
            "assets": [{"name": "codeflow.tar.xz", "digest": "sha256:" + "b" * 64, "size": 1}],
        }

    def state(self, tags: dict[str, str], releases: list[dict[str, object]]) -> dict[str, object]:
        bootstrap = {
            "tag": "v2.0.0",
            "draft": False,
            "prerelease": False,
            "target": self.repo.baseline,
            "body": "bootstrap",
            "assets": [BOOTSTRAP_ARCHIVE],
        }
        return {
            "schema_version": 1,
            "drafts_visible": True,
            "tags": {"v2.0.0": self.repo.baseline, **tags},
            "releases": [bootstrap, *releases],
        }

    def test_new_verified_public_release_automatically_becomes_baseline(self) -> None:
        source = "b" * 40
        state = self.state({"v3.0.0": source}, [self.published("3.0.0", source)])
        baseline = release.resolve_baseline(self.config, state, cwd=self.repo.root)
        self.assertEqual((baseline.version, baseline.source), ("3.0.0", source))
        text = "# Changelog\n\n## [3.0.1]\n\n<!-- codeflow:release-impact patch -->\n- **Later fix.**\n\n## [3.0.0]\n\n- public\n\n## [2.0.0] - 2026-01-01\n\n- old\n"
        self.assertEqual(release.expected_pending(text, baseline, self.config)[0], "3.0.1")

    def test_public_release_target_may_be_branch_when_tag_and_marker_are_exact(self) -> None:
        source = "b" * 40
        published = {**self.published("3.0.0", source), "target": "main"}
        state = self.state({"v3.0.0": source}, [published])
        baseline = release.resolve_baseline(self.config, state, cwd=self.repo.root)
        self.assertEqual((baseline.version, baseline.source), ("3.0.0", source))

    def test_stable_looking_prerelease_never_becomes_public_baseline(self) -> None:
        source = "b" * 40
        state = self.state({"v3.0.0": source}, [self.published("3.0.0", source, prerelease=True)])
        with self.assertRaisesRegex(release.ReleaseError, "without a verified public release"):
            release.resolve_baseline(self.config, state, cwd=self.repo.root)

    def test_tag_draft_bad_source_notes_or_assets_fail_closed(self) -> None:
        source = "b" * 40
        states = [
            self.state({"v3.0.0": source}, []),
            self.state({}, [{"tag": "v3.0.0", "draft": True, "target": source}]),
            self.state(
                {"v3.0.0": source},
                [{key: value for key, value in self.published("3.0.0", source).items()
                  if key != "prerelease"}],
            ),
            self.state({"v3.0.0": source}, [{**self.published("3.0.0", source), "body": ""}]),
            self.state({"v3.0.0": source}, [{**self.published("3.0.0", source), "assets": []}]),
        ]
        for state in states:
            with self.subTest(state=state), self.assertRaises(release.ReleaseError):
                release.resolve_baseline(self.config, state, cwd=self.repo.root)

    def test_public_only_inventory_does_not_claim_draft_absence(self) -> None:
        source = "b" * 40
        state = self.state({}, [{"tag": "v3.0.0", "draft": True, "target": source}])
        state["drafts_visible"] = False
        baseline = release.resolve_baseline(self.config, state, cwd=self.repo.root)
        self.assertEqual(baseline.version, "2.0.0")

    def test_write_visible_duplicate_drafts_are_ambiguous(self) -> None:
        source = "b" * 40
        draft = {"tag": "v3.0.0", "draft": True, "target": source, "assets": []}
        state = self.state({}, [draft, dict(draft)])
        with self.assertRaisesRegex(release.ReleaseError, "duplicate draft"):
            release.resolve_baseline(self.config, state, cwd=self.repo.root)

    def test_bootstrap_preserves_distinct_tag_and_published_source(self) -> None:
        baseline = release.resolve_baseline(
            self.config, json.loads(self.repo.host.read_text()), cwd=self.repo.root
        )
        self.assertEqual(baseline.comparison_commit, self.repo.baseline)
        self.assertEqual(baseline.source, self.repo.baseline)
        config = json.loads(self.repo.config.read_text())
        config["bootstrap"]["published"]["source_commit"] = "f" * 40
        baseline = release.resolve_baseline(config, json.loads(self.repo.host.read_text()), cwd=self.repo.root)
        self.assertNotEqual(baseline.source, baseline.comparison_commit)

    def test_bootstrap_survives_a_history_rewrite_that_keeps_the_tagged_tree(self) -> None:
        # A path filter rewrites every commit id but keeps the tagged content,
        # and a release recreated for the existing tag reports a branch target.
        root = self.repo.root
        rewritten = command(
            root, "git", "commit-tree", "HEAD~1^{tree}", "-m", "chore: baseline (filtered)"
        )
        command(root, "git", "tag", "-f", "v2.0.0", rewritten)
        state = self.state({"v2.0.0": rewritten}, [])
        state["releases"][0]["target"] = "main"
        baseline = release.resolve_baseline(self.config, state, cwd=root)
        self.assertEqual((baseline.version, baseline.comparison_commit), ("2.0.0", rewritten))

    def test_bootstrap_tag_moved_to_other_content_fails_closed(self) -> None:
        root = self.repo.root
        moved = command(root, "git", "rev-parse", "HEAD")
        command(root, "git", "tag", "-f", "v2.0.0", moved)
        state = self.state({"v2.0.0": moved}, [])
        with self.assertRaisesRegex(release.ReleaseError, "never move it"):
            release.resolve_baseline(self.config, state, cwd=root)

    def test_bootstrap_requires_host_tag_and_published_archive_identity(self) -> None:
        wrong_tag = self.state({}, [])
        wrong_tag["tags"]["v2.0.0"] = "f" * 40
        wrong_digest = self.state({}, [])
        wrong_digest["releases"][0]["assets"] = [
            {**BOOTSTRAP_ARCHIVE, "digest": "sha256:" + "f" * 64}
        ]
        no_archive = self.state({}, [])
        no_archive["releases"][0]["assets"] = [{**BOOTSTRAP_ARCHIVE, "name": "other.tar.gz"}]
        for state in [wrong_tag, wrong_digest, no_archive]:
            with self.subTest(state=state), self.assertRaisesRegex(
                release.ReleaseError, "live host state"
            ):
                release.resolve_baseline(self.config, state, cwd=self.repo.root)

    def test_bootstrap_without_a_tree_pin_is_malformed(self) -> None:
        config = json.loads(self.repo.config.read_text())
        for tree in [None, "d" * 39, "D" * 40]:
            config["bootstrap"]["comparison"]["tree"] = tree
            with self.subTest(tree=tree), self.assertRaisesRegex(release.ReleaseError, "malformed"):
                release.validate_bootstrap(config, cwd=self.repo.root)

    def test_missing_live_bootstrap_is_not_silently_trusted(self) -> None:
        with self.assertRaisesRegex(release.ReleaseError, "live host state"):
            release.resolve_baseline(
                self.config,
                {"schema_version": 1, "tags": {}, "releases": []},
                cwd=self.repo.root,
            )

    @mock.patch.object(release, "github_json")
    @mock.patch.object(release, "github_pages")
    def test_discovery_flattens_pages_and_only_peels_annotated_tags(
        self, pages: mock.Mock, one: mock.Mock
    ) -> None:
        direct = "a" * 40
        tag_object = "b" * 40
        peeled = "c" * 40
        pages.side_effect = [
            [{"tag_name": "v3.0.0", "draft": False, "prerelease": False,
              "target_commitish": direct, "body": "notes", "assets": []}],
            [
                {"ref": "refs/tags/v2.0.0", "object": {"type": "commit", "sha": direct}},
                {"ref": "refs/tags/v3.0.0", "object": {"type": "tag", "sha": tag_object}},
            ],
        ]
        one.return_value = {"object": {"type": "commit", "sha": peeled}}
        state = release.discover_host_state("owner/repo", cwd=self.repo.root)
        self.assertEqual(state["tags"], {"v2.0.0": direct, "v3.0.0": peeled})
        one.assert_called_once_with(
            f"repos/owner/repo/git/tags/{tag_object}", cwd=self.repo.root
        )
        self.assertEqual(pages.call_count, 2)


TEMPLATE = SCRIPT.parent.parent / ".github" / "pull_request_template.md"
UNRESOLVED_MIGRATION = '`none`, steps, or "see Breaking change"'


QUOTED_MIGRATION_PLACEHOLDERS = [
    '"none"', "'TODO'", '`" N/A "`', '"  TBD  "', "'-'", '""', "'   '",
]
SUBSTANTIVE_MIGRATIONS = [
    "Run the new command to convert saved records.",
    '"docs/migrate.md"',
    "Run `cat old.json | tool migrate` to convert saved records.",
]
BREAKING_CHANGE = "\n## Breaking change\n\nRun the new command to convert saved records.\n"


def filled_template(
    impact: str, breaking: str, migration: str = UNRESOLVED_MIGRATION
) -> str:
    """The repository's own PR template with its Release impact fields filled
    in the way an author would, leaving Migration as given."""
    values = {
        "Impact": impact,
        "Breaking": breaking,
        "Rationale": "reviewed fixture.",
        "Migration": migration,
        "Unit": "`codeflow`",
        "Evidence": "CHANGELOG.md pending entry.",
    }
    lines = []
    for line in TEMPLATE.read_text(encoding="utf-8").splitlines():
        for key, value in values.items():
            if line.startswith(f"- {key}:"):
                line = f"- {key}: {value}"
        lines.append(line)
    return "\n".join(lines) + "\n"


class ReleaseImpactFieldTests(unittest.TestCase):
    """Breaking replaces the legacy Contract field (operator direction,
    2026-09-24); both are accepted during the transition."""

    body = staticmethod(Repository.body)

    def parse(self, body: str) -> dict[str, str]:
        return release.parse_release_impact(body)

    def test_new_breaking_field_alone_is_accepted(self) -> None:
        fields = self.parse(self.body("minor", breaking="no"))
        self.assertEqual(("minor", "no"), (fields["impact"], fields["breaking"]))
        self.assertNotIn("contract", fields)

    def test_legacy_contract_alone_is_accepted_and_mapped(self) -> None:
        for contract, breaking, impact in [
            ("not-applicable", "no", "none"),
            ("compatible", "no", "minor"),
            ("breaking", "yes", "major"),
        ]:
            with self.subTest(contract=contract):
                fields = self.parse(
                    self.body(impact, contract=contract, migration="docs/migrate.md")
                )
                self.assertEqual(breaking, fields["breaking"])

    def test_agreeing_dual_fields_are_accepted(self) -> None:
        self.parse(self.body("minor", breaking="no", contract="compatible"))
        self.parse(self.body("none", breaking="no", contract="not-applicable"))
        self.parse(
            self.body("major", breaking="yes", contract="breaking", migration="docs/migrate.md")
        )

    def test_disagreeing_dual_fields_are_rejected(self) -> None:
        for breaking, contract, impact in [
            ("no", "breaking", "major"),
            ("yes", "compatible", "major"),
            ("yes", "not-applicable", "major"),
        ]:
            with self.subTest(breaking=breaking, contract=contract):
                with self.assertRaisesRegex(release.ReleaseError, "disagrees"):
                    self.parse(
                        self.body(
                            impact,
                            breaking=breaking,
                            contract=contract,
                            migration="docs/migrate.md",
                        )
                    )

    def test_major_with_breaking_no_is_rejected(self) -> None:
        with self.assertRaisesRegex(release.ReleaseError, "if and only if"):
            self.parse(self.body("major", breaking="no", migration="docs/migrate.md"))
        with self.assertRaisesRegex(release.ReleaseError, "if and only if"):
            self.parse(self.body("major", contract="compatible", migration="docs/migrate.md"))

    def test_breaking_yes_below_major_is_rejected(self) -> None:
        for impact in ["none", "patch", "minor"]:
            with self.subTest(impact=impact):
                with self.assertRaisesRegex(release.ReleaseError, "if and only if"):
                    self.parse(self.body(impact, breaking="yes", migration="docs/migrate.md"))

    def test_migration_is_required_and_substantive_when_breaking(self) -> None:
        with self.assertRaisesRegex(release.ReleaseError, "migration is required"):
            self.parse(self.body("minor", breaking="no", migration=None))
        for placeholder in ["none", "", "N/A"]:
            with self.subTest(migration=placeholder):
                with self.assertRaisesRegex(
                    release.ReleaseError, "migration guidance|migration is required"
                ):
                    self.parse(self.body("major", breaking="yes", migration=placeholder))
        reference = self.body("major", breaking="yes", migration="see Breaking change")
        with self.assertRaisesRegex(release.ReleaseError, "migration guidance"):
            self.parse(reference)
        with self.assertRaisesRegex(release.ReleaseError, "migration guidance"):
            self.parse(reference + "\n## Breaking change\n\n<!-- steps -->\n")
        self.parse(reference + BREAKING_CHANGE)

    def test_unresolved_migration_alternatives_are_rejected(self) -> None:
        for impact, breaking in [("minor", "no"), ("major", "yes")]:
            for value in [UNRESOLVED_MIGRATION, "none, steps, or see Breaking change", "steps"]:
                with self.subTest(impact=impact, migration=value):
                    with self.assertRaisesRegex(release.ReleaseError, "template alternatives"):
                        self.parse(self.body(impact, breaking=breaking, migration=value))

    def test_breaking_rejects_empty_and_placeholder_migration(self) -> None:
        for value in ["``", " NONE ", "n/a", "na", "TODO", "TBD", "-", "`none`", "<steps>"]:
            with self.subTest(migration=value):
                with self.assertRaisesRegex(
                    release.ReleaseError, "migration guidance|migration is required"
                ):
                    self.parse(self.body("major", breaking="yes", migration=value))

    def test_quoted_migration_placeholders_are_rejected(self) -> None:
        for value in QUOTED_MIGRATION_PLACEHOLDERS:
            with self.subTest(migration=value):
                with self.assertRaisesRegex(release.ReleaseError, "migration guidance"):
                    self.parse(self.body("major", breaking="yes", migration=value))

    def test_filled_template_parses_for_every_ordinary_level(self) -> None:
        for impact in ["none", "patch", "minor"]:
            with self.subTest(impact=impact):
                fields = release.parse_release_impact(filled_template(impact, "no", "none"))
                self.assertEqual(("no", "none"), (fields["breaking"], fields["migration"]))
        fields = release.parse_release_impact(
            filled_template("major", "yes", "see Breaking change") + BREAKING_CHANGE
        )
        self.assertEqual("yes", fields["breaking"])
        with self.assertRaisesRegex(release.ReleaseError, "template alternatives"):
            release.parse_release_impact(filled_template("major", "yes"))

    def test_missing_or_unresolved_breaking_is_rejected(self) -> None:
        with self.assertRaisesRegex(release.ReleaseError, "breaking is required"):
            self.parse(self.body("minor", breaking="no").replace("- Breaking: no\n", ""))
        with self.assertRaisesRegex(release.ReleaseError, "yes or no"):
            self.parse(self.body("minor", breaking="`yes | no`"))


class PullRequestTests(unittest.TestCase):
    def setUp(self) -> None:
        self.repo = Repository()

    def tearDown(self) -> None:
        self.repo.cleanup()

    def run_check(self, base: str, head: str, body: str, *, target: str | None = None) -> None:
        body_path = self.repo.root / "body.md"
        body_path.write_text(body)
        release.check_pr(
            self.repo.args(
                base=base,
                head=head,
                target_ref=target or base,
                body_file=body_path,
                body_env=None,
            )
        )

    def test_marker_floor_is_minimum_not_second_calculator(self) -> None:
        base = self.repo.target
        self.repo.pending("2.1.0", [("minor", "feature")])
        head = self.repo.commit("fix: implementation marker")
        self.run_check(base, head, self.repo.body("minor"))
        with self.assertRaisesRegex(release.ReleaseError, "below marker floor|must equal"):
            self.run_check(base, head, self.repo.body("none"))

    def legacy_pending_changelog(self) -> str:
        backlog = "- historical backlog"
        digest = release.hashlib.sha256(backlog.encode()).hexdigest()
        config = json.loads(self.repo.config.read_text())
        config["legacy_pending_group"]["sha256"] = digest
        self.repo.config.write_text(json.dumps(config))
        return (
            "# Changelog\n\n## [3.0.0]\n\n"
            "<!-- codeflow:release-impact minor -->\n- **Existing feature.**\n\n"
            f"<!-- codeflow:release-impact major legacy-group=pre-policy-v3 sha256={digest} -->\n"
            f"{backlog}\n<!-- codeflow:legacy-group-end -->\n\n"
            "## [2.0.0] - 2026-01-01\n\n- public\n\n"
            "[Unreleased]: https://example.invalid/compare/v2.0.0...HEAD\n"
        )

    def test_historical_unreleased_link_is_not_a_new_migration(self) -> None:
        self.repo.write("CHANGELOG.md", self.legacy_pending_changelog())
        self.repo.write_stamps("3.0.0")
        base = self.repo.commit("chore: existing migration")
        self.repo.write("planning.md", "Plan future work.\n")
        head = self.repo.commit("docs: record plan")
        self.run_check(base, head, self.repo.body("none"))

    def test_historical_link_does_not_hide_new_impact(self) -> None:
        changelog = self.legacy_pending_changelog()
        self.repo.write("CHANGELOG.md", changelog)
        self.repo.write_stamps("3.0.0")
        base = self.repo.commit("chore: existing migration")
        self.repo.write("CHANGELOG.md", changelog.replace(
            "## [3.0.0]\n", "## [3.0.0]\n\n<!-- codeflow:release-impact patch -->\n- **Repair.**\n", 1
        ))
        head = self.repo.commit("docs: describe repair")
        self.run_check(base, head, self.repo.body("patch"))
        with self.assertRaisesRegex(release.ReleaseError, "must equal the impact"):
            self.run_check(base, head, self.repo.body("none"))

    def test_declared_impact_must_equal_new_annotation(self) -> None:
        base = self.repo.target
        self.repo.pending("2.0.1", [("patch", "small fix")])
        head = self.repo.commit("feat: overstated declaration")
        with self.assertRaisesRegex(release.ReleaseError, "must equal the impact"):
            self.run_check(base, head, self.repo.body("minor"))

    def test_release_bearing_commit_cannot_reuse_existing_pending_label(self) -> None:
        self.repo.pending("3.0.0", [("major", "existing break")])
        base = self.repo.commit("feat!: existing pending break")
        changelog = self.repo.root / "CHANGELOG.md"
        changelog.write_text(changelog.read_text() + "\n", encoding="utf-8")
        head = self.repo.commit("feat!: unrelated release-bearing commit")
        with self.assertRaisesRegex(release.ReleaseError, "must equal the impact"):
            self.run_check(base, head, self.repo.body("major", migration="docs/migrate.md"))

    def test_new_major_entry_cannot_hide_behind_none_declaration(self) -> None:
        base = self.repo.target
        self.repo.pending("3.0.0", [("major", "hidden break")])
        head = self.repo.commit("docs: claim none")
        with self.assertRaisesRegex(release.ReleaseError, "must equal the impact"):
            self.run_check(base, head, self.repo.body("none"))

    def test_new_major_cannot_hide_behind_a_same_label_rewrite(self) -> None:
        self.repo.pending("3.0.0", [("major", "Existing break", "removes a flag")])
        base = self.repo.commit("feat!: existing break")
        self.repo.pending(
            "3.0.0",
            [("patch", "Existing break", "narrowed to a default"), ("major", "New hidden break")],
        )
        head = self.repo.commit("fix: mix reclassification and new break")
        with self.assertRaisesRegex(release.ReleaseError, "must equal the impact"):
            self.run_check(
                base,
                head,
                self.repo.body("patch", withdrawal="Existing break was narrowed."),
            )
        with self.assertRaisesRegex(release.ReleaseError, "migration guidance"):
            self.run_check(
                base,
                head,
                self.repo.body("major", withdrawal="Existing break was narrowed."),
            )

    def test_feat_then_exact_revert_can_be_net_none(self) -> None:
        base = self.repo.target
        self.repo.write("product.txt", "temporary\n")
        self.repo.commit("feat: temporary")
        command(self.repo.root, "git", "rm", "product.txt")
        head = self.repo.commit("revert: remove temporary")
        self.run_check(
            base,
            head,
            self.repo.body("none", contract="not-applicable").replace(
                "CHANGELOG.md pending entry.", "Net product diff is empty."
            ),
        )

    def test_major_needs_migration_and_watched_path_needs_contract(self) -> None:
        base = self.repo.target
        self.repo.pending("3.0.0", [("major", "break")])
        self.repo.write("docs/releasing.md", "changed\n")
        head = self.repo.commit("feat!: break")
        with self.assertRaisesRegex(release.ReleaseError, "migration"):
            self.run_check(base, head, self.repo.body("major", contract="breaking"))
        self.run_check(base, head, self.repo.body("major", contract="breaking", migration="docs/migrate.md"))

    def test_watched_path_needs_an_explicit_breaking_assessment(self) -> None:
        base = self.repo.target
        self.repo.pending("2.1.0", [("minor", "watched addition")])
        self.repo.write("docs/releasing.md", "changed\n")
        head = self.repo.commit("feat: watched addition")
        with self.assertRaisesRegex(release.ReleaseError, "watched contract"):
            self.run_check(base, head, self.repo.body("minor", contract="not-applicable"))
        self.run_check(base, head, self.repo.body("minor", breaking="no"))

    def test_filled_template_major_needs_a_chosen_migration(self) -> None:
        base = self.repo.target
        self.repo.pending("3.0.0", [("major", "replace old command")])
        head = self.repo.commit("feat!: replace old command")
        with self.assertRaisesRegex(release.ReleaseError, "template alternatives"):
            self.run_check(base, head, filled_template("major", "yes"))
        with self.assertRaisesRegex(release.ReleaseError, "migration is required"):
            self.run_check(base, head, filled_template("major", "yes", "``"))
        self.run_check(base, head, filled_template("major", "yes", "run the new command"))

    def test_declared_breaking_normalizes_quoted_migration_guidance(self) -> None:
        base = self.repo.target
        self.repo.pending("3.0.0", [("major", "replace old command")])
        head = self.repo.commit("feat!: replace old command")
        for value in QUOTED_MIGRATION_PLACEHOLDERS:
            with self.subTest(migration=value):
                with self.assertRaisesRegex(release.ReleaseError, "migration guidance"):
                    self.run_check(base, head, filled_template("major", "yes", value))
        for value in SUBSTANTIVE_MIGRATIONS:
            with self.subTest(migration=value):
                self.run_check(base, head, filled_template("major", "yes", value))

    def test_reconciled_major_normalizes_quoted_migration_guidance(self) -> None:
        self.repo.pending("3.0.0", [("major", "Replace old command", "now")])
        base = self.repo.commit("feat!: pending break")
        self.repo.pending("3.0.0", [("major", "Replace old command", "after migration")])
        head = self.repo.commit("docs: clarify breaking note")
        for value in QUOTED_MIGRATION_PLACEHOLDERS:
            with self.subTest(migration=value):
                with self.assertRaisesRegex(release.ReleaseError, "added major entry"):
                    self.run_check(base, head, filled_template("none", "no", value))
        for value in SUBSTANTIVE_MIGRATIONS:
            with self.subTest(migration=value):
                self.run_check(base, head, filled_template("none", "no", value))

    def test_filled_template_refinement_of_pending_major_keeps_migration(self) -> None:
        self.repo.pending("3.0.0", [("major", "Replace old command", "now")])
        base = self.repo.commit("feat!: pending break")
        self.repo.pending("3.0.0", [("major", "Replace old command", "after migration")])
        head = self.repo.commit("docs: clarify breaking note")
        with self.assertRaisesRegex(release.ReleaseError, "added major entry"):
            self.run_check(base, head, filled_template("none", "no", "none"))
        with self.assertRaisesRegex(release.ReleaseError, "template alternatives"):
            self.run_check(base, head, filled_template("none", "no"))
        self.run_check(base, head, filled_template("none", "no", "docs/migrate.md"))

    def test_additive_task_on_cumulative_major_pending_declares_minor(self) -> None:
        self.repo.pending("3.0.0", [("major", "earlier break")])
        base = self.repo.commit("feat!: earlier break")
        self.repo.pending("3.0.0", [("major", "earlier break"), ("minor", "new command")])
        head = self.repo.commit("feat: new command")
        self.run_check(base, head, self.repo.body("minor", breaking="no"))
        with self.assertRaisesRegex(release.ReleaseError, "if and only if"):
            self.run_check(base, head, self.repo.body("minor", breaking="yes"))

    def test_concurrent_clean_stale_merges_fail_in_both_orders(self) -> None:
        for work_name, target_name in [("alpha", "beta"), ("beta", "alpha")]:
            repo = Repository()
            try:
                base = repo.target
                command(repo.root, "git", "switch", "-q", "-c", work_name)
                repo.pending("2.0.1", [("patch", f"{work_name} fix")])
                head = repo.commit(f"fix: {work_name}")
                command(repo.root, "git", "switch", "-q", "main")
                repo.write(f"{target_name}.txt", "other work advanced main\n")
                target = repo.commit(f"chore: merge {target_name} first")
                merged = command(repo.root, "git", "merge-tree", "--write-tree", target, head)
                self.assertRegex(merged.splitlines()[0], r"^[0-9a-f]{40}$")
                body_path = repo.root / "body.md"
                body_path.write_text(repo.body("patch"))
                with self.subTest(first=target_name, second=work_name), self.assertRaisesRegex(
                    release.ReleaseError, "stale"
                ):
                    release.check_pr(
                        repo.args(
                            base=base,
                            head=head,
                            target_ref=target,
                            body_file=body_path,
                            body_env=None,
                        )
                    )
            finally:
                repo.cleanup()

    def test_withdrawal_requires_rationale_and_can_lower_remaining_state(self) -> None:
        self.repo.pending("3.0.0", [("major", "withdraw me"), ("patch", "keep me")])
        base = self.repo.commit("feat!: pending")
        self.repo.pending("2.0.1", [("patch", "keep me")])
        head = self.repo.commit("revert: withdraw breaking change")
        with self.assertRaisesRegex(release.ReleaseError, "withdrawal rationale"):
            self.run_check(base, head, self.repo.body("none"))
        self.run_check(base, head, self.repo.body("none", withdrawal="Breaking change was fully reverted."))

    def test_withdrawal_cannot_reuse_a_tagged_or_draft_version(self) -> None:
        self.repo.pending("3.0.0", [("major", "withdraw me")])
        base = self.repo.commit("feat!: pending")
        self.repo.write("CHANGELOG.md", "# Changelog\n\n## [2.0.0] - 2026-01-01\n\n- public\n")
        self.repo.write_stamps("2.0.0")
        head = self.repo.commit("revert: withdraw pending break")
        source = "b" * 40
        bootstrap = json.loads(self.repo.host.read_text())["releases"][0]
        states = [
            ({"v2.0.0": self.repo.baseline, "v3.0.0": source}, [bootstrap]),
            (
                {"v2.0.0": self.repo.baseline},
                [
                    bootstrap,
                    {"tag": "v3.0.0", "draft": True, "target": source, "assets": []},
                ],
            ),
        ]
        for tags, releases in states:
            self.repo.write_host(tags=tags, releases=releases)
            with self.subTest(tags=tags, releases=releases), self.assertRaisesRegex(
                release.ReleaseError, "verified public release|unresolved publication attempt"
            ):
                self.run_check(
                    base,
                    head,
                    self.repo.body("none", withdrawal="Breaking change was fully reverted."),
                )

    def test_prose_only_pending_note_edit_uses_existing_rationale(self) -> None:
        self.repo.pending("2.0.1", [("patch", "Fix a crash", "on start")])
        base = self.repo.commit("fix: pending behavior")
        self.repo.pending("2.0.1", [("patch", "Fix a crash", "when the cache is empty")])
        head = self.repo.commit("docs: clarify pending notes")
        self.run_check(
            base,
            head,
            self.repo.body("none", contract="not-applicable").replace(
                "reviewed fixture.", "Clarifies the existing pending note only."
            ),
        )

    def test_major_note_refinement_still_requires_migration_guidance(self) -> None:
        self.repo.pending("3.0.0", [("major", "Replace old command", "now")])
        base = self.repo.commit("feat!: pending break")
        self.repo.pending("3.0.0", [("major", "Replace old command", "after migration")])
        head = self.repo.commit("docs: clarify breaking note")
        with self.assertRaisesRegex(release.ReleaseError, "added major entry"):
            self.run_check(base, head, self.repo.body("none", contract="not-applicable"))
        self.run_check(
            base,
            head,
            self.repo.body("none", contract="not-applicable", migration="docs/migrate.md"),
        )

    def test_actual_unreleased_and_staged_section_can_adopt_new_model(self) -> None:
        old = (
            "# Changelog\n\n## [Unreleased]\n\n### Added\n\n- new guidance\n\n"
            "## [3.0.0] - 2026-02-02\n\n- legacy backlog\n\n"
            "## [2.0.0] - 2026-01-01\n\n- public\n"
        )
        self.repo.write("CHANGELOG.md", old)
        self.repo.write_stamps("3.0.0")
        base = self.repo.commit("chore: staged legacy state")
        legacy = "### Changed\n\n- legacy backlog"
        digest = release.hashlib.sha256(legacy.encode()).hexdigest()
        config = json.loads(self.repo.config.read_text())
        config["legacy_pending_group"]["sha256"] = digest
        self.repo.write(".release/config.json", json.dumps(config, indent=2) + "\n")
        new = (
            "# Changelog\n\n## [3.0.0]\n\n"
            "<!-- codeflow:release-impact minor -->\n- **New guidance.**\n\n"
            f"<!-- codeflow:release-impact major legacy-group=pre-policy-v3 sha256={digest} -->\n"
            f"{legacy}\n<!-- codeflow:legacy-group-end -->\n\n"
            "## [2.0.0] - 2026-01-01\n\n- public\n"
        )
        self.repo.write("CHANGELOG.md", new)
        head = self.repo.commit("feat: adopt same pr releases")
        self.run_check(base, head, self.repo.body("minor"))


class PublicationTests(unittest.TestCase):
    def setUp(self) -> None:
        self.repo = Repository()

    def tearDown(self) -> None:
        self.repo.cleanup()

    def test_actor_rerunner_ref_and_ordinary_pr_authority(self) -> None:
        event = self.repo.root / "event.json"
        users = self.repo.root / "users.json"
        permissions = self.repo.root / "permissions.json"
        pulls = self.repo.root / "pulls.json"
        event.write_text(json.dumps({"event_name": "workflow_dispatch", "ref": "refs/heads/main", "actor": "owner", "triggering_actor": "rerunner"}))
        users.write_text(json.dumps({"owner": "User", "rerunner": "User"}))
        permissions.write_text(json.dumps({"owner": "admin", "rerunner": "write"}))
        pulls.write_text(json.dumps([{"state": "closed", "merged_at": "now", "merge_commit_sha": self.repo.target, "base": {"ref": "main", "repo": {"full_name": "owner/repo"}}, "head": {"repo": {"full_name": "contributor/fork"}}, "merged_by": {"type": "User"}}]))
        args = self.repo.args(event=event, users=users, permissions=permissions, pulls=pulls, repository="owner/repo", source=self.repo.target)
        release.verify_authority(args)
        permissions.write_text(json.dumps({"owner": "admin", "rerunner": "read"}))
        with self.assertRaisesRegex(release.ReleaseError, "triggering_actor"):
            release.verify_authority(args)
        permissions.write_text(json.dumps({"owner": "admin", "rerunner": "write"}))
        event.write_text(json.dumps({"event_name": "workflow_dispatch", "ref": "refs/heads/feature", "actor": "owner", "triggering_actor": "rerunner"}))
        with self.assertRaisesRegex(release.ReleaseError, "on main"):
            release.verify_authority(args)
        event.write_text(json.dumps({"event_name": "workflow_dispatch", "ref": "refs/heads/main", "actor": "owner", "triggering_actor": "rerunner"}))
        pulls.write_text(json.dumps([{"state": "closed", "merged_at": "now", "merge_commit_sha": "f" * 40, "base": {"ref": "main", "repo": {"full_name": "owner/repo"}}, "head": {"repo": {"full_name": "owner/repo"}}, "merged_by": {"type": "User"}}]))
        with self.assertRaisesRegex(release.ReleaseError, "ordinary human-merged"):
            release.verify_authority(args)
        pulls.write_text(json.dumps([{"state": "closed", "merged_at": "now", "merge_commit_sha": self.repo.target, "base": {"ref": "main", "repo": {"full_name": "attacker/repo"}}, "head": {"repo": {"full_name": "contributor/fork"}}, "merged_by": {"type": "User"}}]))
        with self.assertRaisesRegex(release.ReleaseError, "ordinary human-merged"):
            release.verify_authority(args)

    def test_required_checks_are_successful_on_exact_source(self) -> None:
        state = self.repo.root / "checks.json"
        runs_state = self.repo.root / "runs.json"
        source = self.repo.target
        suite_id = 700
        good = [
            {
                "id": index,
                "name": name,
                "head_sha": source,
                "status": "completed",
                "conclusion": "success",
                "app": {"slug": "github-actions"},
                "check_suite": {"id": suite_id},
            }
            for index, name in enumerate(
                release.load_config(self.repo.config)["required_publication_checks"], 1
            )
        ]
        successful_push = {
            "id": 10,
            "run_number": 10,
            "run_attempt": 1,
            "check_suite_id": suite_id,
            "event": "push",
            "head_branch": "main",
            "head_sha": source,
            "path": ".github/workflows/codeflow-ci.yml",
            "status": "completed",
            "conclusion": "success",
        }
        state.write_text(json.dumps([{"check_runs": good}]))
        runs_state.write_text(json.dumps([{"workflow_runs": [successful_push]}]))
        args = self.repo.args(state=state, runs_state=runs_state, source=source)
        release.verify_checks(args)
        good[1]["head_sha"] = "f" * 40
        state.write_text(json.dumps([{"check_runs": good}]))
        with self.assertRaisesRegex(release.ReleaseError, "codeflow gates"):
            release.verify_checks(args)
        good[1]["head_sha"] = source
        good.append({**good[1], "id": 99, "conclusion": "failure"})
        state.write_text(json.dumps([{"check_runs": good}]))
        with self.assertRaisesRegex(release.ReleaseError, "latest trusted"):
            release.verify_checks(args)

    def test_pr_checks_cannot_replace_main_push_or_hide_a_newer_failure(self) -> None:
        state = self.repo.root / "checks.json"
        runs_state = self.repo.root / "runs.json"
        source = self.repo.target
        names = release.load_config(self.repo.config)["required_publication_checks"]
        checks = [
            {
                "id": index,
                "name": name,
                "head_sha": source,
                "status": "completed",
                "conclusion": "success",
                "app": {"slug": "github-actions"},
                "check_suite": {"id": 800},
            }
            for index, name in enumerate(names, 1)
        ]
        state.write_text(json.dumps([{"check_runs": checks}]))
        pr_run = {
            "id": 20,
            "run_number": 20,
            "run_attempt": 1,
            "check_suite_id": 800,
            "event": "pull_request",
            "head_branch": "main",
            "head_sha": source,
            "path": ".github/workflows/codeflow-ci.yml",
            "status": "completed",
            "conclusion": "success",
        }
        runs_state.write_text(json.dumps([{"workflow_runs": [pr_run]}]))
        args = self.repo.args(state=state, runs_state=runs_state, source=source)
        with self.assertRaisesRegex(release.ReleaseError, "main-push workflow run"):
            release.verify_checks(args)
        old_success = {
            **pr_run,
            "id": 21,
            "run_number": 21,
            "check_suite_id": 801,
            "event": "push",
        }
        new_failure = {
            **old_success,
            "id": 22,
            "run_number": 22,
            "check_suite_id": 802,
            "conclusion": "failure",
        }
        runs_state.write_text(json.dumps([{"workflow_runs": [old_success, new_failure]}]))
        with self.assertRaisesRegex(release.ReleaseError, "not successful"):
            release.verify_checks(args)

    def test_publication_rejects_a_selected_snapshot_after_main_changes(self) -> None:
        args = self.repo.args(
            source=self.repo.target,
            main_source="f" * 40,
            main_ref=None,
            tag="v2.0.0",
        )
        with self.assertRaisesRegex(release.ReleaseError, "no longer current main"):
            release.verify_publication(args)

    def test_publication_requires_write_visible_draft_inventory(self) -> None:
        state = json.loads(self.repo.host.read_text())
        state["drafts_visible"] = False
        self.repo.host.write_text(json.dumps(state))
        args = self.repo.args(
            source=self.repo.target,
            main_source=self.repo.target,
            main_ref=None,
            tag="v2.0.0",
        )
        with self.assertRaisesRegex(release.ReleaseError, "write-visible draft"):
            release.verify_publication(args)

    def test_wrong_source_tag_notes_assets_and_partial_publication_fail(self) -> None:
        source = "c" * 40
        notes = self.repo.root / "notes.md"
        state = self.repo.root / "state.json"
        notes.write_text("reviewed\n")
        base = {"draft": True, "assets": [], "tag_name": "v3.0.0", "name": "v3.0.0", "target_commitish": source, "body": "reviewed"}
        state.write_text(json.dumps({"tag_target": source, "release": base}))
        args = argparse.Namespace(state=state, source=source, tag="v3.0.0", notes=notes)
        release.verify_host_state(args)
        for changed in [
            {"tag_target": "d" * 40, "release": None},
            {"tag_target": source, "release": {**base, "body": "wrong"}},
            {"tag_target": source, "release": {**base, "assets": [{"name": "partial"}]}},
            {"tag_target": source, "release": {**base, "draft": False}},
        ]:
            state.write_text(json.dumps(changed))
            with self.subTest(changed=changed), self.assertRaises(release.ReleaseError):
                release.verify_host_state(args)

    def test_publication_attempt_uses_one_write_visible_paginated_inventory(self) -> None:
        source = "c" * 40
        notes = self.repo.root / "notes.md"
        notes.write_text("reviewed\n")
        state = self.repo.root / "state.json"
        draft = {
            "tag": "v3.0.0",
            "draft": True,
            "prerelease": False,
            "name": "v3.0.0",
            "target": source,
            "body": "reviewed",
            "assets": [],
        }
        inventory = {
            "schema_version": 1,
            "drafts_visible": True,
            "tags": {"v3.0.0": source},
            "releases": [draft],
        }
        state.write_text(json.dumps(inventory))
        args = argparse.Namespace(state=state, source=source, tag="v3.0.0", notes=notes)
        release.verify_host_state(args)
        inventory["releases"].append(dict(draft))
        state.write_text(json.dumps(inventory))
        with self.assertRaisesRegex(release.ReleaseError, "duplicate releases"):
            release.verify_host_state(args)

    def test_release_notes_strip_internal_markers_and_bind_source(self) -> None:
        self.repo.pending("2.0.1", [("patch", "reviewed fix")])
        head = self.repo.commit("fix: notes")
        notes = release.release_notes(self.repo.root, head, "v2.0.1", head)
        self.assertIn("reviewed fix", notes)
        self.assertNotIn("release-impact", notes)
        self.assertIn(release.SOURCE_MARKER.format(source=head), notes)

    def test_release_notes_strip_bounded_legacy_delimiters(self) -> None:
        self.repo.write(
            "CHANGELOG.md",
            "# Changelog\n\n## [3.0.0]\n\n"
            "<!-- codeflow:release-impact major legacy-group=pre-policy-v3 "
            f"sha256={'a' * 64} -->\n- reviewed backlog\n"
            f"{release.LEGACY_END}\n\n## [2.0.0] - 2026-01-01\n\n- public\n",
        )
        head = self.repo.commit("docs: bind legacy notes")
        notes = release.release_notes(self.repo.root, head, "v3.0.0", head)
        self.assertIn("reviewed backlog", notes)
        self.assertNotIn("legacy-group", notes)
        self.assertNotIn("legacy-group-end", notes)

    def notes_for(self, section: str) -> str:
        self.repo.write(
            "CHANGELOG.md",
            f"# Changelog\n\n## [3.0.0]\n\n{section}\n## [2.0.0] - 2026-01-01\n\n- public\n",
        )
        head = self.repo.commit("docs: notes fixture")
        return release.release_notes(self.repo.root, head, "v3.0.0", head)

    def test_release_notes_strip_one_line_and_wrapped_staging_notes(self) -> None:
        for note in [
            "_Staging evidence: staged on 2026-08-02._\n",
            "_Staging evidence: this section was first staged on 2026-08-02; that was not a\n"
            "publication date._\n",
            "_Staging evidence: first\nsecond line\nthird line._  \n",
        ]:
            with self.subTest(note=note):
                notes = self.notes_for(f"{note}\n### Added\n\n- kept entry\n")
                self.assertNotIn("Staging evidence", notes)
                self.assertNotIn("publication date", notes)
                self.assertTrue(notes.startswith("### Added\n\n- kept entry\n"))

    def test_release_notes_merge_repeated_headings_in_order(self) -> None:
        notes = self.notes_for(
            "Lead paragraph.\n\n### Added\n\n- a1\n\n### Changed\n\n- c1\n\n"
            "> quoted migration\n\n### Added\n\n- a2\n\n### Fixed\n\n- f1\n\n"
            "### Changed\n\n- c2\n"
        )
        body = notes.split("\n\n<!-- codeflow-release-source")[0]
        self.assertEqual(
            body,
            "Lead paragraph.\n\n### Added\n\n- a1\n\n- a2\n\n### Changed\n\n- c1\n\n"
            "> quoted migration\n\n- c2\n\n### Fixed\n\n- f1",
        )

    def test_release_notes_fail_closed_on_leftover_marker(self) -> None:
        for leftover in [
            "<!-- codeflow:release-impact huge -->\n- entry\n",
            "<!-- codeflow:legacy-group-begin -->\n- entry\n",
            "- entry\n\nSee _Staging evidence: inline_ for details.\n",
        ]:
            with self.subTest(leftover=leftover), self.assertRaisesRegex(
                release.ReleaseError, "internal marker or staging note"
            ):
                self.notes_for(leftover)

    def test_working_tree_changelog_renders_clean_3_0_0_notes(self) -> None:
        # Renders the real CHANGELOG.md this checkout carries, not a fixture.
        text = (Path(__file__).resolve().parent.parent / "CHANGELOG.md").read_text()
        self.repo.write("CHANGELOG.md", text)
        head = self.repo.commit("docs: real changelog")
        notes = release.release_notes(self.repo.root, head, "v3.0.0", head)
        self.assertNotIn("Staging evidence", notes)
        self.assertNotIn("staged on 2026-08-02", notes)
        self.assertNotRegex(notes, r"<!--\s*codeflow:")
        headings = [line for line in notes.splitlines() if line.startswith("### ")]
        self.assertTrue(headings)
        self.assertEqual(len(headings), len(set(headings)), headings)

    def test_published_assets_require_exact_same_run_bytes(self) -> None:
        source = "c" * 40
        artifacts = self.repo.root / "artifacts"
        artifacts.mkdir()
        artifact = artifacts / "codeflow.tar.xz"
        artifact.write_bytes(b"archive")
        state = self.repo.root / "published.json"
        release_data = {
            "draft": False,
            "tag_name": "v3.0.0",
            "target_commitish": source,
            "assets": [{"name": artifact.name, "size": artifact.stat().st_size, "state": "uploaded", "digest": "sha256:" + release.sha256(artifact)}],
        }
        state.write_text(json.dumps({"tag_target": source, "release": release_data}))
        args = argparse.Namespace(state=state, source=source, tag="v3.0.0", artifacts_dir=artifacts)
        release.verify_published_assets(args)
        artifact.write_bytes(b"different")
        with self.assertRaisesRegex(release.ReleaseError, "same-run"):
            release.verify_published_assets(args)

    def test_command_surface_has_no_candidate_or_finalizer(self) -> None:
        commands = {release.parser().parse_args(values).command for values in [
            ["sync"],
            ["check-state"],
            ["check-pr", "--base", "a", "--head", "b", "--target-ref", "c", "--body-env", "BODY"],
            ["verify-publication", "--source", "a", "--main-source", "b" * 40, "--tag", "v1.0.0"],
        ]}
        self.assertEqual(commands, {"sync", "check-state", "check-pr", "verify-publication"})
        for removed in ["prepare", "finalize", "guard-refresh", "authorize-event"]:
            with self.assertRaises(SystemExit), contextlib.redirect_stderr(io.StringIO()):
                release.parser().parse_args([removed])


REPOSITORY = SCRIPT.parent.parent
IMPACT_CASES = REPOSITORY / "scripts" / "fixtures" / "release_impact_cases.json"


class SharedReleaseImpactFixtureTests(unittest.TestCase):
    """TSK-106 AC-8: release.py passes the fixture set the Rust PR-body check
    (`pr_body::tests::release_impact_block_passes_the_shared_fixture_set`)
    passes too."""

    def test_release_impact_block_passes_the_shared_fixture_set(self) -> None:
        cases = json.loads(IMPACT_CASES.read_text(encoding="utf-8"))["cases"]
        self.assertGreaterEqual(len(cases), 20)
        self.assertEqual(len(cases), len({case["name"] for case in cases}))
        self.assertTrue(any(case["valid"] for case in cases))
        self.assertTrue(any(not case["valid"] for case in cases))
        for case in cases:
            with self.subTest(case=case["name"]):
                try:
                    release.parse_release_impact(case["body"])
                    valid = True
                except release.ReleaseError:
                    valid = False
                self.assertEqual(case["valid"], valid)


class EntryIdentityTests(unittest.TestCase):
    """TSK-106 AC-3 (R-92): a pending entry is its bold label."""

    def setUp(self) -> None:
        self.repo = Repository()
        self.config = release.load_config(self.repo.config)
        self.baseline = release.Baseline("2.0.0", "v2.0.0", self.repo.baseline, self.repo.baseline)

    def tearDown(self) -> None:
        self.repo.cleanup()

    def pending_text(self, entries: list[tuple[str, ...]]) -> str:
        self.repo.pending("2.1.0", entries)
        return (self.repo.root / "CHANGELOG.md").read_text()

    def test_labels_are_unique_among_pending_entries(self) -> None:
        text = self.pending_text([("minor", "Add a flag", "one"), ("patch", "add a FLAG", "two")])
        with self.assertRaisesRegex(release.ReleaseError, "not unique"):
            release.expected_pending(text, self.baseline, self.config)
        text = self.pending_text([("minor", "Add a flag", "one"), ("patch", "Fix a flag", "two")])
        self.assertEqual(release.expected_pending(text, self.baseline, self.config)[0], "2.1.0")

    def test_an_entry_without_a_bold_label_blocks(self) -> None:
        text = self.pending_text([("minor", "Add a flag")]).replace("- **Add a flag.**", "- add a flag")
        with self.assertRaisesRegex(release.ReleaseError, "needs a bold label"):
            release.expected_pending(text, self.baseline, self.config)

    def test_the_legacy_group_keeps_an_explicit_identity(self) -> None:
        backlog = "- **Add a flag.** historical"
        digest = release.hashlib.sha256(backlog.encode()).hexdigest()
        self.config["legacy_pending_group"]["sha256"] = digest
        text = (
            "# Changelog\n\n## [3.0.0]\n\n"
            "<!-- codeflow:release-impact minor -->\n- **Add a flag.** new\n\n"
            f"<!-- codeflow:release-impact major legacy-group=pre-policy-v3 sha256={digest} -->\n"
            f"{backlog}\n<!-- codeflow:legacy-group-end -->\n\n"
            "## [2.0.0] - 2026-01-01\n\n- public\n"
        )
        section = release.changelog_sections(text)[0]
        items = release.pending_items(section, self.config)
        self.assertEqual(sorted(items), ["add a flag", "legacy:pre-policy-v3"])
        self.assertEqual(items["legacy:pre-policy-v3"].impact, "major")

    def test_an_entry_is_one_bullet_not_the_heading_after_it(self) -> None:
        self.assertEqual(
            release.entry_extent("- **A.** one\n  two\n\n  three\n\n### Fixed\n\nmore"),
            "- **A.** one\n  two\n\n  three",
        )


class EntryEditTests(unittest.TestCase):
    """TSK-106 AC-3: a changed body or impact under an existing label is an
    edit whose net change is assessed, never wording by default."""

    def setUp(self) -> None:
        self.repo = Repository()

    def tearDown(self) -> None:
        self.repo.cleanup()

    def check(self, base: str, head: str, body: str) -> dict[str, object]:
        body_path = self.repo.root / ".git" / "body.md"
        body_path.write_text(body)
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            release.check_pr(
                self.repo.args(base=base, head=head, target_ref=base, body_file=body_path, body_env=None)
            )
        return json.loads(output.getvalue())

    def test_a_body_edit_with_code_is_assessed_at_the_entry_impact(self) -> None:
        self.repo.pending("2.1.0", [("minor", "Add a flag", "to the command")])
        base = self.repo.commit("feat: add a flag")
        self.repo.pending("2.1.0", [("minor", "Add a flag", "to the command and its alias")])
        self.repo.write("product.txt", "alias\n")
        head = self.repo.commit("chore: wire the alias")
        with self.assertRaisesRegex(release.ReleaseError, "never as wording"):
            self.check(base, head, self.repo.body("none"))
        result = self.check(base, head, self.repo.body("minor"))
        self.assertEqual((result["added"], result["edited"]), ([], ["Add a flag."]))

    def test_a_wording_edit_is_none_only_when_declared_and_docs_only(self) -> None:
        self.repo.pending("2.1.0", [("minor", "Add a flag", "to the command")])
        base = self.repo.commit("feat: add a flag")
        self.repo.pending("2.1.0", [("minor", "Add a flag", "to the `run` command")])
        head = self.repo.commit("docs: name the command")
        self.assertEqual(self.check(base, head, self.repo.body("none"))["edited"], ["Add a flag."])

    def test_raising_an_entry_impact_is_assessed_at_the_new_impact(self) -> None:
        self.repo.pending("2.0.1", [("patch", "Fix a flag", "parsing")])
        base = self.repo.commit("fix: flag parsing")
        self.repo.pending("2.1.0", [("minor", "Fix a flag", "parsing, and accept a new form")])
        head = self.repo.commit("docs: reclassify")
        with self.assertRaisesRegex(release.ReleaseError, r"\(minor\)"):
            self.check(base, head, self.repo.body("none"))
        self.check(base, head, self.repo.body("minor"))

    def test_lowering_an_entry_impact_needs_a_withdrawal(self) -> None:
        self.repo.pending("2.1.0", [("minor", "Add a flag", "and a mode")])
        base = self.repo.commit("feat: add a flag")
        self.repo.pending("2.0.1", [("patch", "Add a flag", "only")])
        head = self.repo.commit("docs: narrow the entry")
        with self.assertRaisesRegex(release.ReleaseError, "withdrawal rationale"):
            self.check(base, head, self.repo.body("patch"))
        self.check(base, head, self.repo.body("patch", withdrawal="The mode was dropped before release."))

    def test_a_relabel_is_a_withdrawal_and_an_addition(self) -> None:
        self.repo.pending("2.1.0", [("minor", "Add a flag")])
        base = self.repo.commit("feat: add a flag")
        self.repo.pending("2.1.0", [("minor", "Add an option")])
        head = self.repo.commit("docs: rename the entry")
        with self.assertRaisesRegex(release.ReleaseError, "withdrawal rationale"):
            self.check(base, head, self.repo.body("minor"))
        result = self.check(base, head, self.repo.body("minor", withdrawal="Renamed; same content."))
        self.assertEqual((result["added"], result["withdrawn"]), (["Add an option."], ["Add a flag."]))


class TypedRepairTests(unittest.TestCase):
    """TSK-106 AC-6 (R-95): a PR repairs a base that fails its own release
    state only by changing the changelog and the coupled version stamps."""

    def setUp(self) -> None:
        self.repo = Repository()
        # The base is broken: a patch entry landed without its stamps.
        self.repo.pending("2.0.1", [("patch", "Fix a crash")])
        self.repo.write_stamps("2.0.0")
        self.base = self.repo.commit("fix: a crash")

    def tearDown(self) -> None:
        self.repo.cleanup()

    def check(self, head: str, body: str | None = None) -> dict[str, object]:
        body_path = self.repo.root / ".git" / "body.md"
        body_path.write_text(body or self.repo.body("none"))
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            release.check_pr(
                self.repo.args(
                    base=self.base, head=head, target_ref=self.base, body_file=body_path, body_env=None
                )
            )
        return json.loads(output.getvalue())

    def test_a_broken_base_blocks_ordinary_work(self) -> None:
        self.repo.write("product.txt", "work\n")
        head = self.repo.commit("chore: unrelated work")
        with self.assertRaisesRegex(release.ReleaseError, "a repair changes only CHANGELOG.md"):
            self.check(head)

    def test_a_stamp_repair_is_accepted_and_names_the_invariant(self) -> None:
        self.repo.write_stamps("2.0.1")
        head = self.repo.commit("chore(release): sync stamps")
        result = self.check(head)
        self.assertEqual(result["repair"], "coupled stamps 2.0.0 disagree with pending target 2.0.1")
        self.assertEqual(result["version"], "2.0.1")

    def test_a_changelog_repair_is_accepted(self) -> None:
        self.repo.pending("2.0.0", [])
        self.repo.write("CHANGELOG.md", "# Changelog\n\n## [2.0.0] - 2026-01-01\n\n- public\n")
        self.repo.write_stamps("2.0.0")
        head = self.repo.commit("docs(changelog): withdraw the entry")
        with self.assertRaisesRegex(release.ReleaseError, "withdrawal rationale"):
            self.check(head)
        result = self.check(head, self.repo.body("none", withdrawal="The fix was reverted."))
        self.assertIn("disagree", str(result["repair"]))

    def test_a_repair_that_changes_more_than_a_stamp_is_refused(self) -> None:
        self.repo.write_stamps("2.0.1")
        cargo = self.repo.root / "Cargo.toml"
        cargo.write_text(cargo.read_text() + '\n[workspace.dependencies]\nserde = "1"\n')
        head = self.repo.commit("chore(release): sync stamps")
        with self.assertRaisesRegex(release.ReleaseError, "version stamp of Cargo.toml"):
            self.check(head)

    def test_a_repair_cannot_change_the_release_configuration(self) -> None:
        self.repo.write_stamps("2.0.1")
        config = json.loads(self.repo.config.read_text())
        config["watched_contract_paths"] = []
        self.repo.write(".release/config.json", json.dumps(config, indent=2) + "\n")
        head = self.repo.commit("chore(release): sync stamps")
        with self.assertRaisesRegex(release.ReleaseError, "cannot change .release/config.json"):
            self.check(head)

    def test_the_repair_is_judged_with_the_base_configuration(self) -> None:
        # The working-tree configuration a CI checkout reads comes from the
        # head; a repair never runs on it.
        self.repo.write_stamps("2.0.1")
        head = self.repo.commit("chore(release): sync stamps")
        config = json.loads(self.repo.config.read_text())
        config["bootstrap"]["published"]["changelog_sha256"] = "0" * 64
        self.repo.config.write_text(json.dumps(config))
        result = self.check(head)
        self.assertEqual(result["repair"], "coupled stamps 2.0.0 disagree with pending target 2.0.1")
        with self.assertRaisesRegex(release.ReleaseError, "head's release configuration differs"):
            release.typed_repair(
                self.base, head, head, ["Cargo.toml"], config, "x", cwd=self.repo.root
            )

    def test_the_proposed_merge_must_pass(self) -> None:
        self.repo.write_stamps("2.0.2")
        head = self.repo.commit("chore(release): wrong stamps")
        with self.assertRaisesRegex(release.ReleaseError, "coupled stamps 2.0.2 disagree"):
            self.check(head)

    def test_frozen_sections_stay_frozen(self) -> None:
        self.repo.write_stamps("2.0.1")
        changelog = self.repo.root / "CHANGELOG.md"
        changelog.write_text(changelog.read_text().replace("- public", "- public, amended"))
        head = self.repo.commit("chore(release): sync stamps")
        with self.assertRaisesRegex(release.ReleaseError, "published changelog sections differ"):
            self.check(head)

    def test_version_non_reuse_holds(self) -> None:
        self.repo.write_host(tags={"v2.0.0": self.repo.baseline, "v2.0.1": "b" * 40})
        self.repo.write_stamps("2.0.1")
        head = self.repo.commit("chore(release): sync stamps")
        with self.assertRaisesRegex(release.ReleaseError, "without a verified public release"):
            self.check(head)

    def test_impact_floors_hold(self) -> None:
        self.repo.pending("2.1.0", [("patch", "Fix a crash"), ("minor", "Add a flag")])
        head = self.repo.commit("feat(release): repair and add")
        with self.assertRaisesRegex(release.ReleaseError, "must equal the impact"):
            self.check(head)
        with self.assertRaisesRegex(release.ReleaseError, "below marker floor"):
            self.check(head, self.repo.body("patch"))
        self.assertIn("repair", self.check(head, self.repo.body("minor")))

    def test_an_entry_moved_out_of_the_legacy_group_is_a_repair(self) -> None:
        backlog = "### Fixed\n\n- **Old fix.** kept"
        digest = release.hashlib.sha256(backlog.encode()).hexdigest()
        config = json.loads(self.repo.config.read_text())
        config["legacy_pending_group"]["sha256"] = digest
        self.repo.write(".release/config.json", json.dumps(config, indent=2) + "\n")
        entry = "<!-- codeflow:release-impact patch -->\n- **New fix.** landed in the group\n\n"
        broken = (
            "# Changelog\n\n## [3.0.0]\n\n"
            f"<!-- codeflow:release-impact major legacy-group=pre-policy-v3 sha256={digest} -->\n"
            f"### Fixed\n\n{entry}- **Old fix.** kept\n<!-- codeflow:legacy-group-end -->\n\n"
            "## [2.0.0] - 2026-01-01\n\n- public\n"
        )
        self.repo.write("CHANGELOG.md", broken)
        self.repo.write_stamps("3.0.0")
        self.base = self.repo.commit("fix: the new fix")
        repaired = (
            "# Changelog\n\n## [3.0.0]\n\n### Fixed\n\n" + entry +
            f"<!-- codeflow:release-impact major legacy-group=pre-policy-v3 sha256={digest} -->\n"
            f"{backlog}\n<!-- codeflow:legacy-group-end -->\n\n"
            "## [2.0.0] - 2026-01-01\n\n- public\n"
        )
        self.repo.write("CHANGELOG.md", repaired)
        head = self.repo.commit("docs(changelog): move the entry out of the group")
        result = self.check(head)
        self.assertEqual(result["repair"], "legacy marker is not the bounded bootstrap group")
        self.assertEqual((result["added"], result["edited"]), ([], []))

    def test_manifest_hashes_must_be_the_managed_baselines(self) -> None:
        for version, commit in [("2.0.0", True), ("2.0.1", False)]:
            marker = f"<!-- codeflow:managed:begin scaffold={version} -->\nrules\n"
            self.repo.write(".codeflow/.baseline/AGENTS.md", marker)
            digest = release.hashlib.sha256(marker.encode()).hexdigest()
            self.repo.write(
                ".codeflow/manifest.json",
                json.dumps({"scaffold_version": version, "files": {"AGENTS.md": {"sha256": digest}}}) + "\n",
            )
            if commit:
                self.base = self.repo.commit("chore: record the baseline")
        self.repo.write_stamps("2.0.1")
        self.repo.write(
            ".codeflow/manifest.json",
            json.dumps({"scaffold_version": "2.0.1", "files": {"AGENTS.md": {"sha256": digest}}}) + "\n",
        )
        good = self.repo.commit("chore(release): sync stamps")
        self.assertIn("repair", self.check(good))
        self.repo.write(
            ".codeflow/manifest.json",
            json.dumps({"scaffold_version": "2.0.1", "files": {"AGENTS.md": {"sha256": "0" * 64}}}) + "\n",
        )
        bad = self.repo.commit("chore(release): forge a hash")
        with self.assertRaisesRegex(release.ReleaseError, "managed baseline"):
            self.check(bad)


class PreflightTests(unittest.TestCase):
    """TSK-106 AC-4 (R-93): the local release checks pre-push runs."""

    def setUp(self) -> None:
        self.repo = Repository()
        command(self.repo.root, "git", "switch", "-q", "-c", "feat/work")

    def tearDown(self) -> None:
        self.repo.cleanup()

    def preflight(self, *, body: str | None = None, **values: object) -> tuple[dict[str, object], int]:
        body_path = None
        if body is not None:
            body_path = self.repo.root / ".git" / "draft.md"
            body_path.write_text(body)
        args = self.repo.args(
            head="HEAD", branch="feat/work", remote="origin", target="", base="", body_file=body_path
        )
        for key, value in values.items():
            setattr(args, key, value)
        output = io.StringIO()
        with contextlib.redirect_stdout(output), mock.patch.dict(os.environ, {"CODEFLOW_PR_DRAFT": ""}):
            release.preflight(args)
        return json.loads(output.getvalue()), getattr(args, "exit_code", 0)

    def test_a_behaviour_change_without_an_entry_warns_and_never_blocks(self) -> None:
        self.repo.write("src/tool.rs", "fn main() {}\n")
        self.repo.commit("wip: start the tool")
        result, code = self.preflight()
        self.assertEqual((result["status"], code), ("warn", 0))
        self.assertIn("src/tool.rs", " ".join(result["notes"]))
        self.assertIn("not checked against the host", result["notes"][0])
        self.assertEqual(result["host"], "not checked against the host")

    def test_an_entry_or_a_none_intent_silences_the_warning(self) -> None:
        self.repo.write("src/tool.rs", "fn main() {}\n")
        self.repo.commit("chore: internal tool")
        result, _ = self.preflight(body=self.repo.body("none"))
        self.assertEqual(result["status"], "ok")
        self.repo.pending("2.0.1", [("patch", "Fix the tool")])
        self.repo.commit("fix: the tool")
        self.assertEqual(self.preflight()[0]["status"], "ok")

    def test_documentation_and_records_are_not_behaviour(self) -> None:
        self.repo.write("docs/guide.md", "guide\n")
        self.repo.write("project-management/tasks/TSK-001.md", "record\n")
        self.repo.commit("docs: guide")
        self.assertEqual(self.preflight()[0]["status"], "ok")

    def test_a_push_that_breaks_a_valid_tree_blocks(self) -> None:
        self.repo.pending("2.0.1", [("patch", "Fix the tool")])
        self.repo.write_stamps("2.0.0")
        self.repo.commit("fix: the tool")
        result, code = self.preflight()
        self.assertEqual((result["status"], code), ("blocked", 1))
        self.assertIn("breaks the release tree", " ".join(result["notes"]))

    def test_a_tree_already_invalid_at_the_base_only_warns(self) -> None:
        command(self.repo.root, "git", "switch", "-q", "main")
        self.repo.pending("2.0.1", [("patch", "Fix the tool")])
        self.repo.write_stamps("2.0.0")
        self.repo.commit("fix: the tool")
        command(self.repo.root, "git", "switch", "-q", "-C", "feat/work")
        self.repo.write("docs/note.md", "note\n")
        self.repo.commit("docs: note")
        result, code = self.preflight()
        self.assertEqual((result["status"], code), ("warn", 0))
        self.assertIn("typed repair", " ".join(result["notes"]))

    def test_a_task_branch_is_compared_with_its_recorded_target(self) -> None:
        command(self.repo.root, "git", "switch", "-q", "-c", "integration/EPC-001-line", "main")
        self.repo.write("line.txt", "line\n")
        line = self.repo.commit("chore: line work")
        command(self.repo.root, "git", "switch", "-q", "-c", "task/TSK-001-work")
        self.repo.write(
            "project-management/tasks/TSK-001.md",
            '---\nid: TSK-001\nintegration_target: "integration/EPC-001-line" # target\n---\n',
        )
        self.repo.commit("docs: record")
        head = command(self.repo.root, "git", "rev-parse", "HEAD")
        self.assertEqual(
            release.target_for_branch("task/TSK-001-work", head, self.repo.load(), cwd=self.repo.root),
            "integration/EPC-001-line",
        )
        result, _ = self.preflight(branch="task/TSK-001-work")
        self.assertEqual(result["status"], "ok", result)
        self.assertNotIn("line.txt", json.dumps(result))
        self.assertEqual(release.resolve_pr_base(self.repo.args(
            base="", target="", branch="task/TSK-001-work", remote="origin"
        ), head, self.repo.load())[0], line)

    def test_structural_check_state_reports_the_host_as_unchecked(self) -> None:
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            release.check_state(self.repo.args(ref="HEAD", structural=True))
        result = json.loads(output.getvalue())
        self.assertEqual((result["status"], result["host"]), ("ok", "not checked against the host"))


class BehaviourPathTests(unittest.TestCase):
    """TSK-106 AC-10: the behaviour paths and release contract members of the
    one path-set table, read the way `classify.rs` reads them."""

    sets = tomllib.loads((REPOSITORY / release.PATH_SETS).read_text(encoding="utf-8"))

    def test_every_member_example_classifies(self) -> None:
        for member in self.sets["not_behaviour"]:
            self.assertFalse(release.is_behaviour_path(member["example"], self.sets), member)
        for member in self.sets["behaviour_always"]:
            self.assertTrue(release.is_behaviour_path(member["example"], self.sets), member)
        for path, behaviour in [
            ("crates/codeflow-core/src/lib.rs", True),
            ("scripts/release.py", True),
            ("README.md", True),
            ("assets/base/agents/skills/cf-ship/SKILL.md", True),
            ("docs/releasing.md", False),
            ("docs/verification/evidence/tsk-106/journey.txt", False),
            ("project-management/templates/task.md", False),
            ("assets/base/pm/spec.md.tmpl", False),
        ]:
            with self.subTest(path=path):
                self.assertEqual(release.is_behaviour_path(path, self.sets), behaviour)

    def test_each_release_contract_member_is_watched(self) -> None:
        watched = release.load_config(release.DEFAULT_CONFIG)["watched_contract_paths"]
        members = [m for m in self.sets["adopter_facing"] if m.get("release_contract")]
        self.assertEqual([m["member"] for m in members], ["policy", "record_schema"])
        for member in members:
            for path in [member["example"], *(g.replace("**", "x/y.md") for g in member["patterns"])]:
                with self.subTest(path=path):
                    self.assertTrue(release.matches_any(path, watched), path)


class ErrataTests(unittest.TestCase):
    """TSK-106 AC-7 (R-96): errata are dated notes outside frozen bytes."""

    PUBLISHED = "## [2.1.0] - 2026-02-01\n\n- feature\n\n## [2.0.0] - 2026-01-01\n\n- public\n"

    def test_a_dated_erratum_before_the_sections_is_accepted(self) -> None:
        text = "# Changelog\n\n## Errata\n\n- 2026-09-27, 2.0.0: the note meant `run`.\n\n" + self.PUBLISHED
        release.validate_errata(text)
        self.assertEqual(
            release.published_snapshot(text, "2.1.0"),
            release.published_snapshot("# Changelog\n\n" + self.PUBLISHED, "2.1.0"),
        )

    def test_misplaced_undated_or_unpublished_errata_fail(self) -> None:
        for text, message in [
            ("# Changelog\n\n" + self.PUBLISHED + "\n## Errata\n\n- 2026-09-27, 2.0.0: x\n", "before the first"),
            ("# Changelog\n\n## Errata\n\n- 2.0.0: x\n\n" + self.PUBLISHED, "YYYY-MM-DD"),
            ("# Changelog\n\n## Errata\n\n- 2026-09-27, 9.9.9: x\n\n" + self.PUBLISHED, "not a published"),
        ]:
            with self.subTest(message=message), self.assertRaisesRegex(release.ReleaseError, message):
                release.validate_errata(text)

    def test_the_repository_errata_sit_outside_frozen_bytes(self) -> None:
        text = (REPOSITORY / "CHANGELOG.md").read_text(encoding="utf-8")
        release.validate_errata(text)
        self.assertIn("## Errata", text)
        self.assertLess(text.index("## Errata"), text.index("## ["))


class PublicationWorkflowTests(unittest.TestCase):
    """TSK-106 AC-8: the release-state job moved to its own workflow, and
    publication reads each configured workflow's latest main-push run."""

    def setUp(self) -> None:
        self.repo = Repository()
        config = json.loads(self.repo.config.read_text())
        config["publication_workflows"] = [
            ".github/workflows/codeflow-ci.yml",
            ".github/workflows/codeflow-release.yml",
        ]
        self.repo.config.write_text(json.dumps(config))

    def tearDown(self) -> None:
        self.repo.cleanup()

    def test_checks_come_from_each_workflow_latest_main_push(self) -> None:
        source = self.repo.target
        names = release.load_config(self.repo.config)["required_publication_checks"]
        checks = [
            {
                "id": index,
                "name": name,
                "head_sha": source,
                "status": "completed",
                "conclusion": "success",
                "app": {"slug": "github-actions"},
                "check_suite": {"id": 901 if name == "release state" else 900},
            }
            for index, name in enumerate(names, 1)
        ]
        state = self.repo.root / "checks.json"
        state.write_text(json.dumps([{"check_runs": checks}]))
        def run(path: str, suite: int, conclusion: str = "success") -> dict[str, object]:
            return {
                "id": suite, "run_number": suite, "run_attempt": 1, "check_suite_id": suite,
                "event": "push", "head_branch": "main", "head_sha": source, "path": path,
                "status": "completed", "conclusion": conclusion,
            }
        ci, rel = self.repo.root / "ci.json", self.repo.root / "release.json"
        ci.write_text(json.dumps([{"workflow_runs": [run(".github/workflows/codeflow-ci.yml", 900)]}]))
        rel.write_text(json.dumps([{"workflow_runs": [run(".github/workflows/codeflow-release.yml", 901)]}]))
        args = self.repo.args(state=state, runs_state=[ci, rel], source=source)
        release.verify_checks(args)
        with self.assertRaisesRegex(release.ReleaseError, "lacks its codeflow-release main-push"):
            release.verify_checks(self.repo.args(state=state, runs_state=[ci], source=source))
        rel.write_text(json.dumps([{"workflow_runs": [run(".github/workflows/codeflow-release.yml", 901, "failure")]}]))
        with self.assertRaisesRegex(release.ReleaseError, "codeflow-release main-push workflow run is not"):
            release.verify_checks(args)


if __name__ == "__main__":
    unittest.main()
