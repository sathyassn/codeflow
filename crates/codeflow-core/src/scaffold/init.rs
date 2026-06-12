//! `codeflow init` — idempotent, non-destructive, offline scaffolding
//! (charter §3.1, §4, AC #1/#2).
//!
//! Bootstrap grace: in a directory without a repo, init runs `git init`,
//! scaffolds, writes `.codeflow/project.toml` with `policy_armed = false`,
//! makes the scaffold commit itself (hooks read the flag and stand down),
//! then arms the policy. A brand-new project's first hour hits zero policy
//! walls without any enforcement exemption mechanism.
//!
//! Idempotence: re-running init never overwrites an existing file without
//! `--force`; re-running at a higher tier installs only the missing manifest
//! entries (additive upgrade); ownership classes drive create-or-update
//! semantics per file.

use std::path::Path;

use super::assets::read_text;
use super::detect::{self, CODEFLOW_HOOKS_PATH};
use super::manifest::{ManifestEntry, Ownership, RegionFormat, ScaffoldManifest, Tier};
use super::region::{self, BlockOutcome};
use super::report::{Action, Report};
use super::settings_merge::merge_settings;
use super::state::{
    set_exec, write_file, Baseline, InstalledFile, InstalledManifest, ProjectState,
    GIT_HOOKS_UNWIRED, GIT_HOOKS_WIRED, PROJECT_TOML,
};
use super::template::TemplateContext;
use super::{gitutil, hash, ScaffoldError};

/// Answers to init's at-most-three questions (charter §4.1: product
/// one-liner, areas, permission preset). `None` = use the default / the
/// previously recorded value. The CLI gathers these (stdin or `--yes`);
/// the engine never prompts.
#[derive(Debug, Clone, Default)]
pub struct InitAnswers {
    pub product_one_liner: Option<String>,
    pub areas: Option<Vec<String>>,
    pub permission_preset: Option<String>,
}

/// Options for [`init`].
#[derive(Debug, Clone)]
pub struct InitOptions {
    /// Requested tier; `None` = previously recorded tier, or standard.
    pub tier: Option<Tier>,
    /// Overwrite existing files (never the default).
    pub force: bool,
    /// The running binary's version — recorded as `scaffold_version`.
    pub binary_version: String,
    pub answers: InitAnswers,
}

