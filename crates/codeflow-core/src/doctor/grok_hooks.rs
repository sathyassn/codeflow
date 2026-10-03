//! Whether Grok can run the `CodeFlow` hooks it loads (TSK-215, issue 29).
//!
//! Grok expands `$name` and `${...}` in a hook command itself and skips the
//! hook, failing open, when a name is unset. A `CodeFlow` command that carries
//! a shell variable therefore never runs in a Grok session. Grok discovers
//! project hooks in `.grok/hooks/*.json` and, with its Claude compatibility
//! on (the default), in `.claude/settings.json` and
//! `.claude/settings.local.json`.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::settings::is_codeflow_command;

/// The tool call the canary sends: a `PreToolUse` payload in the shape Grok
/// Build 1.0.46 sends (every field under both spellings, captured from a
/// live session) for a shell command that the non-relaxable
/// dangerous-command floor refuses under any policy. The guard only reads
/// it; nothing runs the command.
pub(super) const CANARY_PAYLOAD: &str = concat!(
    r#"{"hookEventName":"pre_tool_use","toolName":"run_terminal_command","#,
    r#""toolInput":{"command":"rm -rf /","description":"codeflow doctor canary"},"#,
    r#""hook_event_name":"PreToolUse","tool_name":"run_terminal_command","#,
    r#""tool_input":{"command":"rm -rf /","description":"codeflow doctor canary"}}"#
);

/// Whether `codeflow update` repairs the hook file at `path` (relative to
/// `root`): the installed manifest lists it as managed, whole or by region.
/// A file the manifest does not list, or a manifest that does not read, is
/// the adopter's to fix by hand.
pub(super) fn update_manages(root: &Path, path: &str) -> bool {
    std::fs::read_to_string(root.join(".codeflow").join("manifest.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|manifest| {
            manifest
                .get("files")?
                .get(path)?
                .get("ownership")?
                .as_str()
                .map(|ownership| ownership.starts_with("managed"))
        })
        .unwrap_or(false)
}

/// Where a `PATH` value resolves `codeflow`: the first entry holding an
/// executable `codeflow`, canonical (symlinks resolved). A relative entry
/// is read against `root`, where grok runs project hooks. Doctor reports
/// this; it never runs it.
pub(super) fn path_codeflow(path: &std::ffi::OsStr, root: &Path) -> Option<PathBuf> {
    std::env::split_paths(path).find_map(|entry| {
        let entry = if entry.as_os_str().is_empty() {
            root.to_path_buf()
        } else if entry.is_relative() {
            root.join(entry)
        } else {
            entry
        };
        let candidate = entry.join(if cfg!(windows) {
            "codeflow.exe"
        } else {
            "codeflow"
        });
        let canonical = std::fs::canonicalize(&candidate).ok()?;
        is_executable(&canonical).then_some(canonical)
    })
}

fn is_executable(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        meta.is_file() && meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        meta.is_file()
    }
}

/// The files Grok reads project hooks from, in a stable order.
fn hook_files(root: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(root.join(".grok").join("hooks"))
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| {
                    path.extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
                })
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    for name in ["settings.json", "settings.local.json"] {
        let path = root.join(".claude").join(name);
        if path.is_file() {
            files.push(path);
        }
    }
    files
}

/// Every `CodeFlow` command in a hooks value, with the event it is bound to.
fn codeflow_commands(hooks: &Value) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let Some(events) = hooks.as_object() else {
        return out;
    };
    for (event, groups) in events {
        collect(groups, event, &mut out);
    }
    out
}

fn collect(value: &Value, event: &str, out: &mut Vec<(String, String)>) {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(command)) = map.get("command") {
                if is_codeflow_command(command) {
                    out.push((event.to_string(), command.clone()));
                }
            }
            for child in map.values() {
                collect(child, event, out);
            }
        }
        Value::Array(items) => {
            for child in items {
                collect(child, event, out);
            }
        }
        _ => {}
    }
}

