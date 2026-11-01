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

/// [`classify_landing`] for a landing into `target`. Only an unprotected
/// integration line that is not a release line can take the records-only
/// class: a protected branch or a name matching the built-in release pattern
/// or a `git.release_branch_pattern` (from the line tip's policy or the
/// checkout's) keeps the full gate. Any policy that cannot be read selects
/// the full gate.
#[must_use]
pub fn classify_landing_into(root: &Path, base: &str, head: &str, target: &str) -> LandingDecision {
    match full_gate_target(root, base, target) {
        Some(reason) => LandingDecision {
            class: LandingClass::Code,
            reason,
        },
        None => classify_landing(root, base, head),
    }
}

/// Why `target` keeps the full gate whatever the diff, or `None`.
fn full_gate_target(root: &Path, base: &str, target: &str) -> Option<String> {
    if !target.starts_with("integration/") {
        return Some("the target is not an integration line".into());
    }
    let shown = crate::git::display_text(target);
    let tip = match line_policy(root, base) {
        Ok(policy) => policy,
        Err(error) => return Some(format!("the line tip's policy cannot be read: {error}")),
    };
    let checkout = match crate::hooks::policy::Policy::load(root) {
        Ok(policy) => policy,
        Err(error) => return Some(format!("the checkout's policy cannot be read: {error}")),
    };
    for policy in [&tip, &checkout] {
        if policy.git.branch_is_protected_any_case(target) {
            return Some(format!("{shown} is a protected branch"));
        }
        for pattern in [
            Some(super::release_line::BUILTIN_PATTERN),
            policy.git.release_branch_pattern.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            match glob::Pattern::new(pattern) {
                Ok(glob) if glob.matches(target) => {
                    return Some(format!(
                        "{shown} matches the release pattern {}",
                        crate::git::display_text(pattern)
                    ));
                }
                Ok(_) => {}
                Err(error) => {
                    return Some(format!("the release pattern cannot be read: {error}"));
                }
            }
        }
    }
    None
}

/// The policy at the line tip, or the defaults when the tip has none.
fn line_policy(root: &Path, base: &str) -> Result<crate::hooks::policy::Policy, String> {
    crate::hooks::landed_policy::policy_text_at(root, base)?
        .map(|text| {
            crate::hooks::policy_schema::validate_policy_str(&text)
                .map_err(|errors| format!("invalid target policy: {errors:?}"))?;
            serde_json::from_str::<crate::hooks::policy::Policy>(&text)
                .map_err(|error| error.to_string())
        })
        .transpose()
        .map(Option::unwrap_or_default)
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
    let policy = line_policy(root, &base_id)?;
    let patterns = policy
        .git
        .record_paths
        .iter()
        .map(|pattern| glob::Pattern::new(pattern).map_err(|error| error.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
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
    classify_entries(&entries, &patterns, &project)?;
    // A link can redirect a tool reading a record only when it sits under a
    // declared record folder or on a folder a changed record resolves
    // through. Links elsewhere, such as a root `CLAUDE.md -> AGENTS.md`,
    // cannot, so they do not withhold the class.
    let mut scopes: Vec<String> = policy
        .git
        .record_paths
        .iter()
        .map(|p| literal_dir(p))
        .collect();
    for entry in &entries {
        if let Ok(path) = entry.path.rule_text() {
            scopes.push(path.to_string());
        }
    }
    for commit in [&base, &head] {
        let tree = commit.tree().map_err(|error| error.to_string())?;
        let mut special: Option<String> = None;
        tree.walk(git2::TreeWalkMode::PreOrder, |dir, entry| {
            if matches!(entry.filemode(), 0o120_000 | 0o160_000) {
                let name = crate::git::GitName::from_bytes(entry.name_bytes());
                // A link whose name is not UTF-8 cannot be judged against the
                // record globs, so it withholds the class.
                let reaches = name.rule_text().map_or(true, |name| {
                    link_reaches_records(&format!("{dir}{name}"), &scopes, &patterns)
                });
                if reaches {
                    special = Some(format!("{dir}{}", name.display()));
                    return git2::TreeWalkResult::Abort;
                }
            }
            git2::TreeWalkResult::Ok
        })
        .or_else(|error| {
            if special.is_some() {
                Ok(())
            } else {
                Err(error.to_string())
            }
        })?;
        if let Some(path) = special {
            return Err(format!(
                "{path} is a symbolic link or submodule on a record path"
            ));
        }
    }
    Ok(())
}

/// The folder part of a glob before its first wildcard, ending in `/`, or
/// the empty string (the whole tree) when the glob has no literal folder.
fn literal_dir(pattern: &str) -> String {
    let literal = &pattern[..pattern.find(['*', '?', '[']).unwrap_or(pattern.len())];
    literal
        .rfind('/')
        .map_or_else(String::new, |end| literal[..=end].to_string())
}

/// Whether a link at `path` sits on a record path: it matches a record glob,
/// lies under a declared record folder, or is a folder that a record folder
/// or a changed path resolves through.
fn link_reaches_records(path: &str, scopes: &[String], patterns: &[glob::Pattern]) -> bool {
    let folder = format!("{path}/");
    patterns.iter().any(|pattern| pattern.matches(path))
        || scopes.iter().any(|scope| {
            ((scope.ends_with('/') || scope.is_empty()) && path.starts_with(scope.as_str()))
                || scope.starts_with(&folder)
                || scope == path
        })
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
        // `regular` admits only 000000 and 100644 on each side, so a mode
        // change between two present files cannot pass it.
        if !regular(entry) {
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
