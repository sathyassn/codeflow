#!/usr/bin/env python3
"""Behavior tests for CodeFlow's repository-local release automation."""

from __future__ import annotations

import argparse
import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


SCRIPT = Path(__file__).with_name("release.py")
SPEC = importlib.util.spec_from_file_location("codeflow_release", SCRIPT)
assert SPEC and SPEC.loader
release = importlib.util.module_from_spec(SPEC)
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
        self.write("Cargo.toml", '[workspace]\nmembers = []\n\n[workspace.package]\nversion = "2.0.0"\n')
        self.write(
            "Cargo.lock",
            'version = 4\n\n[[package]]\nname = "codeflow-cli"\nversion = "2.0.0"\n'
            '[[package]]\nname = "codeflow-core"\nversion = "2.0.0"\n'
            '[[package]]\nname = "codeflow-present"\nversion = "2.0.0"\n',
        )
        self.write("CHANGELOG.md", "# Changelog\n\n## [Unreleased]\n\n### Added\n\n- reviewed note\n\n## [2.0.0] - 2026-01-02\n\n- staged prose\n")
        self.write("AGENTS.md", "<!-- codeflow:managed:begin scaffold=2.0.0 -->\n")
        self.write("CLAUDE.md", "<!-- codeflow:managed:begin scaffold=2.0.0 -->\n")
        self.write(".codeflow/project.toml", 'scaffold_version = "2.0.0"\ntier = "full"\n')
        self.write(
            ".codeflow/manifest.json",
            '{"schema_version": 1, "scaffold_version": "2.0.0", "files": {}}\n',
        )
        self.write("docs/releasing.md", "release policy\n")
        self.write("assets/contract.txt", "contract\n")
        self.commit("chore(release): v1.0.0")
        self.baseline = command(self.root, "git", "rev-parse", "HEAD")
        command(self.root, "git", "tag", "v1.0.0")
        self.config = self.root / ".release/config.json"
        self.record = self.root / ".release/candidate.json"
        self.fake_cliff = self.root / "fake-git-cliff"
        self.write("fake-git-cliff", "#!/bin/sh\nprintf 'v2.0.0\\n'\n")
        self.fake_cliff.chmod(0o755)
        self.write_config()
        self.commit("feat: configure release fixture")
        self.source = command(self.root, "git", "rev-parse", "HEAD")

    def cleanup(self) -> None:
        self.temp.cleanup()

    def write(self, path: str, value: str) -> None:
        destination = self.root / path
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text(value, encoding="utf-8")

    def write_config(self, **overrides: object) -> None:
        config = {
            "schema_version": 1,
            "release_unit": "codeflow",
            "main_branch": "main",
            "candidate_branch": "chore/release-codeflow",
            "candidate_record": ".release/candidate.json",
            "comparison": {"tag": "v1.0.0", "commit": self.baseline},
            "published": {
                "version": "1.0.0",
                "source_commit": self.baseline,
                "release_target_commit": self.baseline,
                "source_archive_sha256": "a" * 64,
            },
            "watched_contract_paths": ["assets/**", "docs/releasing.md"],
            "generated_allowlist": [
                ".codeflow/**",
                "AGENTS.md",
                "CLAUDE.md",
                "Cargo.toml",
                "Cargo.lock",
                "CHANGELOG.md",
            ],
        }
        config.update(overrides)
        self.write(".release/config.json", json.dumps(config, indent=2) + "\n")

    def commit(self, message: str) -> str:
        command(self.root, "git", "add", ".")
        command(self.root, "git", "commit", "-q", "-m", message)
        return command(self.root, "git", "rev-parse", "HEAD")

    def args(self, **values: object) -> argparse.Namespace:
        defaults = {"root": self.root, "config": self.config, "record": self.record}
        defaults.update(values)
        return argparse.Namespace(**defaults)

    def prepare_and_finalize(self) -> str:
        release.prepare(
            self.args(git_cliff=str(self.fake_cliff), date="2026-02-03")
        )
        release.finalize(self.args())
        self.commit("chore(release): v2.0.0")
        return command(self.root, "git", "rev-parse", "HEAD")


