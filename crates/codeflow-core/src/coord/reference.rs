//! References: the typed links a follow-up carries.
//!
//! A link is `context`, `prerequisite` or `fulfils`, and points at a record
//! (task, epic, spec or ADR, with the repository identity and the record
//! UID), a pull request, a commit, a branch, another follow-up, a lane or a
//! time. Records resolve through the record files by id and UID, never by
//! title, so a renamed task keeps its links and a reused id is caught.
//!
//! Written as text on the command line:
//!
//! ```text
//! fulfils:TSK-123        prerequisite:PR#45        context:EPC-025
//! fulfils:commit:<sha>   fulfils:branch:<name>     prerequisite:FU-003
//! prerequisite:LN-002    prerequisite:time:2026-10-24T09:00:00Z
//! ```

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::actor::{is_follow_up_id, is_lane_id};

/// What a link means for the follow-up that carries it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkKind {
    /// Background only: never makes the follow-up due or closes it.
    Context,
    /// A change of the target's state makes the follow-up due; never closes it.
    Prerequisite,
    /// The target reaching its done state on its target branch closes the
    /// follow-up, judged by the target-aware readiness machinery.
    Fulfils,
}

impl LinkKind {
    /// The kind as written.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Context => "context",
            Self::Prerequisite => "prerequisite",
            Self::Fulfils => "fulfils",
        }
    }
}

/// What a link points at.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Target {
    /// A task, epic, spec or ADR record.
    Record {
        id: String,
        /// The record's hidden UID, when the record carries one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        uid: Option<String>,
        repository: String,
    },
    /// A pull request on the repository's host.
    PullRequest { number: u64, repository: String },
    /// A commit, by its full id.
    Commit { sha: String, repository: String },
    /// A branch on a remote; `fulfils` means it was deleted there.
    Branch {
        name: String,
        remote: String,
        repository: String,
    },
    /// Another follow-up in this ledger.
    FollowUp { id: String },
    /// A lane in this ledger.
    Lane { id: String },
    /// A time, UTC.
    Time { at: String },
}

/// One typed link.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Link {
    pub kind: LinkKind,
    pub target: Target,
}

/// The record kinds a link can name, by id prefix.
fn record_kind(id: &str) -> Option<&'static str> {
    let digits = |rest: &str, min: usize| {
        rest.len() >= min && rest.len() <= 9 && rest.bytes().all(|byte| byte.is_ascii_digit())
    };
    if id.strip_prefix("TSK-").is_some_and(|rest| digits(rest, 3)) {
        Some("task")
    } else if id.strip_prefix("EPC-").is_some_and(|rest| digits(rest, 3)) {
        Some("epic")
    } else if id.strip_prefix("SPC-").is_some_and(|rest| digits(rest, 3)) {
        Some("spec")
    } else if id.strip_prefix("ADR-").is_some_and(|rest| digits(rest, 4)) {
        Some("adr")
    } else {
        None
    }
}

/// Whether a record id names a task.
#[must_use]
pub fn is_task_id(id: &str) -> bool {
    record_kind(id) == Some("task")
}

impl Target {
    /// The target as written on the command line.
    #[must_use]
    pub fn display(&self) -> String {
        match self {
            Self::Record { id, .. } | Self::FollowUp { id } | Self::Lane { id } => id.clone(),
            Self::PullRequest { number, .. } => format!("PR#{number}"),
            Self::Commit { sha, .. } => format!("commit:{}", &sha[..sha.len().min(12)]),
            Self::Branch { name, .. } => format!("branch:{name}"),
            Self::Time { at } => format!("time:{at}"),
        }
    }

    /// Check the target's own fields, as on input so on replay.
    ///
    /// # Errors
    ///
    /// The reason the target is malformed.
    pub fn validate(&self) -> Result<(), String> {
        let short =
            |field: &str, value: &str| super::text::check_required(field, value, 200, false);
        match self {
            Self::Record {
                id,
                uid,
                repository,
            } => {
                if record_kind(id).is_none() {
                    return Err(format!("`{}` is not a record id", shown(id)));
                }
                if let Some(uid) = uid {
                    short("uid", uid)?;
                }
                short("repository", repository)
            }
            Self::PullRequest { number, repository } => {
                if *number == 0 {
                    return Err("a pull request number starts at 1".into());
                }
                short("repository", repository)
            }
            Self::Commit { sha, repository } => {
                if !(sha.len() == 40 || sha.len() == 64)
                    || !sha.bytes().all(|byte| byte.is_ascii_hexdigit())
                {
                    return Err("a commit link names the full commit id".into());
                }
                short("repository", repository)
            }
            Self::Branch {
                name,
                remote,
                repository,
            } => {
                short("branch", name)?;
                short("remote", remote)?;
                short("repository", repository)
            }
            Self::FollowUp { id } => {
                if is_follow_up_id(id) {
                    Ok(())
                } else {
                    Err(format!("`{}` is not a follow-up id", shown(id)))
                }
            }
            Self::Lane { id } => {
                if is_lane_id(id) {
                    Ok(())
                } else {
                    Err(format!("`{}` is not a lane id", shown(id)))
                }
            }
            Self::Time { at } => super::parse_time(at).map(|_| ()),
        }
    }
}

