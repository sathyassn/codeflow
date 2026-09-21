//! Regression tests for the shipped Claude settings presets
//! (`assets/base/settings/*.json`): the scaffold content is the product as
//! much as the code (charter §4.4), so its invariants are tested like code:
//! every hook command must be a known `codeflow hook` subcommand, the
//! secret-file deny rules must stay present, and top-level keys are pinned
//! against typos.

use std::collections::BTreeSet;
use std::path::PathBuf;

/// The expected preset set, pinned so an addition or removal is a conscious
/// choice; `preset_files_match_the_shipped_directory` keeps it honest.
const PRESET_FILES: [&str; 3] = ["default.json", "acceptEdits.json", "bypass-sandboxed.json"];

/// The known hook subcommands wired by the presets (charter §3.3; the
/// `exec-guard` security stage added in ADR-0008).
const HOOK_NAMES: [&str; 4] = [
    "git-guard",
    "exec-guard",
    "session-orient",
    "session-summary",
];

/// Hardcoded union of top-level keys actually used across the three presets.
/// A typo'd or stray key in any preset fails here; a deliberate new key means
/// updating this list in the same change.
const TOP_LEVEL_KEYS: [&str; 6] = [
    "$schema",
    "effortLevel",
    "hooks",
    "permissions",
    "sandbox",
    "statusLine",
];

#[test]
fn primary_effort_settings_default_to_high() {
    for name in preset_files() {
        let bytes = std::fs::read(settings_dir().join(&name)).unwrap();
        let preset: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(preset["effortLevel"], "high", "{name}");
    }
}

/// Claude's private state must stay unreadable without hiding the official
/// plugin runtime under `~/.claude/plugins` from Claude Code itself.
const CLAUDE_SENSITIVE_READ_DENIES: [&str; 17] = [
    "Read(~/.claude/.credentials.json)",
    "Read(~/.claude/backups/**)",
    "Read(~/.claude/debug/**)",
    "Read(~/.claude/file-history/**)",
    "Read(~/.claude/history.jsonl)",
    "Read(~/.claude/mcp-needs-auth-cache.json)",
    "Read(~/.claude/memory/**)",
    "Read(~/.claude/paste-cache/**)",
    "Read(~/.claude/projects/**)",
    "Read(~/.claude/session-env/**)",
    "Read(~/.claude/sessions/**)",
    "Read(~/.claude/settings.json)",
    "Read(~/.claude/settings.local.json)",
    "Read(~/.claude/settings-bkup.json)",
    "Read(~/.claude/shell-snapshots/**)",
    "Read(~/.claude/tasks/**)",
    "Read(~/.claude/teams/**)",
];

fn settings_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/base/settings")
}

fn normalized(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The preset files actually shipped, derived from the directory listing,
/// every invariant test iterates this, so a newly dropped-in preset is
/// covered the moment it lands, not only once someone remembers a constant.
fn preset_files() -> Vec<String> {
    let dir = settings_dir();
    let entries =
        std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()));
    let mut names: Vec<String> = entries
        .map(|entry| {
            entry
                .expect("dir entry")
                .file_name()
                .into_string()
                .expect("utf-8 file name")
        })
        .filter(|name| {
            std::path::Path::new(name)
                .extension()
                .is_some_and(|e| e == "json")
        })
        .collect();
    assert!(
        !names.is_empty(),
        "no preset files found under {}",
        dir.display()
    );
    names.sort();
    names
}

fn load(name: &str) -> serde_json::Value {
    let path = settings_dir().join(name);
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{name} is not valid JSON: {e}"))
}

/// Collect every `command` string of a `{"type": "command", ...}` hook object
/// anywhere under the given value.
fn collect_hook_commands(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            if map.get("type").and_then(serde_json::Value::as_str) == Some("command") {
                let command = map
                    .get("command")
                    .and_then(serde_json::Value::as_str)
                    .expect("command hook carries a command string");
                out.push(command.to_string());
            }
            for v in map.values() {
                collect_hook_commands(v, out);
            }
        }
        serde_json::Value::Array(items) => {
            for v in items {
                collect_hook_commands(v, out);
            }
        }
        _ => {}
    }
}