class ReleaseImpactTests(unittest.TestCase):
    def setUp(self) -> None:
        self.repo = Repository()

    def tearDown(self) -> None:
        self.repo.cleanup()

    def body(self, impact: str, *, contract: str = "compatible", migration: str = "") -> str:
        return (
            "## Release impact\n\n"
            "- Unit: `codeflow`\n"
            f"- Impact: `{impact}`\n"
            "- Rationale: Reviewed fixture impact.\n"
            "- Evidence: CHANGELOG.md [Unreleased].\n"
            f"- Contract: `{contract}`\n"
            f"- Migration: {migration}\n"
        )

    def add_commit(self, subject: str, path: str = "CHANGELOG.md") -> tuple[str, str]:
        base = command(self.repo.root, "git", "rev-parse", "HEAD")
        with (self.repo.root / path).open("a", encoding="utf-8") as target:
            target.write(f"\n{subject}\n")
        head = self.repo.commit(subject)
        return base, head

    def check(self, base: str, head: str, body: str) -> None:
        body_file = self.repo.root / "body.md"
        body_file.write_text(body, encoding="utf-8")
        release.check_pr(
            self.repo.args(base=base, head=head, body_file=body_file, body_env=None)
        )

    def test_patch_minor_major_and_mixed_markers(self) -> None:
        base, patch = self.add_commit("fix: correct output")
        self.check(base, patch, self.body("patch", contract="not-applicable"))
        base, minor = self.add_commit("feat: add output")
        self.check(base, minor, self.body("minor", contract="not-applicable"))
        base, major = self.add_commit("feat!: replace output")
        self.check(base, major, self.body("major", contract="breaking", migration="docs/migrate.md"))
        self.assertEqual(release.commit_impact(patch, major, cwd=self.repo.root), "major")

    def test_multi_commit_impact_reads_every_record_subject(self) -> None:
        base, feature = self.add_commit("feat: add route", "feature.txt")
        _, fix = self.add_commit("fix: repair route", "fix.txt")
        self.assertEqual(release.commit_impact(base, fix, cwd=self.repo.root), "minor")

        _, major = self.add_commit("feat!: replace route", "major.txt")
        _, later_fix = self.add_commit("fix: follow up", "later-fix.txt")
        self.assertEqual(release.commit_impact(feature, later_fix, cwd=self.repo.root), "major")

        base, older_feature = self.add_commit("feat: expose another route", "older-feature.txt")
        _, chore_one = self.add_commit("chore: tidy one", "chore-one.txt")
        _, chore_two = self.add_commit("docs: tidy two", "chore-two.txt")
        self.assertEqual(
            release.commit_impact(base, chore_two, cwd=self.repo.root), "minor"
        )

    def test_revert_and_all_none_are_none(self) -> None:
        base, head = self.add_commit("revert: undo internal experiment", "internal.txt")
        body = self.body("none", contract="not-applicable").replace(
            "CHANGELOG.md [Unreleased].", "No shipped surface changed."
        )
        self.check(base, head, body)

    def test_rejects_marker_mismatch_and_missing_migration(self) -> None:
        base, head = self.add_commit("feat!: break output")
        with self.assertRaisesRegex(release.ReleaseError, "contradicts"):
            self.check(base, head, self.body("minor"))
        with self.assertRaisesRegex(release.ReleaseError, "migration"):
            self.check(base, head, self.body("major", contract="breaking"))

    def test_watched_path_requires_explicit_contract_assessment(self) -> None:
        base, head = self.add_commit("docs: explain contract", "docs/releasing.md")
        with self.assertRaisesRegex(release.ReleaseError, "watched contract"):
            self.check(base, head, self.body("none", contract="not-applicable"))

    def test_non_none_requires_curated_note_change(self) -> None:
        base, head = self.add_commit("fix: shipped defect", "internal.txt")
        with self.assertRaisesRegex(release.ReleaseError, "CHANGELOG"):
            self.check(base, head, self.body("patch", contract="not-applicable"))

    def test_none_allows_historical_changelog_correction(self) -> None:
        base = command(self.repo.root, "git", "rev-parse", "HEAD")
        changelog = (self.repo.root / "CHANGELOG.md").read_text(encoding="utf-8")
        self.repo.write("CHANGELOG.md", changelog.replace("staged prose", "corrected prose"))
        head = self.repo.commit("docs: correct historical release note")
        self.check(base, head, self.body("none", contract="compatible"))


