use codeflow_core::hooks::{git_hook, policy::GitPolicy};

#[test]
fn staged_private_key_name_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    repo.set_head("refs/heads/feat/path-policy").unwrap();
    std::fs::create_dir(dir.path().join("deploy")).unwrap();
    std::fs::write(dir.path().join("deploy/id_rsa"), "public fixture\n").unwrap();
    let mut index = repo.index().unwrap();
    index
        .add_path(std::path::Path::new("deploy/id_rsa"))
        .unwrap();
    index.write().unwrap();
    let report = git_hook::pre_commit(dir.path(), &GitPolicy::default(), false, false).unwrap();
    assert!(
        report
            .violations
            .iter()
            .any(|v| v.rule == "git.path_policy"),
        "{report:?}"
    );
}

use codeflow_core::hooks::{
    path_policy::{self, PathPolicy},
    PolicyLevel,
};
use git2::{Oid, Repository};
use serde_json::json;

struct Fixture {
    dir: tempfile::TempDir,
    repo: Repository,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        repo.set_head("refs/heads/feat/paths").unwrap();
        Self { dir, repo }
    }
    fn stage(&self, path: &str, mode: u32, contents: &[u8]) {
        let mut index = self.repo.index().unwrap();
        let id = if mode == 0o160_000 {
            Oid::from_str(std::str::from_utf8(contents).unwrap()).unwrap()
        } else {
            self.repo.blob(contents).unwrap()
        };
        index
            .add(&git2::IndexEntry {
                ctime: git2::IndexTime::new(0, 0),
                mtime: git2::IndexTime::new(0, 0),
                dev: 0,
                ino: 0,
                mode,
                uid: 0,
                gid: 0,
                file_size: u32::try_from(contents.len()).unwrap(),
                id,
                flags: 0,
                flags_extended: 0,
                path: path.as_bytes().to_vec(),
            })
            .unwrap();
        index.write().unwrap();
    }
    fn delete(&self, path: &str) {
        let mut index = self.repo.index().unwrap();
        index.remove_path(std::path::Path::new(path)).unwrap();
        index.write().unwrap();
    }
    fn commit(&self, parents: &[Oid]) -> Oid {
        let tree = self
            .repo
            .find_tree(self.repo.index().unwrap().write_tree().unwrap())
            .unwrap();
        let sig = git2::Signature::now("Tester", "test@example.invalid").unwrap();
        let parents: Vec<_> = parents
            .iter()
            .map(|p| self.repo.find_commit(*p).unwrap())
            .collect();
        self.repo
            .commit(
                None,
                &sig,
                &sig,
                "feat: path fixture",
                &tree,
                &parents.iter().collect::<Vec<_>>(),
            )
            .unwrap()
    }
    fn head(&self, id: Oid) {
        self.repo
            .reference("refs/heads/feat/paths", id, true, "test")
            .unwrap();
    }
    fn policy(git: serde_json::Value) -> PathPolicy {
        PathPolicy::from_git(&serde_json::from_value(git).unwrap()).unwrap()
    }
    fn range(&self, base: Oid, head: Oid, p: &PathPolicy) -> path_policy::Report {
        path_policy::range(
            self.dir.path(),
            Some(&base.to_string()),
            &head.to_string(),
            p,
        )
        .unwrap()
    }
}