#[test]
fn preset_files_match_the_shipped_directory() {
    let shipped = preset_files();
    let shipped: Vec<&str> = shipped.iter().map(String::as_str).collect();
    let mut pinned: Vec<&str> = PRESET_FILES.to_vec();
    pinned.sort_unstable();
    assert_eq!(
        shipped, pinned,
        "assets/base/settings/*.json diverged from PRESET_FILES; a new or \
         removed preset must update the pinned set (and its manifest entry \
         plus the init prompt whitelist) in the same change"
    );
}

#[test]
fn presets_parse_as_json() {
    for name in preset_files() {
        let value = load(&name);
        assert!(value.is_object(), "{name}: top level must be an object");
    }
}

#[test]
fn repository_context_policy_is_exact_and_generic_presets_stay_opt_in() {
    let path = settings_dir().join("../../../.claude/settings.json");
    let bytes =
        std::fs::read(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    let repository: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        repository["env"],
        serde_json::json!({
            "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "1000000",
            "CLAUDE_AUTOCOMPACT_PCT_OVERRIDE": "50"
        })
    );

    for name in preset_files() {
        assert!(
            load(&name).get("env").is_none(),
            "{name}: a consuming project must opt into the context policy"
        );
    }
}

#[test]
fn context_policy_guidance_pins_scope_effect_and_lifecycle_caution() {
    let root = settings_dir().join("../../..");
    let customize = normalized(
        &std::fs::read_to_string(root.join("assets/base/agents/skills/cf-customize/SKILL.md"))
            .unwrap(),
    );
    let policy = normalized(
        &std::fs::read_to_string(
            root.join("assets/base/agents/skills/cf-customize/references/claude-context-policy.md"),
        )
        .unwrap(),
    );
    let adapter = normalized(
        &std::fs::read_to_string(
            root.join("assets/base/claude/skills/cf-delegate/resources/claude-turn-completion.md"),
        )
        .unwrap(),
    );

    assert!(customize.contains("references/claude-context-policy.md"));
    for marker in [
        "This is project customization, not a generic CodeFlow default",
        "effective 1M window",
        "200K-limited session remains 200K",
        "roughly 100K",
        "environment controls outrank corresponding command, flag, or settings choices",
        "percentage override applies to qualifying main and subagent sessions",
        "preserves the whole value and does not deep-merge incoming keys",
        "inspect without printing sensitive values",
        "Present only the proposed non-secret context-key delta and any conflicts",
        "A malformed object is a reported configuration error",
        "configured value, or requested launch is not runtime evidence",
        "manual compaction/resume check",
        "does not prove the automatic threshold",
    ] {
        assert!(policy.contains(marker), "context policy lost: {marker}");
    }
    for marker in [
        "Never copy that `env` into the immutable task settings",
        "requested values are not applied evidence",
        "any compact restart poisons this run",
    ] {
        assert!(adapter.contains(marker), "adapter lost: {marker}");
    }
}

#[test]
fn universal_presets_do_not_force_tracked_claude_task_mode() {
    for name in preset_files() {
        let path = settings_dir().join(&name);
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        assert!(
            !text.contains("CLAUDE_CODE_DISABLE_BACKGROUND_TASKS"),
            "{name}: synchronous task mode belongs to the tracked process launch, not a universal preset"
        );
    }
}

#[test]
fn every_hook_command_is_a_known_codeflow_hook() {
    for name in preset_files() {
        let value = load(&name);
        let hooks = value
            .get("hooks")
            .unwrap_or_else(|| panic!("{name}: hooks key missing"));
        let mut commands = Vec::new();
        collect_hook_commands(hooks, &mut commands);
        assert!(!commands.is_empty(), "{name}: no hook commands found");
        for command in &commands {
            assert!(
                command.starts_with("codeflow hook "),
                "{name}: hook command {command:?} must start with \"codeflow hook \""
            );
            let sub = command.trim_start_matches("codeflow hook ").trim();
            assert!(
                HOOK_NAMES.contains(&sub),
                "{name}: hook command {command:?} names unknown subcommand {sub:?}"
            );
        }
        // All three hooks are wired in every preset.
        for hook in HOOK_NAMES {
            assert!(
                commands
                    .iter()
                    .any(|c| c == &format!("codeflow hook {hook}")),
                "{name}: {hook} hook not wired"
            );
        }
    }
}

