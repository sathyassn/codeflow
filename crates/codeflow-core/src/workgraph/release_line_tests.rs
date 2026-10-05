//! The transition tables' relief (TSK-140 AC-10, AC-13, SPC-013 R-120),
//! judged with the approved cutoffs set to the fixture's own. The shipped
//! judge approves only `CodeFlow`'s own cutoffs ([`APPROVED_CUTOFFS`]), so a
//! synthetic history can reach this relief only here; the CLI tests show
//! that the shipped judge refuses the same tables through CI and pre-push.

use std::path::{Path, PathBuf};

use super::*;

const LINE_A: &str = "integration/EPC-001-one";
const LINE_B: &str = "integration/EPC-002-two";
const RELEASE: &str = "integration/release-1";
const PROJECT: &str = "schema_version = 1\ntier = \"full\"\nscaffold_version = \"3.0.0\"\nstack = \"rust\"\nareas = []\npolicy_armed = true\ngit_hooks = \"wired\"\npermission_preset = \"default\"\n";
const CRITERIA: &str = "- AC-1 When run, the system shall work.\n- AC-2 (journey) On a fresh project, the command shall succeed.\n";
const LOOSER: &str = "- AC-1 When run, the system shall work sometimes.\n- AC-2 (journey) On a fresh project, the command shall succeed.\n";
const STRONGER: &str = "- AC-1 When run, the system shall work on every platform.\n- AC-2 (journey) On a fresh project, the command shall succeed.\n";

fn epic(id: &str) -> String {
    format!(
        "---\nid: {id}\ntitle: \"outcome {id}\"\nstatus: planning\nwork_type: feat\nspecs: []\ncreated: 2026-09-26\n---\n\n# {id}: outcome\n\n## Summary\n\nAn outcome.\n\n## Acceptance Criteria\n\n- AC-1 When used, the system shall work.\n"
    )
}

fn path(id: &str) -> String {
    format!("project-management/tasks/{id}.md")
}

/// The record of `id` on its line, with `criteria` and a Closeout.
fn record(id: &str, status: &str, criteria: &str, closeout: &str) -> String {
    let (epic, target) = match id {
        "TSK-001" | "TSK-003" => ("EPC-001", LINE_A),
        _ => ("EPC-002", LINE_B),
    };
    format!(
        "---\nid: {id}\nepic_id: {epic}\nstandalone_reason: null\nintegration_target: {target}\ntitle: \"work {id}\"\nstatus: {status}\nwork_type: feat\nspecs: []\ndepends_on: []\ncreated: 2026-09-26\n---\n\n# {id}: work\n\n## Description\n\nWork.\n\n## Acceptance Criteria\n\n{criteria}\n## Closeout\n\n{closeout}"
    )
}