#[test]
fn every_change_kind_has_the_same_staged_and_range_verdict() {
    for protected in [false, true] {
        for kind in [
            "add",
            "modify",
            "rename",
            "rename-away",
            "type",
            "delete",
            "gitlink",
        ] {
            let f = Fixture::new();
            let name = if protected {
                "private/file"
            } else {
                "deploy/id_rsa"
            };
            if ["modify", "type", "delete", "rename-away"].contains(&kind) {
                f.stage(name, 0o100_644, b"before");
            }
            if kind == "rename" {
                f.stage("ordinary", 0o100_644, b"data");
            }
            let base = f.commit(&[]);
            f.head(base);
            match kind {
                "rename-away" => {
                    f.delete(name);
                    f.stage("ordinary", 0o100_644, b"existing");
                }
                "delete" => f.delete(name),
                "rename" => {
                    f.delete("ordinary");
                    f.stage(name, 0o100_644, b"data");
                }
                "type" => f.stage(name, 0o120_000, b"ordinary"),
                "gitlink" => f.stage(name, 0o160_000, base.to_string().as_bytes()),
                _ => f.stage(name, 0o100_644, b"after"),
            }
            let p = Fixture::policy(json!({"protected_paths":[{"prefix":"private", "allow":[]}]}));
            let staged = path_policy::staged(&f.repo, &p).unwrap();
            let head = f.commit(&[base]);
            let range = f.range(base, head, &p);
            let blocked = protected || !["delete", "rename-away"].contains(&kind);
            for report in [staged, range] {
                assert_eq!(
                    !report.violations.is_empty(),
                    blocked,
                    "{protected} {kind}: {report:?}"
                );
                if !blocked {
                    assert!(report.notes.iter().any(|n| n.contains("history")));
                }
            }
        }
    }
}

#[test]
fn symlinks_are_judged_by_objects_without_following_them() {
    for (name, target, blocked) in [
        ("links/ordinary", "../private/data", true),
        ("links/ordinary", "/private/data", true),
        ("links/ordinary", "C:/data", true),
        ("links/ordinary", "id_rsa", true),
        ("ordinary", "private/data", true),
        ("id_rsa", "ordinary", true),
        ("ordinary", "missing-public-file", false),
        // A letter and a colon start a Windows drive-relative path.
        ("ordinary", "x:public", true),
        ("links/ordinary", "C:Windows/Temp/payload", true),
        ("ordinary", "ab:public", false),
        ("links/ordinary", "public", false),
        ("ordinary", "./docs", false),
        ("ordinary", "docs/", false),
        ("ordinary", "./private/data/", true),
    ] {
        let f = Fixture::new();
        let base = f.commit(&[]);
        f.head(base);
        f.stage(name, 0o120_000, target.as_bytes());
        let p = Fixture::policy(json!({"protected_paths":[{"prefix":"private", "allow":[]}]}));
        let staged = path_policy::staged(&f.repo, &p).unwrap();
        let head = f.commit(&[base]);
        assert_eq!(!staged.violations.is_empty(), blocked, "{name} -> {target}");
        assert_eq!(!f.range(base, head, &p).violations.is_empty(), blocked);
    }
    // The drive rule reads link targets only; a file with that name passes.
    for report in judge_one(
        "x:public",
        0o100_644,
        Some(b"public"),
        &Fixture::policy(json!({})),
    ) {
        assert!(report.violations.is_empty(), "{report:?}");
    }
}

#[test]
fn add_then_delete_is_still_introduced_and_off_skips_only_paths() {
    let f = Fixture::new();
    let base = f.commit(&[]);
    f.head(base);
    f.stage("deploy/id_rsa", 0o100_644, b"public");
    let added = f.commit(&[base]);
    f.delete("deploy/id_rsa");
    let deleted = f.commit(&[added]);
    let mut p = Fixture::policy(json!({}));
    assert!(!f.range(base, deleted, &p).violations.is_empty());
    p.level = PolicyLevel::Off;
    assert!(f.range(base, deleted, &p).violations.is_empty());
}

