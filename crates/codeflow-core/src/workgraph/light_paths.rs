//! The light planning paths of SPC-013 R-36, R-73 and R-77: a follow-up task
//! filed in one command, an epic created with its integration branch, and an
//! ADR numbered and rendered as `proposed`.
//!
//! ADR numbers come from the highest `ADR-NNNN` visible in `docs/decisions/`
//! plus one. The shared id registry (TSK-101) replaces that scan with a
//! reservation; `next_adr_id` is the one touch point.

use std::fs;
use std::path::{Path, PathBuf};

use crate::workgraph::allocate::{create_task, create_task_with, Allocator, NewRecord};
use crate::workgraph::store::StoreError;

/// A kebab-case slug of a title for branch and file names, at most 48
/// characters, never empty.
#[must_use]
pub fn slug(title: &str) -> String {
    let mut out = String::new();
    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let mut out = out.trim_end_matches('-').to_string();
    if out.len() > 48 {
        out.truncate(48);
        out = out.trim_end_matches('-').to_string();
    }
    if out.is_empty() {
        "work".to_string()
    } else {
        out
    }
}

fn frontmatter_map(path: &Path) -> Result<serde_yaml::Mapping, StoreError> {
    let text = fs::read_to_string(path)?;
    let yaml = text
        .strip_prefix("---\n")
        .and_then(|rest| rest.split_once("\n---").map(|(yaml, _)| yaml))
        .ok_or_else(|| StoreError::Yaml {
            path: path.display().to_string(),
            message: "missing frontmatter".to_string(),
        })?;
    serde_yaml::from_str(yaml).map_err(|error| StoreError::Yaml {
        path: path.display().to_string(),
        message: error.to_string(),
    })
}

fn field(map: &serde_yaml::Mapping, key: &str) -> Option<String> {
    map.get(serde_yaml::Value::String(key.to_string()))
        .and_then(serde_yaml::Value::as_str)
        .map(str::to_owned)
        .filter(|value| !value.trim().is_empty() && value != "null")
}

fn current_branch(repo_root: &Path) -> Option<String> {
    let repo = git2::Repository::discover(repo_root).ok()?;
    let head = repo.head().ok()?;
    head.shorthand().ok().map(str::to_owned)
}

/// File a follow-up of `source_id` (R-73): the new task records
/// `follow_up_of` and inherits the source's integration target. A follow-up
/// of an epic task inherits the epic and is written only on a `plan/`
/// branch, so it lands in the epic's batched amendment. A standalone task
/// never uses a `plan/` branch, because a planning pull request names an
/// epic and it has none. A follow-up of a standalone task is itself a
/// standalone task (R-78): it states the source it follows as its reason,
/// is written on a task branch cut from the target, and lands with its work
/// in its own pull request. Running this on a `plan/` branch for a
/// standalone source is refused.
///
/// # Errors
///
/// Returns not-found for a missing source, invalid-record when the current
/// branch does not suit the source (an epic task's follow-up on a branch
/// that is not a `plan/` branch, a standalone task's follow-up on a `plan/`
/// branch) or the source has no resolvable target, and the allocation errors
/// of [`create_task`].
pub fn create_follow_up(
    repo_root: &Path,
    template: &str,
    source_id: &str,
    title: &str,
) -> Result<NewRecord, StoreError> {
    create_follow_up_with(repo_root, template, source_id, title, None)
}

