//! TSK-397 design D, AC-4, by attestation: the shipped configuration of
//! each harness selects the baseline; anything customised beyond a
//! narrower form, unreadable, unknown or unidentified tightens.

use super::*;
use serde_json::json;
use std::fs;

/// A session started at a project whose every file lives under one
/// temporary directory: the shipped default Claude preset as `codeflow
/// update` writes it; the shipped Codex configuration with a user
/// configuration that trusts the project; the shipped Grok profiles with
/// `cf-guard` selected through `GROK_SANDBOX`.
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

const CLAUDE: Harnesses = Harnesses::CLAUDE;
const CODEX: Harnesses = Harnesses::CODEX;
const GROK: Harnesses = Harnesses::GROK;
const CODEX_SHIPPED: &str = include_str!("../../../../assets/base/codex/config.toml");
const GROK_SHIPPED: &str = include_str!("../../../../assets/base/grok/sandbox.toml");

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();
        let root = base.join("project");
        let home = base.join("home");
        fs::create_dir_all(&home).unwrap();
        write(
            &root.join(".claude/settings.json"),
            &serde_json::to_string_pretty(&preset()).unwrap(),
        );
        write(&root.join(".codex/config.toml"), CODEX_SHIPPED);
        write(&root.join(".grok/sandbox.toml"), GROK_SHIPPED);
        let managed = base.join("managed");
        let host = Host {
            session: Some(root.clone()),
            cwd: root.clone(),
            root: root.clone(),
            home: Some(home),
            claude_config_dir: None,
            codex_home: None,
            grok_home: None,
            grok_sandbox: Some("cf-guard".to_string()),
            managed_file: Some(managed.join("managed-settings.json")),
            managed_dir: Some(managed.join("managed-settings.d")),
            claude_unattested: vec![managed.join("com.anthropic.claudecode.plist")],
            codex_unattested: vec![managed.join("codex/config.toml")],
            native_windows: false,
        };
        let fixture = Self { _dir: dir, host };
        fixture.codex_user("");
        fixture
    }

    fn home(&self) -> PathBuf {
        self.host.home.clone().unwrap()
    }

    /// Write the user Codex configuration: one that trusts the project,
    /// followed by `more`.
    fn codex_user(&self, more: &str) {
        write(
            &self.home().join(".codex/config.toml"),
            &format!(
                "{more}\n[projects.{:?}]\ntrust_level = \"trusted\"\n",
                self.host.root.display().to_string()
            ),
        );
    }

    fn project(&self) -> PathBuf {
        self.host.root.join(".claude/settings.json")
    }

    /// Change the project settings with `edit`.
    fn edit_project(&self, edit: impl FnOnce(&mut Value)) {
        let mut value: Value =
            serde_json::from_str(&fs::read_to_string(self.project()).unwrap()).unwrap();
        edit(&mut value);
        write(
            &self.project(),
            &serde_json::to_string_pretty(&value).unwrap(),
        );
    }

    fn outcome(&self, harnesses: Harnesses) -> Outcome {
        outcome(&self.host, harnesses, false, &[])
    }
}

fn assert_tightened(outcome: &Outcome, needle: &str) {
    match outcome {
        Outcome::Tightened(trigger) => assert!(trigger.contains(needle), "{trigger}"),
        Outcome::Baseline => panic!("expected a reason naming {needle}, got the baseline"),
    }
}

/// One reason: the fixture is the baseline for `harnesses`; after `apply`
/// it tightens, and some reason names `needle`.
fn check(harnesses: Harnesses, needle: &str, apply: impl FnOnce(&mut Fixture)) {
    let mut fixture = Fixture::new();
    assert_eq!(
        fixture.outcome(harnesses),
        Outcome::Baseline,
        "{needle}: before"
    );
    apply(&mut fixture);
    let found = triggers(&fixture.host, harnesses);
    assert!(
        found.iter().any(|reason| reason.contains(needle)),
        "{needle}: {found:#?}"
    );
}