/// Runs init against `root`. Returns the full report; the caller prints it.
///
/// # Errors
///
/// Fails on unreadable/unwritable project state, an invalid scaffold
/// manifest, or git plumbing failures. Missing individual assets are NOT
/// errors — they are skipped and warned in the report.
#[allow(clippy::too_many_lines)] // linear phase orchestration; splitting hurts legibility
pub fn init(
    source: &dyn super::AssetSource,
    root: &Path,
    opts: &InitOptions,
) -> Result<Report, ScaffoldError> {
    std::fs::create_dir_all(root).map_err(|e| ScaffoldError::io(root, e))?;
    let manifest = ScaffoldManifest::load(source)?;

    let was_empty = detect::is_empty_dir(root);
    let had_repo = gitutil::is_repo(root);
    if !had_repo {
        gitutil::init_repo(root)?;
    }
    let fresh_repo = !had_repo || !gitutil::has_commits(root);

    // Resolve tier / answers against any previous install.
    let previous = if ProjectState::exists(root) {
        Some(ProjectState::load(root)?)
    } else {
        None
    };
    let recorded_tier = previous.as_ref().map(|s| s.tier);
    let tier = effective_tier(recorded_tier, opts.tier);

    let preset = opts
        .answers
        .permission_preset
        .clone()
        .or_else(|| previous.as_ref().map(|s| s.permission_preset.clone()))
        .unwrap_or_else(|| "default".to_string());
    let areas = opts
        .answers
        .areas
        .clone()
        .or_else(|| previous.as_ref().map(|s| s.areas.clone()))
        .unwrap_or_else(|| vec!["core".to_string()]);
    let project_name = project_name(root);
    let one_liner = opts
        .answers
        .product_one_liner
        .clone()
        .or_else(|| previous.as_ref().map(|s| s.product_one_liner.clone()))
        .unwrap_or_else(|| project_name.clone());
    let detected = detect::detect_stack(root);
    let stack = if detected == "unset" {
        previous
            .as_ref()
            .map_or_else(|| detected.to_string(), |s| s.stack.clone())
    } else {
        detected.to_string()
    };

    let mut report = Report::new(format!("codeflow init ({tier} tier)"));
    if was_empty {
        report
            .notes
            .push("empty directory: bootstrapped a fresh repository".to_string());
    } else if !had_repo {
        report
            .notes
            .push("no git repository found: ran `git init`".to_string());
    }
    if let (Some(recorded), Some(requested)) = (recorded_tier, opts.tier) {
        if requested < recorded {
            report.notes.push(format!(
                "tier downgrade ({recorded} -> {requested}) not performed: downgrade means stop managing, never delete; staying at {recorded}"
            ));
        }
    }

    let ctx = build_context(
        &project_name,
        &one_liner,
        &areas,
        &stack,
        tier,
        &opts.binary_version,
    );

    // Phase 1: state on disk with policy disarmed (fresh installs) BEFORE any
    // commit, so the freshly wired hooks let the scaffold commit through.
    let mut state = ProjectState {
        schema_version: 1,
        tier,
        scaffold_version: opts.binary_version.clone(),
        stack: stack.clone(),
        areas: areas.clone(),
        policy_armed: previous.as_ref().is_some_and(|s| s.policy_armed),
        git_hooks: previous
            .as_ref()
            .map_or_else(|| GIT_HOOKS_UNWIRED.to_string(), |s| s.git_hooks.clone()),
        permission_preset: preset.clone(),
        product_one_liner: one_liner.clone(),
    };
    state.store(root)?;

    // Phase 2: install manifest entries for this tier + preset.
    let mut installed = InstalledManifest::load_or_default(root, &opts.binary_version)?;
    let mut written: Vec<String> = vec![PROJECT_TOML.to_string()];
    for entry in &manifest.entries {
        if !entry.applies(tier, &preset) {
            continue;
        }
        install_entry(
            source,
            root,
            entry,
            &ctx,
            tier,
            opts.force,
            &mut installed,
            &mut report,
            &mut written,
        )?;
    }

    installed.scaffold_version.clone_from(&opts.binary_version);
    installed.store(root)?;
    state.store(root)?;
    written.push(super::state::INSTALLED_MANIFEST.to_string());
    written.push(super::state::BASELINE_DIR.to_string());

    // Phase 3: the scaffold commit (fresh repos only) — policy still
    // disarmed, and hooks not yet wired: bootstrap must never depend on the
    // capabilities of whatever `codeflow` binary is on PATH.
    if fresh_repo {
        gitutil::add_and_commit(
            root,
            &written,
            &format!("chore: scaffold codeflow {tier} tier"),
        )?;
        report
            .notes
            .push("scaffold commit created (branch policy was not yet armed)".to_string());
    } else {
        report.notes.push(
            "existing repository: scaffold files left uncommitted — review and commit them on a branch".to_string(),
        );
    }

    // Phase 4: wire git hooks unless another manager owns them (AC #2).
    match detect::detect_hook_manager(root) {
        None => {
            gitutil::config_set(root, "core.hooksPath", CODEFLOW_HOOKS_PATH)?;
            state.git_hooks = GIT_HOOKS_WIRED.to_string();
            report
                .notes
                .push(format!("git hooks wired: core.hooksPath = {CODEFLOW_HOOKS_PATH}"));
        }
        Some(manager) => {
            state.git_hooks = GIT_HOOKS_UNWIRED.to_string();
            report.notes.push(format!(
                "existing hook manager detected ({manager}) — not clobbered. To enable codeflow's git gates, call the shims from your hook manager, e.g. add `\"$(git rev-parse --show-toplevel)\"/{CODEFLOW_HOOKS_PATH}/pre-commit` to its pre-commit step (same for commit-msg and pre-push). Recorded git_hooks = \"unwired\"; `codeflow doctor` will surface this."
            ));
        }
    }

    // Phase 5: arm branch policy and persist the final state (including the
    // hook-wiring outcome from phase 4).
    if !state.policy_armed {
        state.policy_armed = true;
        report.notes.push(
            "branch policy armed (policy_armed = true); start feature work on a feat/* branch"
                .to_string(),
        );
    }
    state.store(root)?;

    Ok(report)
}

fn effective_tier(recorded: Option<Tier>, requested: Option<Tier>) -> Tier {
    match (recorded, requested) {
        (Some(r), Some(q)) => r.max(q),
        (Some(r), None) => r,
        (None, q) => q.unwrap_or(Tier::Standard),
    }
}

fn project_name(root: &Path) -> String {
    root.canonicalize()
        .ok()
        .as_deref()
        .unwrap_or(root)
        .file_name()
        .map_or_else(|| "project".to_string(), |n| n.to_string_lossy().to_string())
}

/// Builds the substitution context shared by init and update.
pub(crate) fn build_context(
    project_name: &str,
    one_liner: &str,
    areas: &[String],
    stack: &str,
    tier: Tier,
    version: &str,
) -> TemplateContext {
    let mut ctx = TemplateContext::new();
    ctx.set("PROJECT_NAME", project_name);
    ctx.set("PROJECT_ONE_LINER", one_liner);
    ctx.set("AREAS", areas.join(", "));
    ctx.set("STACK", stack);
    ctx.set("TIER", tier.as_str());
    ctx.set("SCAFFOLD_VERSION", version);
    ctx.set("DATE", today_utc());
    ctx
}

