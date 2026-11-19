//! The baseline copy of the landed policy (TSK-1057).
//!
//! The policy authority is a remote-tracking ref (TSK-1077), and a process
//! that can write `.git` can point that ref at a commit with a weaker
//! policy. The baseline is an operator-confirmed copy of the policy at one
//! commit of the authority, kept under the account's operator-authority
//! directory (`~/.codeflow/authority/baselines/`), and the guards enforce
//! the stricter of the ref's policy and the baseline's.
//!
//! The baseline is bound to the repository (its canonical common git
//! directory and its configured remote URL), to the authority (the branch
//! `refs/remotes/<remote>/HEAD` names, so a re-pointed remote `HEAD` is a
//! mismatch, never a new authority) and to one commit and policy digest.
//!
//! Only `codeflow policy baseline` writes it, and only from a terminal
//! outside every harness session ([`require_operator_terminal`]). The
//! enforced boundary is the directory itself: every shipped agent preset
//! denies writes under `~/.codeflow/authority` to the harness sandbox and
//! edit-guard refuses edits there (TSK-397 H-5). The session checks refuse
//! early; they cannot stop a process that runs outside the sandbox with no
//! harness marker, no harness ancestor and a pseudo-terminal.
use std::path::{Path, PathBuf};

use git2::Repository;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::Policy;

/// The record format this binary reads and writes.
pub const SCHEMA_VERSION: u32 = 1;

/// Where the baselines live under the account's home.
pub const DIRECTORY: &str = ".codeflow/authority/baselines";

/// The file listing every repository key ever initialized, one per line, so
/// a record that disappears reads as missing, never as never initialized.
const LEDGER: &str = "initialized";

/// The largest record or ledger read.
const READ_LIMIT: u64 = 1 << 20;

/// One baseline: the policy at one authority commit, and what it is bound to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub schema_version: u32,
    /// The canonical common git directory of the repository.
    pub common_dir: String,
    /// The authority remote's name.
    pub remote: String,
    /// The remote's one configured URL, as configured.
    pub remote_url: String,
    /// The authority ref, `refs/remotes/<remote>/<branch>`.
    pub authority: String,
    /// The authority commit the policy was read at.
    pub commit: String,
    /// SHA-256 of the policy text, or of the empty string when the commit
    /// has no policy file and the shipped defaults apply.
    pub policy_digest: String,
    /// The policy text at `commit`; `None` when the commit has no policy file.
    pub policy_text: Option<String>,
    /// When the record was first written, RFC 3339.
    pub created: String,
    /// When it was last written, RFC 3339.
    pub updated: String,
    /// Every explicit operator reset, oldest first.
    #[serde(default)]
    pub resets: Vec<Reset>,
}

/// An operator reset of the baseline to a commit the refresh rule refuses.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reset {
    pub at: String,
    pub from_commit: String,
    pub to_commit: String,
    pub reason: String,
}

impl Record {
    /// The baseline's policy, after checking the digest and the schema.
    ///
    /// # Errors
    /// The digest does not match the text, or the text is not a valid policy.
    pub fn policy(&self) -> Result<Policy, String> {
        let text = self.policy_text.as_deref().unwrap_or("");
        if digest(text) != self.policy_digest {
            return Err("its policy digest does not match its policy text".into());
        }
        let Some(text) = self.policy_text.as_deref() else {
            return Ok(Policy::default());
        };
        super::policy_schema::validate_policy_str(text).map_err(|errors| {
            let shown: Vec<_> = errors.iter().map(ToString::to_string).collect();
            format!("its policy is invalid: {}", shown.join("; "))
        })?;
        serde_json::from_str(text).map_err(|e| format!("its policy is invalid: {e}"))
    }

    /// The commit, shortened for messages.
    #[must_use]
    pub fn short(&self) -> &str {
        &self.commit[..self.commit.len().min(9)]
    }
}

/// SHA-256 of a policy text, lowercase hex.
#[must_use]
pub fn digest(text: &str) -> String {
    sha256_hex(text.as_bytes())
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};
    super::source_identity::hex_digest(&Sha256::digest(bytes))
}

/// What the store holds for one repository.
#[derive(Debug)]
pub enum Lookup {
    /// No record, and the ledger does not list the repository.
    Never,
    /// The ledger lists the repository but its record is gone.
    Missing,
    /// The record or the ledger cannot be trusted; the reason.
    Malformed(String),
    /// A readable record.
    Found(Box<Record>),
}

