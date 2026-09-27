//! Status verbs (`task status`, `epic status`, `spec status`; SPC-013 R-34,
//! R-35): safe editors that write only the status and the sections the
//! transition needs, judged by [`judge_change`] exactly as a hand edit is.
//!
//! A verb reads the record, builds the proposed text, asks the lifecycle
//! judge for a verdict against the file it replaces, and writes only a clean
//! proposal. The write re-reads the file, refuses when its digest changed
//! since the read (a concurrent edit), and replaces it atomically through a
//! temporary file in the same directory.

use std::io::Write;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use thiserror::Error;

use super::lifecycle::{judge_change, working_context, Baseline, ChangeContext, Graph, RecordView};
use super::record_text::{scan_record, section_span, LineKind};
use super::work_start::RecordKind;

/// What a status verb was asked to do.
#[derive(Debug, Clone, Default)]
pub struct StatusChange {
    /// The requested status.
    pub target: String,
    /// Blocker reason, cancellation reason or reopen reason.
    pub reason: Option<String>,
    /// Blocker owner.
    pub owner: Option<String>,
    /// Blocker revisit event.
    pub revisit: Option<String>,
    /// Where a cancelled record's scope went.
    pub scope: Option<String>,
    /// The successor spec of a superseded spec.
    pub by: Option<String>,
    /// An acceptance block (the text between the fences) to add on completion.
    pub acceptance: Option<String>,
}

/// A written status change.
#[derive(Debug, Clone)]
pub struct VerbOutcome {
    pub path: PathBuf,
    pub from: String,
    pub to: String,
    /// Non-blocking findings of the judge.
    pub warnings: Vec<String>,
}