/// A form the attestation accepts: still the baseline after `apply`.
fn accepted(harnesses: Harnesses, what: &str, apply: impl FnOnce(&mut Fixture)) {
    let mut fixture = Fixture::new();
    apply(&mut fixture);
    assert_eq!(
        triggers(&fixture.host, harnesses),
        Vec::<String>::new(),
        "{what}"
    );
}

/// The shipped configuration selects the baseline for each identified
/// harness, with every shipped Claude preset.
#[test]
fn the_shipped_configuration_selects_the_baseline() {
    let fixture = Fixture::new();
    for harnesses in [CLAUDE, CODEX, GROK] {
        assert_eq!(
            triggers(&fixture.host, harnesses),
            Vec::<String>::new(),
            "{harnesses:?}"
        );
    }
    for name in ["default.json", "acceptEdits.json", "bypass-sandboxed.json"] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/base/settings")
            .join(name);
        let mut settings: Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
        actions::table().apply_to_claude_preset(&mut settings);
        write(&fixture.project(), &settings.to_string());
        assert_eq!(
            triggers(&fixture.host, CLAUDE),
            Vec::<String>::new(),
            "{name}"
        );
    }
}

/// A payload that identifies no single harness cannot be attested
/// (Codex 17: ambiguous or missing identity with no configuration).
#[test]
fn an_unidentified_call_tightens() {
    let keys = |names: &[&str]| -> Vec<String> { names.iter().map(|n| (*n).to_string()).collect() };
    assert_eq!(
        Harnesses::from_keys(&keys(&["tool_name", "prompt_id", "scratchpad_dir"])),
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
    for ambiguous in [
        keys(&["tool_name", "tool_input", "agent_type", "session_id"]),
        keys(&["turn_id"]),
        keys(&["toolName", "prompt_id", "scratchpad_dir"]),
    ] {
        assert_eq!(Harnesses::from_keys(&ambiguous), Harnesses::ALL);
    }
    let fixture = Fixture::new();
    assert_tightened(
        &fixture.outcome(Harnesses::ALL),
        "does not identify one harness",
    );
}

#[test]
fn native_windows_tightens() {
    let mut fixture = Fixture::new();
    fixture.host.native_windows = true;
    assert_tightened(&fixture.outcome(CLAUDE), "native Windows");
    assert_tightened(&fixture.outcome(GROK), "native Windows");
    assert_eq!(fixture.outcome(CODEX), Outcome::Baseline);
}

#[test]
fn claude_sandbox_settings_must_be_shipped_or_narrower() {
    check(CLAUDE, "sandbox.enabled", |f| {
        f.edit_project(|v| v["sandbox"]["enabled"] = json!(false));
    });
    check(CLAUDE, "sandbox.enabled", |f| {
        f.edit_project(|v| {
            v["sandbox"].as_object_mut().unwrap().remove("enabled");
        });
    });
    check(CLAUDE, "sandbox.failIfUnavailable", |f| {
        f.edit_project(|v| v["sandbox"]["failIfUnavailable"] = json!(false));
    });
    check(CLAUDE, "sandbox.excludedCommands", |f| {
        f.edit_project(|v| v["sandbox"]["excludedCommands"] = json!(["git"]));
    });
    for key in [
        "enableWeakerNestedSandbox",
        "enableWeakerNetworkIsolation",
        "allowAppleEvents",
        "allowNestedUserNamespaces",
    ] {
        check(CLAUDE, key, |f| {
            f.edit_project(|v| v["sandbox"][key] = json!(true));
        });
    }
    // A key the attestation does not list is not attested.
    for key in ["ignoreViolations", "somethingNew"] {
        check(CLAUDE, key, |f| {
            f.edit_project(|v| v["sandbox"][key] = json!({}));
        });
    }
    check(CLAUDE, "sandbox.filesystem.disabled", |f| {
        f.edit_project(|v| v["sandbox"]["filesystem"]["disabled"] = json!(true));
    });
    check(CLAUDE, "allowAllUnixSockets", |f| {
        f.edit_project(|v| v["sandbox"]["network"]["allowAllUnixSockets"] = json!(true));
    });
    check(CLAUDE, "allowUnixSockets", |f| {
        f.edit_project(|v| {
            v["sandbox"]["network"]["allowUnixSockets"] = json!(["/var/run/docker.sock"]);
        });
    });
    // C16-2: reads and writes beyond the shipped ones.
    check(CLAUDE, "allowRead", |f| {
        f.edit_project(|v| v["sandbox"]["filesystem"]["allowRead"] = json!(["~/.ssh/config"]));
    });
    for entry in ["~/.zshrc", "~/.gitconfig", "/", "."] {
        check(CLAUDE, entry, |f| {
            f.edit_project(|v| v["sandbox"]["filesystem"]["allowWrite"] = json!([entry]));
        });
    }
    check(CLAUDE, "additionalDirectories", |f| {
        f.edit_project(|v| v["permissions"]["additionalDirectories"] = json!(["/"]));
    });
    check(CLAUDE, "permissions.unknown", |f| {
        f.edit_project(|v| v["permissions"]["unknown"] = json!(true));
    });
    check(CLAUDE, "ANTHROPIC_API_KEY", |f| {
        f.edit_project(|v| v["sandbox"]["credentials"]["envVars"] = json!([]));
    });
    check(CLAUDE, "Edit(/.claude/settings.json)", |f| {
        f.edit_project(|v| {
            v["permissions"]["deny"]
                .as_array_mut()
                .unwrap()
                .retain(|rule| rule != "Edit(/.claude/settings.json)");
            v["sandbox"]["filesystem"]["denyWrite"]
                .as_array_mut()
                .unwrap()
                .retain(|path| path != "./.claude/settings.json");
        });
    });
    // Narrower forms stay attested.
    accepted(CLAUDE, "more denies and fewer grants", |f| {
        f.edit_project(|v| {
            v["sandbox"]["filesystem"]["denyWrite"]
                .as_array_mut()
                .unwrap()
                .push(json!("~/.extra"));
            v["sandbox"]["filesystem"]["allowWrite"] = json!(["~/.codeflow/locks"]);
            v["sandbox"]["network"]["allowedDomains"] = json!(["github.com"]);
            v["permissions"]["allow"] = json!(["Bash(ls:*)"]);
            v["model"] = json!("opus");
        });
    });
}

#[test]
fn every_claude_layer_is_read() {
    // The user layer.
    check(CLAUDE, "settings.json", |f| {
        write(
            &f.home().join(".claude/settings.json"),
            r#"{"sandbox":{"excludedCommands":["git"]}}"#,
        );
    });
    // CLAUDE_CONFIG_DIR moves it.
    check(CLAUDE, "excludedCommands", |f| {
        let dir = f.home().join("claude-config");
        write(
            &dir.join("settings.json"),
            r#"{"sandbox":{"excludedCommands":["git"]}}"#,
        );
        f.host.claude_config_dir = Some(dir);
    });
    // The managed file and drop-ins.
    check(CLAUDE, "managed-settings.json", |f| {
        write(
            f.host.managed_file.as_ref().unwrap(),
            r#"{"sandbox":{"enabled":false}}"#,
        );
    });
    check(CLAUDE, "10-x.json", |f| {
        write(
            &f.host.managed_dir.as_ref().unwrap().join("10-x.json"),
            r#"{"sandbox":{"excludedCommands":["git"]}}"#,
        );
    });
    // The local layer.
    check(CLAUDE, "settings.local.json", |f| {
        write(
            &f.host.root.join(".claude/settings.local.json"),
            r#"{"sandbox":{"enabled":false}}"#,
        );
    });
    // Unattested host files.
    check(CLAUDE, "managed Claude preferences", |f| {
        write(&f.host.claude_unattested[0], "plist");
    });
    check(CLAUDE, "server-managed Claude settings", |f| {
        write(&f.home().join(".claude/remote-settings.json"), "{}");
    });
    // A Claude call whose environment names no session project.
    check(CLAUDE, "CLAUDE_PROJECT_DIR", |f| f.host.session = None);
}

#[test]
fn an_unusable_settings_file_tightens() {
    check(CLAUDE, "not a JSON object", |f| {
        write(&f.project(), "[1, 2]");
    });
    check(CLAUDE, "not a JSON object", |f| {
        write(&f.project(), "{ not json");
    });
    check(CLAUDE, "not a regular file", |f| {
        let local = f.host.root.join(".claude/settings.local.json");
        fs::create_dir_all(&local).unwrap();
    });
    #[cfg(unix)]
    check(CLAUDE, "not a regular file", |f| {
        let local = f.host.root.join(".claude/settings.local.json");
        let c = std::ffi::CString::new(local.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: a valid NUL-terminated path.
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    });
    #[cfg(unix)]
    check(CLAUDE, "cannot be read", |f| {
        use std::os::unix::fs::PermissionsExt;
        let local = f.host.root.join(".claude/settings.local.json");
        write(&local, "{}");
        fs::set_permissions(&local, fs::Permissions::from_mode(0o000)).unwrap();
        // A process that can read anything (root) cannot test this.
        if fs::read(&local).is_ok() {
            write(&local, "{ unreadable stand-in");
        }
    });
}

/// Codex 17, C17-1: a session started in a subdirectory reads the
/// repository root's local settings; a linked worktree reads the main
/// checkout's.
#[test]
fn claude_local_settings_at_the_repository_root_and_main_checkout() {
    let mut fixture = Fixture::new();
    let root = fixture.host.root.clone();
    commit_all(&root);
    let sub = root.join("sub");
    write(
        &sub.join(".claude/settings.json"),
        &serde_json::to_string(&preset()).unwrap(),
    );
    fixture.host.session = Some(sub.clone());
    fixture.host.cwd = sub;
    assert_eq!(triggers(&fixture.host, CLAUDE), Vec::<String>::new());
    write(
        &root.join(".claude/settings.local.json"),
        r#"{"sandbox":{"enabled":false}}"#,
    );
    assert_tightened(&fixture.outcome(CLAUDE), "settings.local.json");

    let mut fixture = Fixture::new();
    let main = fixture.host.root.clone();
    let repo = commit_all(&main);
    let linked = main.parent().unwrap().join("linked");
    repo.worktree("linked", &linked, None).unwrap();
    fixture.host.session = Some(linked.clone());
    fixture.host.cwd = linked.clone();
    fixture.host.root = linked;
    fixture.codex_user("");
    assert_eq!(triggers(&fixture.host, CLAUDE), Vec::<String>::new());
    write(
        &main.join(".claude/settings.local.json"),
        r#"{"sandbox":{"excludedCommands":["git"]}}"#,
    );
    assert_tightened(&fixture.outcome(CLAUDE), "excludedCommands");
}

/// Codex: the shipped configuration under a trusting user configuration
/// is the baseline; everything else here is not attested.
#[test]
fn codex_configuration_must_be_shipped_or_narrower() {
    check(CODEX, "approval_policy", |f| {
        f.codex_user("approval_policy = \"on-request\"");
    });
    // F2-3: the user layer alone is judged, not only under the project.
    check(CODEX, "default_permissions", |f| {
        f.codex_user("default_permissions = \"wide\"");
    });
    // Without trust, Codex skips the project layer; the user layer alone
    // does not select the shipped profile.
    check(CODEX, "does not mark", |f| {
        write(&f.home().join(".codex/config.toml"), "");
    });
    check(CODEX, "user Codex configuration", |f| {
        fs::remove_file(f.home().join(".codex/config.toml")).unwrap();
    });
    // F2-2: a profile table or profile file that sets containment.
    check(CODEX, "profile `loose`", |f| {
        f.codex_user("profile = \"loose\"\n[profiles.loose]\napproval_policy = \"on-request\"");
    });
    check(CODEX, "loose.config.toml", |f| {
        write(
            &f.home().join(".codex/loose.config.toml"),
            "default_permissions = \"wide\"\n",
        );
    });
    // C17-2: a specific read inside a denied region, a workspace secret
    // read, an extra workspace root, the network proxy off.
    let project = |f: &Fixture, from: &str, to: &str| {
        let path = f.host.root.join(".codex/config.toml");
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains(from), "{from}");
        write(&path, &text.replacen(from, to, 1));
    };
    check(CODEX, "`~/.ssh/config` = `read`", |f| {
        project(
            f,
            "\"~/.ssh/**\" = \"deny\"\n",
            "\"~/.ssh/**\" = \"deny\"\n\"~/.ssh/config\" = \"read\"\n",
        );
    });
    check(CODEX, "`.env.local` = `read`", |f| {
        project(
            f,
            "\".env\" = \"deny\"\n",
            "\".env\" = \"deny\"\n\".env.local\" = \"read\"\n",
        );
    });
    check(CODEX, "workspace_roots", |f| {
        project(
            f,
            "[permissions.cf-guard]\n",
            "[permissions.cf-guard]\nworkspace_roots = [\"/\"]\n",
        );
    });
    check(CODEX, "network_proxy", |f| {
        project(f, "network_proxy = true", "network_proxy = false");
    });
    check(CODEX, "`/` = `write`", |f| {
        project(
            f,
            "\"~/.ssh/**\" = \"deny\"\n",
            "\"~/.ssh/**\" = \"deny\"\n\"/\" = \"write\"\n",
        );
    });
    check(CODEX, "every shipped entry", |f| {
        project(f, "\"~/.aws/**\" = \"deny\"\n", "");
    });
    check(CODEX, "allow_local_binding", |f| {
        project(
            f,
            "allow_local_binding = false",
            "allow_local_binding = true",
        );
    });
    check(CODEX, "sandbox_mode", |f| {
        f.codex_user("sandbox_mode = \"danger-full-access\"");
    });
    check(CODEX, "not a key the guard knows", |f| {
        f.codex_user("brand_new_key = 1");
    });
    check(CODEX, "not a feature the guard knows", |f| {
        f.codex_user("[features]\nbrand_new_feature = true");
    });
    check(CODEX, "Codex system configuration", |f| {
        write(&f.host.codex_unattested[0], "");
    });
    // Narrower forms stay attested.
    accepted(CODEX, "an extra deny and a model", |f| {
        project(
            f,
            "\"~/.ssh/**\" = \"deny\"\n",
            "\"~/.ssh/**\" = \"deny\"\n\"~/private/**\" = \"deny\"\n",
        );
        f.codex_user("model = \"gpt-6-astra\"\n[tui]\ntheme = \"dark\"");
    });
}

/// Grok: the selected profile must be a shipped one, defined in the
/// shipped form or a narrower one, in every profiles file read.
#[test]
fn grok_profiles_must_be_shipped_or_narrower() {
    check(GROK, "no Grok sandbox profile is selected", |f| {
        f.host.grok_sandbox = None;
    });
    check(GROK, "not a shipped one", |f| {
        f.host.grok_sandbox = Some("off".to_string());
    });
    // An empty `GROK_SANDBOX` is unset; one that is not UTF-8 selects no
    // shipped profile, even when the user configuration selects one.
    assert_eq!(grok_selection(Some("".into())), None);
    #[cfg(unix)]
    check(GROK, "not a shipped one", |f| {
        use std::os::unix::ffi::OsStringExt;
        f.host.grok_sandbox = grok_selection(Some(std::ffi::OsString::from_vec(vec![0xff])));
        write(
            &f.home().join(".grok/config.toml"),
            "[sandbox]\nprofile = \"cf-guard\"\n",
        );
    });
    accepted(GROK, "the user configuration selects cf-guard", |f| {
        f.host.grok_sandbox = None;
        write(
            &f.home().join(".grok/config.toml"),
            "[sandbox]\nprofile = \"cf-guard\"\n",
        );
    });
    check(GROK, "not a shipped one", |f| {
        f.host.grok_sandbox = None;
        write(
            &f.home().join(".grok/config.toml"),
            "[sandbox]\nprofile = \"workspace\"\n",
        );
    });
    let project = |f: &Fixture, from: &str, to: &str| {
        let path = f.host.root.join(".grok/sandbox.toml");
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains(from), "{from}");
        write(&path, &text.replacen(from, to, 1));
    };
    // F2-1: a write grant.
    check(GROK, "grants the write `/`", |f| {
        project(
            f,
            "restrict_network = false\nread_only",
            "restrict_network = false\nread_write = [\"/\"]\nread_only",
        );
    });
    check(GROK, "lacks the shipped `deny` entry", |f| {
        project(f, "  \"/Users/*/.ssh\",\n", "");
    });
    check(GROK, "not a string", |f| {
        project(f, "read_only = [\n", "read_only = [\n  1,\n");
    });
    check(GROK, "sets `mystery`", |f| {
        project(
            f,
            "restrict_network = false\nread_only",
            "restrict_network = false\nmystery = true\nread_only",
        );
    });
    // C17-3: the user profiles file wins over the project's.
    check(GROK, "lacks the shipped `deny` entry", |f| {
        write(
            &f.home().join(".grok/sandbox.toml"),
            "[profiles.cf-guard]\nextends = \"workspace\"\n",
        );
    });
    check(GROK, "no profiles file the guard reads defines it", |f| {
        fs::remove_file(f.host.root.join(".grok/sandbox.toml")).unwrap();
    });
    accepted(GROK, "an extra deny", |f| {
        project(f, "deny = [\n", "deny = [\n  \"/Users/*/.private\",\n");
    });
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
            &[],
        );
        if retried {
            assert_tightened(&outcome, "dangerouslyDisableSandbox");
        } else {
            assert_eq!(outcome, Outcome::Baseline);
        }
    }
    let odd =
        r#"{"tool_name":"Bash","tool_input":{"command":"true","dangerouslyDisableSandbox":"yes"}}"#;
    assert!(super::super::git_guard::HookPayload::parse(odd)
        .unwrap()
        .tool_input
        .sandbox_retry());
}

/// J4: a policy reason only adds the strict rule.
#[test]
fn a_policy_reason_never_loosens() {
    let fixture = Fixture::new();
    let strict = vec!["the project policy sets `security.text_guard` to `strict`".to_string()];
    assert_tightened(
        &outcome(&fixture.host, CLAUDE, false, &strict),
        "security.text_guard",
    );
    fixture.edit_project(|v| v["sandbox"]["enabled"] = json!(false));
    for policy in [Vec::new(), strict] {
        assert!(matches!(
            outcome(&fixture.host, CLAUDE, false, &policy),
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

/// Initialise a repository at `dir` and commit every file in it.
fn commit_all(dir: &Path) -> git2::Repository {
    let repo = git2::Repository::init(dir).unwrap();
    let tree = {
        let mut index = repo.index().unwrap();
        index
            .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
            .unwrap();
        index.write().unwrap();
        index.write_tree().unwrap()
    };
    let signature = git2::Signature::now("t", "t@example.invalid").unwrap();
    repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        "seed",
        &repo.find_tree(tree).unwrap(),
        &[],
    )
    .unwrap();
    repo
}

/// Write `text` at `path`, creating its directory.
fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}
