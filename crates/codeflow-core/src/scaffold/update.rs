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
//! The update always ends with a printed report
//! (changed / merged / conflicted / skipped / added).

use std::path::{Path, PathBuf};

use super::init::{build_context, render_entry};
use super::manifest::{ManifestEntry, Ownership, RegionFormat, ScaffoldManifest};
use super::region::{self, BlockOutcome};
use super::report::{Action, Report};
use super::settings_merge::merge_settings;
use super::state::{set_exec, write_file, Baseline, InstalledFile, InstalledManifest, ProjectState};
use super::{hash, ScaffoldError};

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
    let manifest = ScaffoldManifest::load(source)?;
    let mut installed = InstalledManifest::load_or_default(root, &state.scaffold_version)?;

    let project = root
        .canonicalize()
        .ok()
        .as_deref()
        .unwrap_or(root)
        .file_name()
        .map_or_else(|| "project".to_string(), |n| n.to_string_lossy().to_string());
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
        update_entry(
            source, root, entry, &ctx, &state, opts, &mut installed, &mut report, &mut diffs,
        )?;
    }

    installed.scaffold_version.clone_from(&opts.binary_version);
    installed.store(root)?;
    state.scaffold_version.clone_from(&opts.binary_version);
    state.store(root)?;

    if let Some(path) = &opts.diff_out {
        let mut out = report.to_string();
        if !diffs.is_empty() {
            out.push('\n');
            out.push_str(&diffs);
        }
        write_file(path, out.as_bytes())?;
        report.notes.push(format!("diff written to {}", path.display()));
    }

    Ok(report)
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

fn write_dest(root: &Path, entry: &ManifestEntry, content: &str) -> Result<(), ScaffoldError> {
    let path = root.join(&entry.dest);
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
    state: &ProjectState,
    opts: &UpdateOptions,
    installed: &mut InstalledManifest,
    report: &mut Report,
    diffs: &mut String,
) -> Result<(), ScaffoldError> {
    let Some(rendered) = render_entry(source, entry, ctx, state.tier, report)? else {
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
                write_file(&root.join(&new_path), rendered.as_bytes())?;
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
                record(installed, entry, hash::sha256_hex(merged.as_bytes()));
                Baseline::write(root, &entry.dest, &rendered)?;
                push_diff(diffs, &entry.dest, &current, &merged);
                report.file_with_notes(
                    &entry.dest,
                    Action::Merged,
                    vec!["3-way merge applied cleanly (base = shipped baseline)".to_string()],
                );
            } else {
                let new_path = format!("{}.new", entry.dest);
                write_file(&root.join(&new_path), rendered.as_bytes())?;
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
                let mut lines = vec![];
                let merged = merge_settings(&current, &rendered, &mut lines)?;
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
                            vec!["managed block regenerated; content outside markers untouched"
                                .to_string()],
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
                report.file_with_notes(
                    &entry.dest,
                    Action::Skipped,
                    vec!["user-owned: never mutated by update".to_string()],
                );
                Baseline::write(root, &entry.dest, &rendered)?;
                record(installed, entry, hash::sha256_hex(rendered.as_bytes()));
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
    let old_default: Option<serde_json::Value> = Baseline::read(root, &entry.dest)
        .and_then(|t| serde_json::from_str(&t).ok());

    let mut added: Vec<String> = vec![];
    if old_default.is_some() {
        add_new_keys(&mut user, old_default.as_ref(), &new_default, "", &mut added);
    }

    // Refresh the shipped-default baseline and record either way.
    Baseline::write(root, &entry.dest, rendered)?;
    record(installed, entry, hash::sha256_hex(rendered.as_bytes()));

    if added.is_empty() {
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
    if let (Some(user_sv), Some(new_sv)) = (
        user.get("schema_version").and_then(serde_json::Value::as_u64),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_keys_added_deletions_respected_values_kept() {
        let mut user: serde_json::Value = serde_json::from_str(
            r#"{"schema_version":1,"git":{"commit_format":"warn"}}"#,
        )
        .unwrap();
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

        assert_eq!(added, vec!["git.push_signed".to_string(), "recall".to_string()]);
        assert_eq!(user["git"]["commit_format"], "warn", "user value kept");
        assert!(
            user["git"].get("secret_scan").is_none(),
            "user deletion respected"
        );
        assert_eq!(user["git"]["push_signed"], "warn");
        assert_eq!(user["recall"]["share"], false);
    }
}