/// The parsed `hooks` object of each hook file Grok reads, with its path
/// relative to `root`. A file that does not parse is left to the checks
/// that own its format.
fn parsed(root: &Path) -> Vec<(String, Value)> {
    hook_files(root)
        .into_iter()
        .filter_map(|path| {
            let text = std::fs::read_to_string(&path).ok()?;
            let value: Value = serde_json::from_str(&text).ok()?;
            let hooks = value.get("hooks")?.clone();
            let shown = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .components()
                .map(|part| part.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            Some((shown, hooks))
        })
        .collect()
}

/// The hook files holding a `CodeFlow` command Grok would skip because it
/// carries a `$`.
pub(super) fn templated(root: &Path) -> Vec<String> {
    parsed(root)
        .into_iter()
        .filter(|(_, hooks)| {
            codeflow_commands(hooks)
                .iter()
                .any(|(_, command)| command.contains('$'))
        })
        .map(|(path, _)| path)
        .collect()
}

/// Grok's shell tool.
const SHELL_TOOL: &str = "run_terminal_command";

/// The Claude names Grok maps to its shell tool (`xai-grok-tools`
/// `claude_alias.rs`: `Bash` names `run_terminal_command`; `PowerShell`
/// names no Grok tool).
const SHELL_TOOL_ALIASES: [&str; 1] = ["Bash"];

/// The hook files `CodeFlow` ships, whose exec-guard commands are the forms
/// doctor recognises.
const SHIPPED_HOOK_FILES: [&str; 5] = [
    include_str!("../../../../assets/base/grok/hooks.json"),
    include_str!("../../../../assets/base/codex/hooks.json"),
    include_str!("../../../../assets/base/settings/default.json"),
    include_str!("../../../../assets/base/settings/acceptEdits.json"),
    include_str!("../../../../assets/base/settings/bypass-sandboxed.json"),
];

/// What decides how Grok runs a command handler: its command, its timeout
/// and its extra environment (`xai-grok-hooks` `RawHandler`). An absent,
/// `null` or empty `env` adds nothing.
#[derive(Debug, PartialEq)]
struct Handler {
    command: String,
    timeout: Option<Value>,
    env: Option<Value>,
}

impl Handler {
    fn read(handler: &Value) -> Option<Self> {
        if handler.get("type").and_then(Value::as_str) != Some("command") {
            return None;
        }
        let command = handler.get("command").and_then(Value::as_str)?.to_string();
        let env = handler
            .get("env")
            .filter(|env| !env.is_null() && env.as_object().is_none_or(|map| !map.is_empty()))
            .cloned();
        Some(Self {
            command,
            timeout: handler.get("timeout").cloned(),
            env,
        })
    }
}

/// The exec-guard handlers `CodeFlow` ships, exactly as shipped.
fn shipped_exec_guard_handlers() -> Vec<Handler> {
    SHIPPED_HOOK_FILES
        .iter()
        .filter_map(|text| serde_json::from_str::<Value>(text).ok())
        .flat_map(|value| shell_guard_handlers(value.get("hooks").unwrap_or(&Value::Null)))
        .collect()
}

fn is_exec_guard(command: &str) -> bool {
    is_codeflow_command(command) && command.contains("hook exec-guard")
}

/// Whether a `PreToolUse` matcher selects Grok's shell tool, as Grok
/// matches it (`xai-grok-hooks` `matcher.rs`): no matcher, an empty one or
/// `*` selects every tool; a simple name or `|` list (ASCII letters,
/// digits, `_` and `|` only) matches exactly, each term also naming the
/// Grok tools it aliases; anything else is an unanchored regular
/// expression tried on the tool name and its Claude aliases, and one that
/// does not compile matches nothing.
fn selects_shell(matcher: Option<&Value>) -> bool {
    let matcher = match matcher {
        None | Some(Value::Null) => return true,
        Some(Value::String(matcher)) => matcher,
        Some(_) => return false,
    };
    if matcher.is_empty() || matcher == "*" {
        return true;
    }
    let simple = matcher
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'|');
    if simple {
        return matcher
            .split('|')
            .any(|term| term == SHELL_TOOL || SHELL_TOOL_ALIASES.contains(&term));
    }
    regex::Regex::new(matcher).is_ok_and(|re| {
        re.is_match(SHELL_TOOL) || SHELL_TOOL_ALIASES.iter().any(|alias| re.is_match(alias))
    })
}

