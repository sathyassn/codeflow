//! Enforcement-path protection for native file-edit tools.
//! Native Grok captures, a documented Codex schema, and synthetic path edge cases.

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Component, Path, PathBuf};

use serde_json::{Map, Value};

use super::{PolicyLevel, Violation};
use crate::security::{actions, pattern::matches_extended_glob};

/// An input which the edit guard cannot safely interpret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditError(pub String);

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for EditError {}

/// Target metadata only. Content never participates in enforcement-path matching.
#[derive(Debug, PartialEq, Eq)]
pub struct EditRequest {
    pub cwd: Option<PathBuf>,
    pub paths: Vec<PathBuf>,
}

/// Runtime authority supplied by the CLI adapter, never by `tool_input`.
/// `cwd`, `root`, `home`, and `git_common_dir`, when present, must be absolute.
pub struct EditContext<'a> {
    pub cwd: &'a Path,
    pub root: &'a Path,
    pub home: &'a Path,
    pub git_common_dir: Option<&'a Path>,
    pub level: PolicyLevel,
}

/// Parse explicitly supported hook forms. Unknown tools are outside this hook.
/// Grok edit forms have native captures; Codex still awaits native qualification.
///
/// # Errors
/// Returns an error for malformed or ambiguous supported edit payloads.
pub fn parse_payload(input: &str) -> Result<Option<EditRequest>, EditError> {
    let value: Value = serde_json::from_str(input).map_err(|e| EditError(e.to_string()))?;
    let object = value
        .as_object()
        .ok_or_else(|| EditError("expected hook object".into()))?;
    for field in ["toolInputTruncated", "tool_input_truncated"] {
        if object.get(field) == Some(&Value::Bool(true)) {
            return Err(EditError(format!(
                "{field} is true; cannot inspect a truncated edit input"
            )));
        }
    }
    let tool = alias(object, &["tool_name", "toolName"])?
        .and_then(Value::as_str)
        .ok_or_else(|| EditError("tool_name must be a string".into()))?;
    if !matches!(
        tool,
        "apply_patch" | "write" | "search_replace" | "Write" | "Edit"
    ) {
        return Ok(None);
    }
    let cwd = match object.get("cwd") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if !s.is_empty() => Some(PathBuf::from(s)),
        _ => {
            return Err(EditError(
                "cwd must be a nonempty string when present".into(),
            ))
        }
    };
    let input = alias(object, &["tool_input", "toolInput"])?
        .ok_or_else(|| EditError("edit call has no tool_input".into()))?;
    let paths = if tool == "apply_patch" {
        let patch = match input {
            Value::String(s) => s.as_str(),
            Value::Object(fields) => alias(fields, &["command", "patch", "input"])?
                .and_then(Value::as_str)
                .ok_or_else(|| EditError("apply_patch input must carry patch text".into()))?,
            _ => {
                return Err(EditError(
                    "apply_patch input must be patch text or an object".into(),
                ))
            }
        };
        patch_paths(patch)?
    } else {
        let fields = input
            .as_object()
            .ok_or_else(|| EditError("file edit input must be an object".into()))?;
        let path = alias(fields, &["file_path", "filePath", "path"])?
            .and_then(Value::as_str)
            .ok_or_else(|| EditError("file edit input must name its path".into()))?;
        vec![checked_path(path)?]
    };
    Ok(Some(EditRequest { cwd, paths }))
}

// Refuse conflicting aliases: choosing the first can inspect a different target
// from the one the harness executes. Equal duplicate aliases are harmless.
fn alias<'a>(
    object: &'a Map<String, Value>,
    names: &[&str],
) -> Result<Option<&'a Value>, EditError> {
    let mut selected = None;
    for name in names {
        if let Some(value) = object.get(*name) {
            if selected.is_some_and(|old| old != value) {
                return Err(EditError(format!(
                    "conflicting aliases: {}",
                    names.join(", ")
                )));
            }
            selected = Some(value);
        }
    }
    Ok(selected)
}

fn checked_path(path: &str) -> Result<PathBuf, EditError> {
    if path.is_empty() || path.contains('\0') {
        return Err(EditError(
            "edit target must be nonempty and contain no NUL".into(),
        ));
    }
    Ok(PathBuf::from(path))
}