#[test]
fn deny_rules_cover_secret_files() {
    for name in preset_files() {
        let value = load(&name);
        let deny = value["permissions"]["deny"]
            .as_array()
            .unwrap_or_else(|| panic!("{name}: permissions.deny missing"));
        let deny: Vec<&str> = deny.iter().filter_map(serde_json::Value::as_str).collect();
        let read_globs: Vec<&str> = deny
            .iter()
            .filter(|d| d.starts_with("Read("))
            .copied()
            .collect();
        assert!(!read_globs.is_empty(), "{name}: no Read deny rules");

        let has = |needle: &str| read_globs.iter().any(|g| g.contains(needle));
        assert!(has(".env"), "{name}: .env files must be deny-read");
        assert!(
            has("*.key") || has("*.pem"),
            "{name}: key material (*.key / *.pem) must be deny-read"
        );
        assert!(
            has("*credentials*"),
            "{name}: *credentials* must be deny-read"
        );
    }
}

/// Collect a `permissions.<key>` array as owned strings.
fn perm_array(value: &serde_json::Value, key: &str) -> Vec<String> {
    value["permissions"][key]
        .as_array()
        .unwrap_or_else(|| panic!("permissions.{key} missing"))
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect()
}

#[test]
fn exec_guard_wired_in_every_preset() {
    // The security stage covers both shell tools Claude exposes across Unix,
    // WSL2, and native Windows.
    for name in preset_files() {
        let value = load(&name);
        let pre = value["hooks"]["PreToolUse"]
            .as_array()
            .expect("PreToolUse hook array");
        assert!(
            pre.iter().any(|group| {
                let matcher = group["matcher"].as_str().unwrap_or_default();
                matcher.contains("Bash") && matcher.contains("PowerShell")
            }),
            "{name}: shell guard matcher must cover Bash and PowerShell"
        );
        let mut commands = Vec::new();
        collect_hook_commands(&value["hooks"], &mut commands);
        assert!(
            commands.iter().any(|c| c == "codeflow hook exec-guard"),
            "{name}: exec-guard hook not wired"
        );
    }
}

#[test]
fn allow_arrays_grant_project_autonomy() {
    // The autonomy posture (ADR-0008): the common project toolchain runs
    // promptless. Spot-check a representative entry from each family.
    let expected = [
        "Bash(cargo *)",
        "Bash(codeflow *)",
        "Bash(git commit *)",
        "Bash(git push *)",
        "Bash(gh pr create *)",
        "Bash(npm test *)",
        "Bash(python3 *)",
        "Bash(uv *)",
        "WebSearch",
        "WebFetch",
        "WebFetch(domain:docs.rs)",
    ];
    for name in preset_files() {
        let allow = perm_array(&load(&name), "allow");
        for entry in expected {
            assert!(
                allow.iter().any(|a| a == entry),
                "{name}: allow missing {entry:?}"
            );
        }
    }
}