#[test]
fn remerge_excludes_inherited_paths_but_refuses_merge_introductions_and_octopus() {
    let f = Fixture::new();
    let root = f.commit(&[]);
    f.stage("deploy/id_rsa", 0o100_644, b"existing target content");
    let base = f.commit(&[root]);
    f.delete("deploy/id_rsa");
    f.stage("ordinary", 0o100_644, b"side");
    let side = f.commit(&[root]);
    f.stage("deploy/id_rsa", 0o100_644, b"existing target content");
    let clean = f.commit(&[side, base]);
    let p = Fixture::policy(json!({}));
    assert!(f.range(base, clean, &p).violations.is_empty());
    f.stage("config/prod/.env.local", 0o100_644, b"new");
    let evil = f.commit(&[side, base]);
    assert!(!f.range(base, evil, &p).violations.is_empty());
    let octopus = f.commit(&[side, base, root]);
    let error = path_policy::range(
        f.dir.path(),
        Some(&base.to_string()),
        &octopus.to_string(),
        &p,
    )
    .unwrap_err();
    assert!(error.contains("3 parents"), "{error}");
    // An object-backed commit whose second parent cannot be read.
    let tree = f.repo.index().unwrap().write_tree().unwrap();
    let raw = format!("tree {tree}\nparent {side}\nparent {}\nauthor Test <test@example.invalid> 1 +0000\ncommitter Test <test@example.invalid> 1 +0000\n\nmerge\n", "a".repeat(40));
    let broken = f
        .repo
        .odb()
        .unwrap()
        .write(git2::ObjectType::Commit, raw.as_bytes())
        .unwrap();
    assert!(path_policy::range(
        f.dir.path(),
        Some(&base.to_string()),
        &broken.to_string(),
        &p
    )
    .is_err());
}

#[test]
fn landed_authority_refuses_branch_waivers_and_unreadable_sources() {
    let f = Fixture::new();
    let target = json!({"git":{"protected_paths":[{"prefix":"private", "allow":[]}]}});
    f.stage(
        ".codeflow/policy.json",
        0o100_644,
        target.to_string().as_bytes(),
    );
    let base = f.commit(&[]);
    f.head(base);
    f.repo
        .remote("origin", "https://example.invalid/project.git")
        .unwrap();
    f.repo
        .reference("refs/remotes/origin/main", base, true, "fixture")
        .unwrap();
    let local: GitPolicy = serde_json::from_value(json!({"path_policy":"off", "blocked_names":[{"pattern":"*.pem","except_under":["/"],"reason":"branch only"}],
        "protected_paths":[{"prefix":"private","allow":[{"path":"/","reason":"branch only"}]}]})).unwrap();
    f.stage("private/cert.pem", 0o100_644, b"public");
    let p = codeflow_core::hooks::landed_policy::staged_paths(f.dir.path(), &local).unwrap();
    assert_eq!(p.level, PolicyLevel::Block);
    assert!(!path_policy::staged(&f.repo, &p)
        .unwrap()
        .violations
        .is_empty());
    // Before first fetch / no remote, HEAD still supplies the policy.
    f.repo.remote_delete("origin").unwrap();
    assert_eq!(
        codeflow_core::hooks::landed_policy::staged_paths(f.dir.path(), &local)
            .unwrap()
            .level,
        PolicyLevel::Block
    );
    f.repo
        .remote("origin", "https://example.invalid/project.git")
        .unwrap();
    f.repo
        .reference_symbolic(
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/missing",
            true,
            "fixture",
        )
        .unwrap();
    assert!(codeflow_core::hooks::landed_policy::staged_paths(f.dir.path(), &local).is_err());
}

#[test]
fn all_gate_inputs_remain_exempt_under_hostile_policy() {
    let f = Fixture::new();
    let base = f.commit(&[]);
    f.head(base);
    let p = Fixture::policy(
        json!({"blocked_names":["*"],"protected_paths":[{"prefix":"/", "allow":[]}]}),
    );
    for name in [
        ".codeflow/policy.json",
        ".codeflow/project.toml",
        ".codeflow/bootstrap.sh",
        ".codeflow/bootstrap.ps1",
        ".codeflow/ci-setup.sh",
        ".codeflow/git-hooks/pre-commit",
        ".github/workflows/codeflow-policy.yml",
        ".github/workflows/codeflow-ci.yml",
        ".gitignore",
        "deep/.gitignore",
    ] {
        f.stage(&name.to_uppercase(), 0o100_644, b"public");
    }
    assert!(path_policy::staged(&f.repo, &p)
        .unwrap()
        .violations
        .is_empty());
    let head = f.commit(&[base]);
    assert!(f.range(base, head, &p).violations.is_empty());
}