/// Extract every native apply-patch operation target, including rename sources
/// and destinations. Hunk content is not searched for strings resembling paths.
fn patch_paths(patch: &str) -> Result<Vec<PathBuf>, EditError> {
    let mut lines = patch.lines();
    if lines.next() != Some("*** Begin Patch") {
        return Err(EditError("patch has no Begin Patch marker".into()));
    }
    let mut paths = Vec::new();
    let mut may_move = false;
    let mut ended = false;
    for line in lines {
        if ended {
            if !line.is_empty() {
                return Err(EditError("content follows End Patch".into()));
            }
            continue;
        }
        if line == "*** End Patch" {
            ended = true;
            continue;
        }
        if let Some(path) = line.strip_prefix("*** Update File: ") {
            paths.push(checked_path(path)?);
            may_move = true;
        } else if let Some(path) = line
            .strip_prefix("*** Add File: ")
            .or_else(|| line.strip_prefix("*** Delete File: "))
        {
            paths.push(checked_path(path)?);
            may_move = false;
        } else if let Some(path) = line.strip_prefix("*** Move to: ") {
            if !may_move {
                return Err(EditError(
                    "Move to must immediately follow Update File".into(),
                ));
            }
            paths.push(checked_path(path)?);
            may_move = false;
        } else {
            if paths.is_empty() || (line.starts_with("*** ") && line != "*** End of File") {
                return Err(EditError("unrecognized patch operation".into()));
            }
            may_move = false;
        }
    }
    if !ended || paths.is_empty() {
        return Err(EditError("patch is incomplete or names no files".into()));
    }
    Ok(paths)
}

/// Shared entry for interpreter literal references. It does not read file
/// contents. An absent home or uninspectable path is explicit uncertainty,
/// never proof that the path is ordinary. Policy handling stays with the caller.
pub(crate) fn enforcement_path(
    path: &str,
    cwd: &Path,
    root: &Path,
    home: Option<&Path>,
) -> Result<bool, String> {
    let home = home.ok_or_else(|| "cannot resolve the home for enforcement paths".to_string())?;
    let repo = super::RepoInfo::discover(root);
    let ctx = EditContext {
        cwd,
        root,
        home,
        git_common_dir: repo.as_ref().map(|repo| repo.common_dir.as_path()),
        level: PolicyLevel::Block,
    };
    let request = EditRequest {
        cwd: None,
        paths: vec![checked_path(path).map_err(|e| e.to_string())?],
    };
    evaluate(&request, &ctx)
        .map(|violations| !violations.is_empty())
        .map_err(|e| e.to_string())
}

