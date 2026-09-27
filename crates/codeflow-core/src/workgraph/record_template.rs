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
            "{{UID}}",
            "{{TITLE_YAML}}",
            "{{DATE}}",
            "{{EPIC_ID}}",
            "{{STANDALONE_REASON}}",
            "{{TARGET_BRANCH}}",
        ],
        RecordKind::Epic | RecordKind::Spec => {
            &["{{NNN}}", "{{UID}}", "{{TITLE_YAML}}", "{{DATE}}"]
        }
    }
}

/// One synthetic allocation a template is rendered with. Two different
/// allocations per kind catch a template that hard-codes a value next to
/// its placeholder (`id: TSK-999 # {{NNN}}`).
struct Trial {
    nnn: &'static str,
    epic: Option<&'static str>,
    reason: Option<&'static str>,
    target: &'static str,
    title: &'static str,
}

const TRIALS: [Trial; 2] = [
    Trial {
        nnn: "986",
        epic: Some("EPC-986"),
        reason: None,
        target: "integration/EPC-986-template-check",
        title: "template check one",
    },
    Trial {
        nnn: "987",
        epic: None,
        reason: Some("template check reason"),
        target: "integration/EPC-987-template-check",
        title: "template check two",
    },
];

/// Check that `text` is a usable template for `kind`: it carries every
/// placeholder, and a record rendered from it passes the record validator
/// and carries the allocated id, uid, title, parent and target in the fields
/// that own them (TSK-107 review F4).
///
/// # Errors
/// The first reason it is not: a missing placeholder, a field that does not
/// carry its allocated value, or the validator's findings.
pub fn check(kind: RecordKind, text: &str) -> Result<(), String> {
    if let Some(missing) = required_placeholders(kind)
        .iter()
        .find(|p| !text.contains(**p))
    {
        return Err(format!("missing placeholder {missing}"));
    }
    for trial in &TRIALS {
        check_trial(kind, text, trial)?;
    }
    Ok(())
}

fn check_trial(kind: RecordKind, text: &str, trial: &Trial) -> Result<(), String> {
    static CHECKS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let (prefix, dir) = match kind {
        RecordKind::Epic => ("EPC", "epics"),
        RecordKind::Spec => ("SPC", "specs"),
        RecordKind::Task => ("TSK", "tasks"),
    };
    let id = format!("{prefix}-{}", trial.nnn);
    let uid = crate::ids::new_uid();
    let yaml = |value: &str| serde_json::to_string(value).unwrap_or_default();
    let mut ctx = TemplateContext::new();
    let (title_yaml, reason_yaml, target_yaml) = (
        yaml(trial.title),
        trial.reason.map_or_else(|| "null".to_string(), yaml),
        yaml(trial.target),
    );
    for (key, value) in [
        ("NNN", trial.nnn),
        ("UID", uid.as_str()),
        ("TITLE", trial.title),
        ("TITLE_YAML", title_yaml.as_str()),
        ("DATE", "2026-01-01"),
        ("EPIC_ID", trial.epic.unwrap_or("null")),
        ("STANDALONE_REASON", reason_yaml.as_str()),
        ("TARGET_BRANCH", target_yaml.as_str()),
    ] {
        ctx.set(key, value);
    }
    let rendered = ctx.substitute(text);
    check_fields(kind, &rendered, &id, &uid, trial)?;
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
    let path = folder.join(format!("{id}.md"));
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

/// The rendered frontmatter carries each allocated value in its own field.
fn check_fields(
    kind: RecordKind,
    rendered: &str,
    id: &str,
    uid: &str,
    trial: &Trial,
) -> Result<(), String> {
    let yaml = rendered
        .strip_prefix("---\n")
        .and_then(|rest| rest.split_once("\n---\n"))
        .map(|(yaml, _)| yaml)
        .ok_or("no frontmatter")?;
    let data: serde_yaml::Mapping =
        serde_yaml::from_str(yaml).map_err(|_| "the frontmatter does not parse".to_string())?;
    let field = |name: &str| match data.get(name) {
        Some(serde_yaml::Value::String(value)) => Some(value.clone()),
        _ => None,
    };
    let mut expected: Vec<(&str, Option<String>)> = vec![
        ("id", Some(id.to_string())),
        ("uid", Some(uid.to_string())),
        ("title", Some(trial.title.to_string())),
    ];
    if kind == RecordKind::Task {
        expected.extend([
            ("epic_id", trial.epic.map(str::to_string)),
            ("standalone_reason", trial.reason.map(str::to_string)),
            ("integration_target", Some(trial.target.to_string())),
        ]);
    }
    for (name, want) in expected {
        if field(name) != want {
            return Err(format!(
                "`{name}` does not carry the value `new` allocates (its placeholder is not in the field's value)"
            ));
        }
    }
    Ok(())
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

    /// Codex review F4: a placeholder moved into a comment beside a fixed
    /// value is present but not used; the template falls back.
    #[test]
    fn placeholders_only_in_comments_are_refused() {
        let cases = [
            (
                RecordKind::Task,
                TASK.replace(
                    "integration_target: {{TARGET_BRANCH}}",
                    "integration_target: main # {{TARGET_BRANCH}}",
                ),
                "integration_target",
            ),
            (
                RecordKind::Task,
                TASK.replace("id: TSK-{{NNN}}", "id: TSK-999 # {{NNN}}"),
                "`id`",
            ),
            (
                RecordKind::Task,
                TASK.replace(
                    "uid: {{UID}}",
                    "uid: 0f8c6e2a-1b3d-4c5e-8f90-a1b2c3d4e5f6 # {{UID}}",
                ),
                "`uid`",
            ),
            (
                RecordKind::Task,
                TASK.replace("epic_id: {{EPIC_ID}}", "epic_id: EPC-001 # {{EPIC_ID}}"),
                "`epic_id`",
            ),
            (
                RecordKind::Task,
                TASK.replace(
                    "standalone_reason: {{STANDALONE_REASON}}",
                    "standalone_reason: null # {{STANDALONE_REASON}}",
                ),
                "`standalone_reason`",
            ),
            (
                RecordKind::Epic,
                EPIC.replace("id: EPC-{{NNN}}", "id: EPC-001 # {{NNN}}"),
                "`id`",
            ),
            (
                RecordKind::Spec,
                SPEC.replace("title: {{TITLE_YAML}}", "title: Fixed # {{TITLE_YAML}}"),
                "`title`",
            ),
        ];
        for (kind, template, field) in cases {
            let error = check(kind, &template).unwrap_err();
            assert!(error.contains(field), "{field}: {error}");
        }
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
