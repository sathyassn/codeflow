//! A registry fixture whose id check makes many guarded history queries,
//! for the tests that count snapshot reads (TSK-1520 AC-1, AC-2).

use std::path::Path;

/// A scratch repository with its own configuration, so no user signing,
/// hooks or identity reach the fixture's commits.
pub(crate) struct Repo {
    pub dir: tempfile::TempDir,
}

impl Repo {
    /// An empty `main` holding one commit.
    pub fn new() -> Repo {
        let repo = Repo {
            dir: tempfile::tempdir().unwrap(),
        };
        repo.git(&["init", "-q", "-b", "main"]);
        for (key, value) in [
            ("user.name", "fixture"),
            ("user.email", "fixture@example.test"),
            ("commit.gpgsign", "false"),
            ("tag.gpgsign", "false"),
        ] {
            repo.git(&["config", key, value]);
        }
        let hooks = repo.dir.path().join(".git/no-hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        repo.git(&["config", "core.hooksPath", hooks.to_str().unwrap()]);
        repo.write("README.md", "fixture\n");
        repo.commit("root");
        repo
    }

    pub fn root(&self) -> &Path {
        self.dir.path()
    }

    pub fn git(&self, args: &[&str]) -> String {
        let out = crate::git::command()
            .args(args)
            .current_dir(self.dir.path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    pub fn write(&self, path: &str, text: &str) {
        let file = self.dir.path().join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }

    pub fn commit(&self, message: &str) -> String {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", message]);
        self.git(&["rev-parse", "HEAD"])
    }

    /// Write a task record without a uid, as records were before backfill.
    pub fn record(&self, id: &str, title: &str) {
        self.write(
            &format!("project-management/tasks/{id}.md"),
            &format!("---\nid: {id}\ntitle: \"{title}\"\nstatus: todo\n---\n\n# {id}\n"),
        );
    }

    /// Mark the repository shallow at its current `main`.
    pub fn make_shallow(&self) {
        let head = self.git(&["rev-parse", "main"]);
        std::fs::write(self.dir.path().join(".git/shallow"), format!("{head}\n")).unwrap();
    }
}

/// The seeded registry fixture, checked out on `main`:
///
/// - `main` lands TSK-001 and TSK-002 without uids; the local registry is
///   seeded from it, so both entries are `landed`;
/// - `integration/EPC-001-line` is a second landing tip, one commit past it;
/// - `feat/one` and `feat/two` each rewrite one of the records identically
///   under another commit, so the check must find each copy's landing among
///   the landing tips (`landed_among`);
/// - `feat/other` holds a different TSK-002, which the check warns about.
///
/// Returns the repository and its root commit, the base of every side line.
pub(crate) fn seeded() -> (Repo, String) {
    let repo = Repo::new();
    let base = repo.git(&["rev-parse", "HEAD"]);
    repo.record("TSK-001", "first");
    repo.record("TSK-002", "second");
    repo.commit("land two records");
    crate::ids::seed::seed(repo.root(), None).unwrap();
    repo.git(&["checkout", "-q", "-b", "integration/EPC-001-line"]);
    repo.write("line.txt", "line\n");
    repo.commit("line work");
    for (branch, id, title) in [
        ("feat/one", "TSK-001", "first"),
        ("feat/two", "TSK-002", "second"),
        ("feat/other", "TSK-002", "another record"),
    ] {
        repo.git(&["checkout", "-q", "-b", branch, &base]);
        repo.record(id, title);
        repo.commit(&format!("{branch}: {id}"));
    }
    repo.git(&["checkout", "-q", "main"]);
    (repo, base)
}