/// The directory that holds the baselines.
#[derive(Clone, Debug)]
pub struct Store {
    dir: PathBuf,
}

impl Store {
    /// A store at `dir`. Production reads [`Store::account`]; tests pass
    /// their own directory.
    #[must_use]
    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// The store under the account's home from the password database.
    /// Neither `HOME` nor `CODEFLOW_HOME` is read, so the command whose
    /// policy is judged cannot point this read at a directory it wrote.
    /// `None` where the account's home is not read (outside Unix).
    ///
    /// # Errors
    /// The account's home cannot be read.
    pub fn account() -> Result<Option<Self>, String> {
        #[cfg(unix)]
        {
            crate::git::remote_identity::account_home()
                .map(|home| Some(Self::at(home.join(DIRECTORY))))
        }
        #[cfg(not(unix))]
        {
            Ok(None)
        }
    }

    /// The directory this store reads.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn record_path(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{key}.json"))
    }

    /// Look up the record for the repository whose canonical common git
    /// directory is `common`.
    #[must_use]
    pub fn lookup(&self, common: &str) -> Lookup {
        let key = key(common);
        match read_owned(&self.record_path(&key)) {
            Ok(Some(text)) => match serde_json::from_str::<Record>(&text) {
                Ok(record) if record.schema_version != SCHEMA_VERSION => {
                    Lookup::Malformed(format!(
                        "it has schema version {}, and this binary reads {SCHEMA_VERSION}",
                        record.schema_version
                    ))
                }
                Ok(record) if record.common_dir != common => Lookup::Malformed(format!(
                    "it is filed for this repository but names {}",
                    crate::git::display_text(&record.common_dir)
                )),
                Ok(record) => Lookup::Found(Box::new(record)),
                Err(error) => Lookup::Malformed(format!("it cannot be parsed ({error})")),
            },
            Ok(None) => match self.ledger() {
                Ok(keys) if keys.contains(&key) => Lookup::Missing,
                Ok(_) => Lookup::Never,
                Err(why) => Lookup::Malformed(why),
            },
            Err(why) => Lookup::Malformed(why),
        }
    }

    fn ledger(&self) -> Result<Vec<String>, String> {
        Ok(read_owned(&self.dir.join(LEDGER))?
            .map(|text| text.lines().map(str::to_owned).collect())
            .unwrap_or_default())
    }

    /// Write `record` and list its repository in the ledger. The directory
    /// is created owner-only; the record is written to a temporary file and
    /// renamed into place.
    ///
    /// # Errors
    /// The directory or a file cannot be written.
    pub fn write(&self, record: &Record) -> Result<PathBuf, String> {
        let key = key(&record.common_dir);
        create_private_dir(&self.dir)?;
        let text = serde_json::to_string_pretty(record).map_err(|e| e.to_string())? + "\n";
        let path = self.record_path(&key);
        write_private(&path, &text)?;
        let mut keys = self.ledger()?;
        if !keys.contains(&key) {
            keys.push(key);
            write_private(&self.dir.join(LEDGER), &(keys.join("\n") + "\n"))?;
        }
        Ok(path)
    }
}

/// The store key for a canonical common git directory.
#[must_use]
pub fn key(common: &str) -> String {
    sha256_hex(common.as_bytes())
}

/// The canonical common git directory of `repo`, as the record binds it.
///
/// # Errors
/// The directory cannot be resolved or is not valid UTF-8.
pub fn common_dir(repo: &Repository) -> Result<String, String> {
    let dir = std::fs::canonicalize(repo.commondir())
        .map_err(|e| format!("the common git directory cannot be resolved: {e}"))?;
    dir.to_str()
        .map(str::to_owned)
        .ok_or_else(|| "the common git directory is not valid UTF-8".into())
}