/// Renders an entry's asset: read, UTF-8 check, optional template
/// substitution, minimal-tier policy softening. `None` (with a report line)
/// when the asset is missing — parallel asset authoring must not break init.
pub(crate) fn render_entry(
    source: &dyn super::AssetSource,
    entry: &ManifestEntry,
    ctx: &TemplateContext,
    tier: Tier,
    report: &mut Report,
) -> Result<Option<String>, ScaffoldError> {
    let asset_path = format!("base/{}", entry.src);
    let Some(text) = read_text(source, &asset_path) else {
        report.file_with_notes(
            &entry.dest,
            Action::MissingAsset,
            vec![format!("asset {asset_path} not shipped in this build")],
        );
        report.warnings.push(format!(
            "asset {asset_path} missing (not authored yet, or not valid UTF-8) — {} skipped",
            entry.dest
        ));
        return Ok(None);
    };
    let mut rendered = if entry.template {
        ctx.substitute(&text)
    } else {
        text
    };
    if tier == Tier::Minimal && entry.dest.ends_with("policy.json") {
        rendered = soften_policy_for_minimal(&rendered)?;
    }
    Ok(Some(rendered))
}

/// `--minimal` flips every blocking git policy to warn except `secret_scan`,
/// and the push test gate to off (charter §4.2: blocking policy on a scratch
/// repo trains bypassing).
fn soften_policy_for_minimal(policy: &str) -> Result<String, ScaffoldError> {
    let mut value: serde_json::Value = serde_json::from_str(policy)?;
    if let Some(git) = value.get_mut("git").and_then(serde_json::Value::as_object_mut) {
        for (key, val) in git.iter_mut() {
            if key == "secret_scan" {
                continue;
            }
            if key == "test_gate_on_push" {
                *val = serde_json::Value::String("off".to_string());
                continue;
            }
            if val.as_str() == Some("block") {
                *val = serde_json::Value::String("warn".to_string());
            }
        }
    }
    let mut text = serde_json::to_string_pretty(&value)?;
    text.push('\n');
    Ok(text)
}