/// Why a verb wrote nothing.
#[derive(Debug, Error)]
pub enum VerbError {
    #[error("{0} not found under project-management/")]
    NotFound(String),
    #[error("{id} is already {status}")]
    NoChange { id: String, status: String },
    #[error("`{verb} status` does not write `{target}`; it writes {allowed}")]
    Vocabulary {
        verb: &'static str,
        target: String,
        allowed: &'static str,
    },
    #[error("refused:\n  {}", .0.join("\n  "))]
    Refused(Vec<String>),
    #[error("{} changed while the verb ran; nothing was written, rerun it", .0.display())]
    Concurrent(PathBuf),
    #[error("io error on {}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> VerbError + '_ {
    move |source| VerbError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// The statuses each verb writes. `in_progress` is never written (R-28);
/// spec `implemented` is derived (R-32).
fn vocabulary(kind: RecordKind) -> (&'static str, &'static [&'static str], &'static str) {
    match kind {
        RecordKind::Task => (
            "task",
            &["todo", "blocked", "complete", "cancelled"],
            "todo, blocked, complete or cancelled",
        ),
        RecordKind::Epic => (
            "epic",
            &["complete", "cancelled", "archived"],
            "complete, cancelled or archived",
        ),
        RecordKind::Spec => (
            "spec",
            &["approved", "superseded"],
            "approved or superseded",
        ),
    }
}

/// Change one record's status through the shared judge.
///
/// # Errors
///
/// [`VerbError`] when the record is missing, the status is outside the
/// verb's vocabulary, the judge refuses the change, the file changed during
/// the verb, or the write fails. Nothing is written on any error.
pub fn set_status(
    repo_root: &Path,
    kind: RecordKind,
    id: &str,
    change: &StatusChange,
) -> Result<VerbOutcome, VerbError> {
    let (verb, allowed, allowed_text) = vocabulary(kind);
    if !allowed.contains(&change.target.as_str()) {
        return Err(VerbError::Vocabulary {
            verb,
            target: change.target.clone(),
            allowed: allowed_text,
        });
    }
    let graph = Graph::from_worktree(repo_root);
    let record = graph
        .records
        .get(id)
        .filter(|record| record.kind == kind)
        .ok_or_else(|| VerbError::NotFound(id.to_string()))?;
    let path = repo_root.join(&record.path);
    let digest = Sha256::digest(record.content.as_bytes());
    if record.status == change.target {
        return Err(VerbError::NoChange {
            id: id.to_string(),
            status: record.status.clone(),
        });
    }
    let proposed = propose(record, change).map_err(|problem| VerbError::Refused(vec![problem]))?;
    let after = RecordView::parse(kind, &record.path, &proposed)
        .map_err(|problem| VerbError::Refused(vec![problem]))?;
    let baseline = Baseline::load(repo_root);
    let refused = baseline.errors();
    if !refused.is_empty() {
        return Err(VerbError::Refused(refused));
    }
    let (base, paths) = working_context(repo_root);
    let verdict = judge_change(
        Some(record),
        &after,
        &graph.with(after.clone()),
        &baseline,
        ChangeContext {
            base: base.as_ref(),
            changed_paths: paths.as_deref(),
        },
    );
    if !verdict.is_clean() {
        return Err(VerbError::Refused(verdict.errors));
    }
    let mut warnings = verdict.warnings;
    if kind == RecordKind::Task && change.target == "complete" {
        let findings = binding(repo_root, &graph.with(after.clone()), &after);
        let (policy, _) = crate::hooks::policy::Policy::load_effective(repo_root);
        if !findings.is_empty() {
            if policy.git.work_records_level() == crate::hooks::PolicyLevel::Block {
                let mut refused = findings;
                refused.push(super::acceptance::SCOPE_NOTE.to_string());
                return Err(VerbError::Refused(refused));
            }
            warnings.extend(findings);
        }
    }
    replace_if_unchanged(&path, digest.as_slice(), &proposed)?;
    Ok(VerbOutcome {
        path,
        from: record.status.clone(),
        to: change.target.clone(),
        warnings,
    })
}

/// The binding of a completion to the reviewed commit (R-60), with `HEAD` as
/// the head: the verb runs where the reviewed result is checked out.
fn binding(repo_root: &Path, graph: &Graph, task: &RecordView) -> Vec<String> {
    let Ok(repo) = git2::Repository::discover(repo_root) else {
        return vec!["cannot open the repository to bind the acceptance block".into()];
    };
    let Some(head) = repo
        .head()
        .ok()
        .and_then(|head| head.peel_to_commit().ok())
        .map(|commit| commit.id())
    else {
        return vec!["no HEAD commit to bind the acceptance block to".into()];
    };
    let target_tip =
        super::work_start::resolve_work_target(repo_root, task.integration_target.as_deref())
            .and_then(|target| repo.revparse_single(&target).ok())
            .and_then(|object| object.peel_to_commit().ok())
            .map(|commit| commit.id());
    super::acceptance::bind_completion(&repo, task, graph, head, target_tip)
        .into_iter()
        .map(|finding| format!("{}: {}", finding.rule, finding.message))
        .collect()
}

/// Replace `path` with `content` only when its bytes still hash to
/// `expected` (SHA-256); the new bytes land through an atomic rename.
///
/// # Errors
///
/// [`VerbError::Concurrent`] when the file changed, [`VerbError::Io`] on I/O
/// failure. The temporary file is removed on a failed write.
pub fn replace_if_unchanged(path: &Path, expected: &[u8], content: &str) -> Result<(), VerbError> {
    let current = std::fs::read(path).map_err(io(path))?;
    if Sha256::digest(&current).as_slice() != expected {
        return Err(VerbError::Concurrent(path.to_path_buf()));
    }
    let name = path.file_name().map_or_else(
        || "record".into(),
        |name| name.to_string_lossy().into_owned(),
    );
    let temporary = path.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    let written = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .and_then(|mut file| {
            file.write_all(content.as_bytes())?;
            file.sync_all()
        })
        .and_then(|()| std::fs::rename(&temporary, path));
    if let Err(source) = written {
        let _ = std::fs::remove_file(&temporary);
        return Err(VerbError::Io {
            path: path.to_path_buf(),
            source,
        });
    }
    Ok(())
}

fn required<'a>(value: Option<&'a String>, flag: &str, why: &str) -> Result<&'a str, String> {
    value
        .map(|value| value.trim())
        .filter(|value| !value.is_empty() && !value.contains('\n'))
        .ok_or_else(|| format!("{why} needs {flag} <one line>"))
}

