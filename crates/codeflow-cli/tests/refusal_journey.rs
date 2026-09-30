//! Installed minimal scaffold, driven through its actual hook commands.
//! Payload replay proves the installed path, not native harness dispatch.
use serde_json::{json, Value};
use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

struct Project {
    temp: tempfile::TempDir,
    root: PathBuf,
    bin: PathBuf,
    home: PathBuf,
}
impl Project {
    fn command(&self, program: &str) -> Command {
        let mut command = Command::new(program);
        let path = std::env::join_paths([self.bin.clone()].into_iter().chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
        ))
        .unwrap();
        command
            .current_dir(&self.root)
            .env("PATH", path)
            .env("CODEFLOW_HOME", &self.home)
            .env("HOME", &self.home)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("CODEFLOW_HUMAN_OVERRIDE")
            .env_remove("CODEFLOW_INTEGRATE_TOKEN");
        command
    }
    fn git(&self, args: &[&str]) {
        success(&self.command("git").args(args).output().unwrap());
    }
    fn new() -> Self {
        Self::from_temp(tempfile::tempdir().unwrap())
    }
    fn from_temp(temp: tempfile::TempDir) -> Self {
        let root = temp.path().join("project");
        let bin = temp.path().join("bin");
        let home = temp.path().join("state");
        for p in [&root, &bin, &home] {
            std::fs::create_dir(p).unwrap();
        }
        std::fs::copy(env!("CARGO_BIN_EXE_codeflow"), bin.join("codeflow")).unwrap();
        let project = Self {
            temp,
            root,
            bin,
            home,
        };
        project.git(&["init", "-b", "task/x"]);
        std::fs::write(project.root.join("README.md"), "fixture\n").unwrap();
        std::fs::create_dir(project.root.join("src")).unwrap();
        std::fs::write(project.root.join("src/main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(
            project.root.join("Cargo.toml"),
            "[workspace]\n\n[package]\nname = \"guard-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(project.root.join(".gitignore"), "target/\n").unwrap();
        project.git(&[
            "add",
            "README.md",
            "src/main.rs",
            "Cargo.toml",
            ".gitignore",
        ]);
        project.git(&["commit", "-m", "chore: seed fixture"]);
        project.git(&["tag", "v1.2.3"]);
        let remote = project.temp.path().join("remote.git");
        success(
            &project
                .command("git")
                .args(["init", "--bare"])
                .arg(&remote)
                .output()
                .unwrap(),
        );
        project.git(&["remote", "add", "origin", remote.to_str().unwrap()]);
        success(
            &project
                .command("codeflow")
                .args(["init", "--yes", "--minimal"])
                .output()
                .unwrap(),
        );
        project
    }
    fn check_shell(&self, cwd: &std::path::Path, command: &str, refused: bool) {
        let payload = json!({"tool_name":"Bash", "cwd":cwd, "tool_input":{"command":command}});
        let mut child = self
            .command("codeflow")
            // The hook process may start outside the payload's directory.
            .current_dir(self.temp.path())
            .args(["hook", "git-guard"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(payload.to_string().as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        if refused {
            assert_eq!(output.status.code(), Some(2), "{command}: {output:?}");
            assert!(String::from_utf8_lossy(&output.stderr).contains("git.hook_integrity"));
        } else {
            success(&output);
        }
    }
    fn replay(&self, harness: &str, tool: &str, input: Value) -> Vec<Output> {
        let mut payload = if harness == "grok" {
            json!({"toolName":tool,"cwd":self.root})
        } else {
            json!({"tool_name":tool,"cwd":self.root})
        };
        payload[if harness == "grok" {
            "toolInput"
        } else {
            "tool_input"
        }] = input;
        self.replay_payload(harness, tool, &payload)
    }
    fn replay_payload(&self, harness: &str, tool: &str, payload: &Value) -> Vec<Output> {
        let file = match harness {
            "claude" => ".claude/settings.json",
            "codex" => ".codex/hooks.json",
            "grok" => ".grok/hooks/codeflow.json",
            _ => unreachable!(),
        };
        let config: Value =
            serde_json::from_slice(&std::fs::read(self.root.join(file)).unwrap()).unwrap();
        let mut outputs = Vec::new();
        for entry in config["hooks"]["PreToolUse"].as_array().unwrap() {
            let matcher = entry["matcher"].as_str().unwrap();
            if !regex::Regex::new(matcher).unwrap().is_match(tool) {
                continue;
            }
            for hook in entry["hooks"].as_array().unwrap() {
                let mut child = self
                    .command("sh")
                    .args(["-c", hook["command"].as_str().unwrap()])
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap();
                child
                    .stdin
                    .take()
                    .unwrap()
                    .write_all(payload.to_string().as_bytes())
                    .unwrap();
                outputs.push(child.wait_with_output().unwrap());
            }
        }
        assert!(
            !outputs.is_empty(),
            "no installed hook for {harness}:{tool}"
        );
        outputs
    }
    fn shell(&self, harness: &str, command: &str, rule: Option<&str>) {
        let outputs = self.replay(harness, "Bash", json!({"command":command}));
        if let Some(rule) = rule {
            assert!(
                outputs.iter().any(|out| out.status.code() == Some(2)
                    && String::from_utf8_lossy(&out.stderr).contains(rule)),
                "{harness}: {command}: {outputs:?}"
            );
            for out in outputs.iter().filter(|out| {
                out.status.code() == Some(2) && String::from_utf8_lossy(&out.stderr).contains(rule)
            }) {
                let text = String::from_utf8_lossy(&out.stderr);
                assert!(
                    text.contains("operator")
                        || text.contains("git status")
                        || text.contains("codeflow delegate")
                        || text.contains("codeflow update"),
                    "missing route: {text}"
                );
            }
        } else {
            for output in outputs {
                success(&output);
            }
        }
    }
}
fn success(output: &Output) {
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn installed_minimal_hooks_refuse_each_family_and_allow_real_ordinary_work() {
    let project = Project::new();
    assert_eq!(
        project
            .command("codeflow")
            .get_envs()
            .find(|(key, _)| *key == "HOME")
            .unwrap()
            .1,
        Some(project.home.as_os_str())
    );
    shell_refusal_pairs(&project);
    ordinary_and_discard_journey(&project);
}

fn shell_refusal_pairs(project: &Project) {
    for harness in ["claude", "codex", "grok"] {
        for (command, rule) in [
            ("timeout 1 sudo true", "security.privilege_escalation"),
            ("nohup cargo +stable publish", "security.outward_actions"),
            ("xargs gh gist create notes.txt", "security.outward_actions"),
            ("gh -R o/r release create v1", "security.outward_actions"),
            ("git push origin v1.2.3", "security.outward_actions"),
            (
                "git tag v9 && git push origin v9",
                "security.outward_actions",
            ),
            ("echo 'cargo publish' | sh", "security.outward_actions"),
            (
                "git -c push.followTags=true push origin task/x",
                "security.outward_actions",
            ),
            (
                "perl -le 'system(\"cargo publish\")'",
                "security.outward_actions",
            ),
            ("crontab", "security.outward_actions"),
            ("cat $HOME/.codex/auth.json", "security.secret_reads"),
            (
                "git push origin HEAD:refs/tags/v2",
                "security.outward_actions",
            ),
            ("git push origin --follow-tags", "security.outward_actions"),
            ("git push origin --mirror", "security.outward_actions"),
            ("env gh repo delete o/r", "security.outward_actions"),
            ("command gh auth login", "security.outward_actions"),
            ("git credential fill", "security.outward_actions"),
            (
                "npx security find-generic-password -s example",
                "security.secret_reads",
            ),
            (
                "defaults write domain name value",
                "security.outward_actions",
            ),
            (
                "python3 -c 'run(\"npm publish\")'",
                "security.outward_actions",
            ),
            (
                "node -e 'run(\"claude -p x\")'",
                "security.headless_peer_runs",
            ),
            (
                "ruby -e 'read(\"~/.codex/auth.json\")'",
                "security.secret_reads",
            ),
            (
                "perl -e 'write(\".codeflow/policy.json\")'",
                "git.hook_integrity",
            ),
        ] {
            project.shell(harness, command, Some(rule));
        }
        for command in [
            "bash -lc 'cargo build'",
            "git push origin task/x",
            "gh release view v1",
            "cargo package",
            "grep -rn sudo docs/",
            "cat README.md",
            "cat .env.example",
            "rg 'claude -p' assets/",
            "git restore README.md",
            "git clean -fdX target/",
            "git stash pop",
        ] {
            project.shell(harness, command, None);
        }
    }
}

fn ordinary_and_discard_journey(project: &Project) {
    // Actually execute the harmless journeys after the hooks accept them.
    success(
        &project
            .command("cargo")
            .arg("build")
            .env("CARGO_TARGET_DIR", project.temp.path().join("build-target"))
            .output()
            .unwrap(),
    );
    project.git(&["push", "origin", "task/x"]);
    std::fs::write(project.root.join("README.md"), "local work\n").unwrap();
    for harness in ["claude", "codex", "grok"] {
        for command in [
            "git reset --hard",
            "git restore .",
            "git checkout -- .",
            "git clean -fd",
        ] {
            project.shell(harness, command, Some("git.discard_uncommitted"));
        }
        project.shell(harness, "git restore README.md", None);
    }
    project.git(&[
        "stash",
        "push",
        "-m",
        "fixture saved work",
        "--",
        "README.md",
    ]);
    for harness in ["claude", "codex", "grok"] {
        for command in ["git stash drop", "git stash clear"] {
            project.shell(harness, command, Some("git.discard_uncommitted"));
        }
        project.shell(harness, "git stash pop", None);
    }
    project.git(&["stash", "pop"]);
    project.git(&["restore", "README.md"]);
    project.git(&["restore", ".gitignore"]);
    assert_eq!(
        std::fs::read_to_string(project.root.join("README.md")).unwrap(),
        "fixture\n"
    );
    for harness in ["claude", "codex", "grok"] {
        project.shell(harness, "git reset --hard", None);
    }
    project.git(&["switch", "-c", "task/unique"]);
    std::fs::write(project.root.join("README.md"), "unique commit\n").unwrap();
    project.git(&["add", "README.md"]);
    project.git(&["commit", "-m", "test: preserve unique fixture work"]);
    project.git(&["switch", "task/x"]);
    for harness in ["claude", "codex", "grok"] {
        project.shell(
            harness,
            "git branch -D task/unique",
            Some("git.discard_uncommitted"),
        );
    }
    let worktree = project.temp.path().join("linked");
    project.git(&["worktree", "add", worktree.to_str().unwrap(), "task/unique"]);
    std::fs::write(worktree.join("README.md"), "dirty linked tree\n").unwrap();
    let remove = format!("git worktree remove --force {}", worktree.display());
    for harness in ["claude", "codex", "grok"] {
        project.shell(harness, &remove, Some("git.discard_uncommitted"));
    }
}

#[test]
fn installed_documented_edit_hook_payloads_protect_paths() {
    let project = Project::new();
    for (harness, tool, data) in [
        (
            "codex",
            "apply_patch",
            include_str!("../../codeflow-core/tests/fixtures/edit-hooks/codex-apply-patch.json"),
        ),
        (
            "grok",
            "write",
            include_str!("../../codeflow-core/tests/fixtures/edit-hooks/grok-write.json"),
        ),
        (
            "grok",
            "search_replace",
            include_str!("../../codeflow-core/tests/fixtures/edit-hooks/grok-search-replace.json"),
        ),
    ] {
        let protected = data
            .replace("/fixture/project", project.root.to_str().unwrap())
            .replace("hello.txt", ".codex/config.toml")
            .replace("notes.txt", ".codeflow/policy.json");
        let payload = serde_json::from_str(&protected).unwrap();
        let denied = project.replay_payload(harness, tool, &payload);
        assert!(
            denied.iter().any(|out| out.status.code() == Some(2)
                && String::from_utf8_lossy(&out.stderr).contains("git.hook_integrity")),
            "{harness}:{tool}: {denied:?}"
        );
        let ordinary = protected
            .replace(".codeflow/policy.json", "notes.md")
            .replace(".codex/config.toml", "notes.md");
        let payload = serde_json::from_str(&ordinary).unwrap();
        for output in project.replay_payload(harness, tool, &payload) {
            success(&output);
        }
    }
}

#[cfg(target_os = "macos")]
#[test]
fn n3_shell_integrity_paths_use_payload_cwd_and_disk_case() {
    let project = Project::new();
    assert!(project.root.join(".CODEFLOW/policy.json").exists());
    let check = |command: &str, refused: bool| project.check_shell(&project.root, command, refused);
    for path in [".CODEFLOW/policy.json", ".codeflow/policy.json"] {
        for command in [
            format!(
                "cat > {path} <<'EOF'
x
EOF"
            ),
            format!("cp x {path}"),
            format!("sed -i '' s/a/b/ {path}"),
            format!("tee {path} < x"),
        ] {
            check(&command, true);
        }
    }
    check("printf x > .Git/hooks/pre-commit", true);
    check("printf x > .git/hooks/pre-commit", true);
    for command in [
        "cat > .CODEFLOW/notes.md <<'EOF'
x
EOF",
        "cp x .CODEFLOW/notes.md",
        "sed -i '' s/a/b/ .CODEFLOW/notes.md",
        "tee .CODEFLOW/notes.md < x",
        "cp .CODEFLOW/policy.json notes.md",
        "cat .CODEFLOW/policy.json",
    ] {
        check(command, false);
    }
}

#[cfg(target_os = "macos")]
fn claude_tempdir() -> tempfile::TempDir {
    let supplied = std::env::temp_dir();
    let base = if supplied.to_string_lossy().contains("/tmp/claude") {
        supplied
    } else {
        PathBuf::from("/tmp")
    };
    tempfile::Builder::new()
        .prefix("claude-tsk188-r1-")
        .tempdir_in(base)
        .unwrap()
}

#[cfg(target_os = "macos")]
#[test]
fn q2_claude_fixture_stays_under_supplied_tmpdir() {
    let temp = claude_tempdir();
    let supplied = std::env::temp_dir();
    if supplied.to_string_lossy().contains("/tmp/claude") {
        assert!(
            temp.path().starts_with(&supplied),
            "fixture {} escaped supplied {}",
            temp.path().display(),
            supplied.display()
        );
    } else {
        assert!(temp.path().to_string_lossy().starts_with("/tmp/claude-"));
    }
}

#[cfg(target_os = "macos")]
#[test]
fn r1_integrity_paths_in_claude_tmp() {
    let temp = claude_tempdir();
    let project = Project::from_temp(temp);
    for path in [".CODEFLOW/policy.json", ".codeflow/policy.json"] {
        project.check_shell(
            &project.root,
            &format!("cat > {path} <<'EOF'\nx\nEOF"),
            true,
        );
        project.check_shell(
            &project.root,
            &format!("printf x > {}", project.root.join(path).display()),
            true,
        );
    }
    project.check_shell(&project.root, "printf x > .CODEFLOW/notes.md", false);
    project.check_shell(
        &project.root,
        "printf x > /tmp/claude-tsk188-other/.codeflow/policy.json",
        false,
    );
}

#[cfg(target_os = "macos")]
#[test]
fn r2_integrity_paths_follow_cd_segments() {
    let project = Project::new();
    for path in [".CODEFLOW/policy.json", ".codeflow/policy.json"] {
        project.check_shell(
            &project.root.join("src"),
            &format!("cd .. && printf x > {path}"),
            true,
        );
        project.check_shell(
            &project.root,
            &format!("cd src && printf x > ../{path}"),
            true,
        );
        project.check_shell(
            &project.root.join("src"),
            &format!("cd ..; printf x > {path}"),
            true,
        );
    }
    project.check_shell(
        &project.root,
        "cd .codeflow && printf x > policy.json",
        true,
    );
    for command in [
        "cd .. && printf x > .CODEFLOW/notes.md",
        "cd .. && rm -rf target",
        "cd .. && cp .codeflow/policy.json notes.md",
    ] {
        project.check_shell(&project.root.join("src"), command, false);
    }
}

#[cfg(target_os = "macos")]
#[test]
fn r3_dd_output_operand_is_an_integrity_path() {
    let project = Project::new();
    for command in [
        "dd if=x of=.CODEFLOW/policy.json",
        "dd if=x of=.codeflow/policy.json",
        "dd of=.Git/hooks/pre-commit if=x",
    ] {
        project.check_shell(&project.root, command, true);
    }
    for command in [
        "dd if=.codeflow/policy.json of=notes.md",
        "dd if=x of=.CODEFLOW/notes.md",
        "dd if=x of=ordinary",
    ] {
        project.check_shell(&project.root, command, false);
    }
}

#[test]
fn r4_integrity_ancestor_directories_are_protected() {
    let project = Project::new();
    let mut dirs = vec![".codeflow", ".git", ".git/hooks"];
    if cfg!(target_os = "macos") {
        dirs.extend([".CODEFLOW", ".Git", ".Git/hooks"]);
    }
    for dir in dirs {
        for command in [
            format!("rm -rf {dir}"),
            format!("mv {dir} x"),
            format!("cp -r stuff {dir}"),
            format!("cp -r stuff/. {dir}/"),
            format!("git rm -r {dir}"),
        ] {
            project.check_shell(&project.root, &command, true);
        }
        project.check_shell(&project.root, &format!("cd {dir} && rm -rf ."), true);
        project.check_shell(&project.root, &format!("cp -r {dir} backup"), false);
        project.check_shell(&project.root, &format!("ln -s {dir} backup"), false);
        project.check_shell(&project.root, &format!("ln -sf stuff {dir}"), true);
        project.check_shell(&project.root, &format!("ls {dir}"), false);
    }
    for command in [
        "rm -rf target",
        "rm -rf src/ordinary",
        "rm -rf .codeflow-notes",
        "cp -r stuff ordinary",
        "printf x > .codeflow/notes.md",
        "mv .codeflow/notes.md notes.md",
        "cp .codeflow/policy.json notes.md",
        "cat .codeflow/policy.json",
    ] {
        project.check_shell(&project.root, command, false);
    }
}

#[test]
fn q1_integrity_directory_patterns_are_refused() {
    let project = Project::new();
    for command in [
        "rm -rf .codeflow/*",
        "rm -rf .codeflow/p*",
        "rm -rf .codeflow/?olicy.json",
        "rm -rf .codeflow/[p]olicy.json",
        "rm -rf .codeflow/{policy.json,git-hooks}",
        "rm -rf .git/*",
        "cd .codeflow && rm -rf *",
        "cd .codeflow && rm -rf p*",
    ] {
        project.check_shell(&project.root, command, true);
    }
    if cfg!(target_os = "macos") {
        project.check_shell(&project.root, "rm -rf .CODEFLOW/*", true);
        project.check_shell(&project.root, "cd .CODEFLOW && rm -rf *", true);
    }
    for command in [
        "rm -rf stuff/*",
        "rm -rf src/*.rs",
        "rm -rf stuff/?otes",
        "rm -rf stuff/[ab]*",
        "rm -rf stuff/{a,b}",
        "cd src && rm -rf *",
        "ls .codeflow/*",
        "rm -rf .codeflow/notes.md",
    ] {
        project.check_shell(&project.root, command, false);
    }
}

#[test]
fn q3_find_mutations_protect_explicit_roots() {
    let project = Project::new();
    for command in [
        "find .codeflow -delete",
        "find .codeflow -type f -delete",
        "find .git -delete",
        "find .codeflow -exec rm -rf {} +",
        "find .git/hooks -type f -exec chmod -x {} \\;",
        "find .codeflow -execdir rm -rf {} +",
        "find .codeflow -execdir chmod 000 {} \\;",
        "find -H .codeflow -delete",
        "cd .codeflow && find . -delete",
    ] {
        project.check_shell(&project.root, command, true);
    }
    if cfg!(target_os = "macos") {
        project.check_shell(&project.root, "find .CODEFLOW -delete", true);
    }
    for command in [
        "find .codeflow -name '*.json'",
        "find .codeflow -name -delete",
        "find .codeflow -exec echo -delete {} +",
        "find .codeflow -exec cat {} +",
        "find stuff -delete",
        "find src -exec rm {} +",
        "find . -name '*.rs' -delete",
        "find .codeflow -name rm -print",
    ] {
        project.check_shell(&project.root, command, false);
    }
}

#[test]
fn q3_rsync_checks_destination_only() {
    let project = Project::new();
    for command in [
        "rsync -a stuff/ .codeflow/",
        "rsync --delete stuff/ .codeflow/",
        "rsync -a --delete stuff/ .git/hooks/",
        "cd .codeflow && rsync -a ../stuff/ .",
    ] {
        project.check_shell(&project.root, command, true);
    }
    if cfg!(target_os = "macos") {
        project.check_shell(&project.root, "rsync -a stuff/ .CODEFLOW/", true);
    }
    for command in [
        "rsync -a .codeflow/ backup/",
        "rsync -a --delete .git/hooks/ backup/",
        "rsync -a stuff/ ordinary/",
        "rsync -a notes.md .codeflow/notes.md",
        "rsync --dry-run -a stuff/ .codeflow/",
        "rsync -an stuff/ .codeflow/",
    ] {
        project.check_shell(&project.root, command, false);
    }
}

#[test]
fn q4_plain_pushd_tracks_integrity_directory() {
    let project = Project::new();
    for command in [
        "pushd .codeflow >/dev/null && printf x > policy.json",
        "pushd .codeflow 2>/dev/null && printf x > policy.json",
        "pushd .codeflow &>/dev/null && printf x > policy.json",
        "pushd .codeflow >/dev/null 2>&1 && printf x > policy.json",
        "pushd .codeflow > /dev/null && printf x > policy.json",
        "pushd .codeflow >/dev/null; rm -rf *",
        "pushd -- .codeflow && printf x > policy.json",
        "pushd -- .codeflow >/dev/null && rm -rf *",
        "pushd .git >/dev/null && rm -rf hooks",
        "pushd .codeflow && printf x > policy.json",
        "pushd .codeflow && rm -rf *",
        "pushd .git && rm -rf hooks",
        "pushd src && printf x > ../.codeflow/policy.json",
    ] {
        project.check_shell(&project.root, command, true);
    }
    for command in [
        "pushd src && printf x > notes.md",
        "pushd .codeflow && cat policy.json",
        "pushd .codeflow && printf x > notes.md",
        "pushd -n .codeflow && printf x > policy.json",
        "pushd -n .codeflow >/dev/null && printf x > policy.json",
        "pushd +1 >/dev/null && printf x > policy.json",
        "pushd -1 >/dev/null && printf x > policy.json",
        "pushd >/dev/null && printf x > policy.json",
        "pushd -- src >/dev/null && printf x > notes.md",
    ] {
        project.check_shell(&project.root, command, false);
    }
}

#[test]
fn r5_n2_root_dot_patterns_match_protected_names_only() {
    let project = Project::new();
    for command in [
        "rm -rf .*",
        "rm -rf .c*",
        "rm -rf .[a-z]*",
        "rm -rf ./.*",
        "rm -rf .code*",
        "rm -rf .{codeflow,git}",
        "rm -rf .?it",
        "rm -rf .[cg]*",
        "rm -rf .{cache,codeflow}",
        "cd src && rm -rf ../.*",
        "mv .c* backup/",
        "cp -r stuff/ .code*",
        "printf x > .code*",
    ] {
        project.check_shell(&project.root, command, true);
    }
    for command in [
        "rm -rf *",
        "rm -rf *.log",
        "rm -rf .cache*",
        "rm -rf .*.swp",
        "rm -rf .{cache,config}",
        "rm -rf .[ab]*",
        "rm -rf .gitignore",
        "rm -rf .ordinary",
        "rm -rf src/.*",
        "cd src && rm -rf .*",
        "ls .*",
        "cp -r .c* backup/",
    ] {
        project.check_shell(&project.root, command, false);
    }
}
