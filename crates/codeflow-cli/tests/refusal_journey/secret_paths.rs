//! Installed hook payloads and the native filesystem backstop are separate
//! fixtures: replay does not establish a harness's live dispatch or sandbox.
use super::*;

#[test]
fn installed_hooks_refuse_secret_path_spellings_without_execution() {
    let project = Project::new();
    let marker = project.root.join("command-ran");
    for harness in ["claude", "codex", "grok"] {
        // The core tests carry the full Cartesian matrix. At the installed
        // boundary, cover each spelling and format once in every harness.
        for path in [
            "id_rsa".to_string(),
            "./id_ed25519".into(),
            "nested/cert.pem".into(),
            format!(
                "{}/key.key",
                codeflow_core::portable_path::slashed(&project.root)
            ),
            "$HOME/cert.p12".into(),
            "${HOME}/cert.pfx".into(),
            "~/.netrc".into(),
        ] {
            let command = format!("cat {path} > command-ran");
            project.shell(harness, &command, Some("security.secret_reads"));
            assert!(!marker.exists(), "hook executed {command}");
        }
        for store in &codeflow_core::security::actions::table().sandbox_read_denies {
            let suffix = store.strip_prefix("~/").unwrap();
            let path = project.home.join(suffix);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "planted fixture\n").unwrap();
            project.shell(
                harness,
                &format!("cat {store}"),
                Some("security.secret_reads"),
            );
            // Every store is covered above; auth.json witnesses the remaining
            // home and cwd spellings at the installed boundary.
            if suffix != ".codex/auth.json" {
                continue;
            }
            for spelling in [
                format!("$HOME/{suffix}"),
                format!("${{HOME}}/{suffix}"),
                codeflow_core::portable_path::slashed(&path),
            ] {
                project.shell(
                    harness,
                    &format!("cat {spelling}"),
                    Some("security.secret_reads"),
                );
            }
            let basename = path.file_name().unwrap().to_str().unwrap();
            for spelling in [basename.to_string(), format!("./{basename}")] {
                let outputs = project.replay_payload(
                    harness,
                    "Bash",
                    &if harness == "grok" {
                        json!({"toolName":"Bash", "cwd":path.parent().unwrap(),
                        "toolInput":{"command":format!("cat {spelling}")}})
                    } else {
                        json!({"tool_name":"Bash", "cwd":path.parent().unwrap(),
                        "tool_input":{"command":format!("cat {spelling}")}})
                    },
                );
                assert!(
                    outputs.iter().any(|out| out.status.code() == Some(2)
                        && String::from_utf8_lossy(&out.stderr).contains("security.secret_reads")),
                    "{harness}: {spelling}: {outputs:?}"
                );
            }
        }
        for command in [
            "cat ~//.codex/auth.json",
            "cat ~/.Codex/AUTH.JSON",
            "cat ~/.codex/auth.{json}",
            "wc < ~/.codex/auth.json",
            "cd ~/.codex && cat auth.json",
        ] {
            project.shell(harness, command, Some("security.secret_reads"));
        }
        for path in [
            "README.md",
            "private.key.rs",
            "~/.ssh-notes/file",
            ".env.example",
            ".env.production",
            "secret_scan.rs",
            "credentials.json",
        ] {
            project.shell(harness, &format!("cat {path}"), None);
        }
    }
}

#[cfg(target_os = "macos")]
fn native_secret_read_profile(settings: &Value, home: &std::path::Path) -> (String, Vec<PathBuf>) {
    use std::fmt::Write as _;
    let mut profile = String::from("(version 1)(allow default)");
    let mut paths = Vec::new();
    for store in settings["sandbox"]["filesystem"]["denyRead"]
        .as_array()
        .unwrap()
    {
        let suffix = store.as_str().unwrap().strip_prefix("~/").unwrap();
        let path = home.join(suffix);
        // Treat each configured root as a subpath deny, just as the native
        // read backstop does; seed a child for the directory store.
        let file = if matches!(
            suffix,
            ".ssh" | ".aws" | ".gnupg" | ".kube" | ".claude" | ".password-store"
        ) {
            path.join("entry")
        } else {
            path.clone()
        };
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "planted fixture\n").unwrap();
        write!(
            profile,
            "(deny file-read* (subpath {}))",
            serde_json::to_string(&path).unwrap()
        )
        .unwrap();
        paths.push(file);
    }
    (profile, paths)
}