fn record(
    installed: &mut InstalledManifest,
    entry: &ManifestEntry,
    sha256: String,
) {
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

fn write_dest(
    root: &Path,
    entry: &ManifestEntry,
    content: &str,
) -> Result<(), ScaffoldError> {
    let path = root.join(&entry.dest);
    write_file(&path, content.as_bytes())?;
    if entry.exec {
        set_exec(&path, true)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn install_entry(
    source: &dyn super::AssetSource,
    root: &Path,
    entry: &ManifestEntry,
    ctx: &TemplateContext,
    tier: Tier,
    force: bool,
    installed: &mut InstalledManifest,
    report: &mut Report,
    written: &mut Vec<String>,
) -> Result<(), ScaffoldError> {
    let Some(rendered) = render_entry(source, entry, ctx, tier, report)? else {
        return Ok(());
    };
    let dest_path = root.join(&entry.dest);
    let exists = dest_path.exists();
    let version = ctx.get("SCAFFOLD_VERSION").unwrap_or("0");

    match entry.ownership {
        Ownership::Managed | Ownership::UserOwned => {
            if !exists || force {
                write_dest(root, entry, &rendered)?;
                record(installed, entry, hash::sha256_hex(rendered.as_bytes()));
                Baseline::write(root, &entry.dest, &rendered)?;
                written.push(entry.dest.clone());
                report.file(&entry.dest, if exists { Action::Forced } else { Action::Created });
                return Ok(());
            }
            // Exists, no force: never overwrite. Report precisely why.
            let current = std::fs::read_to_string(&dest_path).unwrap_or_default();
            let recorded = installed.files.get(&entry.dest);
            let note = match (entry.ownership, recorded) {
                (Ownership::UserOwned, _) => {
                    // Make sure update has a shipped-default baseline to diff.
                    if Baseline::read(root, &entry.dest).is_none() {
                        Baseline::write(root, &entry.dest, &rendered)?;
                    }
                    if recorded.is_none() {
                        record(installed, entry, hash::sha256_hex(rendered.as_bytes()));
                    }
                    "user-owned: existing file left untouched".to_string()
                }
                (_, Some(rec)) if rec.sha256 == hash::sha256_hex(current.as_bytes()) => {
                    if current == rendered {
                        report.file(&entry.dest, Action::Unchanged);
                        return Ok(());
                    }
                    "newer shipped version available — run `codeflow update`".to_string()
                }
                (_, Some(_)) => {
                    "locally modified — run `codeflow update` to 3-way merge".to_string()
                }
                (_, None) => {
                    if current == rendered {
                        // Identical content: adopt it as managed.
                        record(installed, entry, hash::sha256_hex(current.as_bytes()));
                        Baseline::write(root, &entry.dest, &rendered)?;
                        report.file_with_notes(
                            &entry.dest,
                            Action::Unchanged,
                            vec!["identical existing file adopted as managed".to_string()],
                        );
                        return Ok(());
                    }
                    "exists and is not codeflow-managed — left untouched (--force to overwrite)"
                        .to_string()
                }
            };
            report.file_with_notes(&entry.dest, Action::Skipped, vec![note]);
        }
        Ownership::ManagedRegion => match entry.region.unwrap_or(RegionFormat::Markdown) {
            RegionFormat::Json => {
                if !exists || force {
                    write_dest(root, entry, &rendered)?;
                    record(installed, entry, hash::sha256_hex(rendered.as_bytes()));
                    Baseline::write(root, &entry.dest, &rendered)?;
                    written.push(entry.dest.clone());
                    report.file(&entry.dest, if exists { Action::Forced } else { Action::Created });
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
                    written.push(entry.dest.clone());
                    report.file_with_notes(&entry.dest, Action::Merged, lines);
                }
            }
            format @ (RegionFormat::Markdown | RegionFormat::Hash) => {
                let block = region::extract_block(&rendered, format)
                    .unwrap_or_else(|| region::wrap_block(&rendered, format, version));
                record(installed, entry, hash::sha256_hex(block.as_bytes()));
                Baseline::write(root, &entry.dest, &block)?;
                if !exists || force {
                    // Fresh file: ship the full rendered asset when it carries
                    // its own markers, else just the wrapped block.
                    let content = if region::extract_block(&rendered, format).is_some() {
                        rendered.clone()
                    } else {
                        format!("{block}\n")
                    };
                    write_dest(root, entry, &content)?;
                    written.push(entry.dest.clone());
                    report.file(&entry.dest, if exists { Action::Forced } else { Action::Created });
                    return Ok(());
                }
                let current = std::fs::read_to_string(&dest_path)
                    .map_err(|e| ScaffoldError::io(&dest_path, e))?;
                let (next, outcome) = region::upsert_block(&current, &block, format);
                match outcome {
                    BlockOutcome::Unchanged => report.file(&entry.dest, Action::Unchanged),
                    BlockOutcome::Replaced => {
                        write_dest(root, entry, &next)?;
                        written.push(entry.dest.clone());
                        report.file_with_notes(
                            &entry.dest,
                            Action::Changed,
                            vec!["managed block updated; content outside markers untouched"
                                .to_string()],
                        );
                    }
                    BlockOutcome::Appended => {
                        write_dest(root, entry, &next)?;
                        written.push(entry.dest.clone());
                        report.file_with_notes(
                            &entry.dest,
                            Action::Merged,
                            vec!["codeflow block appended to existing file".to_string()],
                        );
                    }
                }
            }
        },
    }
    Ok(())
}

/// Today as `YYYY-MM-DD` (UTC), no chrono dependency.
fn today_utc() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    #[allow(clippy::cast_possible_wrap)]
    let days = (secs / 86_400) as i64;
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Howard Hinnant's `civil_from_days` (public-domain algorithm).
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_date_known_values() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1)); // 2024-01-01
    }

    #[test]
    fn tier_resolution() {
        assert_eq!(effective_tier(None, None), Tier::Standard);
        assert_eq!(effective_tier(None, Some(Tier::Minimal)), Tier::Minimal);
        assert_eq!(
            effective_tier(Some(Tier::Standard), Some(Tier::Minimal)),
            Tier::Standard,
            "downgrade ignored"
        );
        assert_eq!(
            effective_tier(Some(Tier::Minimal), Some(Tier::Full)),
            Tier::Full
        );
    }

    #[test]
    fn minimal_policy_softening() {
        let policy = r#"{
            "schema_version": 1,
            "git": {
                "commit_to_protected": "block",
                "secret_scan": "block",
                "force_push_unprotected": "allow",
                "test_gate_on_push": "warn",
                "protected_branches": ["main"]
            }
        }"#;
        let softened = soften_policy_for_minimal(policy).unwrap();
        let v: serde_json::Value = serde_json::from_str(&softened).unwrap();
        assert_eq!(v["git"]["commit_to_protected"], "warn");
        assert_eq!(v["git"]["secret_scan"], "block");
        assert_eq!(v["git"]["force_push_unprotected"], "allow");
        assert_eq!(v["git"]["test_gate_on_push"], "off");
        assert_eq!(v["git"]["protected_branches"][0], "main");
    }
}
