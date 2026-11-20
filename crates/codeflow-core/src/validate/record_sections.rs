//! Extra sections a project requires in its task, epic and spec records
//! (TSK-1421).
//!
//! A project lists them in `.codeflow/policy.json` as
//! `git.task_required_sections`, `git.epic_required_sections` and
//! `git.spec_required_sections`. `CodeFlow`'s own required sections
//! ([`super::ValidateOptions`]) are checked as before and stay required
//! whatever the project lists.
//!
//! The lists a run enforces are the union of a trusted policy and the
//! working copy's policy ([`Required`]): a change can add a section at once,
//! but cannot drop one it is judged by. `codeflow ci` trusts the policy at
//! the range's base ([`Required::judged`]); `validate --docs` and
//! `work start` trust the landed policy ([`Required::landed`]).
//!
//! A record that lacks a listed section is refused when it is new or has
//! changed since the trusted commit, and warned about when its bytes equal
//! its copy there, so a newly listed section never fails a record nobody
//! touched.
use std::fmt::Write as _;
use std::path::Path;

use crate::hooks::{GitPolicy, Policy};
use crate::workgraph::RecordKind;

/// The extra section names a project lists, per record kind, as heading
/// names without the leading `##`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectSections {
    pub task: Vec<String>,
    pub epic: Vec<String>,
    pub spec: Vec<String>,
}

impl ProjectSections {
    /// The lists one policy declares, each name stripped of the spaces and
    /// tabs `CommonMark` strips from a heading's text; names that could
    /// never be a heading are dropped here because the schema already
    /// refuses them ([`is_valid_name`]).
    #[must_use]
    pub fn from_git(git: &GitPolicy) -> Self {
        let clean = |names: &[String]| {
            let mut kept = Vec::new();
            for name in names.iter().filter(|name| is_valid_name(name)) {
                let name = name.trim_matches([' ', '\t']).to_string();
                if !kept.contains(&name) {
                    kept.push(name);
                }
            }
            kept
        };
        Self {
            task: clean(&git.task_required_sections),
            epic: clean(&git.epic_required_sections),
            spec: clean(&git.spec_required_sections),
        }
    }

    /// Add every name `other` lists that this does not, keeping order.
    pub fn join(&mut self, other: &Self) {
        for (mine, theirs) in [
            (&mut self.task, &other.task),
            (&mut self.epic, &other.epic),
            (&mut self.spec, &other.spec),
        ] {
            for name in theirs {
                if !mine.contains(name) {
                    mine.push(name.clone());
                }
            }
        }
    }

    /// Whether no kind lists a section.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.task.is_empty() && self.epic.is_empty() && self.spec.is_empty()
    }

    /// The names listed for `kind`.
    #[must_use]
    pub fn for_kind(&self, kind: RecordKind) -> &[String] {
        match kind {
            RecordKind::Task => &self.task,
            RecordKind::Epic => &self.epic,
            RecordKind::Spec => &self.spec,
        }
    }
}

/// Whether `name` can be a section name: not blank, on one line, and not
/// itself starting with `#`.
#[must_use]
pub fn is_valid_name(name: &str) -> bool {
    let trimmed = name.trim();
    !trimmed.is_empty() && !name.contains(['\n', '\r']) && !trimmed.starts_with('#')
}

/// The policy key that lists `kind`'s sections.
#[must_use]
pub fn key(kind: RecordKind) -> &'static str {
    match kind {
        RecordKind::Task => "git.task_required_sections",
        RecordKind::Epic => "git.epic_required_sections",
        RecordKind::Spec => "git.spec_required_sections",
    }
}

/// The sections one run enforces, and the trusted commit that decides
/// whether a record is unchanged.
#[derive(Debug, Clone, Default)]
pub struct Required {
    pub sections: ProjectSections,
    /// The trusted commit, as a full sha; `None` when there is none, and
    /// then every record lacking a listed section is refused.
    pub reference: Option<String>,
    /// Why the trusted policy could not be read, when the run fell back;
    /// shown as a warning while any section is listed.
    pub doubt: Option<String>,
    /// Why the working copy's policy could not be read, when it could not;
    /// the trusted lists still apply. `validate --docs` and `work start`
    /// report that failure themselves; the `new` verbs refuse on it.
    pub unread_policy: Option<String>,
}