/// The text of a file only this account can write, or `None` when it does
/// not exist. A link, a file that is not regular, owned by another account,
/// writable by others or over [`READ_LIMIT`] refuses.
fn read_owned(path: &Path) -> Result<Option<String>, String> {
    use std::io::Read as _;
    let shown = path.display();
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let mut file = match options.open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{shown} cannot be opened ({e})")),
    };
    let meta = file
        .metadata()
        .map_err(|e| format!("{shown} cannot be read ({e})"))?;
    if !meta.file_type().is_file() {
        return Err(format!("{shown} is not a regular file"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        // SAFETY: `geteuid` has no preconditions and cannot fail.
        let owner = unsafe { libc::geteuid() };
        if meta.uid() != owner || meta.mode() & 0o022 != 0 {
            return Err(format!(
                "{shown} is not owned by this account or can be written by others"
            ));
        }
    }
    if meta.len() > READ_LIMIT {
        return Err(format!("{shown} is over 1 MiB"));
    }
    let mut text = String::new();
    (&mut file)
        .take(READ_LIMIT + 1)
        .read_to_string(&mut text)
        .map_err(|e| format!("{shown} cannot be read ({e})"))?;
    Ok(Some(text))
}

fn create_private_dir(dir: &Path) -> Result<(), String> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        builder.mode(0o700);
    }
    builder
        .create(dir)
        .map_err(|e| format!("{} cannot be created ({e})", dir.display()))
}