/// [`create_follow_up`] with the id and `uid` chosen by `allocate`, as the
/// shared registry issues them where durable work is tracked (SPC-013 R-12);
/// `None` takes the next number visible in the checkout.
///
/// # Errors
///
/// As [`create_follow_up`], plus the allocator's error.
pub fn create_follow_up_with(
    repo_root: &Path,
    template: &str,
    source_id: &str,
    title: &str,
    allocate: Option<&mut Allocator<'_>>,
) -> Result<NewRecord, StoreError> {
    let pm_root = repo_root.join("project-management");
    let source = crate::workgraph::layout::task_record_files(&pm_root)
        .into_iter()
        .find(|path| path.file_stem().and_then(|stem| stem.to_str()) == Some(source_id))
        .ok_or_else(|| StoreError::NotFound(format!("task:{source_id}")))?;
    let map = frontmatter_map(&source)?;
    let target = field(&map, "integration_target").ok_or_else(|| {
        StoreError::Invalid(format!("{source_id} has no integration_target to follow"))
    })?;
    let branch = current_branch(repo_root).unwrap_or_default();
    let epic = field(&map, "epic_id");
    let planning = branch.starts_with("plan/");
    if epic.is_some() && !planning {
        return Err(StoreError::Invalid(format!(
            "a follow-up is written on a plan/ branch of '{target}' (current branch '{branch}'); \
             run `git switch -c plan/<slug> {target}` first"
        )));
    }
    if epic.is_none() && planning {
        return Err(StoreError::Invalid(format!(
            "{source_id} is a standalone task, so its follow-up is a standalone task too: \
             its record lands with its work on its own task branch, since a planning pull \
             request names an epic (current branch '{branch}'); run \
             `git switch -c task/<slug> {target}`, file the follow-up there, then \
             `codeflow work claim <TSK-NNN>`"
        )));
    }
    let reason = epic
        .is_none()
        .then(|| format!("follow-up of standalone task {source_id}"));
    let record = match allocate {
        Some(allocate) => create_task_with(
            &pm_root,
            template,
            epic.as_deref(),
            reason.as_deref(),
            Some(&target),
            title,
            allocate,
        )?,
        None => create_task(
            &pm_root,
            template,
            epic.as_deref(),
            reason.as_deref(),
            Some(&target),
            title,
        )?,
    };
    let text = fs::read_to_string(&record.path)?;
    let Some(at) = text.find("\ncreated:") else {
        let _ = fs::remove_file(&record.path);
        return Err(StoreError::Invalid(
            "task template has no `created:` line to anchor follow_up_of".to_string(),
        ));
    };
    let updated = format!(
        "{}\nfollow_up_of: {source_id}      # the task this follows up{}",
        &text[..at],
        &text[at..]
    );
    if let Err(error) = crate::file_lock::atomic_write(&record.path, updated.as_bytes()) {
        let _ = fs::remove_file(&record.path);
        return Err(error.into());
    }
    Ok(record)
}

/// What `epic new --integration` did with the integration branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationBranch {
    /// `integration/EPC-NNN-<slug>`.
    pub name: String,
    /// The target it was cut from.
    pub from: String,
    /// Whether it was pushed to `origin`.
    pub pushed: bool,
}

fn git(repo_root: &Path, args: &[&str]) -> Result<String, String> {
    let out = crate::git::command()
        .arg("-C")
        .arg(repo_root)
        .args(args)
        .output()
        .map_err(|error| error.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Cut `integration/<epic_id>-<slug>` from the protected target (R-77) and
/// push it to `origin` when that remote exists. The branch is non-protected;
/// tasks set it as their `integration_target`.
///
/// # Errors
///
/// Returns the reason when no target resolves, the branch exists, or git
/// refuses to create or push it.
pub fn create_integration_branch(
    repo_root: &Path,
    epic_id: &str,
    title: &str,
) -> Result<IntegrationBranch, String> {
    let from = crate::workgraph::default_work_target(repo_root)
        .ok_or("no main or master branch (local or origin) to cut the integration branch from")?;
    let name = format!("integration/{epic_id}-{}", slug(title));
    git(repo_root, &["branch", &name, &from])?;
    let has_origin = git(repo_root, &["remote"])?
        .lines()
        .any(|remote| remote == "origin");
    if has_origin {
        git(repo_root, &["push", "-u", "origin", &name])
            .map_err(|error| format!("created {name} locally; push to origin failed: {error}"))?;
    }
    Ok(IntegrationBranch {
        name,
        from,
        pushed: has_origin,
    })
}

/// The next `ADR-NNNN` after the highest one in `docs/decisions/`.
#[must_use]
pub fn next_adr_id(repo_root: &Path) -> String {
    let highest = fs::read_dir(repo_root.join("docs/decisions"))
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            name.strip_prefix("ADR-")?.get(..4)?.parse::<u32>().ok()
        })
        .max()
        .unwrap_or(0);
    format!("ADR-{:04}", highest + 1)
}

