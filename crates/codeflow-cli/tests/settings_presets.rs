//! Regression tests for the shipped Claude settings presets
//! (`assets/base/settings/*.json`): the scaffold content is the product as
//! much as the code (charter §4.4), so its invariants are tested like code:
//! every hook command must be a known `codeflow hook` subcommand, the
//! secret-file deny rules must stay present, and top-level keys are pinned
//! against typos.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use codeflow_core::security::actions;

/// The expected preset set, pinned so an addition or removal is a conscious
/// choice; `preset_files_match_the_shipped_directory` keeps it honest.
const PRESET_FILES: [&str; 3] = ["default.json", "acceptEdits.json", "bypass-sandboxed.json"];

/// The known hook subcommands wired by the presets (charter §3.3; the
/// `exec-guard` security stage added in ADR-0008). `session-orient` also
/// carries the TSK-128 prompt reminder, dispatched on the event.
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
    "Read(~/.claude/projects/**/*.jsonl)",
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
            let sub = command
                .trim_start_matches("codeflow hook ")
                .split_whitespace()
                .next()
                .unwrap();
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
                    .any(|c| c.starts_with(&format!("codeflow hook {hook} --contract 3"))),
                "{name}: {hook} hook not wired"
            );
        }
    }
}

/// TSK-128 AC-2 (as amended): each host names the `SessionStart` sources it
/// supports. Claude Code adds `fork`, which carries an older conversation
/// and gets the resume guidance; Codex keeps its documented four. Both wire
/// `UserPromptSubmit` to the same stable advisory entry, `session-orient`,
/// which an older binary also accepts, and never to `prompt-reminder`.
#[test]
fn session_start_sources_are_explicit_per_host() {
    let codex_path = settings_dir().join("../codex/hooks.json");
    let codex: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&codex_path).unwrap()).unwrap();
    assert_eq!(
        codex["hooks"]["SessionStart"][0]["matcher"].as_str(),
        Some("startup|resume|clear|compact")
    );
    for name in preset_files() {
        let value = load(&name);
        let groups = value["hooks"]["SessionStart"]
            .as_array()
            .unwrap_or_else(|| panic!("{name}: SessionStart missing"));
        assert_eq!(groups.len(), 1, "{name}: one SessionStart group");
        assert_eq!(
            groups[0]["matcher"].as_str(),
            Some("startup|resume|clear|compact|fork"),
            "{name}: SessionStart must name every Claude source, fork included"
        );
        let mut prompt = Vec::new();
        collect_hook_commands(&value["hooks"]["UserPromptSubmit"], &mut prompt);
        assert_eq!(prompt.len(), 1);
        assert!(prompt[0].starts_with("codeflow hook session-orient --contract 3"));
        assert!(
            value["hooks"]["UserPromptSubmit"][0]
                .get("matcher")
                .is_none(),
            "{name}: UserPromptSubmit takes no matcher"
        );
        let mut all = Vec::new();
        collect_hook_commands(&value["hooks"], &mut all);
        assert!(
            !all.iter().any(|c| c.contains("prompt-reminder")),
            "{name}: prompt-reminder is a manual command, never wired"
        );
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
            commands
                .iter()
                .any(|c| c.starts_with("codeflow hook exec-guard --contract 3")),
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

/// The only extra sandbox write root is the per-user state directory
/// `codeflow present` needs (macOS, then Linux without `XDG_STATE_HOME`), so
/// an agent can run it without a sandbox bypass. `present_cli` proves the
/// the runtime keeps its state under the matching root.
const PRESENT_STATE_WRITE_ROOTS: [&str; 2] = [
    "~/Library/Application Support/codeflow/present",
    "~/.local/state/codeflow/present",
];

#[test]
fn sandbox_allows_writes_only_to_the_present_state_directory() {
    for name in preset_files() {
        let value = load(&name);
        let allow_write: Vec<&str> = value["sandbox"]["filesystem"]["allowWrite"]
            .as_array()
            .unwrap_or_else(|| panic!("{name}: sandbox.filesystem.allowWrite missing"))
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect();
        assert_eq!(
            allow_write, PRESENT_STATE_WRITE_ROOTS,
            "{name}: the sandbox may add only the cf-present state directory as a write root"
        );
        assert!(
            value["sandbox"]["filesystem"]["denyWrite"].is_null(),
            "{name}: no preset denyWrite is expected"
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

/// Privilege-escalation command patterns (TSK-041 defect 2). Claude exposes
/// two shell tools — `Bash` and `PowerShell` — and a permission rule is keyed
/// by the tool that carries the command, so `Bash(...)` alone leaves the
/// Windows/graphical launchers ungated on the `PowerShell` tool. The shell guard
/// matcher (`^(Bash|PowerShell)$`) already covers both; the permission layer
/// now does too.
const PRIVILEGE_ESCALATION_PATTERNS: [&str; 8] = [
    "sudo *",
    "su *",
    "doas *",
    "pkexec *",
    "gsudo *",
    "runas *",
    "Start-Process -Verb RunAs*",
    "Start-Process * -Verb RunAs*",
];

/// DEFECT 2 (negative): never allow. This is the assertion that fails if any
/// escalation pattern is promoted into `allow`, under any tool prefix and in
/// any preset.
#[test]
fn privilege_escalation_is_never_promoted_to_allow() {
    for name in preset_files() {
        let value = load(&name);
        let allow = perm_array(&value, "allow");
        for pattern in PRIVILEGE_ESCALATION_PATTERNS {
            for entry in &allow {
                let inner = entry
                    .split_once('(')
                    .and_then(|(_, rest)| rest.strip_suffix(')'))
                    .unwrap_or(entry.as_str());
                assert_ne!(
                    inner, pattern,
                    "{name}: privilege escalation {pattern:?} was promoted to \
                     allow; this family is denied, never allowed"
                );
            }
        }
    }
}

/// Whether any `tool(...)` rule covers `command`, using the documented
/// matcher below; `PowerShell` rules match case-insensitively. It models
/// one normalized command, not the harness.
fn denied(rules: &[String], tool: &str, command: &str) -> bool {
    let prefix = format!("{tool}(");
    let fold = |text: &str| {
        if tool == "PowerShell" {
            text.to_lowercase()
        } else {
            text.to_string()
        }
    };
    rules.iter().any(|rule| {
        rule.strip_prefix(&prefix)
            .and_then(|inner| inner.strip_suffix(')'))
            .is_some_and(|inner| rule_matches(&fold(inner), &fold(command)))
    })
}

/// Round 1 review (F1): path-qualified launchers, Windows `.exe` spellings
/// and `PowerShell` elevation started from the Bash tool are denied too,
/// while ordinary commands that only mention a launcher are not.
#[test]
fn privilege_escalation_variants_are_denied() {
    let positives = [
        ("Bash", "sudo id"),
        ("Bash", "/usr/bin/sudo id"),
        ("Bash", "/usr/bin/pkexec id"),
        ("Bash", "/bin/su root"),
        ("Bash", "/bin/su"),
        ("Bash", "/usr/bin/sudo"),
        ("Bash", "pwsh -c 'Start-Process pwsh -VERB RUNAS'"),
        ("Bash", "gsudo whoami"),
        ("Bash", "runas.exe /user:Administrator cmd"),
        (
            "Bash",
            "/c/Windows/System32/runas.exe /user:Administrator cmd",
        ),
        ("Bash", "gsudo.exe whoami"),
        (
            "Bash",
            "powershell.exe -Command \"Start-Process cmd -Verb RunAs\"",
        ),
        ("Bash", "pwsh -c 'Start-Process pwsh -verb runas'"),
        ("PowerShell", "Start-Process cmd -Verb RunAs"),
        ("PowerShell", "saps cmd -verb runas"),
        (
            "PowerShell",
            "C:\\Windows\\System32\\runas.exe /user:Administrator cmd",
        ),
        ("PowerShell", "C:\\tools\\gsudo.exe whoami"),
    ];
    let negatives = [
        ("Bash", "git status"),
        ("Bash", "echo pkexec"),
        ("Bash", "cat /etc/sudoers"),
        ("Bash", "grep -rn runas docs"),
        ("Bash", "ls tools/gsudo"),
        ("Bash", "ls /usr/bin/sudoedit"),
        ("PowerShell", "Get-ChildItem"),
        ("PowerShell", "Write-Output runas"),
    ];
    for name in preset_files() {
        let deny = perm_array(&load(&name), "deny");
        for (tool, command) in positives {
            assert!(
                denied(&deny, tool, command),
                "{name}: {tool} command {command:?} escalates but reaches no deny rule"
            );
        }
        for (tool, command) in negatives {
            assert!(
                !denied(&deny, tool, command),
                "{name}: {tool} command {command:?} does not escalate but is denied"
            );
        }
    }
}

/// DEFECT 2 companion: the generated deny entries must not have relaxed
/// anything else. The non-relaxable secret-read prohibitions and the
/// fail-closed sandbox defaults are re-asserted here, so a widening edit
/// cannot land alongside them unnoticed.
#[test]
fn escalation_additions_leave_prohibitions_and_sandbox_unchanged() {
    for name in preset_files() {
        let value = load(&name);
        let deny = perm_array(&value, "deny");
        let allow = perm_array(&value, "allow");

        // The secret-file read denies survive, and nothing re-grants them.
        for rule in CLAUDE_SENSITIVE_READ_DENIES {
            assert!(
                deny.iter().any(|entry| entry == rule),
                "{name}: non-relaxable read deny {rule:?} disappeared"
            );
        }
        for rule in &deny {
            assert!(
                !allow.iter().any(|entry| entry == rule),
                "{name}: {rule:?} is both denied and allowed"
            );
        }

        // The sandbox stays enabled and fail-closed, with no broad static
        // exclusion smuggled in beside the generated entries.
        let sandbox = &value["sandbox"];
        assert_eq!(sandbox["enabled"], true, "{name}: sandbox disabled");
        assert_eq!(
            sandbox["failIfUnavailable"], true,
            "{name}: sandbox must stay fail-closed"
        );
        assert!(
            sandbox["excludedCommands"]
                .as_array()
                .is_none_or(std::vec::Vec::is_empty),
            "{name}: a broad static sandbox exclusion appeared"
        );
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

// ---- Generated from the action table (ADR-0075, TSK-171) ----------------

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A reference sample of the settled design, which the generator must
/// reproduce.
fn evidence(rel: &str) -> PathBuf {
    repo_root()
        .join("docs/verification/evidence/permission-presets")
        .join(rel)
}

fn read_json(path: &Path) -> serde_json::Value {
    let text =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn bless() -> bool {
    std::env::var_os("CODEFLOW_BLESS").is_some()
}

const REGENERATE: &str = "regenerate with CODEFLOW_BLESS=1 cargo test -p codeflow-cli --test settings_presets, then review the diff";

/// Compare a generated asset with the shipped file, or rewrite it under
/// `CODEFLOW_BLESS`.
fn check_generated(path: &Path, generated: &str) {
    let shipped = std::fs::read_to_string(path).unwrap_or_default();
    if shipped == generated {
        return;
    }
    if bless() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, generated).unwrap();
        return;
    }
    panic!(
        "{} is not what the action table generates; {REGENERATE}",
        path.display()
    );
}

/// AC-5: the Claude arrays of every shipped preset are the table's output.
#[test]
fn claude_presets_are_generated_from_the_action_table() {
    let table = actions::table();
    assert_eq!(table.parity_errors(), Vec::<String>::new());
    for name in preset_files() {
        let path = settings_dir().join(&name);
        let mut value = load(&name);
        table.apply_to_claude_preset(&mut value);
        let mut generated = serde_json::to_string_pretty(&value).unwrap();
        generated.push('\n');
        check_generated(&path, &generated);
    }
}

/// AC-5: the Codex rules file is the table's output.
#[test]
fn codex_rules_are_generated_from_the_action_table() {
    check_generated(
        &repo_root().join("assets/base/codex/rules/codeflow.rules"),
        &actions::table().codex_rules(),
    );
}

/// The generator reproduces the design's reference samples from the presets
/// the design started from, with one stated departure: the read rules are
/// ordered in groups (review of PR 749), so the deny array holds the same
/// rules as the reference in a different order. Hook commands are excluded:
/// their fail-closed form and `--contract` flag are TSK-173's.
#[test]
fn the_generator_reproduces_the_reference_samples() {
    let table = actions::table();
    let reference: serde_json::Value = read_json(&evidence("actions.json"));
    let sorted = |value: &serde_json::Value| {
        let mut items: Vec<String> = value
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        items.sort();
        items
    };
    let mut denies = table.claude_read_denies();
    denies.sort();
    assert_eq!(denies, sorted(&reference["claude_read_denies"]));
    let mut carveouts = table.claude_read_carveouts();
    carveouts.sort();
    assert_eq!(carveouts, sorted(&reference["claude_read_carveouts"]));
    for key in [
        "families",
        "claude_edit_denies",
        "sandbox_env_denies",
        "sandbox_read_denies",
        "delegate_denies",
    ] {
        let embedded: serde_json::Value = serde_json::from_str(include_str!(
            "../../codeflow-core/src/security/actions.json"
        ))
        .unwrap();
        assert_eq!(
            embedded[key], reference[key],
            "{key} left the reference table"
        );
    }
    for name in PRESET_FILES {
        let mut generated = read_json(&evidence(&format!("current/{name}")));
        table.apply_to_claude_preset(&mut generated);
        let mut expected = read_json(&evidence(&format!("claude/{name}")));
        for value in [&mut generated, &mut expected] {
            let object = value.as_object_mut().unwrap();
            object.remove("hooks");
            let deny = object["permissions"]["deny"].clone();
            object["permissions"]["deny"] = serde_json::json!(sorted(&deny));
        }
        assert_eq!(generated, expected, "{name}");
    }
    assert_eq!(
        table.codex_rules(),
        std::fs::read_to_string(evidence("codex/rules/codeflow.rules")).unwrap()
    );
    assert_eq!(
        table.delegate_fragment(),
        read_json(&evidence("claude/delegate-settings-fragment.json"))
    );
}

/// AC-1: the commands ordinary work runs, which no deny rule may refuse.
const ORDINARY_WORK: [&str; 11] = [
    "rm -rf /tmp/x",
    "rm -rf ~/scratch/x",
    "gh release view v1",
    "gh secret list",
    "crontab -l",
    "git branch -d topic",
    "cargo test",
    "npm run build",
    "gh pr create --draft",
    "cargo +nightly test --features publish",
    "npm --prefix x run publish-docs",
];

/// AC-1: no preset asks, and no deny rule matches ordinary work.
#[test]
fn presets_carry_no_ask_rules_and_deny_no_ordinary_work() {
    for name in preset_files() {
        let value = load(&name);
        assert!(
            value["permissions"].get("ask").is_none(),
            "{name}: an ask rule prompts in every mode (ADR-0075)"
        );
        let deny = perm_array(&value, "deny");
        for command in ORDINARY_WORK {
            assert!(
                !denied(&deny, "Bash", command),
                "{name}: a deny rule refuses ordinary work {command:?}"
            );
        }
    }
}

/// AC-2: a representative command per family, each denied.
const FAMILY_EXAMPLES: [(&str, &[&str]); 6] = [
    (
        "privilege",
        &[
            "sudo id",
            "env sudo id",
            "/usr/bin/sudo id",
            "doas reboot",
            "osascript -e 'do shell script \"id\" with administrator privileges'",
        ],
    ),
    (
        "publish",
        &[
            "cargo publish",
            "npm publish --access public",
            "yarn npm publish",
            "python3 -m twine upload dist/x.whl",
            "gem push x.gem",
            "gh gist create notes.md",
        ],
    ),
    (
        "release",
        &[
            "gh release create v1",
            "gh release upload v1 x.tar.gz",
            "git push --tags",
            "git push origin --follow-tags",
            "git push origin refs/tags/v1",
        ],
    ),
    (
        "account",
        &[
            "gh auth switch",
            "gh auth login",
            "gh auth setup-git",
            "gh auth token",
            "gh secret set X",
            "gh repo delete o/r",
            "gh repo edit --visibility public",
            "git push --mirror",
            "git credential fill",
            "gh api -X DELETE repos/o/r",
        ],
    ),
    (
        "keychain",
        &[
            "security find-generic-password -s x",
            "security dump-keychain",
        ],
    ),
    (
        "persistence",
        &[
            "defaults write com.x key 1",
            "launchctl load x.plist",
            "crontab -e",
            "crontab -r",
            "systemctl --user enable x",
            "systemctl enable x",
        ],
    ),
];

/// AC-2: every preset denies every family of the table.
#[test]
fn every_preset_denies_every_action_family() {
    let table = actions::table();
    let ids: Vec<&str> = table.families.iter().map(|f| f.id.as_str()).collect();
    let covered: Vec<&str> = FAMILY_EXAMPLES.iter().map(|(id, _)| *id).collect();
    assert_eq!(ids, covered, "a family without examples");
    for name in preset_files() {
        let deny = perm_array(&load(&name), "deny");
        for family in &table.families {
            for rule in &family.claude {
                assert!(deny.contains(rule), "{name}: {} lost {rule}", family.id);
            }
        }
        for (id, commands) in FAMILY_EXAMPLES {
            for command in commands {
                assert!(
                    denied(&deny, "Bash", command),
                    "{name}: {id} command {command:?} is not denied"
                );
            }
        }
    }
}

/// The enforcement paths (design 2.2), each needing an `Edit(...)` deny.
const ENFORCEMENT_PATHS: [&str; 10] = [
    ".claude/settings.json",
    ".codeflow/policy.json",
    ".codeflow/project.toml",
    ".codeflow/git-hooks/pre-commit",
    ".codex/config.toml",
    ".codex/hooks.json",
    ".codex/rules/codeflow.rules",
    ".grok/hooks/codeflow.json",
    ".grok/sandbox.toml",
    ".github/workflows/codeflow-ci.yml",
];

/// Gitignore-style path glob: `**/` spans zero or more directories, a
/// trailing `**` anything, and `*` any run inside one path segment.
fn path_matches(pattern: &str, path: &str) -> bool {
    if let Some(rest) = pattern.strip_prefix("**/") {
        return path_matches(rest, path)
            || path
                .char_indices()
                .filter(|(_, c)| *c == '/')
                .any(|(i, _)| path_matches(rest, &path[i + 1..]));
    }
    if pattern == "**" {
        return true;
    }
    if let Some(rest) = pattern.strip_prefix('*') {
        let mut tail = path;
        loop {
            if path_matches(rest, tail) {
                return true;
            }
            match tail.chars().next() {
                Some(c) if c != '/' => tail = &tail[c.len_utf8()..],
                _ => return false,
            }
        }
    }
    match (pattern.chars().next(), path.chars().next()) {
        (None, None) => true,
        (Some(p), Some(c)) if p == c => {
            path_matches(&pattern[p.len_utf8()..], &path[c.len_utf8()..])
        }
        _ => false,
    }
}

/// Whether a file-tool rule pattern covers `path`: `~/...` names the home
/// directory, `/...` the project root, and anything else matches relative to
/// the project at any depth only when it starts with `**/`.
fn rule_covers_path(pattern: &str, path: &str) -> bool {
    match pattern.strip_prefix('/') {
        Some(anchored) => path_matches(anchored, path),
        None => path_matches(pattern, path),
    }
}

/// AC-2: an `Edit` deny covers each enforcement path, the `!` carve-outs sit
/// after the read denies they narrow, and no preset carries a `Write` rule.
#[test]
fn edit_denies_cover_the_enforcement_paths_and_carveouts_follow_read_denies() {
    for name in preset_files() {
        let value = load(&name);
        let deny = perm_array(&value, "deny");
        let edits: Vec<&str> = deny
            .iter()
            .filter_map(|rule| rule.strip_prefix("Edit(")?.strip_suffix(')'))
            .collect();
        for path in ENFORCEMENT_PATHS {
            assert!(
                edits
                    .iter()
                    .any(|pattern| pattern.starts_with('/') && rule_covers_path(pattern, path)),
                "{name}: no Edit deny covers {path}"
            );
        }
        // Each group's carve-outs follow its own denies, and the hard denies
        // follow every carve-out, so no exception can reopen them.
        let at = |rule: &String| deny.iter().position(|r| r == rule).unwrap();
        let groups = &actions::table().claude_read_groups;
        for group in groups {
            let last_deny = group.denies.iter().map(at).max().unwrap();
            for carveout in &group.carveouts {
                assert!(
                    at(carveout) > last_deny,
                    "{name}: {carveout} precedes its denies"
                );
            }
        }
        let last_carveout = deny.iter().rposition(|r| r.starts_with("Read(!")).unwrap();
        for rule in &groups.last().unwrap().denies {
            assert!(
                at(rule) > last_carveout,
                "{name}: {rule} precedes an exception"
            );
        }
        for key in ["allow", "deny"] {
            assert!(
                !perm_array(&value, key)
                    .iter()
                    .any(|r| r.starts_with("Write(")),
                "{name}: permissions.{key} carries a Write rule"
            );
        }
    }
}

/// Evaluate the preset's `Read` rules in order: a matching rule denies, a
/// matching `!` carve-out lifts the rules before it.
fn read_denied(deny: &[String], path: &str) -> bool {
    let mut denied = false;
    for rule in deny {
        let Some(inner) = rule.strip_prefix("Read(").and_then(|r| r.strip_suffix(')')) else {
            continue;
        };
        let (carveout, pattern) = match inner.strip_prefix('!') {
            Some(pattern) => (true, pattern),
            None => (false, inner),
        };
        if rule_covers_path(pattern, path) {
            denied = !carveout;
        }
    }
    denied
}

/// AC-3: memory, example env files and source files named after secrets stay
/// readable; transcripts, env files and keys stay denied; the sandbox denies
/// the table's credential variables and stores.
#[test]
fn read_denies_keep_memory_and_sources_readable_and_secrets_denied() {
    let table = actions::table();
    for name in preset_files() {
        let value = load(&name);
        let deny = perm_array(&value, "deny");
        for readable in [
            "~/.claude/projects/-work-app/memory/MEMORY.md",
            "~/.claude/projects/-work-app/memory/notes/topic.md",
            ".env.example",
            "web/.env.example",
            "crates/codeflow-core/src/hooks/secret_scan.rs",
            "docs/credentials-guide.md",
        ] {
            assert!(
                !read_denied(&deny, readable),
                "{name}: {readable} is denied"
            );
        }
        for secret in [
            "~/.claude/projects/-work-app/0b1c2d.jsonl",
            "~/.claude/projects/-work-app/sub/agent-1.jsonl",
            ".env",
            "config/.env",
            ".env.local",
            ".env.secret.py",
            ".env.credentials.ts",
            "config/.env.secret.md",
            "certs/server.pem",
            "~/.ssh/id_ed25519",
            "~/.codex/auth.json",
            "~/.claude/.credentials.json",
        ] {
            assert!(read_denied(&deny, secret), "{name}: {secret} is readable");
        }
        let env_vars = value["sandbox"]["credentials"]["envVars"]
            .as_array()
            .unwrap();
        for variable in &table.sandbox_env_denies {
            assert!(
                env_vars
                    .iter()
                    .any(|entry| entry["name"] == variable.as_str() && entry["mode"] == "deny"),
                "{name}: sandbox does not deny {variable}"
            );
        }
        let deny_read = value["sandbox"]["filesystem"]["denyRead"]
            .as_array()
            .unwrap();
        for store in &table.sandbox_read_denies {
            assert!(
                deny_read.iter().any(|entry| entry == store.as_str()),
                "{name}: sandbox denyRead misses {store}"
            );
        }
    }
}

/// Review of PR 749: no `!` exception may reopen a file that a rule of
/// another class denies. Every name here matches both a hard or env deny and
/// a source-name or env exception.
#[test]
fn no_exception_reopens_a_file_another_rule_denies() {
    let mut collisions = Vec::new();
    for ext in [
        "rs", "py", "ts", "tsx", "js", "mjs", "cjs", "go", "java", "kt", "rb", "swift", "md",
    ] {
        for word in ["secret", "credentials"] {
            for name in [
                format!(".env.{word}.{ext}"),
                format!(".env.local-{word}.{ext}"),
                format!("id_rsa_{word}.{ext}"),
                format!("id_ed25519_{word}.{ext}"),
                format!("~/.ssh/{word}.{ext}"),
                format!("~/.aws/{word}.{ext}"),
                format!("~/.gnupg/{word}.{ext}"),
                format!("~/.kube/{word}.{ext}"),
                format!("~/.cargo/credentials-{word}.{ext}"),
                format!("~/.password-store/{word}.{ext}"),
                format!("~/.claude/memory/{word}.{ext}"),
                format!("~/.claude/tasks/{word}.{ext}"),
            ] {
                collisions.push(name);
            }
        }
    }
    for example in [".env.example", ".env.sample", ".env.template", ".env.dist"] {
        collisions.push(format!("~/.ssh/{example}"));
        collisions.push(format!("~/.aws/{example}"));
        collisions.push(format!("~/.claude/memory/{example}"));
    }
    for name in preset_files() {
        let deny = perm_array(&load(&name), "deny");
        for path in &collisions {
            assert!(
                read_denied(&deny, path),
                "{name}: an exception reopens {path}"
            );
        }
    }
}

/// Grok reads the Claude command rules with its own matching (the action
/// table's note). Modelled wider than either reading, so a pass here holds
/// for both: a pattern whose only wildcard is trailing is also read as a
/// literal prefix with no word boundary (`su *` covers `sudoku`), and every
/// pattern is read as a glob that may match anywhere in the command.
fn grok_matches(pattern: &str, command: &str) -> bool {
    let literal = pattern.trim_end_matches('*').trim_end();
    let by_prefix = !literal.is_empty() && !literal.contains('*') && command.starts_with(literal);
    by_prefix || glob_matches(&format!("*{pattern}*"), command)
}

/// AC-4: read with Grok's matching, no preset rule refuses ordinary work.
#[test]
fn grok_matching_of_the_preset_rules_refuses_no_ordinary_work() {
    assert!(
        grok_matches("su *", "sudoku"),
        "the model has no word boundary"
    );
    assert!(grok_matches(
        "gh release create*",
        "x; gh release create v1"
    ));
    for name in preset_files() {
        for rule in perm_array(&load(&name), "deny") {
            let Some(pattern) = ["Bash(", "PowerShell("]
                .iter()
                .find_map(|tool| rule.strip_prefix(tool)?.strip_suffix(')'))
            else {
                continue;
            };
            for command in ORDINARY_WORK {
                assert!(
                    !grok_matches(pattern, command),
                    "{name}: under Grok's matching {rule} refuses {command:?}"
                );
            }
        }
    }
}

/// AC-5: every shipped JSON and TOML asset parses.
#[test]
fn every_shipped_json_and_toml_asset_parses() {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, out);
            } else {
                out.push(path);
            }
        }
    }
    let mut files = Vec::new();
    walk(&repo_root().join("assets"), &mut files);
    let mut checked = 0;
    for path in files {
        let text = || std::fs::read_to_string(&path).unwrap();
        match path.extension().and_then(|e| e.to_str()) {
            Some("json") => {
                serde_json::from_str::<serde_json::Value>(&text())
                    .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            }
            Some("toml") => {
                toml::from_str::<toml::Value>(&text())
                    .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            }
            _ => continue,
        }
        checked += 1;
    }
    assert!(checked > 10, "only {checked} assets checked");
}

fn string_list(value: &toml::Value) -> Vec<String> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

/// AC-5: the Grok builder profile is `cf-guard` plus its two additions.
#[test]
fn grok_worktree_profile_is_cf_guard_plus_the_common_git_dir() {
    let text = std::fs::read_to_string(repo_root().join("assets/base/grok/sandbox.toml")).unwrap();
    let sandbox: toml::Value = toml::from_str(&text).unwrap();
    let guard = &sandbox["profiles"]["cf-guard"];
    let builder = &sandbox["profiles"]["cf-guard-worktree"];
    assert_eq!(guard["extends"].as_str(), Some("workspace"));
    assert_eq!(builder["extends"], guard["extends"]);
    assert_eq!(builder["restrict_network"], guard["restrict_network"]);
    assert_eq!(string_list(&builder["deny"]), string_list(&guard["deny"]));
    let mut read_only = vec!["../../.git/hooks".to_string()];
    read_only.extend(string_list(&guard["read_only"]));
    assert_eq!(string_list(&builder["read_only"]), read_only);
    assert_eq!(string_list(&builder["read_write"]), ["../../.git"]);
    assert!(guard.get("read_write").is_none());
    let reference: toml::Value =
        toml::from_str(&std::fs::read_to_string(evidence("grok/sandbox.toml")).unwrap()).unwrap();
    assert_eq!(
        sandbox, reference,
        "the shipped profiles left the reference"
    );
}

/// AC-6: both new enforcement files ship from the minimal tier, and the
/// Grok profile is written only when absent.
#[test]
fn the_manifest_ships_the_rules_and_the_grok_profile_at_minimal() {
    let text =
        std::fs::read_to_string(repo_root().join("assets/base/scaffold-manifest.toml")).unwrap();
    let manifest: toml::Value = toml::from_str(&text).unwrap();
    let entries = manifest["entry"].as_array().unwrap();
    for (dest, ownership) in [
        (".codex/rules/codeflow.rules", "managed"),
        (".grok/sandbox.toml", "user-owned"),
    ] {
        let entry = entries
            .iter()
            .find(|e| e["dest"].as_str() == Some(dest))
            .unwrap_or_else(|| panic!("{dest} not in the manifest"));
        assert_eq!(entry["ownership"].as_str(), Some(ownership), "{dest}");
        assert!(
            string_list(&entry["tiers"]).contains(&"minimal".to_string()),
            "{dest}: not in the minimal tier"
        );
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
/// nothing of deny precedence over allow rules.
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

// --- AC-7 journey: the 2.1.0 upgrade through the installed CLI -------------

/// Runs the built binary as an adopter would: first on `PATH`, so the hooks
/// the scaffold installs run it too, with an isolated home and host git
/// configuration neutralized.
fn run_codeflow(root: &Path, home: &Path, args: &[&str]) -> std::process::Output {
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_codeflow"));
    let path = std::env::join_paths(exe.parent().map(Path::to_path_buf).into_iter().chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .expect("joinable PATH");
    std::process::Command::new(&exe)
        .args(args)
        .current_dir(root)
        .env("CODEFLOW_HOME", home)
        .env("PATH", path)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("CODEFLOW_INTEGRATE_TOKEN")
        .env_remove("CODEFLOW_HUMAN_OVERRIDE")
        .output()
        .expect("codeflow runs")
}

fn succeeded(out: &std::process::Output, what: &str) -> String {
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "{what} failed:\n{all}");
    all
}

/// Every file under `root` outside `.git`, with its content.
fn snapshot(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut files = vec![];
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.file_name().is_some_and(|n| n == ".git") {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
            } else {
                let content = std::fs::read(&path).unwrap();
                files.push((path.strip_prefix(root).unwrap().to_path_buf(), content));
            }
        }
    }
    files.sort();
    files
}

/// The adopter's own denies: a command, and two reads that overlap the
/// shipped exceptions.
const ADOPTER_DENIES: [&str; 3] = [
    "Bash(terraform apply *)",
    "Read(**/.env.example)",
    "Read(**/team-secret.py)",
];
const ADOPTER_COMMAND: &str = "terraform apply -auto-approve";

/// A project made by `init --minimal`, holding the 2.1.0 Claude preset and
/// policy as its files and their baseline copies. An adopter project adds
/// its own denies; `settings_baseline: false` deletes the settings baseline.
fn planted_2_1_0_project(
    adopter: bool,
    settings_baseline: bool,
) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("proj");
    let home = dir.path().join("home");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    succeeded(
        &run_codeflow(&root, &home, &["init", "--yes", "--minimal"]),
        "init --minimal",
    );
    let fixtures = repo_root().join("crates/codeflow-core/tests/fixtures/presets-2.1.0");
    let settings = std::fs::read_to_string(fixtures.join("default.json")).unwrap();
    let policy = std::fs::read_to_string(fixtures.join("policy.json")).unwrap();
    let mut project: serde_json::Value = serde_json::from_str(&settings).unwrap();
    if adopter {
        let deny = project["permissions"]["deny"].as_array_mut().unwrap();
        deny.extend(ADOPTER_DENIES.map(serde_json::Value::from));
    }
    let plant = |rel: &str, content: &str| {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    };
    plant(
        ".claude/settings.json",
        &(serde_json::to_string_pretty(&project).unwrap() + "\n"),
    );
    plant(".codeflow/policy.json", &policy);
    plant(".codeflow/.baseline/.codeflow/policy.json", &policy);
    plant(".codeflow/.baseline/.claude/settings.json", &settings);
    if !settings_baseline {
        std::fs::remove_file(root.join(".codeflow/.baseline/.claude/settings.json")).unwrap();
    }
    (dir, root, home)
}

