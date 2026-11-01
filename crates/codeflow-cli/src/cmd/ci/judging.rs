//! The policy a `codeflow ci` run judges with: the commit whose policy
//! judges the range, and what a missing or unreadable policy file there
//! selects (TSK-1077). Pre-push runs `codeflow ci --policy-from`, so this
//! file is a hook source input (`hooks/source_identity.rs`): an installed
//! binary built before an edit here warns that it is stale.
use super::{print_source_banner, report_invalid_policy, rev_parse, short};
use codeflow_core::hooks::policy::Policy;
use codeflow_core::hooks::policy_schema;
use std::path::Path;

/// The commit whose policy judges the range.
pub(super) enum Authority {
    /// `--policy-from`: the target tip the pre-push hook established.
    Named(String),
    /// The resolved base, which the caller chose: the hosted templates pass
    /// the pull request's target tip, but a local caller can name any commit.
    Base(String),
    /// The resolved base, with the commit checks run from another commit
    /// (`--commits-from`): never the hosted verdict, whose commit checks
    /// run from the base.
    SplitBase(String),
}

impl Authority {
    pub(super) fn sha(&self) -> &str {
        match self {
            Self::Named(sha) | Self::Base(sha) | Self::SplitBase(sha) => sha,
        }
    }

    pub(super) fn describe(&self) -> String {
        match self {
            Self::Named(sha) => format!("{}, the commit --policy-from names", short(sha)),
            Self::Base(sha) => format!(
                "base {}, the commit this run was given; it is the hosted verdict only when that is the pull request's target tip, which the hosted jobs pass",
                short(sha)
            ),
            Self::SplitBase(sha) => format!(
                "base {}, the commit this run was given; the commit checks run from another commit (--commits-from), so this is not the hosted verdict",
                short(sha)
            ),
        }
    }
}

/// The authority for a run: the commit `--policy-from` names, else the
/// resolved base. `Err` (after naming the error) when `--policy-from` does
/// not name a commit, which verifies nothing.
pub(super) fn select(
    root: &Path,
    policy_from: Option<&str>,
    base: Option<&str>,
    split: bool,
) -> Result<Option<Authority>, ()> {
    let Some(rev) = policy_from else {
        return Ok(base.map(|sha| {
            if split {
                Authority::SplitBase(sha.to_owned())
            } else {
                Authority::Base(sha.to_owned())
            }
        }));
    };
    match rev_parse(root, rev) {
        Ok(Some(sha)) => Ok(Some(Authority::Named(sha))),
        Ok(None) => {
            eprintln!(
                "codeflow ci: error: --policy-from '{rev}' does not name a commit — nothing was verified"
            );
            Err(())
        }
        Err(error) => {
            eprintln!(
                "codeflow ci: {}",
                codeflow_core::git::display_message(&error)
            );
            Err(())
        }
    }
}

/// The policy a run judges with, and its raw form for the level origins.
pub(super) struct Judging {
    pub(super) policy: Policy,
    pub(super) raw: Result<Option<serde_json::Value>, String>,
    /// The trusted automation profiles: only those of a policy read at a
    /// commit, never the working copy's, so a head cannot add one for
    /// itself and a profile the authority revoked no longer applies.
    pub(super) profiles: Vec<codeflow_core::hooks::policy::AutomationProfile>,
}

/// The policy a range is judged with (sathyassn/codeflow#22): the one
/// recorded at `authority`, read as git data, so a head that loosens its own
/// policy cannot judge itself. The hosted templates run from a checkout of
/// the target tip they pass as the base, so there the two are the same.
/// Without an authority, the working copy's. When the authority's tree
/// proves the policy file absent (the adoption change itself), the shipped
/// defaults (TSK-1077). `None` (after naming each error) when the
/// authority's policy is unreadable or invalid, which verifies nothing.
///
/// The policy judges the commit, branch, PR-body and work checks and their
/// levels. Some inputs stay elsewhere, by design: the auto-detected range's
/// protected branches come from the working copy (the range is found before
/// any authority is known), the change class widens the authority's product
/// and watched paths with the working copy's (neither side can narrow them),
/// and the work-record baseline has its own source (`--baseline-from`).
pub(super) fn judging_policy(
    root: &Path,
    authority: Option<&Authority>,
    working: Policy,
) -> Option<Judging> {
    let from_working = |working: Policy| {
        print_source_banner(root);
        Judging {
            policy: working,
            raw: codeflow_core::hooks::adoption::raw_policy(root),
            profiles: Vec::new(),
        }
    };
    let Some(authority) = authority else {
        return Some(from_working(working));
    };
    let base = authority.sha();
    let text = match codeflow_core::hooks::landed_policy::policy_text_at(root, base) {
        Ok(Some(text)) => text,
        // A proven absence at the authority selects the shipped defaults,
        // never the working copy, which the range itself can write
        // (TSK-1077 AC-5).
        Ok(None) => {
            println!(
                "codeflow ci: verifying against the shipped defaults, since .codeflow/policy.json is absent at {}",
                authority.describe()
            );
            let policy = Policy::default();
            return Some(Judging {
                profiles: policy.git.automation_profiles.clone(),
                policy,
                raw: Ok(None),
            });
        }
        Err(error) => {
            eprintln!(
                "codeflow ci: error: cannot read .codeflow/policy.json at {}: {error} — nothing was verified",
                short(base), error = crate::cmd::shown(&error)
            );
            return None;
        }
    };
    if let Err(errors) = policy_schema::validate_policy_str(&text) {
        eprintln!(
            "codeflow ci: the range is judged with the policy at {}, which is invalid:",
            short(base)
        );
        report_invalid_policy(&errors);
        return None;
    }
    // The validator's strict deserialize already accepted the text.
    let parsed = serde_json::from_str::<Policy>(&text)
        .and_then(|policy| serde_json::from_str(&text).map(|raw| (policy, raw)));
    let (policy, raw) = match parsed {
        Ok(parsed) => parsed,
        Err(error) => {
            eprintln!(
                "codeflow ci: error: .codeflow/policy.json at {} cannot be read: {error} — nothing was verified",
                short(base), error = crate::cmd::shown(&error)
            );
            return None;
        }
    };
    println!(
        "codeflow ci: verifying against .codeflow/policy.json at {}",
        authority.describe()
    );
    if serde_json::to_value(&policy).ok()
        != Policy::load(root)
            .ok()
            .and_then(|value| serde_json::to_value(value).ok())
    {
        println!(
            "codeflow ci: the working copy's .codeflow/policy.json differs from the one judging; its rules judge commits once it lands on the target"
        );
    }
    Some(Judging {
        profiles: policy.git.automation_profiles.clone(),
        policy,
        raw: Ok(Some(raw)),
    })
}

/// The size and ignored-file rules (TSK-323), read with their policy keys,
/// the project kind and the `.gitignore` files at the authority commit,
/// never the range or the working copy; a policy that commit lacks gives
/// the shipped defaults. A run with no authority commit (neither
/// `--policy-from` nor a base) cannot judge them and says so, rather than
/// reading them at the head it is judging (TSK-1077, ADR-0091).
///
/// # Errors
/// No authority commit, or as `ContentRules::at_authority`.
pub(super) fn content_rules(
    root: &Path,
    authority: Option<&Authority>,
) -> Result<codeflow_core::hooks::path_policy::ContentRules, String> {
    let Some(authority) = authority else {
        return Err(
            "the size and ignored-file rules are read at the authority commit, and this run has none; pass --base or --policy-from".into(),
        );
    };
    codeflow_core::hooks::path_policy::ContentRules::at_authority(root, authority.sha())
}
