//! `git-guard` — the `PreToolUse` (Bash) hook (charter §3.3, §6.1 plane 2).
//!
//! Intercepts git operations the client-side git hooks can't reach:
//! force-push / push / delete against protected branches, hard reset on a
//! protected branch, checkout-and-commit dodges, raw merges on protected,
//! and AI attribution or emoji in `gh pr create` bodies (charter §6.4,
//! AC #13). Every rule reads its level from `policy.json.git` — the guard
//! gives instant in-session feedback; CI + remote protection stay the hard
//! line (D19).

use std::path::PathBuf;

use serde::Deserialize;

use crate::security::pattern::split_command_segments;

use super::policy::GitPolicy;
use super::{standards, Violation};

/// Parsed Claude Code `PreToolUse` hook payload (the fields the guard reads).
#[derive(Debug, Deserialize)]
pub struct HookPayload {
    #[serde(default)]
    pub tool_name: String,
    #[serde(default)]
    pub tool_input: ToolInput,
    #[serde(default)]
    pub cwd: Option<PathBuf>,
}

/// `tool_input` for Bash invocations.
#[derive(Debug, Default, Deserialize)]
pub struct ToolInput {
    #[serde(default)]
    pub command: Option<String>,
}

impl HookPayload {
    /// Parse the hook JSON from stdin.
    ///
    /// # Errors
    ///
    /// Returns the serde error message when the payload is not valid JSON.
    pub fn parse(json: &str) -> Result<Self, String> {
        serde_json::from_str(json).map_err(|e| e.to_string())
    }

    /// The Bash command to evaluate, when this payload is a Bash tool call.
    #[must_use]
    pub fn bash_command(&self) -> Option<&str> {
        if self.tool_name == "Bash" {
            self.tool_input.command.as_deref()
        } else {
            None
        }
    }
}

/// Evaluation context for a guard run.
pub struct GuardContext<'a> {
    /// The `git` policy section in force.
    pub policy: &'a GitPolicy,
    /// Branch checked out where the command will run (empty when unknown).
    pub current_branch: &'a str,
    /// `true` when the `codeflow integrate` gate-context token is present —
    /// the sanctioned local merge path (charter §6.2).
    pub integrate_token: bool,
}

/// Evaluate a Bash command against the git policy.
///
/// Returns all violations found; the caller maps block-level violations to
/// exit 2 (deny) and warn-level to stderr advice.
#[must_use]
pub fn evaluate(command: &str, ctx: &GuardContext<'_>) -> Vec<Violation> {
    let mut violations = Vec::new();
    // Chained checkout/switch dodges change the branch later segments run on.
    let mut branch = ctx.current_branch.to_string();

    for segment in split_command_segments(command) {
        let tokens = shell_tokens(&segment);
        let Some((program, args)) = strip_env_assignments(&tokens) else {
            continue;
        };
        match program {
            "git" => check_git(args, &mut branch, ctx, &mut violations),
            "gh" => check_gh(args, ctx.policy, &mut violations),
            _ => {}
        }
    }
    violations
}

const SANCTIONED: &str = "land work via PR (gh pr create → merge on green CI) or `codeflow integrate <branch> --into <target>`";

#[allow(clippy::too_many_lines)]
fn check_git(
    args: &[String],
    branch: &mut String,
    ctx: &GuardContext<'_>,
    out: &mut Vec<Violation>,
) {
    let policy = ctx.policy;
    let Some((sub, rest)) = git_subcommand(args) else {
        return;
    };

    match sub {
        "checkout" | "switch" => {
            if let Some(target) = checkout_target(rest) {
                *branch = target;
            }
        }
        "commit" | "merge" | "cherry-pick" => {
            if policy.commit_to_protected.is_active()
                && policy.branch_is_protected(branch)
                && !ctx.integrate_token
            {
                out.push(Violation::new(
                    "git.commit_to_protected",
                    policy.commit_to_protected,
                    format!("`git {sub}` would create commits on protected branch '{branch}'"),
                    SANCTIONED.to_string(),
                ));
            }
        }
        "rebase" => {
            if policy.hard_reset_protected.is_active() && policy.branch_is_protected(branch) {
                out.push(Violation::new(
                    "git.hard_reset_protected",
                    policy.hard_reset_protected,
                    format!("`git rebase` rewrites history on protected branch '{branch}'"),
                    "rebase feature branches in their own worktree; protected history is append-only".to_string(),
                ));
            }
        }
        "reset" => {
            if policy.hard_reset_protected.is_active()
                && policy.branch_is_protected(branch)
                && rest.iter().any(|t| t == "--hard")
            {
                out.push(Violation::new(
                    "git.hard_reset_protected",
                    policy.hard_reset_protected,
                    format!("`git reset --hard` on protected branch '{branch}'"),
                    "create a revert commit instead; protected history is append-only".to_string(),
                ));
            }
        }
        "branch" => {
            if rest
                .iter()
                .any(|t| t == "-D" || t == "-d" || t == "--delete")
                && policy.delete_protected.is_active()
            {
                for target in rest
                    .iter()
                    .filter(|t| !t.starts_with('-'))
                    .filter(|t| policy.branch_is_protected(t))
                {
                    out.push(Violation::new(
                        "git.delete_protected",
                        policy.delete_protected,
                        format!("`git branch` would delete protected branch '{target}'"),
                        "protected branches are never deleted; remove the entry from git.protected_branches first if truly intended".to_string(),
                    ));
                }
            }
        }
        "push" => check_push(rest, branch, ctx, out),
        _ => {}
    }
}

