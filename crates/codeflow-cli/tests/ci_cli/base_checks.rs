//! TSK-1103: `codeflow ci base-checks` runs the checks the trusted commit
//! declares in `.codeflow/base-checks.json` from that commit's own copies,
//! so a head that edits the check, its inputs, the setup hook, the
//! declaration or the workflows still gets the trusted commit's decision;
//! it fails closed on every declared file it cannot run, binds trust to the
//! commit `--trusted` names, and hands the script neither the caller's
//! environment nor the repository's configuration.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::{codeflow, git_command};

const DECLARATION: &str = ".codeflow/base-checks.json";
const SCRIPT: &str = ".codeflow/checks/boundary.sh";
const BOUNDARY: &str = "docs/boundary.txt";

/// The reporter's shape (issue 437): refuse a change that adds a path under
/// a prefix the boundary file lists. It reads the head only as git data.
const BOUNDARY_CHECK: &str = r#"set -eu
git diff --name-only --diff-filter=A "$1" "$2" > "$TMPDIR/added"
while IFS= read -r prefix; do
  [ -n "$prefix" ] || continue
  if grep -q "^$prefix" "$TMPDIR/added"; then
    echo "boundary: this change adds a file under $prefix"
    exit 1
  fi
done < docs/boundary.txt
echo "boundary: holds"
"#;

fn declaration(script: &str, interpreter: &str, inputs: &[&str], timeout: Option<u64>) -> String {
    let inputs = inputs
        .iter()
        .map(|input| format!("\"{input}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let timeout = timeout.map_or(String::new(), |seconds| {
        format!(", \"timeout_seconds\": {seconds}")
    });
    format!(
        "{{\"version\": 1, \"checks\": [{{\"name\": \"boundary\", \"script\": \"{script}\", \"interpreter\": \"{interpreter}\", \"inputs\": [{inputs}]{timeout}}}]}}\n"
    )
}

pub(super) struct Repo {
    _dir: tempfile::TempDir,
    pub(super) root: PathBuf,
}

impl Repo {
    pub(super) fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("repo");
        std::fs::create_dir_all(&root).unwrap();
        let repo = Self { _dir: dir, root };
        repo.git(&["init", "-q", "-b", "main"]);
        repo.git(&["config", "user.name", "Fixture"]);
        repo.git(&["config", "user.email", "fixture@example.com"]);
        repo.git(&["config", "commit.gpgsign", "false"]);
        repo
    }

    /// The base every fixture starts from: the boundary check, its input,
    /// a setup hook and the managed workflows that run it.
    fn with_boundary() -> Self {
        let repo = Self::new();
        repo.write(SCRIPT, BOUNDARY_CHECK);
        repo.write(BOUNDARY, "vendor/\n");
        repo.write(
            DECLARATION,
            &declaration(SCRIPT, "bash", &[BOUNDARY], Some(60)),
        );
        repo.write(".codeflow/ci-setup.sh", "export TOOL=1\n");
        repo.write(
            ".github/workflows/codeflow-policy.yml",
            include_str!("../../../../assets/base/ci/codeflow-policy.yml"),
        );
        repo.write(
            ".github/workflows/codeflow-ci.yml",
            include_str!("../../../../assets/base/ci/codeflow-ci.yml"),
        );
        repo.write("src/lib.rs", "// lib\n");
        repo.commit("chore: seed");
        repo
    }

    pub(super) fn path(&self, path: &str) -> PathBuf {
        self.root.join(path)
    }

    pub(super) fn write(&self, path: &str, text: &str) {
        let file = self.path(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let _ = std::fs::remove_file(&file);
        std::fs::write(file, text).unwrap();
    }

    pub(super) fn git(&self, args: &[&str]) -> String {
        let out = git_command(&self.root)
            .env("GIT_TERMINAL_PROMPT", "0")
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    pub(super) fn commit(&self, message: &str) -> String {
        self.git(&["add", "-A"]);
        self.git(&[
            "commit",
            "-q",
            "--no-verify",
            "--allow-empty",
            "-m",
            message,
        ]);
        self.git(&["rev-parse", "HEAD"])
    }

    /// Run the checks; the exit code and everything printed.
    pub(super) fn checks(
        &self,
        trusted: &str,
        base: &str,
        head: &str,
        extra: &[&str],
    ) -> (i32, String) {
        let out = codeflow()
            .args([
                "ci",
                "base-checks",
                "--trusted",
                trusted,
                "--base",
                base,
                "--head",
                head,
            ])
            .args(extra)
            .current_dir(&self.root)
            .output()
            .unwrap();
        (
            out.status.code().unwrap_or(-1),
            format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ),
        )
    }
}

