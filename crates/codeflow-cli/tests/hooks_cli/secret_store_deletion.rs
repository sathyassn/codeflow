//! TSK-1022: commands are hook payloads, never executed deletions.
use super::*;

fn stores() -> Vec<&'static str> {
    let table = codeflow_core::security::actions::table();
    let mut stores: Vec<_> = table
        .sandbox_read_denies
        .iter()
        .map(String::as_str)
        .chain(
            table
                .claude_read_groups
                .iter()
                .filter(|g| g.carveouts.is_empty())
                .flat_map(|g| &g.denies)
                .filter_map(|s| s.strip_prefix("Read(")?.strip_suffix(')')),
        )
        .filter(|s| s.starts_with("~/"))
        .collect();
    stores.sort_unstable();
    stores.dedup();
    stores
}

fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

struct Fixture {
    dir: tempfile::TempDir,
    home: std::path::PathBuf,
}

impl Fixture {
    fn new(relaxed: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init_repo(root, "main");
        let policy = serde_json::json!({"git":{"protected_branches":["main","master"],
            "root_checkout_commits":"off"},"security":{"text_guard":if relaxed {"relaxed"} else {"strict"}}});
        write_policy(root, &policy.to_string());
        git(root, &["add", ".codeflow/policy.json"]);
        git(root, &["commit", "-m", "chore: fixture policy"]);
        git(
            root,
            &["remote", "add", "origin", "https://example.invalid/r.git"],
        );
        git(root, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
        git(
            root,
            &[
                "symbolic-ref",
                "refs/remotes/origin/HEAD",
                "refs/remotes/origin/main",
            ],
        );
        git(root, &["checkout", "-q", "-b", "task/fixture"]);
        let home = root.join("home");
        std::fs::create_dir_all(&home).unwrap();
        for store in stores() {
            // A concrete witness for each configured pattern. TSK-395 owns
            // blanket harness folders; these are TSK-1022's named entries.
            let suffix = store
                .strip_prefix("~/")
                .unwrap()
                .replace("**/", "nested/")
                .replace("**", "fixture")
                .replace('*', "fixture");
            let mut path = home.join(suffix);
            if stores()
                .iter()
                .any(|other| other.starts_with(&format!("{store}/")))
            {
                path = path.join("fixture");
            }
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "planted fixture\n").unwrap();
        }
        Self { dir, home }
    }

    fn run(&self, guard: &str, command: &str) -> Output {
        run_with_stdin(
            codeflow()
                .args(["hook", guard, "--contract", "3"])
                .current_dir(self.dir.path())
                .env("HOME", &self.home)
                .env("USERPROFILE", &self.home)
                .env("SHELL", "/bin/bash")
                .env_remove("CLAUDE_PROJECT_DIR"),
            &guard_payload(command, self.dir.path()),
        )
    }

    fn refuses(&self, command: &str, store: Option<&str>) {
        for guard in ["git-guard", "exec-guard"] {
            let out = self.run(guard, command);
            let text = String::from_utf8_lossy(&out.stderr);
            assert_eq!(out.status.code(), Some(2), "{guard}: {command}: {out:?}");
            assert!(
                text.contains("security.dangerous_commands"),
                "{guard}: {command}: {text}"
            );
            assert!(
                text.contains("operator") && text.contains("outside the agent session"),
                "{text}"
            );
            if let Some(store) = store {
                assert!(text.contains(store), "{command}: expected {store}: {text}");
            } else {
                assert!(text.contains("cannot verify"), "{command}: {text}");
            }
            assert!(!self.dir.path().join("command-ran").exists());
        }
    }
}

#[test]
fn configured_store_deletion_family_launcher_matrix() {
    let fixture = Fixture::new(true);
    for store in stores() {
        let target = store.strip_suffix("/**").unwrap_or(store);
        let families = [
            format!("rm {target}"),
            format!("rm -f {target}"),
            format!("rm -r {target}"),
            format!("rm -R {target}"),
            format!("rm -rf {target}"),
            format!("rm -f -r {target}"),
            format!("rm --recursive {target}"),
            format!("rmdir {target}"),
            format!("unlink {target}"),
            format!("shred -u {target}"),
            format!("srm {target}"),
            format!("find {target} -delete"),
            format!("find {target} -exec rm -f {{}} +"),
            format!("echo {target} | xargs rm -rf"),
            format!("parallel rm -rf ::: {target}"),
            format!("rsync -a --delete ./source/ {target}"),
            format!("rsync -a --remove-source-files {target} ./destination/"),
        ];
        for command in families {
            for launched in [
                command.clone(),
                format!("env {command}"),
                format!("sh -c {}", quote(&command)),
                format!("bash -c {}", quote(&command)),
                format!("eval {}", quote(&command)),
                format!("command {command}"),
                format!("nohup {command}"),
                format!("exec {command}"),
                format!("printf '' | xargs sh -c {}", quote(&command)),
            ] {
                fixture.refuses(&launched, Some(store));
            }
        }
    }
    for target in ["~/.ssh/id_rsa", "~/.ssh/id_rsa.pub", "~/.ssh/known_hosts"] {
        fixture.refuses(&format!("unlink {target}"), Some("~/.ssh/**"));
    }
}