/// The `CodeFlow` exec-guard handlers Grok runs before its shell tool in
/// one hooks value: command handlers in `PreToolUse` groups whose matcher
/// selects the shell tool.
fn shell_guard_handlers(hooks: &Value) -> Vec<Handler> {
    let Some(groups) = hooks.get("PreToolUse").and_then(Value::as_array) else {
        return Vec::new();
    };
    groups
        .iter()
        .filter(|group| selects_shell(group.get("matcher")))
        .filter_map(|group| group.get("hooks").and_then(Value::as_array))
        .flatten()
        .filter_map(Handler::read)
        .filter(|handler| is_exec_guard(&handler.command))
        .collect()
}

/// The exec-guard Grok runs before a shell command, as doctor can know it
/// without running anything from the repository.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum ShellGuard {
    /// A shipped exec-guard handler, its command, timeout and environment
    /// as shipped, is bound where the shell tool hits it.
    Shipped,
    /// Only customised exec-guard handlers (another command, timeout or
    /// environment) are bound there; the files that hold them. Doctor never
    /// runs repository hook text or environment, so these stay unverified.
    Customised(Vec<String>),
    /// No `CodeFlow` exec-guard is bound where the shell tool hits it.
    Missing,
}

/// Which exec-guard Grok would run before its shell tool, across every
/// hook file it reads.
pub(super) fn shell_guard(root: &Path) -> ShellGuard {
    let shipped = shipped_exec_guard_handlers();
    let mut customised = Vec::new();
    for (path, hooks) in parsed(root) {
        let handlers = shell_guard_handlers(&hooks);
        if handlers.iter().any(|handler| shipped.contains(handler)) {
            return ShellGuard::Shipped;
        }
        if !handlers.is_empty() {
            customised.push(path);
        }
    }
    if customised.is_empty() {
        ShellGuard::Missing
    } else {
        ShellGuard::Customised(customised)
    }
}