/// The effective rules of the updated settings: every deny of the new
/// preset is present; an adopter's command and reads are refused; in the
/// ordinary project, secret env variants are denied while a source file
/// named after secrets and `.env.example` stay readable.
fn assert_effective_rules(root: &Path, case: &str, adopter: bool) -> serde_json::Value {
    let text = std::fs::read_to_string(root.join(".claude/settings.json")).unwrap();
    let updated: serde_json::Value = serde_json::from_str(&text).unwrap();
    let deny = perm_array(&updated, "deny");
    for rule in perm_array(&load("default.json"), "deny") {
        assert!(deny.contains(&rule), "{case}: missing new deny {rule}");
    }
    let expected: &[(&str, bool)] = if adopter {
        &[(".env.example", true), ("app/team-secret.py", true)]
    } else {
        &[
            (".env.secret.py", true),
            ("config/.env.credentials.ts", true),
            ("crates/scan/src/secret_scan.rs", false),
            (".env.example", false),
        ]
    };
    for (path, denied_read) in expected {
        assert_eq!(
            read_denied(&deny, path),
            *denied_read,
            "{case}: reading {path} should be {}",
            if *denied_read { "denied" } else { "allowed" }
        );
    }
    assert_eq!(
        denied(&deny, "Bash", ADOPTER_COMMAND),
        adopter,
        "{case}: {ADOPTER_COMMAND}"
    );
    updated
}