fn absolute_target(path: &Path, cwd: &Path, home: &Path) -> PathBuf {
    if let Ok(relative) = path.strip_prefix("~") {
        return home.join(relative);
    }
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

/// Return a policy finding per refused target, with the existing rule and remedy.
/// Errors must remain distinct from an allowed edit at the hook adapter boundary.
///
/// # Errors
/// Returns an error when the context or a target cannot be resolved safely.
pub fn evaluate(request: &EditRequest, ctx: &EditContext<'_>) -> Result<Vec<Violation>, EditError> {
    if !ctx.level.is_active() {
        return Ok(Vec::new());
    }
    let cwd = request.cwd.as_deref().unwrap_or(ctx.cwd);
    for path in [cwd, ctx.root, ctx.home]
        .into_iter()
        .chain(ctx.git_common_dir)
    {
        if !path.is_absolute() {
            return Err(EditError(format!(
                "edit context must be absolute: {}",
                path.display()
            )));
        }
    }
    let protected = enforcement_patterns(ctx)?;
    let mut seen = BTreeSet::new();
    let mut violations = Vec::new();
    for path in &request.paths {
        let absolute = absolute_target(path, cwd, ctx.home);
        let lexical = normalized(&absolute, false)?;
        let resolved = normalized(&absolute, true)?;
        for candidate in [&lexical, &resolved] {
            let text = path_text(candidate)?;
            if protected.iter().any(|pattern| covers(&text, pattern))
                && seen.insert(lexical.clone())
            {
                violations.push(Violation::new(
                    "git.hook_integrity",
                    ctx.level,
                    format!(
                        "file edit targets the enforcement path `{}`",
                        lexical.display()
                    ),
                    crate::remedy::HOOK_INTEGRITY.remedy(),
                ));
            }
        }
    }
    Ok(violations)
}

fn enforcement_patterns(ctx: &EditContext<'_>) -> Result<Vec<String>, EditError> {
    let mut result = BTreeSet::new();
    let mut reduced = BTreeSet::new();
    for rule in &actions::table().claude_edit_denies {
        let pattern = rule
            .strip_prefix("Edit(")
            .and_then(|s| s.strip_suffix(')'))
            .ok_or_else(|| EditError(format!("invalid enforcement pattern: {rule}")))?;
        let (base, relative) = if let Some(relative) = pattern.strip_prefix("~/") {
            (ctx.home, relative)
        } else if let Some(relative) = pattern.strip_prefix('/') {
            (ctx.root, relative)
        } else {
            return Err(EditError(format!(
                "enforcement pattern has no supported anchor: {rule}"
            )));
        };
        for resolve in [false, true] {
            // A linked worktree's .git is a file, not a directory. Keep its
            // lexical spelling below, and resolve real Git paths through the
            // supplied common dir instead of failing every unrelated edit.
            if resolve
                && base == ctx.root
                && relative.starts_with(".git/")
                && ctx.git_common_dir.is_some()
                && ctx.root.join(".git").is_file()
            {
                continue;
            }
            // Patterns name literal enforcement directories followed by globs.
            // Resolve their literal prefix as well, so a protected directory or
            // file which is a symlink cannot be edited through its other spelling.
            match resolved_pattern(base, relative, resolve) {
                Ok(pattern) => {
                    result.insert(pattern);
                }
                Err(error) if resolve => {
                    reduced.insert(error.to_string());
                }
                Err(error) => return Err(error),
            }
        }
        if base == ctx.root {
            if let (Some(common), Some(relative)) =
                (ctx.git_common_dir, relative.strip_prefix(".git/"))
            {
                for resolve in [false, true] {
                    match resolved_pattern(common, relative, resolve) {
                        Ok(pattern) => {
                            result.insert(pattern);
                        }
                        Err(error) if resolve => {
                            reduced.insert(error.to_string());
                        }
                        Err(error) => return Err(error),
                    }
                }
            }
        }
    }
    if !reduced.is_empty() {
        eprintln!(
            "codeflow edit-guard: reduced alias coverage; lexical enforcement remains active: {}",
            reduced.into_iter().collect::<Vec<_>>().join("; ")
        );
    }
    Ok(result.into_iter().collect())
}

fn resolved_pattern(base: &Path, relative: &str, resolve: bool) -> Result<String, EditError> {
    let parts: Vec<_> = relative.split('/').collect();
    let split = parts
        .iter()
        .position(|p| p.contains(['*', '?', '[']))
        .unwrap_or(parts.len());
    let mut literal = base.to_path_buf();
    for part in &parts[..split] {
        literal.push(part);
    }
    let literal = normalized(&literal, resolve)?;
    let mut pattern = path_text(&literal)?;
    for part in &parts[split..] {
        if !pattern.ends_with('/') {
            pattern.push('/');
        }
        pattern.push_str(part);
    }
    Ok(pattern)
}

fn covers(path: &str, pattern: &str) -> bool {
    matches_extended_glob(path, pattern)
        || pattern
            .strip_suffix("/**")
            .is_some_and(|parent| path == parent)
}

fn path_text(path: &Path) -> Result<String, EditError> {
    let text = path
        .to_str()
        .ok_or_else(|| EditError("non-UTF-8 enforcement path".into()))?;
    #[cfg(windows)]
    let text = text.replace('\\', "/");
    Ok(text.to_string())
}

// APFS realpath retains the caller's case. Recover the directory entry's
// spelling only when its identity matches, so suffix rules stay precise.
#[cfg(target_os = "macos")]
fn normalize_case(path: &mut PathBuf, metadata: &std::fs::Metadata) {
    use std::os::unix::fs::MetadataExt;
    let Some(parent) = path.parent() else {
        return;
    };
    let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        if entry
            .file_name()
            .to_str()
            .is_some_and(|s| s.eq_ignore_ascii_case(name))
            && std::fs::symlink_metadata(entry.path())
                .is_ok_and(|m| m.dev() == metadata.dev() && m.ino() == metadata.ino())
        {
            *path = entry.path();
            return;
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn normalize_case(_path: &mut PathBuf, _metadata: &std::fs::Metadata) {}

// Resolve one component at a time. Lexically deleting `alias/..` before
// following `alias` is wrong when alias is a symlink into another directory.
// Missing suffixes are kept, so adding a new file beneath an existing symlink
// is judged against that symlink's destination. Broken links and permissions
// are errors, not an implicit allow.
fn normalized(path: &Path, resolve: bool) -> Result<PathBuf, EditError> {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => result.push(prefix.as_os_str()),
            Component::RootDir => result.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            Component::Normal(part) => {
                result.push(part);
                if resolve {
                    match std::fs::symlink_metadata(&result) {
                        Ok(metadata) if metadata.file_type().is_symlink() => {
                            result = result.canonicalize().map_err(|e| {
                                EditError(format!("cannot resolve {}: {e}", result.display()))
                            })?;
                        }
                        Ok(metadata) => {
                            normalize_case(&mut result, &metadata);
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                        Err(e) => {
                            return Err(EditError(format!(
                                "cannot inspect {}: {e}",
                                result.display()
                            )))
                        }
                    }
                }
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn synthetic(tool: &str, input: Value) -> EditRequest {
        let mut payload = json!({"tool_name": tool});
        payload["tool_input"] = input;
        parse_payload(&payload.to_string()).unwrap().unwrap()
    }

    struct Fixture {
        temp: tempfile::TempDir,
        root: PathBuf,
        home: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("repo");
            let home = temp.path().join("home");
            std::fs::create_dir_all(&root).unwrap();
            std::fs::create_dir_all(&home).unwrap();
            Self { temp, root, home }
        }
        fn ctx(&self) -> EditContext<'_> {
            EditContext {
                cwd: &self.root,
                root: &self.root,
                home: &self.home,
                git_common_dir: None,
                level: PolicyLevel::Block,
            }
        }
        fn refused(&self, path: &str) -> bool {
            !evaluate(&synthetic("write", json!({"path": path})), &self.ctx())
                .unwrap()
                .is_empty()
        }
    }

    #[test]
    fn f9_truncated_inputs_are_refused() {
        for field in ["toolInputTruncated", "tool_input_truncated"] {
            let mut payload = json!({"toolName":"write","toolInput":{"file_path":"notes.md"}});
            payload[field] = json!(true);
            assert!(parse_payload(&payload.to_string())
                .unwrap_err()
                .to_string()
                .contains(field));
            payload[field] = json!(false);
            assert!(parse_payload(&payload.to_string()).unwrap().is_some());
        }
    }

    #[test]
    fn f10_native_fixtures_keep_duplicate_aliases() {
        for data in [
            include_str!("../../tests/fixtures/edit-hooks/grok-write.json"),
            include_str!("../../tests/fixtures/edit-hooks/grok-search-replace.json"),
        ] {
            let mut payload: Value = serde_json::from_str(data).unwrap();
            assert_eq!(
                payload["_fixture"]["provenance"],
                "native capture, grok 1.0.44"
            );
            assert_eq!(payload["toolInput"], payload["tool_input"]);
            assert!(parse_payload(&payload.to_string()).unwrap().is_some());
            payload["tool_input"]["file_path"] = json!("different.md");
            assert!(parse_payload(&payload.to_string()).is_err());
        }
    }

    #[cfg(unix)]
    #[test]
    fn f8_uninspectable_pattern_does_not_block_unrelated_targets() {
        let f = Fixture::new();
        // A symlink loop gives deterministic metadata failure, even under root.
        std::os::unix::fs::symlink(".claude", f.home.join(".claude")).unwrap();
        assert!(!f.refused("notes.md"));
        assert!(!enforcement_path("src/lib.rs", &f.root, &f.root, Some(&f.home)).unwrap());
        assert!(evaluate(
            &synthetic(
                "write",
                json!({"path":f.home.join(".claude/settings.json")})
            ),
            &f.ctx()
        )
        .is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn f7_case_aliases_preserve_the_protected_suffix() {
        let f = Fixture::new();
        std::fs::create_dir(f.root.join(".codeflow")).unwrap();
        std::fs::write(f.root.join(".codeflow/policy.json"), "{}").unwrap();
        // The fixture must actually exercise a case-insensitive volume.
        assert!(f.root.join(".CODEFLOW/POLICY.JSON").exists());
        for path in [".CODEFLOW/policy.json", ".CODEFLOW/POLICY.JSON"] {
            assert!(f.refused(path), "{path}");
            assert!(enforcement_path(path, &f.root, &f.root, Some(&f.home)).unwrap());
        }
        assert!(!f.refused(".CODEFLOW/notes.md"));
        assert!(!f.refused(".CODEFLOW/policy.json.example"));
        std::fs::create_dir(f.root.join(".codex")).unwrap();
        assert!(f.refused(".CODEX/new.json"));
    }

    #[test]
    fn documented_hook_fixtures_protect_paths_and_allow_ordinary_edits() {
        let fixture = Fixture::new();
        for data in [
            include_str!("../../tests/fixtures/edit-hooks/codex-apply-patch.json"),
            include_str!("../../tests/fixtures/edit-hooks/grok-write.json"),
            include_str!("../../tests/fixtures/edit-hooks/grok-search-replace.json"),
        ] {
            let protected = data
                .replace("/fixture/project", fixture.root.to_str().unwrap())
                .replace("hello.txt", ".codex/config.toml")
                .replace("notes.txt", ".codeflow/policy.json");
            let request = parse_payload(&protected).unwrap().unwrap();
            assert!(!evaluate(&request, &fixture.ctx()).unwrap().is_empty());
            let ordinary = protected
                .replace(".codeflow/policy.json", "notes.md")
                .replace(".codex/config.toml", "notes.md");
            let request = parse_payload(&ordinary).unwrap().unwrap();
            assert!(evaluate(&request, &fixture.ctx()).unwrap().is_empty());
        }
    }

    #[test]
    fn synthetic_snake_and_camel_payloads_extract_paths() {
        for (tool, input) in [
            ("Write", json!({"file_path":"src/lib.rs"})),
            ("Edit", json!({"filePath":"src/lib.rs"})),
            ("write", json!({"path":"src/lib.rs"})),
            ("search_replace", json!({"file_path":"src/lib.rs"})),
        ] {
            let snake = synthetic(tool, input.clone());
            let camel = parse_payload(
                &json!({"toolName":tool,"toolInput":input,"cwd":"/repo"}).to_string(),
            )
            .unwrap()
            .unwrap();
            assert_eq!(snake.paths, camel.paths);
            assert_eq!(camel.paths, vec![PathBuf::from("src/lib.rs")]);
        }
    }

    #[test]
    fn synthetic_patch_includes_add_delete_update_and_move_targets() {
        let patch = "*** Begin Patch\n*** Add File: a\n+x\n*** Delete File: b\n*** Update File: c\n*** Move to: d\n@@\n-x\n+y\n*** End Patch\n";
        for input in [json!(patch), json!({"patch":patch}), json!({"input":patch})] {
            assert_eq!(
                synthetic("apply_patch", input).paths,
                ["a", "b", "c", "d"].map(PathBuf::from)
            );
        }
        assert!(patch_paths("*** Begin Patch\n*** Move to: x\n*** End Patch").is_err());
        assert!(patch_paths("*** Begin Patch\n*** Add File: a\n+x").is_err());
    }

    #[test]
    fn every_table_enforcement_pattern_has_a_refused_witness() {
        let f = Fixture::new();
        for rule in &actions::table().claude_edit_denies {
            let pattern = rule
                .strip_prefix("Edit(")
                .unwrap()
                .strip_suffix(')')
                .unwrap();
            let path = if let Some(relative) = pattern.strip_prefix("~/") {
                f.home.join(relative.replace("**", "witness.json"))
            } else {
                f.root.join(
                    pattern
                        .strip_prefix('/')
                        .unwrap()
                        .replace("**", "witness.json"),
                )
            };
            assert!(f.refused(path.to_str().unwrap()), "{rule}");
        }
    }

    #[test]
    fn paired_paths_and_levels_use_the_existing_rule() {
        let f = Fixture::new();
        for path in [
            ".codeflow/policy.json",
            "./.codeflow//policy.json",
            "src/../.codeflow/policy.json",
            ".codex/new.json",
        ] {
            assert!(f.refused(path), "{path}");
        }
        for path in [
            "src/lib.rs",
            "README.md",
            ".env.example",
            ".codeflow/policy.json.example",
            ".codex-notes/config.toml",
            "assets/base/settings/default.json",
        ] {
            assert!(!f.refused(path), "{path}");
        }
        let request = synthetic("write", json!({"path":".codeflow/policy.json"}));
        for level in [PolicyLevel::Block, PolicyLevel::Warn] {
            let mut ctx = f.ctx();
            ctx.level = level;
            let findings = evaluate(&request, &ctx).unwrap();
            assert_eq!(findings.len(), 1);
            assert_eq!(findings[0].rule, "git.hook_integrity");
            assert_eq!(findings[0].level, level);
        }
        for level in [PolicyLevel::Allow, PolicyLevel::Off] {
            let mut ctx = f.ctx();
            ctx.level = level;
            assert!(evaluate(&request, &ctx).unwrap().is_empty());
        }
    }

    #[test]
    fn protected_patch_destination_and_source_both_refuse() {
        let f = Fixture::new();
        for (source, destination) in [
            ("src/a", ".codeflow/policy.json"),
            (".codeflow/policy.json", "src/a"),
        ] {
            let patch = format!("*** Begin Patch\n*** Add File: docs/new.md\n+ordinary\n*** Update File: {source}\n*** Move to: {destination}\n@@\n-x\n+y\n*** End Patch\n");
            let request = synthetic("apply_patch", json!(patch));
            assert_eq!(evaluate(&request, &f.ctx()).unwrap().len(), 1);
        }
        let content_only = "*** Begin Patch\n*** Add File: docs/example.md\n+*** Delete File: .codeflow/policy.json\n*** End Patch\n";
        assert!(
            evaluate(&synthetic("apply_patch", json!(content_only)), &f.ctx())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn shared_interpreter_entry_matches_paths_without_contents() {
        let f = Fixture::new();
        assert!(enforcement_path("~/.codex/config.toml", &f.root, &f.root, Some(&f.home)).unwrap());
        assert!(
            enforcement_path(".codeflow/policy.json", &f.root, &f.root, Some(&f.home)).unwrap()
        );
        assert!(!enforcement_path("README.md", &f.root, &f.root, Some(&f.home)).unwrap());
        assert!(enforcement_path("README.md", &f.root, &f.root, None).is_err());
    }

    #[test]
    fn aliases_must_agree_and_edit_targets_must_exist_in_payload() {
        for payload in [
            json!({"tool_name":"write","toolName":"Bash","tool_input":{"path":"x"}}),
            json!({"toolName":"write","toolInput":{"path":"x","file_path":".codex/config.toml"}}),
            json!({"toolName":"write","toolInput":{}}),
            json!({"tool_name":"apply_patch","tool_input":{"patch":"bad"}}),
        ] {
            assert!(parse_payload(&payload.to_string()).is_err());
        }
        assert!(parse_payload(r#"{"tool_name":"Read"}"#).unwrap().is_none());
    }

    #[test]
    fn cwd_relative_targets_and_git_common_dir_are_covered() {
        let f = Fixture::new();
        let subdir = f.root.join("src");
        std::fs::create_dir(&subdir).unwrap();
        let request = parse_payload(&json!({"toolName":"write","toolInput":{"path":"../.codeflow/policy.json"},"cwd":subdir}).to_string()).unwrap().unwrap();
        assert_eq!(evaluate(&request, &f.ctx()).unwrap().len(), 1);
        let common = f.temp.path().join("main/.git");
        std::fs::create_dir_all(&common).unwrap();
        std::fs::write(
            f.root.join(".git"),
            format!("gitdir: {}\n", common.display()),
        )
        .unwrap();
        let mut ctx = f.ctx();
        ctx.git_common_dir = Some(&common);
        assert!(
            evaluate(&synthetic("write", json!({"path":"src/lib.rs"})), &ctx)
                .unwrap()
                .is_empty()
        );
        let request = synthetic("write", json!({"path":common.join("config")}));
        assert_eq!(evaluate(&request, &ctx).unwrap().len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_ancestors_new_files_and_parent_traversal_are_covered() {
        use std::os::unix::fs::symlink;
        let f = Fixture::new();
        std::fs::create_dir_all(f.root.join(".codex/inside")).unwrap();
        symlink(f.root.join(".codex"), f.root.join("alias")).unwrap();
        assert!(f.refused("alias/new-file.json"));
        symlink(f.root.join(".codex/inside"), f.root.join("inner")).unwrap();
        assert!(f.refused("inner/../config.toml"));
        let ordinary = f.temp.path().join("ordinary");
        std::fs::create_dir(&ordinary).unwrap();
        symlink(&ordinary, f.root.join("ordinary-link")).unwrap();
        assert!(!f.refused("ordinary-link/new-file.rs"));
    }

    #[cfg(unix)]
    #[test]
    fn enforcement_file_symlink_protects_its_real_destination() {
        use std::os::unix::fs::symlink;
        let f = Fixture::new();
        std::fs::create_dir(f.root.join(".codeflow")).unwrap();
        let shared = f.temp.path().join("shared-policy.json");
        std::fs::write(&shared, "{}").unwrap();
        symlink(&shared, f.root.join(".codeflow/policy.json")).unwrap();
        assert!(f.refused(shared.to_str().unwrap()));
    }
}