fn shown(text: &str) -> String {
    super::text::shown(text)
}

impl Link {
    /// The link as written on the command line.
    #[must_use]
    pub fn display(&self) -> String {
        format!("{}:{}", self.kind.as_str(), self.target.display())
    }

    /// Check the link, as on input so on replay: the target's fields, and
    /// that a `fulfils` link names something with a done state on a target
    /// (a task, a pull request, a commit or a branch deletion).
    ///
    /// # Errors
    ///
    /// The reason the link is malformed.
    pub fn validate(&self) -> Result<(), String> {
        self.target.validate()?;
        if self.kind == LinkKind::Fulfils {
            let closable = match &self.target {
                Target::Record { id, .. } => is_task_id(id),
                Target::PullRequest { .. } | Target::Commit { .. } | Target::Branch { .. } => true,
                Target::FollowUp { .. } | Target::Lane { .. } | Target::Time { .. } => false,
            };
            if !closable {
                return Err(format!(
                    "`{}` cannot be fulfilled: a fulfils link names a task, a pull request, a commit or a branch",
                    self.display()
                ));
            }
        }
        Ok(())
    }
}

/// What resolving a written link needs: the checkout whose records it
/// reads and the repository identity it records.
pub struct Resolver<'a> {
    pub root: &'a Path,
    pub repository: String,
}

impl<'a> Resolver<'a> {
    /// A resolver for the checkout at `root`.
    #[must_use]
    pub fn new(root: &'a Path) -> Self {
        Self {
            root,
            repository: repository_identity(root),
        }
    }

    /// Parse and resolve a written link. A record resolves through its file
    /// to its UID; a missing record is refused, since a link that cannot be
    /// opened is no link.
    ///
    /// # Errors
    ///
    /// The reason the link cannot be formed.
    pub fn link(&self, text: &str) -> Result<Link, String> {
        let (kind, rest) = text.split_once(':').ok_or_else(|| {
            format!(
                "`{}` is not a link: write <kind>:<target>, for example fulfils:TSK-123",
                shown(text)
            )
        })?;
        let kind = match kind {
            "context" => LinkKind::Context,
            "prerequisite" => LinkKind::Prerequisite,
            "fulfils" => LinkKind::Fulfils,
            other => {
                return Err(format!(
                    "`{}` is not a link kind: use context, prerequisite or fulfils",
                    shown(other)
                ))
            }
        };
        let target = self.target(rest)?;
        let link = Link { kind, target };
        link.validate()?;
        Ok(link)
    }

    fn target(&self, text: &str) -> Result<Target, String> {
        let repository = self.repository.clone();
        if let Some(sha) = text.strip_prefix("commit:") {
            let full = resolve_commit(self.root, sha)?;
            return Ok(Target::Commit {
                sha: full,
                repository,
            });
        }
        if let Some(name) = text.strip_prefix("branch:") {
            let (remote, name) = match name.split_once('/') {
                Some((remote, branch)) if remote_exists(self.root, remote) => (remote, branch),
                _ => ("origin", name),
            };
            return Ok(Target::Branch {
                name: name.to_string(),
                remote: remote.to_string(),
                repository,
            });
        }
        if let Some(at) = text.strip_prefix("time:") {
            super::parse_time(at)?;
            return Ok(Target::Time { at: at.to_string() });
        }
        if let Some(number) = text
            .strip_prefix("PR#")
            .or_else(|| text.strip_prefix("pr:"))
            .or_else(|| text.strip_prefix('#'))
        {
            let number = number
                .parse::<u64>()
                .map_err(|_| format!("`{}` is not a pull request number", shown(text)))?;
            return Ok(Target::PullRequest { number, repository });
        }
        if is_follow_up_id(text) {
            return Ok(Target::FollowUp { id: text.into() });
        }
        if is_lane_id(text) {
            return Ok(Target::Lane { id: text.into() });
        }
        if record_kind(text).is_some() {
            let uid = record_uid(self.root, text)?.ok_or_else(|| {
                format!(
                    "{} has no record in this checkout; a link names a record that exists",
                    shown(text)
                )
            })?;
            return Ok(Target::Record {
                id: text.into(),
                uid: uid.0,
                repository,
            });
        }
        Err(format!(
            "`{}` is not a link target: name a record id, PR#N, commit:<sha>, branch:<name>, FU-NNN, LN-NNN or time:<UTC>",
            shown(text)
        ))
    }
}