/// AC-7 journey: three projects on the 2.1.0 preset and policy upgrade
/// through the installed CLI, an ordinary one and two adopters, one of
/// which lost its settings baseline. The first update retires the asks and
/// moves `privilege_escalation` from warn to block, reporting both, and
/// without a baseline says it compared with the 2.1.0 copy. After each
/// update the effective rules hold; the second changes no file and
/// `validate` is clean.
#[test]
fn a_2_1_0_project_upgrades_through_the_installed_cli() {
    for (case, adopter, settings_baseline) in [
        ("ordinary", false, true),
        ("adopter", true, true),
        ("adopter without a settings baseline", true, false),
    ] {
        let (_dir, root, home) = planted_2_1_0_project(adopter, settings_baseline);
        let first = succeeded(&run_codeflow(&root, &home, &["update"]), "first update");
        let updated = assert_effective_rules(&root, case, adopter);
        assert!(
            updated["permissions"].get("ask").is_none(),
            "{case}: the shipped asks are retired:\n{first}"
        );
        assert!(
            first.contains("removed retired permissions.ask entry \"Bash(sudo *)\""),
            "{case}: the retired asks are reported:\n{first}"
        );
        assert_eq!(
            first.contains("compared with the copy CodeFlow 2.1.0 shipped"),
            !settings_baseline,
            "{case}: the copy used is reported only without a baseline:\n{first}"
        );
        let policy: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(root.join(".codeflow/policy.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            policy["security"]["privilege_escalation"], "block",
            "{case}"
        );
        assert!(
            first.contains("moved security.privilege_escalation from \"warn\" to \"block\""),
            "{case}: the moved default is reported:\n{first}"
        );

        let before = snapshot(&root);
        succeeded(&run_codeflow(&root, &home, &["update"]), "second update");
        assert!(
            snapshot(&root) == before,
            "{case}: the second update changed a file"
        );
        assert_effective_rules(&root, &format!("{case}, second update"), adopter);
        let validate = succeeded(&run_codeflow(&root, &home, &["validate"]), "validate");
        assert!(
            !validate.to_lowercase().contains("warn"),
            "{case}: validate is not clean:\n{validate}"
        );
    }
}