class CandidateTests(unittest.TestCase):
    def setUp(self) -> None:
        self.repo = Repository()

    def tearDown(self) -> None:
        self.repo.cleanup()

    def test_staged_unpublished_version_preserves_prose_and_is_idempotent(self) -> None:
        candidate = self.repo.prepare_and_finalize()
        changelog = (self.repo.root / "CHANGELOG.md").read_text(encoding="utf-8")
        self.assertIn("## [2.0.0] - 2026-01-02", changelog)
        self.assertIn("reviewed note", changelog)
        self.assertIn("staged prose", changelog)
        record = release.load_json(self.repo.record)
        self.assertEqual(record["tag"], "v2.0.0")
        self.assertEqual(record["comparison"]["commit"], self.repo.baseline)
        self.assertEqual(record["published_provenance"]["source_archive_sha256"], "a" * 64)
        release.verify_ref_candidate(candidate, release.load_config(self.repo.config), record, cwd=self.repo.root)
        with self.assertRaisesRegex(release.ReleaseError, "equal-tree main merge"):
            release.prepare(self.repo.args(git_cliff=str(self.repo.fake_cliff), date="2026-02-03"))

    def test_empty_unreleased_batch_does_not_create_candidate(self) -> None:
        self.repo.write("CHANGELOG.md", "# Changelog\n\n## [Unreleased]\n\n## [2.0.0] - 2026-01-02\n\n- staged\n")
        self.repo.commit("chore: empty unreleased")
        release.prepare(self.repo.args(git_cliff=str(self.repo.fake_cliff), date="2026-02-03"))
        self.assertFalse(self.repo.record.exists())

    def test_empty_batch_does_not_require_unreachable_historical_source(self) -> None:
        self.repo.write("CHANGELOG.md", "# Changelog\n\n## [Unreleased]\n\n## [2.0.0]\n\n- staged\n")
        config = release.load_config(self.repo.config)
        config["published"]["source_commit"] = "0" * 40
        release.write_json(self.repo.config, config)
        self.repo.commit("chore: no release work")
        release.prepare(self.repo.args(git_cliff="missing-git-cliff", date="2026-02-03"))
        self.assertFalse(self.repo.record.exists())

    def test_all_none_or_revert_history_does_not_invoke_patch_floor(self) -> None:
        command(self.repo.root, "git", "tag", "v2.0.0")
        self.repo.write(
            "CHANGELOG.md",
            "# Changelog\n\n## [Unreleased]\n\n- internal note\n\n"
            "## [2.0.0] - 2026-01-02\n\n- staged\n",
        )
        self.repo.commit("chore: internal bookkeeping")
        release.prepare(
            self.repo.args(git_cliff="missing-git-cliff", date="2026-02-03")
        )
        self.assertFalse(self.repo.record.exists())

        self.repo.write("internal.txt", "reverted\n")
        self.repo.commit("revert: undo internal bookkeeping")
        release.prepare(
            self.repo.args(git_cliff="missing-git-cliff", date="2026-02-03")
        )
        self.assertFalse(self.repo.record.exists())

    def test_next_cycle_uses_new_reachable_tag_not_bootstrap_again(self) -> None:
        candidate = self.repo.prepare_and_finalize()
        command(self.repo.root, "git", "branch", "chore/release-codeflow", candidate)
        command(self.repo.root, "git", "switch", "-q", "main")
        command(self.repo.root, "git", "reset", "--hard", "-q", self.repo.source)
        command(self.repo.root, "git", "merge", "--no-ff", "-q", "chore/release-codeflow", "-m", "Merge release")
        command(self.repo.root, "git", "tag", "v2.0.0", candidate)
        changelog = (self.repo.root / "CHANGELOG.md").read_text(encoding="utf-8")
        changelog = changelog.replace("## [Unreleased]\n", "## [Unreleased]\n\n### Fixed\n\n- later fix\n", 1)
        self.repo.write("CHANGELOG.md", changelog)
        self.repo.write("fake-git-cliff", "#!/bin/sh\nprintf 'v2.0.1\\n'\n")
        self.repo.fake_cliff.chmod(0o755)
        self.repo.commit("fix: later release")
        release.prepare(self.repo.args(git_cliff=str(self.repo.fake_cliff), date="2026-02-04"))
        record = release.load_json(self.repo.record)
        self.assertEqual(record["comparison"]["tag"], "v2.0.0")
        self.assertEqual(record["comparison"]["commit"], candidate)
        self.assertEqual(release.workspace_version(self.repo.root / "Cargo.toml"), "2.0.1")

    def test_stale_merged_candidate_blocks_until_reviewed_source_restore(self) -> None:
        candidate = self.repo.prepare_and_finalize()
        command(self.repo.root, "git", "branch", "chore/release-codeflow", candidate)
        command(self.repo.root, "git", "switch", "-q", "main")
        command(self.repo.root, "git", "reset", "--hard", "-q", self.repo.source)
        self.repo.write("concurrent.txt", "landed after candidate preparation\n")
        self.repo.commit("fix: concurrent main change")
        command(
            self.repo.root,
            "git",
            "merge",
            "--no-ff",
            "-q",
            "chore/release-codeflow",
            "-m",
            "Merge stale release",
        )
        with self.assertRaisesRegex(release.ReleaseError, "equal-tree main merge"):
            release.prepare(
                self.repo.args(git_cliff=str(self.repo.fake_cliff), date="2026-02-04")
            )

        record = release.load_json(self.repo.record)
        for path in record["generated_files"]:
            (self.repo.root / path).write_bytes(
                release.file_at_ref(record["source_commit"], path, cwd=self.repo.root)
            )
        self.repo.record.unlink()
        self.repo.commit("chore(release): restore abandoned candidate")
        release.prepare(
            self.repo.args(git_cliff=str(self.repo.fake_cliff), date="2026-02-04")
        )
        self.assertEqual(release.load_json(self.repo.record)["tag"], "v2.0.0")

    def test_unknown_or_moved_comparison_tag_fails_closed(self) -> None:
        config = release.load_config(self.repo.config)
        config["comparison"]["commit"] = "0" * 40
        release.write_json(self.repo.config, config)
        with self.assertRaisesRegex(release.ReleaseError, "do not move"):
            release.prepare(self.repo.args(git_cliff=str(self.repo.fake_cliff), date="2026-02-03"))

    def test_shallow_clone_needs_targeted_historical_source_fetch(self) -> None:
        command(self.repo.root, "git", "switch", "-q", "-c", "published-source")
        self.repo.write("published.txt", "published bytes\n")
        published = self.repo.commit("chore: historical published source")
        command(self.repo.root, "git", "switch", "main")
        config = release.load_config(self.repo.config)
        config["published"]["source_commit"] = published
        release.write_json(self.repo.config, config)
        self.repo.commit("chore: record historical source")
        with tempfile.TemporaryDirectory(prefix="codeflow-release-origin-") as directory:
            bare = Path(directory) / "origin.git"
            clone = Path(directory) / "clone"
            command(self.repo.root, "git", "clone", "-q", "--bare", str(self.repo.root), str(bare))
            command(Path(directory), "git", "clone", "-q", "--depth=1", "--single-branch", "--branch", "main", f"file://{bare}", str(clone))
            command(clone, "git", "fetch", "-q", "origin", "refs/tags/v1.0.0:refs/tags/v1.0.0")
            cloned_config = release.load_config(clone / ".release/config.json")
            with self.assertRaisesRegex(release.ReleaseError, "published source commit"):
                release.validate_provenance(cloned_config, cwd=clone)
            command(clone, "git", "fetch", "--no-tags", "origin", published)
            release.validate_provenance(cloned_config, cwd=clone)

    def test_finalize_rejects_unrelated_change(self) -> None:
        release.prepare(self.repo.args(git_cliff=str(self.repo.fake_cliff), date="2026-02-03"))
        self.repo.write("secrets.txt", "unrelated\n")
        with self.assertRaisesRegex(release.ReleaseError, "non-generated"):
            release.finalize(self.repo.args())

    def test_refresh_guard_rejects_manual_candidate_edits(self) -> None:
        candidate = self.repo.prepare_and_finalize()
        release.guard_refresh(self.repo.args(ref=candidate))
        self.repo.write("CHANGELOG.md", (self.repo.root / "CHANGELOG.md").read_text() + "manual edit\n")
        edited = self.repo.commit("docs: edit generated candidate")
        with self.assertRaisesRegex(release.ReleaseError, "changed paths|edited after generation"):
            release.guard_refresh(self.repo.args(ref=edited))

    def test_forged_record_tag_or_version_is_rejected(self) -> None:
        self.repo.prepare_and_finalize()
        record = release.load_json(self.repo.record)
        record["tag"] = "v9.9.9"
        release.write_json(self.repo.record, record)
        forged = self.repo.commit("chore: forge candidate tag")
        with self.assertRaisesRegex(release.ReleaseError, "exactly v plus"):
            release.verify_ref_candidate(
                forged, release.load_config(self.repo.config), record, cwd=self.repo.root
            )
        record["tag"] = "v2.0.1"
        record["version"] = "2.0.1"
        release.write_json(self.repo.record, record)
        forged = self.repo.commit("chore: forge candidate version")
        with self.assertRaisesRegex(release.ReleaseError, "coupled version stamps"):
            release.verify_ref_candidate(
                forged, release.load_config(self.repo.config), record, cwd=self.repo.root
            )

    def test_lock_and_manifest_version_drift_are_rejected(self) -> None:
        files = {
            path: (self.repo.root / path).read_bytes()
            for path in release.VERSION_STAMP_PATHS
        }
        files["Cargo.lock"] = files["Cargo.lock"].replace(b'version = "2.0.0"', b'version = "9.0.0"', 1)
        with self.assertRaisesRegex(release.ReleaseError, "workspace package versions"):
            release.validate_version_stamps(files)

        files = {
            path: (self.repo.root / path).read_bytes()
            for path in release.VERSION_STAMP_PATHS
        }
        files[".codeflow/manifest.json"] = files[".codeflow/manifest.json"].replace(
            b'"scaffold_version": "2.0.0"', b'"scaffold_version": "9.0.0"'
        )
        with self.assertRaisesRegex(release.ReleaseError, "manifest stamp"):
            release.validate_version_stamps(files)


