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
import unittest
from unittest import mock

SCRIPT = Path(__file__).with_name("release.py")
SPEC = importlib.util.spec_from_file_location("codeflow_release", SCRIPT)
assert SPEC and SPEC.loader
release = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = release
SPEC.loader.exec_module(release)


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
                "comparison": {"tag": "v2.0.0", "commit": self.baseline},
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
            "assets": [],
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

    def pending(self, version: str, entries: list[tuple[str, str]]) -> None:
        body = "\n".join(
            f"<!-- codeflow:release-impact {impact} -->\n- {note}\n"
            for impact, note in entries
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
        contract: str = "compatible",
        migration: str = "",
        withdrawal: str = "",
    ) -> str:
        return (
            "## Release impact\n\n"
            "- Unit: codeflow\n"
            f"- Impact: {impact}\n"
            "- Rationale: reviewed fixture.\n"
            "- Evidence: CHANGELOG.md pending entry.\n"
            f"- Contract: {contract}\n"
            f"- Migration: {migration}\n"
            f"- Withdrawal: {withdrawal}\n"
        )


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
            "<!-- codeflow:release-impact minor -->\n- new guidance\n\n"
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
        text = text.replace("## [2.0.1] - 2026-02-02", "## [2.1.0]\n\n<!-- codeflow:release-impact minor -->\n- feature\n\n## [2.0.1]")
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
            "assets": [],
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
        text = "# Changelog\n\n## [3.0.1]\n\n<!-- codeflow:release-impact patch -->\n- later fix\n\n## [3.0.0]\n\n- public\n\n## [2.0.0] - 2026-01-01\n\n- old\n"
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
            "<!-- codeflow:release-impact minor -->\n- existing feature\n\n"
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
            "## [3.0.0]\n", "## [3.0.0]\n\n<!-- codeflow:release-impact patch -->\n- repair\n", 1
        ))
        head = self.repo.commit("docs: describe repair")
        self.run_check(base, head, self.repo.body("patch"))
        with self.assertRaisesRegex(release.ReleaseError, "must equal newly added"):
            self.run_check(base, head, self.repo.body("none"))

    def test_declared_impact_must_equal_new_annotation(self) -> None:
        base = self.repo.target
        self.repo.pending("2.0.1", [("patch", "small fix")])
        head = self.repo.commit("feat: overstated declaration")
        with self.assertRaisesRegex(release.ReleaseError, "must equal newly added"):
            self.run_check(base, head, self.repo.body("minor"))

    def test_release_bearing_commit_cannot_reuse_existing_pending_label(self) -> None:
        self.repo.pending("3.0.0", [("major", "existing break")])
        base = self.repo.commit("feat!: existing pending break")
        changelog = self.repo.root / "CHANGELOG.md"
        changelog.write_text(changelog.read_text() + "\n", encoding="utf-8")
        head = self.repo.commit("feat!: unrelated release-bearing commit")
        with self.assertRaisesRegex(release.ReleaseError, "must equal newly added"):
            self.run_check(base, head, self.repo.body("major", migration="docs/migrate.md"))

    def test_new_major_entry_cannot_hide_behind_none_declaration(self) -> None:
        base = self.repo.target
        self.repo.pending("3.0.0", [("major", "hidden break")])
        head = self.repo.commit("docs: claim none")
        with self.assertRaisesRegex(release.ReleaseError, "must equal newly added"):
            self.run_check(base, head, self.repo.body("none"))

    def test_new_major_cannot_hide_behind_a_same_label_rewrite(self) -> None:
        self.repo.pending("3.0.0", [("major", "existing break")])
        base = self.repo.commit("feat!: existing break")
        self.repo.pending(
            "3.0.0",
            [("patch", "existing break was narrowed"), ("major", "new hidden break")],
        )
        head = self.repo.commit("fix: mix reclassification and new break")
        with self.assertRaisesRegex(release.ReleaseError, "must equal newly added"):
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
        self.repo.pending("2.0.1", [("patch", "fix a crash")])
        base = self.repo.commit("fix: pending behavior")
        self.repo.pending("2.0.1", [("patch", "clarify when the crash occurs")])
        head = self.repo.commit("docs: clarify pending notes")
        self.run_check(
            base,
            head,
            self.repo.body("none", contract="not-applicable").replace(
                "reviewed fixture.", "Clarifies the existing pending note only."
            ),
        )

    def test_major_note_refinement_still_requires_migration_guidance(self) -> None:
        self.repo.pending("3.0.0", [("major", "replace old command")])
        base = self.repo.commit("feat!: pending break")
        self.repo.pending("3.0.0", [("major", "replace old command after migration")])
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
            "<!-- codeflow:release-impact minor -->\n- new guidance\n\n"
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


if __name__ == "__main__":
    unittest.main()
