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
        ("credentials.json", 0o100_644, "{}"),
        ("config/CREDENTIALS.JSON", 0o100_644, "{}"),
        ("home/.claude/.credentials.json", 0o100_644, "{}"),
        // Gate inputs stay exempt only as regular files.
        (".codeflow/bootstrap.sh", 0o120_000, "/tmp/payload"),
        (".codeflow/git-hooks/pre-commit", 0o120_000, "../../outside"),
        (".gitignore", 0o160_000, ""),
        (".github/workflows/codeflow-ci.yml", 0o160_000, ""),
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

/// TSK-323: the size and ignored-file rules through the installed hook
/// shim, `git commit` and `codeflow ci`, judged by the authority commit.
#[cfg(unix)]
mod content_rules {
    use super::{said, Adopter};
    use serde_json::json;
    use std::process::Output;

    const MIB: usize = 1 << 20;

    /// An adopter whose authority commit holds `.gitignore` and, for a new
    /// scaffold, the project kind `codeflow init` writes.
    fn adopter(new_scaffold: bool) -> Adopter {
        let mut f = Adopter::new(&json!({}));
        f.stage(".gitignore", 0o100_644, b"build/\n");
        if new_scaffold {
            f.stage(
                ".codeflow/project.toml",
                0o100_644,
                concat!(
                    "schema_version = 1\ntier = \"minimal\"\nscaffold_version = \"3.2.0\"\n",
                    "stack = \"generic\"\nareas = []\npolicy_armed = true\ngit_hooks = \"wired\"\n",
                    "permission_preset = \"default\"\nfile_size_rule = \"refuse\"\n"
                )
                .as_bytes(),
            );
        }
        f.base = f.commit(&[f.base]);
        f.head(f.base);
        f.repo
            .checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
            .unwrap();
        f.install_hook();
        f
    }

    fn git(f: &Adopter, args: &[&str]) -> Output {
        f.command("git").args(args).output().unwrap()
    }