class AuthorizationTests(unittest.TestCase):
    def setUp(self) -> None:
        self.repo = Repository()
        self.candidate = self.repo.prepare_and_finalize()
        command(self.repo.root, "git", "branch", "chore/release-codeflow", self.candidate)
        command(self.repo.root, "git", "switch", "-q", "main")
        command(self.repo.root, "git", "reset", "--hard", "-q", self.repo.source)
        command(self.repo.root, "git", "merge", "--no-ff", "-q", "chore/release-codeflow", "-m", "Merge release")
        self.merge = command(self.repo.root, "git", "rev-parse", "HEAD")

    def tearDown(self) -> None:
        self.repo.cleanup()

    def test_exact_second_parent_and_equal_tree_authorize(self) -> None:
        result = release.merged_candidate(
            self.candidate, "main", release.load_config(self.repo.config), cwd=self.repo.root
        )
        self.assertEqual(result["candidate"], self.candidate)
        self.assertEqual(result["merge"], self.merge)

    def test_event_requires_human_and_exact_merge(self) -> None:
        event = {
            "action": "closed",
            "pull_request": {
                "merged": True,
                "state": "closed",
                "base": {"ref": "main", "repo": {"full_name": "owner/repo"}},
                "head": {"ref": "chore/release-codeflow", "sha": self.candidate, "repo": {"full_name": "owner/repo"}},
                "merge_commit_sha": self.merge,
                "merged_by": {"type": "Bot", "login": "release-bot"},
                "user": {"login": "candidate-author", "type": "User"},
            },
        }
        event_path = self.repo.root / "event.json"
        pull_path = self.repo.root / "pull.json"
        reviews_path = self.repo.root / "reviews.json"
        permissions_path = self.repo.root / "permissions.json"
        event_path.write_text(json.dumps(event), encoding="utf-8")
        pull_path.write_text(json.dumps(event["pull_request"]), encoding="utf-8")
        reviews = [{
            "id": 1,
            "state": "APPROVED",
            "commit_id": self.candidate,
            "submitted_at": "2026-02-03T00:00:00Z",
            "author_association": "OWNER",
            "user": {"login": "reviewer", "type": "User"},
        }]
        reviews_path.write_text(json.dumps(reviews), encoding="utf-8")
        permissions_path.write_text(json.dumps({"reviewer": "write"}), encoding="utf-8")
        with self.assertRaisesRegex(release.ReleaseError, "human"):
            release.authorize_event(self.repo.args(event=event_path, pull=pull_path, reviews=reviews_path, permissions=permissions_path, repository="owner/repo"))
        event["pull_request"]["merged_by"] = {"type": "User", "login": "maintainer"}
        event_path.write_text(json.dumps(event), encoding="utf-8")
        pull_path.write_text(json.dumps(event["pull_request"]), encoding="utf-8")
        release.authorize_event(self.repo.args(event=event_path, pull=pull_path, reviews=reviews_path, permissions=permissions_path, repository="owner/repo"))

    def test_latest_change_request_and_stale_approval_block(self) -> None:
        pull = {
            "state": "closed",
            "merged": True,
            "merged_by": {"login": "maintainer", "type": "User"},
            "user": {"login": "candidate-author", "type": "User"},
            "head": {"sha": self.candidate, "repo": {"full_name": "owner/repo"}},
            "base": {"repo": {"full_name": "owner/repo"}},
        }
        approval = {
            "id": 1,
            "state": "APPROVED",
            "commit_id": self.repo.source,
            "submitted_at": "2026-02-03T00:00:00Z",
            "author_association": "MEMBER",
            "user": {"login": "reviewer", "type": "User"},
        }
        with self.assertRaisesRegex(release.ReleaseError, "exact head"):
            release.verify_review_data(pull, [approval], {"reviewer": "write"}, self.candidate, "owner/repo")
        approval["commit_id"] = self.candidate
        change = dict(approval, id=2, state="CHANGES_REQUESTED", submitted_at="2026-02-03T00:01:00Z")
        with self.assertRaisesRegex(release.ReleaseError, "requests changes"):
            release.verify_review_data(pull, [approval, change], {"reviewer": "write"}, self.candidate, "owner/repo")
        approval["user"] = {"login": "candidate-author", "type": "User"}
        with self.assertRaisesRegex(release.ReleaseError, "exact head"):
            release.verify_review_data(pull, [approval], {"candidate-author": "admin"}, self.candidate, "owner/repo")
        approval["user"] = {"login": "reviewer", "type": "User"}
        with self.assertRaisesRegex(release.ReleaseError, "exact head"):
            release.verify_review_data(pull, [approval], {"reviewer": "read"}, self.candidate, "owner/repo")

    def test_bot_review_is_irrelevant_when_current_human_approval_exists(self) -> None:
        pull = {
            "state": "closed",
            "merged": True,
            "merged_by": {"login": "maintainer", "type": "User"},
            "user": {"login": "candidate-author", "type": "User"},
            "head": {"sha": self.candidate, "repo": {"full_name": "owner/repo"}},
            "base": {"repo": {"full_name": "owner/repo"}},
        }
        approval = {
            "id": 1,
            "state": "APPROVED",
            "commit_id": self.candidate,
            "submitted_at": "2026-02-03T00:00:00Z",
            "user": {"login": "reviewer", "type": "User"},
        }
        bot = {
            "id": 2,
            "state": "CHANGES_REQUESTED",
            "commit_id": self.candidate,
            "submitted_at": "2026-02-04T00:00:00Z",
            "user": {"login": "release-bot", "type": "Bot"},
        }
        release.verify_review_data(
            pull,
            [approval, bot],
            {"reviewer": "write"},
            self.candidate,
            "owner/repo",
        )

    def test_moving_branch_without_merged_head_is_not_authority(self) -> None:
        command(self.repo.root, "git", "switch", "-q", "chore/release-codeflow")
        self.repo.write("CHANGELOG.md", (self.repo.root / "CHANGELOG.md").read_text() + "stale\n")
        moved = self.repo.commit("chore(release): move candidate")
        with self.assertRaises(release.ReleaseError):
            release.merged_candidate(
                moved, "main", release.load_config(self.repo.config), cwd=self.repo.root
            )

    def test_wrong_existing_tag_is_rejected(self) -> None:
        command(self.repo.root, "git", "tag", "v2.0.0", self.repo.source)
        with self.assertRaisesRegex(release.ReleaseError, "not reviewed candidate"):
            release.merged_candidate(
                self.candidate, "main", release.load_config(self.repo.config), cwd=self.repo.root
            )


