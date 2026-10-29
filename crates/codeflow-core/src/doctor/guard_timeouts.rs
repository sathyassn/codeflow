//! Whether every `CodeFlow` guard hook entry a harness reads carries the
//! bounds that make a stalled guard block (TSK-1082).
//!
//! Claude Code, Codex and Grok each let a `PreToolUse` tool call through
//! when a command hook times out. The shipped entries carry the shared
//! [`GUARD_HOOK_TIMEOUT_SECS`], and Claude's carry `"onFailure": "block"`,
//! which turns a Claude timeout, crash or start failure into a block. An
//! entry without them, or with another timeout, warns: a Claude entry
//! without `onFailure` lets a stalled guard through after Claude's default
//! of 600 seconds, and a Grok entry without a timeout gets Grok's default of
//! 5 seconds, which a loaded host has passed. Grok and Codex have no
//! fail-closed setting, so the check also states that residual.
//!
//! Files read, as the harnesses read them: `.claude/settings.json` and
//! `.claude/settings.local.json` (Claude, and Grok with its Claude
//! compatibility on), `.grok/hooks/*.json` (Grok) and `.codex/hooks.json`
//! (Codex).

use std::path::{Path, PathBuf};
use std::time::Instant;

use serde_json::Value;

use super::{CheckResult, Options, Status};
use crate::hooks::guard_deadline::{guard_of, GUARD_DEADLINE_SECS, GUARD_HOOK_TIMEOUT_SECS};

const NAME: &str = "guard-timeouts";

/// Which harness format a hook file is in, which decides the fields a
/// guard entry needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Format {
    /// Claude settings: `timeout` and `"onFailure": "block"`.
    Claude,
    /// A Grok hook file: `timeout`.
    Grok,
    /// Codex hooks: `timeout`.
    Codex,
}

/// The hook files the harnesses read in `root` that exist, with their
/// format, in a stable order.
fn hook_files(root: &Path) -> Result<Vec<(PathBuf, Format)>, String> {
    let mut files = Vec::new();
    for name in ["settings.json", "settings.local.json"] {
        files.push((root.join(".claude").join(name), Format::Claude));
    }
    let grok = root.join(".grok/hooks");
    if grok.try_exists().map_err(|error| {
        format!(
            "cannot inspect {}: {error}",
            crate::git::display_path(&grok)
        )
    })? {
        let mut found = Vec::new();
        for entry in std::fs::read_dir(&grok)
            .map_err(|error| format!("cannot read {}: {error}", crate::git::display_path(&grok)))?
        {
            let path = entry
                .map_err(|error| format!("cannot read a hook entry: {error}"))?
                .path();
            if path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
            {
                found.push(path);
            }
        }
        found.sort();
        files.extend(found.into_iter().map(|path| (path, Format::Grok)));
    }
    files.push((root.join(".codex/hooks.json"), Format::Codex));
    let mut present = Vec::new();
    for (path, format) in files {
        if path.try_exists().map_err(|error| {
            format!(
                "cannot inspect {}: {error}",
                crate::git::display_path(&path)
            )
        })? {
            present.push((path, format));
        }
    }
    Ok(present)
}

