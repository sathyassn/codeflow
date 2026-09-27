//! `exec-guard` — the `PreToolUse` shell security hook (ADR-0008).
//!
//! A thin adapter over two existing security modules — [`DangerousModule`] and
//! [`PrivilegeModule`] — that runs them against a Bash command. Catastrophic
//! commands are a non-relaxable block; privilege escalation maps to the level
//! configured in `policy.json`'s `security` section. It sits alongside
//! `git-guard` on the `PreToolUse` (Bash/PowerShell) event and
//! shares its lenient payload contract, so the same binary serves both Claude
//! and the Codex hooks engine (ADR-0008).
//!
//! ## Why these two modules, and why block vs warn
//!
//! The owner posture is autonomy by default: hard protections exist only for
//! (a) credential/secret reads, (b) truly destructive commands, (c) protected
//! refs. This guard owns (b), and only advises on privilege escalation:
//!
//! - **`dangerous_commands` = block.** `rm -rf` on `/`, `~`, or a system dir;
//!   `dd` to a block device; `mkfs`; fork bombs; recursive chmod/chown on system
//!   paths. None of these is ever a legitimate project operation, so there is no
//!   sanctioned path to offer — a hard block is the whole point.
//! - **`privilege_escalation` = warn, deliberately NOT block.** sudo/su/doas/
//!   pkexec, shell `-c` chains, `LD_PRELOAD`/PATH injection. A hook that exited 2
//!   here would override even an explicit human approval, because a `PreToolUse`
//!   deny wins unconditionally. The guard therefore advises on stderr; the agent
//!   obtains applicable task authority in the authenticated conversation and
//!   observes effective harness controls. Some production modes show no
//!   permission prompt. Set `block` in `policy.json` to harden a specific repo;
//!   authorization never relaxes the catastrophic floor above.
//!
//! These two are the only scanner modules; the unwired v1 modules were
//! removed (TSK-137). The live git protections are `hooks/git_guard.rs`.

use crate::security::dangerous::DangerousModule;
use crate::security::privilege::PrivilegeModule;
use crate::security::{CheckContext, SecurityModule, SecurityPolicy, Verdict};

use super::policy::{PolicyLevel, SecuritySection};
use super::Violation;

/// Evaluate a Bash command against the `security` policy section.
///
/// Returns all violations found (at most one per module); the caller maps
/// block-level violations to exit 2 (deny) and warn-level to stderr advice.
/// Privilege checking is skipped when its level is `off`; catastrophic-command
/// checking is a non-relaxable floor.
#[must_use]
pub fn evaluate(command: &str, levels: &SecuritySection) -> Vec<Violation> {
    // The dangerous/privilege modules read only `command`; branch, sandbox
    // bypass, and the protected-path policy are irrelevant to them, so default
    // values suffice.
    let policy = SecurityPolicy::defaults();
    let ctx = CheckContext {
        command,
        sandbox_bypass: false,
        current_branch: "",
        policy: &policy,
    };

    let mut violations = Vec::new();

    // A consuming repository cannot turn the catastrophic floor off or
    // downgrade it to advice. The serialized key stays explicit for policy
    // compatibility, but a stale or hand-edited weaker value is not authority.
    if let Some(verdict) = DangerousModule.check(&ctx) {
        violations.push(dangerous_violation(PolicyLevel::Block, &verdict));
    }
    if levels.privilege_escalation.is_active() {
        if let Some(verdict) = PrivilegeModule.check(&ctx) {
            violations.push(privilege_violation(levels.privilege_escalation, &verdict));
        }
    }

    violations
}

fn dangerous_violation(level: PolicyLevel, verdict: &Verdict) -> Violation {
    Violation::new(
        "security.dangerous_commands",
        level,
        format!(
            "{cat}: {reason} (pattern `{pat}`)",
            cat = verdict.category,
            reason = verdict.reason,
            pat = verdict.pattern,
        ),
        "destructive commands have no sanctioned path — do not run them; if this is a false positive, scope the command away from system paths".to_string(),
    )
}