#[test]
fn ask_arrays_gate_escalation_and_publish() {
    // Hard protections stay: privilege escalation and irreversible publish/
    // delete are prompted even under the most permissive preset (an `ask` rule
    // fires in bypassPermissions mode too).
    let expected = [
        "Bash(sudo *)",
        "Bash(su *)",
        "Bash(doas *)",
        "Bash(rm -rf /)",
        "Bash(rm -fr /)",
        "Bash(git reset --hard)",
        "Bash(git reset --hard *)",
        "Bash(git clean *)",
        "Bash(git checkout -- .)",
        "Bash(git checkout -- *)",
        "Bash(git checkout .)",
        "Bash(git checkout * -- *)",
        "Bash(git checkout -f)",
        "Bash(git checkout -f *)",
        "Bash(git checkout --force)",
        "Bash(git checkout --force *)",
        "Bash(git checkout -B *)",
        "Bash(git switch -C *)",
        "Bash(git switch --force-create *)",
        "Bash(git switch -f)",
        "Bash(git switch -f *)",
        "Bash(git switch --force)",
        "Bash(git switch --force *)",
        "Bash(git switch --discard-changes)",
        "Bash(git switch --discard-changes *)",
        "Bash(git restore .)",
        "Bash(git restore *)",
        "Bash(git stash drop)",
        "Bash(git stash drop *)",
        "Bash(git stash clear)",
        "Bash(git push --force)",
        "Bash(git push --force *)",
        "Bash(git push -f)",
        "Bash(git push -f *)",
        "Bash(git push * --force)",
        "Bash(git push * --force *)",
        "Bash(git push * -f)",
        "Bash(git push * -f *)",
        "Bash(git push --delete *)",
        "Bash(git push * --delete *)",
        "Bash(git push -d *)",
        "Bash(git push * -d *)",
        "Bash(git push +*)",
        "Bash(git push * +*)",
        "Bash(git push --mirror *)",
        "Bash(git push * --mirror *)",
        "Bash(git push --prune *)",
        "Bash(git push * --prune *)",
        "Bash(git branch -D *)",
        "Bash(git branch --delete --force *)",
        "Bash(git branch --force --delete *)",
        "Bash(git branch -d -f *)",
        "Bash(git branch -f -d *)",
        "Bash(git branch -f *)",
        "Bash(git branch --force *)",
        "Bash(git branch -M *)",
        "Bash(git branch -m -f *)",
        "Bash(git branch -m --force *)",
        "Bash(git branch --move -f *)",
        "Bash(git branch --move --force *)",
        "Bash(cargo publish *)",
        "Bash(npm publish *)",
        "Bash(gh release *)",
        "Bash(gh repo delete *)",
    ];
    for name in preset_files() {
        let ask = perm_array(&load(&name), "ask");
        for entry in expected {
            assert!(
                ask.iter().any(|a| a == entry),
                "{name}: ask missing {entry:?}"
            );
        }
        assert!(
            !perm_array(&load(&name), "allow")
                .iter()
                .any(|entry| entry.starts_with("Bash(git restore")),
            "{name}: git restore must not remain in allow; ask rules take precedence"
        );
        assert!(
            !ask.iter().any(|entry| entry == "Bash(git push * :*)"),
            "{name}: a rule ending in :* is parsed as a legacy literal-prefix rule"
        );
        // rm -rf on / and ~ is asked in some form.
        assert!(
            ask.iter()
                .any(|a| a.starts_with("Bash(rm -") && a.contains('/')),
            "{name}: ask missing an rm -rf / rule"
        );
        assert!(
            ask.iter()
                .any(|a| a.starts_with("Bash(rm -") && a.contains('~')),
            "{name}: ask missing an rm -rf ~ rule"
        );
    }
}

#[test]
fn sandbox_removes_raw_model_and_cloud_credentials_from_bash() {
    let expected = [
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "OPENAI_API_KEY",
        "AWS_SECRET_ACCESS_KEY",
        "AWS_SESSION_TOKEN",
    ];

    for name in preset_files() {
        let value = load(&name);
        let env_vars = value["sandbox"]["credentials"]["envVars"]
            .as_array()
            .unwrap_or_else(|| panic!("{name}: sandbox.credentials.envVars missing"));
        for variable in expected {
            assert!(
                env_vars.iter().any(|entry| {
                    entry["name"].as_str() == Some(variable)
                        && entry["mode"].as_str() == Some("deny")
                }),
                "{name}: {variable} must be denied to sandboxed Bash"
            );
        }
    }
}

#[test]
fn sandbox_denies_secret_stores_to_every_subprocess() {
    // Read-tool rules do not cover arbitrary Python/Node/shell subprocess
    // reads. Pin the OS-sandbox layer as well so an allowed development tool
    // cannot become a credential-reading escape hatch.
    let expected = [
        "~/.ssh",
        "~/.aws",
        "~/.gnupg",
        "~/.netrc",
        "~/.kube",
        "~/.cargo/credentials",
        "~/.cargo/credentials.toml",
        "~/.claude",
    ];

    for name in preset_files() {
        let value = load(&name);
        let deny_read = value["sandbox"]["filesystem"]["denyRead"]
            .as_array()
            .unwrap_or_else(|| panic!("{name}: sandbox.filesystem.denyRead missing"));
        for path in expected {
            assert!(
                deny_read.iter().any(|entry| entry == path),
                "{name}: sandbox denyRead missing {path:?}"
            );
        }
    }
}