fn check_push(rest: &[String], branch: &str, ctx: &GuardContext<'_>, out: &mut Vec<Violation>) {
    let policy = ctx.policy;
    let push = parse_push(rest, branch);

    for target in &push.deletions {
        if policy.delete_protected.is_active() && policy.branch_is_protected(target) {
            out.push(Violation::new(
                "git.delete_protected",
                policy.delete_protected,
                format!("`git push` would delete protected branch '{target}' on the remote"),
                "protected branches are never deleted remotely; adjust git.protected_branches first if truly intended".to_string(),
            ));
        }
    }

    for target in &push.updates {
        let protected = policy.branch_is_protected(target);
        if push.force {
            if protected {
                if policy.force_push_protected.is_active() {
                    out.push(Violation::new(
                        "git.force_push_protected",
                        policy.force_push_protected,
                        format!("force-push to protected branch '{target}'"),
                        SANCTIONED.to_string(),
                    ));
                }
            } else if policy.force_push_unprotected.is_active() {
                // Default policy is allow (D8): rebasing feature branches is
                // normal. Only flips on if the user tightens the policy.
                out.push(Violation::new(
                    "git.force_push_unprotected",
                    policy.force_push_unprotected,
                    format!("force-push to branch '{target}'"),
                    "policy git.force_push_unprotected restricts force-pushes in this repo".to_string(),
                ));
            }
        } else if protected && policy.push_to_protected.is_active() && !ctx.integrate_token {
            out.push(Violation::new(
                "git.push_to_protected",
                policy.push_to_protected,
                format!("direct push to protected branch '{target}'"),
                SANCTIONED.to_string(),
            ));
        }
    }
}

fn check_gh(args: &[String], policy: &GitPolicy, out: &mut Vec<Violation>) {
    let plain: Vec<&str> = args.iter().map(String::as_str).collect();
    if plain.len() < 2 || plain[0] != "pr" || plain[1] != "create" {
        return;
    }
    let Some(body) = flag_value(&plain[2..], &["--body", "-b"]) else {
        return;
    };
    if policy.ai_attribution.is_active() {
        if let Some(which) = standards::find_attribution(body) {
            out.push(Violation::new(
                "git.ai_attribution",
                policy.ai_attribution,
                format!("PR body contains AI attribution ({which})"),
                "remove the attribution — project policy forbids AI attribution in commits and PR bodies (charter §6.4)".to_string(),
            ));
        }
    }
    if policy.commit_emoji.is_active() {
        if let Some(c) = standards::find_emoji(body) {
            out.push(Violation::new(
                "git.commit_emoji",
                policy.commit_emoji,
                format!("PR body contains emoji ('{c}')"),
                "remove emoji from the PR body (charter §6.4)".to_string(),
            ));
        }
    }
}

// ---------------------------------------------------------------------------
// command parsing helpers
// ---------------------------------------------------------------------------