fn write_private(path: &Path, text: &str) -> Result<(), String> {
    use std::io::Write as _;
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(dir)
        .map_err(|e| format!("{} cannot be written ({e})", dir.display()))?;
    file.write_all(text.as_bytes())
        .and_then(|()| file.as_file().sync_all())
        .map_err(|e| format!("{} cannot be written ({e})", path.display()))?;
    file.persist(path)
        .map_err(|e| format!("{} cannot be written ({})", path.display(), e.error))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// The stricter of two policies
// ---------------------------------------------------------------------------

/// Lists where more entries refuse more: the union is the stricter value.
/// Repeated path rules are concurrent constraints (`path_policy.rs`), so a
/// union of rules is a union of denials.
const UNION_LISTS: &[&str] = &[
    "git.protected_branches",
    "git.required_checks",
    "git.commit_required_footers",
    "git.pr_required_sections",
    "git.pr_code_sections",
    "git.breaking_watch_paths",
    "git.blocked_names",
    "git.protected_paths",
    "git.branch_name_omit_words",
    // A range touching a product path needs a journey criterion, so more
    // paths refuse more.
    "git.product_paths",
    // A record lacking a listed section is refused, so more sections refuse
    // more records (TSK-1421).
    "git.task_required_sections",
    "git.epic_required_sections",
    "git.spec_required_sections",
];

/// Lists where more entries allow more: the intersection is the stricter
/// value.
const INTERSECT_LISTS: &[&str] = &[
    "git.worktree_locations",
    "git.claim_remotes",
    "git.commit_types",
    "git.commit_footer_tokens",
    "git.commit_ticket_keys",
    "git.branch_prefixes",
    "git.document_indexes",
    "git.automation_profiles",
    "git.clean_regenerable",
    "security.sandbox_retry_allow",
    "security.trusted_client_configs",
    "git.ignored_files.allow",
];

/// String values that are not levels even when they spell one.
const NOT_LEVELS: &[&str] = &[
    "git.root_branch",
    "git.candidate_prefix",
    "git.direct_changes",
    "git.pr_breaking_level",
    "git.commit_ticket_pattern",
    "git.release_branch_pattern",
    "security.workflow_pushes",
    "security.sandbox_retry",
];

/// The stricter of two policies: `baseline` and the authority ref's `other`.
///
/// Every level takes the higher of block, warn, allow and off; the lists in
/// `UNION_LISTS` take the union and those in `INTERSECT_LISTS` the
/// intersection; `security.text_guard` is strict when either is. Every
/// other value is the baseline's: the ref can add strictness through these
/// keys, never change anything else, until the operator refreshes the
/// baseline.
///
/// # Errors
/// Either policy cannot be serialized, or the result does not read back as
/// a policy.
pub fn stricter(baseline: &Policy, other: &Policy) -> Result<Policy, String> {
    let base = explicit(serde_json::to_value(baseline).map_err(|e| e.to_string())?);
    let other = explicit(serde_json::to_value(other).map_err(|e| e.to_string())?);
    let merged = merge(&base, &other, "");
    serde_json::from_value(merged).map_err(|e| format!("the stricter policy is invalid: {e}"))
}

/// Whether `candidate` is weaker than `baseline` anywhere: the stricter of
/// the two differs from `candidate`.
///
/// # Errors
/// As [`stricter`].
pub fn weaker(candidate: &Policy, baseline: &Policy) -> Result<bool, String> {
    let strict = stricter(baseline, candidate)?;
    let left = explicit(serde_json::to_value(&strict).map_err(|e| e.to_string())?);
    let right = explicit(serde_json::to_value(candidate).map_err(|e| e.to_string())?);
    Ok(left != right)
}

/// The enforced keys that serialization leaves out at their default. A key
/// one side omits must merge as its default: otherwise the other side's
/// relaxed text rule or allowance list would survive the merge unchanged.
/// The required record sections leave out an empty list, so a side that
/// lists none must still merge with the other side's sections (TSK-1421).
const OMITTED_DEFAULTS: &[(&str, &str)] = &[
    ("security", "text_guard"),
    ("security", "trusted_client_configs"),
    ("git", "ignored_files"),
    ("git", "task_required_sections"),
    ("git", "epic_required_sections"),
    ("git", "spec_required_sections"),
];

/// `policy` with each [`OMITTED_DEFAULTS`] key written out at its default.
fn explicit(mut policy: Value) -> Value {
    for (section, key) in OMITTED_DEFAULTS {
        let Some(Value::Object(map)) = policy.get_mut(*section) else {
            continue;
        };
        if map.contains_key(*key) {
            continue;
        }
        let default = match *key {
            "text_guard" => Value::String("strict".into()),
            "trusted_client_configs"
            | "task_required_sections"
            | "epic_required_sections"
            | "spec_required_sections" => Value::Array(Vec::new()),
            // Absent means level warn and no allowance (TSK-323).
            _ => serde_json::json!({"level": "warn", "allow": []}),
        };
        map.insert((*key).to_owned(), default);
    }
    policy
}

fn merge(base: &Value, other: &Value, path: &str) -> Value {
    let child = |key: &str| {
        if path.is_empty() {
            key.to_owned()
        } else {
            format!("{path}.{key}")
        }
    };
    match (base, other) {
        (Value::Object(left), Value::Object(right)) => {
            let mut out = left.clone();
            for (key, value) in left {
                if let Some(theirs) = right.get(key) {
                    out.insert(key.clone(), merge(value, theirs, &child(key)));
                }
            }
            Value::Object(out)
        }
        _ if UNION_LISTS.contains(&path) => union(base, other),
        _ if INTERSECT_LISTS.contains(&path) => intersect(base, other),
        _ if path == "security.text_guard" => {
            if base == "relaxed" && other == "relaxed" {
                base.clone()
            } else {
                Value::String("strict".into())
            }
        }
        (Value::String(left), Value::String(right)) if !NOT_LEVELS.contains(&path) => {
            match (rank(left), rank(right)) {
                (Some(l), Some(r)) if r > l => other.clone(),
                _ => base.clone(),
            }
        }
        _ => base.clone(),
    }
}

fn rank(level: &str) -> Option<u8> {
    match level {
        "block" => Some(3),
        "warn" => Some(2),
        "allow" => Some(1),
        "off" => Some(0),
        _ => None,
    }
}

fn union(base: &Value, other: &Value) -> Value {
    match (base, other) {
        (Value::Array(left), Value::Array(right)) => {
            let mut out = left.clone();
            for item in right {
                if !out.contains(item) {
                    out.push(item.clone());
                }
            }
            Value::Array(out)
        }
        // An absent list (`null`) means a default the merge cannot list, so
        // the other side's list never replaces it; the baseline's stands.
        _ => base.clone(),
    }
}

fn intersect(base: &Value, other: &Value) -> Value {
    match (base, other) {
        (Value::Array(left), Value::Array(right)) => Value::Array(
            left.iter()
                .filter(|item| right.contains(item))
                .cloned()
                .collect(),
        ),
        // An absent list means its default; the baseline's stands.
        _ => base.clone(),
    }
}

// ---------------------------------------------------------------------------
// Applying the baseline to the authority read
// ---------------------------------------------------------------------------

/// How the baseline bore on one policy read; doctor and the diagnostic show it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// No baseline applies on this platform.
    Unsupported,
    /// The repository was never initialized: the authority read stands.
    Never,
    /// The authority ref is at the baseline's commit.
    Current,
    /// The authority ref is a descendant of the baseline's commit: the
    /// stricter of the two is enforced, and the operator refreshes it.
    Stale { baseline: String },
    /// The authority ref is not a descendant of the baseline's commit: the
    /// baseline's policy is enforced.
    Diverged { baseline: String },
    /// The authority remote is gone: the baseline's policy is enforced.
    RemoteRemoved { remote: String, baseline: String },
    /// A labelled state with a baseline for the same remote: the baseline's
    /// policy is enforced.
    Labelled { baseline: String },
}

