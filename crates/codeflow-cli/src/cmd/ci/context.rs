//! Resolve the range and branch context before any range rule runs.
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use codeflow_core::hooks::repo;
use codeflow_core::workgraph::{declared_work_target_at_revision, default_work_target};

use super::{detect_range, detect_target, named_branch, resolve_base, rev_parse, CiArgs};

pub(super) struct Context {
    pub head: String,
    pub base: Option<String>,
    pub base_candidates: Vec<String>,
    pub range_source: String,
    pub branch: String,
    pub into: Option<String>,
    pub target: String,
}

pub(super) fn resolve(
    root: &Path,
    args: &CiArgs,
    environment: &BTreeMap<String, String>,
    protected: &[String],
) -> Result<Context, String> {
    let env = |key: &str| environment.get(key).cloned().filter(|v| !v.is_empty());
    let detected = detect_range(env, protected);
    let head_ref = args.head.as_deref().unwrap_or(&detected.head);
    let head = rev_parse(root, head_ref)?
        .ok_or_else(|| format!("head '{head_ref}' does not name a commit; nothing was verified"))?;
    let head_source = head_source(args.head.is_some(), env);
    let (branch, branch_source) = branch(root, args, environment)?;
    println!("codeflow ci: head {head} ({head_source}: {head_ref}); branch context '{}' ({branch_source})", if branch.is_empty() { "none" } else { &branch });
    if args.head.is_some() && args.branch.is_none() {
        println!("codeflow ci: context: pass --branch <name> if this head belongs to another line; branch context selects frozen or amendable acceptance binding");
    }
    let explicit_target = args
        .into
        .clone()
        .map(|name| (name, "--into"))
        .or_else(|| detect_target(env).map(|name| (name, "CI target metadata")));
    let declared = if explicit_target.is_none() {
        declared_target(root, &branch, &head)?
    } else {
        None
    };
    let mut range_source = detected.source;
    let mut base_candidates = detected.base_candidates;
    if let Some(base) = &args.base {
        base_candidates = vec![base.clone()];
        range_source = "explicit --base flag".into();
    } else if range_source == "policy protected branches (fallback)" {
        if let Some((target, source)) = explicit_target
            .as_ref()
            .map(|(name, source)| (name, *source))
            .or_else(|| {
                declared
                    .as_ref()
                    .map(|name| (name, "branch declared target"))
            })
        {
            let candidates = target_candidates(target);
            if resolve_base(root, &candidates)?.is_some() {
                base_candidates = candidates;
                range_source = format!("{source} '{target}' for branch '{branch}'");
            } else {
                let warning = codeflow_core::remedy::Finding::new(
                    format!("declared target '{target}' does not resolve; base falls back to the policy protected branches"),
                    codeflow_core::remedy::CI_BASE_UNRESOLVED.remedy(),
                );
                println!("{}", warning.line("codeflow ci", "warning"));
            }
        }
    }
    let base = resolve_base(root, &base_candidates).map_err(|error| base_refusal(&error))?;
    // Pin every downstream reader to the same object, even if a ref moves.
    if let Some(base) = &base {
        base_candidates = vec![base.clone()];
    }
    let tracked = tracked_range(root, base.as_deref(), &head)?;
    if branch.is_empty() && tracked {
        return Err("detached checkout has no branch context; pass --branch <name> for this head before acceptance can be judged".into());
    }
    // A body-less push with no changed commits has no acceptance or PR-body
    // decision that depends on a target. Several refs may share its tip.
    let has_body =
        args.pr_body.is_some() || args.pr_body_file.is_some() || env("CODEFLOW_PR_BODY").is_some();
    let empty_push = base.as_deref() == Some(head.as_str()) && !has_body;
    let (into, source) = if let Some((name, source)) = explicit_target {
        (Some(normalize(&name)), source)
    } else if let Some(target) = declared {
        (Some(normalize(&target)), "branch declared target")
    } else if empty_push {
        (None, "range without an acceptance target")
    } else {
        let inferred = base
            .as_deref()
            .map(|base| target_at_commit(root, base, protected, tracked || has_body))
            .transpose()?
            .flatten();
        (inferred, "branches at the resolved base commit")
    };
    if into.is_none() && base.is_some() && tracked && !empty_push {
        return Err("acceptance target cannot be derived from the resolved base commit; pass --into <target> or --branch <name> with a declared target; nothing was verified".into());
    }
    let target = super::line_target(root, into.as_deref())?;
    if into.is_some() {
        println!("codeflow ci: acceptance target '{target}' ({source})");
    } else {
        println!("codeflow ci: target '{target}' (repository default)");
    }
    Ok(Context {
        head,
        base,
        base_candidates,
        range_source,
        branch,
        into,
        target,
    })
}

fn base_refusal(error: &str) -> String {
    codeflow_core::remedy::Finding::new(
        format!(
            "cannot resolve the base because git refused it: {}",
            error.replace('\n', "\n  ")
        ),
        codeflow_core::remedy::CI_BASE_REFUSED.remedy(),
    )
    .to_string()
}