fn assert_refused(result: &(i32, String), needles: &[&str]) {
    assert_ne!(result.0, 0, "{}", result.1);
    for needle in needles {
        assert!(
            result.1.contains(needle),
            "missing {needle:?}:\n{}",
            result.1
        );
    }
}

/// AC-1, issue 437: a clean change passes, one that adds a file the
/// boundary forbids fails, and the same change still fails when its head
/// rewrites the check to pass, empties the boundary file, edits the setup
/// hook, drops the check from its declaration and deletes the step from its
/// workflows (AC-9).
#[test]
fn the_trusted_commit_decides_whatever_the_head_edits() {
    let repo = Repo::with_boundary();
    repo.git(&["switch", "-q", "-c", "feat/clean"]);
    repo.write("src/more.rs", "// more\n");
    let clean = repo.commit("feat: a clean change");
    let result = repo.checks("main", "main", &clean, &[]);
    assert_eq!(result.0, 0, "{}", result.1);
    assert!(result.1.contains("boundary passed"), "{}", result.1);

    repo.git(&["switch", "-q", "-c", "feat/vendor", "main"]);
    repo.write("vendor/lib.rs", "// copied in\n");
    let bad = repo.commit("feat: vendor a library");
    assert_refused(
        &repo.checks("main", "main", &bad, &[]),
        &[
            "boundary: this change adds a file under vendor/",
            "check boundary",
            DECLARATION,
        ],
    );

    repo.write(SCRIPT, "exit 0\n");
    repo.write(BOUNDARY, "");
    repo.write(".codeflow/ci-setup.sh", "exit 0\n");
    repo.write(DECLARATION, "{\"version\": 1}\n");
    for workflow in [
        ".github/workflows/codeflow-policy.yml",
        ".github/workflows/codeflow-ci.yml",
    ] {
        let text = std::fs::read_to_string(repo.path(workflow)).unwrap();
        repo.write(workflow, &text.replace("codeflow ci base-checks", "true"));
    }
    let rewritten = repo.commit("ci: make every check pass");
    assert_refused(
        &repo.checks("main", "main", &rewritten, &[]),
        &["boundary: this change adds a file under vendor/"],
    );
}

/// AC-1, issue 510: a check that reads its checker with a git show of the
/// trusted commit fails the run, and a head that rewrites the checker to
/// pass changes nothing.
#[test]
fn a_checker_read_from_the_trusted_commit_cannot_be_replaced() {
    let repo = Repo::new();
    repo.write(
        "tools/check.py",
        "import subprocess, sys\nadded = subprocess.run(['git', 'diff', '--name-only', '--diff-filter=A', sys.argv[1], sys.argv[2]], capture_output=True, text=True, check=True).stdout.split()\nbad = [p for p in added if p.startswith('generated/')]\nprint('generated files edited by hand:', bad) if bad else None\nsys.exit(1 if bad else 0)\n",
    );
    repo.write(
        ".codeflow/checks/generated.sh",
        "set -eu\ngit show \"$CODEFLOW_TRUSTED_SHA:tools/check.py\" > \"$TMPDIR/check.py\"\npython3 \"$TMPDIR/check.py\" \"$1\" \"$2\"\n",
    );
    repo.write(
        DECLARATION,
        &declaration(".codeflow/checks/generated.sh", "sh", &[], None),
    );
    repo.commit("chore: seed");
    repo.git(&["switch", "-q", "-c", "feat/x"]);
    repo.write("generated/api.rs", "// by hand\n");
    repo.write("tools/check.py", "import sys\nsys.exit(0)\n");
    let head = repo.commit("feat: edit generated code and the checker");
    assert_refused(
        &repo.checks("main", "main", &head, &[]),
        &[
            "generated files edited by hand: ['generated/api.rs']",
            ".codeflow/checks/generated.sh",
        ],
    );
}