impl State {
    /// Whether the baseline replaced the read's policy: the baseline's own,
    /// or the stricter of the two.
    #[must_use]
    pub fn enforces_policy(&self) -> bool {
        matches!(
            self,
            Self::Stale { .. }
                | Self::Diverged { .. }
                | Self::RemoteRemoved { .. }
                | Self::Labelled { .. }
        )
    }

    /// The note doctor and the policy diagnostic print, if any.
    #[must_use]
    pub fn note(&self) -> Option<String> {
        match self {
            Self::Unsupported => Some("policy baseline: not read on this platform".into()),
            Self::Never => Some(
                "policy baseline: none for this repository; the operator runs codeflow policy baseline init in a terminal outside any agent session"
                    .into(),
            ),
            Self::Current => None,
            Self::Stale { baseline } => Some(format!(
                "policy baseline: at {baseline}, behind the authority; the stricter of the two is enforced; the operator runs codeflow policy baseline refresh"
            )),
            Self::Diverged { baseline } => Some(format!(
                "policy baseline: the authority ref is not a descendant of {baseline}; the baseline's policy is enforced"
            )),
            Self::RemoteRemoved { remote, baseline } => Some(format!(
                "policy baseline: the authority remote {remote} is gone; the baseline's policy at {baseline} is enforced"
            )),
            Self::Labelled { baseline } => Some(format!(
                "policy baseline: no authority ref; the baseline's policy at {baseline} is enforced"
            )),
        }
    }
}

/// The authority as the read found it, which the baseline must match.
pub struct Bound<'a> {
    pub remote: &'a str,
    /// The authority ref and commit, when the namespace held one.
    pub authority: Option<&'a super::landed_policy::Authority>,
}

/// The repository's single configured URL for `remote`.
///
/// # Errors
/// The URL is unset, set more than once, or not valid UTF-8.
pub fn configured_url(repo: &Repository, remote: &str) -> Result<String, String> {
    let config = repo.config().map_err(|e| e.to_string())?;
    let key = format!("remote.{remote}.url");
    let mut urls = Vec::new();
    let mut entries = config
        .multivar(&key, None)
        .map_err(|e| format!("cannot read {key}: {e}"))?;
    while let Some(entry) = entries.next() {
        let entry = entry.map_err(|e| format!("cannot read {key}: {e}"))?;
        let url = entry
            .value()
            .map_err(|_| format!("{key} is not valid UTF-8"))?;
        urls.push(url.to_owned());
    }
    match urls.as_slice() {
        [url] => Ok(url.clone()),
        [] => Err(format!("{key} is not set")),
        _ => Err(format!("{key} has several values")),
    }
}

/// The policy to enforce after the baseline: `Ok(None)` when the read's
/// policy stands unchanged.
pub struct Applied {
    pub policy: Option<Policy>,
    pub state: State,
}

