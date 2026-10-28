//! TSK-397 design D, AC-4: each trigger selects the tightened outcome when
//! present and the baseline when absent with every other trigger absent.

use super::*;
use serde_json::json;
use std::fs;

/// A host whose every file lives under one temporary directory, with the
/// shipped default preset, as `codeflow update` writes it, as the
/// project's Claude settings.
struct Fixture {
    _dir: tempfile::TempDir,
    host: Host,
}

fn preset() -> Value {
    let mut settings: Value = serde_json::from_str(include_str!(
        "../../../../assets/base/settings/default.json"
    ))
    .unwrap();
    actions::table().apply_to_claude_preset(&mut settings);
    settings
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        let home = dir.path().join("home");
        fs::create_dir_all(root.join(".claude")).unwrap();
        fs::create_dir_all(&home).unwrap();
        fs::write(
            root.join(".claude/settings.json"),
            serde_json::to_string_pretty(&preset()).unwrap(),
        )
        .unwrap();
        let managed = dir.path().join("managed");
        let host = Host {
            root,
            main_root: Ok(None),
            home: Some(home),
            claude_config_dir: None,
            codex_home: None,
            managed_file: Some(managed.join("managed-settings.json")),
            managed_dir: Some(managed.join("managed-settings.d")),
            native_windows: false,
        };
        Self { _dir: dir, host }
    }

    fn project(&self) -> PathBuf {
        self.host.root.join(".claude/settings.json")
    }

    /// Change the project settings with `edit`.
    fn edit_project(&self, edit: impl FnOnce(&mut Value)) {
        let mut value: Value =
            serde_json::from_str(&fs::read_to_string(self.project()).unwrap()).unwrap();
        edit(&mut value);
        fs::write(
            self.project(),
            serde_json::to_string_pretty(&value).unwrap(),
        )
        .unwrap();
    }

    fn outcome(&self, harnesses: Harnesses) -> Outcome {
        outcome(&self.host, harnesses, false, false)
    }
}

const CLAUDE: Harnesses = Harnesses {
    claude: true,
    codex: false,
    grok: false,
};
const CODEX: Harnesses = Harnesses {
    claude: false,
    codex: true,
    grok: false,
};
const GROK: Harnesses = Harnesses {
    claude: false,
    codex: false,
    grok: true,
};

fn assert_tightened(outcome: &Outcome, needle: &str) {
    match outcome {
        Outcome::Tightened(trigger) => assert!(trigger.contains(needle), "{trigger}"),
        Outcome::Baseline => panic!("expected a trigger naming {needle}, got the baseline"),
    }
}

/// One trigger: the fixture with `apply` tightens, naming `needle`; the
/// same fixture without it is the baseline.
fn check(harnesses: Harnesses, needle: &str, apply: impl FnOnce(&Fixture)) {
    let fixture = Fixture::new();
    assert_eq!(
        fixture.outcome(harnesses),
        Outcome::Baseline,
        "{needle}: before"
    );
    apply(&fixture);
    assert_tightened(&fixture.outcome(harnesses), needle);
    assert_tightened(&fixture.outcome(Harnesses::ALL), needle);
}

/// The shipped preset, alone, selects the baseline on macOS and Linux.
#[test]
fn the_shipped_preset_selects_the_baseline() {
    let fixture = Fixture::new();
    assert_eq!(fixture.outcome(Harnesses::ALL), Outcome::Baseline);
    assert_eq!(
        triggers(&fixture.host, Harnesses::ALL),
        Vec::<String>::new()
    );
    // Each shipped preset as `codeflow update` writes it.
    for name in ["default.json", "acceptEdits.json", "bypass-sandboxed.json"] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/base/settings")
            .join(name);
        let mut settings: Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
        actions::table().apply_to_claude_preset(&mut settings);
        write(&fixture.project(), &settings.to_string());
        assert_eq!(fixture.outcome(Harnesses::ALL), Outcome::Baseline, "{name}");
    }
}

/// Native Windows has no sandbox under Claude Code or Grok Build.
#[test]
fn native_windows_tightens() {
    let mut fixture = Fixture::new();
    fixture.host.native_windows = true;
    assert_tightened(&fixture.outcome(CLAUDE), "native Windows");
    assert_tightened(&fixture.outcome(GROK), "native Windows");
    assert_eq!(fixture.outcome(CODEX), Outcome::Baseline);
}

