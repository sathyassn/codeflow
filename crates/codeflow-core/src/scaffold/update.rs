//! `codeflow update` — manifest + 3-way merge refresh of managed files
//! (charter §3.1, §4.3, §10, AC #8).
//!
//! Per installed manifest entry:
//! - **managed, unmodified** (current hash == recorded hash): replaced with
//!   the new shipped version; record and baseline refreshed.
//! - **managed, user-modified**: 3-way merge with base = `.codeflow/.baseline/`
//!   copy, ours = the user's file, theirs = the new shipped version. Clean
//!   merge is applied and reported; a conflict writes `<path>.new` and the
//!   report — the user's file is NEVER clobbered and NEVER silently skipped.
//! - **managed-region**: only the marked block (markdown/hash) or the
//!   codeflow-owned keys (settings JSON) are regenerated.
//! - **user-owned**: never mutated; schema-versioned JSON (policy.json) gains
//!   NEW default keys (absent from both the user file and the old shipped
//!   default), added with defaults and reported.
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

use super::init::{build_context, render_entry};
use super::manifest::{ManifestEntry, Ownership, RegionFormat, ScaffoldManifest};
use super::region::{self, BlockOutcome};
use super::report::{Action, Report};
use super::settings_merge::merge_settings_from_baseline;
use super::state::{
    guard_beneath_root, remove_beneath_root, set_exec, write_beneath_root, write_file, Baseline,
    InstalledFile, InstalledManifest, ProjectState, ScaffoldConfig,
};
use super::{hash, should_skip_initial_stack_adr, ScaffoldError};

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
/// `.codeflow/project.toml`; otherwise IO, JSON, or manifest failures.
/// Per-file merge conflicts are NOT errors — they are reported.
pub fn update(
    source: &dyn super::AssetSource,
    root: &Path,
    opts: &UpdateOptions,
) -> Result<Report, ScaffoldError> {
    let mut state = ProjectState::load(root)?;
    let ignore = ScaffoldConfig::load(root)?;
    let manifest = ScaffoldManifest::load(source)?;
    let mut installed = InstalledManifest::load_or_default(root, &state.scaffold_version)?;

    let project = root
        .canonicalize()
        .ok()
        .as_deref()
        .unwrap_or(root)
        .file_name()
        .map_or_else(
            || "project".to_string(),
            |n| n.to_string_lossy().to_string(),
        );
    let ctx = build_context(
        &project,
        &state.product_one_liner,
        &state.areas,
        &state.stack,
        state.tier,
        &opts.binary_version,
    );

    let mut report = Report::new(format!(
        "codeflow update (scaffold {} -> {}, {} tier)",
        state.scaffold_version, opts.binary_version, state.tier
    ));
    let mut diffs = String::new();

    for entry in &manifest.entries {
        if !entry.applies(state.tier, &state.permission_preset) {
            continue;
        }
        if ignore.is_ignored(&entry.dest) {
            // Opted out via `[scaffold] ignore`: never rewrite it, and never
            // resurrect it if the user deleted it. Its manifest record and
            // baseline are left untouched so removing the glob later restores
            // normal management.
            report.file_with_notes(
                &entry.dest,
                Action::Skipped,
                vec!["ignored via [scaffold] ignore in project.toml".to_string()],
            );
            continue;
        }
        if should_skip_initial_stack_adr(root, &entry.dest)? {
            report.file_with_notes(
                &entry.dest,
                Action::Skipped,
                vec![
                    "brownfield repository already has ADRs; starter stack decision not added"
                        .to_string(),
                ],
            );
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

    installed.scaffold_version.clone_from(&opts.binary_version);
    installed.store(root)?;
    state.scaffold_version.clone_from(&opts.binary_version);
    state.store(root)?;
    if let Some(note) = record_work_records_baseline(root)? {
        report.notes.push(note);
    }

    if let Some(path) = &opts.diff_out {
        let mut out = report.to_string();
        if !diffs.is_empty() {
            out.push('\n');
            out.push_str(&diffs);
        }
        write_file(path, out.as_bytes())?;
        report
            .notes
            .push(format!("diff written to {}", path.display()));
    }

    Ok(report)
}

/// The work-record migration (SPC-013 R-83): an existing project whose
/// records predate the lifecycle rules gets its migration baseline recorded
/// once, as the current `HEAD`, so older records keep their exact blobs
/// exempt and the rules apply by transition from here. A project with no
/// record, no commit, or a recorded baseline is left alone.
fn record_work_records_baseline(root: &Path) -> Result<Option<String>, ScaffoldError> {
    use crate::workgraph::lifecycle::{recorded_baseline, Graph, BASELINE_KEY};
    if recorded_baseline(root).is_some() || Graph::from_worktree(root).records.is_empty() {
        return Ok(None);
    }
    let Some(head) = git2::Repository::discover(root).ok().and_then(|repo| {
        let id = repo.head().ok()?.peel_to_commit().ok()?.id();
        Some(id.to_string())
    }) else {
        return Ok(None);
    };
    let path = ProjectState::path(root);
    let text = std::fs::read_to_string(&path).map_err(|e| ScaffoldError::io(&path, e))?;
    let mut table: toml::Table =
        toml::from_str(&text).map_err(|e| ScaffoldError::InvalidState {
            what: path.display().to_string(),
            detail: e.to_string(),
        })?;
    table.insert(BASELINE_KEY.to_string(), toml::Value::String(head.clone()));
    let next = toml::to_string_pretty(&table).map_err(|e| ScaffoldError::InvalidState {
        what: path.display().to_string(),
        detail: e.to_string(),
    })?;
    write_file(&path, next.as_bytes())?;
    Ok(Some(format!(
        "recorded {BASELINE_KEY} = {head}: work-record rules apply to records changed after this commit"
    )))
}

/// Policy values a released binary no longer accepts, rewritten to their
/// nearest accepted value with a note. Only `git.work_records = "off"` from
/// an unreleased build qualifies (SPC-013 R-81); every other value is kept.
fn migrate_policy_values(dest: &str, user: &mut serde_json::Value) -> Vec<String> {
    if dest != ".codeflow/policy.json" {
        return Vec::new();
    }
    match user.pointer_mut("/git/work_records") {
        Some(value) if value.as_str() == Some("off") => {
            *value = serde_json::Value::from("warn");
            vec!["git.work_records `off` no longer exists; rewritten to `warn`".to_string()]
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
        return Err(ScaffoldError::ManifestInvalid(format!(
            "refusing to write outside the repo: unsafe dest {:?}",
            entry.dest
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

fn push_diff(diffs: &mut String, dest: &str, old: &str, new: &str) {
    use std::fmt::Write;
    let patch = diffy::create_patch(old, new);
    let _ = write!(diffs, "=== {dest}\n{patch}\n");
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
    diffs: &mut String,
) -> Result<(), ScaffoldError> {
    // Refuse to touch an entry whose on-disk dest traverses a symlink (leaf or
    // ancestor): a pre-planted link would otherwise let the write below escape
    // the repo. Reported, never silent (module invariant #2).
    if let Err(e) = guard_beneath_root(root, Path::new(&entry.dest)) {
        report.file_with_notes(&entry.dest, Action::Skipped, vec![e.to_string()]);
        return Ok(());
    }
    let Some(rendered) = render_entry(source, entry, ctx, report)? else {
        return Ok(());
    };
    let dest_path = root.join(&entry.dest);
    let version = ctx.get("SCAFFOLD_VERSION").unwrap_or("0");

    match entry.ownership {
        Ownership::Managed => {
            if !dest_path.exists() {
                // New manifest entry (or deleted file): install it.
                write_dest(root, entry, &rendered)?;
                record(installed, entry, hash::sha256_hex(rendered.as_bytes()));
                Baseline::write(root, &entry.dest, &rendered)?;
                report.file(&entry.dest, Action::Added);
                return Ok(());
            }
            let current = std::fs::read_to_string(&dest_path)
                .map_err(|e| ScaffoldError::io(&dest_path, e))?;
            if current == rendered {
                // Already at the new version (however it got there): adopt.
                record(installed, entry, hash::sha256_hex(current.as_bytes()));
                Baseline::write(root, &entry.dest, &rendered)?;
                report.file(&entry.dest, Action::Unchanged);
                return Ok(());
            }
            let recorded = installed.files.get(&entry.dest).map(|f| f.sha256.clone());
            let unmodified =
                recorded.as_deref() == Some(hash::sha256_hex(current.as_bytes()).as_str());

            if unmodified {
                write_dest(root, entry, &rendered)?;
                record(installed, entry, hash::sha256_hex(rendered.as_bytes()));
                Baseline::write(root, &entry.dest, &rendered)?;
                push_diff(diffs, &entry.dest, &current, &rendered);
                report.file(&entry.dest, Action::Changed);
                return Ok(());
            }
            if opts.force {
                write_dest(root, entry, &rendered)?;
                record(installed, entry, hash::sha256_hex(rendered.as_bytes()));
                Baseline::write(root, &entry.dest, &rendered)?;
                push_diff(diffs, &entry.dest, &current, &rendered);
                report.file_with_notes(
                    &entry.dest,
                    Action::Forced,
                    vec!["user modifications overwritten (--force)".to_string()],
                );
                return Ok(());
            }
            // User-modified: 3-way merge against the baseline.
            let Some(base) = Baseline::read(root, &entry.dest) else {
                let new_path = format!("{}.new", entry.dest);
                write_beneath_root(root, &new_path, rendered.as_bytes())?;
                report.file_with_notes(
                    &entry.dest,
                    Action::Conflicted,
                    vec![format!(
                        "no baseline available for 3-way merge; new version written to {new_path}"
                    )],
                );
                return Ok(());
            };
            if rendered == base {
                report.file_with_notes(
                    &entry.dest,
                    Action::KeptUserModified,
                    vec!["no upstream change; your modifications stand".to_string()],
                );
                return Ok(());
            }
            if let Ok(merged) = diffy::merge(&base, &current, &rendered) {
                write_dest(root, entry, &merged)?;
                // Record the pristine shipped hash (not the merged file's), so the
                // manifest invariant `recorded == hash(baseline)` holds: the merged
                // file carries user edits, so the next update must classify it
                // "user-modified" and re-merge, never treat it as pristine and
                // overwrite. Recording hash(merged) here silently wiped the merge
                // on the following update.
                record(installed, entry, hash::sha256_hex(rendered.as_bytes()));
                Baseline::write(root, &entry.dest, &rendered)?;
                push_diff(diffs, &entry.dest, &current, &merged);
                report.file_with_notes(
                    &entry.dest,
                    Action::Merged,
                    vec!["3-way merge applied cleanly (base = shipped baseline)".to_string()],
                );
            } else {
                let new_path = format!("{}.new", entry.dest);
                write_beneath_root(root, &new_path, rendered.as_bytes())?;
                report.file_with_notes(
                    &entry.dest,
                    Action::Conflicted,
                    vec![format!(
                        "your modifications conflict with the new shipped version; \
                         file untouched, new version written to {new_path}"
                    )],
                );
            }
        }
        Ownership::ManagedRegion => match entry.region.unwrap_or(RegionFormat::Markdown) {
            RegionFormat::Json => {
                if !dest_path.exists() {
                    write_dest(root, entry, &rendered)?;
                    record(installed, entry, hash::sha256_hex(rendered.as_bytes()));
                    Baseline::write(root, &entry.dest, &rendered)?;
                    report.file(&entry.dest, Action::Added);
                    return Ok(());
                }
                let current = std::fs::read_to_string(&dest_path)
                    .map_err(|e| ScaffoldError::io(&dest_path, e))?;
                let previous = Baseline::read(root, &entry.dest);
                let mut lines = vec![];
                let merged = merge_settings_from_baseline(
                    &current,
                    previous.as_deref(),
                    &rendered,
                    &mut lines,
                )?;
                record(installed, entry, hash::sha256_hex(rendered.as_bytes()));
                Baseline::write(root, &entry.dest, &rendered)?;
                if merged == current {
                    report.file(&entry.dest, Action::Unchanged);
                } else {
                    write_dest(root, entry, &merged)?;
                    push_diff(diffs, &entry.dest, &current, &merged);
                    report.file_with_notes(&entry.dest, Action::Merged, lines);
                }
            }
            format @ (RegionFormat::Markdown | RegionFormat::Hash) => {
                let block = region::extract_block(&rendered, format)
                    .unwrap_or_else(|| region::wrap_block(&rendered, format, version));
                record(installed, entry, hash::sha256_hex(block.as_bytes()));
                Baseline::write(root, &entry.dest, &block)?;
                if !dest_path.exists() {
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
                        push_diff(diffs, &entry.dest, &current, &next);
                        report.file_with_notes(
                            &entry.dest,
                            Action::Changed,
                            vec![
                                "managed block regenerated; content outside markers untouched"
                                    .to_string(),
                            ],
                        );
                    }
                    BlockOutcome::Appended => {
                        write_dest(root, entry, &next)?;
                        push_diff(diffs, &entry.dest, &current, &next);
                        report.file_with_notes(
                            &entry.dest,
                            Action::Merged,
                            vec!["markers were missing; codeflow block re-appended".to_string()],
                        );
                    }
                }
            }
        },
        Ownership::UserOwned => {
            if !dest_path.exists() {
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
                sync_user_owned_json(root, entry, &rendered, installed, report, diffs)?;
            } else {
                let mut notes = vec!["user-owned: never mutated by update".to_string()];
                let is_new_entry = !installed.files.contains_key(&entry.dest);
                let baseline_missing = Baseline::read(root, &entry.dest).is_none();
                if is_new_entry || baseline_missing {
                    Baseline::write(root, &entry.dest, &rendered)?;
                    record(installed, entry, hash::sha256_hex(rendered.as_bytes()));
                    notes.push(if is_new_entry {
                        "existing file adopted into the installed manifest".to_string()
                    } else {
                        "missing shipped baseline restored without changing the live file"
                            .to_string()
                    });
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
    installed: &mut InstalledManifest,
    report: &mut Report,
    diffs: &mut String,
) -> Result<(), ScaffoldError> {
    let dest_path = root.join(&entry.dest);
    let current_text =
        std::fs::read_to_string(&dest_path).map_err(|e| ScaffoldError::io(&dest_path, e))?;
    let mut user: serde_json::Value = serde_json::from_str(&current_text)?;
    let new_default: serde_json::Value = serde_json::from_str(rendered)?;
    let old_default: Option<serde_json::Value> =
        Baseline::read(root, &entry.dest).and_then(|t| serde_json::from_str(&t).ok());

    let mut added: Vec<String> = vec![];
    let mut moved: Vec<String> = vec![];
    if let Some(old) = old_default.as_ref() {
        add_new_keys(&mut user, Some(old), &new_default, "", &mut added);
        moved = migrate_defaults(&mut user, old, &new_default);
    }

    let migrated = migrate_policy_values(&entry.dest, &mut user);

    // Refresh the shipped-default baseline and record either way.
    Baseline::write(root, &entry.dest, rendered)?;
    record(installed, entry, hash::sha256_hex(rendered.as_bytes()));

    if added.is_empty() && migrated.is_empty() && moved.is_empty() {
        let mut notes = vec!["user-owned: values never mutated; no new default keys".to_string()];
        if old_default.is_none() {
            notes.push(
                "no shipped-default baseline existed; key sync starts from this version"
                    .to_string(),
            );
        }
        report.file_with_notes(&entry.dest, Action::Skipped, notes);
        return Ok(());
    }

    // schema_version awareness: carry the new default's schema_version when
    // keys were added and the user has not customized it past the default.
    let mut notes: Vec<String> = added.iter().map(|k| format!("added key {k}")).collect();
    notes.extend(migrated);
    notes.extend(moved);
    if let (Some(user_sv), Some(new_sv)) = (
        user.get("schema_version")
            .and_then(serde_json::Value::as_u64),
        new_default
            .get("schema_version")
            .and_then(serde_json::Value::as_u64),
    ) {
        if new_sv > user_sv {
            user["schema_version"] = serde_json::Value::from(new_sv);
            notes.push(format!("schema_version advanced {user_sv} -> {new_sv}"));
        }
    }

    let mut next = serde_json::to_string_pretty(&user)?;
    next.push('\n');
    write_file(&dest_path, next.as_bytes())?;
    push_diff(diffs, &entry.dest, &current_text, &next);
    report.file_with_notes(&entry.dest, Action::KeysAdded, notes);
    Ok(())
}

/// Default changes that `codeflow update` carries to an adopter who never
/// chose a value: `(dotted key path, reason)`. Only these keys move, and only
/// while the adopter's value still equals the old shipped default; a value
/// the adopter set explicitly is kept.
const MIGRATED_DEFAULTS: &[(&str, &str)] = &[(
    "git.test_gate_on_push",
    "the push set is now fast (TSK-132); set \"warn\" to restore the old behaviour",
)];

/// Move each [`MIGRATED_DEFAULTS`] key from the old shipped default to the
/// new one when the adopter's value equals the old default. Returns one note
/// per moved key.
fn migrate_defaults(
    user: &mut serde_json::Value,
    old_default: &serde_json::Value,
    new_default: &serde_json::Value,
) -> Vec<String> {
    let mut notes = Vec::new();
    for (path, reason) in MIGRATED_DEFAULTS {
        let pointer = format!("/{}", path.replace('.', "/"));
        let (Some(old), Some(new)) = (old_default.pointer(&pointer), new_default.pointer(&pointer))
        else {
            continue;
        };
        if old == new {
            continue;
        }
        let Some(current) = user.pointer_mut(&pointer) else {
            continue;
        };
        if *current == *old {
            *current = new.clone();
            notes.push(format!("moved default {path} {old} -> {new}: {reason}"));
        }
    }
    notes
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
            report.file_with_notes(&dest, Action::Skipped, vec![e.to_string()]);
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
                    vec!["no longer shipped; unmodified managed file removed".to_string()],
                );
                continue;
            }
            // User-modified: keep the file, stop managing it.
            Baseline::remove(root, &dest)?;
            installed.files.remove(&dest);
            report.file_with_notes(
                &dest,
                Action::KeptUserModified,
                vec![
                    "no longer shipped; your modified copy kept and no longer managed".to_string(),
                ],
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
            vec!["no longer shipped; file kept (not codeflow-owned to remove)".to_string()],
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