/// Build the proposed record text. Writes only the status and the sections
/// the transition needs.
fn propose(record: &RecordView, change: &StatusChange) -> Result<String, String> {
    let mut content = record.content.clone();
    let from = record.status.as_str();
    match (record.kind, change.target.as_str()) {
        (RecordKind::Task, "blocked") => {
            let reason = required(change.reason.as_ref(), "--reason", "blocking")?;
            let owner = required(change.owner.as_ref(), "--owner", "blocking")?;
            let revisit = required(change.revisit.as_ref(), "--revisit", "blocking")?;
            content = remove_section(&content, "## Blocker");
            content = insert_section_before(
                &content,
                &format!(
                    "## Blocker\n\n- reason: {reason}\n- owner: {owner}\n- revisit: {revisit}\n"
                ),
                "## Closeout",
            );
        }
        (RecordKind::Task, "todo") => {
            if from == "blocked" {
                content = remove_section(&content, "## Blocker");
            }
            if from == "complete" {
                let reason = required(change.reason.as_ref(), "--reason", "reopening")?;
                content = match supersede_active_block(&content, reason) {
                    Some(superseded) => superseded,
                    // A record completed before the migration has no block;
                    // the reopen reason is recorded, no block is invented.
                    None => append_to_section(
                        &content,
                        "## Closeout",
                        &format!("- reopened: {reason}\n"),
                    ),
                };
            }
        }
        (RecordKind::Task | RecordKind::Epic, "complete") => {
            if let Some(block) = &change.acceptance {
                let inner = block.trim_end();
                content =
                    append_to_section(&content, "## Closeout", &format!("```yaml\n{inner}\n```\n"));
            }
        }
        (RecordKind::Task | RecordKind::Epic, "cancelled") => {
            let reason = required(change.reason.as_ref(), "--reason", "cancelling")?;
            let scope = required(change.scope.as_ref(), "--scope", "cancelling")?;
            content = append_to_section(
                &content,
                "## Closeout",
                &format!("- cancelled: {reason}\n- scope: {scope}\n"),
            );
        }
        (RecordKind::Spec, "superseded") => {
            let by = required(change.by.as_ref(), "--by", "superseding")?;
            content = set_frontmatter_value(&content, "superseded_by", by)?;
        }
        _ => {}
    }
    set_frontmatter_value(&content, "status", &change.target)
}

/// Set one top-level frontmatter scalar, keeping an inline comment in its
/// column; add the key before the closing delimiter when absent.
///
/// # Errors
///
/// Returns a message when the frontmatter delimiters are missing.
pub fn set_frontmatter_value(content: &str, key: &str, value: &str) -> Result<String, String> {
    let mut lines: Vec<String> = content.split('\n').map(str::to_owned).collect();
    if lines.first().map(|line| line.trim_end()) != Some("---") {
        return Err("record has no frontmatter".into());
    }
    let close = lines
        .iter()
        .skip(1)
        .position(|line| line.trim_end() == "---")
        .map(|index| index + 1)
        .ok_or("record frontmatter is not closed")?;
    let prefix = format!("{key}:");
    let existing = (1..close).find(|&index| lines[index].starts_with(&prefix));
    match existing {
        Some(index) => {
            let line = &lines[index];
            let after = &line[prefix.len()..];
            let comment = after.find(" #").map(|position| {
                let column = prefix.len() + position + 1;
                (column, after[position + 1..].to_string())
            });
            let mut replacement = format!("{prefix} {value}");
            if let Some((column, comment)) = comment {
                let pad = column.saturating_sub(replacement.len()).max(1);
                replacement.push_str(&" ".repeat(pad));
                replacement.push_str(&comment);
            }
            lines[index] = replacement;
        }
        None => lines.insert(close, format!("{prefix} {value}")),
    }
    Ok(lines.join("\n"))
}

/// Line index range `[start, end)` of a level-two section, heading included,
/// read in Markdown context exactly as the judge reads it.
fn section_range(lines: &[&str], heading: &str) -> Option<(usize, usize)> {
    section_span(&scan_record(lines), heading)
}

fn remove_section(content: &str, heading: &str) -> String {
    let lines: Vec<&str> = content.split('\n').collect();
    let Some((start, end)) = section_range(&lines, heading) else {
        return content.to_string();
    };
    let mut kept: Vec<&str> = lines[..start].to_vec();
    kept.extend_from_slice(&lines[end..]);
    kept.join("\n")
}

fn insert_section_before(content: &str, section: &str, before: &str) -> String {
    let lines: Vec<&str> = content.split('\n').collect();
    match section_range(&lines, before) {
        Some((start, _)) => {
            let mut out: Vec<String> = lines[..start]
                .iter()
                .map(|line| (*line).to_string())
                .collect();
            out.push(section.trim_end().to_string());
            out.push(String::new());
            out.extend(lines[start..].iter().map(|line| (*line).to_string()));
            out.join("\n")
        }
        None => format!("{}\n\n{}\n", content.trim_end(), section.trim_end()),
    }
}

/// Append text at the end of a section, creating the section at the end of
/// the record when it is absent.
fn append_to_section(content: &str, heading: &str, text: &str) -> String {
    let lines: Vec<&str> = content.split('\n').collect();
    match section_range(&lines, heading) {
        Some((_, end)) => {
            let mut head: Vec<&str> = lines[..end].to_vec();
            while head.last().is_some_and(|line| line.trim().is_empty()) {
                head.pop();
            }
            let mut out = head.join("\n");
            out.push_str("\n\n");
            out.push_str(text.trim_end());
            out.push('\n');
            if end < lines.len() {
                out.push('\n');
                out.push_str(&lines[end..].join("\n"));
            }
            out
        }
        None => format!(
            "{}\n\n{heading}\n\n{}\n",
            content.trim_end(),
            text.trim_end()
        ),
    }
}