#[test]
fn a_disabled_or_unset_sandbox_tightens() {
    check(CLAUDE, "sandbox.enabled", |f| {
        f.edit_project(|v| v["sandbox"]["enabled"] = json!(false));
    });
    check(CLAUDE, "sandbox.enabled", |f| {
        f.edit_project(|v| {
            v["sandbox"].as_object_mut().unwrap().remove("enabled");
        });
    });
    check(CLAUDE, "sandbox.filesystem.disabled", |f| {
        f.edit_project(|v| v["sandbox"]["filesystem"]["disabled"] = json!(true));
    });
    // A higher layer wins: the local file turns the sandbox off.
    check(CLAUDE, "sandbox.enabled", |f| {
        write(
            &f.host.root.join(".claude/settings.local.json"),
            r#"{"sandbox":{"enabled":false}}"#,
        );
    });
    // The managed layer outranks the project.
    check(CLAUDE, "sandbox.enabled", |f| {
        write(
            f.host.managed_file.as_ref().unwrap(),
            r#"{"sandbox":{"enabled":false}}"#,
        );
    });
    // A lower layer does not: the user's `false` loses to the project's
    // `true`.
    let fixture = Fixture::new();
    write(
        &fixture
            .host
            .home
            .as_ref()
            .unwrap()
            .join(".claude/settings.json"),
        r#"{"sandbox":{"enabled":false}}"#,
    );
    assert_eq!(fixture.outcome(CLAUDE), Outcome::Baseline);
}

#[test]
fn a_fail_open_sandbox_tightens() {
    check(CLAUDE, "sandbox.failIfUnavailable", |f| {
        f.edit_project(|v| v["sandbox"]["failIfUnavailable"] = json!(false));
    });
}

/// The two local layers of a worktree session disagree: the value cannot
/// be computed.
#[test]
fn disagreeing_layers_of_one_rank_tighten() {
    let mut fixture = Fixture::new();
    let main = fixture.host.root.parent().unwrap().join("main");
    write(
        &main.join(".claude/settings.local.json"),
        r#"{"sandbox":{"enabled":false}}"#,
    );
    write(
        &fixture.host.root.join(".claude/settings.local.json"),
        r#"{"sandbox":{"enabled":true}}"#,
    );
    fixture.host.main_root = Ok(Some(main));
    assert_tightened(&fixture.outcome(CLAUDE), "disagree");
    fixture.host.main_root = Err("no repository".to_string());
    assert_tightened(&fixture.outcome(CLAUDE), "main checkout");
}

/// A missing floor entry tightens, whichever mirror carries it: removing
/// the `Edit` deny alone leaves the `denyWrite` entry, and removing both
/// tightens.
#[test]
fn a_missing_floor_entry_tightens() {
    check(CLAUDE, "Edit(/.claude/settings.json)", |f| {
        f.edit_project(|v| {
            v["permissions"]["deny"]
                .as_array_mut()
                .unwrap()
                .retain(|r| r != "Edit(/.claude/settings.json)");
            v["sandbox"]["filesystem"]["denyWrite"]
                .as_array_mut()
                .unwrap()
                .retain(|r| r != "./.claude/settings.json");
        });
    });
    let fixture = Fixture::new();
    fixture.edit_project(|v| {
        v["permissions"]["deny"]
            .as_array_mut()
            .unwrap()
            .retain(|r| r != "Edit(/.claude/settings.json)");
    });
    assert_eq!(fixture.outcome(CLAUDE), Outcome::Baseline);
    check(CLAUDE, "Read(**/*secret*)", |f| {
        f.edit_project(|v| {
            v["permissions"]["deny"]
                .as_array_mut()
                .unwrap()
                .retain(|r| r != "Read(**/*secret*)");
        });
    });
    // A secret store needs its `denyRead` entry or its `Read(...)` deny;
    // either alone holds the floor, and neither tightens.
    check(CLAUDE, "~/.pypirc", |f| {
        f.edit_project(|v| {
            v["sandbox"]["filesystem"]["denyRead"]
                .as_array_mut()
                .unwrap()
                .retain(|r| r != "~/.pypirc");
            v["permissions"]["deny"]
                .as_array_mut()
                .unwrap()
                .retain(|r| r != "Read(~/.pypirc)");
        });
    });
    let fixture = Fixture::new();
    fixture.edit_project(|v| {
        v["sandbox"]["filesystem"]["denyRead"]
            .as_array_mut()
            .unwrap()
            .retain(|r| r != "~/.pypirc");
    });
    assert_eq!(fixture.outcome(CLAUDE), Outcome::Baseline);
    // A settings file with no sandbox block at all (J5).
    check(CLAUDE, "sandbox.enabled", |f| {
        f.edit_project(|v| {
            v.as_object_mut().unwrap().remove("sandbox");
        });
    });
}

