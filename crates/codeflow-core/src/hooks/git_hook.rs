//! Git client hook stages (charter §6.1 plane 1): pre-commit, commit-msg,
//! pre-push. Harness-agnostic — they work for any agent or human.
//!
//! The shims in `.git/hooks/` exec `codeflow git-hook <stage>`; ALL levels
//! and lists come from `.codeflow/policy.json` (D7/D8 — nothing hardcoded).

use std::path::Path;

use git2::Repository;

use crate::error::HookError;
use crate::testing::gate::{run_gate, GateOutcome};

use super::policy::GitPolicy;
use super::repo::current_branch;
use super::scan;
use super::{standards, Violation};

/// Outcome of running one hook stage.
#[derive(Debug, Default)]
pub struct StageReport {
    /// Policy violations (block and warn level).
    pub violations: Vec<Violation>,
    /// Non-violation diagnostics (skipped gates, degraded checks) — printed
    /// so degradation stays legible (charter principle 8).
    pub notes: Vec<String>,
}

const SANCTIONED: &str =
    "land work via PR (gh pr create → merge on green CI) or `codeflow integrate <branch> --into <target>`";

// ---------------------------------------------------------------------------
// pre-commit
// ---------------------------------------------------------------------------

/// The pre-commit stage: protected-branch commit check, staged-.env block,
/// and the secret content scan over the staged diff (charter §6.1, §6.3).
///
/// # Errors
///
/// Returns [`HookError::Config`] when `root` is not inside a git repository.
pub fn pre_commit(
    root: &Path,
    policy: &GitPolicy,
    integrate_token: bool,
) -> Result<StageReport, HookError> {
    let repo = Repository::discover(root)
        .map_err(|e| HookError::Config(format!("not a git repository: {e}")))?;
    let mut report = StageReport::default();

    let branch = current_branch(&repo);
    if policy.commit_to_protected.is_active()
        && policy.branch_is_protected(&branch)
        && !integrate_token
    {
        report.violations.push(Violation::new(
            "git.commit_to_protected",
            policy.commit_to_protected,
            format!("commit on protected branch '{branch}'"),
            SANCTIONED.to_string(),
        ));
    }

    if policy.secret_scan.is_active() {
        scan_staged(&repo, policy, &mut report);
    }

    Ok(report)
}

/// Scan the staged diff (index vs HEAD) for .env files and secret content.
fn scan_staged(repo: &Repository, policy: &GitPolicy, report: &mut StageReport) {
    let head_tree = repo.head().ok().and_then(|h| h.peel_to_tree().ok());
    let Ok(diff) = repo.diff_tree_to_index(head_tree.as_ref(), None, None) else {
        report
            .notes
            .push("secret scan skipped: could not read the staged diff".to_string());
        return;
    };

    for delta in diff.deltas() {
        let Some(path) = delta.new_file().path() else {
            continue;
        };
        let path_str = path.to_string_lossy();
        if scan::is_env_file(&path_str) {
            report.violations.push(Violation::new(
                "git.secret_scan",
                policy.secret_scan,
                format!("dotenv file staged for commit: {path_str}"),
                "keep env files out of git (.gitignore covers them); commit a .env.example instead"
                    .to_string(),
            ));
        }
    }

    let mut hits: Vec<scan::SecretHit> = Vec::new();
    let _ = diff.foreach(
        &mut |_, _| true,
        None,
        None,
        Some(&mut |delta, _hunk, line| {
            if line.origin() == '+' {
                let content = String::from_utf8_lossy(line.content());
                if let Some(pattern) = scan::scan_line(&content) {
                    hits.push(scan::SecretHit {
                        file: delta
                            .new_file()
                            .path()
                            .map(|p| p.to_string_lossy().to_string())
                            .unwrap_or_default(),
                        line: line.new_lineno().unwrap_or(0),
                        pattern,
                    });
                }
            }
            true
        }),
    );
    for hit in hits {
        report.violations.push(Violation::new(
            "git.secret_scan",
            policy.secret_scan,
            format!(
                "possible secret ({}) staged at {}:{}",
                hit.pattern, hit.file, hit.line
            ),
            "remove the secret from the staged content; rotate it if it was ever committed"
                .to_string(),
        ));
    }
}

