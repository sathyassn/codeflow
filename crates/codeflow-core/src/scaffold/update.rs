//! `codeflow update` — manifest + 3-way merge refresh of managed files
//! (charter §3.1, §4.3, §10, AC #8).
//!
//! Per installed manifest entry:
//! - **managed, unmodified** (current hash == recorded hash): replaced with
//!   the new shipped version; record and baseline refreshed.
//! - **managed, user-modified**: 3-way merge with base = `.codeflow/.baseline/`
//!   copy, ours = the user's file, theirs = the new shipped version. Clean
//!   merge is applied and reported; a conflict writes `<path>.new` holding
//!   the merge with conflict markers (the user's non-overlapping changes
//!   kept) and the report; the user's file is NEVER clobbered and NEVER
//!   silently skipped.
//! - **managed-region**: only the marked block (markdown/hash) or the
//!   codeflow-owned keys (settings JSON) are regenerated.
//! - **user-owned**: never mutated; schema-versioned JSON (policy.json) gains
//!   NEW default keys (absent from both the user file and the old shipped
//!   default), added with defaults and reported, and a scalar that still
//!   equals the old shipped default moves to a changed new default, reported
//!   (ADR-0075); a value that differs from the old default is kept.
//!
//! Manifest invariant: a managed file's recorded `sha256` is the hash of the
//! pristine shipped version (== the `.baseline/` copy), NEVER the hash of a
//! merged file. That is what makes "current hash == recorded hash" mean
//! "unmodified"; recording a merged hash would misclassify a customized file as
//! pristine and overwrite it on the next update.
//!
//! After applying the new manifest, update **reconciles orphans**: files
//! managed under the OLD installed manifest but absent from the NEW manifest
//! entirely (an artifact removed or renamed upstream, e.g. the
//! `.claude/commands/*` -> `.claude/skills/*/SKILL.md` migration). An unmodified
//! whole-file `managed` orphan is deleted (file + baseline + record); anything
//! that might hold user content (user-modified `managed`, `managed-region`,
//! `user-owned`) is kept and merely unmanaged. Without this, a stale orphan
//! lingers and can collide with its renamed replacement.
//!
//! A project may opt individual managed files out of all of the above via a
//! `[scaffold] ignore = ["glob", ...]` list in `.codeflow/project.toml`: any
//! managed file whose repo-relative dest matches an ignore glob is skipped
//! entirely — never rewritten, never resurrected if the user deleted it, and
//! never pruned/reported as an orphan (a non-Codex team drops `.codex/**`, a
//! GitLab team `.github/**`). Its manifest record and baseline are left intact,
//! so removing the glob restores normal management on the next update.
//!
//! The update always ends with a printed report
//! (changed / merged / conflicted / skipped / added / removed).

use std::path::{Path, PathBuf};

use super::assets::Shipped;
use super::init::{build_context, render_entry};
use super::manifest::{ManifestEntry, Ownership, RegionFormat, ScaffoldManifest};
use super::prior_release;
use super::region::{self, BlockOutcome};
use super::report::{Action, Report};
use super::settings_merge::{
    is_standing_note, merge_settings_from_baseline, merge_settings_from_prior_release,
};
use super::state::{
    guard_beneath_root, remove_beneath_root, set_exec, write_beneath_root, write_file,
    write_record, Baseline, InstalledFile, InstalledManifest, ProjectState, ScaffoldConfig,
    SyncBatch, PROJECT_TOML,
};
use super::version::check_write_version;
use super::{hash, pr_template, should_skip_initial_stack_adr, ScaffoldError, Shown};
use crate::hooks::policy_schema::DEPRECATED_KEYS;
use crate::shown;

mod pending;
use pending::PendingResolution;

/// Options for [`update`].
#[derive(Debug, Clone)]
pub struct UpdateOptions {
    /// Replace user-modified managed files with the new shipped version
    /// instead of merging.
    pub force: bool,
    /// The running binary's version — becomes the new scaffold version.
    pub binary_version: String,
    /// When set, write the report plus unified diffs of every applied change
    /// to this file.
    pub diff_out: Option<PathBuf>,
}

/// Runs update against an initialized project. Returns the report; the
/// caller prints it.
///
/// # Errors
///
/// [`ScaffoldError::NotInitialized`] when the project has no
/// `.codeflow/project.toml`; invalid versions or a binary older than the
/// project's pin refuse before writing. Otherwise IO, JSON, or manifest failures.
/// Per-file merge conflicts are NOT errors — they are reported.
pub fn update(
    source: &dyn super::AssetSource,
    root: &Path,
    opts: &UpdateOptions,
) -> Result<Report, ScaffoldError> {
    // One run flushes each touched directory and the device once (TSK-153).
    let batch = SyncBatch::begin();
    let report = update_writes(source, root, opts)?;
    batch.finish()?;
    Ok(report)
}

/// What [`update`] does with one shipped file, by its own rules. Doctor
/// reads it to offer `codeflow update` only where update repairs a file
/// (TSK-215).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// The update input could not be obtained; no repair is proven.
    Unproven(Shown),
    /// The file already is what update would leave.
    Current,
    /// Update writes the file: it installs or replaces it, merges it
    /// cleanly, or merges its managed entries or block. Holds the text it
    /// writes, which keeps any edit of the project's that the merge keeps.
    Rewrite(String),
    /// Update leaves the file as it is and writes `<dest>.new` beside it,
    /// holding the shipped version or a 3-way merge to resolve; holds that
    /// proposal.
    ConflictProposal(String),
    /// Update leaves a file that differs from the shipped version as it is
    /// and writes no `.new`: the shipped version has not changed since the
    /// recorded baseline, so the project's edit stands.
    KeptUserModification,
    /// Update skips the path, for this reason.
    Skipped(Shown),
}

/// What [`update`] without `--force` would do with the file at `dest`
/// (relative to `root`), decided by the same steps it takes and writing
/// nothing. `None` when update does not manage the file: the project
/// state, `[scaffold]` config or installed manifest does not read, or no
/// shipped entry for `dest` applies to the project's tier and permission
/// preset with managed ownership, whole or by region. A record in the
/// installed manifest alone never counts.
///
/// # Errors
///
/// Returns an error when an input to the update decision cannot be read or parsed.
pub fn decide(
    source: &dyn super::AssetSource,
    root: &Path,
    dest: &str,
    binary_version: &str,
) -> Result<Option<Decision>, ScaffoldError> {
    let state = ProjectState::load(root)?;
    let ignore = ScaffoldConfig::load(root)?;
    let manifest = ScaffoldManifest::load(source)?;
    let installed = InstalledManifest::load_or_default(root, &state.scaffold_version)?;
    let Some(entry) = manifest
        .entries
        .iter()
        .find(|entry| entry.dest == dest && entry.applies(state.tier, &state.permission_preset))
        .filter(|entry| {
            matches!(
                entry.ownership,
                Ownership::Managed | Ownership::ManagedRegion
            )
        })
    else {
        return Ok(None);
    };
    let kept_template = super::assets::read_text(source, "base/ci/pull_request_template.md")?
        .map(|shipped| pr_template::find_kept(root, &shipped, &installed))
        .transpose()?
        .flatten();
    if let Some(note) = skip_note(root, entry, &ignore, kept_template.as_ref())? {
        return Ok(Some(Decision::Skipped(note)));
    }
    match guard_beneath_root(root, Path::new(dest)) {
        Err(ScaffoldError::UnsafeSymlink { path }) => {
            let link = path.strip_prefix(root).unwrap_or(&path);
            return Ok(Some(Decision::Skipped(shown!(
                "{} is a symlink, and update never writes through one",
                Shown::path(link)
            ))));
        }
        Err(error) => return Err(error),
        Ok(_) => {}
    }
    let ctx = update_context(root, &state, binary_version);
    let mut scratch = Report::new(Shown::default());
    let Some(rendered) = render_entry(source, entry, &ctx, &mut scratch)? else {
        return Ok(None);
    };
    let current = super::state::read_beneath_root(root, dest)?;
    if entry.ownership == Ownership::Managed {
        // While the `.new` proposal is still beside the file, the conflict
        // stays open: preview it against the recorded base, as before the
        // pending record existed. The update itself still refuses.
        let proposal_open =
            super::state::read_beneath_root(root, &format!("{dest}.new"))?.is_some();
        let pending = if proposal_open {
            None
        } else {
            PendingResolution::read(root, dest, &installed)?
        };
        let recorded = pending
            .as_ref()
            .map(|pending| pending.new_sha256.as_str())
            .or_else(|| installed.files.get(dest).map(|f| f.sha256.as_str()));
        let base = pending
            .as_ref()
            .map(|pending| pending.shipped.clone())
            .or(Baseline::read(root, dest)?);
        let step = managed_step(
            current.as_deref(),
            &rendered,
            recorded,
            base.as_deref(),
            false,
        );
        return Ok(Some(step.decision(rendered)));
    }
    let format = entry.region.unwrap_or(RegionFormat::Markdown);
    let next = match (format, &current) {
        (RegionFormat::Json, None) => rendered.clone(),
        (RegionFormat::Json, Some(current)) => {
            let previous = Baseline::read(root, dest)?;
            merge_json_region(
                entry,
                current,
                previous.as_deref(),
                &rendered,
                &mut Vec::new(),
            )?
        }
        (format @ (RegionFormat::Markdown | RegionFormat::Hash), current) => {
            let version = ctx.get("SCAFFOLD_VERSION").unwrap_or("0");
            let extracted = region::extract_block(&rendered, format);
            let block = extracted
                .clone()
                .unwrap_or_else(|| region::wrap_block(&rendered, format, version));
            match current {
                Some(current) => region::upsert_block(current, &block, format).0,
                None if extracted.is_some() => rendered.clone(),
                None => format!("{block}\n"),
            }
        }
    };
    Ok(Some(match current {
        Some(current) if next == current && current == rendered => Decision::Current,
        Some(current) if next == current => Decision::KeptUserModification,
        _ => Decision::Rewrite(next),
    }))
}

