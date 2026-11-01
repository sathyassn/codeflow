//! The size and ignored-file rules (TSK-323), judged with the policy, the
//! project kind and the `.gitignore` files of the authority commit, on
//! staged changes and on commit ranges.
use codeflow_core::hooks::path_policy::{self, ContentRules, PathPolicy, Report};
use codeflow_core::hooks::policy::GitPolicy;
use codeflow_core::hooks::PolicyLevel;
use git2::{Oid, Repository};
use serde_json::json;

const MIB: usize = 1 << 20;

struct Fixture {
    dir: tempfile::TempDir,
    repo: Repository,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        repo.set_head("refs/heads/feat/content").unwrap();
        Self { dir, repo }
    }

    fn stage(&self, path: &str, contents: &[u8]) {
        let mut index = self.repo.index().unwrap();
        index
            .add(&git2::IndexEntry {
                ctime: git2::IndexTime::new(0, 0),
                mtime: git2::IndexTime::new(0, 0),
                dev: 0,
                ino: 0,
                mode: 0o100_644,
                uid: 0,
                gid: 0,
                file_size: u32::try_from(contents.len()).unwrap(),
                id: self.repo.blob(contents).unwrap(),
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
        let id = self
            .repo
            .commit(
                None,
                &sig,
                &sig,
                "feat: content fixture",
                &tree,
                &parents.iter().collect::<Vec<_>>(),
            )
            .unwrap();
        self.repo
            .reference("refs/heads/feat/content", id, true, "test")
            .unwrap();
        id
    }

    /// The authority commit: its `.gitignore`, its project kind and its
    /// policy, which `git` returns parsed.
    fn authority(&self, gitignore: &str, new_scaffold: bool, git: &serde_json::Value) -> Oid {
        self.stage(".gitignore", gitignore.as_bytes());
        if new_scaffold {
            self.stage(".codeflow/project.toml", b"file_size_rule = \"refuse\"\n");
        }
        self.stage(
            ".codeflow/policy.json",
            json!({ "git": git }).to_string().as_bytes(),
        );
        self.commit(&[])
    }

    fn rules(&self, authority: Oid, git: &serde_json::Value) -> ContentRules {
        let git: GitPolicy = serde_json::from_value(git.clone()).unwrap();
        ContentRules::at(&self.repo, &git, Some(authority), None).unwrap()
    }

    /// The staged verdict and the verdict on the commit made from it.
    fn judge(&self, authority: Oid, git: &serde_json::Value) -> [Report; 2] {
        let rules = self.rules(authority, git);
        let paths = PathPolicy::from_git(&GitPolicy::default()).unwrap();
        let staged = path_policy::staged_with(&self.repo, &paths, Some(&rules)).unwrap();
        let head = self.commit(&[authority]);
        let range = path_policy::range_with(
            self.dir.path(),
            Some(&authority.to_string()),
            &head.to_string(),
            &paths,
            Some(&rules),
        )
        .unwrap();
        [staged, range]
    }
}

fn levels(report: &Report, rule: &str) -> Vec<PolicyLevel> {
    report
        .violations
        .iter()
        .filter(|v| v.rule == rule)
        .map(|v| v.level)
        .collect()
}

fn big(size: usize) -> Vec<u8> {
    vec![b'x'; size]
}

#[test]
fn an_ignored_file_warns_by_default_and_block_refuses_it() {
    for (git, level) in [
        (json!({}), PolicyLevel::Warn),
        (
            json!({"ignored_files": {"level": "block"}}),
            PolicyLevel::Block,
        ),
    ] {
        let f = Fixture::new();
        let authority = f.authority("build/\n*.log\n", false, &git);
        f.stage("build/out.o", b"object");
        f.stage("src/debug.log", b"log");
        f.stage("src/main.rs", b"fn main() {}");
        for report in f.judge(authority, &git) {
            assert_eq!(
                levels(&report, "git.ignored_files"),
                [level, level],
                "{report:?}"
            );
            let text = format!("{:?}", report.violations);
            assert!(text.contains("build/out.o matches `build/`"), "{text}");
            assert!(!text.contains("src/main.rs"), "{text}");
        }
    }
}

#[test]
fn a_tracked_ignored_file_is_still_judged() {
    let f = Fixture::new();
    f.stage("app.log", b"tracked before the rule");
    let authority = f.authority("*.log\n", false, &json!({}));
    f.stage("app.log", b"changed");
    for report in f.judge(authority, &json!({})) {
        assert_eq!(
            levels(&report, "git.ignored_files"),
            [PolicyLevel::Warn],
            "{report:?}"
        );
    }
}

#[test]
fn an_allow_entry_in_the_authority_admits_its_path() {
    let git = json!({"ignored_files": {"level": "block",
        "allow": [{"path": "Build/Kept", "reason": "generated fixture the tests read"}]}});
    let f = Fixture::new();
    let authority = f.authority("build/\n", false, &git);
    f.stage("build/kept/data.bin", b"kept");
    f.stage("build/other.bin", b"other");
    for report in f.judge(authority, &git) {
        let text = format!("{:?}", report.violations);
        assert_eq!(levels(&report, "git.ignored_files"), [PolicyLevel::Block]);
        assert!(text.contains("build/other.bin"), "{text}");
        assert!(!text.contains("kept/data.bin"), "{text}");
    }
}

#[test]
fn a_gitignore_change_in_the_change_admits_and_adds_nothing() {
    let f = Fixture::new();
    let authority = f.authority("build/\n", false, &json!({}));
    // The change re-includes build/ and ignores *.dat in its own file.
    f.stage(".gitignore", b"!build/\n*.dat\n");
    f.stage("build/out.o", b"object");
    f.stage("table.dat", b"data");
    for report in f.judge(authority, &json!({})) {
        let text = format!("{:?}", report.violations);
        assert_eq!(
            levels(&report, "git.ignored_files"),
            [PolicyLevel::Warn],
            "{text}"
        );
        assert!(text.contains("build/out.o"), "{text}");
        assert!(!text.contains("table.dat"), "{text}");
        assert!(!text.contains("changed: .gitignore"), "{text}");
    }
}

#[test]
fn matching_follows_git_for_negation_nesting_and_case() {
    let f = Fixture::new();
    f.stage("build/.gitignore", b"!keep.o\n");
    f.stage("docs/.gitignore", b"*.tmp\n!keep.tmp\n");
    let authority = f.authority("build/\n*.log\n!keep.log\n", false, &json!({}));
    for (path, ignored) in [
        // An excluded directory cannot be re-included beneath it.
        ("build/keep.o", true),
        ("BUILD/Out.o", true),
        ("keep.log", false),
        ("deep/other.LOG", true),
        ("docs/draft.tmp", true),
        ("docs/keep.tmp", false),
        ("elsewhere/draft.tmp", false),
        ("builder/x", false),
    ] {
        f.stage(path, b"x");
        let rules = f.rules(authority, &json!({}));
        let paths = PathPolicy::from_git(&GitPolicy::default()).unwrap();
        let report = path_policy::staged_with(&f.repo, &paths, Some(&rules)).unwrap();
        assert_eq!(
            !levels(&report, "git.ignored_files").is_empty(),
            ignored,
            "{path}: {report:?}"
        );
        f.delete(path);
    }
}

#[test]
fn a_new_scaffold_warns_over_one_mib_and_refuses_over_ten() {
    for (size, expected) in [
        (MIB, vec![]),
        (MIB + 1, vec![PolicyLevel::Warn]),
        (10 * MIB, vec![PolicyLevel::Warn]),
        (10 * MIB + 1, vec![PolicyLevel::Block]),
    ] {
        let f = Fixture::new();
        let authority = f.authority("", true, &json!({}));
        f.stage("assets/blob.bin", &big(size));
        for report in f.judge(authority, &json!({})) {
            assert_eq!(
                levels(&report, "git.max_file_bytes"),
                expected,
                "{size}: {report:?}"
            );
            assert!(report.notes.is_empty(), "{:?}", report.notes);
        }
    }
}

#[test]
fn an_updated_project_reports_both_thresholds_and_refuses_neither() {
    for size in [MIB + 1, 10 * MIB + 1] {
        let f = Fixture::new();
        let authority = f.authority("", false, &json!({}));
        f.stage("assets/blob.bin", &big(size));
        for report in f.judge(authority, &json!({})) {
            assert!(
                levels(&report, "git.max_file_bytes").is_empty(),
                "{report:?}"
            );
            let notes = report.notes.join("\n");
            assert!(notes.contains("assets/blob.bin"), "{notes}");
            assert!(notes.contains("reported only"), "{notes}");
        }
    }
}

#[test]
fn an_explicit_setting_replaces_the_default_in_either_project_kind() {
    let git = json!({"max_file_bytes": {"limit": 2 * MIB, "except_under": ["Media"]}});
    for new_scaffold in [false, true] {
        let f = Fixture::new();
        let authority = f.authority("", new_scaffold, &git);
        f.stage("data/two.bin", &big(2 * MIB + 1));
        f.stage("media/film.mov", &big(11 * MIB));
        for report in f.judge(authority, &git) {
            let text = format!("{:?}", report.violations);
            assert_eq!(
                levels(&report, "git.max_file_bytes"),
                [PolicyLevel::Block],
                "{text}"
            );
            assert!(text.contains("data/two.bin"), "{text}");
            assert!(!text.contains("film.mov"), "{text}");
            assert!(report.notes.is_empty(), "{:?}", report.notes);
        }
    }
}

#[test]
fn the_working_copy_and_the_change_cannot_relax_the_authority() {
    let f = Fixture::new();
    let authority = f.authority("build/\n", true, &json!({}));
    // The change lowers its own policy, drops the project kind and empties
    // the ignore rules; the authority still judges.
    f.stage(
        ".codeflow/policy.json",
        json!({"git": {"ignored_files": {"level": "off"},
            "max_file_bytes": {"limit": 1_000_000_000_u64}}})
        .to_string()
        .as_bytes(),
    );
    f.stage(".codeflow/project.toml", b"");
    f.stage(".gitignore", b"");
    f.stage("build/huge.bin", &big(10 * MIB + 1));
    for report in f.judge(authority, &json!({})) {
        assert_eq!(levels(&report, "git.max_file_bytes"), [PolicyLevel::Block]);
        assert_eq!(levels(&report, "git.ignored_files"), [PolicyLevel::Warn]);
    }
}

#[test]
fn the_rules_never_judge_the_gate_inputs() {
    let ignore_everything = "*\n.codeflow/\n.github/\n";
    for path in [
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
        let f = Fixture::new();
        let git = json!({"ignored_files": {"level": "block"}});
        let authority = f.authority(ignore_everything, true, &git);
        f.stage(path, &big(10 * MIB + 1));
        for report in f.judge(authority, &git) {
            assert!(
                levels(&report, "git.max_file_bytes").is_empty()
                    && levels(&report, "git.ignored_files").is_empty(),
                "{path}: {report:?}"
            );
        }
        println!("over 10 MiB and ignored, not judged: {path}");
    }
}

#[test]
fn a_size_refusal_names_the_file_its_size_the_limit_and_the_routes() {
    let f = Fixture::new();
    let authority = f.authority("", true, &json!({}));
    f.stage("models/weights.bin", &big(11 * MIB));
    let [staged, _] = f.judge(authority, &json!({}));
    let refusal = staged
        .violations
        .iter()
        .find(|v| v.rule == "git.max_file_bytes")
        .expect("refused");
    let text = refusal.render("pre-commit");
    for part in [
        "models/weights.bin",
        "11534336 bytes (11.0 MiB)",
        "over the limit of 10485760 bytes (10.0 MiB)",
        "Git LFS",
        "`limit` or `except_under` change to git.max_file_bytes",
        "on the default branch through a reviewed pull request",
        "from 4.2.0, by a per-path admitted row in the exemption register",
    ] {
        assert!(text.contains(part), "{part}: {text}");
    }
    println!("{text}");
}

#[test]
fn a_git_lfs_path_is_sized_by_its_pointer() {
    let f = Fixture::new();
    let authority = f.authority("", true, &json!({}));
    let payload = big(11 * MIB);
    let digest = {
        use sha2::Digest;
        let hash = sha2::Sha256::digest(&payload);
        hash.iter().fold(String::new(), |mut hex, b| {
            use std::fmt::Write as _;
            let _ = write!(hex, "{b:02x}");
            hex
        })
    };
    let pointer = format!(
        "version https://git-lfs.github.com/spec/v1\noid sha256:{digest}\nsize {}\n",
        payload.len()
    );
    f.stage(
        ".gitattributes",
        b"*.bin filter=lfs diff=lfs merge=lfs -text\n",
    );
    f.stage("models/weights.bin", pointer.as_bytes());
    for report in f.judge(authority, &json!({})) {
        assert!(report.violations.is_empty(), "{report:?}");
        assert!(report.notes.is_empty(), "{:?}", report.notes);
    }
}

#[test]
fn a_file_added_then_deleted_in_the_range_is_still_refused() {
    let f = Fixture::new();
    let authority = f.authority("", true, &json!({}));
    f.stage("big.bin", &big(10 * MIB + 1));
    let added = f.commit(&[authority]);
    f.delete("big.bin");
    let head = f.commit(&[added]);
    let rules = f.rules(authority, &json!({}));
    let paths = PathPolicy::from_git(&GitPolicy::default()).unwrap();
    let report = path_policy::range_with(
        f.dir.path(),
        Some(&authority.to_string()),
        &head.to_string(),
        &paths,
        Some(&rules),
    )
    .unwrap();
    assert_eq!(levels(&report, "git.max_file_bytes"), [PolicyLevel::Block]);
}

#[test]
fn the_rules_run_when_the_path_gate_is_off() {
    let git = json!({"path_policy": "off"});
    let f = Fixture::new();
    let authority = f.authority("", true, &git);
    f.stage("big.bin", &big(10 * MIB + 1));
    let rules = f.rules(authority, &git);
    let paths = PathPolicy::from_git(&serde_json::from_value(git).unwrap()).unwrap();
    let report = path_policy::staged_with(&f.repo, &paths, Some(&rules)).unwrap();
    assert_eq!(levels(&report, "git.max_file_bytes"), [PolicyLevel::Block]);
}

#[test]
fn an_unreadable_ignore_file_is_reported_at_the_rule_level() {
    for (git, level) in [
        (json!({}), PolicyLevel::Warn),
        (
            json!({"ignored_files": {"level": "block"}}),
            PolicyLevel::Block,
        ),
    ] {
        let f = Fixture::new();
        f.stage("sub/.gitignore", b"\xff\xfe not text\n");
        let authority = f.authority("", false, &git);
        f.stage("src/lib.rs", b"");
        let [staged, _] = f.judge(authority, &git);
        assert_eq!(levels(&staged, "git.ignored_files"), [level], "{staged:?}");
        assert!(format!("{:?}", staged.violations).contains("sub/.gitignore is not UTF-8"));
    }
}

/// Review 01, finding 1: a clean automatic merge can record a blob neither
/// parent holds, and that blob is sized like any other new file.
#[test]
fn a_clean_merge_that_produces_an_oversized_file_is_refused() {
    let f = Fixture::new();
    let line = |byte: u8, len: usize| {
        let mut line = vec![byte; len - 1];
        line.push(b'\n');
        line
    };
    // 8 MiB of 1 KiB lines; each side widens one line far from the other.
    let mut lines: Vec<Vec<u8>> = (0..8192).map(|_| line(b'x', 1024)).collect();
    f.stage("data.txt", &lines.concat());
    let authority = f.authority("", true, &json!({}));
    let wide = MIB + MIB / 2;
    lines[10] = line(b'a', wide);
    f.stage("data.txt", &lines.concat());
    let ours = f.commit(&[authority]);
    lines[10] = line(b'x', 1024);
    lines[8000] = line(b'b', wide);
    f.stage("data.txt", &lines.concat());
    let theirs = f.commit(&[authority]);
    let (ours_commit, theirs_commit) = (
        f.repo.find_commit(ours).unwrap(),
        f.repo.find_commit(theirs).unwrap(),
    );
    let mut merged = f
        .repo
        .merge_commits(&ours_commit, &theirs_commit, None)
        .unwrap();
    assert!(!merged.has_conflicts());
    let tree = f
        .repo
        .find_tree(merged.write_tree_to(&f.repo).unwrap())
        .unwrap();
    let sig = git2::Signature::now("Tester", "test@example.invalid").unwrap();
    let merge = f
        .repo
        .commit(
            None,
            &sig,
            &sig,
            "chore(merge): join both widenings",
            &tree,
            &[&ours_commit, &theirs_commit],
        )
        .unwrap();
    let size = tree
        .get_name("data.txt")
        .unwrap()
        .to_object(&f.repo)
        .unwrap();
    let size = size.as_blob().unwrap().size();
    assert!(size > 10 * MIB, "the merged file is {size} bytes");
    let rules = f.rules(authority, &json!({}));
    let paths = PathPolicy::from_git(&GitPolicy::default()).unwrap();
    let report = path_policy::range_with(
        f.dir.path(),
        Some(&authority.to_string()),
        &merge.to_string(),
        &paths,
        Some(&rules),
    )
    .unwrap();
    // Each parent warns; the merge's own blob is over the limit.
    let mut found = levels(&report, "git.max_file_bytes");
    found.sort_by_key(|level| format!("{level:?}"));
    assert_eq!(
        found,
        [PolicyLevel::Block, PolicyLevel::Warn, PolicyLevel::Warn],
        "{report:?}"
    );
    assert!(format!("{:?}", report.violations).contains("file too large: data.txt"));
}

/// Review 01, finding 2: git skips a leading byte order mark, and patterns
/// compare in the same composed (NFC) spelling as paths.
#[test]
fn a_byte_order_mark_and_decomposed_patterns_match_as_git_reads_them() {
    for (path, ignored) in [
        ("build/out.o", true),
        ("BUILD/x.o", true),
        ("sub/caf\u{e9}.dat", true),
        ("sub/cafe\u{301}.dat", true),
        ("sub/draft.tmp", true),
        ("sub/\u{e9}t\u{e9}.tmp", false),
        ("sub/e\u{301}te\u{301}.tmp", false),
        ("sub/other.dat", false),
    ] {
        let f = Fixture::new();
        f.stage(
            "sub/.gitignore",
            "\u{feff}cafe\u{301}.dat\n*.tmp\n!e\u{301}te\u{301}.tmp\n".as_bytes(),
        );
        let authority = f.authority("\u{feff}build/\n", false, &json!({}));
        f.stage(path, b"x");
        for report in f.judge(authority, &json!({})) {
            assert_eq!(
                !levels(&report, "git.ignored_files").is_empty(),
                ignored,
                "{path}: {report:?}"
            );
        }
    }
}

/// Review 01, finding 3, under ADR-0091: no default branch is guessed.
/// With `main` and `master` fetched and no remote HEAD the rules refuse and
/// name the repair; once origin/HEAD names `main`, main's tree and policy
/// alone judge, and master's stricter limit is never read.
#[test]
fn only_the_recorded_default_branch_judges_and_none_is_guessed() {
    let f = Fixture::new();
    f.repo
        .remote("origin", "https://example.invalid/adopter.git")
        .unwrap();
    let main = f.authority(
        "*.bin\n",
        false,
        &json!({"ignored_files": {"level": "block"},
            "max_file_bytes": {"limit": 5 * MIB}}),
    );
    f.stage("sub/.gitignore", b"!payload.bin\n");
    let master = f.authority(
        "",
        false,
        &json!({"ignored_files": {"level": "warn"},
            "max_file_bytes": {"limit": 3 * MIB}}),
    );
    for (name, id) in [("main", main), ("master", master)] {
        f.repo
            .reference(&format!("refs/remotes/origin/{name}"), id, true, "fixture")
            .unwrap();
    }
    f.repo
        .reference("refs/heads/feat/content", main, true, "fixture")
        .unwrap();
    let mut index = f.repo.index().unwrap();
    index
        .read_tree(&f.repo.find_commit(main).unwrap().tree().unwrap())
        .unwrap();
    index.write().unwrap();
    f.stage("sub/payload.bin", &big(4 * MIB));
    let error =
        codeflow_core::hooks::landed_policy::staged_rules(f.dir.path(), &GitPolicy::default())
            .err()
            .expect("an unset origin/HEAD refuses");
    assert!(error.contains("git remote set-head origin -a"), "{error}");
    f.repo
        .reference_symbolic(
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
            true,
            "fixture",
        )
        .unwrap();
    let (paths, rules) =
        codeflow_core::hooks::landed_policy::staged_rules(f.dir.path(), &GitPolicy::default())
            .unwrap();
    let report = path_policy::staged_with(&f.repo, &paths, Some(&rules)).unwrap();
    assert_eq!(
        levels(&report, "git.ignored_files"),
        [PolicyLevel::Block],
        "{report:?}"
    );
    assert!(
        !levels(&report, "git.max_file_bytes").contains(&PolicyLevel::Block),
        "master's 3 MiB limit was read: {report:?}"
    );
}

/// Review 01, finding 4: `codeflow ci` reads the policy keys at the
/// authority commit itself, and the shipped defaults where it has none.
#[test]
fn the_authority_commits_own_policy_or_the_defaults_judge() {
    let f = Fixture::new();
    f.stage(".codeflow/project.toml", b"file_size_rule = \"refuse\"\n");
    f.stage("readme.md", b"no policy file here");
    let bare = f.commit(&[]);
    let rules = ContentRules::at_authority(f.dir.path(), &bare.to_string()).unwrap();
    assert!(rules.refuses());
    let authority = f.authority("", true, &json!({"max_file_bytes": {"limit": 2 * MIB}}));
    f.stage("data/three.bin", &big(3 * MIB));
    let paths = PathPolicy::from_git(&GitPolicy::default()).unwrap();
    for (at, expected) in [(bare, PolicyLevel::Warn), (authority, PolicyLevel::Block)] {
        let rules = ContentRules::at_authority(f.dir.path(), &at.to_string()).unwrap();
        let report = path_policy::staged_with(&f.repo, &paths, Some(&rules)).unwrap();
        assert_eq!(
            levels(&report, "git.max_file_bytes"),
            [expected],
            "{report:?}"
        );
    }
}

/// Review 01, finding 6: `except_under` names directories; a prefix that is
/// a file's own path does not exempt that file.
#[test]
fn except_under_exempts_directories_and_never_one_named_file() {
    let git = json!({"max_file_bytes": {"except_under": ["models/weights.bin", "Media"]}});
    let f = Fixture::new();
    let authority = f.authority("", false, &git);
    f.stage("models/weights.bin", &big(10 * MIB + 1));
    f.stage("media/film.mov", &big(10 * MIB + 1));
    for report in f.judge(authority, &git) {
        let text = format!("{:?}", report.violations);
        assert_eq!(
            levels(&report, "git.max_file_bytes"),
            [PolicyLevel::Block],
            "{text}"
        );
        assert!(text.contains("models/weights.bin"), "{text}");
        assert!(!text.contains("film.mov"), "{text}");
    }
}

/// Review 02, N1: a hand resolution that takes one parent's blob is judged,
/// though that parent holds it, whether the path gate is on or off.
#[test]
fn a_hand_resolution_that_restores_a_parents_blob_is_judged() {
    for level in ["block", "off"] {
        let git = json!({"path_policy": level});
        let f = Fixture::new();
        // The ancestor holds an 11 MiB file and an ignored log.
        f.stage("data.bin", &big(11 * MIB));
        f.stage("app.log", b"log");
        let ancestor = f.authority("*.log\n", true, &git);
        // The authority shrinks the file and deletes the log.
        f.stage("data.bin", &big(MIB));
        f.delete("app.log");
        let authority = f.commit(&[ancestor]);
        // A side branch from the ancestor changes only its readme.
        let mut index = f.repo.index().unwrap();
        index
            .read_tree(&f.repo.find_commit(ancestor).unwrap().tree().unwrap())
            .unwrap();
        index.write().unwrap();
        f.stage("README.md", b"side");
        let side = f.commit(&[ancestor]);
        // The merge records the side's tree: the old file and the log.
        let merge = f.commit(&[authority, side]);
        let rules = f.rules(authority, &git);
        let paths = PathPolicy::from_git(&serde_json::from_value(git.clone()).unwrap()).unwrap();
        let report = path_policy::range_with(
            f.dir.path(),
            Some(&authority.to_string()),
            &merge.to_string(),
            &paths,
            Some(&rules),
        )
        .unwrap();
        let text = format!("{:?}", report.violations);
        assert_eq!(
            levels(&report, "git.max_file_bytes"),
            [PolicyLevel::Block],
            "{level}: {text}"
        );
        assert!(text.contains("file too large: data.bin"), "{level}: {text}");
        assert_eq!(
            levels(&report, "git.ignored_files"),
            [PolicyLevel::Warn],
            "{level}: {text}"
        );
        assert!(text.contains("app.log"), "{level}: {text}");
    }
}

/// Review 02, N2: a content setting the schema refuses at the authority
/// stops the check in pre-commit and CI, never reading as an absent key.
#[test]
fn a_schema_invalid_authority_setting_stops_the_check() {
    for git in [
        json!({"max_file_bytes": null}),
        json!({"ignored_files": null}),
        json!({"max_file_byte": {"limit": 5}}),
        json!({"ignored_files": {"level": "block", "alow": []}}),
    ] {
        let f = Fixture::new();
        let authority = f.authority("", false, &git);
        f.stage("data/eleven.bin", &big(11 * MIB));
        let error = ContentRules::at_authority(f.dir.path(), &authority.to_string())
            .err()
            .unwrap_or_else(|| panic!("{git} was read as valid"));
        assert!(error.contains("invalid policy at"), "{git}: {error}");
        // Before the first fetch pre-commit reads HEAD, the same commit.
        let error =
            codeflow_core::hooks::landed_policy::staged_rules(f.dir.path(), &GitPolicy::default())
                .err()
                .unwrap_or_else(|| panic!("{git} was read as valid in pre-commit"));
        assert!(error.contains("invalid policy at"), "{git}: {error}");
    }
}