/// Write the next ADR from `template` with `status: proposed` (R-36),
/// numbered from what the checkout shows.
///
/// # Errors
///
/// Returns an invalid-record error for an empty or multi-line title and an
/// I/O error when the file cannot be created (an existing file is never
/// overwritten).
pub fn create_adr(repo_root: &Path, template: &str, title: &str) -> Result<NewRecord, StoreError> {
    create_adr_with(repo_root, template, title, &mut |_| {
        Ok((next_adr_id(repo_root), crate::ids::new_uid()))
    })
}

/// [`create_adr`] with the id and `uid` chosen by `allocate`, as the shared
/// registry issues them where durable work is tracked (SPC-013 R-12). The
/// `uid` is written after the `id:` line, as on every registry record.
///
/// # Errors
///
/// As [`create_adr`], plus the allocator's error and an invalid-record
/// error for a template without an `id:` frontmatter line.
pub fn create_adr_with(
    repo_root: &Path,
    template: &str,
    title: &str,
    allocate: &mut Allocator<'_>,
) -> Result<NewRecord, StoreError> {
    let title = title.trim();
    if title.is_empty() || title.contains(['\r', '\n']) {
        return Err(StoreError::Invalid(
            "ADR title must be one non-empty line".to_string(),
        ));
    }
    let (id, uid) = allocate(&crate::workgraph::allocate::planning_target(
        &repo_root.join("project-management"),
    ))?;
    let date = crate::workgraph::now_rfc3339()[..10].to_string();
    // The frontmatter title is a YAML scalar; the heading keeps the text.
    let encoded =
        serde_json::to_string(title).map_err(|error| StoreError::Invalid(error.to_string()))?;
    let mut content = template
        .replace("ADR-NNNN", &id)
        .replace(
            "title: <short decision title>",
            &format!("title: {encoded}"),
        )
        .replace("<short decision title>", title)
        .replace("YYYY-MM-DD", &date);
    if let Some(start) = content.find("\nstatus:") {
        let end = content[start + 1..]
            .find('\n')
            .map_or(content.len(), |offset| start + 1 + offset);
        content.replace_range(
            start..end,
            "\nstatus: proposed          # proposed | accepted | superseded",
        );
    }
    content = with_uid(&content, &uid).ok_or_else(|| {
        StoreError::Invalid("the ADR template has no `id:` frontmatter line".to_string())
    })?;
    check_adr(&content, &id, title)?;
    let dir = repo_root.join("docs/decisions");
    fs::create_dir_all(&dir)?;
    let path: PathBuf = dir.join(format!("{id}-{}.md", slug(title)));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    std::io::Write::write_all(&mut file, content.as_bytes())?;
    Ok(NewRecord { id, uid, path })
}

/// The generated ADR must parse and carry what was asked for, before it is
/// written and reported.
/// Insert `uid: <uid>` after the frontmatter `id:` line, unless the
/// template already carries a `uid:` line.
fn with_uid(content: &str, uid: &str) -> Option<String> {
    let front = content.strip_prefix("---\n")?.split_once("\n---\n")?.0;
    if front.lines().any(|line| line.starts_with("uid:")) {
        return Some(content.replace("{{UID}}", uid));
    }
    let at = front.lines().position(|line| line.starts_with("id:"))?;
    let mut lines: Vec<&str> = content.split('\n').collect();
    let line = format!("uid: {uid}");
    lines.insert(at + 2, &line);
    Some(lines.join("\n"))
}

