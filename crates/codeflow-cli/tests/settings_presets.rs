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
const TOP_LEVEL_KEYS: [&str; 8] = [
    "$schema",
    "attribution",
    "env",
    "hooks",
    "includeCoAuthoredBy",
    "permissions",
    "sandbox",
    "statusLine",
];

/// Project policy forbids AI attribution in commits and PR bodies. Every
/// preset turns the host injection off at the source with both the older
/// boolean and the newer object, so no agent learns the rule only by being
/// blocked.
#[test]
fn presets_disable_host_attribution() {
    for name in preset_files() {
        let bytes = std::fs::read(settings_dir().join(&name)).unwrap();
        let preset: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(preset["includeCoAuthoredBy"], false, "{name}");
        assert_eq!(preset["attribution"]["commit"], "", "{name}");
        assert_eq!(preset["attribution"]["pr"], "", "{name}");
        assert_eq!(preset["attribution"]["sessionUrl"], false, "{name}");
    }
}

/// TSK-1028 AC-3: effort is each person's setting, so no preset pins it;
/// seat launches pass their effort explicitly (ADR-0087 supersedes the
/// settings pin of ADR-0056).
#[test]
fn presets_leave_effort_to_each_person() {
    for name in preset_files() {
        let bytes = std::fs::read(settings_dir().join(&name)).unwrap();
        let preset: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(preset.get("effortLevel").is_none(), "{name}");
    }
}