#[test]
fn declared_integration_target_contributes_denials_and_intersects_waivers() {
    let f = Fixture::new();
    f.stage(".codeflow/policy.json", 0o100_644, br#"{"git":{"blocked_names":[{"pattern":"*.pem","except_under":["fixtures"],"reason":"public"}]}}"#);
    f.stage(
        "project-management/tasks/TSK-007.md",
        0o100_644,
        b"---\nid: TSK-007\nintegration_target: integration/paths\n---\n",
    );
    let main = f.commit(&[]);
    f.stage(".codeflow/policy.json", 0o100_644, br#"{"git":{"blocked_names":[{"pattern":"*.pem","except_under":["fixtures/narrow"],"reason":"narrow"},"target-only"],"protected_paths":[{"prefix":"private","allow":[]}]}}"#);
    let line = f.commit(&[main]);
    f.repo
        .remote("origin", "https://example.invalid/repo.git")
        .unwrap();
    for (name, id) in [("main", main), ("integration/paths", line)] {
        f.repo
            .reference(&format!("refs/remotes/origin/{name}"), id, true, "fixture")
            .unwrap();
    }
    f.repo
        .reference("refs/heads/task/TSK-007-paths", line, true, "fixture")
        .unwrap();
    f.repo.set_head("refs/heads/task/TSK-007-paths").unwrap();
    let landed = codeflow_core::hooks::landed_policy::load(f.dir.path()).unwrap();
    assert!(landed.source.contains("integration/paths"));
    let p = PathPolicy::from_git(&landed.policy.git).unwrap();
    for (path, blocked) in [
        ("target-only", true),
        ("private/file", true),
        ("fixtures/cert.pem", true),
        ("fixtures/narrow/cert.pem", false),
    ] {
        f.stage(path, 0o100_644, b"public");
        let report = path_policy::staged(&f.repo, &p).unwrap();
        assert_eq!(!report.violations.is_empty(), blocked, "{path}: {report:?}");
        f.delete(path);
    }
}

/// A remote holding `main` and `integration/paths` at the given policies,
/// with the task branch on the line and the line's policy checked out.
fn line_fixture(main: &str, line: &str) -> Fixture {
    let f = Fixture::new();
    f.stage(".codeflow/policy.json", 0o100_644, main.as_bytes());
    f.stage(
        "project-management/tasks/TSK-007.md",
        0o100_644,
        b"---\nid: TSK-007\nintegration_target: integration/paths\n---\n",
    );
    let main_id = f.commit(&[]);
    f.stage(".codeflow/policy.json", 0o100_644, line.as_bytes());
    let line_id = f.commit(&[main_id]);
    f.repo
        .remote("origin", "https://example.invalid/repo.git")
        .unwrap();
    for (name, id) in [("main", main_id), ("integration/paths", line_id)] {
        f.repo
            .reference(&format!("refs/remotes/origin/{name}"), id, true, "fixture")
            .unwrap();
    }
    f.repo
        .reference("refs/heads/task/TSK-007-paths", line_id, true, "fixture")
        .unwrap();
    f.repo.set_head("refs/heads/task/TSK-007-paths").unwrap();
    std::fs::create_dir_all(f.dir.path().join(".codeflow")).unwrap();
    std::fs::write(f.dir.path().join(".codeflow/policy.json"), line).unwrap();
    f
}

#[test]
fn line_exceptions_stand_and_a_line_cannot_remove_a_main_denial() {
    let f = line_fixture(
        r#"{"git":{"blocked_names":["main-only"],"protected_paths":[{"prefix":"vault","allow":[]}]}}"#,
        r#"{"git":{"blocked_names":[{"pattern":"*.pem","except_under":["fixtures"],"reason":"public"}],
            "protected_paths":[{"prefix":"private","allow":[{"path":"private/examples","reason":"public"}]}]}}"#,
    );
    let local = codeflow_core::hooks::Policy::load(f.dir.path())
        .unwrap()
        .git;
    let p = codeflow_core::hooks::landed_policy::staged_paths(f.dir.path(), &local).unwrap();
    for (path, blocked) in [
        ("fixtures/cert.pem", false),
        ("private/examples/x", false),
        ("other/cert.pem", true),
        ("private/x", true),
        ("main-only", true),
        ("vault/x", true),
    ] {
        f.stage(path, 0o100_644, b"public");
        let report = path_policy::staged(&f.repo, &p).unwrap();
        assert_eq!(!report.violations.is_empty(), blocked, "{path}: {report:?}");
        f.delete(path);
    }
}