/// Why update skips `entry` before reading it (`[scaffold] ignore`, the
/// brownfield starter decision, a kept PR template), as its report says;
/// `None` when update goes on to the file.
fn skip_note(
    root: &Path,
    entry: &ManifestEntry,
    ignore: &ScaffoldConfig,
    kept_template: Option<&pr_template::KeptTemplate>,
) -> Result<Option<Shown>, ScaffoldError> {
    if ignore.is_ignored(&entry.dest) {
        // Opted out via `[scaffold] ignore`: never rewrite it, and never
        // resurrect it if the user deleted it. Its manifest record and
        // baseline are left untouched so removing the glob later restores
        // normal management.
        return Ok(Some(Shown::fixed(
            "ignored via [scaffold] ignore in project.toml",
        )));
    }
    if should_skip_initial_stack_adr(root, &entry.dest)? {
        return Ok(Some(Shown::fixed(
            "brownfield repository already has ADRs; starter stack decision not added",
        )));
    }
    if let Some(kept) = kept_template.filter(|_| entry.dest == pr_template::MANAGED_TEMPLATE) {
        // A kept brownfield template is the project's (SPC-013 R-84):
        // never merged into, never shadowed by a sidecar or a second
        // template.
        return Ok(Some(shown!(
            "the project's PR template {} is kept; see git.pr_section_mapping",
            Shown::path(&kept.path)
        )));
    }
    Ok(None)
}

/// The template context update renders shipped files with.
fn update_context(
    root: &Path,
    state: &ProjectState,
    binary_version: &str,
) -> super::template::TemplateContext {
    let project = root
        .canonicalize()
        .ok()
        .as_deref()
        .unwrap_or(root)
        .file_name()
        .map_or_else(
            || "project".to_string(),
            |n| crate::git::GitName::from_os_str(n).display().to_string(),
        );
    build_context(
        &project,
        &state.product_one_liner,
        &state.areas,
        &state.stack,
        state.tier,
        binary_version,
    )
}

#[allow(clippy::too_many_lines)] // linear phase orchestration, as in `init`
fn update_writes(
    source: &dyn super::AssetSource,
    root: &Path,
    opts: &UpdateOptions,
) -> Result<Report, ScaffoldError> {
    let mut state = ProjectState::load(root)?;
    check_write_version(&state.scaffold_version, &opts.binary_version, "update")?;
    let ignore = ScaffoldConfig::load(root)?;
    let manifest = ScaffoldManifest::load(source)?;
    let mut installed = InstalledManifest::load_or_default(root, &state.scaffold_version)?;

    let ctx = update_context(root, &state, &opts.binary_version);

    let mut report = Report::new(shown!(
        "codeflow update (scaffold {} -> {}, {} tier)",
        Shown::version(&state.scaffold_version),
        Shown::version(&opts.binary_version),
        Shown::fixed(state.tier.as_str())
    ));
    // Validate every outstanding resolution before writing anything, including
    // state, reports and files that precede the conflicted entry in the manifest.
    // Include old entries so pruning cannot discard an unresolved proposal.
    let managed: std::collections::BTreeSet<_> = installed
        .files
        .iter()
        .filter(|(_, file)| file.ownership == Ownership::Managed)
        .map(|(dest, _)| dest.as_str())
        .chain(
            manifest
                .entries
                .iter()
                .filter(|entry| {
                    entry.ownership == Ownership::Managed
                        && entry.applies(state.tier, &state.permission_preset)
                })
                .map(|entry| entry.dest.as_str()),
        )
        .filter(|dest| !ignore.is_ignored(dest))
        .collect();
    let mut resolved = Vec::new();
    for dest in managed {
        if let Some(pending) = PendingResolution::read(root, dest, &installed)? {
            resolved.push((dest.to_string(), pending));
        }
    }
    for (dest, pending) in &resolved {
        Baseline::write(root, dest, &pending.shipped)?;
        // `read` requires a manifest entry with a known shipped hash.
        if let Some(file) = installed.files.get_mut(dest) {
            file.sha256 = hash::sha256_hex(pending.shipped.as_bytes());
        }
    }
    if !resolved.is_empty() {
        // Persist the accepted base before dropping its pending record. A retry
        // accepts either old or new metadata after an interrupted write.
        installed.store(root)?;
        for (dest, _) in &resolved {
            remove_beneath_root(root, &pending::path(dest))?;
        }
    }
    let mut diffs = Shown::default();
    let kept_template = super::assets::read_text(source, "base/ci/pull_request_template.md")?
        .map(|shipped| pr_template::find_kept(root, &shipped, &installed))
        .transpose()?
        .flatten();

    for entry in &manifest.entries {
        if !entry.applies(state.tier, &state.permission_preset) {
            continue;
        }
        if let Some(note) = skip_note(root, entry, &ignore, kept_template.as_ref())? {
            report.file_with_notes(&entry.dest, Action::Skipped, vec![note]);
            continue;
        }
        update_entry(
            source,
            root,
            entry,
            &ctx,
            &state,
            opts,
            &mut installed,
            &mut report,
            &mut diffs,
        )?;
    }

    prune_orphans(root, &manifest, &ignore, &mut installed, &mut report)?;
    for entry in manifest.entries.iter().filter(|entry| {
        entry.src == SECRET_SCAN_WORKFLOW
            && entry.applies(state.tier, &state.permission_preset)
            && !ignore.is_ignored(&entry.dest)
    }) {
        warn_steps_before_secret_scan(root, &entry.dest, &mut report);
    }
    if let Some(kept) = &kept_template {
        // A policy that cannot be read refuses; only a missing one skips
        // the diagnosis.
        if super::path_exists(&root.join(".codeflow").join("policy.json"))? {
            let diagnosis = pr_template::diagnose(root, kept, false)?;
            if let Some(line) = pr_template::describe(kept, &diagnosis, false) {
                report.notes.push(line);
            }
            if matches!(
                diagnosis,
                pr_template::Diagnosis::Diagnosed { .. }
                    | pr_template::Diagnosis::Recorded(
                        crate::hooks::policy::MappingState::Diagnosed
                    )
            ) {
                report.pending_pr_template = Some(kept.clone());
            }
        }
    }

    installed.scaffold_version.clone_from(&opts.binary_version);
    installed.store(root)?;
    state.scaffold_version.clone_from(&opts.binary_version);
    // The release rule's adoption marker (SPC-013 R-120): written once by
    // the release that brings the rule, never rewritten; never a table.
    state.release_rules.get_or_insert(1);
    state.store(root)?;
    if let Some(note) = record_work_records_baseline(root)? {
        report.notes.push(note);
    }
    if let Some(note) = record_outcome_marker(root)? {
        report.notes.push(note);
    }
    if let Some(note) = size_rule_note(root, &state) {
        report.notes.push(note);
    }
    // The brownfield hook choice stays visible until the adopter makes it.
    if super::detect::configured_hooks_path(root)?.as_deref()
        == Some(super::detect::CODEFLOW_HOOKS_PATH)
    {
        if let Some(found) = super::detect::git_dir_hooks(root) {
            report.notes.push(found.report_note());
        }
    }

    if let Some(path) = &opts.diff_out {
        let mut out = report.shown();
        if !diffs.is_empty() {
            out.push(&Shown::fixed("\n"));
            out.push(&diffs);
        }
        write_file(path, out.as_bytes())?;
        report
            .notes
            .push(shown!("diff written to {}", Shown::path(path)));
    }

    Ok(report)
}

/// The shipped CI workflow whose secret-scan job must run nothing but the
/// checkout before its gitleaks step (TSK-210).
const SECRET_SCAN_WORKFLOW: &str = "ci/codeflow-ci.yml";