fn git_in(dir: &Path, args: &[&str]) -> String {
    let out = crate::git::command()
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// `main` with two epics, TSK-001..004 on their lines and project config
/// without the marker; both lines cut from it, all pushed to a bare
/// `origin`.
struct Fx {
    _dir: tempfile::TempDir,
    root: PathBuf,
    origin: PathBuf,
}

impl Fx {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("work");
        let origin = dir.path().join("origin.git");
        std::fs::create_dir_all(&root).unwrap();
        git_in(
            dir.path(),
            &["init", "-q", "--bare", "-b", "main", "origin.git"],
        );
        let fx = Self {
            _dir: dir,
            root,
            origin,
        };
        fx.git(&["init", "-q", "-b", "main"]);
        fx.git(&["remote", "add", "origin", fx.origin.to_str().unwrap()]);
        for id in ["EPC-001", "EPC-002"] {
            fx.write(&format!("project-management/epics/{id}.md"), &epic(id));
        }
        for id in ["TSK-001", "TSK-002", "TSK-003", "TSK-004"] {
            fx.write(&path(id), &record(id, "todo", CRITERIA, "Pending.\n"));
        }
        fx.write(
            ".codeflow/policy.json",
            "{\n  \"schema_version\": 1,\n  \"git\": {\"product_paths\": [\"src/**\"]}\n}\n",
        );
        fx.write("src/lib.rs", "pub fn base() {}\n");
        fx.write(".codeflow/project.toml", PROJECT);
        fx.commit("chore: plan the work");
        for line in [LINE_A, LINE_B] {
            fx.git(&["branch", line, "main"]);
        }
        fx.git(&["push", "-q", "origin", "main", LINE_A, LINE_B]);
        fx
    }

    fn git(&self, args: &[&str]) -> String {
        git_in(&self.root, args)
    }

    fn write(&self, relative: &str, content: &str) {
        let full = self.root.join(relative);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, content).unwrap();
    }

    fn commit(&self, message: &str) -> String {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", message]);
        self.git(&["rev-parse", "HEAD"])
    }

    fn merge(&self, branch: &str) -> String {
        self.git(&[
            "merge",
            "-q",
            "--no-ff",
            "-m",
            &format!("merge: {branch}"),
            branch,
        ]);
        self.git(&["rev-parse", "HEAD"])
    }

    fn land(&self, line: &str, branch: &str) -> String {
        self.git(&["switch", "-q", line]);
        let merge = self.merge(branch);
        self.git(&["push", "-q", "origin", line]);
        merge
    }

    /// Change `files` on a branch of `line` and land it; returns the landing.
    fn land_files(&self, line: &str, branch: &str, files: &[(String, String)]) -> String {
        self.git(&["switch", "-q", "-C", branch, line]);
        for (file, content) in files {
            self.write(file, content);
        }
        self.commit("chore: change the line");
        self.land(line, branch)
    }

    /// Land a looser TSK-003 criterion with code on line A.
    fn land_mixed(&self, file: &str) -> String {
        let current = std::fs::read_to_string(self.root.join(path("TSK-003"))).unwrap();
        self.land_files(
            LINE_A,
            "task/TSK-001-mixed",
            &[
                (file.to_string(), "// mixed\n".to_string()),
                (path("TSK-003"), current.replace(CRITERIA, LOOSER)),
            ],
        )
    }

    /// A planning pull request on `line` strengthening `id`'s criteria.
    fn amend_on_line(&self, line: &str, id: &str) -> String {
        let branch = format!("plan/amend-{id}");
        self.git(&["switch", "-q", "-C", &branch, line]);
        let current = std::fs::read_to_string(self.root.join(path(id))).unwrap();
        self.write(&path(id), &current.replace(CRITERIA, STRONGER));
        self.commit("docs(records): amend the criterion");
        self.land(line, &branch)
    }

    /// TSK-003 completed on line A without an acceptance block.
    fn land_legacy_completion(&self) -> String {
        self.land_files(
            LINE_A,
            "chore/legacy",
            &[(
                path("TSK-003"),
                record(
                    "TSK-003",
                    "complete",
                    CRITERIA,
                    "Completed before the migration.\n",
                ),
            )],
        )
    }

    /// Write `key`'s table (`line = "commit"` lines) with the marker on
    /// main and publish it.
    fn record_table(&self, key: &str, entries: &str) {
        self.git(&["switch", "-q", "main"]);
        self.write(
            ".codeflow/project.toml",
            &format!("{PROJECT}{MARKER_KEY} = 1\n\n[{key}]\n{entries}"),
        );
        self.commit("chore: record the transition table");
        self.git(&["push", "-q", "origin", "main"]);
    }

    /// Cut the release branch from main, publish it, and import `line`.
    fn release_importing(&self, line: &str) {
        self.git(&["switch", "-q", "-C", RELEASE, "main"]);
        self.git(&["push", "-q", "origin", RELEASE]);
        self.git(&["fetch", "-q", "origin"]);
        self.git(&[
            "merge",
            "-q",
            "--no-ff",
            "-m",
            &format!("merge: import {line}"),
            &format!("origin/{line}"),
        ]);
    }

    /// Judge the release push with `approved` as the approved cutoffs.
    fn judged(&self, approved: &[(&str, &str)]) -> Result<Judgement, String> {
        let destination = ask_destination(&self.root, Some(self.origin.to_str().unwrap())).unwrap();
        let published = self.git(&["rev-parse", &format!("origin/{RELEASE}")]);
        judge(&self.root, &destination, &published, "HEAD", approved)
    }

    /// The records rule on the same release push, fed what `judged`
    /// brought, as `codeflow ci` runs it with main as the authority.
    fn records(&self, judged: &Judgement) -> super::super::lifecycle::Verdict {
        let published = self.git(&["rev-parse", &format!("origin/{RELEASE}")]);
        super::super::lifecycle::judge_release_range(
            &self.root,
            &published,
            "HEAD",
            "main",
            &judged.brought,
        )
        .unwrap()
    }
}

