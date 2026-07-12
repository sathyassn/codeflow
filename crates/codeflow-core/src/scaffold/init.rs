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
        // Owner autonomy posture (ADR-0008): a brand-new init with no answer
        // defaults to acceptEdits, not `default`. An existing repo keeps its
        // recorded preset (the `.or_else` above), so update never surprises.
        .unwrap_or_else(|| "acceptEdits".to_string());
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

    // Finalize the recorded state BEFORE the fresh-repo scaffold commit, so the
    // committed project.toml carries its FINAL policy_armed + git_hooks values
    // and the working tree matches HEAD once init returns. The hook-manager
    // check here is a pure detection (AC #2); the actual `git config
    // core.hooksPath` side effect is deferred to phase 4 (below) so the scaffold
    // commit still runs with hooks unwired and never depends on whatever
    // `codeflow` binary is on PATH honoring the gate token (charter D9). Baking
    // the values in before the commit is what stops init leaving a fresh adopter
    // with a dirty, un-committable project.toml on the just-armed protected
    // branch: a post-commit re-store would otherwise flip git_hooks and dirty
    // the file that the armed policy now refuses to let them commit.
    let hook_manager = detect::detect_hook_manager(root);
    state.git_hooks = if hook_manager.is_none() {
        GIT_HOOKS_WIRED.to_string()
    } else {
        GIT_HOOKS_UNWIRED.to_string()
    };
    // Arm the policy. Fresh repos need the committed project.toml to carry the
    // armed state — otherwise a later `git checkout <protected-branch>` or a
    // fresh clone silently resurrects the disarmed bootstrap state. Existing
    // repos are armed for their still-uncommitted scaffold.
    let newly_armed = !state.policy_armed;
    state.policy_armed = true;
    state.store(root)?;

    // Phase 3: the scaffold commit (fresh repos only). project.toml already
    // carries its final values, so nothing dirties the tree afterward. The
    // commit is a sanctioned path: it passes via the gate-context token, and
    // hooks are still unwired at this point.
    if fresh_repo {
        gitutil::add_and_commit(
            root,
            &written,
            &format!("chore: scaffold codeflow {tier} tier"),
        )?;
        report
            .notes
            .push("scaffold commit created with branch policy armed".to_string());
    } else {
        report.notes.push(
            "existing repository: scaffold files left uncommitted — review and commit them on a branch".to_string(),
        );
    }

    // Phase 4: apply the hook-wiring side effect, deferred from the detection
    // above so the scaffold commit ran with hooks unwired (AC #2).
    match hook_manager {
        None => {
            gitutil::config_set(root, "core.hooksPath", CODEFLOW_HOOKS_PATH)?;
            report
                .notes
                .push(format!("git hooks wired: core.hooksPath = {CODEFLOW_HOOKS_PATH}"));
        }
        Some(manager) => {
            report.notes.push(format!(
                "existing hook manager detected ({manager}) — not clobbered. To enable codeflow's git gates, call the shims from your hook manager, e.g. add `\"$(git rev-parse --show-toplevel)\"/{CODEFLOW_HOOKS_PATH}/pre-commit` to its pre-commit step (same for commit-msg and pre-push). Recorded git_hooks = \"unwired\"; `codeflow doctor` will surface this."
            ));
        }
    }

    // Harness honesty (ADR-0008): the permission preset above configured
    // Claude Code only. When the codex layer is present, say what binds it —
    // a fixed posture in .codex/config.toml — and the one manual step its
    // in-session guards need. Trust state is codex-internal and not
    // inspectable from here, so this is a pointer, never a claim.
    let codex_dir = root.join(".codex");
    if codex_dir.join("hooks.json").exists() || codex_dir.join("config.toml").exists() {
        report.notes.push(
            "codex harness present (.codex/): autonomy posture lives in .codex/config.toml — the Claude Code permission preset does not apply to codex; in-session guards are wired structurally and activate after a one-time `/hooks` trust inside interactive codex (git hooks + CI enforce regardless)"
                .to_string(),
        );
    }

    // Phase 5: for an existing repo the armed state is still uncommitted —
    // surface the arming and the branch guidance. Fresh repos already committed
    // the armed state above, so their tree is clean and needs no such note.
    if newly_armed && !fresh_repo {
        report.notes.push(
            "branch policy armed (policy_armed = true); start feature work on a feat/* branch"
                .to_string(),
        );
    }

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
/// substitution. `None` (with a report line) when the asset is missing —
/// parallel asset authoring must not break init.
// Result kept for symmetry with the render/install pipeline (callers use `?`);
// the body is infallible now that minimal-policy softening is gone.
#[allow(clippy::unnecessary_wraps)]
pub(crate) fn render_entry(
    source: &dyn super::AssetSource,
    entry: &ManifestEntry,
    ctx: &TemplateContext,
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
    let rendered = if entry.template {
        ctx.substitute(&text)
    } else {
        text
    };
    Ok(Some(rendered))
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
    _tier: Tier,
    force: bool,
    installed: &mut InstalledManifest,
    report: &mut Report,
    written: &mut Vec<String>,
) -> Result<(), ScaffoldError> {
    let Some(rendered) = render_entry(source, entry, ctx, report)? else {
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
                        // Identical content, no installed record. Two distinct
                        // cases, told apart by the baseline (only codeflow
                        // writes `.codeflow/.baseline/`): a matching baseline
                        // means an earlier init wrote this file but never got
                        // to record it (interrupted mid-run) — the file did
                        // NOT pre-exist as a user file, so this run completes
                        // its creation. No baseline means a genuinely
                        // pre-existing user file: adopt it as managed.
                        let written_by_codeflow =
                            Baseline::read(root, &entry.dest).is_some_and(|b| b == rendered);
                        record(installed, entry, hash::sha256_hex(current.as_bytes()));
                        Baseline::write(root, &entry.dest, &rendered)?;
                        if written_by_codeflow {
                            written.push(entry.dest.clone());
                            report.file_with_notes(
                                &entry.dest,
                                Action::Created,
                                vec!["written by an earlier interrupted init — record completed"
                                    .to_string()],
                            );
                        } else {
                            report.file_with_notes(
                                &entry.dest,
                                Action::Unchanged,
                                vec!["identical existing file adopted as managed".to_string()],
                            );
                        }
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
        .map_or(0, |d| d.as_secs());
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

    /// Renders the AGENTS.md entry the shipped manifest selects at `tier`.
    fn render_shipped_agents(tier: Tier) -> String {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        let source = crate::scaffold::DirSource::new(root);
        let manifest = ScaffoldManifest::load(&source).expect("shipped manifest loads");
        let entry = manifest
            .entries
            .iter()
            .find(|e| e.dest == "AGENTS.md" && e.applies(tier, "default"))
            .expect("an AGENTS.md entry for this tier");
        let ctx = build_context("demo", "one liner", &["core".to_string()], "rust", tier, "9.9.9");
        let mut report = Report::new("test".to_string());
        render_entry(&source, entry, &ctx, &mut report)
            .expect("render succeeds")
            .expect("AGENTS.md asset is present")
    }

    #[test]
    fn minimal_agents_is_tier_honest() {
        // A minimal repo installs no project-management/ and no /cf-* skills, so
        // its rendered AGENTS.md must not advertise machinery its tier never
        // installed. The full tier still does — the split is honest both ways.
        let minimal = render_shipped_agents(Tier::Minimal);
        assert!(
            !minimal.contains("project-management/"),
            "minimal AGENTS.md must not reference project-management/:\n{minimal}"
        );
        assert!(
            !minimal.contains("/cf-plan"),
            "minimal AGENTS.md must not reference /cf-plan:\n{minimal}"
        );

        let full = render_shipped_agents(Tier::Full);
        assert!(
            full.contains("project-management/") && full.contains("/cf-plan"),
            "full AGENTS.md still advertises the full method"
        );
    }
}