// ---------------------------------------------------------------------------
// commit-msg
// ---------------------------------------------------------------------------

/// The commit-msg stage: conventional format (whitelisted types), the
/// no-AI-attribution rule, and the no-emoji rule (charter §6.4, AC #13).
#[must_use]
pub fn commit_msg(policy: &GitPolicy, message: &str) -> StageReport {
    let mut report = StageReport::default();
    let cleaned = strip_commit_comments(message);
    let Some(subject) = cleaned.lines().find(|l| !l.trim().is_empty()) else {
        return report; // git rejects empty messages itself
    };

    if policy.commit_format.is_active() {
        if let Some(reason) = standards::check_commit_format(subject, &policy.commit_types) {
            report.violations.push(Violation::new(
                "git.commit_format",
                policy.commit_format,
                reason,
                format!(
                    "use `type(scope): description` with type one of: {}",
                    policy.commit_types.join(", ")
                ),
            ));
        }
    }

    if policy.ai_attribution.is_active() {
        if let Some(which) = standards::find_attribution(&cleaned) {
            report.violations.push(Violation::new(
                "git.ai_attribution",
                policy.ai_attribution,
                format!("commit message contains AI attribution ({which})"),
                "remove it — project policy forbids AI attribution in commits and PR bodies (charter §6.4)"
                    .to_string(),
            ));
        }
    }

    if policy.commit_emoji.is_active() {
        if let Some(c) = standards::find_emoji(subject) {
            report.violations.push(Violation::new(
                "git.commit_emoji",
                policy.commit_emoji,
                format!("commit subject contains emoji ('{c}')"),
                "remove emoji from the commit subject (charter §6.4)".to_string(),
            ));
        }
    }

    report
}

/// Drop the verbose-commit scissors section and `#` comment lines.
fn strip_commit_comments(message: &str) -> String {
    let mut out = Vec::new();
    for line in message.lines() {
        if line.starts_with('#') {
            if line.contains(">8") {
                break; // scissors: everything below is the diff preview
            }
            continue;
        }
        out.push(line);
    }
    out.join("\n")
}

// ---------------------------------------------------------------------------
// pre-push
// ---------------------------------------------------------------------------

/// One `<local ref> <local sha> <remote ref> <remote sha>` line from the
/// pre-push stdin contract.
#[derive(Debug, Clone)]
pub struct PushRef {
    pub local_ref: String,
    pub local_sha: String,
    pub remote_ref: String,
    pub remote_sha: String,
}

impl PushRef {
    /// `true` when this ref update deletes the remote branch.
    #[must_use]
    pub fn is_delete(&self) -> bool {
        self.local_ref == "(delete)" || is_zero_sha(&self.local_sha)
    }

    /// Remote branch name, when this updates a branch (not a tag).
    #[must_use]
    pub fn remote_branch(&self) -> Option<&str> {
        self.remote_ref.strip_prefix("refs/heads/")
    }
}

fn is_zero_sha(sha: &str) -> bool {
    !sha.is_empty() && sha.chars().all(|c| c == '0')
}

/// Parse the pre-push stdin lines.
#[must_use]
pub fn parse_push_refs(input: &str) -> Vec<PushRef> {
    input
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            Some(PushRef {
                local_ref: parts.next()?.to_string(),
                local_sha: parts.next()?.to_string(),
                remote_ref: parts.next()?.to_string(),
                remote_sha: parts.next()?.to_string(),
            })
        })
        .collect()
}

