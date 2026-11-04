//! Scratch-adopter journeys through the shipped hook shim and real CI binary.
use git2::{Oid, Repository};
use serde_json::json;
use std::path::Path;
use std::process::{Command, Output};

struct Adopter {
    dir: tempfile::TempDir,
    repo: Repository,
    base: Oid,
}
impl Adopter {
    fn new(git: &serde_json::Value) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        repo.set_head("refs/heads/feat/paths").unwrap();
        repo.config().unwrap().set_str("user.name", "Test").unwrap();
        repo.config()
            .unwrap()
            .set_str("user.email", "test@example.invalid")
            .unwrap();
        let mut f = Self {
            dir,
            repo,
            base: Oid::ZERO_SHA1,
        };
        f.stage(
            ".codeflow/policy.json",
            0o100_644,
            json!({"git": git}).to_string().as_bytes(),
        );
        f.base = f.commit(&[]);
        f.head(f.base);
        f.repo
            .reference("refs/heads/main", f.base, true, "fixture")
            .unwrap();
        f.repo
            .checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
            .unwrap();
        f
    }
    fn stage(&self, path: &str, mode: u32, bytes: &[u8]) {
        let mut index = self.repo.index().unwrap();
        let id = if mode == 0o160_000 {
            self.base
        } else {
            self.repo.blob(bytes).unwrap()
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
                file_size: u32::try_from(bytes.len()).unwrap(),
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
        index.remove_path(Path::new(path)).unwrap();
        index.write().unwrap();
    }
    fn commit(&self, parents: &[Oid]) -> Oid {
        let tree = self
            .repo
            .find_tree(self.repo.index().unwrap().write_tree().unwrap())
            .unwrap();
        let sig = git2::Signature::now("Test", "test@example.invalid").unwrap();
        let parents: Vec<_> = parents
            .iter()
            .map(|p| self.repo.find_commit(*p).unwrap())
            .collect();
        self.repo
            .commit(
                None,
                &sig,
                &sig,
                "feat: exercise path checks",
                &tree,
                &parents.iter().collect::<Vec<_>>(),
            )
            .unwrap()
    }
    fn head(&self, head: Oid) {
        self.repo
            .reference("refs/heads/feat/paths", head, true, "fixture")
            .unwrap();
    }
    fn command(&self, program: &str) -> Command {
        let mut c = Command::new(program);
        c.current_dir(self.dir.path())
            .env("CODEFLOW_HOOK_BINARY", env!("CARGO_BIN_EXE_codeflow"))
            .env("CODEFLOW_HOME", self.dir.path().join("codeflow-home"))
            .env(
                "GIT_CONFIG_GLOBAL",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
            )
            .env(
                "GIT_CONFIG_SYSTEM",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
            );
        // Literal keys let the fixture contract verify that injected Git
        // configuration remains inherited by every child process.
        c.env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("CODEFLOW_PR_BODY")
            .env_remove("GITHUB_EVENT_NAME")
            .env_remove("GITHUB_HEAD_REF")
            .env_remove("GITHUB_BASE_REF")
            .env_remove("CODEFLOW_HUMAN_OVERRIDE")
            .env_remove("CODEFLOW_INTEGRATE_TOKEN")
            .env_remove("CLAUDECODE")
            .env_remove("CODEX_SESSION_ID")
            .env_remove("CODEX_THREAD_ID")
            .env_remove("CODEX_CI")
            .env_remove("GROK_SESSION_ID");
        c
    }
    fn ci(&self, head: Oid) -> Output {
        self.command(env!("CARGO_BIN_EXE_codeflow"))
            .args([
                "ci",
                "--base",
                &self.base.to_string(),
                "--head",
                &head.to_string(),
                "--branch",
                "feat/paths",
            ])
            .output()
            .unwrap()
    }
    fn hook(&self) -> Output {
        self.command(env!("CARGO_BIN_EXE_codeflow"))
            .args(["git-hook", "pre-commit"])
            .output()
            .unwrap()
    }
    #[cfg(unix)]
    fn install_hook(&self) {
        use std::os::unix::fs::PermissionsExt;
        let hook = self.repo.path().join("hooks/pre-commit");
        std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
        std::fs::write(
            &hook,
            include_str!("../../../assets/base/git-hooks/pre-commit"),
        )
        .unwrap();
        std::fs::set_permissions(hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}
fn said(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}
fn verdict(output: &Output, blocked: bool) {
    let text = said(output);
    assert_eq!(output.status.success(), !blocked, "{text}");
    if blocked {
        assert!(text.contains("git.path_policy"), "{text}");
    }
}

#[test]
fn cli_judges_every_change_kind_on_staged_and_committed_objects() {
    for protected in [true, false] {
        for kind in [
            "add",
            "modify",
            "rename",
            "rename-away",
            "type",
            "delete",
            "gitlink",
        ] {
            let mut f =
                Adopter::new(&json!({"protected_paths":[{"prefix":"private", "allow":[]}]}));
            let path = if protected {
                "private/data"
            } else {
                "deploy/id_rsa"
            };
            if ["modify", "type", "delete", "rename-away"].contains(&kind) {
                f.stage(path, 0o100_644, b"existing");
            }
            if kind == "rename" {
                f.stage("ordinary", 0o100_644, b"existing");
            }
            f.base = f.commit(&[f.base]);
            f.head(f.base);
            match kind {
                "rename-away" => {
                    f.delete(path);
                    f.stage("ordinary", 0o100_644, b"existing");
                }
                "delete" => f.delete(path),
                "rename" => {
                    f.delete("ordinary");
                    f.stage(path, 0o100_644, b"existing");
                }
                "type" => f.stage(path, 0o120_000, b"public"),
                "gitlink" => f.stage(path, 0o160_000, b""),
                _ => f.stage(path, 0o100_644, b"new"),
            }
            let blocked = protected || !["delete", "rename-away"].contains(&kind);
            verdict(&f.hook(), blocked);
            let output = f.ci(f.commit(&[f.base]));
            verdict(&output, blocked);
            if !blocked {
                assert!(said(&output).contains("remains in history"));
            }
        }
    }
}

#[cfg(unix)]
#[test]
fn installed_hooks_and_ci_refuse_deep_names_and_protected_symlinks() {
    for (path, mode, content) in [
        ("config/prod/.env.local", 0o100_644, "public"),
        ("deploy/id_rsa", 0o100_644, "public"),
        ("link", 0o120_000, "private/data"),
    ] {
        let f = Adopter::new(&json!({"protected_paths":[{"prefix":"private", "allow":[]}]}));
        f.install_hook();
        f.stage(path, mode, content.as_bytes());
        let output = f
            .command("git")
            .args(["commit", "-m", "feat: rejected fixture"])
            .output()
            .unwrap();
        verdict(&output, true);
        verdict(&f.ci(f.commit(&[f.base])), true);
        println!("installed hook and CI refused {path}");
    }
}

#[test]
fn ci_remerge_refuses_introductions_and_passes_inherited_names() {
    let mut f = Adopter::new(&json!({}));
    let root = f.base;
    f.stage("deploy/id_rsa", 0o100_644, b"target content");
    f.base = f.commit(&[root]);
    f.delete("deploy/id_rsa");
    f.stage("ordinary", 0o100_644, b"side");
    let side = f.commit(&[root]);
    f.stage("deploy/id_rsa", 0o100_644, b"target content");
    let inherited = f.commit(&[side, f.base]);
    verdict(&f.ci(inherited), false);
    f.stage("config/prod/.env.local", 0o100_644, b"new");
    let introduced = f.commit(&[side, f.base]);
    verdict(&f.ci(introduced), true);
    println!("CI passed inherited blocked name and refused merge introduction");
}

/// Review probe H: an exception landed only on the declared integration
/// line passes pre-commit as it passes `codeflow ci` against that line.
#[test]
fn hook_and_ci_agree_on_an_exception_landed_only_on_the_line() {
    let f = Adopter::new(&json!({}));
    f.stage(
        "project-management/tasks/TSK-007.md",
        0o100_644,
        b"---\nid: TSK-007\nintegration_target: integration/x\n---\n",
    );
    let main = f.commit(&[f.base]);
    let line_policy = json!({"git":{"blocked_names":[{"pattern":"*.pem",
        "except_under":["fixtures"],"reason":"public certificates"}]}})
    .to_string();
    f.stage(".codeflow/policy.json", 0o100_644, line_policy.as_bytes());
    let line = f.commit(&[main]);
    f.repo
        .remote("origin", "https://example.invalid/repo.git")
        .unwrap();
    for (name, id) in [("main", main), ("integration/x", line)] {
        f.repo
            .reference(&format!("refs/remotes/origin/{name}"), id, true, "fixture")
            .unwrap();
    }
    f.repo
        .reference("refs/heads/task/TSK-007-certs", line, true, "fixture")
        .unwrap();
    f.repo.set_head("refs/heads/task/TSK-007-certs").unwrap();
    std::fs::write(f.dir.path().join(".codeflow/policy.json"), &line_policy).unwrap();
    f.stage("fixtures/cert.pem", 0o100_644, b"public");
    verdict(&f.hook(), false);
    let head = f.commit(&[line]);
    let base = line.to_string();
    let ci = f
        .command(env!("CARGO_BIN_EXE_codeflow"))
        .args([
            "ci",
            "--base",
            &base,
            "--head",
            &head.to_string(),
            "--branch",
            "task/TSK-007-certs",
        ])
        .output()
        .unwrap();
    assert!(!said(&ci).contains("git.path_policy"), "{}", said(&ci));
    f.stage("other/cert.pem", 0o100_644, b"public");
    verdict(&f.hook(), true);
}

#[test]
fn branch_only_exceptions_and_disabled_level_cannot_relax_target_policy() {
    let f = Adopter::new(&json!({"protected_paths":[{"prefix":"private", "allow":[]}]}));
    let local =
        json!({"git":{"path_policy":"off", "blocked_names":[], "protected_paths":[]}}).to_string();
    std::fs::write(f.dir.path().join(".codeflow/policy.json"), &local).unwrap();
    f.stage(".codeflow/policy.json", 0o100_644, local.as_bytes());
    f.stage("private/data", 0o100_644, b"public");
    verdict(&f.hook(), true);
    let head = f.commit(&[f.base]);
    verdict(&f.ci(head), true);
    let under_warn = f
        .command(env!("CARGO_BIN_EXE_codeflow"))
        .args([
            "ci",
            "--base",
            &f.base.to_string(),
            "--head",
            &head.to_string(),
            "--branch",
            "feat/paths",
            "--run-level",
            "warn",
        ])
        .output()
        .unwrap();
    verdict(&under_warn, true);
}

#[test]
fn hostile_policy_cannot_lock_inputs_and_exemptions_do_not_disable_secret_scan() {
    let f = Adopter::new(
        &json!({"blocked_names":["*"],"protected_paths":[{"prefix":"/", "allow":[]}]}),
    );
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
        f.stage(&path.to_uppercase(), 0o100_644, b"public");
    }
    verdict(&f.hook(), false);
    verdict(&f.ci(f.commit(&[f.base])), false);
    f.stage(
        ".gitignore",
        0o100_644,
        concat!("-----BEGIN RSA ", "PRIVATE KEY-----\n").as_bytes(),
    );
    let output = f.hook();
    assert!(!output.status.success());
    assert!(said(&output).contains("git.secret_scan"));
    let f = Adopter::new(&json!({"path_policy":"off"}));
    f.stage("config/prod/.env.local", 0o100_644, b"public");
    let output = f.hook();
    assert!(!output.status.success());
    assert!(said(&output).contains("git.secret_scan"));
    assert!(!said(&output).contains("policy rule git.path_policy"));
}

#[test]
fn doctor_names_root_waivers_and_policy_explain_lists_new_keys() {
    let f = Adopter::new(
        &json!({"blocked_names":[{"pattern":"*.pem","except_under":["/"],"reason":"public certificates"}],
        "protected_paths":[{"prefix":"private","allow":[{"path":".","reason":"whole rule waiver"}]}]}),
    );
    let output = f
        .command(env!("CARGO_BIN_EXE_codeflow"))
        .args(["doctor", "--check", "policy-source"])
        .output()
        .unwrap();
    let text = said(&output);
    assert!(text.contains("2 whole-rule waivers"), "{text}");
    assert!(
        text.contains("public certificates") && text.contains("whole rule waiver"),
        "{text}"
    );
    let output = f
        .command(env!("CARGO_BIN_EXE_codeflow"))
        .args(["policy", "explain"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", said(&output));
    for key in [
        "git.path_policy",
        "git.blocked_names",
        "git.protected_paths",
    ] {
        assert!(said(&output).contains(key));
    }
}