#[test]
fn identical_policies_with_explicit_rules_report_no_local_difference() {
    let policy = r#"{"git":{"blocked_names":[{"pattern":"*.pem","except_under":["fixtures"],"reason":"public"},"extra"],
        "protected_paths":[{"prefix":"private","allow":[{"path":"private/examples","reason":"public"}]}]}}"#;
    let f = line_fixture(policy, policy);
    let landed = codeflow_core::hooks::landed_policy::load(f.dir.path()).unwrap();
    assert!(
        landed.source.contains("integration/paths"),
        "{}",
        landed.source
    );
    assert!(!landed.local_differs);
    std::fs::write(
        f.dir.path().join(".codeflow/policy.json"),
        r#"{"git":{"blocked_names":["extra"]}}"#,
    )
    .unwrap();
    assert!(
        codeflow_core::hooks::landed_policy::load(f.dir.path())
            .unwrap()
            .local_differs
    );
}

#[test]
fn schema_rejects_invalid_declarations_and_keeps_new_keys_out_of_shipped_policy() {
    use codeflow_core::hooks::policy_schema::validate_policy_str;
    for value in [
        json!({"blocked_names":[{"pattern":"*.pem","except_under":["a/../b"],"reason":"invalid"}]}),
        json!({"blocked_names":[{"pattern":"*.pem","except_under":["tests"],"reason":"", "extra":true}]}),
        json!({"protected_paths":[{"prefix":"private", "allow":[{"path":"x", "reason":""}]}]}),
    ] {
        assert!(validate_policy_str(&json!({"git":value}).to_string()).is_err());
    }
    let shipped: serde_json::Value =
        serde_json::from_str(include_str!("../../../assets/base/policy.json")).unwrap();
    for key in ["path_policy", "blocked_names", "protected_paths"] {
        assert!(shipped["git"].get(key).is_none());
    }
    let note = include_str!("../../../changelog.d/TSK-322.md");
    // A name added after TSK-322 is named by the release note that added it.
    let later = include_str!("../../../changelog.d/TSK-1079.md");
    for name in path_policy::DEFAULT_NAMES {
        assert!(note.contains(name) || later.contains(name), "{name}");
    }
    assert!(later.contains("`credentials.json`"));
    assert!(later.contains("`.credentials.json`"));
    assert!(note.contains("except_under"));
}

/// Every gate input the exemption admits (`path_policy::exempt`).
const GATE_INPUTS: [&str; 10] = [
    ".gitignore",
    "deep/.gitignore",
    ".codeflow/policy.json",
    ".codeflow/project.toml",
    ".codeflow/bootstrap.sh",
    ".codeflow/bootstrap.ps1",
    ".codeflow/ci-setup.sh",
    ".codeflow/git-hooks/pre-commit",
    ".github/workflows/codeflow-policy.yml",
    ".github/workflows/codeflow-ci.yml",
];

/// Stage one entry on an empty base and return the staged and range reports.
fn judge_one(
    path: &str,
    mode: u32,
    contents: Option<&[u8]>,
    p: &PathPolicy,
) -> [path_policy::Report; 2] {
    let f = Fixture::new();
    let base = f.commit(&[]);
    f.head(base);
    let gitlink = base.to_string();
    f.stage(path, mode, contents.unwrap_or(gitlink.as_bytes()));
    let staged = path_policy::staged(&f.repo, p).unwrap();
    let head = f.commit(&[base]);
    [staged, f.range(base, head, p)]
}