/// The pre-push stage: protected-branch push/delete/force checks, branch
/// naming against `branch_prefixes`, and the optional quick test gate.
///
/// # Errors
///
/// Returns [`HookError::Config`] when `root` is not inside a git repository.
pub fn pre_push(
    root: &Path,
    policy: &GitPolicy,
    refs: &[PushRef],
    integrate_token: bool,
) -> Result<StageReport, HookError> {
    let repo = Repository::discover(root)
        .map_err(|e| HookError::Config(format!("not a git repository: {e}")))?;
    let mut report = StageReport::default();

    for r in refs {
        let Some(branch) = r.remote_branch() else {
            continue; // tags and other refs are out of scope
        };
        let protected = policy.branch_is_protected(branch);

        if r.is_delete() {
            if protected && policy.delete_protected.is_active() {
                report.violations.push(Violation::new(
                    "git.delete_protected",
                    policy.delete_protected,
                    format!("push would delete protected branch '{branch}'"),
                    "protected branches are never deleted remotely; adjust git.protected_branches first if truly intended".to_string(),
                ));
            }
            continue;
        }

        if protected && policy.push_to_protected.is_active() && !integrate_token {
            report.violations.push(Violation::new(
                "git.push_to_protected",
                policy.push_to_protected,
                format!("direct push to protected branch '{branch}'"),
                SANCTIONED.to_string(),
            ));
        }

        if is_force_update(&repo, r) {
            if protected {
                if policy.force_push_protected.is_active() {
                    report.violations.push(Violation::new(
                        "git.force_push_protected",
                        policy.force_push_protected,
                        format!("non-fast-forward (force) push to protected branch '{branch}'"),
                        SANCTIONED.to_string(),
                    ));
                }
            } else if policy.force_push_unprotected.is_active() {
                report.violations.push(Violation::new(
                    "git.force_push_unprotected",
                    policy.force_push_unprotected,
                    format!("non-fast-forward (force) push to branch '{branch}'"),
                    "policy git.force_push_unprotected restricts force-pushes in this repo"
                        .to_string(),
                ));
            }
        }

        if !protected && policy.branch_naming.is_active() && !policy.branch_name_ok(branch) {
            report.violations.push(Violation::new(
                "git.branch_naming",
                policy.branch_naming,
                format!(
                    "branch '{branch}' does not match `{{prefix}}/{{kebab-name}}`"
                ),
                format!("rename with a sanctioned prefix: {}", policy.branch_prefixes.join(" ")),
            ));
        }
    }

    let pushes_branches = refs.iter().any(|r| r.remote_branch().is_some() && !r.is_delete());
    if pushes_branches && policy.test_gate_on_push.is_active() {
        run_test_gate(root, policy, &mut report);
    }

    Ok(report)
}

/// `true` when the remote ref exists and the local sha does not descend from
/// it (a history rewrite). Unknown objects (e.g. shallow clones) skip the
/// check rather than guessing.
fn is_force_update(repo: &Repository, r: &PushRef) -> bool {
    if is_zero_sha(&r.remote_sha) || r.remote_sha.is_empty() {
        return false; // new branch on the remote
    }
    let (Ok(local), Ok(remote)) = (
        git2::Oid::from_str(&r.local_sha),
        git2::Oid::from_str(&r.remote_sha),
    ) else {
        return false;
    };
    if local == remote {
        return false;
    }
    match repo.graph_descendant_of(local, remote) {
        Ok(descends) => !descends,
        Err(_) => false, // remote sha unknown locally — cannot judge
    }
}