fn check_adr(content: &str, id: &str, title: &str) -> Result<(), StoreError> {
    let invalid =
        |why: &str| StoreError::Invalid(format!("generated {id} is not a valid ADR: {why}"));
    let yaml = content
        .strip_prefix("---\n")
        .and_then(|rest| rest.split_once("\n---\n"))
        .map(|(yaml, _)| yaml)
        .ok_or_else(|| invalid("no frontmatter"))?;
    let data: serde_yaml::Mapping =
        serde_yaml::from_str(yaml).map_err(|error| invalid(&error.to_string()))?;
    let field = |name: &str| data.get(name).and_then(serde_yaml::Value::as_str);
    if field("id") != Some(id) {
        return Err(invalid("the id does not round-trip"));
    }
    if field("title") != Some(title) {
        return Err(invalid("the title does not round-trip"));
    }
    if field("status") != Some("proposed") {
        return Err(invalid("the status is not proposed"));
    }
    if !field("uid").is_some_and(crate::ids::is_uid) {
        return Err(invalid("the uid is missing or malformed"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adr_uid_goes_after_the_id_line_or_fills_the_placeholder() {
        let uid = "0f8c6e2a-1b3d-4c5e-8f90-a1b2c3d4e5f6";
        assert_eq!(
            with_uid("---\nid: ADR-0002\ntitle: x\n---\nid: body\n", uid).unwrap(),
            format!("---\nid: ADR-0002\nuid: {uid}\ntitle: x\n---\nid: body\n")
        );
        assert_eq!(
            with_uid("---\nid: ADR-0002\nuid: {{UID}}\n---\n", uid).unwrap(),
            format!("---\nid: ADR-0002\nuid: {uid}\n---\n")
        );
        assert!(with_uid("---\ntitle: x\n---\n", uid).is_none());
        assert!(with_uid("no frontmatter\nid: x\n", uid).is_none());
    }

    #[test]
    fn slug_is_kebab_case_bounded_and_never_empty() {
        assert_eq!(
            slug("Work classification, for PRs!"),
            "work-classification-for-prs"
        );
        assert_eq!(slug("***"), "work");
        assert!(slug(&"long title ".repeat(10)).len() <= 48);
    }

    #[test]
    fn adr_numbers_follow_the_highest_visible_and_start_proposed() {
        let dir = tempfile::tempdir().unwrap();
        let decisions = dir.path().join("docs/decisions");
        fs::create_dir_all(&decisions).unwrap();
        assert_eq!(next_adr_id(dir.path()), "ADR-0001");
        fs::write(decisions.join("ADR-0007-old.md"), "x").unwrap();
        fs::write(decisions.join("template.md"), "x").unwrap();
        let template = "---\nid: ADR-NNNN\ntitle: <short decision title>\ndate: YYYY-MM-DD\nstatus: accepted          # proposed | accepted | superseded\n---\n\n# ADR-NNNN: <short decision title>\n";
        let record = create_adr(dir.path(), template, "Adopt a registry").unwrap();
        assert_eq!(record.id, "ADR-0008");
        let text = fs::read_to_string(&record.path).unwrap();
        assert!(text.contains("id: ADR-0008\n"), "{text}");
        assert!(text.contains("\nstatus: proposed "), "{text}");
        assert!(text.contains("# ADR-0008: Adopt a registry"), "{text}");
        assert!(record.path.ends_with("ADR-0008-adopt-a-registry.md"));
    }

    #[test]
    fn adr_titles_are_yaml_encoded_and_the_record_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let template = "---\nid: ADR-NNNN\ntitle: <short decision title>\ndate: YYYY-MM-DD\nstatus: accepted          # proposed | accepted | superseded\n---\n\n# ADR-NNNN: <short decision title>\n";
        for title in [
            "Choose storage: keep SQLite",
            "Say \"no\" to 'magic'",
            "Pin #1 # not a comment",
            "- a leading dash, [brackets] and {braces}",
            "yes",
        ] {
            let record = create_adr(dir.path(), template, title).unwrap();
            let text = fs::read_to_string(&record.path).unwrap();
            check_adr(&text, &record.id, title).unwrap();
            assert!(text.contains(&format!(": {title}\n")), "{text}");
        }
        let generated = |title: &str| {
            format!(
                "---\nid: ADR-0001\nuid: 0f8c6e2a-1b3d-4c5e-8f90-a1b2c3d4e5f6\ntitle: {title}\nstatus: proposed\n---\n"
            )
        };
        assert!(
            check_adr(&generated("a: b"), "ADR-0001", "a: b").is_err(),
            "unparseable"
        );
        assert!(
            check_adr(&generated("other"), "ADR-0001", "asked").is_err(),
            "wrong title"
        );
        assert!(check_adr(&generated("asked"), "ADR-0001", "asked").is_ok());
        let no_uid = "---\nid: ADR-0001\ntitle: asked\nstatus: proposed\n---\n";
        assert!(check_adr(no_uid, "ADR-0001", "asked").is_err(), "no uid");
    }
}