#[test]
fn an_allow_entry_at_a_protected_path_tightens() {
    check(CLAUDE, "allowWrite", |f| {
        f.edit_project(|v| {
            v["sandbox"]["filesystem"]["allowWrite"]
                .as_array_mut()
                .unwrap()
                .push(json!("./.git/hooks"));
        });
    });
    check(CLAUDE, "allowWrite", |f| {
        f.edit_project(|v| {
            v["sandbox"]["filesystem"]["allowWrite"]
                .as_array_mut()
                .unwrap()
                .push(json!("~/.codeflow/authority/receipts"));
        });
    });
    check(CLAUDE, "Edit", |f| {
        f.edit_project(|v| {
            v["permissions"]["allow"]
                .as_array_mut()
                .unwrap()
                .push(json!("Edit(/.git/hooks/pre-commit)"));
        });
    });
    check(CLAUDE, "Edit", |f| {
        f.edit_project(|v| {
            v["permissions"]["allow"]
                .as_array_mut()
                .unwrap()
                .push(json!("Edit"));
        });
    });
    // A glob whose literal start can reach a protected path.
    check(CLAUDE, "allowWrite", |f| {
        f.edit_project(|v| {
            v["sandbox"]["filesystem"]["allowWrite"]
                .as_array_mut()
                .unwrap()
                .push(json!("./.cl*"));
        });
    });
    check(CLAUDE, "additionalDirectories", |f| {
        f.edit_project(|v| {
            v["permissions"]["additionalDirectories"] = json!(["~/.codex/rules"]);
        });
    });
    // An entry that reaches no protected path is not a trigger, nor is
    // one above a protected path: the project directory is writable by
    // default, and the floor's denies take precedence there.
    let fixture = Fixture::new();
    fixture.edit_project(|v| {
        v["sandbox"]["filesystem"]["allowWrite"]
            .as_array_mut()
            .unwrap()
            .push(json!("./.claude"));
        v["sandbox"]["filesystem"]["allowWrite"]
            .as_array_mut()
            .unwrap()
            .push(json!("./target/**"));
        v["permissions"]["allow"]
            .as_array_mut()
            .unwrap()
            .push(json!("Edit(/src/**)"));
    });
    assert_eq!(fixture.outcome(CLAUDE), Outcome::Baseline);
}

/// Any non-empty exclusion list tightens the whole session; no matcher
/// decides which call it covers.
#[test]
fn any_excluded_command_tightens() {
    check(CLAUDE, "excludedCommands", |f| {
        write(
            f.host.managed_file.as_ref().unwrap(),
            r#"{"sandbox":{"excludedCommands":["git","gh"]}}"#,
        );
    });
    check(CLAUDE, "excludedCommands", |f| {
        f.edit_project(|v| v["sandbox"]["excludedCommands"] = json!(["docker"]));
    });
}

#[test]
fn escape_settings_tighten() {
    for (pointer, value) in [
        ("/sandbox/enableWeakerNestedSandbox", json!(true)),
        ("/sandbox/allowNestedUserNamespaces", json!(true)),
        ("/sandbox/network/allowAllUnixSockets", json!(true)),
        (
            "/sandbox/network/allowUnixSockets",
            json!(["/var/run/docker.sock"]),
        ),
        ("/sandbox/ignoreViolations", json!({"*": ["/usr/bin"]})),
    ] {
        let key = pointer.rsplit('/').next().unwrap().to_string();
        check(CLAUDE, &key, |f| {
            f.edit_project(|v| {
                let mut at = v;
                let parts: Vec<&str> = pointer.trim_start_matches('/').split('/').collect();
                for part in &parts[..parts.len() - 1] {
                    at = at
                        .as_object_mut()
                        .unwrap()
                        .entry((*part).to_string())
                        .or_insert_with(|| json!({}));
                }
                at[parts[parts.len() - 1]] = value;
            });
        });
    }
}