class HostingTests(unittest.TestCase):
    def verify(self, state: dict[str, object], candidate: str = "c" * 40) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "host.json"
            notes = Path(directory) / "notes.md"
            path.write_text(json.dumps(state), encoding="utf-8")
            notes.write_text("reviewed notes\n", encoding="utf-8")
            release.verify_host_state(
                argparse.Namespace(
                    state=path,
                    candidate=candidate,
                    tag="v2.0.0",
                    notes=notes,
                )
            )

    def test_fresh_exact_tag_and_owned_empty_draft_are_retryable(self) -> None:
        self.verify({"tag_target": None, "release": None})
        self.verify({"tag_target": "c" * 40, "release": None})
        self.verify(
            {
                "tag_target": None,
                "release": {
                    "draft": True,
                    "assets": [],
                    "tag_name": "v2.0.0",
                    "name": "v2.0.0",
                    "target_commitish": "c" * 40,
                    "body": "reviewed notes",
                },
            }
        )

    def test_partial_public_or_mismatched_state_fails_closed(self) -> None:
        with self.assertRaisesRegex(release.ReleaseError, "cannot be proven"):
            self.verify({"tag_target": "c" * 40, "release": {"draft": True, "assets": [{"name": "partial"}]}})
        with self.assertRaisesRegex(release.ReleaseError, "publicly"):
            self.verify({"tag_target": "c" * 40, "release": {"draft": False, "assets": []}})
        with self.assertRaisesRegex(release.ReleaseError, "reviewed release notes"):
            self.verify(
                {
                    "tag_target": "c" * 40,
                    "release": {
                        "draft": True,
                        "assets": [],
                        "tag_name": "v2.0.0",
                        "name": "v2.0.0",
                        "target_commitish": "c" * 40,
                        "body": "unrelated draft",
                    },
                }
            )
        with self.assertRaisesRegex(release.ReleaseError, "does not target"):
            self.verify({"tag_target": "d" * 40, "release": None})

    def test_published_assets_match_same_run_names_sizes_and_digests(self) -> None:
        candidate = "c" * 40
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            artifacts = root / "artifacts"
            artifacts.mkdir()
            (artifacts / "codeflow.tar.gz").write_bytes(b"archive")
            (artifacts / "dist-manifest.json").write_bytes(b"{}\n")
            (artifacts / "linux-dist-manifest.json").write_bytes(b"not uploaded")

            assets = []
            for path in [artifacts / "codeflow.tar.gz", artifacts / "dist-manifest.json"]:
                assets.append(
                    {
                        "name": path.name,
                        "size": path.stat().st_size,
                        "state": "uploaded",
                        "digest": f"sha256:{release.sha256(path)}",
                    }
                )
            state = root / "published.json"
            state.write_text(
                json.dumps(
                    {
                        "tag_target": candidate,
                        "release": {
                            "draft": False,
                            "tag_name": "v2.0.0",
                            "target_commitish": candidate,
                            "assets": assets,
                        },
                    }
                ),
                encoding="utf-8",
            )
            args = argparse.Namespace(
                state=state,
                artifacts_dir=artifacts,
                candidate=candidate,
                tag="v2.0.0",
            )
            release.verify_published_assets(args)

            if os.name != "nt":
                symlink = artifacts / "linked-archive"
                symlink.symlink_to(artifacts / "codeflow.tar.gz")
                with self.assertRaisesRegex(release.ReleaseError, "not a regular file"):
                    release.verify_published_assets(args)
                symlink.unlink()

            assets[0]["digest"] = "sha256:" + "0" * 64
            state.write_text(
                json.dumps(
                    {
                        "tag_target": candidate,
                        "release": {
                            "draft": False,
                            "tag_name": "v2.0.0",
                            "target_commitish": candidate,
                            "assets": assets,
                        },
                    }
                ),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(release.ReleaseError, "same-run staged files"):
                release.verify_published_assets(args)

            assets.append(dict(assets[0]))
            state.write_text(
                json.dumps(
                    {
                        "tag_target": candidate,
                        "release": {
                            "draft": False,
                            "tag_name": "v2.0.0",
                            "target_commitish": candidate,
                            "assets": assets,
                        },
                    }
                ),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(release.ReleaseError, "duplicated"):
                release.verify_published_assets(args)


class ReleaseNotesTests(unittest.TestCase):
    def test_extracts_only_the_exact_curated_version_and_binds_candidate(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "CHANGELOG.md").write_text(
                "# Changelog\n\n## [Unreleased]\n\n## [3.0.0] - 2026-09-13\n\n"
                "### Added\n\n- reviewed note\n\n## [2.1.0] - 2026-07-05\n\n- old\n",
                encoding="utf-8",
            )
            candidate = "c" * 40
            self.assertEqual(
                release.release_notes(root, "v3.0.0", candidate),
                "### Added\n\n- reviewed note\n\n"
                f"<!-- codeflow-release-candidate: {candidate} -->\n",
            )

    def test_command_surface_materializes_notes_and_reports_errors(self) -> None:
        commands = {
            release.parser().parse_args(values).command
            for values in [
                ["check-pr", "--base", "a", "--head", "b", "--body-env", "BODY"],
                ["prepare"],
                ["finalize"],
                ["guard-refresh", "--ref", "candidate"],
                [
                    "authorize-event",
                    "--event",
                    "event.json",
                    "--pull",
                    "pull.json",
                    "--reviews",
                    "reviews.json",
                    "--permissions",
                    "permissions.json",
                    "--repository",
                    "owner/repo",
                ],
                [
                    "verify-review",
                    "--pull",
                    "pull.json",
                    "--reviews",
                    "reviews.json",
                    "--permissions",
                    "permissions.json",
                    "--candidate",
                    "c" * 40,
                    "--repository",
                    "owner/repo",
                ],
                ["verify-dispatch"],
                [
                    "verify-host-state",
                    "--state",
                    "state.json",
                    "--candidate",
                    "c" * 40,
                    "--tag",
                    "v3.0.0",
                    "--notes",
                    "notes.md",
                ],
                [
                    "verify-published-assets",
                    "--state",
                    "state.json",
                    "--artifacts-dir",
                    "artifacts",
                    "--candidate",
                    "c" * 40,
                    "--tag",
                    "v3.0.0",
                ],
                [
                    "release-notes",
                    "--candidate",
                    "c" * 40,
                    "--tag",
                    "v3.0.0",
                    "--output",
                    "notes.md",
                ],
            ]
        }
        self.assertEqual(
            commands,
            {
                "check-pr",
                "prepare",
                "finalize",
                "guard-refresh",
                "authorize-event",
                "verify-review",
                "verify-dispatch",
                "verify-host-state",
                "verify-published-assets",
                "release-notes",
            },
        )

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "CHANGELOG.md").write_text(
                "# Changelog\n\n## [Unreleased]\n\n## [3.0.0]\n\n- reviewed\n",
                encoding="utf-8",
            )
            output = root / "notes.md"
            argv = [
                "release.py",
                "--root",
                str(root),
                "release-notes",
                "--candidate",
                "c" * 40,
                "--tag",
                "v3.0.0",
                "--output",
                str(output),
            ]
            with mock.patch.object(sys, "argv", argv), contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(release.main(), 0)
            self.assertIn("reviewed", output.read_text(encoding="utf-8"))

            (root / "CHANGELOG.md").unlink()
            with (
                mock.patch.object(sys, "argv", argv),
                contextlib.redirect_stdout(io.StringIO()),
                contextlib.redirect_stderr(io.StringIO()),
            ):
                self.assertEqual(release.main(), 2)


@unittest.skipUnless(
    os.environ.get("CODEFLOW_REAL_GIT_CLIFF") == "1" and shutil.which("git-cliff"),
    "set CODEFLOW_REAL_GIT_CLIFF=1 with git-cliff 2.13.1 installed",
)
class RealGitCliffTests(unittest.TestCase):
    def calculate(self, subjects: list[str]) -> str:
        with tempfile.TemporaryDirectory(prefix="codeflow-cliff-") as directory:
            root = Path(directory)
            command(root, "git", "init", "-q", "-b", "main")
            command(root, "git", "config", "user.name", "Release Test")
            command(root, "git", "config", "user.email", "release@example.invalid")
            (root / "file.txt").write_text("one\n", encoding="utf-8")
            command(root, "git", "add", ".")
            command(root, "git", "commit", "-q", "-m", "chore: initial")
            command(root, "git", "tag", "v1.0.0")
            for index, subject in enumerate(subjects, start=2):
                (root / "file.txt").write_text(f"{index}\n", encoding="utf-8")
                command(root, "git", "commit", "-q", "-am", subject)
            return release.cliff_version("git-cliff", cwd=root)

    def test_real_calculator_contract(self) -> None:
        for subjects, expected in [
            (["fix: repair output"], "v1.0.1"),
            (["feat: add output"], "v1.1.0"),
            (["feat!: replace output"], "v2.0.0"),
            (["fix: repair output", "feat: add output"], "v1.1.0"),
        ]:
            with self.subTest(subjects=subjects):
                self.assertEqual(self.calculate(subjects), expected)

    def test_real_calculator_all_none_and_revert_behavior_is_pinned(self) -> None:
        # git-cliff 2.13.1 filters these from rendered notes but applies its
        # patch floor while calculating a bumped version. The preparer's empty
        # curated-notes check is therefore the all-none no-op boundary.
        self.assertEqual(self.calculate(["chore: internal only"]), "v1.0.1")
        self.assertEqual(self.calculate(["revert: undo internal experiment"]), "v1.0.1")


if __name__ == "__main__":
    unittest.main(verbosity=2)