/// Apply the baseline in `store` to a read of the authority.
///
/// `bound` is `None` when the repository has no remote. `policy` is the
/// policy the read found.
///
/// # Errors
/// The repository was initialized and its baseline is missing, malformed or
/// bound to another repository, remote URL or authority; or ancestry cannot
/// be read. Each blocks the policy check.
pub fn apply(
    repo: &Repository,
    store: Option<&Store>,
    bound: Option<&Bound<'_>>,
    policy: &Policy,
) -> Result<Applied, String> {
    let unchanged = |state| {
        Ok(Applied {
            policy: None,
            state,
        })
    };
    let Some(store) = store else {
        return unchanged(State::Unsupported);
    };
    let common = common_dir(repo)?;
    let shown = store.record_path(&key(&common));
    let blocked = |why: String| {
        format!(
            "the policy baseline {} {why}; the policy check blocks until the operator repairs it (codeflow policy baseline refresh --reset, in a terminal outside any agent session)",
            shown.display()
        )
    };
    let record = match store.lookup(&common) {
        Lookup::Never => return unchanged(State::Never),
        Lookup::Missing => {
            return Err(blocked(
                "is missing, though this repository was initialized".into(),
            ))
        }
        Lookup::Malformed(why) => return Err(blocked(why)),
        Lookup::Found(record) => record,
    };
    let base_policy = record.policy().map_err(blocked)?;
    let baseline = record.short().to_owned();
    let Some(bound) = bound else {
        return Ok(Applied {
            policy: Some(base_policy),
            state: State::RemoteRemoved {
                remote: record.remote.clone(),
                baseline,
            },
        });
    };
    if bound.remote != record.remote {
        return Err(blocked(format!(
            "is bound to remote {}, and the authority remote is now {}",
            crate::git::display_text(&record.remote),
            crate::git::display_text(bound.remote)
        )));
    }
    let url = configured_url(repo, bound.remote)
        .map_err(|why| blocked(format!("cannot be matched: {why}")))?;
    if url != record.remote_url {
        return Err(blocked(format!(
            "is bound to {}, and {} now points at {}",
            crate::git::remote_query::display_url(&record.remote_url),
            crate::git::display_text(bound.remote),
            crate::git::remote_query::display_url(&url)
        )));
    }
    let Some(authority) = bound.authority else {
        return Ok(Applied {
            policy: Some(base_policy),
            state: State::Labelled { baseline },
        });
    };
    if authority.reference != record.authority {
        return Err(blocked(format!(
            "is bound to authority {}, and refs/remotes/{}/HEAD now names {}",
            crate::git::display_text(&record.authority),
            crate::git::display_text(bound.remote),
            crate::git::display_text(&authority.reference)
        )));
    }
    if authority.sha == record.commit {
        return unchanged(State::Current);
    }
    let base_oid = git2::Oid::from_str(&record.commit).map_err(|e| blocked(e.to_string()))?;
    let ref_oid = git2::Oid::from_str(&authority.sha).map_err(|e| e.to_string())?;
    // A baseline commit missing from the object store cannot be an ancestor
    // the read can prove, so the ref is not taken as its descendant.
    let descends = repo.find_commit(base_oid).is_ok()
        && crate::workgraph::history::descendant_of(repo, ref_oid, base_oid).map_err(|e| {
            format!("cannot tell whether the authority descends from the policy baseline: {e}")
        })?;
    if descends {
        Ok(Applied {
            policy: Some(stricter(&base_policy, policy)?),
            state: State::Stale { baseline },
        })
    } else {
        Ok(Applied {
            policy: Some(base_policy),
            state: State::Diverged { baseline },
        })
    }
}

// ---------------------------------------------------------------------------
// Reading the authority fresh from the remote
// ---------------------------------------------------------------------------

/// The authority as the remote itself reports it, with its policy read from
/// the fetched commit. `crate::git::baseline_fresh::fresh` reads it; that
/// read uses configured waits, so it stays out of the guard code.
#[derive(Clone, Debug)]
pub struct Fresh {
    pub common_dir: String,
    pub remote: String,
    pub remote_url: String,
    pub authority: String,
    pub commit: String,
    pub policy_text: Option<String>,
    pub policy_digest: String,
}

/// The record that initializes a repository from a fresh read.
#[must_use]
pub fn initial(fresh: &Fresh, now: &str) -> Record {
    Record {
        schema_version: SCHEMA_VERSION,
        common_dir: fresh.common_dir.clone(),
        remote: fresh.remote.clone(),
        remote_url: fresh.remote_url.clone(),
        authority: fresh.authority.clone(),
        commit: fresh.commit.clone(),
        policy_digest: fresh.policy_digest.clone(),
        policy_text: fresh.policy_text.clone(),
        created: now.to_owned(),
        updated: now.to_owned(),
        resets: Vec::new(),
    }
}