/// AC-3: trust is the commit `--trusted` names. A pull request into an
/// integration line whose base carries a weakened check is judged by the
/// default branch's copy.
#[test]
fn the_default_branch_copy_decides_for_a_line() {
    let repo = Repo::with_boundary();
    repo.git(&["switch", "-q", "-c", "integration/EPC-001-line"]);
    repo.write(SCRIPT, "echo the line passes everything\n");
    let line = repo.commit("chore: weaken the check on the line");
    repo.git(&["switch", "-q", "-c", "feat/x"]);
    repo.write("vendor/lib.rs", "// copied in\n");
    let head = repo.commit("feat: vendor a library");
    assert_refused(
        &repo.checks("main", &line, &head, &[]),
        &["boundary: this change adds a file under vendor/"],
    );
    // The line's own copy would pass it: the trusted commit is what differs.
    let line_trusted = repo.checks(&line, &line, &head, &[]);
    assert_eq!(line_trusted.0, 0, "{}", line_trusted.1);
}

/// How a fail-closed case breaks the trusted commit.
type Breaks = Box<dyn Fn(&Repo)>;

/// AC-2: each declared file the step cannot run fails it, naming the
/// declaration and the path.
#[test]
fn the_step_fails_closed() {
    let cases: Vec<(&str, Breaks, Vec<&str>)> = vec![
        (
            "a missing script",
            Box::new(|repo: &Repo| {
                std::fs::remove_file(repo.path(SCRIPT)).unwrap();
            }),
            vec![SCRIPT, "is missing on the trusted commit"],
        ),
        (
            "a script that is a symbolic link",
            Box::new(|repo: &Repo| {
                repo.write(".codeflow/checks/real.sh", "exit 0\n");
                std::fs::remove_file(repo.path(SCRIPT)).unwrap();
                std::os::unix::fs::symlink("real.sh", repo.path(SCRIPT)).unwrap();
            }),
            vec![SCRIPT, "is a symbolic link on the trusted commit"],
        ),
        (
            "an input that is a symbolic link",
            Box::new(|repo: &Repo| {
                std::fs::remove_file(repo.path(BOUNDARY)).unwrap();
                std::os::unix::fs::symlink("/etc/hosts", repo.path(BOUNDARY)).unwrap();
            }),
            vec![BOUNDARY, "is a symbolic link on the trusted commit"],
        ),
        (
            "a missing input",
            Box::new(|repo: &Repo| {
                std::fs::remove_file(repo.path(BOUNDARY)).unwrap();
            }),
            vec![BOUNDARY, "is missing on the trusted commit"],
        ),
        (
            "an interpreter not on PATH",
            Box::new(|repo: &Repo| {
                repo.write(
                    DECLARATION,
                    &declaration(SCRIPT, "codeflow-no-such-shell", &[BOUNDARY], None),
                );
            }),
            vec![
                SCRIPT,
                "its interpreter codeflow-no-such-shell is not on PATH",
            ],
        ),
        (
            "a declaration that cannot be parsed",
            Box::new(|repo: &Repo| {
                repo.write(DECLARATION, "{\"version\": 1, \"checks\": [}\n");
            }),
            vec![DECLARATION, "cannot be read"],
        ),
        (
            "a declaration that is a symbolic link",
            Box::new(|repo: &Repo| {
                repo.write(".codeflow/other.json", "{\"version\": 1}\n");
                std::fs::remove_file(repo.path(DECLARATION)).unwrap();
                std::os::unix::fs::symlink("other.json", repo.path(DECLARATION)).unwrap();
            }),
            vec![DECLARATION, "is not a regular file (a symbolic link)"],
        ),
    ];
    for (case, breaks, needles) in cases {
        let repo = Repo::with_boundary();
        breaks(&repo);
        let trusted = repo.commit("chore: break the check");
        let result = repo.checks(&trusted, &trusted, &trusted, &[]);
        assert_ne!(result.0, 0, "{case}: {}", result.1);
        for needle in needles {
            assert!(
                result.1.contains(needle),
                "{case}: missing {needle:?}:\n{}",
                result.1
            );
        }
        if case != "a declaration that cannot be parsed"
            && case != "a declaration that is a symbolic link"
        {
            assert!(result.1.contains(DECLARATION), "{case}: {}", result.1);
        }
    }
}