fn branch(
    root: &Path,
    args: &CiArgs,
    environment: &BTreeMap<String, String>,
) -> Result<(String, String), String> {
    if let Some(name) = &args.branch {
        return Ok((name.clone(), "--branch".into()));
    }
    let detected =
        super::detect_branch(|key| environment.get(key).cloned().filter(|v| !v.is_empty()));
    for key in [
        "GITHUB_HEAD_REF",
        "CI_MERGE_REQUEST_SOURCE_BRANCH_NAME",
        "CI_COMMIT_REF_NAME",
        "BITBUCKET_BRANCH",
    ] {
        if environment.get(key).is_some_and(|value| !value.is_empty()) {
            return Ok((detected.unwrap_or_default(), key.into()));
        }
    }
    let name = repo::open(root)?
        .map(|repo| repo::current_branch(&repo))
        .transpose()?
        .unwrap_or_default();
    let source = if name.is_empty() {
        "detached checkout"
    } else {
        "checkout HEAD"
    };
    Ok((name, source.into()))
}

fn normalize(name: &str) -> String {
    named_branch(name).unwrap_or_else(|| name.to_string())
}

fn target_candidates(target: &str) -> Vec<String> {
    let name = normalize(target);
    vec![
        format!("refs/remotes/origin/{name}"),
        format!("refs/heads/{name}"),
    ]
}

fn declared_target(root: &Path, branch: &str, head: &str) -> Result<Option<String>, String> {
    if let Some(target) =
        declared_work_target_at_revision(root, branch, head).map_err(|e| e.to_string())?
    {
        return Ok(Some(target));
    }
    if branch.starts_with("integration/") {
        return default_work_target(root).map_err(|e| e.to_string());
    }
    let Some(suffix) = branch.strip_prefix("plan/") else {
        return Ok(None);
    };
    let candidate = format!("integration/{suffix}");
    let Ok(epic) = codeflow_core::workgraph::line_adoption::epic_id(&candidate) else {
        return Ok(None);
    };
    let names = branches(root)?
        .into_iter()
        .filter(|(name, _)| name.starts_with(&format!("integration/{epic}-")))
        .map(|(name, _)| name)
        .collect::<BTreeSet<_>>();
    match names.len() {
        0 => Ok(None),
        1 => Ok(names.into_iter().next()),
        _ => Err(format!("planning branch '{branch}' has several epic lines: {}; pass --into <target> and --base <ref>", names.into_iter().collect::<Vec<_>>().join(", "))),
    }
}

/// Infer by object identity, never by the spelling supplied by the caller.
fn target_at_commit(
    root: &Path,
    base: &str,
    protected: &[String],
    required: bool,
) -> Result<Option<String>, String> {
    let mut names = branches(root)?
        .into_iter()
        .filter(|(_, sha)| sha == base)
        .map(|(name, _)| name)
        .collect::<BTreeSet<_>>();
    if names.len() > 1 {
        let targets = names
            .iter()
            .filter(|name| {
                name.starts_with("integration/") || protected.iter().any(|p| normalize(p) == **name)
            })
            .cloned()
            .collect::<BTreeSet<_>>();
        if !targets.is_empty() {
            names = targets;
        }
    }
    if names.len() > 1 {
        if !required {
            return Ok(None);
        }
        return Err(format!("resolved base commit names several branches: {}; pass --into <target> or --branch <name> with a declared target", names.into_iter().collect::<Vec<_>>().join(", ")));
    }
    Ok(names.into_iter().next())
}

fn branches(root: &Path) -> Result<Vec<(String, String)>, String> {
    let repo = repo::open(root)?.ok_or("repository is absent")?;
    let mut names = Vec::new();
    for branch in repo.branches(None).map_err(|e| e.to_string())? {
        let (branch, _) = branch.map_err(|e| e.to_string())?;
        if branch.get().symbolic_target_bytes().is_some() {
            continue;
        }
        let Some(name) = branch.name().map_err(|e| e.to_string())? else {
            // A byte-only name cannot be supplied as CLI branch context.
            continue;
        };
        let name = if branch.get().is_remote() {
            name.split_once('/').map_or(name, |(_, name)| name)
        } else {
            name
        };
        let commit = branch.get().peel_to_commit().map_err(|e| e.to_string())?;
        names.push((name.to_string(), commit.id().to_string()));
    }
    Ok(names)
}

fn head_source(explicit: bool, env: impl Fn(&str) -> Option<String>) -> &'static str {
    if explicit {
        "--head"
    } else if env("CI_MERGE_REQUEST_DIFF_BASE_SHA").is_some() && env("CI_COMMIT_SHA").is_some() {
        "CI_COMMIT_SHA"
    } else if env("CI_MERGE_REQUEST_DIFF_BASE_SHA").is_none()
        && env("BITBUCKET_PR_DESTINATION_COMMIT").is_some()
        && env("BITBUCKET_COMMIT").is_some()
    {
        "BITBUCKET_COMMIT"
    } else {
        "checkout HEAD"
    }
}

fn tracked_range(root: &Path, base: Option<&str>, head: &str) -> Result<bool, String> {
    use codeflow_core::workgraph::work_start::durable_work_tracking_enabled_at;
    Ok(durable_work_tracking_enabled_at(root, head)
        .map_err(|error| format!("cannot read head configuration: {error}"))?
        || base
            .map(|base| durable_work_tracking_enabled_at(root, base))
            .transpose()?
            .unwrap_or(false))
}