/// Exercise the OS with paths from the installed denyRead list, without
/// invoking a guard. This is a native macOS primitive test, not a claim that
/// any running harness adopted the generated profile. Linux/WSL and Windows
/// need their own native evidence; they do not run this Seatbelt fixture.
#[cfg(target_os = "macos")]
#[test]
fn native_macos_sandbox_denies_stores_and_leaves_workspace_files_readable() {
    let project = Project::new();
    let settings: Value =
        serde_json::from_slice(&std::fs::read(project.root.join(".claude/settings.json")).unwrap())
            .unwrap();
    let home = project.home.canonicalize().unwrap();
    let (profile, paths) = native_secret_read_profile(&settings, &home);
    for path in &paths {
        success(&Command::new("/bin/cat").arg(path).output().unwrap());
        for spelling in [
            path.clone(),
            PathBuf::from(path.file_name().unwrap()),
            PathBuf::from(".").join(path.file_name().unwrap()),
        ] {
            let out = Command::new("/usr/bin/sandbox-exec")
                .current_dir(path.parent().unwrap())
                .args(["-p", &profile, "/bin/cat"])
                .arg(&spelling)
                .output()
                .unwrap();
            assert!(!out.status.success(), "sandbox read {}", spelling.display());
            assert!(out.stdout.is_empty(), "sandbox leaked fixture");
            assert!(
                String::from_utf8_lossy(&out.stderr).contains("Operation not permitted"),
                "{out:?}"
            );
        }
    }
    // File-tool rules are a separate plane from native store-root denies.
    assert!(settings["permissions"]["deny"]
        .as_array()
        .unwrap()
        .iter()
        .any(|rule| rule == "Read(**/id_rsa*)"));
    let key = project.root.join("id_rsa");
    std::fs::write(&key, "planted fixture\n").unwrap();
    let out = Command::new("/usr/bin/sandbox-exec")
        .args(["-p", &profile, "/bin/cat"])
        .arg(&key)
        .output()
        .unwrap();
    success(&out);
    assert_eq!(out.stdout, b"planted fixture\n");
    for name in ["README.md", ".env"] {
        let path = project.root.join(name);
        std::fs::write(&path, "ordinary fixture\n").unwrap();
        success(
            &Command::new("/usr/bin/sandbox-exec")
                .args(["-p", &profile, "/bin/cat"])
                .arg(path)
                .output()
                .unwrap(),
        );
    }
}

#[cfg(target_os = "macos")]
#[test]
fn tsk1002_review2_b06_native_store_profile_leaves_workspace_key_readable() {
    let project = Project::new();
    let settings: Value =
        serde_json::from_slice(&std::fs::read(project.root.join(".claude/settings.json")).unwrap())
            .unwrap();
    assert!(settings["permissions"]["deny"]
        .as_array()
        .unwrap()
        .iter()
        .any(|rule| rule == "Read(**/id_rsa*)"));
    let (profile, _) = native_secret_read_profile(&settings, &project.home.canonicalize().unwrap());
    let key = project.root.join("id_rsa");
    let contents = "fake workspace key fixture\n";
    std::fs::write(&key, contents).unwrap();
    let native = Command::new("/usr/bin/sandbox-exec")
        .args(["-p", &profile, "/bin/cat"])
        .arg(&key)
        .output()
        .unwrap();
    success(&native);
    assert_eq!(native.stdout, contents.as_bytes());
    for harness in ["claude", "codex", "grok"] {
        project.shell(harness, "cat id_rsa", Some("security.secret_reads"));
    }
}

#[path = "../support/guard_cases.rs"]
mod guard_cases;

#[path = "../support/guard_review_cases.rs"]
mod guard_review_cases;

#[test]
fn tsk1002_review_installed_hooks_refuse_executable_inputs() {
    let project = Project::new();
    guard_review_cases::seed(&project.root, &project.home);
    #[cfg(unix)]
    guard_review_cases::seed_links(&project.root, &project.home);
    let mut wrong = Vec::new();
    // One representative per mechanism through each installed hook route;
    // hooks_cli covers the complete payload table and native Windows names.
    for harness in ["claude", "codex", "grok"] {
        for finding in [1, 2, 3, 4, 5, 6, 8, 9] {
            #[cfg(not(unix))]
            if finding == 8 {
                continue;
            }
            let &(_, owner, command, refuse) = guard_review_cases::CASES
                .iter()
                .find(|c| c.0 == finding)
                .unwrap();
            let line = guard_review_cases::render(command, &project.root, &project.home);
            let outputs = project.replay(harness, "Bash", json!({"command":line}));
            let expected_rule = if owner == "git-guard" || finding == 6 {
                "git.hook_integrity"
            } else {
                "security.secret_reads"
            };
            let refused = outputs.iter().any(|out| {
                out.status.code() == Some(2)
                    && String::from_utf8_lossy(&out.stderr).contains(expected_rule)
            });
            if (refuse && !refused) || (!refuse && outputs.iter().any(|out| !out.status.success()))
            {
                wrong.push(format!("{harness}: F{finding}: {line:?}: {outputs:?}"));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn tsk1002_ac14_installed_hooks_classify_every_planted_case() {
    let project = Project::new();
    guard_cases::seed_glob_matches(&project.root);
    let mut wrong = Vec::new();
    for harness in ["claude", "codex", "grok"] {
        for &(ac, line, refuse) in guard_cases::CASES {
            // replay() invokes the installed harness's hook commands. It never
            // invokes the planted shell text as a command.
            let outputs = project.replay(harness, "Bash", json!({"command": line}));
            let expected_rule =
                if guard_cases::refusing_guard(ac, line) == "git-guard" || ac == 5 || ac == 2 {
                    "git.hook_integrity"
                } else {
                    "security.secret_reads"
                };
            let refused = outputs.iter().any(|out| {
                out.status.code() == Some(2)
                    && String::from_utf8_lossy(&out.stderr).contains(expected_rule)
            });
            if (refuse && !refused) || (!refuse && outputs.iter().any(|out| !out.status.success()))
            {
                wrong.push(format!(
                    "{harness}: AC-{ac}: expected refuse={refuse}: {line:?}: {outputs:?}"
                ));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