impl Required {
    /// No project sections: the run judges `CodeFlow`'s own sections only.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// `codeflow ci`'s lists: the judging policy (read at the range's base
    /// or `--policy-from`) joined with the working copy's, with the base as
    /// the trusted commit.
    #[must_use]
    pub fn judged(
        judging: &GitPolicy,
        working: &ProjectSections,
        reference: Option<String>,
    ) -> Self {
        let mut sections = ProjectSections::from_git(judging);
        sections.join(working);
        Self {
            sections,
            reference,
            doubt: None,
            unread_policy: None,
        }
    }

    /// The lists for `validate --docs` and `work start`: the landed policy
    /// ([`crate::hooks::landed_policy::load_local`]) joined with the working
    /// copy's, with the landed commit as the trusted commit. When the landed
    /// policy cannot be read, `HEAD`'s committed policy stands in for it and
    /// [`Required::doubt`] says why; a `HEAD` that cannot be read leaves no
    /// trusted commit, so every record lacking a section is refused.
    #[must_use]
    pub fn landed(root: &Path) -> Self {
        let mut required = Self::default();
        match Policy::load_effective(root) {
            Ok((working, _)) => required.sections = ProjectSections::from_git(&working.git),
            // The trusted lists still apply; the caller reports the failure
            // ([`Required::unread_policy`]).
            Err(error) => required.unread_policy = Some(error),
        }
        match crate::hooks::landed_policy::load_local(root) {
            Ok(landed) => {
                required
                    .sections
                    .join(&ProjectSections::from_git(&landed.policy.git));
                required.reference = landed.commit.map(|commit| commit.to_string());
            }
            Err(error) => {
                required.doubt = Some(error);
                if let Some((head, policy)) = head_policy(root) {
                    required
                        .sections
                        .join(&ProjectSections::from_git(&policy.git));
                    required.reference = Some(head);
                }
            }
        }
        required
    }

    /// The names `body` lacks for `kind`, leaving out any `CodeFlow` already
    /// requires (`own`, written `## <name>`).
    #[must_use]
    pub fn missing(&self, kind: RecordKind, own: &[String], body: &str) -> Vec<String> {
        let headings = heading_lines(body);
        self.sections
            .for_kind(kind)
            .iter()
            .filter(|name| {
                !own.iter()
                    .any(|own| own.trim_matches([' ', '\t']) == format!("## {name}"))
            })
            .filter(|name| !headings.iter().any(|heading| heading == *name))
            .cloned()
            .collect()
    }

    /// Whether the file at `relative` (from the repository root) holds
    /// exactly `bytes` at the trusted commit, line endings aside. Anything
    /// that cannot be read counts as changed, so the record is refused.
    #[must_use]
    pub fn unchanged(&self, repo_root: &Path, relative: &Path, bytes: &[u8]) -> bool {
        let Some(reference) = &self.reference else {
            return false;
        };
        let Ok(repo) = git2::Repository::open(repo_root) else {
            return false;
        };
        let Ok(oid) = git2::Oid::from_str(reference) else {
            return false;
        };
        let Some(blob) = repo
            .find_commit(oid)
            .and_then(|commit| commit.tree())
            .and_then(|tree| tree.get_path(relative))
            .and_then(|entry| entry.to_object(&repo))
            .ok()
            .and_then(|object| object.into_blob().ok())
        else {
            return false;
        };
        without_cr(blob.content()) == without_cr(bytes)
    }

    /// The trusted commit, shortened for messages.
    #[must_use]
    pub fn reference_short(&self) -> String {
        self.reference
            .as_deref()
            .map_or_else(String::new, |sha| sha[..sha.len().min(9)].to_string())
    }
}