/// The record file for `id` in the checkout at `root`, and its UID.
/// `Ok(None)` when no file holds the id; `Ok(Some(None))`-shaped results
/// (a record without a UID) come back as `Some(RecordUid(None))`.
///
/// # Errors
///
/// The reason the records cannot be read.
pub fn record_uid(root: &Path, id: &str) -> Result<Option<RecordUid>, String> {
    let pm = root.join("project-management");
    let files = match record_kind(id) {
        Some("task") => crate::workgraph::layout::task_record_files(&pm),
        Some("epic") => crate::workgraph::layout::epic_record_files(&pm),
        Some("spec") => list_markdown(&pm.join("specs")),
        Some("adr") => list_markdown(&root.join("docs").join("decisions")),
        _ => return Ok(None),
    }
    .map_err(|error| format!("cannot read the records: {error}"))?;
    for path in files {
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let matches = stem == id || stem.starts_with(&format!("{id}-"));
        if !matches {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        if crate::ids::entry::frontmatter_value(&text, "id").as_deref() != Some(id) {
            continue;
        }
        let uid = crate::ids::entry::frontmatter_value(&text, "uid")
            .map(|value| {
                value
                    .split('#')
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .to_string()
            })
            .filter(|value| !value.is_empty());
        return Ok(Some(RecordUid(uid)));
    }
    Ok(None)
}

/// A record's UID as its file holds it; `None` for a record written
/// before UIDs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordUid(pub Option<String>);

fn list_markdown(dir: &Path) -> std::io::Result<Vec<std::path::PathBuf>> {
    match std::fs::read_dir(dir) {
        Ok(entries) => Ok(entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "md"))
            .collect()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error),
    }
}

fn resolve_commit(root: &Path, text: &str) -> Result<String, String> {
    let repo = git2::Repository::discover(root).map_err(|error| error.to_string())?;
    if text.len() < 7 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("`{}` is not a commit id", shown(text)));
    }
    let object = repo
        .revparse_single(text)
        .and_then(|object| object.peel_to_commit())
        .map_err(|_| format!("commit `{}` is not in this repository", shown(text)))?;
    Ok(object.id().to_string())
}

fn remote_exists(root: &Path, remote: &str) -> bool {
    git2::Repository::discover(root).is_ok_and(|repo| repo.find_remote(remote).is_ok())
}

/// The repository identity a link records: the `origin` URL as
/// `host/path`, without credentials or a `.git` suffix, or `local` when
/// the clone has no `origin`.
#[must_use]
pub fn repository_identity(root: &Path) -> String {
    let url = git2::Repository::discover(root).ok().and_then(|repo| {
        repo.find_remote("origin")
            .ok()
            .and_then(|remote| remote.url().ok().map(str::to_string))
    });
    url.map_or_else(|| "local".to_string(), |url| normalize_url(&url))
}

/// `https://user:token@github.com/a/b.git` and `git@github.com:a/b` both
/// become `github.com/a/b`.
#[must_use]
pub fn normalize_url(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let rest = rest.rsplit_once('@').map_or(rest, |(_, rest)| rest);
    let rest = if url.contains("://") {
        rest.to_string()
    } else {
        rest.replacen(':', "/", 1)
    };
    let rest = rest.trim_end_matches('/');
    let rest = rest.strip_suffix(".git").unwrap_or(rest);
    match rest.split_once('/') {
        Some((host, path)) => format!("{}/{path}", host.to_ascii_lowercase()),
        None => rest.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_normalize_without_credentials() {
        assert_eq!(
            normalize_url("https://u:tok@GitHub.com/a/b.git"),
            "github.com/a/b"
        );
        assert_eq!(normalize_url("git@github.com:a/b"), "github.com/a/b");
        assert_eq!(normalize_url("/srv/repo.git"), "/srv/repo");
    }

    #[test]
    fn fulfils_names_something_with_a_done_state() {
        let link = Link {
            kind: LinkKind::Fulfils,
            target: Target::FollowUp {
                id: "FU-001".into(),
            },
        };
        assert!(link.validate().is_err());
        let link = Link {
            kind: LinkKind::Fulfils,
            target: Target::Record {
                id: "EPC-001".into(),
                uid: None,
                repository: "local".into(),
            },
        };
        assert!(link.validate().is_err());
        let link = Link {
            kind: LinkKind::Prerequisite,
            target: Target::Time {
                at: "2026-10-23T00:00:00Z".into(),
            },
        };
        assert!(link.validate().is_ok());
    }

    #[test]
    fn a_link_round_trips_through_json() {
        let link = Link {
            kind: LinkKind::Fulfils,
            target: Target::Record {
                id: "TSK-001".into(),
                uid: Some("u-1".into()),
                repository: "local".into(),
            },
        };
        let text = serde_json::to_string(&link).unwrap();
        assert_eq!(serde_json::from_str::<Link>(&text).unwrap(), link);
        let extra = text.replace("\"id\"", "\"extra\":1,\"id\"");
        assert!(serde_json::from_str::<Link>(&extra).is_err());
    }
}