/// Whether the records rule refuses TSK-003 for its missing block.
fn missing_block(verdict: &super::super::lifecycle::Verdict) -> bool {
    verdict.errors.iter().any(|error| {
        error.contains("TSK-003") && error.contains("a complete record needs an acceptance block")
    })
}

fn frozen(judgement: &Judgement, landing: &str) -> bool {
    let at = format!(
        "TSK-003 changes its criteria on its line at {}",
        &landing[..9]
    );
    judgement
        .findings
        .iter()
        .any(|finding| finding.message.contains(&at))
}

/// AC-10: a criteria change landed with code at or before its line's
/// approved release-rule cutoff is information; the same table with the
/// cutoff unapproved refuses every release check.
#[test]
fn an_approved_rule_cutoff_turns_a_covered_change_into_information() {
    let key = BASELINE_KEY;
    // At the cutoff.
    let fx = Fx::new();
    let landing = fx.land_mixed("src/mixed.rs");
    fx.record_table(key, &format!("\"{LINE_A}\" = \"{landing}\"\n"));
    fx.release_importing(LINE_A);
    let judged = fx.judged(&[(LINE_A, &landing)]).unwrap();
    assert!(judged.findings.is_empty(), "{:?}", judged.findings);
    let note = format!(
        "legacy criteria change, landed before the release rule: TSK-003 on {LINE_A}, landing {}, cutoff {}",
        &landing[..9],
        &landing[..9]
    );
    assert!(
        judged.notes.iter().any(|line| line.contains(&note)),
        "{:?}",
        judged.notes
    );
    // Unapproved, the same table refuses.
    let refused = fx.judged(&[]).unwrap_err();
    assert!(
        refused.contains(&format!(
            "names {} as the cutoff of {LINE_A}, which is not one of CodeFlow's approved cutoffs",
            &landing[..9]
        )),
        "{refused}"
    );

    // Before the cutoff: a later planning landing on the line is the cutoff.
    let fx = Fx::new();
    let landing = fx.land_mixed("src/mixed.rs");
    let cutoff = fx.amend_on_line(LINE_A, "TSK-004");
    fx.record_table(key, &format!("\"{LINE_A}\" = \"{cutoff}\"\n"));
    fx.release_importing(LINE_A);
    let judged = fx.judged(&[(LINE_A, &cutoff)]).unwrap();
    assert!(judged.findings.is_empty(), "{:?}", judged.findings);
    assert!(
        judged.notes.iter().any(|line| line.contains(&format!(
            "landing {}, cutoff {}",
            &landing[..9],
            &cutoff[..9]
        ))),
        "{:?}",
        judged.notes
    );
}