/// `template` with a `## <name>` section for each name `required` lists for
/// `kind` that the template lacks, so a new record passes the check it will
/// meet. The sections go before `## Closeout` when the template has one,
/// else at the end.
#[must_use]
pub fn with_sections(template: &str, kind: RecordKind, required: &Required) -> String {
    let opts = super::ValidateOptions::default();
    let own = match kind {
        RecordKind::Task => &opts.task_required_sections,
        RecordKind::Epic => &opts.epic_required_sections,
        RecordKind::Spec => &opts.spec_required_sections,
    };
    let missing = required.missing(kind, own, template);
    if missing.is_empty() {
        return template.to_string();
    }
    let key = key(kind);
    let mut added = String::new();
    for name in &missing {
        let _ = write!(
            added,
            "## {name}\n\n<!-- Required by the project's policy (`{key}`). -->\n\n"
        );
    }
    if let Some((_, offset)) = headings(template)
        .into_iter()
        .find(|(name, _)| name == "Closeout")
    {
        let mut text = template.to_string();
        text.insert_str(offset, &added);
        return text;
    }
    let mut text = template.to_string();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    if !text.is_empty() && !text.ends_with("\n\n") {
        text.push('\n');
    }
    // `added` ends each section with a blank line; the record ends with one
    // newline after the last section.
    text.push_str(added.strip_suffix('\n').unwrap_or(&added));
    text
}

/// `HEAD`'s commit and its committed policy, when both can be read.
fn head_policy(root: &Path) -> Option<(String, Policy)> {
    let repo = git2::Repository::discover(root).ok()?;
    let head = repo.head().ok()?.peel_to_commit().ok()?.id();
    let policy = crate::hooks::landed_policy::checked_policy_at(&repo, head).ok()?;
    Some((head.to_string(), policy))
}

fn without_cr(bytes: &[u8]) -> Vec<u8> {
    bytes
        .iter()
        .copied()
        .filter(|byte| *byte != b'\r')
        .collect()
}

/// The text of each level-2 heading in `body`, trimmed, as `CommonMark`
/// parses it ([`headings`]).
fn heading_lines(body: &str) -> Vec<String> {
    headings(body).into_iter().map(|(name, _)| name).collect()
}

/// Each level-2 ATX heading `CommonMark` parses at the top level of `body`,
/// with its text trimmed and the byte offset where its line starts. A `## `
/// line in fenced code, an HTML comment (closed or not) or an HTML block is
/// not a heading, so it never satisfies a listed section. The heading must
/// start at column zero, and a setext heading does not count, as in
/// `codeflow ci`'s pull request body outline.
fn headings(body: &str) -> Vec<(String, usize)> {
    use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};
    let mut found = Vec::new();
    let mut depth: usize = 0;
    let mut heading: Option<(String, usize)> = None;
    for (event, span) in Parser::new(body).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading {
                level: HeadingLevel::H2,
                ..
            }) if depth == 0 && heading.is_none() && atx_at_line_start(body, span.start) => {
                heading = Some((String::new(), span.start));
            }
            Event::End(TagEnd::Heading(_)) if heading.is_some() => {
                if let Some((name, offset)) = heading.take() {
                    found.push((name.trim_matches([' ', '\t']).to_string(), offset));
                }
            }
            Event::Text(text) | Event::Code(text) if heading.is_some() => {
                if let Some((name, _)) = heading.as_mut() {
                    name.push_str(&text);
                }
            }
            // Inline markup inside the heading leaves the depth alone.
            _ if heading.is_some() => {}
            Event::Start(_) => depth += 1,
            Event::End(_) => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    found
}