#[test]
fn sandbox_reallows_only_immutable_plugin_code_under_claude_state() {
    for name in preset_files() {
        let value = load(&name);
        let allow_read = value["sandbox"]["filesystem"]["allowRead"]
            .as_array()
            .unwrap_or_else(|| panic!("{name}: sandbox.filesystem.allowRead missing"));
        let allow_read: Vec<&str> = allow_read
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect();
        assert_eq!(
            allow_read,
            ["~/.claude/plugins/cache"],
            "{name}: sandbox may reallow installed plugin code, not mutable plugin state"
        );
    }
}

#[test]
fn deny_extends_to_pure_secret_home_stores() {
    // Read protection reaches beyond the project cwd to the home-dir secret
    // stores an agent must never read (ADR-0008), while keeping the cwd globs.
    let home_stores = [
        "Read(~/.ssh/**)",
        "Read(~/.aws/**)",
        "Read(~/.gnupg/**)",
        "Read(~/.kube/**)",
    ];
    for name in preset_files() {
        let deny = perm_array(&load(&name), "deny");
        for entry in home_stores {
            assert!(
                deny.iter().any(|d| d == entry),
                "{name}: deny missing {entry:?}"
            );
        }
        assert!(
            deny.iter().any(|d| d.contains(".cargo/credentials")),
            "{name}: deny missing ~/.cargo/credentials"
        );
        // The original cwd globs survive alongside the new home-dir rules.
        assert!(
            deny.iter().any(|d| d == "Read(**/.env)"),
            "{name}: lost cwd .env deny"
        );
    }
}

#[test]
fn claude_state_denies_leave_the_official_plugin_runtime_available() {
    let expected: BTreeSet<&str> = CLAUDE_SENSITIVE_READ_DENIES.into_iter().collect();

    for name in preset_files() {
        let deny = perm_array(&load(&name), "deny");
        let actual: BTreeSet<&str> = deny
            .iter()
            .map(String::as_str)
            .filter(|entry| entry.starts_with("Read(~/.claude/"))
            .collect();

        assert_eq!(
            actual, expected,
            "{name}: Claude state denies must stay narrow and complete"
        );
        assert!(
            !deny.iter().any(|entry| entry == "Read(~/.claude/**)"),
            "{name}: a broad ~/.claude deny hides the official Codex plugin runtime"
        );
        assert!(
            !deny.iter().any(|entry| entry.contains("~/.claude/plugins")),
            "{name}: ~/.claude/plugins must remain readable for official plugins"
        );
    }
}

#[test]
fn authenticated_tool_configuration_remains_available() {
    // GitHub CLI and Docker may resolve real credentials through the OS
    // keychain or another broker, but they still need their non-secret config
    // to locate that path. Blocking the whole config file disables the tool.
    for name in preset_files() {
        let deny = perm_array(&load(&name), "deny");
        for entry in ["Read(~/.config/gh/**)", "Read(~/.docker/config.json)"] {
            assert!(
                !deny.iter().any(|d| d == entry),
                "{name}: {entry} prevents an approved authenticated tool from using brokered configuration"
            );
        }
    }
}

#[test]
fn every_preset_has_a_fail_closed_autonomous_sandbox() {
    // Broad public access is available to both native web tools and sandboxed
    // development commands. Auto mode still classifies every shell command,
    // while the sandbox blocks common private/link-local destinations.
    for name in preset_files() {
        let value = load(&name);
        let sandbox = &value["sandbox"];
        assert_eq!(sandbox["enabled"], true, "{name}: sandbox disabled");
        assert_eq!(
            sandbox["failIfUnavailable"], true,
            "{name}: sandbox must fail closed"
        );
        assert_eq!(
            sandbox["autoAllowBashIfSandboxed"], true,
            "{name}: sandboxed Bash should run autonomously"
        );
        assert_eq!(
            sandbox["allowUnsandboxedCommands"], true,
            "{name}: trusted tools that cannot run in the OS sandbox need the classified retry path"
        );
        assert!(
            sandbox["excludedCommands"]
                .as_array()
                .is_none_or(std::vec::Vec::is_empty),
            "{name}: do not grant a static broad sandbox exclusion; use the classified retry path"
        );
        assert_eq!(
            sandbox["network"]["allowLocalBinding"], true,
            "{name}: local dev servers must be available to UI tests"
        );
        let allowed_domains = sandbox["network"]["allowedDomains"]
            .as_array()
            .expect("allowedDomains array");
        assert!(
            allowed_domains.iter().any(|domain| domain == "*"),
            "{name}: sandboxed development tools need broad public egress"
        );
        let denied_domains = sandbox["network"]["deniedDomains"]
            .as_array()
            .expect("deniedDomains array");
        for destination in ["10.*", "169.254.*", "192.168.*", "*.internal"] {
            assert!(
                denied_domains.iter().any(|domain| domain == destination),
                "{name}: private/link-local destination {destination} is not guarded"
            );
        }
        assert!(
            value.get("autoMode").is_none(),
            "{name}: Claude ignores autoMode in shared project settings; pass it at user/CLI scope"
        );
    }
}