/// Warns, on every update, about a step the project runs before the gitleaks
/// step in the secret-scan job of `dest` or of its conflict proposal
/// `dest.new`. Such a step can set the scan's environment through
/// `GITHUB_ENV` or `GITHUB_PATH`, and one that runs the pull request's code
/// lets the pull request hide its own leak. A 3-way merge keeps a step the
/// project added before this protection shipped, so the step is kept as
/// written and the warning stands until it moves. When the job has no step
/// update recognises as the scan (a renamed scan step, say), it says the
/// order needs a person's review instead of guessing.
fn warn_steps_before_secret_scan(root: &Path, dest: &str, report: &mut Report) {
    for path in [dest.to_string(), format!("{dest}.new")] {
        let Ok(text) = std::fs::read_to_string(root.join(&path)) else {
            continue;
        };
        let name = Shown::path(&path);
        match scan_order(&text) {
            ScanOrder::NotChecked => {}
            ScanOrder::Checked(steps) if steps.is_empty() => {}
            ScanOrder::Checked(steps) => report.warnings.push(shown!(
                "{}: the secret-scan job runs {} before its gitleaks step. A step there can \
                 set the scan's environment through GITHUB_ENV or GITHUB_PATH, and one that runs \
                 the pull request's code lets the pull request hide a leak from the scan; move it \
                 after the gitleaks step or into another job. Update kept it as you wrote it.",
                name,
                Shown::join(
                    steps
                        .iter()
                        .map(|step| shown!("step {}", Shown::count(*step))),
                    ", "
                )
            )),
            ScanOrder::Unrecognized => report.warnings.push(shown!(
                "{}: the secret-scan job has no step named `gitleaks` with TRUSTED_SHA in its \
                 env, so update cannot check what runs before the scan; review the job's step \
                 order by hand. A step before the scan can set its environment through GITHUB_ENV \
                 or GITHUB_PATH, so move any step other than the checkout after the scan or into \
                 another job.",
                name
            )),
        }
    }
}

/// What update can tell about the steps a workflow's `secret-scan` job runs
/// before its gitleaks step.
#[derive(Debug, PartialEq, Eq)]
enum ScanOrder {
    /// The workflow does not parse (a conflict proposal with markers) or has
    /// no `secret-scan` job: nothing to check.
    NotChecked,
    /// The job exists, but no step is the shipped scan step (named
    /// `gitleaks`, with `TRUSTED_SHA` in its env), so its order needs a
    /// person's review.
    Unrecognized,
    /// The steps before the scan other than `actions/checkout`, each by its
    /// position in the job, counted from 1; empty when there are none. A
    /// step is never named by its `name`, `uses` or `run`, which the project
    /// writes and which can hold a secret (TSK-1029).
    Checked(Vec<usize>),
}

fn scan_order(workflow: &str) -> ScanOrder {
    use serde_yaml::Value;
    let Ok(doc) = serde_yaml::from_str::<Value>(workflow) else {
        return ScanOrder::NotChecked;
    };
    let Some(job) = doc.get("jobs").and_then(|jobs| jobs.get("secret-scan")) else {
        return ScanOrder::NotChecked;
    };
    let Some(steps) = job.get("steps").and_then(Value::as_sequence) else {
        return ScanOrder::Unrecognized;
    };
    let text = |step: &Value, key: &str| step.get(key).and_then(Value::as_str).map(str::to_string);
    // The scanner is the shipped step: named `gitleaks` and reading the
    // trusted commit from its env. A step that merely mentions gitleaks, such
    // as a canary or an install, is never taken for it, and a renamed scan
    // step is not guessed at.
    let is_scan = |step: &Value| {
        text(step, "name").as_deref() == Some("gitleaks")
            && step
                .get("env")
                .and_then(|env| env.get("TRUSTED_SHA"))
                .is_some()
    };
    let Some(scan) = steps.iter().position(is_scan) else {
        return ScanOrder::Unrecognized;
    };
    ScanOrder::Checked(
        steps[..scan]
            .iter()
            .enumerate()
            .filter(|(_, step)| {
                !(text(step, "uses").is_some_and(|uses| uses.starts_with("actions/checkout@"))
                    && step.get("run").is_none())
            })
            .map(|(index, _)| index + 1)
            .collect(),
    )
}

/// The work-record migration (SPC-013 R-83): an existing project whose
/// records predate the lifecycle rules gets its migration baseline recorded
/// once, as the current `HEAD`, so older records keep their exact blobs
/// exempt and the rules apply by transition from here. A project with no
/// record, no commit, or a recorded baseline is left alone.
fn record_work_records_baseline(root: &Path) -> Result<Option<Shown>, ScaffoldError> {
    use crate::workgraph::lifecycle::{recorded_baseline, Graph, BASELINE_KEY};
    // The readers' own errors quote what the records and the state file
    // hold (a YAML diagnostic quotes the frontmatter), so each is named by
    // a fixed text and the command that shows the detail (TSK-1029).
    if !recorded_baseline(root)
        .map_err(|_| {
            ScaffoldError::Git(shown!(
                "cannot read {} in {}; run `codeflow validate` to see why",
                Shown::fixed(BASELINE_KEY),
                Shown::path(PROJECT_TOML)
            ))
        })?
        .is_empty()
        || Graph::from_worktree(root)
            .map_err(|_| {
                ScaffoldError::Git(Shown::fixed(
                    "cannot read the work records for the migration; run `codeflow validate` \
                     to see which record does not parse; update changed nothing past this point",
                ))
            })?
            .records
            .is_empty()
    {
        return Ok(None);
    }
    let Some(repo) = crate::hooks::repo::open(root)
        .map_err(|_| ScaffoldError::Git(Shown::fixed("cannot read migration repository")))?
    else {
        return Ok(None);
    };
    let head = match repo.head() {
        Ok(head) => head,
        Err(error) if error.code() == git2::ErrorCode::UnbornBranch => return Ok(None),
        Err(_) => {
            return Err(ScaffoldError::Git(Shown::fixed(
                "cannot read migration HEAD",
            )))
        }
    };
    let head = head
        .peel_to_commit()
        .map_err(|_| ScaffoldError::Git(Shown::fixed("cannot read migration commit")))?
        .id();
    let path = ProjectState::path(root);
    let text = std::fs::read_to_string(&path).map_err(|e| ScaffoldError::io(&path, e))?;
    let mut table: toml::Table = toml::from_str(&text).map_err(|e| {
        ScaffoldError::invalid(
            Shown::path(&path),
            Shown::place(&crate::scaffold::ParsePlace::toml(&text, &e)),
        )
    })?;
    table.insert(
        BASELINE_KEY.to_string(),
        toml::Value::String(head.to_string()),
    );
    let next = super::state::state_text(&table).map_err(|_| {
        ScaffoldError::invalid(
            Shown::path(&path),
            Shown::fixed("the project state could not be written as TOML"),
        )
    })?;
    write_record(root, PROJECT_TOML, next.as_bytes())?;
    Ok(Some(shown!(
        "recorded {} = {}: work-record rules apply to records changed after this commit",
        Shown::fixed(BASELINE_KEY),
        Shown::object_id(head)
    )))
}

/// The report line for the size rule's default (TSK-323): a project that
/// `update` brings forward, which `init` never marked a new scaffold,
/// reports large files and refuses none until its policy sets
/// `git.max_file_bytes`.
fn size_rule_note(root: &Path, state: &ProjectState) -> Option<Shown> {
    let opted_in = crate::hooks::policy::Policy::load(root)
        .is_ok_and(|policy| policy.git.max_file_bytes.is_some());
    (state.file_size_rule.is_none() && !opted_in).then(|| {
        Shown::fixed(
            "file size rule: files over 1 MiB and over 10 MiB are report lines in this project, never refused; to refuse files over a limit, set git.max_file_bytes in .codeflow/policy.json on the default branch once every binary that judges it is 4.0.0 or later",
        )
    })
}

/// The Outcome section's adoption marker (TSK-435): written once, by the
/// first `update` (or `init`) of a release that has the section, as the
/// top-level `outcome_present_uids`, the `uid` of every task and epic
/// record in the working tree, committed or not, and
/// `outcome_required_since`, the project's `HEAD` when it has one. Those
/// records only warn without the section; every later record must state
/// it. A marker already present, in any shape, is left as written.
pub(crate) fn record_outcome_marker(root: &Path) -> Result<Option<Shown>, ScaffoldError> {
    use crate::workgraph::outcome::{present_uids, with_marker, MARKER_KEY};
    // The readers' own errors quote record paths and what the records hold,
    // so each is named by a fixed text and the command that shows the
    // detail (TSK-1029).
    let path = ProjectState::path(root);
    let text = std::fs::read_to_string(&path).map_err(|e| ScaffoldError::io(&path, e))?;
    let table: toml::Table = toml::from_str(&text).map_err(|e| {
        ScaffoldError::invalid(
            Shown::path(&path),
            Shown::place(&crate::scaffold::ParsePlace::toml(&text, &e)),
        )
    })?;
    if table.contains_key(MARKER_KEY) {
        return Ok(None);
    }
    let uids = present_uids(root).map_err(|_| {
        ScaffoldError::Git(Shown::fixed(
            "cannot read the work records for the Outcome marker; run `codeflow validate` \
             to see which record does not read; update changed nothing past this point",
        ))
    })?;
    let head = match crate::hooks::repo::open(root)
        .map_err(|_| ScaffoldError::Git(Shown::fixed("cannot read the repository")))?
    {
        None => None,
        Some(repo) => match repo.head() {
            Ok(head) => Some(
                head.peel_to_commit()
                    .map_err(|_| {
                        ScaffoldError::Git(Shown::fixed("cannot read the adoption commit"))
                    })?
                    .id(),
            ),
            // A repository with no commit yet: the marker has no commit.
            Err(error) if error.code() == git2::ErrorCode::UnbornBranch => None,
            Err(_) => {
                return Err(ScaffoldError::Git(Shown::fixed(
                    "cannot read the adoption HEAD",
                )))
            }
        },
    };
    let commit = head.map(|head| head.to_string());
    let next =
        super::state::state_text(&with_marker(table, commit.as_deref(), &uids)).map_err(|_| {
            ScaffoldError::invalid(
                Shown::path(&path),
                Shown::fixed("the project state could not be written as TOML"),
            )
        })?;
    write_record(root, PROJECT_TOML, next.as_bytes())?;
    Ok(Some(shown!(
        "recorded {} with {} present record(s){}: they warn without an `## Outcome` section, and every later open task or epic must state one",
        Shown::fixed(MARKER_KEY),
        Shown::count(uids.len()),
        head.map(|head| shown!(" at {}", Shown::object_id(head)))
            .unwrap_or_default()
    )))
}

