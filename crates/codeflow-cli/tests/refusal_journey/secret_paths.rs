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

/// Exercise the OS with paths from the installed denyRead list, without
/// invoking a guard. This is a native macOS primitive test, not a claim that
/// any running harness adopted the generated profile. Linux/WSL and Windows
/// need their own native evidence; they do not run this Seatbelt fixture.
#[cfg(target_os = "macos")]
#[test]
fn native_macos_sandbox_denies_stores_but_not_workspace_key_names() {
    use std::fmt::Write as _;
    let project = Project::new();
    let settings: Value =
        serde_json::from_slice(&std::fs::read(project.root.join(".claude/settings.json")).unwrap())
            .unwrap();
    let home = project.home.canonicalize().unwrap();
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
    for name in ["README.md", "id_rsa", ".env"] {
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