/// Mark the active acceptance block superseded with the reopen reason,
/// keeping every other byte of it; `None` when the Closeout has no active
/// block (a record completed before the migration).
fn supersede_active_block(content: &str, reason: &str) -> Option<String> {
    let lines: Vec<&str> = content.split('\n').collect();
    let scanned = scan_record(&lines);
    let (start, end) = section_span(&scanned, "## Closeout")?;
    let mut index = start;
    while index < end {
        let open = &scanned[index];
        index += 1;
        if open.kind != LineKind::FenceOpen || open.marker != b'`' || open.info != "yaml" {
            continue;
        }
        let body_end = (index..end).find(|&at| scanned[at].kind != LineKind::FenceContent)?;
        if scanned[body_end].kind != LineKind::FenceClose {
            return None;
        }
        let header = (index..body_end).find(|&at| !scanned[at].visible.trim().is_empty());
        if let Some(header) = header.filter(|&at| scanned[at].visible.trim_end() == "acceptance:") {
            let mut out: Vec<String> = lines.iter().map(|line| (*line).to_string()).collect();
            out[header] = "acceptance_superseded:".to_string();
            out.insert(header + 1, format!("  reason: {reason}"));
            return Some(out.join("\n"));
        }
        index = body_end + 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontmatter_value_keeps_the_comment_column() {
        let content = "---\nid: TSK-001\nstatus: todo             # todo | blocked\n---\nbody\n";
        let updated = set_frontmatter_value(content, "status", "blocked").unwrap();
        assert!(updated.contains("status: blocked          # todo | blocked\n"));
        let added = set_frontmatter_value(content, "superseded_by", "SPC-002").unwrap();
        assert!(added.contains("superseded_by: SPC-002\n---"));
        assert!(set_frontmatter_value("no frontmatter", "status", "x").is_err());
    }

    #[test]
    fn sections_are_replaced_and_appended_without_touching_others() {
        let content = "---\nid: TSK-001\n---\n\n## Description\n\nText\n\n## Blocker\n\n- reason: a\n\n## Closeout\n\nPending.\n";
        let removed = remove_section(content, "## Blocker");
        assert!(!removed.contains("## Blocker"));
        assert!(removed.contains("## Description\n\nText\n\n## Closeout"));
        let inserted =
            insert_section_before(&removed, "## Blocker\n\n- reason: b\n", "## Closeout");
        assert!(inserted.contains("## Blocker\n\n- reason: b\n\n## Closeout"));
        let appended = append_to_section(&inserted, "## Closeout", "- cancelled: x\n");
        assert!(appended.ends_with("Pending.\n\n- cancelled: x\n"));
        let created =
            append_to_section("---\nid: EPC-001\n---\n\n## Summary\n", "## Closeout", "x");
        assert!(created.ends_with("## Summary\n\n## Closeout\n\nx\n"));
    }

    #[test]
    fn reopening_supersedes_the_real_block_not_an_example() {
        let block = "acceptance:\n  reviewed: x\n";
        let hidden = format!(
            "---\nid: TSK-001\n---\n\n## Closeout\n\n<!--\n```yaml\n{block}```\n-->\n\n````text\n```yaml\n{block}```\n````\n"
        );
        assert_eq!(supersede_active_block(&hidden, "regression"), None);
        let content = format!("{hidden}\n```yaml\n{block}```\n");
        let reopened = supersede_active_block(&content, "regression").unwrap();
        assert_eq!(reopened.matches("acceptance_superseded:").count(), 1);
        assert!(reopened.ends_with(
            "```yaml\nacceptance_superseded:\n  reason: regression\n  reviewed: x\n```\n"
        ));
    }

    #[test]
    fn a_concurrent_edit_is_detected_by_digest() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("TSK-001.md");
        std::fs::write(&path, "one").unwrap();
        let stale = Sha256::digest(b"one");
        std::fs::write(&path, "two").unwrap();
        assert!(matches!(
            replace_if_unchanged(&path, stale.as_slice(), "three"),
            Err(VerbError::Concurrent(_))
        ));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "two");
        let fresh = Sha256::digest(b"two");
        replace_if_unchanged(&path, fresh.as_slice(), "three").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "three");
        let leftovers = std::fs::read_dir(dir.path()).unwrap().count();
        assert_eq!(leftovers, 1, "no temporary file is left behind");
    }
}