#[test]
fn links_and_gitlinks_at_gate_inputs_refuse_while_files_stay_exempt() {
    let default = Fixture::policy(json!({}));
    let hostile = Fixture::policy(
        json!({"blocked_names":["*"],"protected_paths":[{"prefix":"/", "allow":[]}]}),
    );
    // Every case is judged and every wrong verdict is listed, so a failure
    // names each exempt path and mode it affects.
    let mut wrong = Vec::new();
    let mut expect = |case: String, reports: [path_policy::Report; 2], blocked: bool| {
        for (mode, report) in ["staged", "range"].into_iter().zip(reports) {
            if report.violations.is_empty() == blocked {
                wrong.push(format!("{case} ({mode}): {report:?}"));
            }
        }
    };
    for path in GATE_INPUTS {
        for (what, mode, contents) in [
            ("absolute link", 0o120_000, Some(b"/tmp/payload".as_slice())),
            ("drive link", 0o120_000, Some(b"C:/payload".as_slice())),
            (
                "drive-relative link",
                0o120_000,
                Some(b"C:Windows/Temp/payload".as_slice()),
            ),
            ("parent link", 0o120_000, Some(b"../outside".as_slice())),
            ("invalid link", 0o120_000, Some(b"a//b".as_slice())),
            // Resolved beside the link; a target that is itself a gate input
            // passes, so the blocked name sits one directory down.
            (
                "link to a blocked name",
                0o120_000,
                Some(b"keys/id_rsa".as_slice()),
            ),
            ("gitlink", 0o160_000, None),
        ] {
            let reports = judge_one(path, mode, contents, &default);
            expect(format!("{path} {what}"), reports, true);
        }
        for (name, p) in [("default", &default), ("hostile", &hostile)] {
            let upper = path.to_uppercase();
            let reports = judge_one(&upper, 0o100_644, Some(b"public"), p);
            expect(format!("{upper} file, {name} policy"), reports, false);
            let reports = judge_one(path, 0o100_755, Some(b"#!/bin/sh\n"), p);
            expect(format!("{path} executable, {name} policy"), reports, false);
        }
        // A safe in-tree link is judged by its target, which here is ordinary.
        let reports = judge_one(path, 0o120_000, Some(b"ordinary"), &default);
        expect(format!("{path} safe link"), reports, false);
    }
    assert!(
        wrong.is_empty(),
        "{} wrong:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}

#[test]
fn removing_a_link_or_gitlink_from_a_gate_input_passes() {
    for (mode, contents) in [(0o120_000, "/tmp/payload"), (0o160_000, "")] {
        let f = Fixture::new();
        let root = f.commit(&[]);
        let contents = if contents.is_empty() {
            root.to_string()
        } else {
            contents.to_string()
        };
        f.stage(".codeflow/bootstrap.sh", mode, contents.as_bytes());
        let base = f.commit(&[root]);
        f.head(base);
        f.delete(".codeflow/bootstrap.sh");
        f.stage(".codeflow/bootstrap.sh", 0o100_755, b"#!/bin/sh\n");
        let p = Fixture::policy(json!({}));
        assert!(path_policy::staged(&f.repo, &p)
            .unwrap()
            .violations
            .is_empty());
        let head = f.commit(&[base]);
        assert!(f.range(base, head, &p).violations.is_empty());
        f.delete(".codeflow/bootstrap.sh");
        let gone = f.commit(&[head]);
        assert!(f.range(head, gone, &p).violations.is_empty());
    }
}

#[test]
fn credentials_json_is_a_shipped_blocked_name_at_any_depth_and_case() {
    let p = Fixture::policy(json!({}));
    for (path, blocked) in [
        ("credentials.json", true),
        ("config/service/credentials.json", true),
        ("Deploy/CREDENTIALS.JSON", true),
        (".credentials.json", true),
        ("home/.claude/.credentials.json", true),
        ("Backup/.CREDENTIALS.JSON", true),
        ("credentials.json.example", false),
        ("docs/credentials.md", false),
    ] {
        for report in judge_one(path, 0o100_644, Some(b"{}"), &p) {
            assert_eq!(!report.violations.is_empty(), blocked, "{path}: {report:?}");
        }
    }
}