/// Whether the heading at `offset` starts its line with `#`.
fn atx_at_line_start(body: &str, offset: usize) -> bool {
    let line_start = body[..offset].rfind('\n').map_or(0, |pos| pos + 1);
    offset == line_start && body[offset..].starts_with('#')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn required(task: &[&str]) -> Required {
        Required {
            sections: ProjectSections {
                task: task.iter().map(ToString::to_string).collect(),
                ..ProjectSections::default()
            },
            ..Required::default()
        }
    }

    #[test]
    fn a_heading_matches_only_as_its_own_line() {
        let required = required(&["Risks"]);
        let own = super::super::ValidateOptions::default().task_required_sections;
        assert!(required
            .missing(RecordKind::Task, &own, "## Risks\n\nnone\n")
            .is_empty());
        assert!(required
            .missing(RecordKind::Task, &own, "## Risks  \n")
            .is_empty());
        for body in [
            "## Risks and rollback\n",
            "### Risks\n",
            "text ## Risks\n",
            "```\n## Risks\n```\n",
            "~~~md\n## Risks\n~~~\n",
            "<!--\n## Risks\n-->\n",
            "<!-- opened earlier\nnotes\n## Risks\n-->\n",
            "text\n\n<!-- never closed\n## Risks\n",
            "<div>\n## Risks\n</div>\n",
        ] {
            assert_eq!(
                required.missing(RecordKind::Task, &own, body),
                ["Risks"],
                "{body:?}"
            );
        }
    }

    #[test]
    fn codeflow_own_sections_are_left_to_the_own_check() {
        let required = required(&["Acceptance Criteria", "Risks"]);
        let own = super::super::ValidateOptions::default().task_required_sections;
        assert_eq!(required.missing(RecordKind::Task, &own, ""), ["Risks"]);
    }

    #[test]
    fn names_are_checked_trimmed_and_deduplicated() {
        for bad in ["", "  ", "## Risks", "#Risks", "Risks\nRollback", "Risks\r"] {
            assert!(!is_valid_name(bad), "{bad:?}");
        }
        assert!(is_valid_name(" Risks "));
        let git = GitPolicy {
            task_required_sections: vec![" Risks ".into(), "Risks".into(), String::new()],
            ..GitPolicy::default()
        };
        assert_eq!(ProjectSections::from_git(&git).task, ["Risks"]);
    }

    #[test]
    fn a_template_gains_each_missing_section_before_the_closeout() {
        let required = required(&["Risks", "Description"]);
        let template = "---\nid: x\n---\n\n## Description\n\n## Closeout\n\nend\n";
        let text = with_sections(template, RecordKind::Task, &required);
        assert_eq!(
            text,
            "---\nid: x\n---\n\n## Description\n\n## Risks\n\n<!-- Required by the project's policy (`git.task_required_sections`). -->\n\n## Closeout\n\nend\n"
        );
        let ended = with_sections("## Summary\nbody", RecordKind::Task, &required);
        assert!(ended.ends_with("body\n\n## Risks\n\n<!-- Required by the project's policy (`git.task_required_sections`). -->\n"), "{ended}");
        assert_eq!(with_sections(&text, RecordKind::Task, &required), text);
    }

    #[test]
    fn a_commented_heading_does_not_stop_the_real_section() {
        let required = required(&["Risks"]);
        let template = "## Summary\n\n<!--\n## Risks\n## Closeout\n-->\n\n## Closeout\n\nend\n";
        assert_eq!(
            with_sections(template, RecordKind::Task, &required),
            "## Summary\n\n<!--\n## Risks\n## Closeout\n-->\n\n## Risks\n\n<!-- Required by the project's policy (`git.task_required_sections`). -->\n\n## Closeout\n\nend\n"
        );
    }

    #[test]
    fn the_judged_lists_keep_every_base_section() {
        let base = GitPolicy {
            task_required_sections: vec!["Risks".into(), "Rollback".into()],
            ..GitPolicy::default()
        };
        let head = GitPolicy {
            task_required_sections: vec!["Rollback".into(), "Owner".into()],
            ..GitPolicy::default()
        };
        let required = Required::judged(&base, &ProjectSections::from_git(&head), None);
        assert_eq!(required.sections.task, ["Risks", "Rollback", "Owner"]);
    }
}