/// Policy values a released binary no longer accepts, rewritten to their
/// nearest accepted value with a note. Only `git.work_records = "off"` from
/// an unreleased build qualifies (SPC-013 R-81); every other value is kept.
fn migrate_policy_values(dest: &str, user: &mut serde_json::Value) -> Vec<Shown> {
    if dest != ".codeflow/policy.json" {
        return Vec::new();
    }
    match user.pointer_mut("/git/work_records") {
        Some(value) if value.as_str() == Some("off") => {
            *value = serde_json::Value::from("warn");
            vec![Shown::fixed(
                "git.work_records `off` no longer exists; rewritten to `warn`",
            )]
        }
        _ => Vec::new(),
    }
}

fn record(installed: &mut InstalledManifest, entry: &ManifestEntry, sha256: String) {
    installed.files.insert(
        entry.dest.clone(),
        InstalledFile {
            src: entry.src.clone(),
            ownership: entry.ownership,
            sha256,
            exec: entry.exec,
        },
    );
}

/// A manifest `dest` is trusted only when it is a normal project-relative path:
/// not absolute, no `..` traversal, no root/prefix component. A tampered
/// manifest could otherwise point `dest` outside the repo, and `root.join`
/// would resolve there — letting update write or delete an arbitrary file.
fn is_safe_relative_dest(dest: &str) -> bool {
    use std::path::{Component, Path};
    let p = Path::new(dest);
    !p.is_absolute()
        && p.components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

fn write_dest(root: &Path, entry: &ManifestEntry, content: &str) -> Result<(), ScaffoldError> {
    if !is_safe_relative_dest(&entry.dest) {
        return Err(ScaffoldError::ManifestInvalid(shown!(
            "refusing to write outside the repo: unsafe dest {}",
            Shown::path(&entry.dest)
        )));
    }
    // Beneath-root, no-follow: refuses a leaf or ancestor symlink so the write
    // (and the exec-bit set that follows) never escapes the tree.
    let path = guard_beneath_root(root, Path::new(&entry.dest))?;
    write_file(&path, content.as_bytes())?;
    if entry.exec {
        set_exec(&path, true)?;
    }
    Ok(())
}

/// Append the section for `dest` to the `--diff` export. Every caller
/// goes through here, and the section is judged only against the asset
/// embedded in this binary (`raw`, and `rendered` as update writes it):
/// the export never prints a value, key or line from the project's files or
/// from the recorded baseline, which the project can write (TSK-1029,
/// `diff_redact`).
fn push_diff(
    diffs: &mut Shown,
    dest: &str,
    old: &str,
    new: &str,
    rendered: &str,
    raw: Option<Shipped<'_>>,
) {
    diffs.push(&super::diff_redact::section(
        dest,
        old,
        new,
        super::diff_redact::Reference { rendered, raw },
    ));
}

/// Update's step for a whole managed file, from the file on disk (`None`
/// when absent), the shipped rendering, the installed record's hash, the
/// baseline and `--force`. [`update_entry`] applies it; [`decide`] reports
/// it.
enum ManagedStep {
    /// Absent: install it.
    Add,
    /// Already the shipped version: adopt it.
    Adopt,
    /// Unmodified since it was installed: replace it.
    Replace,
    /// Modified, replaced under `--force`.
    Force,
    /// Modified, with no baseline to merge against: propose the shipped
    /// version in `.new`.
    NoBaseline,
    /// Modified, and the shipped version equals the baseline: keep it.
    Kept,
    /// Modified, and the 3-way merge applies cleanly: write the merge.
    Merge(String),
    /// Modified, and the 3-way merge conflicts: propose it in `.new`.
    Conflict(String),
}

impl ManagedStep {
    fn decision(self, rendered: String) -> Decision {
        match self {
            Self::Add | Self::Replace | Self::Force => Decision::Rewrite(rendered),
            Self::Merge(merged) => Decision::Rewrite(merged),
            Self::Adopt => Decision::Current,
            Self::NoBaseline => Decision::ConflictProposal(rendered),
            Self::Conflict(proposal) => Decision::ConflictProposal(proposal),
            Self::Kept => Decision::KeptUserModification,
        }
    }
}

fn managed_step(
    current: Option<&str>,
    rendered: &str,
    recorded: Option<&str>,
    base: Option<&str>,
    force: bool,
) -> ManagedStep {
    let Some(current) = current else {
        return ManagedStep::Add;
    };
    if current == rendered {
        return ManagedStep::Adopt;
    }
    if recorded == Some(hash::sha256_hex(current.as_bytes()).as_str()) {
        return ManagedStep::Replace;
    }
    if force {
        return ManagedStep::Force;
    }
    let Some(base) = base else {
        return ManagedStep::NoBaseline;
    };
    if rendered == base {
        return ManagedStep::Kept;
    }
    match diffy::merge(base, current, rendered) {
        Ok(merged) => ManagedStep::Merge(merged),
        Err(conflicted) => ManagedStep::Conflict(conflicted),
    }
}

/// Update's merge of a JSON settings file by its managed hook entries:
/// against the recorded baseline or, with none, the settings an earlier
/// release shipped.
fn merge_json_region(
    entry: &ManifestEntry,
    current: &str,
    previous: Option<&str>,
    rendered: &str,
    lines: &mut Vec<Shown>,
) -> Result<String, ScaffoldError> {
    let prior = previous
        .is_none()
        .then(|| prior_release::settings(&entry.src))
        .flatten();
    if let Some(prior) = prior {
        let merged = merge_settings_from_prior_release(current, prior, rendered, lines)?;
        lines.insert(0, prior_release_note());
        Ok(merged)
    } else {
        merge_settings_from_baseline(current, previous, rendered, lines)
    }
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn update_entry(
    source: &dyn super::AssetSource,
    root: &Path,
    entry: &ManifestEntry,
    ctx: &super::template::TemplateContext,
    _state: &ProjectState,
    opts: &UpdateOptions,
    installed: &mut InstalledManifest,
    report: &mut Report,
    diffs: &mut Shown,
) -> Result<(), ScaffoldError> {
    // Refuse to touch an entry whose on-disk dest traverses a symlink (leaf or
    // ancestor): a pre-planted link would otherwise let the write below escape
    // the repo. Reported, never silent (module invariant #2).
    if let Err(e) = guard_beneath_root(root, Path::new(&entry.dest)) {
        report.file_with_notes(&entry.dest, Action::Skipped, vec![Shown::from(&e)]);
        return Ok(());
    }
    let Some(rendered) = render_entry(source, entry, ctx, report)? else {
        return Ok(());
    };
    // The embedded asset, before templating: what the `--diff` export may
    // show of this file (TSK-1029).
    let asset = super::assets::read_shipped(source, &format!("base/{}", entry.src))?;
    let raw = asset.as_ref().map(super::assets::AssetText::shipped);
    let dest_path = root.join(&entry.dest);
    let exists = super::path_exists(&dest_path)?;
    let version = ctx.get("SCAFFOLD_VERSION").unwrap_or("0");

    match entry.ownership {
        Ownership::Managed => {
            let current = if exists {
                Some(
                    std::fs::read_to_string(&dest_path)
                        .map_err(|e| ScaffoldError::io(&dest_path, e))?,
                )
            } else {
                None
            };
            let old = current.as_deref().unwrap_or_default();
            let recorded = installed.files.get(&entry.dest).map(|f| f.sha256.clone());
            let base = Baseline::read(root, &entry.dest)?;
            match managed_step(
                current.as_deref(),
                &rendered,
                recorded.as_deref(),
                base.as_deref(),
                opts.force,
            ) {
                ManagedStep::Add => {
                    // New manifest entry (or deleted file): install it.
                    write_dest(root, entry, &rendered)?;
                    record(installed, entry, hash::sha256_hex(rendered.as_bytes()));
                    Baseline::write(root, &entry.dest, &rendered)?;
                    report.file(&entry.dest, Action::Added);
                }
                ManagedStep::Adopt => {
                    // Already at the new version (however it got there): adopt.
                    record(installed, entry, hash::sha256_hex(old.as_bytes()));
                    Baseline::write(root, &entry.dest, &rendered)?;
                    report.file(&entry.dest, Action::Unchanged);
                }
                ManagedStep::Replace => {
                    write_dest(root, entry, &rendered)?;
                    record(installed, entry, hash::sha256_hex(rendered.as_bytes()));
                    Baseline::write(root, &entry.dest, &rendered)?;
                    push_diff(diffs, &entry.dest, old, &rendered, &rendered, raw);
                    report.file(&entry.dest, Action::Changed);
                }
                ManagedStep::Force => {
                    write_dest(root, entry, &rendered)?;
                    record(installed, entry, hash::sha256_hex(rendered.as_bytes()));
                    Baseline::write(root, &entry.dest, &rendered)?;
                    push_diff(diffs, &entry.dest, old, &rendered, &rendered, raw);
                    report.file_with_notes(
                        &entry.dest,
                        Action::Forced,
                        vec![Shown::fixed("user modifications overwritten (--force)")],
                    );
                }
                ManagedStep::NoBaseline => {
                    let new_path = format!("{}.new", entry.dest);
                    write_beneath_root(root, &new_path, rendered.as_bytes())?;
                    report.file_with_notes(
                        &entry.dest,
                        Action::Conflicted,
                        vec![shown!(
                            "no baseline available for 3-way merge; new version written to {}",
                            Shown::path(&new_path)
                        )],
                    );
                }
                ManagedStep::Kept => {
                    report.file_with_notes(
                        &entry.dest,
                        Action::KeptUserModified,
                        vec![Shown::fixed("no upstream change; your modifications stand")],
                    );
                }
                ManagedStep::Merge(merged) => {
                    write_dest(root, entry, &merged)?;
                    // Record the pristine shipped hash (not the merged file's), so the
                    // manifest invariant `recorded == hash(baseline)` holds: the merged
                    // file carries user edits, so the next update must classify it
                    // "user-modified" and re-merge, never treat it as pristine and
                    // overwrite. Recording hash(merged) here silently wiped the merge
                    // on the following update.
                    record(installed, entry, hash::sha256_hex(rendered.as_bytes()));
                    Baseline::write(root, &entry.dest, &rendered)?;
                    push_diff(diffs, &entry.dest, old, &merged, &rendered, raw);
                    report.file_with_notes(
                        &entry.dest,
                        Action::Merged,
                        vec![Shown::fixed(
                            "3-way merge applied cleanly (base = shipped baseline)",
                        )],
                    );
                }
                ManagedStep::Conflict(conflicted) => {
                    // The proposal is the 3-way merge with conflict markers, not
                    // the bare shipped file: every change of yours that does not
                    // overlap an upstream change (a job you added to a workflow,
                    // a section you appended) is kept in it, and only the
                    // overlapping hunks wait for you.
                    let new_path = format!("{}.new", entry.dest);
                    write_beneath_root(root, &new_path, conflicted.as_bytes())?;
                    PendingResolution::write(
                        root,
                        &entry.dest,
                        base.as_deref().expect("conflict requires a baseline"),
                        &rendered,
                    )?;
                    let proposal = Shown::path(&new_path);
                    let mut notes = vec![shown!(
                        "your modifications conflict with the new shipped version; file untouched. \
                         {} holds the 3-way merge: your changes are kept and each \
                         overlapping hunk carries conflict markers. Resolve them, move {} \
                         over the file, then rerun codeflow update to record the shipped baseline",
                        proposal,
                        proposal
                    )];
                    if entry.src == "ci/codeflow-ci.yml" {
                        notes.push(Shown::fixed(
                            "the commit standards job moved to .github/workflows/codeflow-policy.yml on \
                             pull_request_target; drop the commit-lint job from this file when you resolve",
                        ));
                    }
                    report.file_with_notes(&entry.dest, Action::Conflicted, notes);
                }
            }
        }
        Ownership::ManagedRegion => match entry.region.unwrap_or(RegionFormat::Markdown) {
            RegionFormat::Json => {
                if !exists {
                    write_dest(root, entry, &rendered)?;
                    record(installed, entry, hash::sha256_hex(rendered.as_bytes()));
                    Baseline::write(root, &entry.dest, &rendered)?;
                    report.file(&entry.dest, Action::Added);
                    return Ok(());
                }
                let current = std::fs::read_to_string(&dest_path)
                    .map_err(|e| ScaffoldError::io(&dest_path, e))?;
                let previous = Baseline::read(root, &entry.dest)?;
                let mut lines = vec![];
                let merged =
                    merge_json_region(entry, &current, previous.as_deref(), &rendered, &mut lines)?;
                record(installed, entry, hash::sha256_hex(rendered.as_bytes()));
                Baseline::write(root, &entry.dest, &rendered)?;
                if merged == current {
                    // A standing condition is reported on every run: a kept
                    // removal means the project still runs without a shipped
                    // rule, and a malformed or partial `env` stays as it is.
                    lines.retain(|line| is_standing_note(line));
                    report.file_with_notes(&entry.dest, Action::Unchanged, lines);
                } else {
                    write_dest(root, entry, &merged)?;
                    push_diff(diffs, &entry.dest, &current, &merged, &rendered, raw);
                    report.file_with_notes(&entry.dest, Action::Merged, lines);
                }
            }
            format @ (RegionFormat::Markdown | RegionFormat::Hash) => {
                let block = region::extract_block(&rendered, format)
                    .unwrap_or_else(|| region::wrap_block(&rendered, format, version));
                record(installed, entry, hash::sha256_hex(block.as_bytes()));
                Baseline::write(root, &entry.dest, &block)?;
                if !exists {
                    let content = if region::extract_block(&rendered, format).is_some() {
                        rendered.clone()
                    } else {
                        format!("{block}\n")
                    };
                    write_dest(root, entry, &content)?;
                    report.file(&entry.dest, Action::Added);
                    return Ok(());
                }
                let current = std::fs::read_to_string(&dest_path)
                    .map_err(|e| ScaffoldError::io(&dest_path, e))?;
                let (next, outcome) = region::upsert_block(&current, &block, format);
                match outcome {
                    BlockOutcome::Unchanged => report.file(&entry.dest, Action::Unchanged),
                    BlockOutcome::Replaced => {
                        write_dest(root, entry, &next)?;
                        push_diff(diffs, &entry.dest, &current, &next, &block, raw);
                        report.file_with_notes(
                            &entry.dest,
                            Action::Changed,
                            vec![Shown::fixed(
                                "managed block regenerated; content outside markers untouched",
                            )],
                        );
                    }
                    BlockOutcome::Appended => {
                        write_dest(root, entry, &next)?;
                        push_diff(diffs, &entry.dest, &current, &next, &block, raw);
                        report.file_with_notes(
                            &entry.dest,
                            Action::Merged,
                            vec![Shown::fixed(
                                "markers were missing; codeflow block re-appended",
                            )],
                        );
                    }
                }
            }
        },
        Ownership::UserOwned => {
            if !exists {
                write_dest(root, entry, &rendered)?;
                record(installed, entry, hash::sha256_hex(rendered.as_bytes()));
                Baseline::write(root, &entry.dest, &rendered)?;
                report.file(&entry.dest, Action::Added);
                return Ok(());
            }
            let is_json = Path::new(&entry.dest)
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("json"));
            if is_json {
                sync_user_owned_json(root, entry, &rendered, raw, installed, report, diffs)?;
            } else {
                let mut notes = vec![Shown::fixed("user-owned: never mutated by update")];
                let is_new_entry = !installed.files.contains_key(&entry.dest);
                let baseline_missing = Baseline::read(root, &entry.dest)?.is_none();
                if is_new_entry || baseline_missing {
                    Baseline::write(root, &entry.dest, &rendered)?;
                    record(installed, entry, hash::sha256_hex(rendered.as_bytes()));
                    notes.push(Shown::fixed(if is_new_entry {
                        "existing file adopted into the installed manifest"
                    } else {
                        "missing shipped baseline restored without changing the live file"
                    }));
                }
                report.file_with_notes(&entry.dest, Action::Skipped, notes);
            }
        }
    }
    Ok(())
}

/// Additive key sync for schema-versioned, user-owned JSON (policy.json):
/// keys present in the new shipped default but absent from BOTH the user's
/// file and the old shipped default (baseline) are added with their default
/// values. User values are never mutated; deliberate deletions (key existed
/// in the old default) are respected.
fn sync_user_owned_json(
    root: &Path,
    entry: &ManifestEntry,
    rendered: &str,
    raw: Option<Shipped<'_>>,
    installed: &mut InstalledManifest,
    report: &mut Report,
    diffs: &mut Shown,
) -> Result<(), ScaffoldError> {
    let dest_path = root.join(&entry.dest);
    let current_text =
        std::fs::read_to_string(&dest_path).map_err(|e| ScaffoldError::io(&dest_path, e))?;
    let mut user: serde_json::Value = serde_json::from_str(&current_text)?;
    let new_default: serde_json::Value = serde_json::from_str(rendered)?;
    let old_default: Option<serde_json::Value> = Baseline::read(root, &entry.dest)?
        .map(|t| serde_json::from_str(&t))
        .transpose()?;

    let mut added: Vec<String> = vec![];
    let mut moved: Vec<MovedDefault> = vec![];
    let mut recommended: Vec<Shown> = vec![];
    // Without a recorded baseline, the policies earlier versions shipped
    // stand in for it: the oldest decides which keys are new, and a value
    // still equal to any of their defaults moves.
    let prior: Vec<serde_json::Value> = if old_default.is_none() {
        prior_release::policies(&entry.src)
            .into_iter()
            .flatten()
            .map(serde_json::from_str)
            .collect::<Result<_, _>>()?
    } else {
        vec![]
    };
    if let Some(old) = old_default.as_ref().or(prior.first()) {
        add_new_keys(&mut user, Some(old), &new_default, "", &mut added);
        move_unchanged_defaults(&mut user, old, &new_default, "", &mut moved);
        recommended = recommend_defaults(&user, old, &new_default, raw);
    }
    for old in prior.iter().skip(1) {
        move_unchanged_defaults(&mut user, old, &new_default, "", &mut moved);
    }

    // Keys the new shipped default dropped and the schema deprecates are
    // deleted (TSK-137): they are inert, and the loader warns while they stay.
    let mut removed = remove_deprecated_keys(&mut user, &new_default);
    removed.extend(remove_unsupported_defaults(
        &mut user,
        old_default.as_ref(),
        &new_default,
    ));
    added.extend(removed.iter().map(|k| format!("-{k}")));

    let migrated = migrate_policy_values(&entry.dest, &mut user);

    // Refresh the shipped-default baseline and record either way.
    Baseline::write(root, &entry.dest, rendered)?;
    record(installed, entry, hash::sha256_hex(rendered.as_bytes()));

    if added.is_empty() && migrated.is_empty() && moved.is_empty() {
        let mut notes = vec![Shown::fixed(
            "user-owned: values never mutated; no new default keys",
        )];
        notes.extend(recommended);
        notes.extend(unsupported_policy_notes(&user));
        if !prior.is_empty() {
            notes.push(prior_release_note());
        } else if old_default.is_none() {
            notes.push(Shown::fixed(
                "no shipped-default baseline existed; key sync starts from this version",
            ));
        }
        report.file_with_notes(&entry.dest, Action::Skipped, notes);
        return Ok(());
    }

    // schema_version awareness: carry the new default's schema_version when
    // keys were added and the user has not customized it past the default.
    let mut notes: Vec<Shown> = added.iter().map(|key| key_change_note(key, raw)).collect();
    notes.extend(moved.iter().map(|change| change.note(raw)));
    if !prior.is_empty() {
        notes.insert(0, prior_release_note());
    }
    let migrated_any = !migrated.is_empty();
    notes.extend(migrated);
    notes.extend(recommended);
    notes.extend(unsupported_policy_notes(&user));
    let mut schema_advanced = false;
    if let (Some(user_sv), Some(new_sv)) = (
        user.get("schema_version")
            .and_then(serde_json::Value::as_u64),
        new_default
            .get("schema_version")
            .and_then(serde_json::Value::as_u64),
    ) {
        if new_sv > user_sv {
            user["schema_version"] = serde_json::Value::from(new_sv);
            // The project's own number is not shown; the new one is the
            // shipped default's.
            notes.push(shown!(
                "schema_version advanced to {}",
                Shown::number(new_sv)
            ));
            schema_advanced = true;
        }
    }

    // Splice only the added keys into the adopter's bytes; a migrated value
    // or a layout the splice cannot reproduce falls back to a rewrite.
    let preserved = if migrated_any {
        None
    } else {
        preserving_edit(&current_text, &user, &added, &moved, schema_advanced)
    };
    let next = if let Some(text) = preserved {
        text
    } else {
        let mut text = serde_json::to_string_pretty(&user)?;
        text.push('\n');
        text
    };
    write_file(&dest_path, next.as_bytes())?;
    push_diff(diffs, &entry.dest, &current_text, &next, rendered, raw);
    report.file_with_notes(&entry.dest, Action::KeysAdded, notes);
    Ok(())
}

/// The report line for an update that compared with an earlier release's
/// shipped copy because no baseline was recorded.
fn prior_release_note() -> Shown {
    shown!(
        "no shipped-default baseline was recorded; compared with the copy CodeFlow {} shipped",
        Shown::fixed(prior_release::RELEASE)
    )
}

/// A policy key path, by the name `CodeFlow` defines: the schema's own path
/// for the key, or each segment as the embedded default (`raw`) holds it
/// quoted; any other key is a placeholder (TSK-1029).
fn policy_key(path: &str, raw: Option<Shipped<'_>>) -> Shown {
    if let Some(spec) = crate::hooks::policy_schema::spec_for(path) {
        return Shown::fixed(spec.path);
    }
    if let Some((key, _)) = DEPRECATED_KEYS.iter().find(|(key, _)| *key == path) {
        return Shown::fixed(key);
    }
    let segments: Option<Vec<Shown>> = raw.and_then(|raw| {
        path.split('.')
            .map(|segment| {
                raw.as_str()
                    .contains(&format!("\"{segment}\""))
                    .then(|| Shown::shipped(segment, raw))
                    .flatten()
            })
            .collect()
    });
    segments.map_or_else(
        || Shown::fixed("<a key the shipped default does not hold>"),
        |segments| Shown::join(segments, "."),
    )
}

/// A shipped default value, as JSON, when the embedded default (`raw`)
/// holds it as written.
fn shipped_value(value: &serde_json::Value, raw: Option<Shipped<'_>>) -> Shown {
    raw.and_then(|raw| Shown::shipped(&value.to_string(), raw))
        .unwrap_or_else(|| Shown::fixed("<the new shipped default>"))
}

/// The adopter's file with only the `added` keys (and removed deprecated
/// ones), and an advanced `schema_version`, spliced in, when that reproduces
/// `user` exactly: every
/// byte the adopter wrote stays as written (TSK-107 review F6).
fn preserving_edit(
    current: &str,
    user: &serde_json::Value,
    added: &[String],
    moved: &[MovedDefault],
    schema_advanced: bool,
) -> Option<String> {
    let mut text = current.to_string();
    for change in moved {
        let parts: Vec<&str> = change.path.split('.').collect();
        text = super::json_edit::replace_value(&text, &parts, &change.new.to_string())?;
    }
    for path in added {
        // A `-` marks a deprecated key the sync removed.
        if let Some(gone) = path.strip_prefix('-') {
            let parts: Vec<&str> = gone.split('.').collect();
            text = super::json_edit::remove_member_line(&text, &parts)?;
            continue;
        }
        let parts: Vec<&str> = path.split('.').collect();
        let (key, parent) = parts.split_last()?;
        let value = user.pointer(&format!("/{}", parts.join("/")))?;
        text = super::json_edit::insert_member(
            &text,
            parent,
            key,
            &serde_json::to_string(value).ok()?,
        )?;
    }
    if schema_advanced {
        let version = user.get("schema_version")?.to_string();
        text = super::json_edit::replace_value(&text, &["schema_version"], &version)?;
    }
    super::json_edit::verified(text, user)
}

/// A policy scalar that still held the prior shipped default and moved to
/// the new one (ADR-0075, TSK-171).
struct MovedDefault {
    path: String,
    new: serde_json::Value,
}

impl MovedDefault {
    /// The report line: the key and the new shipped value, never the old
    /// value, which comes from the project's file and its recorded baseline
    /// (TSK-1029).
    fn note(&self, raw: Option<Shipped<'_>>) -> Shown {
        shown!(
            "moved {} to {}: it held the previous shipped default. To keep the \
             previous value, set it again in .codeflow/policy.json; update keeps a \
             value that differs from the shipped default",
            policy_key(&self.path, raw),
            shipped_value(&self.new, raw)
        )
    }
}

/// Move every scalar leaf whose value equals the prior shipped default to
/// the new default, when the default changed. A value that differs from the
/// prior default is the project's choice and is kept; arrays and objects are
/// never replaced here.
fn move_unchanged_defaults(
    user: &mut serde_json::Value,
    old_default: &serde_json::Value,
    new_default: &serde_json::Value,
    path: &str,
    moved: &mut Vec<MovedDefault>,
) {
    let (Some(user_obj), Some(new_obj)) = (user.as_object_mut(), new_default.as_object()) else {
        return;
    };
    for (key, new_val) in new_obj {
        let Some(old_val) = old_default.get(key) else {
            continue;
        };
        let Some(user_val) = user_obj.get_mut(key) else {
            continue;
        };
        let key_path = if path.is_empty() {
            key.clone()
        } else {
            format!("{path}.{key}")
        };
        if new_val.is_object() {
            move_unchanged_defaults(user_val, old_val, new_val, &key_path, moved);
            continue;
        }
        let scalar = |value: &serde_json::Value| !value.is_object() && !value.is_array();
        if key_path != "schema_version"
            && scalar(new_val)
            && scalar(old_val)
            && old_val != new_val
            && *user_val == *old_val
        {
            *user_val = new_val.clone();
            moved.push(MovedDefault {
                path: key_path,
                new: new_val.clone(),
            });
        }
    }
}

/// Changed shipped defaults that `codeflow update` recommends to an existing
/// install whose value differs from both the old and the new default:
/// `(dotted key path, reason)`. A value equal to the old default moves to
/// the new one ([`move_unchanged_defaults`]).
const RECOMMENDED_DEFAULTS: &[(&str, &str)] = &[(
    "git.test_gate_on_push",
    "the push set now finishes in seconds and blocks a failed push (TSK-132)",
)];

/// One note per [`RECOMMENDED_DEFAULTS`] key whose shipped default changed
/// and whose current value differs from the new default.
fn recommend_defaults(
    user: &serde_json::Value,
    old_default: &serde_json::Value,
    new_default: &serde_json::Value,
    raw: Option<Shipped<'_>>,
) -> Vec<Shown> {
    let mut notes = Vec::new();
    for (path, reason) in RECOMMENDED_DEFAULTS {
        let pointer = format!("/{}", path.replace('.', "/"));
        let (Some(old), Some(new), Some(current)) = (
            old_default.pointer(&pointer),
            new_default.pointer(&pointer),
            user.pointer(&pointer),
        ) else {
            continue;
        };
        // The project's value is never printed (TSK-1029).
        if old != new && current != new {
            let new = shipped_value(new, raw);
            notes.push(shown!(
                "kept the project's {}; new installs default to {}: {}. \
                 To adopt it, set it to {} in .codeflow/policy.json",
                Shown::fixed(path),
                new,
                Shown::fixed(reason),
                new
            ));
        }
    }
    notes
}

fn unsupported_policy_notes(value: &serde_json::Value) -> impl Iterator<Item = Shown> {
    crate::hooks::policy_schema::unsupported_control_paths(value)
        .into_iter()
        .map(|path| {
            shown!(
                "policy key {} is not supported, has no effect",
                Shown::fixed(path)
            )
        })
}

fn key_change_note(key: &str, raw: Option<Shipped<'_>>) -> Shown {
    match key.strip_prefix('-') {
        Some(gone) => {
            let kind = if crate::hooks::policy_schema::spec_for(gone)
                .is_some_and(|spec| spec.is_unsupported())
            {
                "unsupported"
            } else {
                "deprecated"
            };
            shown!(
                "removed {} key {}",
                Shown::fixed(kind),
                policy_key(gone, raw)
            )
        }
        None => shown!("added key {}", policy_key(key, raw)),
    }
}

/// Remove an unsupported policy key only when its value matches the
/// recorded shipped baseline. Without that evidence, preserve the value.
fn remove_unsupported_defaults(
    user: &mut serde_json::Value,
    old: Option<&serde_json::Value>,
    new: &serde_json::Value,
) -> Vec<String> {
    use crate::hooks::policy_schema::{lookup, schema};
    let mut removed = Vec::new();
    for spec in schema()
        .iter()
        .filter(|spec| spec.path.contains('.') && spec.is_unsupported())
    {
        let previous = old.and_then(|value| lookup(value, spec.path));
        if previous.is_some()
            && lookup(user, spec.path) == previous
            && lookup(new, spec.path).is_none()
        {
            let (section, key) = spec.path.split_once('.').expect("section key");
            if let Some(object) = user
                .get_mut(section)
                .and_then(serde_json::Value::as_object_mut)
            {
                object.remove(key);
                removed.push(spec.path.to_string());
            }
        }
    }
    removed
}

/// Delete each dotted-path [`DEPRECATED_KEYS`] entry the user file carries and
/// the new shipped default does not. Returns the removed keys.
fn remove_deprecated_keys(
    user: &mut serde_json::Value,
    new_default: &serde_json::Value,
) -> Vec<String> {
    DEPRECATED_KEYS
        .iter()
        .map(|(key, _)| *key)
        .filter(|key| {
            let pointer = format!("/{}", key.replace('.', "/"));
            if new_default.pointer(&pointer).is_some() {
                return false;
            }
            let (parent, leaf) = pointer.rsplit_once('/').expect("pointer has a slash");
            user.pointer_mut(parent)
                .and_then(serde_json::Value::as_object_mut)
                .is_some_and(|obj| obj.remove(leaf).is_some())
        })
        .map(str::to_string)
        .collect()
}

/// Recursively adds keys present in `new_default` but absent from both
/// `user` and `old_default`.
fn add_new_keys(
    user: &mut serde_json::Value,
    old_default: Option<&serde_json::Value>,
    new_default: &serde_json::Value,
    path: &str,
    added: &mut Vec<String>,
) {
    let (Some(user_obj), Some(new_obj)) = (user.as_object_mut(), new_default.as_object()) else {
        return;
    };
    for (key, new_val) in new_obj {
        let key_path = if path.is_empty() {
            key.clone()
        } else {
            format!("{path}.{key}")
        };
        let old_val = old_default.and_then(|o| o.get(key));
        match user_obj.get_mut(key) {
            None => {
                if old_val.is_none() {
                    user_obj.insert(key.clone(), new_val.clone());
                    added.push(key_path);
                }
                // else: user deleted a key the old default had — respected.
            }
            Some(user_val) if user_val.is_object() && new_val.is_object() => {
                add_new_keys(user_val, old_val, new_val, &key_path, added);
            }
            Some(_) => {} // user value: never mutated
        }
    }
}

/// Reconcile files managed under the OLD installed manifest that are absent
/// from the NEW source manifest entirely — an artifact removed or renamed
/// upstream (e.g. the `.claude/commands/*` -> `.claude/skills/*/SKILL.md`
/// migration). Without this, an orphan lingers and can collide with its
/// renamed replacement (a stale `/cf-plan` command beside the new skill).
///
/// Never deletes user data:
/// - **managed** (whole-file, codeflow-owned): the file + baseline are removed
///   when the file is unmodified (or already gone); a user-MODIFIED orphan is
///   kept on disk and merely unmanaged (baseline + record dropped).
/// - **managed-region / user-owned**: the file may hold content outside a
///   codeflow block, so it is never deleted — only unmanaged.
///
/// The "still shipped" set spans ALL tiers of the new manifest, so a lower-tier
/// project never prunes a file the manifest still ships at a higher tier (a
/// tier downgrade stops managing, never deletes).
fn prune_orphans(
    root: &Path,
    manifest: &ScaffoldManifest,
    ignore: &ScaffoldConfig,
    installed: &mut InstalledManifest,
    report: &mut Report,
) -> Result<(), ScaffoldError> {
    let shipped: std::collections::BTreeSet<&str> =
        manifest.entries.iter().map(|e| e.dest.as_str()).collect();
    let orphans: Vec<String> = installed
        .files
        .keys()
        // A path matched by `[scaffold] ignore` is the user's to manage: never
        // prune it or report it as orphaned, even once upstream stops shipping
        // it — its record simply lingers, inert, until the glob is removed.
        .filter(|dest| !shipped.contains(dest.as_str()) && !ignore.is_ignored(dest))
        .cloned()
        .collect();

    for dest in orphans {
        // A tampered installed manifest could carry an absolute or `..`-escaping
        // `dest`; `root.join` on it would resolve OUTSIDE the repo, and pruning
        // would then delete an arbitrary hash-matching file. Never touch an
        // unsafe path — report it so the tamper is visible (codex pre-flip
        // review: arbitrary out-of-repo deletion via a tampered manifest).
        if !is_safe_relative_dest(&dest) {
            report.file(&dest, Action::Skipped);
            continue;
        }
        // Also refuse a dest that traverses a symlink on disk: a pre-planted
        // leaf or ancestor link would turn an orphan prune into an out-of-repo
        // delete. Report the tamper (visible, never silent), never follow it.
        if let Err(e) = guard_beneath_root(root, Path::new(&dest)) {
            report.file_with_notes(&dest, Action::Skipped, vec![Shown::from(&e)]);
            continue;
        }
        let file = installed.files[&dest].clone();
        let dest_path = root.join(&dest);

        if file.ownership == Ownership::Managed {
            let unmodified = match std::fs::read_to_string(&dest_path) {
                Ok(current) => hash::sha256_hex(current.as_bytes()) == file.sha256,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
                Err(e) => return Err(ScaffoldError::io(&dest_path, e)),
            };
            if unmodified {
                if dest_path.exists() {
                    remove_beneath_root(root, &dest)?;
                    remove_empty_ancestors(root, &dest_path);
                }
                let baseline_path = Baseline::path(root, &dest);
                Baseline::remove(root, &dest)?;
                remove_empty_ancestors(root, &baseline_path);
                installed.files.remove(&dest);
                report.file_with_notes(
                    &dest,
                    Action::Removed,
                    vec![Shown::fixed(
                        "no longer shipped; unmodified managed file removed",
                    )],
                );
                continue;
            }
            // User-modified: keep the file, stop managing it.
            Baseline::remove(root, &dest)?;
            installed.files.remove(&dest);
            report.file_with_notes(
                &dest,
                Action::KeptUserModified,
                vec![Shown::fixed(
                    "no longer shipped; your modified copy kept and no longer managed",
                )],
            );
            continue;
        }

        // managed-region / user-owned: never delete a file that may hold user
        // content outside a codeflow block — just stop managing it.
        Baseline::remove(root, &dest)?;
        installed.files.remove(&dest);
        report.file_with_notes(
            &dest,
            Action::Skipped,
            vec![Shown::fixed(
                "no longer shipped; file kept (not codeflow-owned to remove)",
            )],
        );
    }
    Ok(())
}

/// Removes now-empty ancestor directories of `file`, up to (not including)
/// `root`, stopping at the first non-empty directory. `remove_dir` only
/// succeeds on an empty directory, so a non-empty ancestor ends the walk.
fn remove_empty_ancestors(root: &Path, file: &Path) {
    let mut cur = file.parent().map(Path::to_path_buf);
    while let Some(dir) = cur {
        if dir == root || !dir.starts_with(root) || std::fs::remove_dir(&dir).is_err() {
            break;
        }
        cur = dir.parent().map(Path::to_path_buf);
    }
}

#[cfg(test)]
mod tests {
    /// TSK-323: an updated project, which `init` never marked a new
    /// scaffold, is told its size findings are report lines and how to opt
    /// in; a new scaffold or a project that opted in is not.
    #[test]
    fn the_size_rule_note_names_the_opt_in_only_where_nothing_refuses() {
        use super::super::manifest::Tier;
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".codeflow")).unwrap();
        let mut state = super::ProjectState {
            schema_version: 1,
            tier: Tier::Minimal,
            scaffold_version: "3.1.1".to_string(),
            stack: "generic".to_string(),
            areas: vec![],
            policy_armed: true,
            git_hooks: "wired".to_string(),
            permission_preset: "default".to_string(),
            product_one_liner: String::new(),
            release_rules: Some(1),
            file_size_rule: None,
        };
        let note = super::size_rule_note(dir.path(), &state).expect("updated project");
        assert!(note.to_string().contains("report lines"), "{note}");
        assert!(note.to_string().contains("git.max_file_bytes"), "{note}");
        state.file_size_rule = Some("refuse".into());
        assert!(super::size_rule_note(dir.path(), &state).is_none());
        state.file_size_rule = None;
        std::fs::write(
            dir.path().join(".codeflow").join("policy.json"),
            r#"{"git": {"max_file_bytes": {"limit": 5000000}}}"#,
        )
        .unwrap();
        assert!(super::size_rule_note(dir.path(), &state).is_none());
    }

    #[test]
    fn r17_migration_without_repository_has_no_baseline() {
        let dir = tempfile::tempdir().unwrap();
        let tasks = dir.path().join("project-management/tasks");
        std::fs::create_dir_all(&tasks).unwrap();
        std::fs::write(
            tasks.join("TSK-001.md"),
            "---\nid: TSK-001\ntitle: Fixture\nstatus: planned\n---\n",
        )
        .unwrap();
        assert!(super::record_work_records_baseline(dir.path())
            .unwrap()
            .is_none());
        git2::Repository::init(dir.path()).unwrap();
        assert!(super::record_work_records_baseline(dir.path())
            .unwrap()
            .is_none());
        std::fs::remove_file(dir.path().join(".git/HEAD")).unwrap();
        assert!(super::record_work_records_baseline(dir.path()).is_err());
    }

    /// The shipped workflow and this repository's own run only the checkout
    /// before the gitleaks step; a step added there is counted by position, a renamed scan
    /// step is reported as unrecognised, and a conflict proposal that does
    /// not parse is left alone (TSK-210).
    #[test]
    fn scan_order_names_only_added_steps() {
        use super::{scan_order, ScanOrder};
        let none = ScanOrder::Checked(vec![]);
        let named = |steps: &[usize]| ScanOrder::Checked(steps.to_vec());
        let shipped = include_str!("../../../../assets/base/ci/codeflow-ci.yml");
        let own = include_str!("../../../../.github/workflows/codeflow-ci.yml");
        assert_eq!(scan_order(shipped), none);
        assert_eq!(scan_order(own), none);

        let checkout = "    name: secret scan\n    runs-on: ubuntu-24.04\n    timeout-minutes: 30\n    steps:\n      \
                        - uses: actions/checkout@d23441a48e516b6c34aea4fa41551a30e30af803 # v6.1.0\n        with:\n          fetch-depth: 0\n";
        assert!(shipped.contains(checkout));
        let added = shipped.replacen(
            checkout,
            &format!(
                "{checkout}      - name: setup\n        run: ./scripts/setup.sh\n      - uses: ./.github/actions/prepare\n"
            ),
            1,
        );
        assert_eq!(scan_order(&added), named(&[2, 3]));
        let renamed = added.replacen(
            "      - name: gitleaks\n",
            "      - name: Repository credential scan\n",
            1,
        );
        assert_ne!(renamed, added);
        assert_eq!(scan_order(&renamed), ScanOrder::Unrecognized);
        // A step that mentions gitleaks before the scan is listed, never
        // taken for the scan.
        let canary = shipped.replacen(
            checkout,
            &format!(
                "{checkout}      - run: sh scripts/check-gitleaks-allowlist.sh \"$RUNNER_TEMP/gitleaks\"\n      \
                 - name: setup\n        run: ./scripts/setup.sh\n"
            ),
            1,
        );
        assert_eq!(scan_order(&canary), named(&[2, 3]));
        let conflicted = format!("<<<<<<< ours\n{added}=======\n{shipped}>>>>>>> theirs\n");
        assert_eq!(scan_order(&conflicted), ScanOrder::NotChecked);
        assert_eq!(
            scan_order("jobs:\n  gates:\n    runs-on: x\n"),
            ScanOrder::NotChecked
        );
    }

    /// `CodeFlow`'s release jobs live in its own `codeflow-release.yml`, not
    /// in its copy of the managed CI file (TSK-106, SPC-013 R-96), so an
    /// update proposal for that file carries none of them (TSK-107 kept them
    /// through the merge while they lived there).
    #[test]
    fn dogfood_ci_proposal_carries_no_release_job() {
        let base =
            include_str!("../../../../.codeflow/.baseline/.github/workflows/codeflow-ci.yml");
        let ours = include_str!("../../../../.github/workflows/codeflow-ci.yml");
        let theirs = include_str!("../../../../assets/base/ci/codeflow-ci.yml");
        let own = include_str!("../../../../.github/workflows/codeflow-release.yml");
        let proposal = match diffy::merge(base, ours, theirs) {
            Ok(merged) | Err(merged) => merged,
        };
        for text in [ours, &proposal] {
            assert!(!text.contains("\n  release-impact:\n"), "{text}");
            assert!(!text.contains("scripts/release.py"), "{text}");
        }
        assert!(own.contains("\n  release-impact:\n"));
        assert!(own.contains("scripts/release.py check-pr"));
    }

    #[test]
    fn test_unsafe_dest_rejected() {
        // codex pre-flip review: a tampered manifest must not escape the repo.
        for bad in ["/etc/passwd", "../../etc/x", "..", "a/../../b", "/abs"] {
            assert!(!is_safe_relative_dest(bad), "must reject {bad}");
        }
        for ok in ["AGENTS.md", ".codeflow/policy.json", "a/b/c.md", "./x"] {
            assert!(is_safe_relative_dest(ok), "must allow {ok}");
        }
    }

    use super::*;

    #[test]
    fn new_keys_added_deletions_respected_values_kept() {
        let mut user: serde_json::Value =
            serde_json::from_str(r#"{"schema_version":1,"git":{"commit_format":"warn"}}"#).unwrap();
        let old: serde_json::Value = serde_json::from_str(
            r#"{"schema_version":1,"git":{"commit_format":"block","secret_scan":"block"}}"#,
        )
        .unwrap();
        let new: serde_json::Value = serde_json::from_str(
            r#"{"schema_version":2,"git":{"commit_format":"block","secret_scan":"block","push_signed":"warn"},"recall":{"share":false}}"#,
        )
        .unwrap();
        let mut added = vec![];
        add_new_keys(&mut user, Some(&old), &new, "", &mut added);

        assert_eq!(
            added,
            vec!["git.push_signed".to_string(), "recall".to_string()]
        );
        assert_eq!(user["git"]["commit_format"], "warn", "user value kept");
        assert!(
            user["git"].get("secret_scan").is_none(),
            "user deletion respected"
        );
        assert_eq!(user["git"]["push_signed"], "warn");
        assert_eq!(user["recall"]["share"], false);
    }
}