    /// Write `size` bytes at `path`, add it with git, and commit through
    /// the installed hook; then judge the range with `codeflow ci`.
    fn commit_file(f: &Adopter, path: &str, size: usize) -> (Output, Option<Output>) {
        let file = f.dir.path().join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, vec![b'x'; size]).unwrap();
        let add = git(f, &["add", "-f", "--", path]);
        assert!(add.status.success(), "{}", said(&add));
        let commit = git(f, &["commit", "-m", "feat: add a fixture file"]);
        let ci = commit.status.success().then(|| {
            let head = f.repo.head().unwrap().peel_to_commit().unwrap().id();
            f.ci(head)
        });
        (commit, ci)
    }

    #[test]
    fn a_new_scaffold_warns_at_two_mib_and_refuses_eleven_naming_the_routes() {
        let f = adopter(true);
        let (commit, ci) = commit_file(&f, "assets/two.bin", 2 * MIB);
        let ci = ci.expect("the 2 MiB commit passes");
        for output in [&commit, &ci] {
            let text = said(output);
            assert!(output.status.success(), "{text}");
            assert!(
                text.contains("warning — policy rule git.max_file_bytes (warn)"),
                "{text}"
            );
        }
        println!("2 MiB: {}", said(&commit));
        let f = adopter(true);
        let (commit, ci) = commit_file(&f, "assets/eleven.bin", 11 * MIB);
        assert!(ci.is_none());
        let text = said(&commit);
        assert!(!commit.status.success(), "{text}");
        for part in [
            "BLOCKED — policy rule git.max_file_bytes (block)",
            "assets/eleven.bin is 11534336 bytes",
            "Git LFS",
            "on the default branch through a reviewed pull request",
            "admitted row",
        ] {
            assert!(text.contains(part), "{part}: {text}");
        }
        println!("11 MiB: {text}");
        // The same file committed without the hook still fails in CI.
        f.stage("assets/eleven.bin", 0o100_644, &vec![b'x'; 11 * MIB]);
        let ci = f.ci(f.commit(&[f.base]));
        assert!(!ci.status.success(), "{}", said(&ci));
        assert!(said(&ci).contains("git.max_file_bytes"), "{}", said(&ci));
    }

    #[test]
    fn an_ignored_build_artefact_passes_with_a_warning() {
        let f = adopter(true);
        let (commit, ci) = commit_file(&f, "build/app.o", 10);
        let ci = ci.expect("the ignored file passes");
        for output in [&commit, &ci] {
            let text = said(output);
            assert!(output.status.success(), "{text}");
            assert!(
                text.contains("ignored file changed: build/app.o matches `build/`"),
                "{text}"
            );
        }
        println!("ignored: {}", said(&commit));
    }

    #[test]
    fn an_eleven_mib_file_tracked_by_git_lfs_passes() {
        let f = adopter(true);
        // git's own filter mechanism with a clean command that writes the
        // Git LFS pointer, as `git lfs install` configures it.
        let clean = f.dir.path().join("lfs-clean.sh");
        std::fs::write(
            &clean,
            "t=$(mktemp); cat > \"$t\"; size=$(wc -c < \"$t\" | tr -d ' '); \
             oid=$( (sha256sum \"$t\" 2>/dev/null || shasum -a 256 \"$t\") | cut -d' ' -f1); \
             rm -f \"$t\"; printf 'version https://git-lfs.github.com/spec/v1\\noid sha256:%s\\nsize %s\\n' \"$oid\" \"$size\"\n",
        )
        .unwrap();
        let filter = format!("sh {}", clean.display());
        assert!(git(&f, &["config", "filter.lfs.clean", &filter])
            .status
            .success());
        assert!(git(&f, &["config", "filter.lfs.required", "true"])
            .status
            .success());
        std::fs::write(
            f.dir.path().join(".gitattributes"),
            "*.bin filter=lfs diff=lfs merge=lfs -text\n",
        )
        .unwrap();
        assert!(git(&f, &["add", ".gitattributes"]).status.success());
        let (commit, ci) = commit_file(&f, "models/weights.bin", 11 * MIB);
        let ci = ci.expect("the LFS pointer passes");
        for output in [&commit, &ci] {
            let text = said(output);
            assert!(output.status.success(), "{text}");
            assert!(!text.contains("git.max_file_bytes"), "{text}");
        }
        let recorded = git(&f, &["cat-file", "-p", "HEAD:models/weights.bin"]);
        assert!(said(&recorded).starts_with("version https://git-lfs.github.com/spec/v1"));
        println!("LFS pointer recorded: {}", said(&recorded));
    }

    #[test]
    fn an_updated_project_gives_a_report_line_and_passes() {
        let f = adopter(false);
        let (commit, ci) = commit_file(&f, "assets/eleven.bin", 11 * MIB);
        let ci = ci.expect("an updated project passes");
        for output in [&commit, &ci] {
            let text = said(output);
            assert!(output.status.success(), "{text}");
            assert!(!text.contains("policy rule git.max_file_bytes"), "{text}");
            assert!(
                text.contains("file too large: assets/eleven.bin")
                    && text.contains("reported only"),
                "{text}"
            );
        }
        println!("updated project: {}", said(&commit));
    }

    /// Review 01, finding 1: a clean automatic merge that records a file
    /// over the limit, though neither parent holds one, fails in CI.
    #[test]
    fn ci_refuses_a_clean_merge_that_produces_an_oversized_file() {
        let mut f = adopter(true);
        let line = |byte: u8, len: usize| {
            let mut line = vec![byte; len - 1];
            line.push(b'\n');
            line
        };
        let mut lines: Vec<Vec<u8>> = (0..8192).map(|_| line(b'x', 1024)).collect();
        f.stage("data.txt", 0o100_644, &lines.concat());
        f.base = f.commit(&[f.base]);
        let wide = MIB + MIB / 2;
        lines[10] = line(b'a', wide);
        f.stage("data.txt", 0o100_644, &lines.concat());
        let ours = f.repo.find_commit(f.commit(&[f.base])).unwrap();
        lines[10] = line(b'x', 1024);
        lines[8000] = line(b'b', wide);
        f.stage("data.txt", 0o100_644, &lines.concat());
        let theirs = f.repo.find_commit(f.commit(&[f.base])).unwrap();
        let mut merged = f.repo.merge_commits(&ours, &theirs, None).unwrap();
        assert!(!merged.has_conflicts());
        let tree = f
            .repo
            .find_tree(merged.write_tree_to(&f.repo).unwrap())
            .unwrap();
        let sig = git2::Signature::now("Test", "test@example.invalid").unwrap();
        let merge = f
            .repo
            .commit(
                None,
                &sig,
                &sig,
                "chore(merge): join both widenings",
                &tree,
                &[&ours, &theirs],
            )
            .unwrap();
        let ci = f.ci(merge);
        let text = said(&ci);
        assert!(!ci.status.success(), "{text}");
        assert!(
            text.contains("BLOCKED — policy rule git.max_file_bytes (block)")
                && text.contains("file too large: data.txt"),
            "{text}"
        );
    }

    /// Review 01, finding 4: where the authority commit has no policy file,
    /// the shipped defaults judge, never a weaker working copy.
    #[test]
    fn ci_never_takes_the_size_rules_from_the_working_copy() {
        let mut f = adopter(true);
        f.delete(".codeflow/policy.json");
        f.base = f.commit(&[f.base]);
        f.head(f.base);
        f.repo
            .checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
            .unwrap();
        std::fs::write(
            f.dir.path().join(".codeflow/policy.json"),
            json!({"git": {"ignored_files": {"level": "off"},
                "max_file_bytes": {"limit": 1_000_000_000_u64}}})
            .to_string(),
        )
        .unwrap();
        f.stage("assets/eleven.bin", 0o100_644, &vec![b'x'; 11 * MIB]);
        f.stage("build/app.o", 0o100_644, b"object");
        let ci = f.ci(f.commit(&[f.base]));
        let text = said(&ci);
        assert!(!ci.status.success(), "{text}");
        assert!(text.contains("file too large: assets/eleven.bin"), "{text}");
        assert!(text.contains("ignored file changed: build/app.o"), "{text}");
    }

    /// Review 02, N1: CI judges a merge that hand-picks a side branch's
    /// oversized file and ignored file over the base's shrink and delete.
    #[test]
    fn ci_judges_a_hand_resolution_that_restores_a_parents_blob() {
        let mut f = adopter(true);
        f.stage("assets/data.bin", 0o100_644, &vec![b'x'; 11 * MIB]);
        f.stage("build/app.o", 0o100_644, b"object");
        let ancestor = f.commit(&[f.base]);
        f.stage("assets/data.bin", 0o100_644, &vec![b'x'; MIB]);
        f.delete("build/app.o");
        f.base = f.commit(&[ancestor]);
        let mut index = f.repo.index().unwrap();
        index
            .read_tree(&f.repo.find_commit(ancestor).unwrap().tree().unwrap())
            .unwrap();
        index.write().unwrap();
        f.stage("README.md", 0o100_644, b"side");
        let side = f.commit(&[ancestor]);
        let merge = f.commit(&[f.base, side]);
        let ci = f.ci(merge);
        let text = said(&ci);
        assert!(!ci.status.success(), "{text}");
        assert!(
            text.contains("file too large: assets/data.bin")
                && text.contains("ignored file changed: build/app.o"),
            "{text}"
        );
    }

    /// Review 02, N2: a content setting the schema refuses at the authority
    /// stops the installed pre-commit hook, though the working policy is
    /// valid.
    #[test]
    fn the_hook_refuses_when_the_authority_content_setting_is_invalid() {
        let f = Adopter::new(&json!({"max_file_bytes": null}));
        f.install_hook();
        std::fs::write(f.dir.path().join(".codeflow/policy.json"), "{\"git\": {}}").unwrap();
        let file = f.dir.path().join("assets/eleven.bin");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, vec![b'x'; 11 * MIB]).unwrap();
        assert!(git(&f, &["add", "--", "assets/eleven.bin"])
            .status
            .success());
        let commit = git(&f, &["commit", "-m", "feat: add a fixture file"]);
        let text = said(&commit);
        assert!(!commit.status.success(), "{text}");
        assert!(text.contains("invalid policy at"), "{text}");
    }

    /// Run `program` in the adopter with this build first on `PATH`.
    fn run(f: &Adopter, path: &std::ffi::OsStr, program: &str, args: &[&str]) -> Output {
        let output = f
            .command(program)
            .env("PATH", path)
            .args(args)
            .output()
            .unwrap();
        println!("$ {program} {}\n{}", args.join(" "), said(&output));
        output
    }

    /// A scratch project made by `codeflow init`, its scaffold on the
    /// origin's `main` as a merged pull request leaves it (so origin/main is
    /// the authority), checked out on a new feature branch.
    fn real_scaffold() -> (Adopter, std::ffi::OsString, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init_opts(
            dir.path(),
            git2::RepositoryInitOptions::new().initial_head("main"),
        )
        .unwrap();
        repo.config().unwrap().set_str("user.name", "Test").unwrap();
        repo.config()
            .unwrap()
            .set_str("user.email", "test@example.invalid")
            .unwrap();
        let f = Adopter {
            dir,
            repo,
            base: git2::Oid::ZERO_SHA1,
        };
        let root = f.dir.path().to_path_buf();
        std::fs::write(root.join(".gitignore"), "build/\n").unwrap();
        let exe = std::path::PathBuf::from(env!("CARGO_BIN_EXE_codeflow"));
        let path = std::env::join_paths(
            exe.parent()
                .map(std::path::Path::to_path_buf)
                .into_iter()
                .chain(std::env::split_paths(
                    &std::env::var_os("PATH").unwrap_or_default(),
                )),
        )
        .unwrap();
        let run = |program: &str, args: &[&str]| run(&f, &path, program, args);
        let init = run(
            env!("CARGO_BIN_EXE_codeflow"),
            &["init", "--minimal", "--yes"],
        );
        assert!(init.status.success(), "{}", said(&init));
        let project = std::fs::read_to_string(root.join(".codeflow/project.toml")).unwrap();
        assert!(project.contains("file_size_rule = \"refuse\""), "{project}");
        if !said(&run("git", &["status", "--porcelain"]))
            .trim()
            .is_empty()
        {
            assert!(run("git", &["add", "-A"]).status.success());
            let commit = run("git", &["commit", "-m", "chore: adopt codeflow"]);
            assert!(commit.status.success(), "{}", said(&commit));
        }
        let remote = tempfile::tempdir().unwrap();
        let origin = remote.path().join("origin.git");
        let origin_text = origin.to_str().unwrap();
        let root_text = root.to_str().unwrap();
        for args in [
            &["init", "-q", "--bare", origin_text][..],
            &[
                "--git-dir",
                origin_text,
                "fetch",
                "-q",
                root_text,
                "+main:main",
            ],
            &["remote", "add", "origin", origin_text],
            &["fetch", "-q", "origin"],
            &["remote", "set-head", "origin", "main"],
            &["switch", "-q", "-c", "feat/large-files"],
        ] {
            let output = run("git", args);
            assert!(output.status.success(), "{}", said(&output));
        }
        (f, path, remote)
    }

    /// AC-5 through the real commands: `codeflow init` makes the scaffold,
    /// git commits through the hooks it installs, and `codeflow ci` judges
    /// the branch against the origin's default branch.
    #[test]
    fn a_real_init_scaffold_judges_large_and_ignored_files() {
        let (f, path, _remote) = real_scaffold();
        let root = f.dir.path().to_path_buf();
        let run = |program: &str, args: &[&str]| run(&f, &path, program, args);
        let write = |name: &str, size: usize| {
            let file = root.join(name);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, vec![b'x'; size]).unwrap();
            assert!(run("git", &["add", "-f", "--", name]).status.success());
        };
        write("assets/two.bin", 2 * MIB);
        let two = run("git", &["commit", "-m", "feat: add a two mebibyte asset"]);
        assert!(two.status.success(), "{}", said(&two));
        assert!(
            said(&two).contains("large file: assets/two.bin"),
            "{}",
            said(&two)
        );
        write("assets/eleven.bin", 11 * MIB);
        let eleven = run(
            "git",
            &["commit", "-m", "feat: add an eleven mebibyte asset"],
        );
        let text = said(&eleven);
        assert!(!eleven.status.success(), "{text}");
        assert!(
            text.contains("file too large: assets/eleven.bin")
                && text.contains("Git LFS")
                && text.contains("admitted row"),
            "{text}"
        );
        assert!(run("git", &["rm", "-q", "--cached", "assets/eleven.bin"])
            .status
            .success());
        write("build/app.o", 10);
        let ignored = run("git", &["commit", "-m", "feat: add a build artefact"]);
        assert!(ignored.status.success(), "{}", said(&ignored));
        assert!(
            said(&ignored).contains("ignored file changed: build/app.o"),
            "{}",
            said(&ignored)
        );
        let ci = run(
            env!("CARGO_BIN_EXE_codeflow"),
            &[
                "ci",
                "--base",
                "origin/main",
                "--head",
                "HEAD",
                "--branch",
                "feat/large-files",
            ],
        );
        let text = said(&ci);
        assert!(ci.status.success(), "{text}");
        assert!(
            text.contains("large file: assets/two.bin")
                && text.contains("ignored file changed: build/app.o")
                && !text.contains("eleven.bin"),
            "{text}"
        );
    }
}
