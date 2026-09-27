//! Project record templates (SPC-013 R-37): `epic new`, `spec new` and
//! `task new` read `project-management/templates/<kind>.md` when it is a
//! usable template and fall back to the embedded one otherwise. A template
//! is usable when it carries the placeholders the allocator fills and a
//! record rendered from it passes the same validator every record meets.

use std::path::Path;

use super::RecordKind;
use crate::scaffold::template::TemplateContext;
use crate::validate::{validate_epic, validate_spec, validate_task, ValidateOptions};

/// Where a project keeps its record templates.
pub const PROJECT_TEMPLATES: &str = "project-management/templates";

fn name(kind: RecordKind) -> &'static str {
    match kind {
        RecordKind::Epic => "epic",
        RecordKind::Spec => "spec",
        RecordKind::Task => "task",
    }
}

/// The placeholders the allocator fills for `kind`.
fn required_placeholders(kind: RecordKind) -> &'static [&'static str] {
    match kind {
        RecordKind::Task => &[
            "{{NNN}}",
            "{{TITLE_YAML}}",
            "{{DATE}}",
            "{{EPIC_ID}}",
            "{{STANDALONE_REASON}}",
            "{{TARGET_BRANCH}}",
        ],
        RecordKind::Epic | RecordKind::Spec => &["{{NNN}}", "{{TITLE_YAML}}", "{{DATE}}"],
    }
}

/// Check that `text` is a usable template for `kind`.
///
/// # Errors
/// The first reason it is not: a missing placeholder, or the validator's
/// findings on a record rendered from it.
pub fn check(kind: RecordKind, text: &str) -> Result<(), String> {
    static CHECKS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    if let Some(missing) = required_placeholders(kind)
        .iter()
        .find(|p| !text.contains(**p))
    {
        return Err(format!("missing placeholder {missing}"));
    }
    let (prefix, dir) = match kind {
        RecordKind::Epic => ("EPC", "epics"),
        RecordKind::Spec => ("SPC", "specs"),
        RecordKind::Task => ("TSK", "tasks"),
    };
    let mut ctx = TemplateContext::new();
    for (key, value) in [
        ("NNN", "999"),
        ("TITLE", "template check"),
        ("TITLE_YAML", "\"template check\""),
        ("DATE", "2026-01-01"),
        ("EPIC_ID", "EPC-999"),
        ("STANDALONE_REASON", "null"),
        ("TARGET_BRANCH", "\"main\""),
    ] {
        ctx.set(key, value);
    }
    let rendered = ctx.substitute(text);
    // The validator reads a record from its canonical path, so the trial
    // record is written to a private scratch folder and removed after.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let seq = CHECKS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let scratch = std::env::temp_dir().join(format!(
        "codeflow-template-check-{}-{nanos}-{seq}",
        std::process::id()
    ));
    let folder = scratch.join(dir);
    let path = folder.join(format!("{prefix}-999.md"));
    let written = std::fs::create_dir_all(&folder).and_then(|()| std::fs::write(&path, rendered));
    let result = written
        .map_err(|e| format!("cannot check the template: {e}"))
        .map(|()| {
            let opts = ValidateOptions::default();
            match kind {
                RecordKind::Epic => validate_epic(&path, &opts),
                RecordKind::Spec => validate_spec(&path, &opts),
                RecordKind::Task => validate_task(&path, &opts),
            }
        });
    let _ = std::fs::remove_dir_all(&scratch);
    match result? {
        Ok((errors, _)) if errors.is_empty() => Ok(()),
        Ok((errors, _)) => Err(errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ")),
        Err(e) => Err(e.to_string()),
    }
}

/// Which template `new` used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplateSource {
    /// The project's own template.
    Project,
    /// The embedded template: the project has none.
    Embedded,
    /// The embedded template, because the project's is not usable.
    Fallback(String),
}

/// The template for `kind`: the project's when usable, otherwise
/// `embedded`, with where it came from.
#[must_use]
pub fn load(root: &Path, kind: RecordKind, embedded: &str) -> (String, TemplateSource) {
    let path = root
        .join(PROJECT_TEMPLATES)
        .join(format!("{}.md", name(kind)));
    match std::fs::read_to_string(&path) {
        Ok(text) => match check(kind, &text) {
            Ok(()) => (text, TemplateSource::Project),
            Err(reason) => (
                embedded.to_string(),
                TemplateSource::Fallback(format!(
                    "{PROJECT_TEMPLATES}/{}.md is not a usable {} template ({reason}); used the embedded template",
                    name(kind),
                    name(kind)
                )),
            ),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            (embedded.to_string(), TemplateSource::Embedded)
        }
        Err(e) => (
            embedded.to_string(),
            TemplateSource::Fallback(format!(
                "cannot read {PROJECT_TEMPLATES}/{}.md ({e}); used the embedded template",
                name(kind)
            )),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TASK: &str = include_str!("../../../../assets/base/pm/task.md.tmpl");
    const EPIC: &str = include_str!("../../../../assets/base/pm/epic.md.tmpl");
    const SPEC: &str = include_str!("../../../../assets/base/pm/spec.md.tmpl");

    #[test]
    fn shipped_templates_are_usable() {
        assert_eq!(check(RecordKind::Task, TASK), Ok(()));
        assert_eq!(check(RecordKind::Epic, EPIC), Ok(()));
        assert_eq!(check(RecordKind::Spec, SPEC), Ok(()));
    }

    #[test]
    fn broken_templates_are_refused_with_a_reason() {
        let no_target = TASK.replace("{{TARGET_BRANCH}}", "main");
        assert!(check(RecordKind::Task, &no_target)
            .unwrap_err()
            .contains("{{TARGET_BRANCH}}"));
        let no_criteria = TASK.replace("## Acceptance Criteria", "## Done when");
        assert!(check(RecordKind::Task, &no_criteria).is_err());
        let no_frontmatter = TASK.replacen("---\n", "", 1);
        assert!(check(RecordKind::Task, &no_frontmatter).is_err());
    }

    #[test]
    fn load_prefers_a_usable_project_template_and_falls_back_otherwise() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            load(dir.path(), RecordKind::Task, TASK).1,
            TemplateSource::Embedded
        );
        let folder = dir.path().join(PROJECT_TEMPLATES);
        std::fs::create_dir_all(&folder).unwrap();
        let custom = TASK.replace(
            "## Description",
            "## Description\n\nTeam note: link the ticket.",
        );
        std::fs::write(folder.join("task.md"), &custom).unwrap();
        assert_eq!(
            load(dir.path(), RecordKind::Task, TASK),
            (custom, TemplateSource::Project)
        );
        std::fs::write(folder.join("task.md"), "# not a template\n").unwrap();
        let (text, source) = load(dir.path(), RecordKind::Task, TASK);
        assert_eq!(text, TASK);
        assert!(
            matches!(source, TemplateSource::Fallback(reason) if reason.contains("not a usable task template"))
        );
    }
}