/// An existing file that cannot be read tightens; an absent one does not
/// (J2).
#[test]
fn a_malformed_file_tightens_and_an_absent_one_does_not() {
    check(CLAUDE, "not a JSON object", |f| {
        write(
            &f.host.root.join(".claude/settings.local.json"),
            "{ not json",
        );
    });
    check(CLAUDE, "not a JSON object", |f| {
        write(
            &f.host.managed_dir.as_ref().unwrap().join("10-policy.json"),
            "[]",
        );
    });
    check(CODEX, "not valid TOML", |f| {
        write(&f.host.root.join(".codex/config.toml"), "= broken");
    });
    check(GROK, "not valid TOML", |f| {
        write(&f.host.root.join(".grok/sandbox.toml"), "[profiles");
    });
    // No Codex or Grok file at all: no trigger for those harnesses.
    let fixture = Fixture::new();
    assert_eq!(fixture.outcome(CODEX), Outcome::Baseline);
    assert_eq!(fixture.outcome(GROK), Outcome::Baseline);
}

#[cfg(unix)]
#[test]
fn an_unreadable_file_tightens() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    let local = fixture.host.root.join(".claude/settings.local.json");
    write(&local, "{}");
    fs::set_permissions(&local, fs::Permissions::from_mode(0o000)).unwrap();
    // A process that can read the file anyway (root) cannot test this.
    if fs::read(&local).is_err() {
        assert_tightened(&fixture.outcome(CLAUDE), "cannot be read");
    }
    fs::set_permissions(&local, fs::Permissions::from_mode(0o600)).unwrap();
}

fn codex_config() -> String {
    include_str!("../../../../assets/base/codex/config.toml").to_string()
}

#[test]
fn codex_triggers() {
    // The shipped profile selects the baseline.
    let fixture = Fixture::new();
    write(
        &fixture.host.root.join(".codex/config.toml"),
        &codex_config(),
    );
    assert_eq!(fixture.outcome(CODEX), Outcome::Baseline);
    check(CODEX, "sandbox_mode", |f| {
        write(
            &f.host.root.join(".codex/config.toml"),
            &format!("sandbox_mode = \"danger-full-access\"\n{}", codex_config()),
        );
    });
    check(CODEX, "default permission profile", |f| {
        write(
            &f.host.root.join(".codex/config.toml"),
            &codex_config().replace(
                "default_permissions = \"cf-guard\"",
                "default_permissions = \":workspace\"",
            ),
        );
    });
    check(CODEX, "approval policy", |f| {
        write(
            &f.host.root.join(".codex/config.toml"),
            &codex_config().replace(
                "approval_policy = \"never\"",
                "approval_policy = \"on-request\"",
            ),
        );
    });
    // The user layer alone, with no profile, is not the shipped posture.
    check(CODEX, "default permission profile", |f| {
        write(
            &f.host.home.as_ref().unwrap().join(".codex/config.toml"),
            "model = \"x\"\n",
        );
    });
}

#[test]
fn grok_triggers() {
    let shipped = include_str!("../../../../assets/base/grok/sandbox.toml");
    let fixture = Fixture::new();
    write(&fixture.host.root.join(".grok/sandbox.toml"), shipped);
    assert_eq!(fixture.outcome(GROK), Outcome::Baseline);
    check(GROK, "read_only", |f| {
        write(
            &f.host.root.join(".grok/sandbox.toml"),
            &shipped.replacen("  \".codex\",\n", "", 1),
        );
    });
}