#[test]
fn bypass_sandbox_keeps_os_level_secret_denies() {
    // bypassPermissions skips the ordinary permission layer. This preset is
    // isolated-host-only, and its fail-closed sandbox keeps the pure-secret
    // filesystem boundary active.
    let value = load("bypass-sandboxed.json");
    let sandbox = &value["sandbox"];
    let deny_read = sandbox["filesystem"]["denyRead"]
        .as_array()
        .expect("sandbox.filesystem.denyRead array");
    let deny_read: Vec<&str> = deny_read
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect();
    assert!(deny_read.contains(&"~/.ssh"), "denyRead missing ~/.ssh");
    assert!(deny_read.contains(&"~/.aws"), "denyRead missing ~/.aws");
}

#[test]
fn top_level_keys_stay_within_the_pinned_union() {
    let allowed: BTreeSet<&str> = TOP_LEVEL_KEYS.into_iter().collect();
    let mut union: BTreeSet<String> = BTreeSet::new();
    for name in preset_files() {
        let value = load(&name);
        let keys = value.as_object().unwrap().keys();
        for key in keys {
            assert!(
                allowed.contains(key.as_str()),
                "{name}: top-level key {key:?} is not in the pinned allowlist {allowed:?}; \
                 typo, or a deliberate addition that must update TOP_LEVEL_KEYS"
            );
            union.insert(key.clone());
        }
    }
    // The pinned list is exactly the union in use: a key dropped from every
    // preset must be removed here too, keeping the allowlist honest.
    let union: BTreeSet<&str> = union.iter().map(String::as_str).collect();
    assert_eq!(union, allowed, "pinned union out of date with the presets");
}

#[test]
fn minimal_claude_guidance_matches_the_classified_retry_setting() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/base/CLAUDE.minimal.md.tmpl");
    let guidance = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    assert!(guidance.contains("auto-classified unsandboxed retry"));
    assert!(guidance.contains("trusted installed tool"));
    assert!(guidance.contains("This is not a general bypass"));
    assert!(!guidance.contains("unsandboxed retry is disabled"));
}

/// A project settings file ignores `auto` and `bypassPermissions`, while its
/// other values apply from project scope and outrank the user file (see
/// "which mode a session starts in" in the permission-modes reference). A
/// `defaultMode` in the default preset could therefore only pull an
/// operator's chosen mode back to manual prompting, so the default preset
/// leaves the mode alone; `acceptEdits.json` still names its mode because
/// `acceptEdits` remains valid from project scope.
#[test]
fn default_preset_leaves_the_permission_mode_to_the_operator() {
    let value = load("default.json");
    assert!(
        value["permissions"].get("defaultMode").is_none(),
        "default.json must not set permissions.defaultMode"
    );
    assert_eq!(
        load("acceptEdits.json")["permissions"]["defaultMode"],
        "acceptEdits"
    );
}