fn privilege_violation(level: PolicyLevel, verdict: &Verdict) -> Violation {
    let enforcement = if level == PolicyLevel::Block {
        "configured policy denies this action"
    } else {
        "the default warn policy advises only"
    };
    Violation::new(
        "security.privilege_escalation",
        level,
        format!(
            "{cat}: {reason} (pattern `{pat}`)",
            cat = verdict.category,
            reason = verdict.reason,
            pat = verdict.pattern,
        ),
        format!("privilege escalation needs applicable operator authority; {enforcement}; the effective harness may show no permission prompt (policy security.privilege_escalation)"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::any_blocking;

    fn levels(dangerous: PolicyLevel, privilege: PolicyLevel) -> SecuritySection {
        SecuritySection {
            dangerous_commands: dangerous,
            privilege_escalation: privilege,
        }
    }

    #[test]
    fn test_default_levels_are_block_and_warn() {
        let s = SecuritySection::default();
        assert_eq!(s.dangerous_commands, PolicyLevel::Block);
        assert_eq!(s.privilege_escalation, PolicyLevel::Warn);
    }

    #[test]
    fn test_dangerous_command_blocks() {
        let v = evaluate("rm -rf /", &SecuritySection::default());
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "security.dangerous_commands");
        assert_eq!(v[0].level, PolicyLevel::Block);
        assert!(any_blocking(&v));
    }

    #[test]
    fn test_dangerous_variants_block() {
        for cmd in [
            "rm -rf ~",
            "rm -rf /etc",
            "dd if=image of=/dev/sda",
            "mkfs.ext4 /dev/sda1",
            "chmod -R 777 /",
            ":(){ :|:& };:",
        ] {
            let v = evaluate(cmd, &SecuritySection::default());
            assert!(
                v.iter().any(|x| x.rule == "security.dangerous_commands"),
                "{cmd}: {v:?}"
            );
        }
    }

    #[test]
    fn test_privilege_warns_not_blocks_by_default() {
        // sudo produces advice, not a veto; task authority and technical harness
        // prompting are separate, and some production modes show no prompt.
        let v = evaluate("sudo apt-get install foo", &SecuritySection::default());
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "security.privilege_escalation");
        assert_eq!(v[0].level, PolicyLevel::Warn);
        assert!(
            !any_blocking(&v),
            "privilege escalation must not block by default"
        );
        assert!(v[0].remedy.contains("default warn policy advises only"));
        assert!(v[0].remedy.contains("may show no permission prompt"));
        assert!(!v[0].remedy.contains("approve it there"));
    }

    #[test]
    fn test_privilege_can_be_hardened_to_block() {
        let v = evaluate(
            "sudo rm file",
            &levels(PolicyLevel::Block, PolicyLevel::Block),
        );
        assert_eq!(v[0].rule, "security.privilege_escalation");
        assert_eq!(v[0].level, PolicyLevel::Block);
        assert!(any_blocking(&v));
        assert!(v[0].remedy.contains("configured policy denies"));
        assert!(!v[0].remedy.contains("advises only"));
    }

    #[test]
    fn test_catastrophic_floor_cannot_be_disabled() {
        let off = levels(PolicyLevel::Off, PolicyLevel::Off);
        let v = evaluate("rm -rf /", &off);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "security.dangerous_commands");
        assert_eq!(v[0].level, PolicyLevel::Block);
        assert!(any_blocking(&v));
        assert!(evaluate("sudo rm file", &off).is_empty());
    }

    #[test]
    fn test_catastrophic_floor_and_privilege_warning_both_apply() {
        let s = levels(PolicyLevel::Off, PolicyLevel::Warn);
        let v = evaluate("sudo dd if=image of=/dev/sda", &s);
        assert_eq!(v.len(), 2);
        assert!(v
            .iter()
            .any(|x| { x.rule == "security.dangerous_commands" && x.level == PolicyLevel::Block }));
        assert!(v.iter().any(|x| {
            x.rule == "security.privilege_escalation" && x.level == PolicyLevel::Warn
        }));
    }

    #[test]
    fn test_clean_commands_untouched() {
        for cmd in [
            "cargo test --workspace",
            "ls -la",
            "git status",
            "rm -rf target/debug",
            "npm run build",
            "chmod +x ./script.sh",
        ] {
            assert!(
                evaluate(cmd, &SecuritySection::default()).is_empty(),
                "{cmd}"
            );
        }
    }
}