/// AC-10 negative twins under an approved table: a landing after the
/// cutoff, a topic committed before the cutoff but landed after it, and a
/// resolution changing criteria again at a covered import.
#[test]
fn an_approved_rule_cutoff_covers_nothing_after_it() {
    // After the cutoff.
    let fx = Fx::new();
    let before = fx.git(&["rev-parse", LINE_A]);
    let landing = fx.land_mixed("src/mixed.rs");
    fx.record_table(BASELINE_KEY, &format!("\"{LINE_A}\" = \"{before}\"\n"));
    fx.release_importing(LINE_A);
    let judged = fx.judged(&[(LINE_A, &before)]).unwrap();
    assert!(frozen(&judged, &landing), "{:?}", judged.findings);

    // A backdated topic: committed before the cutoff, landed after it.
    let fx = Fx::new();
    fx.git(&["switch", "-q", "-C", "task/TSK-001-early", LINE_A]);
    fx.write("src/early.rs", "// early\n");
    let current = std::fs::read_to_string(fx.root.join(path("TSK-003"))).unwrap();
    fx.write(&path("TSK-003"), &current.replace(CRITERIA, LOOSER));
    fx.git(&["add", "-A"]);
    fx.git(&[
        "commit",
        "-q",
        "--date=2001-01-01T00:00:00",
        "-m",
        "feat: an early topic",
    ]);
    let cutoff = fx.amend_on_line(LINE_A, "TSK-004");
    let landing = fx.land(LINE_A, "task/TSK-001-early");
    fx.record_table(BASELINE_KEY, &format!("\"{LINE_A}\" = \"{cutoff}\"\n"));
    fx.release_importing(LINE_A);
    let judged = fx.judged(&[(LINE_A, &cutoff)]).unwrap();
    assert!(frozen(&judged, &landing), "{:?}", judged.findings);

    // A covered import whose resolution changes the criterion again.
    let fx = Fx::new();
    let landing = fx.land_mixed("src/mixed.rs");
    fx.record_table(BASELINE_KEY, &format!("\"{LINE_A}\" = \"{landing}\"\n"));
    fx.git(&["switch", "-q", "-C", RELEASE, "main"]);
    fx.git(&["push", "-q", "origin", RELEASE]);
    fx.git(&["fetch", "-q", "origin"]);
    fx.git(&[
        "merge",
        "-q",
        "--no-ff",
        "--no-commit",
        &format!("origin/{LINE_A}"),
    ]);
    let brought = std::fs::read_to_string(fx.root.join(path("TSK-003"))).unwrap();
    fx.write(
        &path("TSK-003"),
        &brought.replace(LOOSER, &LOOSER.replace("sometimes", "rarely")),
    );
    fx.commit("merge: import line A, resolving TSK-003");
    let judged = fx.judged(&[(LINE_A, &landing)]).unwrap();
    assert!(
        judged.findings.iter().any(|finding| finding
            .message
            .contains("TSK-003 changes its criteria directly on the release line")),
        "{:?}",
        judged.findings
    );
}