/// Run the quick test gate (charter §6.1 `test_gate_on_push`), honoring the
/// absence of a configured stack gracefully (AC #7: no stack = loud no-op).
fn run_test_gate(root: &Path, policy: &GitPolicy, report: &mut StageReport) {
    let cfg_path = root.join(".codeflow").join("test-config.json");
    if !cfg_path.exists() {
        report.notes.push(
            "test gate skipped: no .codeflow/test-config.json (run /cf-stack to add a stack, \
             or create .codeflow/test-config.json)"
                .to_string(),
        );
        return;
    }
    // Delegate to the shared gate so `quick` resolves to the `essential` mode
    // that shipped test-configs actually define (see `gate::run_gate`).
    match run_gate(root, "quick") {
        Ok(GateOutcome::NoTargets { reason }) => {
            report.notes.push(format!("test gate skipped: {reason}"));
        }
        Ok(GateOutcome::Completed { results, passed, .. }) => {
            if passed {
                report
                    .notes
                    .push(format!("quick test gate passed ({} target(s))", results.len()));
            } else {
                let failed: Vec<String> = results
                    .iter()
                    .filter(|r| !r.passed())
                    .map(|r| r.name.clone())
                    .collect();
                report.violations.push(Violation::new(
                    "git.test_gate_on_push",
                    policy.test_gate_on_push,
                    format!("quick test gate failed for: {}", failed.join(", ")),
                    "fix the failing tests, or run `codeflow test --mode quick` to reproduce"
                        .to_string(),
                ));
            }
        }
        Err(e) => {
            report
                .notes
                .push(format!("test gate skipped: test-config.json unreadable: {e}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use super::super::policy::PolicyLevel;
    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .expect("git runs");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn init_repo(dir: &Path, branch: &str) {
        git(dir, &["init", "-b", branch]);
        git(dir, &["config", "user.email", "t@example.com"]);
        git(dir, &["config", "user.name", "t"]);
        std::fs::write(dir.join("base.txt"), "base\n").unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-m", "chore: init"]);
    }

    fn stage(dir: &Path, file: &str, content: &str) {
        let path = dir.join(file);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, content).unwrap();
        git(dir, &["add", file]);
    }

    fn release_policy() -> GitPolicy {
        GitPolicy {
            protected_branches: vec!["main".into(), "master".into(), "release/*".into()],
            ..GitPolicy::default()
        }
    }

    // -- pre-commit --

    #[test]
    fn test_pre_commit_clean_on_feature_branch() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        stage(dir.path(), "src/lib.rs", "pub fn hello() {}\n");
        let report = pre_commit(dir.path(), &GitPolicy::default(), false).unwrap();
        assert!(report.violations.is_empty(), "{:?}", report.violations);
    }

    #[test]
    fn test_pre_commit_blocks_on_protected_branch() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "main");
        stage(dir.path(), "a.txt", "hello\n");
        let report = pre_commit(dir.path(), &GitPolicy::default(), false).unwrap();
        assert_eq!(report.violations.len(), 1);
        assert_eq!(report.violations[0].rule, "git.commit_to_protected");
        assert_eq!(report.violations[0].level, PolicyLevel::Block);
    }

    #[test]
    fn test_pre_commit_integrate_token_passes_protected() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "main");
        stage(dir.path(), "a.txt", "hello\n");
        let report = pre_commit(dir.path(), &GitPolicy::default(), true).unwrap();
        assert!(report.violations.is_empty());
    }

    #[test]
    fn test_pre_commit_warn_level_on_protected() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "main");
        let policy = GitPolicy {
            commit_to_protected: PolicyLevel::Warn,
            ..GitPolicy::default()
        };
        let report = pre_commit(dir.path(), &policy, false).unwrap();
        assert_eq!(report.violations[0].level, PolicyLevel::Warn);
    }

    #[test]
    fn test_pre_commit_release_glob_extension() {
        // AC #3: release/* added to policy is honored by this plane too.
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "release/2.0");
        let report = pre_commit(dir.path(), &release_policy(), false).unwrap();
        assert_eq!(report.violations[0].rule, "git.commit_to_protected");
    }

    #[test]
    fn test_pre_commit_blocks_staged_env_file() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        stage(dir.path(), ".env", "DB_PASSWORD=hunter2hunter2\n");
        let report = pre_commit(dir.path(), &GitPolicy::default(), false).unwrap();
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.rule == "git.secret_scan" && v.message.contains(".env"))
        );
    }

    #[test]
    fn test_pre_commit_allows_env_example() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        stage(dir.path(), ".env.example", "DB_PASSWORD=\n");
        let report = pre_commit(dir.path(), &GitPolicy::default(), false).unwrap();
        assert!(report.violations.is_empty());
    }

    #[test]
    fn test_pre_commit_blocks_staged_secret_content() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        stage(
            dir.path(),
            "src/config.rs",
            "let key = \"AKIAIOSFODNN7EXAMPLF\";\n",
        );
        let report = pre_commit(dir.path(), &GitPolicy::default(), false).unwrap();
        assert_eq!(report.violations.len(), 1);
        assert_eq!(report.violations[0].rule, "git.secret_scan");
        assert!(report.violations[0].message.contains("src/config.rs"));
        assert!(report.violations[0].message.contains("AWS access key id"));
    }

    #[test]
    fn test_pre_commit_secret_scan_off_skips() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        stage(dir.path(), ".env", "X=1\n");
        let policy = GitPolicy {
            secret_scan: PolicyLevel::Off,
            ..GitPolicy::default()
        };
        let report = pre_commit(dir.path(), &policy, false).unwrap();
        assert!(report.violations.is_empty());
    }

    #[test]
    fn test_pre_commit_outside_repo_is_config_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(pre_commit(dir.path(), &GitPolicy::default(), false).is_err());
    }

    // -- commit-msg --

    #[test]
    fn test_commit_msg_valid() {
        let report = commit_msg(&GitPolicy::default(), "feat(hooks): add the guard\n");
        assert!(report.violations.is_empty());
    }

    #[test]
    fn test_commit_msg_malformed_subject_blocked() {
        let report = commit_msg(&GitPolicy::default(), "Added stuff\n");
        assert_eq!(report.violations[0].rule, "git.commit_format");
        assert_eq!(report.violations[0].level, PolicyLevel::Block);
    }

    #[test]
    fn test_commit_msg_attribution_blocked() {
        // AC #13: attribution trailer blocked at commit-msg.
        let msg = "feat: x\n\nCo-Authored-By: Claude Fable 5 <noreply@anthropic.com>\n";
        let report = commit_msg(&GitPolicy::default(), msg);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.rule == "git.ai_attribution")
        );
    }

    #[test]
    fn test_commit_msg_emoji_blocked() {
        let report = commit_msg(&GitPolicy::default(), "feat: ship \u{1F680}\n");
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.rule == "git.commit_emoji")
        );
    }

    #[test]
    fn test_commit_msg_levels_from_policy() {
        // warn → still reported, at warn; off → silent (D7: nothing hardcoded).
        let warn_policy = GitPolicy {
            commit_format: PolicyLevel::Warn,
            ..GitPolicy::default()
        };
        let report = commit_msg(&warn_policy, "Bad subject\n");
        assert_eq!(report.violations[0].level, PolicyLevel::Warn);

        let off_policy = GitPolicy {
            commit_format: PolicyLevel::Off,
            ai_attribution: PolicyLevel::Off,
            commit_emoji: PolicyLevel::Off,
            ..GitPolicy::default()
        };
        let msg = "Bad subject \u{1F680}\n\nGenerated with a robot\n";
        assert!(commit_msg(&off_policy, msg).violations.is_empty());
    }

    #[test]
    fn test_commit_msg_ignores_comment_lines() {
        let msg = "feat: x\n# Co-Authored-By: Claude <noreply@anthropic.com>\n";
        assert!(commit_msg(&GitPolicy::default(), msg).violations.is_empty());
    }

    #[test]
    fn test_commit_msg_ignores_scissors_section() {
        let msg = "feat: x\n# ------------------------ >8 ------------------------\ndiff: Generated with Claude\n";
        assert!(commit_msg(&GitPolicy::default(), msg).violations.is_empty());
    }

    #[test]
    fn test_commit_msg_merge_subject_exempt_from_format() {
        let report = commit_msg(&GitPolicy::default(), "Merge branch 'main' into feat/x\n");
        assert!(report.violations.is_empty());
    }

    // -- pre-push --

    fn pref(local_ref: &str, local: &str, remote_ref: &str, remote: &str) -> PushRef {
        PushRef {
            local_ref: local_ref.into(),
            local_sha: local.into(),
            remote_ref: remote_ref.into(),
            remote_sha: remote.into(),
        }
    }

    const ZERO: &str = "0000000000000000000000000000000000000000";

    #[test]
    fn test_parse_push_refs() {
        let input = format!("refs/heads/feat/x abc123 refs/heads/feat/x {ZERO}\n");
        let refs = parse_push_refs(&input);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].remote_branch(), Some("feat/x"));
        assert!(!refs[0].is_delete());
    }

    #[test]
    fn test_pre_push_feature_branch_allowed() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        let refs = [pref("refs/heads/feat/x", "abc1", "refs/heads/feat/x", ZERO)];
        let report = pre_push(dir.path(), &GitPolicy::default(), &refs, false).unwrap();
        assert!(report.violations.is_empty(), "{:?}", report.violations);
    }

    #[test]
    fn test_pre_push_to_protected_blocked() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "main");
        let refs = [pref("refs/heads/main", "abc1", "refs/heads/main", ZERO)];
        let report = pre_push(dir.path(), &GitPolicy::default(), &refs, false).unwrap();
        assert_eq!(report.violations[0].rule, "git.push_to_protected");
    }

    #[test]
    fn test_pre_push_integrate_token_passes() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "main");
        let refs = [pref("refs/heads/main", "abc1", "refs/heads/main", ZERO)];
        let report = pre_push(dir.path(), &GitPolicy::default(), &refs, true).unwrap();
        assert!(report.violations.is_empty());
    }

    #[test]
    fn test_pre_push_delete_protected_blocked() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        let refs = [pref("(delete)", ZERO, "refs/heads/main", "abc1")];
        let report = pre_push(dir.path(), &GitPolicy::default(), &refs, false).unwrap();
        assert_eq!(report.violations[0].rule, "git.delete_protected");
    }

    #[test]
    fn test_pre_push_delete_feature_allowed() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        let refs = [pref("(delete)", ZERO, "refs/heads/feat/old", "abc1")];
        let report = pre_push(dir.path(), &GitPolicy::default(), &refs, false).unwrap();
        assert!(report.violations.is_empty());
    }

    #[test]
    fn test_pre_push_branch_naming_blocked() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "bad-name");
        let refs = [pref(
            "refs/heads/bad-name",
            "abc1",
            "refs/heads/bad-name",
            ZERO,
        )];
        let report = pre_push(dir.path(), &GitPolicy::default(), &refs, false).unwrap();
        assert_eq!(report.violations[0].rule, "git.branch_naming");
        assert!(report.violations[0].remedy.contains("feat/"));
    }

    #[test]
    fn test_pre_push_branch_naming_off_allows() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "bad-name");
        let policy = GitPolicy {
            branch_naming: PolicyLevel::Off,
            ..GitPolicy::default()
        };
        let refs = [pref(
            "refs/heads/bad-name",
            "abc1",
            "refs/heads/bad-name",
            ZERO,
        )];
        let report = pre_push(dir.path(), &policy, &refs, false).unwrap();
        assert!(report.violations.is_empty());
    }

    #[test]
    fn test_pre_push_tags_ignored() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        let refs = [pref("refs/tags/v1.0", "abc1", "refs/tags/v1.0", ZERO)];
        let report = pre_push(dir.path(), &GitPolicy::default(), &refs, false).unwrap();
        assert!(report.violations.is_empty());
    }

    #[test]
    fn test_pre_push_release_glob_extension() {
        // AC #3: the policy glob extension is honored at pre-push too.
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        let refs = [pref(
            "refs/heads/release/2.0",
            "abc1",
            "refs/heads/release/2.0",
            ZERO,
        )];
        let report = pre_push(dir.path(), &release_policy(), &refs, false).unwrap();
        assert_eq!(report.violations[0].rule, "git.push_to_protected");
    }

    #[test]
    fn test_pre_push_force_update_detection() {
        // Build real history: main at C1; branch rewrites to C2' not
        // descending from C2.
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        let c1 = rev_parse(dir.path(), "HEAD");
        std::fs::write(dir.path().join("f.txt"), "v1\n").unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-m", "feat: v1"]);
        let c2 = rev_parse(dir.path(), "HEAD");
        // Rewrite: drop C2, add a different commit.
        git(dir.path(), &["reset", "--hard", &c1]);
        std::fs::write(dir.path().join("g.txt"), "v2\n").unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-m", "feat: v2"]);
        let c2_prime = rev_parse(dir.path(), "HEAD");

        // Force-push to a feature branch: allowed by default policy (D8).
        let refs = [pref(
            "refs/heads/feat/x",
            &c2_prime,
            "refs/heads/feat/x",
            &c2,
        )];
        let report = pre_push(dir.path(), &GitPolicy::default(), &refs, false).unwrap();
        assert!(report.violations.is_empty(), "{:?}", report.violations);

        // Same rewrite against a protected branch: blocked.
        let refs = [pref("refs/heads/main", &c2_prime, "refs/heads/main", &c2)];
        let report = pre_push(dir.path(), &GitPolicy::default(), &refs, false).unwrap();
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.rule == "git.force_push_protected"),
            "{:?}",
            report.violations
        );

        // Fast-forward (c1 → c2_prime ancestor path): no force violation.
        let refs = [pref(
            "refs/heads/feat/x",
            &c2_prime,
            "refs/heads/feat/x",
            &c1,
        )];
        let report = pre_push(dir.path(), &GitPolicy::default(), &refs, false).unwrap();
        assert!(report.violations.is_empty());
    }

    fn rev_parse(dir: &Path, what: &str) -> String {
        let out = Command::new("git")
            .args(["rev-parse", what])
            .current_dir(dir)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    // -- test gate --

    #[test]
    fn test_gate_skips_without_config() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        let refs = [pref("refs/heads/feat/x", "abc1", "refs/heads/feat/x", ZERO)];
        let report = pre_push(dir.path(), &GitPolicy::default(), &refs, false).unwrap();
        assert!(report.violations.is_empty());
        assert!(
            report.notes.iter().any(|n| n.contains("test gate skipped")),
            "absence must be loud: {:?}",
            report.notes
        );
    }

    fn write_test_config(dir: &Path, command: &str) {
        let cf = dir.join(".codeflow");
        std::fs::create_dir_all(&cf).unwrap();
        let config = format!(
            r#"{{
  "schema_version": "1.0",
  "targets": [
    {{
      "name": "demo",
      "runner": "custom",
      "enabled": true,
      "modes": {{ "quick": {{ "command": "{command}" }} }}
    }}
  ]
}}"#
        );
        std::fs::write(cf.join("test-config.json"), config).unwrap();
    }

    #[test]
    fn test_gate_failure_blocks_when_policy_blocks() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        write_test_config(dir.path(), "false");
        let policy = GitPolicy {
            test_gate_on_push: PolicyLevel::Block,
            ..GitPolicy::default()
        };
        let refs = [pref("refs/heads/feat/x", "abc1", "refs/heads/feat/x", ZERO)];
        let report = pre_push(dir.path(), &policy, &refs, false).unwrap();
        let gate: Vec<_> = report
            .violations
            .iter()
            .filter(|v| v.rule == "git.test_gate_on_push")
            .collect();
        assert_eq!(gate.len(), 1);
        assert_eq!(gate[0].level, PolicyLevel::Block);
    }

    #[test]
    fn test_gate_pass_is_quiet() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        write_test_config(dir.path(), "true");
        let refs = [pref("refs/heads/feat/x", "abc1", "refs/heads/feat/x", ZERO)];
        let report = pre_push(dir.path(), &GitPolicy::default(), &refs, false).unwrap();
        assert!(report.violations.is_empty(), "{:?}", report.violations);
    }

    #[test]
    fn test_gate_off_does_not_run() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path(), "feat/x");
        write_test_config(dir.path(), "false");
        let policy = GitPolicy {
            test_gate_on_push: PolicyLevel::Off,
            ..GitPolicy::default()
        };
        let refs = [pref("refs/heads/feat/x", "abc1", "refs/heads/feat/x", ZERO)];
        let report = pre_push(dir.path(), &policy, &refs, false).unwrap();
        assert!(report.violations.is_empty());
    }
}