/// Why a refresh from `record` to `fresh` needs an explicit operator reset,
/// or `None` when it may proceed: the same repository, remote, URL and
/// authority, a commit that is the bound one or a descendant, and a policy
/// no weaker than the bound one.
///
/// # Errors
/// Ancestry or a policy cannot be read.
pub fn refresh_refusal(
    repo: &Repository,
    record: &Record,
    fresh: &Fresh,
) -> Result<Option<String>, String> {
    if record.common_dir != fresh.common_dir
        || record.remote != fresh.remote
        || record.remote_url != fresh.remote_url
        || record.authority != fresh.authority
    {
        return Ok(Some(format!(
            "the baseline is bound to {} at {}, and the authority is now {} at {}",
            crate::git::display_text(&record.authority),
            crate::git::remote_query::display_url(&record.remote_url),
            crate::git::display_text(&fresh.authority),
            crate::git::remote_query::display_url(&fresh.remote_url)
        )));
    }
    if record.commit == fresh.commit {
        return Ok(None);
    }
    let base = git2::Oid::from_str(&record.commit).map_err(|e| e.to_string())?;
    let new = git2::Oid::from_str(&fresh.commit).map_err(|e| e.to_string())?;
    let descends = repo.find_commit(base).is_ok()
        && crate::workgraph::history::descendant_of(repo, new, base).map_err(|e| e.to_string())?;
    if !descends {
        return Ok(Some(format!(
            "{} is not a descendant of the baseline's commit {}",
            &fresh.commit[..9],
            record.short()
        )));
    }
    let base_policy = record.policy()?;
    let new_policy = match fresh.policy_text.as_deref() {
        Some(text) => serde_json::from_str(text).map_err(|e| e.to_string())?,
        None => Policy::default(),
    };
    if weaker(&new_policy, &base_policy)? {
        return Ok(Some(format!(
            "the policy at {} is weaker than the baseline's at {}",
            &fresh.commit[..9],
            record.short()
        )));
    }
    Ok(None)
}

/// The record after a refresh to `fresh`; `reset` records an explicit
/// operator reset with its reason.
#[must_use]
pub fn refreshed(record: &Record, fresh: &Fresh, now: &str, reset: Option<&str>) -> Record {
    let mut next = initial(fresh, now);
    next.created.clone_from(&record.created);
    next.resets.clone_from(&record.resets);
    if let Some(reason) = reset {
        next.resets.push(Reset {
            at: now.to_owned(),
            from_commit: record.commit.clone(),
            to_commit: fresh.commit.clone(),
            reason: reason.to_owned(),
        });
    }
    next
}

// ---------------------------------------------------------------------------
// The writer's session checks
// ---------------------------------------------------------------------------

/// Executable names of the harnesses whose descendants are agent sessions.
pub const HARNESS_PROCESSES: &[&str] = &["claude", "codex", "grok"];

/// What the writer's session checks read; tests pass their own.
pub struct Probe<'a> {
    pub env: crate::root_checkout::EnvLookup<'a>,
    /// The executable names of every ancestor process, nearest first, or why
    /// they cannot be read.
    pub ancestors: &'a dyn Fn() -> Result<Vec<String>, String>,
    pub stdin_is_terminal: bool,
}

/// Refuse unless the writer runs at an operator's terminal: no harness
/// marker in the environment, no harness among the ancestor processes, and
/// standard input a terminal. Ancestry that cannot be read refuses.
///
/// # Errors
/// The reason the writer refuses.
pub fn require_operator_terminal(probe: &Probe<'_>) -> Result<(), String> {
    let route = "the operator runs it in a terminal outside every agent session";
    if let Some(marker) = crate::root_checkout::AGENT_MARKERS
        .iter()
        .find(|name| (probe.env)(name).is_some_and(|v| !v.is_empty()))
    {
        return Err(format!(
            "the policy baseline is written only by the operator: {marker} is set, so this runs in an agent session; {route}"
        ));
    }
    let ancestors = (probe.ancestors)().map_err(|why| {
        format!("the policy baseline is written only by the operator, and the process ancestry cannot be read ({why}); {route}")
    })?;
    if let Some(found) = ancestors.iter().find(|name| {
        let name = name.to_ascii_lowercase();
        HARNESS_PROCESSES
            .iter()
            .any(|harness| name == *harness || name.starts_with(&format!("{harness}-")))
    }) {
        return Err(format!(
            "the policy baseline is written only by the operator: an ancestor process is {}, an agent harness; {route}",
            crate::git::display_text(found)
        ));
    }
    if !probe.stdin_is_terminal {
        return Err(format!(
            "the policy baseline is written only by the operator: standard input is not a terminal; {route}"
        ));
    }
    Ok(())
}