/// AC-2: a gitlink or an unreadable object in the script's place, and a
/// named commit that does not resolve, fail closed too.
#[test]
fn the_step_refuses_gitlinks_unreadable_objects_and_unknown_commits() {
    // A gitlink in the script's place.
    let repo = Repo::with_boundary();
    let seed = repo.git(&["rev-parse", "HEAD"]);
    repo.git(&["rm", "-q", "--cached", SCRIPT]);
    std::fs::remove_file(repo.path(SCRIPT)).unwrap();
    repo.git(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("160000,{seed},{SCRIPT}"),
    ]);
    repo.git(&["commit", "-q", "--no-verify", "-m", "chore: a gitlink"]);
    assert_refused(
        &repo.checks("HEAD", "HEAD", "HEAD", &[]),
        &[
            SCRIPT,
            "is a gitlink (submodule) on the trusted commit",
            DECLARATION,
        ],
    );

    // A script whose object cannot be read.
    let repo = Repo::with_boundary();
    let blob = repo.git(&["rev-parse", &format!("HEAD:{SCRIPT}")]);
    std::fs::remove_file(repo.path(&format!(".git/objects/{}/{}", &blob[..2], &blob[2..])))
        .unwrap();
    assert_refused(
        &repo.checks("HEAD", "HEAD", "HEAD", &[]),
        &[SCRIPT, "cannot be read on the trusted commit", DECLARATION],
    );

    // A named commit that does not resolve judges nothing.
    let repo = Repo::with_boundary();
    let result = repo.checks("no-such-ref", "HEAD", "HEAD", &[]);
    assert_eq!(result.0, 2, "{}", result.1);
    for needle in [
        "the trusted commit no-such-ref cannot be resolved",
        "nothing is judged",
        "git fetch",
    ] {
        assert!(
            result.1.contains(needle),
            "missing {needle:?}:\n{}",
            result.1
        );
    }
}

/// AC-2: a file in a declared input folder whose name is not UTF-8 fails
/// the check naming that, never as a file it could not write.
#[cfg(unix)]
#[test]
fn an_input_folder_name_that_is_not_utf8_fails_naming_it() {
    use std::os::unix::ffi::OsStrExt;

    let repo = Repo::with_boundary();
    repo.write(
        DECLARATION,
        &declaration(SCRIPT, "bash", &["docs/inputs"], Some(60)),
    );
    repo.write("docs/inputs/plain.txt", "plain\n");
    repo.commit("chore: an input folder");
    let blob = repo.git(&["rev-parse", "HEAD:docs/inputs/plain.txt"]);
    let mut entry = b"100644,".to_vec();
    entry.extend_from_slice(blob.as_bytes());
    entry.extend_from_slice(b",docs/inputs/bad-\xff.txt");
    let out = git_command(&repo.root)
        .args(["update-index", "--add", "--cacheinfo"])
        .arg(std::ffi::OsStr::from_bytes(&entry))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    repo.git(&["commit", "-q", "--no-verify", "-m", "chore: a raw name"]);
    assert_refused(
        &repo.checks("HEAD", "HEAD", "HEAD", &[]),
        &[
            "docs/inputs/",
            "has a name that is not UTF-8 on the trusted commit",
            DECLARATION,
        ],
    );
}

