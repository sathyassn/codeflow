//! `exec-guard` — the `PreToolUse` shell security hook (ADR-0008).
//!
//! A thin adapter over two existing security modules — [`DangerousModule`] and
//! [`PrivilegeModule`] — that runs them against a Bash command, plus the
//! headless peer run check (`claude -p`, `codex exec`, `grok -p`; TSK-136) at
//! the level `security.headless_peer_runs` sets, `block` by default. Catastrophic
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
//! - **`privilege_escalation` = block** (operator decision D5, ADR-0075,
//!   amending ADR-0008's warn). sudo/su/doas/pkexec, shell `-c` chains,
//!   `LD_PRELOAD`/PATH injection. Agent sessions no longer prompt for these:
//!   the presets deny the plain forms and this guard refuses the rest, and
//!   the operator runs privileged commands personally. A project may set
//!   `warn` in `policy.json`; `codeflow update` keeps a value that differs
//!   from the shipped default. Authorization never relaxes the catastrophic
//!   floor above.
//!
//! These two are the only scanner modules; the unwired v1 modules were
//! removed (TSK-137). The live git protections are `hooks/git_guard.rs`.

use crate::security::dangerous::DangerousModule;
use crate::security::headless::{headless_peer_run, HeadlessRun};
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
        violations.push(dangerous_violation(&verdict));
    }
    if levels.privilege_escalation.is_active() {
        if let Some(verdict) = PrivilegeModule.check(&ctx) {
            violations.push(privilege_violation(levels.privilege_escalation, &verdict));
        }
    }
    if levels.headless_peer_runs.is_active() {
        if let Some(run) = headless_peer_run(command) {
            violations.push(headless_violation(levels.headless_peer_runs, &run));
        }
    }

    violations
}

fn headless_violation(level: PolicyLevel, run: &HeadlessRun) -> Violation {
    let enforcement = if level == PolicyLevel::Block {
        "configured policy refuses it"
    } else {
        "the configured warn level lets a script outside a delegation run"
    };
    Violation::new(
        "security.headless_peer_runs",
        level,
        format!(
            "headless peer run `{}`{}: peer seats run interactively, so a headless run has no \
             verified native session, task tools or recheckable thread",
            run.form,
            if run.parsed {
                ""
            } else {
                " (the line could not be fully parsed; its text names the peer with a \
                 headless flag)"
            }
        ),
        crate::remedy::HEADLESS_PEER_RUN.with(&[("enforcement", enforcement)]),
    )
}

fn dangerous_violation(verdict: &Verdict) -> Violation {
    Violation::always_blocking(
        "security.dangerous_commands",
        format!(
            "{cat}: {reason} (pattern `{pat}`)",
            cat = verdict.category,
            reason = verdict.reason,
            pat = verdict.pattern,
        ),
        "destructive commands have no sanctioned path — do not run them; if this is a false positive, scope the command away from system paths",
    )
}

fn privilege_violation(level: PolicyLevel, verdict: &Verdict) -> Violation {
    let enforcement = if level == PolicyLevel::Block {
        "policy refuses it; the operator runs privileged commands"
    } else {
        "the configured warn level advises only"
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
        crate::remedy::PRIVILEGE_ESCALATION.with(&[("enforcement", enforcement)]),
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
            headless_peer_runs: PolicyLevel::Warn,
            ..SecuritySection::default()
        }
    }

    #[test]
    fn test_headless_peer_run_blocks_by_default_and_warns_when_set() {
        let v = evaluate("codex exec 'fix it'", &SecuritySection::default());
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "security.headless_peer_runs");
        assert_eq!(v[0].level, PolicyLevel::Block);
        assert!(any_blocking(&v));
        assert!(v[0].message.contains("`codex exec`"));
        assert!(v[0].remedy.contains("codeflow delegate"));

        let warn = SecuritySection {
            headless_peer_runs: PolicyLevel::Warn,
            ..SecuritySection::default()
        };
        let v = evaluate("claude -p 'review'", &warn);
        assert_eq!(v[0].level, PolicyLevel::Warn);
        assert!(!any_blocking(&v));
        let block = SecuritySection::default();

        let off = SecuritySection {
            headless_peer_runs: PolicyLevel::Off,
            ..SecuritySection::default()
        };
        assert!(evaluate("grok -p x", &off).is_empty());
        assert!(evaluate("codex --version", &block).is_empty());
    }

    #[test]
    fn test_default_levels_block() {
        let s = SecuritySection::default();
        assert_eq!(s.dangerous_commands, PolicyLevel::Block);
        assert_eq!(s.privilege_escalation, PolicyLevel::Block);
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
    fn test_privilege_blocks_by_default_and_can_be_set_to_warn() {
        // D5 (ADR-0075): the operator runs privileged commands; a project
        // may still set warn, which only advises.
        let v = evaluate("sudo apt-get install foo", &SecuritySection::default());
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "security.privilege_escalation");
        assert_eq!(v[0].level, PolicyLevel::Block);
        assert!(any_blocking(&v));
        assert!(v[0]
            .remedy
            .contains("the operator runs privileged commands"));

        let v = evaluate(
            "sudo apt-get install foo",
            &levels(PolicyLevel::Block, PolicyLevel::Warn),
        );
        assert_eq!(v[0].level, PolicyLevel::Warn);
        assert!(!any_blocking(&v));
        assert!(v[0].remedy.contains("configured warn level advises only"));
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
        assert!(v[0].remedy.contains("policy refuses it"));
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