#[test]
fn secret_deletion_spellings_and_other_destructive_forms() {
    let fixture = Fixture::new(true);
    let home = codeflow_core::portable_path::slashed(&fixture.home);
    for command in [
        "rm -rf ~/.ssh".into(),
        "rm -rf ~//.ssh".into(),
        "rm -rf $HOME/.ssh".into(),
        "rm -rf ${HOME}/.ssh".into(),
        format!("rm -rf {home}/.ssh"),
        "cd home; rm -rf .ssh".into(),
        "d=~/.ssh; rm -rf \"$d\"".into(),
        "rm -rf ~/.ss?".into(),
        "mv ~/.ssh ./moved".into(),
        "truncate -s 0 ~/.ssh/id_rsa".into(),
        ": > ~/.ssh/id_rsa".into(),
        "printf 'replacement' > ~/.ssh/id_rsa".into(),
    ] {
        fixture.refuses(&command, Some("~/.ssh/**"));
    }
    for command in [
        "rm -rf home",
        "git clean -fdx home",
        "git -C home clean -fdx",
    ] {
        fixture.refuses(command, Some("~/"));
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(fixture.home.join(".ssh"), fixture.dir.path().join("alias"))
            .unwrap();
        fixture.refuses("rm -rf alias/", Some("~/.ssh/**"));
        fixture.refuses("rm -rf alias/../.ssh", Some("~/.ssh/**"));
    }
}

#[test]
fn unread_deletion_targets_fail_closed_even_beside_ordinary_operands() {
    let fixture = Fixture::new(true);
    for command in [
        "xargs --arg-file=paths rm -rf ./target",
        "rm --unknown-mode ./target",
        "env --unknown-launcher-mode rm -rf ./target",
        "command --unknown-mode rm -rf ./target",
        "unknown-launcher rm -rf ./target",
        "cat paths | xargs rm -rf ./target",
        "echo ./target | unknown-transform | xargs rm -rf ./target",
        "rm -rf \"$UNKNOWN\"",
        "rm -rf \"$(unknown-output)\"",
        "parallel --unknown-input rm -rf ./target",
        "rsync --delete --files-from=paths ./source ./target",
    ] {
        fixture.refuses(command, None);
    }
}

#[test]
fn ordinary_cleanup_and_configured_entry_siblings_stay_allowed() {
    // These siblings are absent from the configured home rules. In
    // particular, SSH public keys and known_hosts are not controls.
    for sibling in [
        "~/.sshx-notes",
        "~/.cargo/config.toml",
        "~/.gem/README.md",
        "~/scratch/x",
    ] {
        for store in stores() {
            assert!(!codeflow_core::security::pattern::matches_extended_glob(
                sibling, store
            ));
        }
    }
    for relaxed in [false, true] {
        let fixture = Fixture::new(relaxed);
        for command in [
            "rm -rf ./target",
            "rm ./*.log",
            "printf './target\\n' | xargs rm -rf",
            "rm -rf ~/scratch/x",
            "rm ~/.sshx-notes",
            "rm ~/.cargo/config.toml",
            "rm ~/.gem/README.md",
        ] {
            for guard in ["git-guard", "exec-guard"] {
                let out = fixture.run(guard, command);
                let home_control = command.contains('~');
                if guard == "exec-guard" && home_control && (!relaxed || cfg!(windows)) {
                    assert!(line_allows(&out), "{command}: {out:?}");
                    assert_eq!(out.status.code(), Some(2), "{command}: {out:?}");
                } else {
                    assert!(out.status.success(), "{guard}: {command}: {out:?}");
                }
            }
        }
    }
}
