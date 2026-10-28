//! The Outcome section's adoption marker (TSK-435).
//!
//! A task or epic states, under `## Outcome`, one line saying what someone
//! can do once it lands. The rule binds by record identity: when a project
//! first takes a release that has the section, `codeflow update` (or a
//! fresh `codeflow init`) writes two top-level keys into
//! `.codeflow/project.toml`: `outcome_present_uids`, the `uid` of every
//! task and epic record then in the working tree, which is the marker, and
//! `outcome_required_since`, the project's `HEAD` at that moment when it
//! has one. They are top-level keys, never a table, so writing them
//! introduces no new table scope. A record whose uid is listed is "present
//! at adoption" and only warns without the section; any other open record
//! must state it. With no marker every missing section warns and nothing
//! refuses.

use std::collections::BTreeSet;
use std::path::Path;

/// The marker: the uids of the records present at adoption.
pub const MARKER_KEY: &str = "outcome_present_uids";
/// The project's `HEAD` when the marker was written, when it had one.
pub const COMMIT_KEY: &str = "outcome_required_since";

/// How a project stands toward the Outcome rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Adoption {
    /// No marker: every missing Outcome warns, nothing refuses.
    Unmarked,
    /// The uids of the records present when the project adopted the rule.
    Marked(BTreeSet<String>),
    /// A marker or state file that cannot be read, with why; the rule
    /// fails closed and refuses a record without the section.
    Unreadable(String),
}

impl Adoption {
    /// The project's adoption, read from `.codeflow/project.toml` under
    /// `repo_root`. A file proven absent is [`Adoption::Unmarked`]; one
    /// that exists but cannot be read is [`Adoption::Unreadable`].
    #[must_use]
    pub fn of(repo_root: &Path) -> Self {
        let path = repo_root.join(crate::scaffold::state::PROJECT_TOML);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) => {
                return match crate::absence::proven_absent(&path) {
                    Ok(true) => Self::Unmarked,
                    Ok(false) => Self::Unreadable(format!("cannot read project.toml: {error}")),
                    Err(absence) => Self::Unreadable(format!(
                        "cannot establish whether project.toml exists: {absence}"
                    )),
                }
            }
        };
        match toml::from_str::<toml::Table>(&text) {
            Ok(table) => Self::in_table(&table),
            Err(error) => Self::Unreadable(format!("project.toml is not valid TOML: {error}")),
        }
    }

    /// The adoption a parsed `project.toml` records.
    #[must_use]
    pub fn in_table(table: &toml::Table) -> Self {
        let shape = || {
            Self::Unreadable(format!(
                "`{MARKER_KEY}` in .codeflow/project.toml must be a list of uid strings"
            ))
        };
        let Some(uids) = table.get(MARKER_KEY) else {
            return Self::Unmarked;
        };
        let Some(uids) = uids.as_array() else {
            return shape();
        };
        let mut present = BTreeSet::new();
        for uid in uids {
            let Some(uid) = uid.as_str() else {
                return shape();
            };
            present.insert(uid.to_string());
        }
        Self::Marked(present)
    }
}