/// AC-2: a folder entry in a declared input folder whose name is not UTF-8
/// fails the check before it runs, whatever the folder holds. The trusted
/// commit gets that entry through git2, which writes the raw name and can
/// hold an empty folder; `fill` adds the folder's own entries.
fn assert_raw_folder_refused(fill: impl FnOnce(&git2::Repository, &mut git2::TreeBuilder<'_>)) {
    let repo = Repo::with_boundary();
    repo.write(
        DECLARATION,
        &declaration(SCRIPT, "bash", &[BOUNDARY, "docs/inputs"], Some(60)),
    );
    repo.write("docs/inputs/plain.txt", "plain\n");
    repo.commit("chore: an input folder");
    let clean = repo.checks("HEAD", "HEAD", "HEAD", &[]);
    assert_eq!(clean.0, 0, "{}", clean.1);

    let git = git2::Repository::open(&repo.root).unwrap();
    let head = git.head().unwrap().peel_to_commit().unwrap();
    let root = head.tree().unwrap();
    let subtree = |path: &str| {
        git.find_tree(root.get_path(std::path::Path::new(path)).unwrap().id())
            .unwrap()
    };
    let mut raw = git.treebuilder(None).unwrap();
    fill(&git, &mut raw);
    let raw = raw.write().unwrap();
    let mut inputs = git.treebuilder(Some(&subtree("docs/inputs"))).unwrap();
    inputs.insert(&b"raw-\xff"[..], raw, 0o040_000).unwrap();
    let inputs = inputs.write().unwrap();
    let mut docs = git.treebuilder(Some(&subtree("docs"))).unwrap();
    docs.insert("inputs", inputs, 0o040_000).unwrap();
    let docs = docs.write().unwrap();
    let mut top = git.treebuilder(Some(&root)).unwrap();
    top.insert("docs", docs, 0o040_000).unwrap();
    let tree = git.find_tree(top.write().unwrap()).unwrap();
    let signature = git2::Signature::now("Fixture", "fixture@example.com").unwrap();
    git.commit(
        Some("HEAD"),
        &signature,
        &signature,
        "chore: a raw folder name",
        &tree,
        &[&head],
    )
    .unwrap();

    let result = repo.checks("HEAD", "HEAD", "HEAD", &[]);
    assert_refused(
        &result,
        &[
            "docs/inputs/",
            "has a name that is not UTF-8 on the trusted commit",
            DECLARATION,
        ],
    );
    assert!(!result.1.contains("boundary: holds"), "{}", result.1);
}

#[test]
fn an_input_subfolder_name_that_is_not_utf8_fails_with_a_file_in_it() {
    assert_raw_folder_refused(|git, raw| {
        let blob = git.blob(b"child\n").unwrap();
        raw.insert("child.txt", blob, 0o100_644).unwrap();
    });
}

#[test]
fn an_input_subfolder_name_that_is_not_utf8_fails_with_a_link_in_it() {
    assert_raw_folder_refused(|git, raw| {
        let blob = git.blob(b"../plain.txt").unwrap();
        raw.insert("link", blob, 0o120_000).unwrap();
    });
}

#[test]
fn an_empty_input_subfolder_name_that_is_not_utf8_fails() {
    assert_raw_folder_refused(|_, _| {});
}

/// AC-2: a check past its timeout is stopped, with everything it started,
/// and fails naming the timeout.
#[test]
fn a_check_past_its_timeout_is_stopped_with_its_children() {
    let repo = Repo::with_boundary();
    let pid_file = repo.root.parent().unwrap().join("child.pid");
    repo.write(
        SCRIPT,
        &format!(
            "sleep 120 &\necho $! > '{}'\nsleep 120\n",
            pid_file.display()
        ),
    );
    repo.write(DECLARATION, &declaration(SCRIPT, "sh", &[], Some(1)));
    let trusted = repo.commit("chore: a slow check");
    let started = Instant::now();
    let result = repo.checks(&trusted, &trusted, &trusted, &[]);
    assert!(
        started.elapsed() < Duration::from_secs(30),
        "{:?}",
        started.elapsed()
    );
    assert_refused(
        &result,
        &[SCRIPT, "ran past its timeout of 1 seconds and was stopped"],
    );
    let pid = std::fs::read_to_string(&pid_file).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    let alive = std::process::Command::new("kill")
        .args(["-0", pid.trim()])
        .status()
        .unwrap()
        .success();
    assert!(!alive, "the check's child {pid} outlived the timeout");
}

/// The script gets the commits as arguments and the head as git data, but
/// neither the caller's environment (a token) nor the repository's
/// configuration (a persisted credential header).
#[test]
fn the_check_sees_the_commits_and_no_credential() {
    let repo = Repo::with_boundary();
    repo.git(&[
        "config",
        "http.https://github.com/.extraheader",
        "AUTHORIZATION: basic c2VjcmV0LWhlYWRlcg==",
    ]);
    repo.write(
        SCRIPT,
        "echo \"args=$# base=$1 head=$2 merge=[$3] repo=[$4]\"\nenv | sort\ngit config --list --show-origin || true\necho \"head file: $(git show \"$2:src/head.rs\")\"\necho \"trusted ref: $(git rev-parse refs/codeflow/trusted)\"\necho \"cwd holds: $(find . -type f | sort | tr '\\n' ' ')\"\n",
    );
    let trusted = repo.commit("chore: a reporting check");
    repo.git(&["switch", "-q", "-c", "feat/x"]);
    repo.write("src/head.rs", "// from the head\n");
    let head = repo.commit("feat: a head file");
    let out = codeflow()
        .args([
            "ci",
            "base-checks",
            "--trusted",
            &trusted,
            "--base",
            &trusted,
            "--head",
            &head,
            "--head-repo",
            "fork/repo",
        ])
        .env("GITHUB_TOKEN", "ghs_secret_token_value")
        .env("GH_TOKEN", "ghs_secret_token_value")
        .env("ACTIONS_RUNTIME_TOKEN", "ghs_secret_token_value")
        .current_dir(&repo.root)
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "{text}");
    for needle in [
        format!("args=4 base={trusted} head={head} merge=[] repo=[fork/repo]"),
        format!("CODEFLOW_HEAD_SHA={head}"),
        format!("CODEFLOW_TRUSTED_SHA={trusted}"),
        "CODEFLOW_HEAD_REPO=fork/repo".to_string(),
        "head file: // from the head".to_string(),
        format!("trusted ref: {trusted}"),
        "cwd holds: ./.codeflow/checks/boundary.sh ./docs/boundary.txt ".to_string(),
    ] {
        assert!(text.contains(&needle), "missing {needle:?}:\n{text}");
    }
    for leak in [
        "ghs_secret_token_value",
        "c2VjcmV0LWhlYWRlcg",
        "extraheader",
        "GITHUB_TOKEN",
        "GH_TOKEN",
    ] {
        assert!(!text.contains(leak), "{leak} reached the check:\n{text}");
    }
}