/// The executable names of this process's ancestors, nearest first.
///
/// # Errors
/// The ancestry cannot be read on this platform or for some process.
#[cfg(unix)]
pub fn process_ancestors() -> Result<Vec<String>, String> {
    let mut names = Vec::new();
    // SAFETY: `getppid` has no preconditions and cannot fail.
    let mut pid = unsafe { libc::getppid() };
    let mut steps = 0;
    while pid > 1 {
        steps += 1;
        if steps > 256 {
            return Err("the ancestry is deeper than 256 processes".into());
        }
        let (name, parent) = process_entry(pid)?;
        names.push(name);
        if parent == pid {
            break;
        }
        pid = parent;
    }
    Ok(names)
}

/// The executable names of this process's ancestors: not read outside Unix.
///
/// # Errors
/// Always: the ancestry is not read on this platform.
#[cfg(not(unix))]
pub fn process_ancestors() -> Result<Vec<String>, String> {
    Err("process ancestry is not read on this platform".into())
}

/// The size of the 64-bit macOS `struct kinfo_proc` (`<sys/sysctl.h>`):
/// `struct extern_proc` (296 bytes) then `struct eproc` (352 bytes).
#[cfg(target_os = "macos")]
const KINFO_PROC_SIZE: usize = 648;
/// Offset of `kp_proc.p_pid` (`pid_t`).
#[cfg(target_os = "macos")]
const KINFO_PID: usize = 40;
/// Offset and length of `kp_proc.p_comm` (`char[MAXCOMLEN + 1]`).
#[cfg(target_os = "macos")]
const KINFO_COMM: std::ops::Range<usize> = 243..260;
/// Offset of `kp_eproc.e_ppid` (`pid_t`).
#[cfg(target_os = "macos")]
const KINFO_PPID: usize = 560;

/// One process's executable name and parent pid, read the way `ps` reads
/// them: `sysctl(CTL_KERN, KERN_PROC, KERN_PROC_PID, pid)`. Unlike
/// `proc_pidinfo`, this reads a process owned by another account, such as
/// the root-owned `login` above a terminal shell. A failed call, a reply of
/// the wrong size, a reply for another process or an empty name is "cannot
/// be read", so the writer refuses.
#[cfg(target_os = "macos")]
fn process_entry(pid: libc::pid_t) -> Result<(String, libc::pid_t), String> {
    let unreadable = || format!("process {pid} cannot be read");
    let mut mib = [libc::CTL_KERN, libc::KERN_PROC, libc::KERN_PROC_PID, pid];
    let mut buffer = [0u8; KINFO_PROC_SIZE];
    let mut len: libc::size_t = KINFO_PROC_SIZE;
    // SAFETY: `mib` holds 4 names; `buffer` is writable for `len` bytes; the
    // kernel writes at most `len` bytes and stores the count it wrote.
    let status = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            4,
            buffer.as_mut_ptr().cast(),
            &raw mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    // A process that does not exist answers with status 0 and length 0.
    if status != 0 || len != KINFO_PROC_SIZE {
        return Err(unreadable());
    }
    let pid_at = |offset: usize| {
        let mut bytes = [0u8; 4];
        bytes.copy_from_slice(&buffer[offset..offset + 4]);
        libc::pid_t::from_ne_bytes(bytes)
    };
    if pid_at(KINFO_PID) != pid {
        return Err(unreadable());
    }
    let name = std::ffi::CStr::from_bytes_until_nul(&buffer[KINFO_COMM])
        .map_err(|_| unreadable())?
        .to_string_lossy()
        .into_owned();
    if name.is_empty() {
        return Err(unreadable());
    }
    Ok((name, pid_at(KINFO_PPID)))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn process_entry(pid: libc::pid_t) -> Result<(String, libc::pid_t), String> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .map_err(|e| format!("process {pid} cannot be read ({e})"))?;
    let open = stat.find('(').ok_or("an unreadable /proc stat line")?;
    let close = stat.rfind(')').ok_or("an unreadable /proc stat line")?;
    let name = stat[open + 1..close].to_owned();
    let parent = stat[close + 1..]
        .split_whitespace()
        .nth(1)
        .and_then(|field| field.parse().ok())
        .ok_or("an unreadable /proc stat line")?;
    Ok((name, parent))
}

#[cfg(test)]
#[path = "policy_baseline_tests.rs"]
mod tests;