/// The `uid` of every task and epic record in the working tree of the
/// project at `repo_root`, committed or not. A record with no `uid`, or
/// whose frontmatter does not parse, is left out: the validator reports
/// the second, and treats the first as present (see the validator).
///
/// # Errors
///
/// A record inventory or a record file that cannot be read.
pub fn present_uids(repo_root: &Path) -> Result<BTreeSet<String>, String> {
    let pm_root = repo_root.join("project-management");
    let mut files = super::layout::task_record_files(&pm_root)
        .map_err(|error| format!("cannot list task records: {error}"))?;
    files.extend(
        super::layout::epic_record_files(&pm_root)
            .map_err(|error| format!("cannot list epic records: {error}"))?,
    );
    let mut uids = BTreeSet::new();
    for path in files {
        let content = std::fs::read(&path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        if let Ok((data, _)) = crate::validate::parse_frontmatter(&content) {
            let uid = crate::validate::get_string_field(&data, "uid");
            if !uid.is_empty() {
                uids.insert(uid);
            }
        }
    }
    Ok(uids)
}

/// `table` with the marker added: `uids`, and `commit` when one is known.
#[must_use]
pub fn with_marker(
    mut table: toml::Table,
    commit: Option<&str>,
    uids: &BTreeSet<String>,
) -> toml::Table {
    if let Some(commit) = commit {
        table.insert(COMMIT_KEY.into(), toml::Value::String(commit.to_string()));
    }
    table.insert(
        MARKER_KEY.into(),
        toml::Value::Array(uids.iter().cloned().map(toml::Value::String).collect()),
    );
    table
}

/// The refusals for a change to the marker, judged against the trusted
/// target (TSK-435): `target` and `head` are the parsed
/// `.codeflow/project.toml` at the target tip and at the change's head,
/// `None` where the file is absent. A target with no marker admits any head
/// marker: that is the adoption write. Once the target has a marker, the
/// head's list may only keep or drop uids the target lists; a uid the
/// change adds, a removed or reshaped marker, or a target marker that
/// cannot be read is refused, so a change cannot list its own record as
/// present to turn the Outcome refusal into a warning.
#[must_use]
pub fn marker_change_errors(
    target: Option<&toml::Table>,
    head: Option<&toml::Table>,
) -> Vec<String> {
    let at = |table: Option<&toml::Table>| table.map_or(Adoption::Unmarked, Adoption::in_table);
    let base = match at(target) {
        Adoption::Unmarked => return Vec::new(),
        Adoption::Unreadable(why) => {
            return vec![format!(
                "the target's {MARKER_KEY} cannot be read, so no change to it can be judged: {why}"
            )]
        }
        Adoption::Marked(base) => base,
    };
    match at(head) {
        Adoption::Unmarked => vec![format!(
            "this change removes {MARKER_KEY}, which the target records; the marker stays once written, and dropping it would turn every Outcome refusal into a warning"
        )],
        Adoption::Unreadable(why) => vec![format!("this change leaves {MARKER_KEY} unreadable: {why}")],
        Adoption::Marked(head) => {
            let added: Vec<&str> = head.difference(&base).map(String::as_str).collect();
            if added.is_empty() {
                Vec::new()
            } else {
                vec![format!(
                    "this change adds uids to {MARKER_KEY} that the target does not list ({}); the list names only records present when the project adopted the Outcome section, so a record made later states its Outcome instead",
                    added.join(", ")
                )]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adoption(text: &str) -> Adoption {
        Adoption::in_table(&toml::from_str(text).unwrap())
    }

    #[test]
    fn the_marker_reads_its_uids_and_refuses_another_shape() {
        assert_eq!(adoption("tier = \"full\"\n"), Adoption::Unmarked);
        assert_eq!(
            adoption("outcome_required_since = \"abc\"\n"),
            Adoption::Unmarked
        );
        assert_eq!(
            adoption("outcome_required_since = \"abc\"\noutcome_present_uids = [\"a\", \"b\"]\n"),
            Adoption::Marked(["a".to_string(), "b".to_string()].into())
        );
        assert_eq!(
            adoption("outcome_present_uids = []\n"),
            Adoption::Marked(BTreeSet::new())
        );
        for broken in [
            "outcome_present_uids = \"x\"\n",
            "outcome_present_uids = [1]\n",
            "[outcome_present_uids]\nuids = []\n",
        ] {
            assert!(
                matches!(adoption(broken), Adoption::Unreadable(_)),
                "{broken}"
            );
        }
    }

    #[test]
    fn a_written_marker_reads_back() {
        let uids: BTreeSet<String> = ["u1".to_string(), "u2".to_string()].into();
        let table = with_marker(toml::Table::new(), Some("abc"), &uids);
        let text = crate::scaffold::state::state_text(&table).unwrap();
        // The marker opens no table: in a file with none, a key appended
        // after it is still top-level.
        let back: toml::Table = toml::from_str(&format!("{text}appended = 1\n")).unwrap();
        assert_eq!(Adoption::in_table(&back), Adoption::Marked(uids));
        assert_eq!(back["outcome_required_since"].as_str(), Some("abc"));
        assert_eq!(back["appended"].as_integer(), Some(1));
    }

    fn table(text: &str) -> toml::Table {
        toml::from_str(text).unwrap()
    }

    #[test]
    fn a_change_may_keep_or_drop_listed_uids_but_never_add_one() {
        let base = table("outcome_present_uids = [\"a\", \"b\"]\n");
        let same = table("x = 1\noutcome_present_uids = [\"b\", \"a\"]\n");
        let dropped = table("outcome_present_uids = [\"a\"]\n");
        let added = table("outcome_present_uids = [\"a\", \"b\", \"mine\"]\n");
        let removed = table("tier = \"full\"\n");
        assert!(marker_change_errors(Some(&base), Some(&same)).is_empty());
        assert!(marker_change_errors(Some(&base), Some(&dropped)).is_empty());
        let refused = marker_change_errors(Some(&base), Some(&added));
        assert_eq!(refused.len(), 1, "{refused:?}");
        assert!(refused[0].contains("(mine)"), "{refused:?}");
        assert_eq!(marker_change_errors(Some(&base), Some(&removed)).len(), 1);
        assert_eq!(marker_change_errors(Some(&base), None).len(), 1);
        // The adoption write: a target with no marker admits any list.
        assert!(marker_change_errors(Some(&removed), Some(&added)).is_empty());
        assert!(marker_change_errors(None, Some(&added)).is_empty());
        // A target marker that cannot be read refuses.
        let broken = table("outcome_present_uids = \"x\"\n");
        assert_eq!(marker_change_errors(Some(&broken), Some(&same)).len(), 1);
    }
}