/// AC-10 (Codex R145-R2-1): the cutoff is the one of the line the task
/// targets. A shadow line advertised at the landing, or a line stacked on
/// line A after it, never lends its cutoff; line A's own cutoff covers the
/// landing whichever line brings it.
#[test]
fn an_approved_cutoff_comes_from_the_line_the_task_targets() {
    // A shadow line, sorted before line A, advertised at the landing with
    // that landing as its cutoff; line A's own cutoff is earlier.
    let fx = Fx::new();
    let shadow = "integration/EPC-000-shadow";
    fx.git(&["switch", "-q", "main"]);
    fx.write("project-management/epics/EPC-000.md", &epic("EPC-000"));
    fx.write(
        &path("TSK-005"),
        &record("TSK-005", "todo", CRITERIA, "Pending.\n")
            .replace(LINE_B, shadow)
            .replace("EPC-002", "EPC-000"),
    );
    fx.commit("docs(records): plan the shadow line");
    fx.git(&["push", "-q", "origin", "main"]);
    let before = fx.git(&["rev-parse", LINE_A]);
    let landing = fx.land_mixed("src/mixed.rs");
    fx.git(&[
        "push",
        "-q",
        "origin",
        &format!("{landing}:refs/heads/{shadow}"),
    ]);
    fx.record_table(
        BASELINE_KEY,
        &format!("\"{shadow}\" = \"{landing}\"\n\"{LINE_A}\" = \"{before}\"\n"),
    );
    fx.release_importing(LINE_A);
    let judged = fx.judged(&[(shadow, &landing), (LINE_A, &before)]).unwrap();
    assert!(frozen(&judged, &landing), "{:?}", judged.findings);
    assert!(judged.notes.is_empty(), "{:?}", judged.notes);

    let fx = Fx::new();
    let before = fx.git(&["rev-parse", LINE_A]);
    let landing = fx.land_mixed("src/mixed.rs");
    fx.git(&[
        "push",
        "-q",
        "--force",
        "origin",
        &format!("{landing}:refs/heads/{LINE_B}"),
    ]);
    fx.record_table(
        BASELINE_KEY,
        &format!("\"{LINE_A}\" = \"{before}\"\n\"{LINE_B}\" = \"{landing}\"\n"),
    );
    fx.release_importing(LINE_B);
    let judged = fx.judged(&[(LINE_A, &before), (LINE_B, &landing)]).unwrap();
    assert!(frozen(&judged, &landing), "{:?}", judged.findings);

    let fx = Fx::new();
    let landing = fx.land_mixed("src/mixed.rs");
    fx.git(&[
        "push",
        "-q",
        "--force",
        "origin",
        &format!("{landing}:refs/heads/{LINE_B}"),
    ]);
    fx.record_table(BASELINE_KEY, &format!("\"{LINE_A}\" = \"{landing}\"\n"));
    fx.release_importing(LINE_B);
    let judged = fx.judged(&[(LINE_A, &landing)]).unwrap();
    assert!(judged.findings.is_empty(), "{:?}", judged.findings);
    assert!(
        judged
            .notes
            .iter()
            .any(|line| line.contains(&format!("TSK-003 on {LINE_A}, landing {}", &landing[..9]))),
        "{:?}",
        judged.notes
    );
}

/// AC-13: a brought complete task without an acceptance block, last
/// changed on its line at or before the approved records cutoff, is a
/// legacy record, which the records rule passes with its notice; after the
/// cutoff, or from an old base landed after it, it is not, and the records
/// rule refuses it for the missing block; unapproved, the table refuses
/// every release check.
#[test]
fn an_approved_records_cutoff_lists_only_covered_legacy_records() {
    let key = RECORDS_BASELINE_KEY;
    let fx = Fx::new();
    let landing = fx.land_legacy_completion();
    fx.record_table(key, &format!("\"{LINE_A}\" = \"{landing}\"\n"));
    fx.release_importing(LINE_A);
    let judged = fx.judged(&[(LINE_A, &landing)]).unwrap();
    let legacy = judged
        .brought
        .legacy
        .get("TSK-003")
        .expect("a legacy record");
    assert!(
        legacy.contains(&format!(
            "legacy record, completed before the release rule: TSK-003 on {LINE_A}, landing {}, cutoff {}",
            &landing[..9],
            &landing[..9]
        )),
        "{legacy}"
    );
    let verdict = fx.records(&judged);
    assert!(!missing_block(&verdict), "{:?}", verdict.errors);
    assert!(
        verdict.notices.iter().any(|notice| notice.text == *legacy),
        "the notice reaches the records rule: {:?}",
        verdict.notices
    );
    let refused = fx.judged(&[]).unwrap_err();
    assert!(
        refused.contains("which is not one of CodeFlow's approved cutoffs"),
        "{refused}"
    );

    // After the cutoff.
    let fx = Fx::new();
    let before = fx.git(&["rev-parse", LINE_A]);
    fx.land_legacy_completion();
    fx.record_table(key, &format!("\"{LINE_A}\" = \"{before}\"\n"));
    fx.release_importing(LINE_A);
    let judged = fx.judged(&[(LINE_A, &before)]).unwrap();
    assert!(
        judged.brought.legacy.is_empty(),
        "{:?}",
        judged.brought.legacy
    );
    let verdict = fx.records(&judged);
    assert!(missing_block(&verdict), "{:?}", verdict.errors);

    // A topic branched from an old base, landed after the cutoff.
    let fx = Fx::new();
    fx.git(&["switch", "-q", "-C", "chore/old-base", LINE_A]);
    fx.write(
        &path("TSK-003"),
        &record(
            "TSK-003",
            "complete",
            CRITERIA,
            "Completed before the migration.\n",
        ),
    );
    fx.commit("docs(records): an old completion");
    let cutoff = fx.amend_on_line(LINE_A, "TSK-004");
    fx.land(LINE_A, "chore/old-base");
    fx.record_table(key, &format!("\"{LINE_A}\" = \"{cutoff}\"\n"));
    fx.release_importing(LINE_A);
    let judged = fx.judged(&[(LINE_A, &cutoff)]).unwrap();
    assert!(
        judged.brought.legacy.is_empty(),
        "{:?}",
        judged.brought.legacy
    );
    let verdict = fx.records(&judged);
    assert!(missing_block(&verdict), "{:?}", verdict.errors);
}