/// TSK-1028 AC-5: with no effort in the shared settings, every seat launch
/// in `cf-herdr` still passes its effort, so seat effort is unchanged. The
/// transport posture table adds only posture flags after these.
#[test]
fn every_seat_launch_passes_its_effort() {
    let root = settings_dir().join("../../..");
    let herdr =
        std::fs::read_to_string(root.join("assets/base/agents/skills/cf-herdr/SKILL.md")).unwrap();
    assert!(herdr.contains(
        "Native args after `--` are the ensemble's primary selector and effort, then\n\
         the seat's posture flags from the transport table"
    ));
    for launch in [
        "- Claude: `--model <selector> --effort <effort> <posture flags>`",
        "- Codex: `--model <selector> -c model_reasoning_effort=\"<effort>\" <posture flags>`",
        "- Grok: `--model <selector> --reasoning-effort <effort> <posture flags>`",
    ] {
        assert!(herdr.contains(launch), "cf-herdr lost {launch}");
    }
    let transport = std::fs::read_to_string(
        root.join("assets/base/agents/skills/cf-model-orchestrator/resources/routing/transport.md"),
    )
    .unwrap();
    for row in transport.lines().filter(|line| {
        ["| Claude |", "| Codex |", "| Grok |"]
            .iter()
            .any(|seat| line.starts_with(seat))
    }) {
        assert!(
            !row.contains("effort"),
            "a posture row overrides effort: {row}"
        );
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

/// TSK-211 AC-1: every preset, and this repository's own settings, carry the
/// compaction default as strings, and nothing else in `env`.
#[test]
fn every_preset_and_the_repository_carry_the_compaction_default() {
    let expected = serde_json::json!({
        "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "1000000",
        "CLAUDE_AUTOCOMPACT_PCT_OVERRIDE": "35"
    });
    let path = settings_dir().join("../../../.claude/settings.json");
    let bytes =
        std::fs::read(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    let repository: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(repository["env"], expected, "repository settings");

    for name in preset_files() {
        assert_eq!(load(&name)["env"], expected, "{name}");
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
        "`codeflow init` writes these string values",
        "\"CLAUDE_CODE_AUTO_COMPACT_WINDOW\": \"1000000\"",
        "\"CLAUDE_AUTOCOMPACT_PCT_OVERRIDE\": \"35\"",
        "capped at the model's real context window",
        "can only lower the compaction threshold",
        "applies to main sessions and to subagents",
        "| Model with a native 1M window | compacts at about 967K tokens | compacts at about 350K tokens |",
        "| Session limited to 200K | compacts at the 200K boundary | compacts at about 70K tokens |",
        "`used_percentage` measures against the full model window, so it no longer shows when compaction runs",
        "Update keeps a value that differs from the one CodeFlow last shipped",
        "update cannot tell a deliberate 50 from the shipped 50, so a project on 50 moves to 35",
        "To keep 50, set it again in the shared `env` after the update",
        "the update report names the key it kept, never the value",
        "A key CodeFlow wrote that the project has since deleted stays deleted",
        "a turn near 350K carries about 0.7 times the input of a turn near 500K",
        "A lower point compacts more often",
        "compact at about 70K",
        "The point matters most for long orchestrator sessions",
        "Detail that lives only in the conversation may be lost",
        "let a session recover its task, branch and next step when it rereads them",
        "supplies context and pointers to those records; it does not restore the session's state by itself",
        "is revisited once an adopter's measurement of 50, 40, 35 and 30 percent is published on CodeFlow issue 358",
        "Since Claude Code v2.1.211 a worktree session uses the file at the main checkout's root",
        "delete both keys or the whole `env` object. It stays deleted",
        "With no recorded baseline, update adds the keys only when the project has no `env` at all",
        "Update reports key names, never values",
        "inspect without printing sensitive values",
        "Present only the proposed non-secret context-key delta and any conflicts",
        "A malformed object is a reported configuration error",
        "configured value, or requested launch is not runtime evidence",
        "manual compaction/resume check",
        "does not prove the automatic threshold",
        "never copy this `env` into the immutable delegated task settings",
    ] {
        assert!(policy.contains(marker), "context policy lost: {marker}");
    }
    for stale in [
        "opt-in",
        "Do not add it to CodeFlow's generic settings presets",
    ] {
        assert!(!policy.contains(stale), "context policy kept: {stale}");
    }
    assert!(customize.contains("init writes a compaction default"));
    for marker in [
        "carries a context-window/compaction environment by default",
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
            has("*credential*.json") && has("*secret*.json"),
            "{name}: secret-named data files must be deny-read"
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

/// TSK-1070 (operator decision 2026-10-31): Claude Code merges `Read` deny
/// rules into the OS sandbox, which cannot apply a `!` exception, so a
/// secret-name deny must not match a source file on its own. A source or
/// documentation file named after secrets stays readable by every rule
/// taken without its exceptions, while secret-named data files, exact
/// secret names and key material stay denied.
#[test]
fn secret_name_denies_spare_source_files_without_any_exception() {
    let read_denies = |value: &serde_json::Value| -> Vec<String> {
        perm_array(value, "deny")
            .into_iter()
            .filter_map(|rule| {
                let inner = rule.strip_prefix("Read(")?.strip_suffix(')')?;
                (!inner.starts_with('!')).then(|| inner.to_string())
            })
            .collect()
    };
    for name in preset_files() {
        let value = load(&name);
        let denies = read_denies(&value);
        for source in [
            "crates/codeflow-cli/tests/ci_cli/secret_scan_exemption.rs",
            "crates/codeflow-cli/tests/refusal_journey/secret_paths.rs",
            "crates/codeflow-core/tests/managed_secret_scan.rs",
            "src/secrets.ts",
            "lib/credentials_helper.py",
            "pkg/secret.go",
            "docs/credentials-guide.md",
            "web/secretStore.js",
        ] {
            let hit: Vec<&String> = denies
                .iter()
                .filter(|pattern| rule_covers_path(pattern, source))
                .collect();
            assert!(hit.is_empty(), "{name}: {source} is denied by {hit:?}");
        }
        for data in [
            "config/app-secret.json",
            "k8s/secrets.yaml",
            "deploy/db-credentials.env",
            "gcp-credential.json",
            "secret",
            "config/.secrets",
            "infra/credentials",
            "vault/secret.toml",
            "keys/service-secret.pem",
        ] {
            assert!(
                denies.iter().any(|pattern| rule_covers_path(pattern, data)),
                "{name}: {data} is readable"
            );
        }
    }
}

/// TSK-1070 (operator decision 2026-10-31): the Read tool and the sandbox
/// refuse the token stores of the common CLIs.
#[test]
fn token_stores_are_denied_to_the_read_tool_and_the_sandbox() {
    let stores = [
        ("~/.config/gh/hosts.yml", "~/.config/gh"),
        ("~/.git-credentials", "~/.git-credentials"),
        ("~/.docker/config.json", "~/.docker/config.json"),
        ("~/.config/gcloud/credentials.db", "~/.config/gcloud"),
        ("~/.azure/msal_token_cache.json", "~/.azure"),
        ("~/.npmrc", "~/.npmrc"),
        (
            "~/Library/Keychains/login.keychain-db",
            "~/Library/Keychains",
        ),
    ];
    for name in preset_files() {
        let value = load(&name);
        let deny = perm_array(&value, "deny");
        let deny_read = value["sandbox"]["filesystem"]["denyRead"]
            .as_array()
            .unwrap();
        for (file, store) in stores {
            assert!(read_denied(&deny, file), "{name}: Read of {file} allowed");
            assert!(
                deny_read.iter().any(|entry| entry == store),
                "{name}: sandbox denyRead misses {store}"
            );
        }
    }
}

/// TSK-1070 (operator decision 2026-10-31): a command the sandbox refuses
/// cannot rerun outside it, in any mode. The few tools that need host
/// state run outside it by name: the `gh` subcommands Codeflow uses read
/// the token from `~/.config/gh` and fail TLS under Seatbelt, and a plain
/// `git push` over HTTPS asks `gh` for the credential. `herdr` is offered
/// at user scope only (sandbox exemptions research, section 6). Bash deny
/// and ask rules still apply to them.
#[test]
fn every_preset_runs_a_strict_sandbox_with_a_minimal_excluded_list() {
    for name in preset_files() {
        let sandbox = &load(&name)["sandbox"];
        assert_eq!(sandbox["enabled"], true, "{name}");
        assert_eq!(sandbox["failIfUnavailable"], true, "{name}");
        assert_eq!(sandbox["allowUnsandboxedCommands"], false, "{name}");
        assert_eq!(
            sandbox["excludedCommands"],
            serde_json::json!(SHIPPED_EXCLUDED_COMMANDS),
            "{name}"
        );
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

/// The extra sandbox write roots, so an agent can run these commands without
/// a sandbox bypass: the per-user state directory `codeflow present` needs
/// (macOS, then Linux without `XDG_STATE_HOME`), whose runtime match
/// `present_cli` proves. With them the presets grant the `CodeFlow` home's
/// machine-wide lock directory (TSK-216), and never its durable evidence
/// directory (TSK-1044 AC-4: full gates run outside the sandbox). The rest
/// of the home, which holds qualification and binding state, stays out.
const PRESENT_STATE_WRITE_ROOTS: [&str; 2] = [
    "~/Library/Application Support/codeflow/present",
    "~/.local/state/codeflow/present",
];

#[test]
fn sandbox_allows_writes_only_to_present_state_and_the_gate_directories() {
    use codeflow_core::testing::gate_guard::HOME_LOCK_DIR;
    let gate_roots = [format!("~/.codeflow/{HOME_LOCK_DIR}")];
    let expected: Vec<&str> = PRESENT_STATE_WRITE_ROOTS
        .iter()
        .copied()
        .chain(gate_roots.iter().map(String::as_str))
        .collect();
    for name in preset_files() {
        let value = load(&name);
        let allow_write: Vec<&str> = value["sandbox"]["filesystem"]["allowWrite"]
            .as_array()
            .unwrap_or_else(|| panic!("{name}: sandbox.filesystem.allowWrite missing"))
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect();
        assert_eq!(
            allow_write, expected,
            "{name}: the sandbox may add only the cf-present state directory and the gate's lock directory as write roots"
        );
        // The only write denies are the shell startup class and the default
        // git configuration files (TSK-242), and the tracked hook directory a
        // push runs outside the sandbox (TSK-1070).
        let table = actions::table();
        let mut denied = table.startup_paths.sandbox_write_denies();
        denied.extend(table.git_config_paths.sandbox_write_denies());
        denied.extend(table.sandbox_write_denies.iter().cloned());
        assert_eq!(
            value["sandbox"]["filesystem"]["denyWrite"],
            serde_json::json!(denied),
            "{name}: the sandbox denies writes only to the shell startup class and the default git configuration files"
        );
    }
}

/// TSK-1044 AC-4, decided by the operator on 2026-10-30: no Claude preset
/// grants the gate evidence folder, so a sandboxed session cannot plant or
/// replace durable evidence; full gates run outside the sandbox. Checked in
/// every shipped preset and in this repository's managed baseline copy.
#[test]
fn no_claude_settings_grant_the_gate_evidence_folder() {
    use codeflow_core::testing::gate_guard::HOME_EVIDENCE_DIR;
    let evidence = format!(".codeflow/{HOME_EVIDENCE_DIR}");
    let mut copies: Vec<(String, serde_json::Value)> = preset_files()
        .into_iter()
        .map(|name| {
            let value = load(&name);
            (name, value)
        })
        .collect();
    let baseline = repo_root().join(".codeflow/.baseline/.claude/settings.json");
    copies.push((baseline.display().to_string(), read_json(&baseline)));
    for (name, value) in copies {
        let grants: Vec<&str> = value["sandbox"]["filesystem"]["allowWrite"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
            .filter(|path| path.contains(&evidence))
            .collect();
        assert!(grants.is_empty(), "{name} grants {grants:?}");
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

/// The GitHub CLI still reaches its own configuration. Until TSK-1070 the
/// presets left `~/.config/gh` and `~/.docker/config.json` readable so a
/// sandboxed `gh` or `docker` could find its credential. The operator
/// decided on 2026-10-31 to deny those token stores to the Read tool and
/// the sandbox, so `gh` runs outside the sandbox by name instead, where it
/// reads its configuration and the guards and deny rules still judge it.
/// Docker gets no exclusion: a sandboxed `docker` cannot reach its socket
/// either, so a project that drives it adds its own entry.
#[test]
fn authenticated_tool_configuration_remains_available() {
    for name in preset_files() {
        let value = load(&name);
        let deny = perm_array(&value, "deny");
        assert!(deny.iter().any(|d| d == "Read(~/.config/gh/**)"), "{name}");
        let excluded = value["sandbox"]["excludedCommands"].as_array().unwrap();
        assert!(
            excluded.iter().any(|entry| entry == "gh pr view *"),
            "{name}: gh cannot read its configuration inside the sandbox"
        );
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
        // TSK-1070 (operator decision 2026-10-31): the only exclusions are
        // the named host-state tools, never a broad one.
        assert_eq!(
            sandbox["excludedCommands"],
            serde_json::json!(SHIPPED_EXCLUDED_COMMANDS),
            "{name}: the sandbox exclusions changed"
        );
        assert_eq!(sandbox["allowUnsandboxedCommands"], false, "{name}");
    }
}

/// The tools the shipped sandbox runs outside it by name (TSK-1070): only
/// `gh` verbs that change nothing on this machine (security review SEC-03:
/// `gh run download` extracts an archive, `gh pr checkout` and
/// `gh issue develop` run git, so they stay sandboxed).
const SHIPPED_EXCLUDED_COMMANDS: [&str; 31] = [
    "gh pr view *",
    "gh pr list *",
    "gh pr checks *",
    "gh pr diff *",
    "gh pr status *",
    "gh pr create *",
    "gh pr comment *",
    "gh pr edit *",
    "gh pr ready *",
    "gh pr review *",
    "gh pr merge *",
    "gh run view *",
    "gh run list *",
    "gh run watch *",
    "gh run rerun *",
    "gh run cancel *",
    "gh issue view *",
    "gh issue list *",
    "gh issue status *",
    "gh issue create *",
    "gh issue comment *",
    "gh issue edit *",
    "gh issue close *",
    "gh issue reopen *",
    "gh api *",
    "gh repo view *",
    "gh release view *",
    "gh release list *",
    "gh search *",
    "gh auth status",
    "git push *",
];

/// The development hosts sandboxed commands may reach (TSK-1070, operator
/// decision 2026-10-31, replacing ADR-0025's `*`). Each has a use:
/// GitHub for git, releases, raw files and archives; crates.io and
/// `static.rust-lang.org` for cargo and rustup; npm, `PyPI` and the Go module
/// proxy for the shipped node, python and go stacks; the two Playwright
/// download hosts for the docs portal's browser check; the Claude Code and
/// Anthropic docs for lookups from the shell.
const SHIPPED_ALLOWED_DOMAINS: [&str; 17] = [
    "github.com",
    "api.github.com",
    "codeload.github.com",
    "*.githubusercontent.com",
    "crates.io",
    "index.crates.io",
    "static.crates.io",
    "static.rust-lang.org",
    "registry.npmjs.org",
    "pypi.org",
    "files.pythonhosted.org",
    "proxy.golang.org",
    "sum.golang.org",
    "cdn.playwright.dev",
    "playwright.download.prss.microsoft.com",
    "code.claude.com",
    "docs.anthropic.com",
];

#[test]
fn every_preset_has_a_fail_closed_autonomous_sandbox() {
    // Native web tools keep public research; sandboxed commands reach only
    // the shipped development hosts, and the sandbox also blocks common
    // private and link-local destinations.
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
        // TSK-1070 (operator decision 2026-10-31) replaced ADR-0029's
        // classified retry: a refused command never reruns outside the
        // sandbox, and only the named host-state tools run outside it.
        assert_eq!(
            sandbox["allowUnsandboxedCommands"], false,
            "{name}: a refused command must not rerun outside the sandbox"
        );
        assert_eq!(
            sandbox["excludedCommands"],
            serde_json::json!(SHIPPED_EXCLUDED_COMMANDS),
            "{name}: only the named host-state tools run outside the sandbox"
        );
        assert_eq!(
            sandbox["network"]["allowLocalBinding"], true,
            "{name}: local dev servers must be available to UI tests"
        );
        // TSK-1070 (operator decision 2026-10-31): sandboxed commands reach
        // only the named development hosts, never every public host.
        assert_eq!(
            sandbox["network"]["allowedDomains"],
            serde_json::json!(SHIPPED_ALLOWED_DOMAINS),
            "{name}: the sandbox reaches only the shipped development hosts"
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
fn minimal_claude_guidance_matches_the_strict_sandbox_setting() {
    // TSK-1070 (operator decision 2026-10-31) replaced ADR-0029's classified
    // retry: the guidance must not promise a rerun outside the sandbox.
    for template in ["CLAUDE.minimal.md.tmpl", "CLAUDE.md.tmpl"] {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/base")
            .join(template);
        let guidance = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        assert!(!guidance.contains("unsandboxed retry"), "{template}");
        assert!(
            guidance.contains("never reruns outside"),
            "{template}: say a refused command is not rerun outside the sandbox"
        );
        assert!(guidance.contains("development hosts"), "{template}");
    }
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

/// TSK-1070 (issue 451): no preset ships a `defaultMode` that Claude Code
/// ignores at project scope. Since v2.1.257 a project `bypassPermissions`
/// does not take effect and the session starts in Manual mode, and a
/// project value outranks the user file, so shipping it would also hold a
/// person's user-level bypass back. The bypass preset therefore leaves the
/// mode unset; bypass comes from the user settings file or the launch flag,
/// and the guard hooks keep blocking on a timeout or a crash.
#[test]
fn no_preset_ships_a_mode_claude_code_ignores_at_project_scope() {
    for name in preset_files() {
        let value = load(&name);
        let mode = value["permissions"].get("defaultMode");
        assert!(
            !matches!(
                mode.and_then(serde_json::Value::as_str),
                Some("bypassPermissions" | "auto")
            ),
            "{name}: ships defaultMode {mode:?}, which a project file cannot set"
        );
        for hook in value["hooks"]["PreToolUse"][0]["hooks"].as_array().unwrap() {
            assert_eq!(hook["onFailure"], "block", "{name}: {hook}");
            assert!(hook["timeout"].as_u64().is_some(), "{name}: {hook}");
        }
        assert_eq!(value["sandbox"]["enabled"], true, "{name}");
        assert_eq!(value["sandbox"]["failIfUnavailable"], true, "{name}");
    }
    assert!(load("bypass-sandboxed.json")["permissions"]
        .get("defaultMode")
        .is_none());
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
/// the design started from, with two stated departures: the read rules are
/// ordered in groups (review of PR 749), so the deny array holds the same
/// rules as the reference in a different order; and the shell startup class
/// and the default git configuration files (TSK-242), added after the
/// samples were taken, add their `Edit` denies and sandbox `denyWrite`
/// entries, which are removed here before the comparison. Hook commands are excluded: their fail-closed form and
/// `--contract` flag are TSK-173's. A third departure is TSK-1070's
/// (operator decision 2026-10-31): the bare `*secret*` and `*credentials*`
/// denies and their source exceptions became a list of secret-named data
/// files, and seven token stores joined the hard denies and the sandbox
/// `denyRead`; four token variables joined the sandbox env denies; and the
/// `forge_exec` and `forge_write` families were added (sandbox exemptions
/// research); [`tsk1070_departure`] maps the reference across it.
#[test]
fn the_generator_reproduces_the_reference_samples() {
    let table = actions::table();
    let reference: serde_json::Value = read_json(&evidence("actions.json"));
    let departure = tsk1070_departure();
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
    denies.retain(|rule| !departure.added_denies.contains(rule));
    denies.extend(departure.removed_denies.iter().cloned());
    denies.sort();
    assert_eq!(denies, sorted(&reference["claude_read_denies"]));
    let mut carveouts = table.claude_read_carveouts();
    carveouts.extend(departure.removed_carveouts.iter().cloned());
    carveouts.sort();
    assert_eq!(carveouts, sorted(&reference["claude_read_carveouts"]));
    for key in [
        "families",
        "claude_edit_denies",
        "sandbox_env_denies",
        "sandbox_read_denies",
        "delegate_denies",
    ] {
        let mut embedded: serde_json::Value = serde_json::from_str(include_str!(
            "../../codeflow-core/src/security/actions.json"
        ))
        .unwrap();
        if key == "sandbox_read_denies" {
            embedded[key]
                .as_array_mut()
                .unwrap()
                .retain(|store| !departure.added_stores.iter().any(|s| store == s));
        }
        if key == "sandbox_env_denies" {
            embedded[key]
                .as_array_mut()
                .unwrap()
                .retain(|name| !departure.added_env.iter().any(|s| name == s));
        }
        if key == "families" {
            let families = embedded[key].as_array_mut().unwrap();
            families.retain(|f| !departure.added_families.iter().any(|id| f["id"] == *id));
            for family in families {
                family["claude"]
                    .as_array_mut()
                    .unwrap()
                    .retain(|rule| !departure.added_rules.iter().any(|r| rule == r));
            }
        }
        assert_eq!(
            embedded[key], reference[key],
            "{key} left the reference table"
        );
    }
    let mut reference_table = table.clone();
    reference_table
        .families
        .retain(|f| !departure.added_families.contains(&f.id.as_str()));
    for name in PRESET_FILES {
        let mut generated = read_json(&evidence(&format!("current/{name}")));
        table.apply_to_claude_preset(&mut generated);
        departure.map_to_reference(table, &mut generated);
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
        reference_table.codex_rules(),
        std::fs::read_to_string(evidence("codex/rules/codeflow.rules")).unwrap()
    );
    assert_eq!(
        table.delegate_fragment(),
        read_json(&evidence("claude/delegate-settings-fragment.json"))
    );
}

/// The read rules TSK-1070 changed against the design's reference table.
struct Departure {
    added_denies: Vec<String>,
    removed_denies: Vec<String>,
    removed_carveouts: Vec<String>,
    added_stores: Vec<String>,
    added_env: Vec<&'static str>,
    added_families: Vec<&'static str>,
    /// Rules TSK-1070's security review added to a family the reference
    /// already had: the other spellings of a `gh api` DELETE (SEC-05).
    added_rules: Vec<String>,
}

impl Departure {
    /// Map a generated preset back to the reference: drop what the shell
    /// startup class, the git configuration files and TSK-1070 added, and
    /// restore what TSK-1070 removed.
    fn map_to_reference(&self, table: &actions::ActionTable, generated: &mut serde_json::Value) {
        let added_family_rules: Vec<String> = table
            .families
            .iter()
            .filter(|f| self.added_families.contains(&f.id.as_str()))
            .flat_map(|f| f.claude.iter().cloned())
            .collect();
        generated["permissions"]
            .as_object_mut()
            .unwrap()
            .remove("ask");
        generated["sandbox"]["credentials"]["envVars"]
            .as_array_mut()
            .unwrap()
            .retain(|e| !self.added_env.iter().any(|s| e["name"] == *s));
        let mut startup = table.startup_paths.claude_edit_denies();
        startup.extend(table.git_config_paths.claude_edit_denies());
        let deny = generated["permissions"]["deny"].as_array_mut().unwrap();
        deny.retain(|rule| {
            !startup
                .iter()
                .chain(&added_family_rules)
                .chain(&self.added_denies)
                .chain(&self.added_rules)
                .any(|s| rule == s)
        });
        deny.extend(
            self.removed_denies
                .iter()
                .chain(&self.removed_carveouts)
                .map(|rule| serde_json::json!(rule)),
        );
        generated["sandbox"]["filesystem"]["denyRead"]
            .as_array_mut()
            .unwrap()
            .retain(|store| !self.added_stores.iter().any(|s| store == s));
        generated["sandbox"]["filesystem"]
            .as_object_mut()
            .unwrap()
            .remove("denyWrite");
    }
}

/// The `gh api` DELETE spellings the reference samples deny.
const REFERENCE_DELETE_FORMS: [&str; 4] = [
    "-X DELETE",
    "-XDELETE",
    "--method DELETE",
    "--method=DELETE",
];

fn tsk1070_departure() -> Departure {
    let table = actions::table();
    let stores = [
        ("Read(~/.config/gh/**)", "~/.config/gh"),
        ("Read(~/.git-credentials)", "~/.git-credentials"),
        ("Read(~/.docker/config.json)", "~/.docker/config.json"),
        ("Read(~/.config/gcloud/**)", "~/.config/gcloud"),
        ("Read(~/.azure/**)", "~/.azure"),
        ("Read(~/.npmrc)", "~/.npmrc"),
        ("Read(~/Library/Keychains/**)", "~/Library/Keychains"),
    ];
    let mut added_denies = table.claude_read_groups[0].denies.clone();
    added_denies.extend(stores.iter().map(|(rule, _)| (*rule).to_string()));
    let mut removed_carveouts = Vec::new();
    for word in ["secret", "credentials"] {
        for ext in [
            "rs", "py", "ts", "tsx", "js", "mjs", "cjs", "go", "java", "kt", "rb", "swift", "md",
        ] {
            removed_carveouts.push(format!("Read(!**/*{word}*.{ext})"));
        }
    }
    Departure {
        added_denies,
        removed_denies: vec![
            "Read(**/*credentials*)".to_string(),
            "Read(**/*secret*)".to_string(),
        ],
        removed_carveouts,
        added_stores: stores.iter().map(|(_, s)| (*s).to_string()).collect(),
        added_env: vec!["GH_TOKEN", "GITHUB_TOKEN", "NPM_TOKEN", "NODE_AUTH_TOKEN"],
        added_families: vec!["forge_exec", "forge_write"],
        added_rules: table
            .families
            .iter()
            .flat_map(|f| f.claude.iter())
            .filter(|rule| {
                rule.starts_with("Bash(gh api ")
                    && rule.to_ascii_uppercase().contains("DELETE")
                    && !REFERENCE_DELETE_FORMS.iter().any(|form| {
                        *rule == &format!("Bash(gh api {form}*)")
                            || *rule == &format!("Bash(gh api * {form}*)")
                    })
            })
            .cloned()
            .collect(),
    }
}

/// AC-1: the commands ordinary work runs, which no deny rule may refuse.
/// `gh secret list` left the list with TSK-1070: every `gh secret` form is
/// refused now that `gh` runs outside the sandbox.
const ORDINARY_WORK: [&str; 10] = [
    "rm -rf /tmp/x",
    "rm -rf ~/scratch/x",
    "gh release view v1",
    "crontab -l",
    "git branch -d topic",
    "cargo test",
    "npm run build",
    "gh pr create --draft",
    "cargo +nightly test --features publish",
    "npm --prefix x run publish-docs",
];

/// AC-1: the only asks are the forge writes (TSK-1070, ADR-0093), and no
/// deny or ask rule matches ordinary work.
#[test]
fn presets_ask_only_for_forge_writes_and_deny_no_ordinary_work() {
    for name in preset_files() {
        let value = load(&name);
        let ask = perm_array(&value, "ask");
        assert_eq!(ask, actions::table().claude_ask(), "{name}");
        let deny = perm_array(&value, "deny");
        for command in ORDINARY_WORK.iter().chain(&FORGE_READS) {
            assert!(
                !denied(&deny, "Bash", command),
                "{name}: a deny rule refuses ordinary work {command:?}"
            );
            assert!(
                !denied(&ask, "Bash", command),
                "{name}: an ask rule prompts for ordinary work {command:?}"
            );
        }
        for command in FORGE_WRITES {
            assert!(
                denied(&ask, "Bash", command),
                "{name}: {command:?} does not ask"
            );
        }
    }
}

/// `gh` reads an agent runs outside the sandbox without a prompt.
const FORGE_READS: [&str; 6] = [
    "gh pr view 12 --json state",
    "gh api repos/o/r/pulls/1",
    "gh api -X GET search/issues",
    "gh run view 3 --log-failed",
    "gh auth status",
    "gh repo view o/r",
];

/// `gh api` writes that prompt in every mode (TSK-1070).
const FORGE_WRITES: [&str; 22] = [
    "gh api -X POST repos/o/r/issues",
    "gh api repos/o/r/issues -X PATCH",
    "gh api --method=PUT repos/o/r/x",
    "gh api repos/o/r/issues/1/comments -f body=x",
    "gh api repos/o/r/issues -F title=x",
    "gh api repos/o/r/x --input body.json",
    "gh api -XPOST repos/o/r/x",
    "gh api graphql -f query=x",
    // TSK-1070 SEC-05: attached values, `=` and lowercase spellings, an
    // `-i` bundle, graphql after other flags and another host.
    "gh api repos/o/r/issues/1/comments -fbody=x",
    "gh api repos/o/r/issues/1/comments -Fbody=x",
    "gh api -fbody=x repos/o/r/issues/1/comments",
    "gh api repos/o/r/x --field=body=x",
    "gh api repos/o/r/x --raw-field=body=x",
    "gh api -X=POST repos/o/r/x",
    "gh api repos/o/r/x -Xpost",
    "gh api repos/o/r/x --method patch",
    "gh api repos/o/r/x --method=put",
    "gh api -iXPOST repos/o/r/x",
    "gh api repos/o/r/x -ifbody=x",
    "gh api -H 'Accept: x' graphql -f query=x",
    "gh api --hostname example.invalid /",
    "gh api / --hostname=example.invalid",
];

/// AC-2: a representative command per family, each denied.
const FAMILY_EXAMPLES: [(&str, &[&str]); 8] = [
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
            "gh api -X=DELETE repos/o/r",
            "gh api repos/o/r --method delete",
            "gh api repos/o/r -Xdelete",
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
    (
        "forge_exec",
        &[
            "gh extension install o/gh-x",
            "gh alias set co 'pr checkout'",
            "gh config set git_protocol ssh",
            "gh auth status --show-token",
            "gh auth status -h github.com -t",
            "gh codespace ssh",
            "gh ssh-key add k.pub",
            "gh secret list",
            "gh variable set X",
            "gh repo clone o/r",
            "git push --receive-pack=/tmp/x origin HEAD",
            "git push origin HEAD --exec=/tmp/x",
            "git push ext::sh\\ -c\\ id HEAD",
        ],
    ),
    ("forge_write", &FORGE_WRITES),
];

/// AC-2: every preset denies every family of the table.
#[test]
fn every_preset_denies_every_action_family() {
    let table = actions::table();
    let ids: Vec<&str> = table.families.iter().map(|f| f.id.as_str()).collect();
    let covered: Vec<&str> = FAMILY_EXAMPLES.iter().map(|(id, _)| *id).collect();
    assert_eq!(ids, covered, "a family without examples");
    for name in preset_files() {
        let value = load(&name);
        // The forge write family asks (TSK-1070); every other family denies.
        let rules = |id: &str| perm_array(&value, if id == "forge_write" { "ask" } else { "deny" });
        for family in &table.families {
            let rules = rules(&family.id);
            for rule in &family.claude {
                assert!(rules.contains(rule), "{name}: {} lost {rule}", family.id);
            }
        }
        for (id, commands) in FAMILY_EXAMPLES {
            let rules = rules(id);
            for command in commands {
                assert!(
                    denied(&rules, "Bash", command),
                    "{name}: {id} command {command:?} is not refused"
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

/// TSK-1070 SEC-02: Codeflow's launcher files under the home (the shell
/// function, the editor wrapper, the session file and the install records)
/// are denied to the Edit and Write tools and to sandboxed writes.
#[test]
fn launcher_files_are_denied_to_edits_and_sandboxed_writes() {
    const LAUNCHER_FILES: [&str; 5] = [
        ".codeflow/shell/claude.sh",
        ".codeflow/shell/claude-wrapper.sh",
        ".codeflow/claude/session.json",
        ".codeflow/claude/install.json",
        ".codeflow/claude-user-settings.json",
    ];
    for name in preset_files() {
        let value = load(&name);
        let deny = perm_array(&value, "deny");
        let writes: Vec<String> = value["sandbox"]["filesystem"]["denyWrite"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
        for file in LAUNCHER_FILES {
            assert!(
                deny.iter()
                    .filter_map(|rule| rule.strip_prefix("Edit(~/")?.strip_suffix(')'))
                    .any(|pattern| path_matches(pattern, file)),
                "{name}: no Edit deny covers ~/{file}"
            );
            assert!(
                writes.iter().any(|entry| entry
                    .strip_prefix("~/")
                    .is_some_and(|dir| file == dir || file.starts_with(&format!("{dir}/")))),
                "{name}: no sandbox write deny covers ~/{file}"
            );
        }
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
    codeflow_cmd(root, home, args)
        .output()
        .expect("codeflow runs")
}

/// [`run_codeflow`] with `input` on stdin, such as the init answers.
fn run_codeflow_with_input(
    root: &Path,
    home: &Path,
    args: &[&str],
    input: &str,
) -> std::process::Output {
    let mut child = codeflow_cmd(root, home, args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("codeflow starts");
    std::io::Write::write_all(&mut child.stdin.take().unwrap(), input.as_bytes()).unwrap();
    child.wait_with_output().expect("codeflow runs")
}

fn codeflow_cmd(root: &Path, home: &Path, args: &[&str]) -> std::process::Command {
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_codeflow"));
    let path = std::env::join_paths(exe.parent().map(Path::to_path_buf).into_iter().chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .expect("joinable PATH");
    let mut command = std::process::Command::new(&exe);
    command
        .args(args)
        .current_dir(root)
        .env("CODEFLOW_HOME", home)
        // Never the person's own Claude user settings (TSK-1070).
        .env("CLAUDE_CONFIG_DIR", home.join("claude-config"))
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
        .env_remove("CODEFLOW_HUMAN_OVERRIDE");
    command
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

/// TSK-1070 AC-13: `codeflow update` on a project that holds an earlier
/// bypass preset, with the adopter's own edits, removes the shipped
/// `defaultMode` Claude Code ignores at project scope, moves the shipped
/// status line, and keeps every edit of the adopter's. A mode the project
/// set itself, which the baseline does not hold, stays and is reported.
#[test]
fn update_retires_the_shipped_bypass_mode_and_keeps_the_adopters_edits() {
    for shipped_mode in [true, false] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("proj");
        let home = dir.path().join("home");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        // The adopter chose the bypass preset; the follow-up offer to write
        // the user file gets end of input, so nothing is written there.
        succeeded(
            &run_codeflow_with_input(
                &root,
                &home,
                &["init", "--minimal"],
                "proj\ncore\nbypassPermissions\n",
            ),
            "init --minimal",
        );
        assert!(!home.join("claude-config/settings.json").exists());
        let fixture = repo_root()
            .join("crates/codeflow-core/tests/fixtures/presets-2.1.0/bypass-sandboxed.json");
        let mut baseline: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(fixture).unwrap()).unwrap();
        let mut project = baseline.clone();
        if !shipped_mode {
            // The project chose the mode; the recorded baseline never held it.
            baseline["permissions"]
                .as_object_mut()
                .unwrap()
                .remove("defaultMode");
        }
        project["permissions"]["allow"]
            .as_array_mut()
            .unwrap()
            .push("Bash(make *)".into());
        project["env"] = serde_json::json!({"ADOPTER_FLAG": "1"});
        project["adopterKey"] = serde_json::json!(true);
        let plant = |rel: &str, value: &serde_json::Value| {
            let path = root.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, serde_json::to_string_pretty(value).unwrap() + "\n").unwrap();
        };
        plant(".claude/settings.json", &project);
        plant(".codeflow/.baseline/.claude/settings.json", &baseline);

        let report = succeeded(&run_codeflow(&root, &home, &["update"]), "update");
        let merged = read_json(&root.join(".claude/settings.json"));
        let allow = merged["permissions"]["allow"].as_array().unwrap();
        assert!(allow.iter().any(|e| e == "Bash(make *)"), "{shipped_mode}");
        assert_eq!(merged["env"]["ADOPTER_FLAG"], "1", "{shipped_mode}");
        assert_eq!(merged["adopterKey"], true, "{shipped_mode}");
        assert_eq!(
            merged["statusLine"],
            load("default.json")["statusLine"],
            "{shipped_mode}: the shipped status line moves"
        );
        // D1 to D3 reach the adopter too: no unsandboxed retry, the
        // excluded host-state tools, no bare secret-name deny.
        assert_eq!(merged["sandbox"]["allowUnsandboxedCommands"], false);
        assert_eq!(
            merged["sandbox"]["excludedCommands"],
            load("default.json")["sandbox"]["excludedCommands"]
        );
        let deny = perm_array(&merged, "deny");
        assert!(
            !deny.iter().any(|rule| rule == "Read(**/*secret*)"),
            "{report}"
        );
        assert!(deny.iter().any(|rule| rule == "Read(~/.config/gh/**)"));
        if shipped_mode {
            assert!(
                merged["permissions"].get("defaultMode").is_none(),
                "{merged}\n{report}"
            );
            assert!(
                report.contains("removed permissions.defaultMode \"bypassPermissions\""),
                "{report}"
            );
        } else {
            assert_eq!(merged["permissions"]["defaultMode"], "bypassPermissions");
            assert!(
                report.contains("kept the project's \"permissions.defaultMode\""),
                "{report}"
            );
        }
    }
}

/// TSK-1070 (operator decision 2026-10-31): `codeflow update` on a project
/// that still holds the shipped `*` network allowlist retires `*`, adds
/// every shipped development host and keeps the hosts the adopter added.
/// A project that removed `*` itself is not given it back.
#[test]
fn update_narrows_the_shipped_network_and_keeps_the_adopters_hosts() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("proj");
    let home = dir.path().join("home");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    succeeded(
        &run_codeflow(&root, &home, &["init", "--yes", "--minimal"]),
        "init --minimal",
    );
    // The previous release shipped `*`; the adopter added two hosts.
    let mut baseline = load("acceptEdits.json");
    baseline["sandbox"]["network"]["allowedDomains"] = serde_json::json!(["*"]);
    let mut project = baseline.clone();
    project["sandbox"]["network"]["allowedDomains"] =
        serde_json::json!(["*", "internal.example", "registry.example.org"]);
    let plant = |rel: &str, value: &serde_json::Value| {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, serde_json::to_string_pretty(value).unwrap() + "\n").unwrap();
    };
    plant(".claude/settings.json", &project);
    plant(".codeflow/.baseline/.claude/settings.json", &baseline);

    let report = succeeded(&run_codeflow(&root, &home, &["update"]), "update");
    let merged = read_json(&root.join(".claude/settings.json"));
    let domains = merged["sandbox"]["network"]["allowedDomains"]
        .as_array()
        .unwrap();
    assert!(!domains.iter().any(|d| d == "*"), "{merged}\n{report}");
    for host in SHIPPED_ALLOWED_DOMAINS {
        assert!(domains.iter().any(|d| d == host), "{host}\n{report}");
    }
    for host in ["internal.example", "registry.example.org"] {
        assert!(
            domains.iter().any(|d| d == host),
            "{host}: the adopter's host"
        );
    }
    assert!(
        report.contains("removed retired sandbox.network.allowedDomains entry"),
        "{report}"
    );
    let denied = merged["sandbox"]["network"]["deniedDomains"]
        .as_array()
        .unwrap();
    assert!(denied.iter().any(|d| d == "192.168.*"));
    assert_eq!(merged["sandbox"]["network"]["allowLocalBinding"], true);

    // A second update changes nothing.
    let again = succeeded(&run_codeflow(&root, &home, &["update"]), "update again");
    assert_eq!(
        read_json(&root.join(".claude/settings.json")),
        merged,
        "{again}"
    );
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
        assert_eq!(
            perm_array(&updated, "ask"),
            actions::table().claude_ask(),
            "{case}: the shipped asks are retired and the forge writes ask:\n{first}"
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
            first.contains("moved security.privilege_escalation to \"block\""),
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

// ---- The shipped status line (TSK-1028, issue 351) ----------------------

/// The status JSON Claude Code sends the status line on stdin, with the
/// context used set to `used` tokens of a 1M window.
#[cfg(unix)]
fn status_json(folder: &Path, used: u64) -> String {
    serde_json::json!({
        "model": {"id": "claude-opus-5-5", "display_name": "Opus 5.5"},
        "version": "2.3.1",
        "output_style": {"name": "Explanatory"},
        "workspace": {"current_dir": folder},
        "context_window": {
            "context_window_size": 1_000_000,
            "current_usage": {
                "input_tokens": used - 3000,
                "cache_read_input_tokens": 1000,
                "cache_creation_input_tokens": 1000,
                "output_tokens": 1000
            }
        }
    })
    .to_string()
}

/// A fresh folder under the system temporary directory, with a git
/// repository on branch `feature-x` when `git_branch` is set.
#[cfg(unix)]
fn status_folder(name: &str, git_branch: bool) -> PathBuf {
    let folder = std::env::temp_dir().join(format!("cf-status-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    if git_branch {
        let status = std::process::Command::new("git")
            .args(["init", "--quiet", "-b", "feature-x"])
            .current_dir(&folder)
            .status()
            .expect("git runs");
        assert!(status.success());
    }
    folder
}

/// Runs the preset's status line command in `folder` with `json` on stdin
/// and `path` as `PATH`; returns stdout and the exit code.
#[cfg(unix)]
fn run_status_line(
    preset: &str,
    folder: &Path,
    json: &str,
    path: &std::ffi::OsStr,
) -> (String, i32) {
    use std::io::Write;
    let value = load(preset);
    assert_eq!(value["statusLine"]["type"], "command", "{preset}");
    let command = value["statusLine"]["command"].as_str().unwrap();
    let mut child = std::process::Command::new("/bin/sh")
        .args(["-c", command])
        .current_dir(folder)
        .env("PATH", path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("sh runs");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(json.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.stderr.is_empty(),
        "{preset}: stderr {}",
        String::from_utf8_lossy(&output.stderr)
    );
    (
        String::from_utf8(output.stdout).unwrap(),
        output.status.code().unwrap_or(-1),
    )
}

/// The ANSI escape sequences stripped, leaving what a reader sees.
#[cfg(unix)]
fn visible(text: &str) -> String {
    regex::Regex::new("\u{1b}\\[[0-9;]*m")
        .unwrap()
        .replace_all(text, "")
        .into_owned()
}

/// AC-1: given Claude Code's status JSON, every preset's line shows the
/// model name and id, the version, the output style, the folder, the branch
/// when there is one, and the context used out of the window with a
/// percentage coloured green below 50, yellow below 80 and red from 80.
/// Needs `jq`, which the issue's line reads the fields with.
#[cfg(unix)]
#[test]
fn status_line_shows_model_version_style_folder_branch_and_context() {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let has_jq = std::env::split_paths(&path).any(|dir| dir.join("jq").is_file());
    assert!(
        has_jq,
        "this test needs jq on PATH, as the shipped line does"
    );
    let repo = status_folder("repo", true);
    let plain = status_folder("plain", false);
    for preset in preset_files() {
        for (used, colour, shown) in [
            (120_000, "32", "ctx:120k/1000k (12%)"),
            (550_000, "33", "ctx:550k/1000k (55%)"),
            (900_000, "31", "ctx:900k/1000k (90%)"),
        ] {
            let (out, code) = run_status_line(&preset, &repo, &status_json(&repo, used), &path);
            assert_eq!(code, 0, "{preset}");
            let folder = repo.file_name().unwrap().to_str().unwrap();
            assert_eq!(
                visible(&out),
                format!("Opus 5.5 (claude-opus-5-5) | v2.3.1 | Explanatory | {folder} (feature-x) | {shown}\n"),
                "{preset}"
            );
            assert!(
                out.contains(&format!("\u{1b}[2;{colour}mctx:")),
                "{preset}: {out:?}"
            );
        }
        let (out, code) = run_status_line(&preset, &plain, &status_json(&plain, 10_000), &path);
        assert_eq!(code, 0, "{preset}");
        let folder = plain.file_name().unwrap().to_str().unwrap();
        assert_eq!(
            visible(&out),
            format!("Opus 5.5 (claude-opus-5-5) | v2.3.1 | Explanatory | {folder} | ctx:10k/1000k (1%)\n"),
            "{preset}: no branch outside a repository"
        );
    }
    let _ = std::fs::remove_dir_all(repo);
    let _ = std::fs::remove_dir_all(plain);
}

/// AC-2: without `jq` on `PATH`, the line prints the folder and branch and
/// exits 0, never an error or an empty line.
#[cfg(unix)]
#[test]
fn status_line_without_jq_prints_folder_and_branch() {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let find = |tool: &str| {
        std::env::split_paths(&path)
            .map(|dir| dir.join(tool))
            .find(|candidate| candidate.is_file())
            .unwrap_or_else(|| panic!("{tool} not on PATH"))
    };
    let bin = status_folder("bin", false);
    for tool in ["cat", "basename", "git", "tr"] {
        std::os::unix::fs::symlink(find(tool), bin.join(tool)).unwrap();
    }
    let repo = status_folder("repo-nojq", true);
    let plain = status_folder("plain-nojq", false);
    for preset in preset_files() {
        let (out, code) =
            run_status_line(&preset, &repo, &status_json(&repo, 10_000), bin.as_os_str());
        assert_eq!(code, 0, "{preset}");
        let folder = repo.file_name().unwrap().to_str().unwrap();
        assert_eq!(
            out,
            format!("{folder} (feature-x) | jq not found\n"),
            "{preset}"
        );

        let (out, code) = run_status_line(
            &preset,
            &plain,
            &status_json(&plain, 10_000),
            bin.as_os_str(),
        );
        assert_eq!(code, 0, "{preset}");
        let folder = plain.file_name().unwrap().to_str().unwrap();
        assert_eq!(out, format!("{folder} | jq not found\n"), "{preset}");
    }
    for folder in [bin, repo, plain] {
        let _ = std::fs::remove_dir_all(folder);
    }
}

/// Review of PR 368: the line hands the status JSON to `jq` byte for byte,
/// so a backslash in a path or name, or an escaped control character, stays
/// literal under `/bin/sh` instead of turning into an escape or a parse
/// error. Since TSK-1070 SEC-01 a tab shows as a space and other control
/// characters are removed.
#[cfg(unix)]
#[test]
fn status_line_keeps_backslashes_and_escapes_literal() {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let slashed = status_folder("back\\slash", true);
    let windows = "C:\\Users\\dev\\proj";
    for preset in preset_files() {
        for (cwd, style, folder, branch) in [
            (
                slashed.to_str().unwrap().to_string(),
                "tab\there",
                slashed.file_name().unwrap().to_str().unwrap().to_string(),
                " (feature-x)",
            ),
            (
                windows.to_string(),
                "back\\slash\\nstyle",
                windows.to_string(),
                "",
            ),
        ] {
            let json = serde_json::json!({
                "model": {"id": "claude-opus-5-5", "display_name": "Opus 5.5"},
                "version": "2.3.1",
                "output_style": {"name": style},
                "workspace": {"current_dir": cwd},
                "context_window": {
                    "context_window_size": 1_000_000,
                    "current_usage": {
                        "input_tokens": 1000,
                        "cache_read_input_tokens": 0,
                        "cache_creation_input_tokens": 0,
                        "output_tokens": 0
                    }
                }
            })
            .to_string();
            let (out, code) = run_status_line(&preset, &slashed, &json, &path);
            assert_eq!(code, 0, "{preset}");
            assert_eq!(
                visible(&out),
                format!(
                    "Opus 5.5 (claude-opus-5-5) | v2.3.1 | {} | {folder}{branch} | ctx:1k/1000k (0%)\n",
                    style.replace('\t', " ")
                ),
                "{preset}"
            );
        }
    }
    let _ = std::fs::remove_dir_all(slashed);
}

// ---- The richer status line (TSK-1070, issue 402) -----------------------

/// TSK-1028's base status JSON with the given extra top-level fields merged
/// in; `worktree` goes into `workspace.git_worktree`.
#[cfg(unix)]
fn rich_json(folder: &Path, extra: &serde_json::Value, worktree: Option<&str>) -> String {
    let mut value: serde_json::Value = serde_json::from_str(&status_json(folder, 120_000)).unwrap();
    for (key, field) in extra.as_object().unwrap() {
        value[key] = field.clone();
    }
    if let Some(name) = worktree {
        value["workspace"]["git_worktree"] = serde_json::json!(name);
    }
    value.to_string()
}

/// AC-6's separator rule: no `null`, empty brackets or doubled separator.
#[cfg(unix)]
fn assert_clean(line: &str, case: &str) {
    for bad in ["null", "()", "[]", "| |", "||", "  "] {
        assert!(!line.contains(bad), "{case}: {bad:?} in {line:?}");
    }
}

#[cfg(unix)]
fn has_jq(path: &std::ffi::OsStr) -> bool {
    std::env::split_paths(path).any(|dir| dir.join("jq").is_file())
}

/// The visible line for `json`, from every preset, asserted identical.
#[cfg(unix)]
fn line_for(folder: &Path, json: &str, path: &std::ffi::OsStr) -> String {
    let mut seen: Option<String> = None;
    for preset in preset_files() {
        let (out, code) = run_status_line(&preset, folder, json, path);
        assert_eq!(code, 0, "{preset}");
        let out = visible(&out);
        if let Some(first) = &seen {
            assert_eq!(&out, first, "{preset} differs from the other presets");
        }
        seen = Some(out);
    }
    seen.unwrap()
}

/// AC-2: the effort prints beside the model at each documented level, and
/// nothing prints for it when `effort` is absent.
#[cfg(unix)]
#[test]
fn status_line_shows_effort_beside_the_model() {
    let path = std::env::var_os("PATH").unwrap_or_default();
    assert!(has_jq(&path), "this test needs jq on PATH");
    let repo = status_folder("levels", true);
    for level in ["low", "medium", "high", "xhigh", "max"] {
        let json = rich_json(
            &repo,
            &serde_json::json!({"effort": {"level": level}}),
            None,
        );
        let line = line_for(&repo, &json, &path);
        assert!(
            line.starts_with(&format!("Opus 5.5 (claude-opus-5-5) · {level} | v2.3.1")),
            "{line}"
        );
        assert_clean(&line, level);
    }
    let line = line_for(
        &repo,
        &rich_json(&repo, &serde_json::json!({}), None),
        &path,
    );
    assert!(
        line.starts_with("Opus 5.5 (claude-opus-5-5) | v2.3.1"),
        "{line}"
    );
    let _ = std::fs::remove_dir_all(repo);
}

/// AC-3: the pull request number and review state, the number alone, and
/// nothing when `pr` is absent.
#[cfg(unix)]
#[test]
fn status_line_shows_the_pull_request_and_its_review_state() {
    let path = std::env::var_os("PATH").unwrap_or_default();
    assert!(has_jq(&path), "this test needs jq on PATH");
    let repo = status_folder("pr", true);
    for (state, shown) in [
        ("approved", "approved"),
        ("pending", "pending"),
        ("changes_requested", "changes requested"),
        ("draft", "draft"),
    ] {
        let json = rich_json(
            &repo,
            &serde_json::json!({"pr": {
                "number": 451,
                "url": "https://example.invalid/451",
                "review_state": state
            }}),
            None,
        );
        let line = line_for(&repo, &json, &path);
        assert!(
            line.contains(&format!(" | PR 451 {shown} | ctx:")),
            "{line}"
        );
        assert_clean(&line, state);
    }
    let json = rich_json(&repo, &serde_json::json!({"pr": {"number": 7}}), None);
    let line = line_for(&repo, &json, &path);
    assert!(line.contains(" | PR 7 | ctx:"), "{line}");
    assert_clean(&line, "number only");
    let line = line_for(
        &repo,
        &rich_json(&repo, &serde_json::json!({}), None),
        &path,
    );
    assert!(!line.contains("PR"), "{line}");
    let _ = std::fs::remove_dir_all(repo);
}

/// AC-4: each usage window with its percentage, either alone, none, a
/// fractional value, 100, and a present 0, which prints.
#[cfg(unix)]
#[test]
fn status_line_shows_plan_usage_windows() {
    let path = std::env::var_os("PATH").unwrap_or_default();
    assert!(has_jq(&path), "this test needs jq on PATH");
    let repo = status_folder("usage", true);
    let window = |pct: serde_json::Value| serde_json::json!({"used_percentage": pct, "resets_at": 1_738_425_600});
    for (limits, shown) in [
        (
            serde_json::json!({"five_hour": window(23.5.into()), "seven_day": window(41.into())}),
            Some("| 5h 24% · 7d 41%"),
        ),
        (
            serde_json::json!({"five_hour": window(100.into())}),
            Some("| 5h 100%"),
        ),
        (
            serde_json::json!({"seven_day": window(0.into())}),
            Some("| 7d 0%"),
        ),
        (serde_json::json!({}), None),
    ] {
        let json = rich_json(&repo, &serde_json::json!({"rate_limits": limits}), None);
        let line = line_for(&repo, &json, &path);
        match shown {
            Some(text) => assert!(line.trim_end().ends_with(text), "{line}"),
            None => assert!(line.trim_end().ends_with("(12%)"), "{line}"),
        }
        assert_clean(&line, &limits.to_string());
    }
    let line = line_for(
        &repo,
        &rich_json(&repo, &serde_json::json!({}), None),
        &path,
    );
    assert!(!line.contains("5h ") && !line.contains("7d "), "{line}");
    let _ = std::fs::remove_dir_all(repo);
}

/// AC-5: the linked worktree's name, and nothing for it in the main tree.
#[cfg(unix)]
#[test]
fn status_line_shows_the_linked_worktree() {
    let path = std::env::var_os("PATH").unwrap_or_default();
    assert!(has_jq(&path), "this test needs jq on PATH");
    let repo = status_folder("worktree", true);
    let folder = repo.file_name().unwrap().to_str().unwrap().to_string();
    let json = rich_json(&repo, &serde_json::json!({}), Some("tsk-1070"));
    let line = line_for(&repo, &json, &path);
    assert!(
        line.contains(&format!("| {folder} (feature-x) · wt:tsk-1070 | ctx:")),
        "{line}"
    );
    let line = line_for(
        &repo,
        &rich_json(&repo, &serde_json::json!({}), None),
        &path,
    );
    assert!(!line.contains("wt:"), "{line}");
    let _ = std::fs::remove_dir_all(repo);
}

/// AC-6: with none of the new fields the line is TSK-1028's exactly, and
/// all sixteen combinations of the four groups keep the separator rule.
#[cfg(unix)]
#[test]
fn status_line_keeps_the_existing_text_and_clean_separators() {
    let path = std::env::var_os("PATH").unwrap_or_default();
    assert!(has_jq(&path), "this test needs jq on PATH");
    let repo = status_folder("combos", true);
    let folder = repo.file_name().unwrap().to_str().unwrap().to_string();
    let base = line_for(
        &repo,
        &rich_json(&repo, &serde_json::json!({}), None),
        &path,
    );
    assert_eq!(
        base,
        format!(
            "Opus 5.5 (claude-opus-5-5) | v2.3.1 | Explanatory | {folder} (feature-x) | ctx:120k/1000k (12%)\n"
        )
    );
    for mask in 0u8..16 {
        let mut extra = serde_json::Map::new();
        if mask & 1 != 0 {
            extra.insert("effort".into(), serde_json::json!({"level": "high"}));
        }
        if mask & 2 != 0 {
            extra.insert(
                "pr".into(),
                serde_json::json!({"number": 12, "review_state": "pending"}),
            );
        }
        if mask & 4 != 0 {
            extra.insert(
                "rate_limits".into(),
                serde_json::json!({
                    "five_hour": {"used_percentage": 23.5},
                    "seven_day": {"used_percentage": 81}
                }),
            );
        }
        let worktree = (mask & 8 != 0).then_some("wt");
        let json = rich_json(&repo, &serde_json::Value::Object(extra), worktree);
        let line = line_for(&repo, &json, &path);
        assert_clean(&line, &format!("mask {mask}"));
        assert_eq!(
            line.contains("(claude-opus-5-5) · high |"),
            mask & 1 != 0,
            "{mask}: {line}"
        );
        assert_eq!(
            line.contains("| PR 12 pending |"),
            mask & 2 != 0,
            "{mask}: {line}"
        );
        assert_eq!(
            line.contains("| 5h 24% · 7d 81%"),
            mask & 4 != 0,
            "{mask}: {line}"
        );
        assert_eq!(line.contains("· wt:wt |"), mask & 8 != 0, "{mask}: {line}");
        assert!(
            line.ends_with('\n') && line.matches('\n').count() == 1,
            "{mask}: {line:?}"
        );
    }
    let _ = std::fs::remove_dir_all(repo);
}

/// AC-7: without `jq` the line still prints the folder and branch, whatever
/// new fields the status JSON carries.
#[cfg(unix)]
#[test]
fn status_line_without_jq_ignores_the_new_fields() {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let find = |tool: &str| {
        std::env::split_paths(&path)
            .map(|dir| dir.join(tool))
            .find(|candidate| candidate.is_file())
            .unwrap_or_else(|| panic!("{tool} not on PATH"))
    };
    let bin = status_folder("bin-rich", false);
    for tool in ["cat", "basename", "git", "tr"] {
        std::os::unix::fs::symlink(find(tool), bin.join(tool)).unwrap();
    }
    let repo = status_folder("rich-nojq", true);
    let folder = repo.file_name().unwrap().to_str().unwrap().to_string();
    let json = rich_json(
        &repo,
        &serde_json::json!({
            "effort": {"level": "max"},
            "pr": {"number": 3, "review_state": "draft"},
            "rate_limits": {"five_hour": {"used_percentage": 50}}
        }),
        Some("wt"),
    );
    for preset in preset_files() {
        let (out, code) = run_status_line(&preset, &repo, &json, bin.as_os_str());
        assert_eq!(code, 0, "{preset}");
        assert_eq!(
            out,
            format!("{folder} (feature-x) | jq not found\n"),
            "{preset}"
        );
    }
    for folder in [bin, repo] {
        let _ = std::fs::remove_dir_all(folder);
    }
}

/// Claude Code reads a rule ending in `:*` as its old prefix form and the
/// `*` before it literally, so `Bash(git push *ext::*)` denied nothing; it
/// warned "reads its * literally" and let `ext::` through (TSK-1070 live
/// canary on 2.1.316). `*ext::**` matches. No shipped rule may end so.
#[test]
fn no_shipped_permission_rule_ends_in_the_legacy_prefix_form() {
    for name in preset_files() {
        let preset = load(&name);
        for key in ["allow", "deny", "ask"] {
            for rule in preset["permissions"][key].as_array().into_iter().flatten() {
                let rule = rule.as_str().unwrap_or_default();
                if let Some(head) = rule.strip_suffix(":*)") {
                    assert!(
                        !head.contains('*'),
                        "{name}: {key} rule {rule} ends in `:*` with another `*`, which Claude Code reads literally"
                    );
                }
            }
        }
    }
}

/// Runs `command` under `shell -c` in `folder` with `json` on stdin; returns
/// stdout and stderr.
#[cfg(unix)]
fn run_under(shell: &str, command: &str, folder: &Path, json: &str) -> (String, String) {
    use std::io::Write;
    let mut child = std::process::Command::new(shell)
        .args(["-c", command])
        .current_dir(folder)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("shell runs");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(json.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// TSK-1070 SEC-01: no status JSON field is ever evaluated by the shell.
/// A folder name carrying the field separator and a command substitution
/// in arithmetic position (the security review's canary) prints as plain
/// text under `/bin/sh` and Bash in every preset, control characters and
/// all other text from the JSON are stripped, and a non-numeric, negative
/// or zero count falls back to safe numbers instead of reaching the shell.
#[cfg(unix)]
#[test]
fn status_line_never_evaluates_status_text() {
    let path = std::env::var_os("PATH").unwrap_or_default();
    assert!(has_jq(&path), "this test needs jq on PATH");
    let repo = status_folder("inject", true);
    let marker = repo.join("CODEFLOW_ARITH_CANARY");
    let payload = format!(
        "/tmp/x\u{1f}1\u{1f}2\u{1f}3\u{1f}4\u{1f}5\u{1f}6\u{1f}a[$(printf CODEFLOW_ARITH_CANARY >&2; touch {})0]",
        marker.display()
    );
    let hostile = serde_json::json!({
        "model": {"id": "id\u{1b}[31m$(id)", "display_name": "Opus\u{7}\n5.5"},
        "version": "`touch x`",
        "output_style": {"name": "a\u{1f}b"},
        "workspace": {"current_dir": payload, "git_worktree": "wt\u{1b}]0;t\u{7}"},
        "effort": {"level": "$((1+1))"},
        "pr": {"number": "1$(id)", "review_state": "x\u{9b}31m"},
        "rate_limits": {"five_hour": {"used_percentage": "a[$(id)]"}, "seven_day": {"used_percentage": -5}},
        "context_window": {
            "context_window_size": "x[$(id)]",
            "current_usage": {"input_tokens": "a[$(id)0]", "cache_read_input_tokens": -1,
                "cache_creation_input_tokens": 1e300, "output_tokens": 2000}
        }
    })
    .to_string();
    for shell in ["/bin/sh", "/bin/bash"] {
        if !Path::new(shell).is_file() {
            continue;
        }
        for preset in preset_files() {
            let command = load(&preset)["statusLine"]["command"]
                .as_str()
                .unwrap()
                .to_string();
            let (out, err) = run_under(shell, &command, &repo, &hostile);
            assert!(
                !err.contains("CODEFLOW_ARITH_CANARY"),
                "{shell} {preset}: {err}"
            );
            assert!(err.is_empty(), "{shell} {preset}: stderr {err:?}");
            assert!(!marker.exists(), "{shell} {preset}: the payload ran");
            let shown = visible(&out);
            assert!(
                shown.ends_with('\n') && shown.matches('\n').count() == 1,
                "{shell} {preset}: {shown:?}"
            );
            for byte in shown.trim_end_matches('\n').chars() {
                assert!(
                    !byte.is_control(),
                    "{shell} {preset}: control {byte:?} in {shown:?}"
                );
            }
            assert!(
                shown.contains("ctx:2k/200k (1%)"),
                "{shell} {preset}: {shown:?}"
            );
            assert!(
                !shown.contains("5h ") && !shown.contains("7d "),
                "{shell} {preset}: {shown:?}"
            );
            assert!(shown.contains("wt:wt]0;t"), "{shell} {preset}: {shown:?}");
        }
    }
    let _ = std::fs::remove_dir_all(repo);
}

/// TSK-1070 SEC-01: without jq the fallback prints the folder with its
/// control characters removed.
#[cfg(unix)]
#[test]
fn status_line_fallback_strips_control_characters() {
    let base = status_folder("fallback", false);
    let folder = base.join("a\u{1b}[31mb\u{7}c");
    std::fs::create_dir_all(&folder).unwrap();
    let bin = base.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    for tool in ["cat", "basename", "git", "tr", "printf"] {
        let found = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .map(|dir| dir.join(tool))
            .find(|p| p.is_file());
        if let Some(found) = found {
            let _ = std::os::unix::fs::symlink(found, bin.join(tool));
        }
    }
    for preset in preset_files() {
        let (out, _) = run_status_line(&preset, &folder, "{}", bin.as_os_str());
        assert_eq!(out, "a[31mbc | jq not found\n", "{preset}");
    }
    let _ = std::fs::remove_dir_all(base);
}
