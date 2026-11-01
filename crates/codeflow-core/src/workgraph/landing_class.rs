//! Landing checks selected from the exact line-tip-to-candidate diff.
//! Shares CI's inventory parser and planning-path exclusions.

use std::path::Path;

use super::change_class::{parse_raw, project_paths, regular, RangeEntry};
use super::classify::{amendment_path, valid_document_index, AmendmentPath, ProjectPaths};

/// Required landing gate. Classification is never proof that checks ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LandingClass {
    /// Only declared ordinary Markdown records changed.
    RecordsOnly,
    /// Includes code or a change whose record status could not be proven.
    Code,
}

impl LandingClass {
    /// Stable name for terminal and landing evidence.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::RecordsOnly => "records-only",
            Self::Code => "code",
        }
    }

    /// Test mode required in addition to CI, validation and the secret scan.
    #[must_use]
    pub const fn gate_mode(self) -> &'static str {
        match self {
            Self::RecordsOnly => "quick",
            Self::Code => "full",
        }
    }
}

/// Classification and the evidence that selected it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LandingDecision {
    /// Required gate class.
    pub class: LandingClass,
    /// Why the class applied; failures never grant an exemption.
    pub reason: String,
}

/// Judge a candidate against the exact line tip, not a merge base.
/// Policy declarations come from that tip, so a candidate cannot grant itself
/// an exemption. Any unreadable input selects the full gate.
#[must_use]
pub fn classify_landing(root: &Path, base: &str, head: &str) -> LandingDecision {
    match records_only(root, base, head) {
        Ok(()) => LandingDecision {
            class: LandingClass::RecordsOnly,
            reason: "every changed path is a declared regular Markdown record".into(),
        },
        Err(reason) => LandingDecision {
            class: LandingClass::Code,
            reason,
        },
    }
}

fn records_only(root: &Path, base: &str, head: &str) -> Result<(), String> {
    super::history::check_command(root)?;
    let repo = git2::Repository::discover(root).map_err(|error| error.to_string())?;
    let commit = |revision| {
        repo.revparse_single(revision)
            .and_then(|object| object.peel_to_commit())
            .map_err(|error| error.to_string())
    };
    let base = commit(base)?;
    let head = commit(head)?;
    if base.id() == head.id()
        || !super::history::descendant_of(&repo, head.id(), base.id())
            .map_err(|error| error.to_string())?
    {
        return Err("candidate must descend from the line tip and contain a change".into());
    }
    let base_id = base.id().to_string();
    let project = project_paths(root, &base_id)?;
    let policy = crate::hooks::landed_policy::policy_text_at(root, &base_id)?
        .map(|text| {
            crate::hooks::policy_schema::validate_policy_str(&text)
                .map_err(|errors| format!("invalid target policy: {errors:?}"))?;
            serde_json::from_str::<crate::hooks::policy::Policy>(&text)
                .map_err(|error| error.to_string())
        })
        .transpose()?
        .unwrap_or_default();
    let patterns = policy
        .git
        .record_paths
        .iter()
        .map(|pattern| glob::Pattern::new(pattern).map_err(|error| error.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    // Existing links anywhere in either tree can redirect a tool reading a
    // documentation path. Reuse the planning class's conservative boundary.
    for commit in [&base, &head] {
        let tree = commit.tree().map_err(|error| error.to_string())?;
        let mut special = false;
        tree.walk(git2::TreeWalkMode::PreOrder, |_, entry| {
            if matches!(entry.filemode(), 0o120_000 | 0o160_000) {
                special = true;
            }
            git2::TreeWalkResult::Ok
        })
        .map_err(|error| error.to_string())?;
        if special {
            return Err("a candidate or line tree contains a symbolic link or submodule".into());
        }
    }
    let output = super::history::command()
        .arg("-C")
        .arg(root)
        .args([
            "diff",
            "--raw",
            "-z",
            "--no-ext-diff",
            "--no-textconv",
            "--find-renames",
            "--find-copies",
            "--find-copies-harder",
            &base_id,
            &head.id().to_string(),
            "--",
        ])
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!("cannot read candidate diff: {}", output.status));
    }
    let entries = parse_raw(&output.stdout).ok_or("cannot parse candidate diff")?;
    classify_entries(&entries, &patterns, &project)
}

fn classify_entries(
    entries: &[RangeEntry],
    patterns: &[glob::Pattern],
    project: &ProjectPaths,
) -> Result<(), String> {
    if entries.is_empty() {
        return Err("empty candidate diff".into());
    }
    for entry in entries {
        let path = entry
            .path
            .rule_text()
            .map_err(|_| "a changed path is not UTF-8")?;
        if !regular(entry)
            || (entry.old_mode != "000000"
                && entry.new_mode != "000000"
                && entry.old_mode != entry.new_mode)
        {
            return Err(format!(
                "{} changes mode or is not a regular non-executable file",
                crate::git::display_text(path)
            ));
        }
        if !Path::new(path)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
            || !valid_document_index(path)
            || !patterns.iter().any(|pattern| pattern.matches(path))
        {
            return Err(format!(
                "{} is outside the declared Markdown records",
                crate::git::display_text(path)
            ));
        }
        // A configured record outside the usual folders is treated as a
        // declared document index by the existing planning classifier.
        let mut configured = project.clone();
        configured.document_indexes.push(path.to_string());
        if !matches!(
            amendment_path(path, &configured),
            Some(AmendmentPath::Record | AmendmentPath::Doc)
        ) {
            return Err(format!(
                "{} is excluded by the planning-path classifier",
                crate::git::display_text(path)
            ));
        }
    }
    Ok(())
}