/// TSK-093's history in the 3.0.0 release: main lacks the record; one line
/// brings it with the `uid` it backfilled, and a later import brings it
/// from its own line without one. The merge keeps the `uid`, so it differs
/// from what the import brings, but a record that gains a `uid` keeps its
/// identity: no criteria change. Putting another `uid` there still is one.
#[test]
fn a_resolution_keeping_a_backfilled_uid_changes_no_criteria() {
    const UID: &str = "9b92f152-ea2d-4372-8ffa-ae3d5771f83d";
    const NEW: &str = "00000000-0000-4000-8000-000000000000";
    // (line B's uid, line A's uid, the uid the resolution writes, whether
    // it takes line A's body, frozen): a resolution may keep a uid one side
    // holds, never bring one neither side held.
    let cases = [
        (Some(UID), None, UID, true, false),
        (None, Some(UID), UID, false, false),
        (Some(UID), None, NEW, true, true),
        (None, None, NEW, true, true),
    ];
    for (b_uid, a_uid, kept, take_a, other) in cases {
        let fx = Fx::new();
        let plain = record("TSK-003", "todo", CRITERIA, "Pending.\n").replace("TSK-003", "TSK-006");
        let with_uid = |text: &str, uid: Option<&str>| match uid {
            Some(uid) => text.replacen("id: TSK-006\n", &format!("id: TSK-006\nuid: {uid}\n"), 1),
            None => text.to_string(),
        };
        let planned = plain.replace("Pending.\n", "Pending, planned.\n");
        fx.land_files(
            LINE_B,
            "chore/bring-tsk-006",
            &[(path("TSK-006"), with_uid(&plain, b_uid))],
        );
        fx.land_files(
            LINE_A,
            "docs/plan-tsk-006",
            &[(path("TSK-006"), with_uid(&planned, a_uid))],
        );
        fx.git(&["switch", "-q", "-C", RELEASE, "main"]);
        fx.git(&["push", "-q", "origin", RELEASE]);
        fx.git(&["fetch", "-q", "origin"]);
        fx.git(&[
            "merge",
            "-q",
            "--no-ff",
            "-m",
            "merge: import line B",
            &format!("origin/{LINE_B}"),
        ]);
        // Both sides add the record: the resolution writes one side's body
        // with the case's `uid`, never line A's record as it came.
        let _ = crate::git::command()
            .args([
                "merge",
                "-q",
                "--no-ff",
                "--no-commit",
                &format!("origin/{LINE_A}"),
            ])
            .current_dir(&fx.root)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.com")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.com")
            .output()
            .unwrap();
        let body = if take_a { &planned } else { &plain };
        fx.write(&path("TSK-006"), &with_uid(body, Some(kept)));
        fx.commit("merge: import line A, keeping the uid");
        let judged = fx.judged(&[]).unwrap();
        assert!(
            judged
                .path
                .iter()
                .any(|line| line.contains("resolved path")),
            "the merge is a resolution: {:?}",
            judged.path
        );
        let frozen = judged.findings.iter().any(|finding| {
            finding
                .message
                .contains("TSK-006 changes its criteria directly")
        });
        assert_eq!(
            frozen, other,
            "line B {b_uid:?}, line A {a_uid:?}, resolved {kept}: {:?}",
            judged.findings
        );
    }
}