/// Every handler object under `hooks` whose command runs a `CodeFlow`
/// guard, with the guard's name.
fn guard_handlers(value: &Value, out: &mut Vec<(&'static str, serde_json::Map<String, Value>)>) {
    match value {
        Value::Object(map) => {
            if let Some(guard) = map
                .get("command")
                .and_then(Value::as_str)
                .and_then(guard_of)
            {
                out.push((guard, map.clone()));
            }
            for child in map.values() {
                guard_handlers(child, out);
            }
        }
        Value::Array(items) => {
            for child in items {
                guard_handlers(child, out);
            }
        }
        _ => {}
    }
}

/// What is wrong with one guard handler in a file of `format`, if anything.
fn problem(handler: &serde_json::Map<String, Value>, format: Format) -> Option<String> {
    let mut wrong = Vec::new();
    match handler.get("timeout") {
        None => wrong.push("no `timeout`".to_string()),
        Some(timeout) if timeout.as_u64() == Some(GUARD_HOOK_TIMEOUT_SECS) => {}
        Some(timeout) => wrong.push(format!(
            "`timeout` {}, not the shipped {GUARD_HOOK_TIMEOUT_SECS}",
            crate::git::display_line(&timeout.to_string())
        )),
    }
    if format == Format::Claude && handler.get("onFailure").and_then(Value::as_str) != Some("block")
    {
        wrong.push("no `\"onFailure\": \"block\"`".to_string());
    }
    (!wrong.is_empty()).then(|| wrong.join(" and "))
}

/// Each guard entry in `root` that lacks the shipped bounds, as
/// `<file>: <guard> has …`, and which formats hold guard entries.
fn findings(root: &Path) -> Result<(Vec<String>, Vec<Format>), String> {
    let mut found = Vec::new();
    let mut formats = Vec::new();
    for (path, format) in hook_files(root)? {
        let text = std::fs::read_to_string(&path)
            .map_err(|error| format!("cannot read {}: {error}", crate::git::display_path(&path)))?;
        let value: Value = serde_json::from_str(&text).map_err(|error| {
            format!("cannot parse {}: {error}", crate::git::display_path(&path))
        })?;
        let mut handlers = Vec::new();
        if let Some(hooks) = value.get("hooks") {
            guard_handlers(hooks, &mut handlers);
        }
        if !handlers.is_empty() && !formats.contains(&format) {
            formats.push(format);
        }
        let shown = crate::git::display_key(&crate::portable_path::slashed(
            path.strip_prefix(root).unwrap_or(&path),
        ));
        for (guard, handler) in handlers {
            if let Some(problem) = problem(&handler, format) {
                found.push(format!("{shown}: {guard} has {problem}"));
            }
        }
    }
    Ok((found, formats))
}

/// The residual in Grok and Codex, which have no fail-closed setting.
fn residual(formats: &[Format]) -> String {
    let open: Vec<&str> = [(Format::Grok, "Grok"), (Format::Codex, "Codex")]
        .iter()
        .filter(|(format, _)| formats.contains(format))
        .map(|(_, name)| *name)
        .collect();
    if open.is_empty() {
        return String::new();
    }
    format!(
        "; {} have no fail-closed setting: the guard refuses on its own after {GUARD_DEADLINE_SECS} seconds, but one that cannot reach that deadline (it does not start, or is starved) fails open there after the {GUARD_HOOK_TIMEOUT_SECS} second timeout",
        open.join(" and ")
    )
}

/// The `guard-timeouts` doctor check.
pub(super) fn check(opts: &Options) -> CheckResult {
    let start = Instant::now();
    let root = Path::new(&opts.project_dir);
    let (found, formats) = match findings(root) {
        Ok(found) => found,
        Err(error) => {
            return CheckResult {
                name: NAME.into(),
                status: Status::Fail,
                message: format!(
                    "{}; repair the file, then rerun `codeflow doctor --check {NAME}`",
                    crate::git::display_line(&error)
                ),
                duration: start.elapsed(),
            }
        }
    };
    let residual = residual(&formats);
    let (status, message) = if found.is_empty() {
        let message = if formats.is_empty() {
            "no CodeFlow guard hook entry in a harness hook file".to_string()
        } else {
            format!(
                "every CodeFlow guard hook entry carries the shipped {GUARD_HOOK_TIMEOUT_SECS} second timeout, and in Claude settings `\"onFailure\": \"block\"`{residual}"
            )
        };
        (Status::Pass, message)
    } else {
        let timeout = GUARD_HOOK_TIMEOUT_SECS.to_string();
        (
            Status::Warn(crate::remedy::DOCTOR_GUARD_TIMEOUTS.with(&[("timeout", &timeout)])),
            format!(
                "a guard that does not answer in time can let the tool call through: {}{residual}",
                found.join("; ")
            ),
        )
    };
    CheckResult {
        name: NAME.into(),
        status,
        message,
        duration: start.elapsed(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLAUDE_DEFAULT: &str = include_str!("../../../../assets/base/settings/default.json");
    const CLAUDE_ACCEPT: &str = include_str!("../../../../assets/base/settings/acceptEdits.json");
    const CLAUDE_BYPASS: &str =
        include_str!("../../../../assets/base/settings/bypass-sandboxed.json");
    const GROK: &str = include_str!("../../../../assets/base/grok/hooks.json");
    const CODEX: &str = include_str!("../../../../assets/base/codex/hooks.json");

    fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (path, text) in files {
            let path = dir.path().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        dir
    }

    fn run(dir: &tempfile::TempDir) -> CheckResult {
        check(&Options {
            project_dir: dir.path().to_string_lossy().into_owned(),
            ..Options::default()
        })
    }

    fn handlers(text: &str) -> Vec<(&'static str, serde_json::Map<String, Value>)> {
        let value: Value = serde_json::from_str(text).unwrap();
        let mut out = Vec::new();
        guard_handlers(&value["hooks"], &mut out);
        out
    }

    /// The contract the shipped files hold: every guard entry carries the
    /// shared timeout, every Claude one blocks on failure, and no other
    /// Claude hook entry carries `onFailure`.
    #[test]
    fn every_shipped_guard_entry_carries_the_shared_bounds() {
        for (text, format, guards) in [
            (CLAUDE_DEFAULT, Format::Claude, 2),
            (CLAUDE_ACCEPT, Format::Claude, 2),
            (CLAUDE_BYPASS, Format::Claude, 2),
            (GROK, Format::Grok, 3),
            (CODEX, Format::Codex, 3),
        ] {
            let found = handlers(text);
            assert_eq!(found.len(), guards, "{text}");
            for (guard, handler) in &found {
                assert_eq!(problem(handler, format), None, "{guard}: {handler:?}");
                assert_eq!(
                    handler["timeout"],
                    Value::from(GUARD_HOOK_TIMEOUT_SECS),
                    "{guard}"
                );
            }
            if format == Format::Claude {
                let value: Value = serde_json::from_str(text).unwrap();
                let with_on_failure = value["hooks"]
                    .as_object()
                    .unwrap()
                    .values()
                    .flat_map(|groups| groups.as_array().unwrap())
                    .flat_map(|group| group["hooks"].as_array().unwrap())
                    .filter(|handler| handler.get("onFailure").is_some())
                    .count();
                assert_eq!(with_on_failure, guards, "only guards block on failure");
            }
        }
    }

    #[test]
    fn the_shipped_files_pass_and_state_the_grok_and_codex_residual() {
        let dir = project(&[
            (".claude/settings.json", CLAUDE_DEFAULT),
            (".grok/hooks/codeflow.json", GROK),
            (".codex/hooks.json", CODEX),
        ]);
        let result = run(&dir);
        assert_eq!(result.status, Status::Pass, "{}", result.message);
        assert!(
            result
                .message
                .contains("Grok and Codex have no fail-closed setting"),
            "{}",
            result.message
        );
        let dir = project(&[(".claude/settings.json", CLAUDE_DEFAULT)]);
        let result = run(&dir);
        assert_eq!(result.status, Status::Pass, "{}", result.message);
        assert!(
            !result.message.contains("fail-closed"),
            "{}",
            result.message
        );
        let dir = project(&[]);
        assert_eq!(run(&dir).status, Status::Pass);
    }

    fn warned(files: &[(&str, &str)], expected: &str) {
        let dir = project(files);
        let result = run(&dir);
        assert!(result.status.is_warn(), "{expected}: {}", result.message);
        assert!(
            result.message.contains(expected),
            "{expected}: {}",
            result.message
        );
        let Status::Warn(remedy) = &result.status else {
            unreachable!()
        };
        assert!(remedy.to_string().contains("codeflow update"), "{remedy}");
    }

    /// The shipped Claude preset as releases before TSK-1082 shipped it:
    /// guards with neither field.
    fn claude_without(fields: &[&str]) -> String {
        let mut text = CLAUDE_DEFAULT.to_string();
        for field in fields {
            text = match *field {
                "timeout" => text.replace(
                    &format!(",\n            \"timeout\": {GUARD_HOOK_TIMEOUT_SECS}"),
                    "",
                ),
                "onFailure" => text.replace(",\n            \"onFailure\": \"block\"", ""),
                _ => unreachable!(),
            };
        }
        assert_ne!(text, CLAUDE_DEFAULT, "{fields:?}");
        serde_json::from_str::<Value>(&text).expect("still JSON");
        text
    }

    #[test]
    fn a_claude_guard_without_a_timeout_warns() {
        let text = claude_without(&["timeout"]);
        warned(
            &[(".claude/settings.json", &text)],
            ".claude/settings.json: git-guard has no `timeout`",
        );
    }

    #[test]
    fn a_claude_guard_without_on_failure_block_warns() {
        let text = claude_without(&["onFailure"]);
        warned(
            &[(".claude/settings.json", &text)],
            ".claude/settings.json: exec-guard has no `\"onFailure\": \"block\"`",
        );
        let text =
            CLAUDE_DEFAULT.replace("\"onFailure\": \"block\"", "\"onFailure\": \"continue\"");
        warned(
            &[(".claude/settings.json", &text)],
            "git-guard has no `\"onFailure\": \"block\"`",
        );
    }

    #[test]
    fn the_pre_fix_claude_preset_warns_for_both_fields() {
        let text = claude_without(&["timeout", "onFailure"]);
        warned(
            &[(".claude/settings.json", &text)],
            "git-guard has no `timeout` and no `\"onFailure\": \"block\"`",
        );
    }

    #[test]
    fn a_claude_guard_with_another_timeout_warns() {
        let text = CLAUDE_DEFAULT.replace(
            &format!("\"timeout\": {GUARD_HOOK_TIMEOUT_SECS}"),
            "\"timeout\": 5",
        );
        warned(
            &[(".claude/settings.json", &text)],
            "git-guard has `timeout` 5, not the shipped 30",
        );
    }

    #[test]
    fn a_local_claude_settings_guard_is_read_too() {
        let local = r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"codeflow hook git-guard --contract 3"}]}]}}"#;
        warned(
            &[
                (".claude/settings.json", CLAUDE_DEFAULT),
                (".claude/settings.local.json", local),
            ],
            ".claude/settings.local.json: git-guard has no `timeout` and no `\"onFailure\": \"block\"`",
        );
    }

    #[test]
    fn a_grok_guard_without_or_with_another_timeout_warns() {
        let shipped = format!("\"timeout\": {GUARD_HOOK_TIMEOUT_SECS} }}");
        let missing = GROK.replacen(&format!(", {shipped}"), " }", 1);
        assert_ne!(missing, GROK);
        warned(
            &[(".grok/hooks/codeflow.json", &missing)],
            ".grok/hooks/codeflow.json: git-guard has no `timeout`",
        );
        let ten = GROK.replace(&shipped, "\"timeout\": 10 }");
        warned(
            &[(".grok/hooks/codeflow.json", &ten)],
            ".grok/hooks/codeflow.json: edit-guard has `timeout` 10, not the shipped 30",
        );
        // A Grok file needs no `onFailure`, which Grok does not read.
        let dir = project(&[(".grok/hooks/codeflow.json", GROK)]);
        assert_eq!(run(&dir).status, Status::Pass);
    }

    #[test]
    fn a_codex_guard_with_another_timeout_warns() {
        let shipped = format!("\"timeout\": {GUARD_HOOK_TIMEOUT_SECS} }}");
        let ten = CODEX.replacen(&shipped, "\"timeout\": 10 }", 1);
        assert_ne!(ten, CODEX);
        warned(
            &[(".codex/hooks.json", &ten)],
            ".codex/hooks.json: git-guard has `timeout` 10, not the shipped 30",
        );
    }

    #[test]
    fn a_hook_file_that_does_not_parse_fails() {
        let dir = project(&[(".codex/hooks.json", "{not json")]);
        let result = run(&dir);
        assert_eq!(result.status, Status::Fail, "{}", result.message);
        assert!(
            result.message.contains("cannot parse"),
            "{}",
            result.message
        );
    }

    #[test]
    fn other_codeflow_hooks_are_not_judged() {
        let text = r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"codeflow hook session-orient --contract 3"}]}]}}"#;
        let dir = project(&[(".claude/settings.json", text)]);
        let result = run(&dir);
        assert_eq!(result.status, Status::Pass, "{}", result.message);
        assert!(
            result.message.starts_with("no CodeFlow guard"),
            "{}",
            result.message
        );
    }
}