/// J1: shared keys never identify a harness. A Claude payload carrying
/// `agent_type`, with a loosened Claude layer, tightens.
#[test]
fn harnesses_are_read_by_the_union_rule() {
    let keys = |names: &[&str]| -> Vec<String> { names.iter().map(|n| (*n).to_string()).collect() };
    assert_eq!(
        Harnesses::from_keys(&keys(&[
            "tool_name",
            "tool_input",
            "agent_type",
            "agent_id",
            "session_id",
            "cwd"
        ])),
        Harnesses::ALL
    );
    assert_eq!(
        Harnesses::from_keys(&keys(&[
            "tool_name",
            "prompt_id",
            "scratchpad_dir",
            "agent_type"
        ])),
        CLAUDE
    );
    assert_eq!(
        Harnesses::from_keys(&keys(&["tool_name", "turn_id", "model"])),
        CODEX
    );
    assert_eq!(
        Harnesses::from_keys(&keys(&["toolName", "toolInput"])),
        GROK
    );
    // `turn_id` alone, or `model` alone, is not Codex.
    assert_eq!(Harnesses::from_keys(&keys(&["turn_id"])), Harnesses::ALL);
    // Unique keys of two harnesses contradict each other: all are read.
    assert_eq!(
        Harnesses::from_keys(&keys(&["toolName", "prompt_id", "scratchpad_dir"])),
        Harnesses::ALL
    );
    let fixture = Fixture::new();
    write(
        &fixture.host.root.join(".codex/config.toml"),
        &codex_config(),
    );
    fixture.edit_project(|v| v["sandbox"]["enabled"] = json!(false));
    let subagent = Harnesses::from_keys(&keys(&["tool_name", "agent_type", "agent_id", "model"]));
    assert_tightened(&fixture.outcome(subagent), "sandbox.enabled");
}

/// AC-3: the recorded Claude Code 2.1.312 retry payload is judged under
/// the strict rule; the same call without the flag follows the host.
#[test]
fn a_recognised_retry_tightens() {
    let fixture = Fixture::new();
    let retry = r#"{"session_id":"9ad9d406","transcript_path":"/t.jsonl","cwd":"/p","permission_mode":"default","hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":"echo canary-unsandboxed","description":"Echo canary string without sandbox","dangerouslyDisableSandbox":true},"tool_use_id":"toolu_1","prompt_id":"p1","scratchpad_dir":"/s","effort":"medium"}"#;
    let plain = r#"{"session_id":"9ad9d406","cwd":"/p","permission_mode":"default","hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":"echo canary-sandboxed","description":"Echo canary string in sandbox"},"prompt_id":"p1","scratchpad_dir":"/s"}"#;
    for (payload, retried) in [(retry, true), (plain, false)] {
        let parsed = super::super::git_guard::HookPayload::parse(payload).unwrap();
        assert_eq!(parsed.tool_input.sandbox_retry(), retried);
        let harnesses = Harnesses::from_keys(&parsed.keys);
        assert_eq!(harnesses, CLAUDE);
        let outcome = outcome(
            &fixture.host,
            harnesses,
            parsed.tool_input.sandbox_retry(),
            false,
        );
        if retried {
            assert_tightened(&outcome, "dangerouslyDisableSandbox");
        } else {
            assert_eq!(outcome, Outcome::Baseline);
        }
    }
    // A flag of another type tightens rather than passes.
    let odd =
        r#"{"tool_name":"Bash","tool_input":{"command":"true","dangerouslyDisableSandbox":"yes"}}"#;
    assert!(super::super::git_guard::HookPayload::parse(odd)
        .unwrap()
        .tool_input
        .sandbox_retry());
}

/// J4: `security.text_guard` only adds the strict rule. `strict` tightens
/// a clean host, and no value selects the baseline on a tightened host.
#[test]
fn the_policy_key_never_loosens() {
    let fixture = Fixture::new();
    assert_tightened(
        &outcome(&fixture.host, Harnesses::ALL, false, true),
        "security.text_guard",
    );
    fixture.edit_project(|v| v["sandbox"]["enabled"] = json!(false));
    for policy_strict in [false, true] {
        assert!(matches!(
            outcome(&fixture.host, Harnesses::ALL, false, policy_strict),
            Outcome::Tightened(_)
        ));
    }
}

#[test]
fn the_write_spelling_of_each_edit_deny() {
    assert_eq!(
        actions::edit_deny_write_path("Edit(/.claude/settings.json)").as_deref(),
        Some("./.claude/settings.json")
    );
    assert_eq!(
        actions::edit_deny_write_path("Edit(/.git/hooks/**)").as_deref(),
        Some("./.git/hooks")
    );
    assert_eq!(
        actions::edit_deny_write_path("Edit(~/.codeflow/authority/**)").as_deref(),
        Some("~/.codeflow/authority")
    );
    assert_eq!(
        actions::edit_deny_write_path("Edit(//etc/profile)").as_deref(),
        Some("/etc/profile")
    );
    assert_eq!(actions::edit_deny_write_path("Edit(//**/.envrc)"), None);
}

/// Write `text` at `path`, creating its directory.
fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}
