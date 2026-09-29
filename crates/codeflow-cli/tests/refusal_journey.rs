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
        let temp = tempfile::tempdir().unwrap();
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
            "[package]\nname = \"guard-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
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
        let protected = data.replace("/fixture/project", project.root.to_str().unwrap());
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