/// Whether a guard's stdout is the deny answer Grok honours: a JSON object
/// whose `decision` is `deny` with a nonempty `reason`.
pub(super) fn is_deny_answer(stdout: &str) -> bool {
    serde_json::from_str::<Value>(stdout.trim()).is_ok_and(|value| {
        value.get("decision").and_then(Value::as_str) == Some("deny")
            && value
                .get("reason")
                .and_then(Value::as_str)
                .is_some_and(|reason| !reason.trim().is_empty())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (path, text) in files {
            let path = dir.path().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        dir
    }

    const SHIPPED: &str = include_str!("../../../../assets/base/grok/hooks.json");
    const TEMPLATED: &str = r#"{"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":"codeflow hook exec-guard --contract 3; codeflow_status=$?; exit 2"}]}]}}"#;

    #[test]
    fn the_shipped_grok_hooks_carry_no_template_and_bind_the_shell_guard() {
        let dir = project(&[(".grok/hooks/codeflow.json", SHIPPED)]);
        assert!(templated(dir.path()).is_empty());
        assert_eq!(shell_guard(dir.path()), ShellGuard::Shipped);
    }

    #[test]
    fn every_shipped_hook_file_binds_a_recognised_shell_guard() {
        for text in SHIPPED_HOOK_FILES {
            let hooks: Value = serde_json::from_str(text).unwrap();
            let handlers = shell_guard_handlers(&hooks["hooks"]);
            assert!(!handlers.is_empty(), "{text}");
            assert!(handlers.iter().all(|handler| handler
                .command
                .starts_with("codeflow hook exec-guard --contract 3 || ")
                && handler.env.is_none()));
        }
    }

    /// PR 35 review round 2, finding 1: where PATH resolves `codeflow`,
    /// canonical, through a symlink, and from a relative entry read against
    /// the project root; a file that is not executable is passed over.
    #[cfg(unix)]
    #[test]
    fn path_resolves_codeflow_canonically() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        let real = dir.path().join("real");
        let bin = root.join("bin");
        let links = dir.path().join("links");
        let plain = dir.path().join("plain");
        for folder in [&real, &bin, &links, &plain] {
            std::fs::create_dir_all(folder).unwrap();
        }
        let exe = |path: &Path| {
            std::fs::write(path, "#!/bin/sh\n").unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        };
        exe(&real.join("codeflow"));
        exe(&bin.join("codeflow"));
        std::fs::write(plain.join("codeflow"), "not executable").unwrap();
        std::os::unix::fs::symlink(real.join("codeflow"), links.join("codeflow")).unwrap();
        let canonical = |path: &Path| std::fs::canonicalize(path).unwrap();
        let path = |entries: &[&Path]| std::env::join_paths(entries).unwrap();

        assert_eq!(
            path_codeflow(&path(&[&real]), &root),
            Some(canonical(&real.join("codeflow")))
        );
        assert_eq!(
            path_codeflow(&path(&[&links]), &root),
            Some(canonical(&real.join("codeflow")))
        );
        assert_eq!(
            path_codeflow(&path(&[Path::new("bin"), &real]), &root),
            Some(canonical(&bin.join("codeflow")))
        );
        assert_eq!(
            path_codeflow(&path(&[&plain, &real]), &root),
            Some(canonical(&real.join("codeflow")))
        );
        assert_eq!(path_codeflow(&path(&[&plain]), &root), None);
    }

    #[test]
    fn update_manages_only_what_the_manifest_lists_as_managed() {
        let manifest = r#"{"files":{".grok/hooks/codeflow.json":{"ownership":"managed"},".claude/settings.json":{"ownership":"managed-region"},".grok/sandbox.toml":{"ownership":"user-owned"}}}"#;
        let dir = project(&[(".codeflow/manifest.json", manifest)]);
        assert!(update_manages(dir.path(), ".grok/hooks/codeflow.json"));
        assert!(update_manages(dir.path(), ".claude/settings.json"));
        assert!(!update_manages(dir.path(), ".grok/sandbox.toml"));
        assert!(!update_manages(dir.path(), ".grok/hooks/custom.json"));
        assert!(!update_manages(dir.path(), ".claude/settings.local.json"));
        let none = project(&[]);
        assert!(!update_manages(none.path(), ".grok/hooks/codeflow.json"));
    }

    /// Grok's matcher semantics (`xai-grok-hooks` `matcher.rs`): empty or
    /// `*` matches every tool; a simple name or `|` list matches exactly,
    /// each term also naming the Grok tools it aliases (`Bash` names
    /// `run_terminal_command`); anything else is an unanchored regex tried
    /// on the tool name and its Claude aliases; an invalid regex matches
    /// nothing.
    #[test]
    fn a_matcher_selects_the_shell_as_grok_matches_it() {
        for matcher in [
            Value::Null,
            Value::from(""),
            Value::from("*"),
            Value::from("Bash"),
            Value::from("run_terminal_command"),
            Value::from("Read|Bash"),
            Value::from("|Bash|"),
            Value::from("^(Bash|PowerShell)$"),
            Value::from("^Bas"),
            Value::from("terminal.command"),
            Value::from("run_.*"),
            Value::from("^Bash$"),
        ] {
            assert!(selects_shell(Some(&matcher)), "{matcher}");
        }
        assert!(selects_shell(None));
        for matcher in [
            Value::from("^Read$"),
            Value::from("Bas"),
            Value::from("terminal_"),
            Value::from("PowerShell"),
            Value::from("Edit|Write"),
            Value::from("run_terminal_command_v2"),
            Value::from("^Bas$"),
            Value::from("("),
            Value::from(1),
        ] {
            assert!(!selects_shell(Some(&matcher)), "{matcher}");
        }
    }

    /// PR 35 review round 2, finding 2: Grok applies a handler's `env` and
    /// `timeout` when it runs the command, so a shipped command under other
    /// metadata is a customised handler doctor does not run.
    #[test]
    fn a_shipped_command_under_other_handler_metadata_is_customised() {
        let exec_line = SHIPPED
            .lines()
            .find(|line| line.contains("hook exec-guard"))
            .unwrap();
        let with = |metadata: &str| {
            SHIPPED.replace(exec_line, &exec_line.replace(r#""timeout": 10"#, metadata))
        };
        for metadata in [
            r#""timeout": 10, "env": { "PATH": "/tmp/elsewhere" }"#,
            r#""timeout": 1"#,
            r#""timeout": 0"#,
        ] {
            let edited = with(metadata);
            assert_ne!(edited, SHIPPED);
            let dir = project(&[(".grok/hooks/codeflow.json", &edited)]);
            assert_eq!(
                shell_guard(dir.path()),
                ShellGuard::Customised(vec![".grok/hooks/codeflow.json".into()]),
                "{metadata}"
            );
        }
        for metadata in [
            r#""timeout": 10, "env": {}"#,
            r#""timeout": 10, "env": null"#,
        ] {
            let dir = project(&[(".grok/hooks/codeflow.json", &with(metadata))]);
            assert_eq!(shell_guard(dir.path()), ShellGuard::Shipped, "{metadata}");
        }
    }

    #[test]
    fn a_customised_or_misbound_guard_is_not_the_shipped_one() {
        let customised = SHIPPED.replace(
            "codeflow hook exec-guard --contract 3 || ",
            "codeflow hook exec-guard --contract 3; touch /tmp/x || ",
        );
        let dir = project(&[(".grok/hooks/codeflow.json", &customised)]);
        assert_eq!(
            shell_guard(dir.path()),
            ShellGuard::Customised(vec![".grok/hooks/codeflow.json".into()])
        );
        let read_only = SHIPPED.replace("^(Bash|PowerShell)$", "^Read$");
        let dir = project(&[(".grok/hooks/codeflow.json", &read_only)]);
        assert_eq!(shell_guard(dir.path()), ShellGuard::Missing);
        let prompt = SHIPPED.replace(r#""type": "command""#, r#""type": "prompt""#);
        let dir = project(&[(".grok/hooks/codeflow.json", &prompt)]);
        assert_eq!(shell_guard(dir.path()), ShellGuard::Missing);
    }

    #[test]
    fn only_a_json_deny_with_a_reason_is_a_deny_answer() {
        assert!(is_deny_answer(r#"{"decision":"deny","reason":"blocked"}"#));
        assert!(is_deny_answer(
            "{\"decision\":\"deny\",\"reason\":\"blocked\"}\n"
        ));
        for stdout in [
            "",
            "codeflow exec-guard: BLOCKED",
            r#"{"decision":"allow","reason":"x"}"#,
            r#"{"decision":"deny","reason":" "}"#,
            r#"{"decision":"deny"}"#,
        ] {
            assert!(!is_deny_answer(stdout), "{stdout}");
        }
    }

    #[test]
    fn a_dollar_in_a_codeflow_command_is_named_in_every_file_grok_reads() {
        let dir = project(&[
            (".grok/hooks/codeflow.json", TEMPLATED),
            (".claude/settings.json", TEMPLATED),
            (".claude/settings.local.json", TEMPLATED),
        ]);
        assert_eq!(
            templated(dir.path()),
            [
                ".grok/hooks/codeflow.json",
                ".claude/settings.json",
                ".claude/settings.local.json"
            ]
        );
    }

    #[test]
    fn only_codeflow_hook_commands_count() {
        let other = r#"{"$schema":"x","statusLine":{"type":"command","command":"echo $(pwd)"},"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":"my-tool $HOME"}]}]}}"#;
        let dir = project(&[
            (".grok/hooks/codeflow.json", SHIPPED),
            (".claude/settings.json", other),
        ]);
        assert!(templated(dir.path()).is_empty());
    }

    #[test]
    fn the_canary_payload_is_the_shape_grok_sends_and_the_guard_reads() {
        let value: Value = serde_json::from_str(CANARY_PAYLOAD).unwrap();
        assert_eq!(value["toolName"], value["tool_name"]);
        assert_eq!(value["toolInput"], value["tool_input"]);
        let payload = crate::hooks::git_guard::HookPayload::parse(CANARY_PAYLOAD).unwrap();
        assert_eq!(payload.shell_command(), Some("rm -rf /"));
    }

    #[test]
    fn no_exec_guard_on_pre_tool_use_means_no_shell_guard() {
        let elsewhere = r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"codeflow hook exec-guard --contract 3"}]}]}}"#;
        let dir = project(&[(".grok/hooks/codeflow.json", elsewhere)]);
        assert_eq!(shell_guard(dir.path()), ShellGuard::Missing);
        let dir = project(&[(".grok/hooks/codeflow.json", "{}")]);
        assert_eq!(shell_guard(dir.path()), ShellGuard::Missing);
    }
}