/// Tokenize a shell command segment, honoring single/double quotes and
/// backslash escapes (outside single quotes).
#[must_use]
pub fn shell_tokens(segment: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut cur = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut started = false;
    let mut chars = segment.chars();

    while let Some(c) = chars.next() {
        match c {
            '\'' if !in_double => {
                in_single = !in_single;
                started = true;
            }
            '"' if !in_single => {
                in_double = !in_double;
                started = true;
            }
            '\\' if !in_single => {
                if let Some(next) = chars.next() {
                    cur.push(next);
                    started = true;
                }
            }
            c if c.is_whitespace() && !in_single && !in_double => {
                if started {
                    tokens.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            c => {
                cur.push(c);
                started = true;
            }
        }
    }
    if started {
        tokens.push(cur);
    }
    tokens
}

/// Skip leading `VAR=value` assignments; return `(program, args)`.
fn strip_env_assignments(tokens: &[String]) -> Option<(&str, &[String])> {
    let mut idx = 0;
    while idx < tokens.len() {
        let t = &tokens[idx];
        let is_assignment = t
            .split_once('=')
            .is_some_and(|(name, _)| !name.is_empty() && is_identifier(name));
        if is_assignment {
            idx += 1;
        } else {
            break;
        }
    }
    let program = tokens.get(idx)?;
    Some((program.as_str(), &tokens[idx + 1..]))
}

fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Global git flags that consume the following token.
const GIT_GLOBAL_VALUE_FLAGS: &[&str] = &["-C", "-c", "--git-dir", "--work-tree", "--namespace"];

/// Find the git subcommand, skipping global flags (`git -C path commit …`).
fn git_subcommand(args: &[String]) -> Option<(&str, &[String])> {
    let mut idx = 0;
    while idx < args.len() {
        let t = &args[idx];
        if GIT_GLOBAL_VALUE_FLAGS.contains(&t.as_str()) {
            idx += 2;
        } else if t.starts_with('-') {
            idx += 1;
        } else {
            return Some((t.as_str(), &args[idx + 1..]));
        }
    }
    None
}

/// The branch a `git checkout`/`git switch` lands on, when determinable.
fn checkout_target(rest: &[String]) -> Option<String> {
    let mut iter = rest.iter().peekable();
    while let Some(t) = iter.next() {
        match t.as_str() {
            "-b" | "-B" | "-c" | "-C" => return iter.next().cloned(),
            "--detach" | "--" => return None,
            s if s.starts_with('-') => {}
            s => return Some(s.to_string()),
        }
    }
    None
}

/// Parsed `git push` intent.
struct PushIntent {
    force: bool,
    /// Remote branch names being updated.
    updates: Vec<String>,
    /// Remote branch names being deleted.
    deletions: Vec<String>,
}

/// Push flags that consume the following token.
const PUSH_VALUE_FLAGS: &[&str] = &["-o", "--push-option", "--receive-pack", "--exec", "--repo"];

fn parse_push(rest: &[String], current_branch: &str) -> PushIntent {
    let mut force = false;
    let mut delete_mode = false;
    let mut positional: Vec<&str> = Vec::new();

    let mut idx = 0;
    while idx < rest.len() {
        let t = rest[idx].as_str();
        if PUSH_VALUE_FLAGS.contains(&t) {
            idx += 2;
            continue;
        }
        if t == "--force" || t.starts_with("--force-with-lease") {
            force = true;
        } else if t == "--delete" {
            delete_mode = true;
        } else if t.starts_with("--") {
            // other long flag, no value consumed
        } else if let Some(cluster) = t.strip_prefix('-') {
            if !cluster.is_empty() && cluster.chars().all(char::is_alphanumeric) {
                if cluster.contains('f') {
                    force = true;
                }
                if cluster.contains('d') {
                    delete_mode = true;
                }
            }
        } else {
            positional.push(t);
        }
        idx += 1;
    }

    let mut updates = Vec::new();
    let mut deletions = Vec::new();

    // positional[0] is the remote; the rest are refspecs.
    let refspecs = if positional.len() > 1 {
        &positional[1..]
    } else {
        &[][..]
    };

    if refspecs.is_empty() {
        // `git push` / `git push origin`: updates the current branch.
        if !current_branch.is_empty() {
            let list = if delete_mode {
                &mut deletions
            } else {
                &mut updates
            };
            list.push(current_branch.to_string());
        }
    } else {
        for spec in refspecs {
            let spec = spec.strip_prefix('+').map_or(*spec, |s| {
                force = true;
                s
            });
            if delete_mode {
                deletions.push(normalize_ref(spec, current_branch));
                continue;
            }
            if let Some((src, dst)) = spec.split_once(':') {
                if src.is_empty() {
                    deletions.push(normalize_ref(dst, current_branch));
                } else {
                    updates.push(normalize_ref(dst, current_branch));
                }
            } else {
                updates.push(normalize_ref(spec, current_branch));
            }
        }
    }

    PushIntent {
        force,
        updates,
        deletions,
    }
}

/// Strip `refs/heads/`, resolve `HEAD` to the current branch.
fn normalize_ref(name: &str, current_branch: &str) -> String {
    let name = name.strip_prefix("refs/heads/").unwrap_or(name);
    if name == "HEAD" {
        current_branch.to_string()
    } else {
        name.to_string()
    }
}

/// Extract the value of a flag (`--body x`, `--body=x`, `-b x`).
fn flag_value<'a>(args: &'a [&'a str], names: &[&str]) -> Option<&'a str> {
    let mut idx = 0;
    while idx < args.len() {
        let t = args[idx];
        for n in names {
            if t == *n {
                return args.get(idx + 1).copied();
            }
            if let Some(v) = t.strip_prefix(&format!("{n}=")) {
                return Some(v);
            }
        }
        idx += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::super::policy::PolicyLevel;
    use super::*;

    fn ctx<'a>(policy: &'a GitPolicy, branch: &'a str) -> GuardContext<'a> {
        GuardContext {
            policy,
            current_branch: branch,
            integrate_token: false,
        }
    }

    fn default_policy() -> GitPolicy {
        GitPolicy::default()
    }

    fn release_policy() -> GitPolicy {
        GitPolicy {
            protected_branches: vec!["main".into(), "master".into(), "release/*".into()],
            ..GitPolicy::default()
        }
    }

    // -- payload parsing --

    #[test]
    fn test_payload_parse_bash() {
        let json = r#"{"tool_name":"Bash","tool_input":{"command":"git status"},"cwd":"/x"}"#;
        let p = HookPayload::parse(json).unwrap();
        assert_eq!(p.bash_command(), Some("git status"));
        assert_eq!(p.cwd.as_deref(), Some(std::path::Path::new("/x")));
    }

    #[test]
    fn test_payload_non_bash_tool_ignored() {
        let json = r#"{"tool_name":"Write","tool_input":{"file_path":"a"}}"#;
        let p = HookPayload::parse(json).unwrap();
        assert_eq!(p.bash_command(), None);
    }

    #[test]
    fn test_payload_malformed_json_is_error() {
        assert!(HookPayload::parse("{ nope").is_err());
    }

    // -- commit on protected --

    #[test]
    fn test_commit_on_protected_blocked() {
        let p = default_policy();
        let v = evaluate("git commit -m 'feat: x'", &ctx(&p, "main"));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "git.commit_to_protected");
        assert_eq!(v[0].level, PolicyLevel::Block);
        assert!(v[0].remedy.contains("codeflow integrate"));
    }

    #[test]
    fn test_commit_on_feature_allowed() {
        let p = default_policy();
        assert!(evaluate("git commit -m 'feat: x'", &ctx(&p, "feat/x")).is_empty());
    }

    #[test]
    fn test_commit_with_integrate_token_allowed() {
        let p = default_policy();
        let c = GuardContext {
            policy: &p,
            current_branch: "main",
            integrate_token: true,
        };
        assert!(evaluate("git commit -m 'feat: x'", &c).is_empty());
    }

    #[test]
    fn test_merge_on_protected_blocked() {
        // Charter AC #5: raw `git merge` into main blocked.
        let p = default_policy();
        let v = evaluate("git merge feat/x", &ctx(&p, "main"));
        assert_eq!(v[0].rule, "git.commit_to_protected");
    }

    #[test]
    fn test_merge_on_feature_allowed() {
        let p = default_policy();
        assert!(evaluate("git merge main", &ctx(&p, "feat/x")).is_empty());
    }

    #[test]
    fn test_checkout_dodge_detected() {
        let p = default_policy();
        let v = evaluate("git checkout main && git merge feat/x", &ctx(&p, "feat/x"));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "git.commit_to_protected");
    }

    #[test]
    fn test_switch_back_to_feature_not_flagged() {
        let p = default_policy();
        let v = evaluate(
            "git switch feat/y && git commit -m 'fix: y'",
            &ctx(&p, "main"),
        );
        assert!(v.is_empty());
    }

    #[test]
    fn test_checkout_new_branch_from_main_allowed() {
        let p = default_policy();
        let v = evaluate(
            "git checkout -b feat/z && git commit -m 'feat: z'",
            &ctx(&p, "main"),
        );
        assert!(v.is_empty());
    }

    #[test]
    fn test_commit_warn_level_warns() {
        let p = GitPolicy {
            commit_to_protected: PolicyLevel::Warn,
            ..default_policy()
        };
        let v = evaluate("git commit -m 'feat: x'", &ctx(&p, "main"));
        assert_eq!(v[0].level, PolicyLevel::Warn);
    }

    #[test]
    fn test_commit_check_off_silent() {
        let p = GitPolicy {
            commit_to_protected: PolicyLevel::Off,
            ..default_policy()
        };
        assert!(evaluate("git commit -m 'feat: x'", &ctx(&p, "main")).is_empty());
    }

    // -- push / force-push / delete --

    #[test]
    fn test_push_to_protected_explicit_refspec_blocked() {
        let p = default_policy();
        let v = evaluate("git push origin main", &ctx(&p, "feat/x"));
        assert_eq!(v[0].rule, "git.push_to_protected");
        assert!(v[0].message.contains("'main'"));
    }

    #[test]
    fn test_push_head_to_protected_blocked() {
        let p = default_policy();
        let v = evaluate("git push origin HEAD:main", &ctx(&p, "feat/x"));
        assert_eq!(v[0].rule, "git.push_to_protected");
    }

    #[test]
    fn test_bare_push_on_protected_blocked() {
        let p = default_policy();
        let v = evaluate("git push", &ctx(&p, "main"));
        assert_eq!(v[0].rule, "git.push_to_protected");
    }

    #[test]
    fn test_push_feature_branch_allowed() {
        let p = default_policy();
        assert!(evaluate("git push origin feat/x", &ctx(&p, "feat/x")).is_empty());
        assert!(evaluate("git push -u origin feat/x", &ctx(&p, "feat/x")).is_empty());
    }

    #[test]
    fn test_force_push_feature_branch_allowed() {
        // AC #4 / D8: force-push to a feature branch is normal.
        let p = default_policy();
        assert!(evaluate("git push --force origin feat/x", &ctx(&p, "feat/x")).is_empty());
        assert!(evaluate("git push -f origin feat/x", &ctx(&p, "feat/x")).is_empty());
        assert!(
            evaluate(
                "git push --force-with-lease origin feat/x",
                &ctx(&p, "feat/x")
            )
            .is_empty()
        );
    }

    #[test]
    fn test_force_push_protected_blocked() {
        let p = default_policy();
        for cmd in [
            "git push --force origin main",
            "git push -f origin main",
            "git push --force-with-lease origin main",
            "git push origin +feat/x:main",
        ] {
            let v = evaluate(cmd, &ctx(&p, "feat/x"));
            assert_eq!(v.len(), 1, "{cmd}");
            assert_eq!(v[0].rule, "git.force_push_protected", "{cmd}");
        }
    }

    #[test]
    fn test_delete_protected_via_push_blocked() {
        let p = default_policy();
        for cmd in [
            "git push origin --delete main",
            "git push origin :main",
            "git push -d origin main",
        ] {
            let v = evaluate(cmd, &ctx(&p, "feat/x"));
            assert_eq!(v.len(), 1, "{cmd}");
            assert_eq!(v[0].rule, "git.delete_protected", "{cmd}");
        }
    }

    #[test]
    fn test_delete_feature_branch_allowed() {
        let p = default_policy();
        assert!(evaluate("git push origin --delete feat/x", &ctx(&p, "feat/x")).is_empty());
        assert!(evaluate("git branch -D feat/x", &ctx(&p, "feat/x")).is_empty());
    }

    #[test]
    fn test_delete_protected_local_branch_blocked() {
        let p = default_policy();
        let v = evaluate("git branch -D main", &ctx(&p, "feat/x"));
        assert_eq!(v[0].rule, "git.delete_protected");
    }

    // -- hard reset / rebase --

    #[test]
    fn test_hard_reset_on_protected_blocked() {
        let p = default_policy();
        let v = evaluate("git reset --hard HEAD~3", &ctx(&p, "main"));
        assert_eq!(v[0].rule, "git.hard_reset_protected");
    }

    #[test]
    fn test_hard_reset_on_feature_allowed() {
        let p = default_policy();
        assert!(evaluate("git reset --hard HEAD~3", &ctx(&p, "feat/x")).is_empty());
    }

    #[test]
    fn test_soft_reset_on_protected_allowed() {
        let p = default_policy();
        assert!(evaluate("git reset --soft HEAD~1", &ctx(&p, "main")).is_empty());
    }

    #[test]
    fn test_rebase_on_protected_blocked() {
        let p = default_policy();
        let v = evaluate("git rebase feat/x", &ctx(&p, "main"));
        assert_eq!(v[0].rule, "git.hard_reset_protected");
    }

    // -- glob extension (AC #3) --

    #[test]
    fn test_release_glob_honored_by_guard() {
        let p = release_policy();
        let v = evaluate("git commit -m 'fix: x'", &ctx(&p, "release/2.0"));
        assert_eq!(v[0].rule, "git.commit_to_protected");
        let v = evaluate("git push origin release/2.0", &ctx(&p, "feat/x"));
        assert_eq!(v[0].rule, "git.push_to_protected");
        let v = evaluate("git push -f origin release/2.0", &ctx(&p, "feat/x"));
        assert_eq!(v[0].rule, "git.force_push_protected");
        assert!(evaluate("git commit -m 'fix: x'", &ctx(&p, "feat/x")).is_empty());
    }

    // -- gh pr create body scan (AC #13) --

    #[test]
    fn test_pr_body_attribution_blocked() {
        let p = default_policy();
        let cmd = "gh pr create --title 'feat: x' --body 'Done.\n\nGenerated with Claude Code'";
        let v = evaluate(cmd, &ctx(&p, "feat/x"));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "git.ai_attribution");
    }

    #[test]
    fn test_pr_body_coauthored_blocked() {
        let p = default_policy();
        let cmd = r#"gh pr create -t "feat: x" -b "ok Co-Authored-By: Claude <noreply@anthropic.com>""#;
        let v = evaluate(cmd, &ctx(&p, "feat/x"));
        assert_eq!(v[0].rule, "git.ai_attribution");
    }

    #[test]
    fn test_pr_body_emoji_blocked() {
        let p = default_policy();
        let cmd = "gh pr create --title 'feat: x' --body 'ship it \u{1F680}'";
        let v = evaluate(cmd, &ctx(&p, "feat/x"));
        assert_eq!(v[0].rule, "git.commit_emoji");
    }

    #[test]
    fn test_pr_body_clean_allowed() {
        let p = default_policy();
        let cmd = "gh pr create --title 'feat: x' --body 'Summary: adds the hook plane.'";
        assert!(evaluate(cmd, &ctx(&p, "feat/x")).is_empty());
    }

    #[test]
    fn test_pr_body_scan_respects_policy_off() {
        let p = GitPolicy {
            ai_attribution: PolicyLevel::Off,
            commit_emoji: PolicyLevel::Off,
            ..default_policy()
        };
        let cmd = "gh pr create --body 'Generated with Bot \u{1F916}'";
        assert!(evaluate(cmd, &ctx(&p, "feat/x")).is_empty());
    }

    // -- safe commands --

    #[test]
    fn test_everyday_commands_untouched() {
        let p = default_policy();
        for cmd in [
            "git status",
            "git log --oneline -5",
            "git add -A",
            "git diff main...HEAD",
            "cargo test -p codeflow-core",
            "ls -la",
            "git fetch origin",
            "gh pr view 12",
        ] {
            assert!(evaluate(cmd, &ctx(&p, "main")).is_empty(), "{cmd}");
        }
    }

    #[test]
    fn test_env_prefix_does_not_hide_git() {
        let p = default_policy();
        let v = evaluate("FOO=1 git commit -m 'feat: x'", &ctx(&p, "main"));
        assert_eq!(v[0].rule, "git.commit_to_protected");
    }

    #[test]
    fn test_git_with_global_flags() {
        let p = default_policy();
        let v = evaluate("git -C /repo commit -m 'feat: x'", &ctx(&p, "main"));
        assert_eq!(v[0].rule, "git.commit_to_protected");
    }

    // -- tokenizer --

    #[test]
    fn test_shell_tokens_quotes() {
        assert_eq!(
            shell_tokens(r#"git commit -m "feat: a b" -n"#),
            vec!["git", "commit", "-m", "feat: a b", "-n"]
        );
        assert_eq!(
            shell_tokens("echo 'single quoted arg'"),
            vec!["echo", "single quoted arg"]
        );
    }

    #[test]
    fn test_shell_tokens_escapes() {
        assert_eq!(shell_tokens(r"echo a\ b"), vec!["echo", "a b"]);
    }
}