/// This preset drops a confirmation layer for in-tree recursive deletes and
/// for safe branch deletes; the sandbox limits the blast radius but does not
/// make untracked unique data recoverable. `git branch -d` only refuses a
/// branch unmerged into its upstream (or HEAD when it has none), so landing
/// evidence, not this rule, is what gates a branch delete.
#[test]
fn ask_arrays_prompt_only_for_unrecoverable_deletes() {
    for name in preset_files() {
        let ask = perm_array(&load(&name), "ask");
        for absent in [
            "Bash(rm -rf *)",
            "Bash(rm -fr *)",
            "Bash(git branch -d *)",
            "Bash(git branch --delete *)",
        ] {
            assert!(
                !ask.iter().any(|a| a == absent),
                "{name}: {absent} prompts on ordinary work"
            );
        }
        for present in [
            "Bash(rm -rf /)",
            "Bash(rm -rf /*)",
            "Bash(rm -rf ~*)",
            "Bash(git branch -D *)",
            "Bash(git branch --delete --force *)",
        ] {
            assert!(
                ask.iter().any(|a| a == present),
                "{name}: {present} must stay behind a prompt"
            );
        }
        // The ask rules match the command text as written, so they require
        // the delete flag (`-d`, `--delete`, `-D`, or a cluster starting with
        // `-d`, `-D`, `-f` or `-q` as listed below) to be the first option
        // after `git branch`. These are the forms that reach a prompt.
        for forced in [
            "git branch -D topic",
            "git branch -d topic --force",
            "git branch --delete topic --force",
            "git branch -d topic -f",
            "git branch -df topic",
            "git branch -Df topic",
            "git branch --delete --force topic",
            "git branch -d --force topic",
            "git branch --delete -f topic",
            "git branch -d -f topic",
            "git branch -f -d topic",
            "git branch -d topic -qf",
            "git branch --delete topic -qf",
            "git branch -d -fq topic",
        ] {
            assert!(
                ask_covers(&ask, forced),
                "{name}: no ask rule covers {forced:?}"
            );
        }
        // Ordinary work stays unprompted, and a branch name that merely
        // looks like an option must not be read as one. The three force forms
        // below record the boundary rather than claim it away: prefix globs
        // cannot enumerate every aggregated cluster, and they cannot match an
        // option placed before the delete flag. `git-guard` is what still
        // blocks a protected branch, whether the delete flag stands alone or
        // sits inside a cluster.
        for ordinary in [
            "git branch -d topic",
            "git branch --delete topic",
            "git branch -d topic-force",
            "git branch -d topic -vqf",
            "git branch -q -d topic --force",
            "git branch -qf -d topic",
            "rm -rf target",
            "rm -rf ./build",
        ] {
            assert!(
                !ask_covers(&ask, ordinary),
                "{name}: an ask rule prompts on {ordinary:?}"
            );
        }
    }
}

/// Model how Claude Code matches a Bash permission rule against a command: a
/// rule `Bash(p)` matches when `p`, with `*` standing for any run of
/// characters, matches the whole command; a `p` without `*` is exact; and a
/// trailing ` *` that is the only wildcard also matches the bare command, so
/// `Bash(ls *)` covers `ls`.
///
/// This models a single normalized command only. It is not the Bash
/// permission evaluator: it does not split compound commands, and it knows
/// nothing of deny precedence or of ask precedence over allow rules.
fn ask_covers(ask: &[String], command: &str) -> bool {
    ask.iter().any(|rule| {
        rule.strip_prefix("Bash(")
            .and_then(|rest| rest.strip_suffix(')'))
            .is_some_and(|pattern| rule_matches(pattern, command))
    })
}

fn rule_matches(pattern: &str, command: &str) -> bool {
    if let Some(head) = pattern.strip_suffix(" *") {
        if !head.contains('*') && head == command {
            return true;
        }
    }
    glob_matches(pattern, command)
}

/// Whole-string glob match where `*` spans any run of characters, including
/// none.
fn glob_matches(pattern: &str, text: &str) -> bool {
    let Some((head, rest)) = pattern.split_once('*') else {
        return pattern == text;
    };
    let Some(mut tail) = text.strip_prefix(head) else {
        return false;
    };
    loop {
        if glob_matches(rest, tail) {
            return true;
        }
        let Some(next) = tail.chars().next() else {
            return false;
        };
        tail = &tail[next.len_utf8()..];
    }
}

/// Conformance fixtures for the rule matcher itself, so the delete fixtures
/// above rest on a model that matches the documented rule syntax.
#[test]
fn rule_matcher_follows_the_documented_bash_rule_syntax() {
    assert!(
        rule_matches("ls *", "ls"),
        "a lone trailing wildcard is optional"
    );
    assert!(rule_matches("ls *", "ls -la"));
    assert!(!rule_matches(
        "git branch -d * --force",
        "git branch -d topic"
    ));
    assert!(!rule_matches(
        "git branch -d * --force",
        "git branch -d topic-force"
    ));
    assert!(rule_matches("git branch -D *", "git branch -D topic"));
    assert!(!rule_matches("rm -rf /*", "rm -rf ./build"));
}