/// TSK-214: an epic closed on the release line binds its own block as the
/// verb and CI do. A waiver naming no commit is refused; a block reviewed
/// at the commit it completes on is bound.
#[test]
fn an_epic_closed_on_the_release_line_binds_its_own_block() {
    let fx = Fx::new();
    fx.release_importing(LINE_A);
    let reviewed = fx.git(&["rev-parse", "HEAD"]);
    let closed = |ac1: &str| {
        format!(
            "{}\n## Closeout\n\n```yaml\nacceptance:\n  reviewed: {reviewed}\n  review: https://example.test/review/1\n  criteria:\n    AC-1: {ac1}\n  journey: none | no journey criterion\n  not_verified: none\n  follow_ups: none: done\n  verdict: approved\n```\n",
            epic("EPC-002").replace("status: planning", "status: complete")
        )
    };
    let epic_path = "project-management/epics/EPC-002.md";
    fx.write(epic_path, &closed("waived | deadbee"));
    fx.commit("docs(epics): close EPC-002");
    let judged = fx.judged(&[]).unwrap();
    assert!(
        judged
            .findings
            .iter()
            .any(|found| found.message.contains("EPC-002: AC-1 waiver names deadbee")),
        "{:?}",
        judged.findings
    );
    fx.git(&["reset", "-q", "--hard", "HEAD~1"]);
    fx.write(epic_path, &closed("verified | the release journey"));
    fx.commit("docs(epics): close EPC-002");
    let judged = fx.judged(&[]).unwrap();
    assert!(
        !judged
            .findings
            .iter()
            .any(|found| found.message.contains("EPC-002")),
        "{:?}",
        judged.findings
    );
}

/// Review finding on issue 79: tree and change paths were keyed by a lossy
/// spelling, so `caf` plus an invalid byte and `caf` plus a real U+FFFD were
/// one path and a change to one hid behind the other.
#[test]
fn paths_that_differ_only_in_an_invalid_byte_stay_two_paths() {
    let dir = tempfile::tempdir().unwrap();
    let repo = Repository::init(dir.path()).unwrap();
    let blob = repo.blob(b"x").unwrap();
    let mut inner = repo.treebuilder(None).unwrap();
    inner.insert(&b"f"[..], blob, 0o100_644).unwrap();
    let inner = inner.write().unwrap();
    let mut builder = repo.treebuilder(None).unwrap();
    builder.insert(&b"caf\xe9"[..], blob, 0o100_644).unwrap();
    builder
        .insert("caf\u{fffd}".as_bytes(), blob, 0o100_644)
        .unwrap();
    builder.insert(&b"dir\xff"[..], inner, 0o040_000).unwrap();
    let tree = repo.find_tree(builder.write().unwrap()).unwrap();
    let entries = tree_entries(&repo, &tree).unwrap();
    let keys: Vec<_> = entries.keys().map(String::as_str).collect();
    assert_eq!(keys.len(), 3, "{keys:?}");
    assert!(keys.contains(&"caf\u{fffd}"));
    assert!(keys.iter().any(|key| key.starts_with("caf\u{fffd}\0")));
    assert!(
        entries
            .keys()
            .any(|key| crate::git::path_key_bytes(key) == b"dir\xff/f"),
        "a file under a directory that is not valid UTF-8 is walked, with its exact path"
    );
}

/// A branch name that is not valid UTF-8 reaches `scope` spelled by
/// `ref_text`, whose escapes a release glob would misjudge, so it refuses.
#[test]
fn a_release_scope_refuses_a_branch_name_that_is_not_utf8() {
    let destination = Destination {
        url: None,
        default: None,
        heads: Vec::new(),
    };
    let name = crate::git::ref_text(b"integration/release-\xe9");
    let error = scope(Path::new("."), &destination, &name, None).unwrap_err();
    assert!(error.contains("not valid UTF-8"), "{error}");
    assert!(scope(Path::new("."), &destination, "task/TSK-001-x", None).is_ok());
}