/// A trusted commit without a declaration, or with no checks, passes with
/// a line that says so.
#[test]
fn no_declaration_passes_and_says_so() {
    let repo = Repo::new();
    repo.write("README.md", "readme\n");
    let trusted = repo.commit("chore: seed");
    let result = repo.checks(&trusted, &trusted, &trusted, &[]);
    assert_eq!(result.0, 0, "{}", result.1);
    assert!(
        result
            .1
            .contains("declares no checks (.codeflow/base-checks.json is absent)"),
        "{}",
        result.1
    );
    repo.write(DECLARATION, "{\"version\": 1, \"checks\": []}\n");
    let trusted = repo.commit("chore: an empty declaration");
    let result = repo.checks(&trusted, &trusted, &trusted, &[]);
    assert_eq!(result.0, 0, "{}", result.1);
}

/// `codeflow ci` over `main..HEAD`: everything printed.
fn ci_notes(repo: &Repo) -> String {
    let out = codeflow()
        .args([
            "ci", "--base", "main", "--head", "HEAD", "--branch", "feat/x",
        ])
        .current_dir(&repo.root)
        .output()
        .unwrap();
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// AC-14's adjacent cases: a rename names the old path removed and the new
/// one added; a file replaced by a symbolic link with the same text is an
/// edit; several changed inputs print one note each; a head tree that
/// cannot be read is named unknown, never read as a removal.
#[test]
fn the_notes_name_renames_links_and_each_changed_input() {
    let repo = Repo::with_boundary();
    repo.git(&["switch", "-q", "-c", "feat/x"]);
    repo.git(&["mv", SCRIPT, ".codeflow/checks/edges.sh"]);
    repo.write(
        DECLARATION,
        &declaration(".codeflow/checks/edges.sh", "bash", &[BOUNDARY], Some(60)),
    );
    repo.commit("ci: rename the check");
    let notes = ci_notes(&repo);
    for needle in [
        format!("this change removes {SCRIPT}"),
        "this change adds .codeflow/checks/edges.sh".to_string(),
        format!("this change edits {DECLARATION}"),
    ] {
        assert!(notes.contains(&needle), "missing {needle:?}:\n{notes}");
    }

    let repo = Repo::new();
    repo.write("docs/a.txt", "x.txt");
    repo.write("docs/b.txt", "b\n");
    repo.write(SCRIPT, "exit 0\n");
    repo.write(
        DECLARATION,
        &declaration(SCRIPT, "sh", &["docs/a.txt", "docs/b.txt"], None),
    );
    repo.commit("chore: seed");
    repo.git(&["switch", "-q", "-c", "feat/x"]);
    std::fs::remove_file(repo.path("docs/a.txt")).unwrap();
    std::os::unix::fs::symlink("x.txt", repo.path("docs/a.txt")).unwrap();
    repo.write("docs/b.txt", "b2\n");
    repo.commit("ci: change both inputs");
    assert_eq!(
        repo.git(&["rev-parse", "main:docs/a.txt"]),
        repo.git(&["rev-parse", "HEAD:docs/a.txt"]),
        "the link must keep the file's blob for this case"
    );
    let notes = ci_notes(&repo);
    for path in ["docs/a.txt", "docs/b.txt"] {
        let needle = format!("this change edits {path}, a file a trusted-base check declared in .codeflow/base-checks.json runs from");
        assert_eq!(notes.matches(&needle).count(), 1, "{needle:?}:\n{notes}");
    }

    let tree = repo.git(&["rev-parse", "HEAD:docs"]);
    std::fs::remove_file(repo.path(&format!(".git/objects/{}/{}", &tree[..2], &tree[2..])))
        .unwrap();
    let notes = ci_notes(&repo);
    assert!(
        notes.contains("whether this change edits docs/a.txt"),
        "{notes}"
    );
    assert!(notes.contains("cannot be told"), "{notes}");
    assert!(!notes.contains("this change removes docs/a.txt"), "{notes}");
}

/// The subcommand does not mix with the range judgment's flags.
#[test]
fn the_subcommand_refuses_range_flags() {
    let repo = Repo::new();
    let out = codeflow()
        .args([
            "ci",
            "--base",
            "main",
            "base-checks",
            "--trusted",
            "x",
            "--base",
            "x",
            "--head",
            "x",
        ])
        .current_dir(&repo.root)
        .output()
        .unwrap();
    assert!(!out.status.success());
}
