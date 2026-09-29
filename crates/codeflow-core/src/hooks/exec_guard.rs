//! Shell execution policy, backed by the shared action table (ADR-0075).
//!
//! Catastrophic commands always block. Privilege, outward actions, secret
//! reads and headless peer runs use their existing policy levels. The family
//! classifier unwraps supported launchers; interpreter literals use the same
//! rule as the action they name. Enforcement-path references use
//! `git.hook_integrity`. Opaque child programs remain outside this parser.

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
    let cwd = std::env::current_dir().unwrap_or_else(|_| "/".into());
    evaluate_at(command, levels, PolicyLevel::Block, &cwd, &cwd)
}

/// Evaluate commands and interpreter literals at the shell's actual directory.
#[must_use]
pub fn evaluate_at(
    command: &str,
    levels: &SecuritySection,
    integrity: PolicyLevel,
    cwd: &std::path::Path,
    root: &std::path::Path,
) -> Vec<Violation> {
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

    for violation in crate::security::outward::evaluate_at(command, levels, Some(cwd)) {
        let duplicate = violations.iter().any(|existing| {
            existing.rule == violation.rule
                && (existing.message == violation.message
                    || violation.rule == "security.privilege_escalation")
        });
        if !duplicate {
            violations.push(violation);
        }
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(std::path::PathBuf::from);
    for finding in crate::security::interpreter::evaluate(
        command,
        levels,
        integrity,
        cwd,
        root,
        home.as_deref(),
    ) {
        if !violations.iter().any(|v| v.rule == finding.rule) {
            violations.push(finding);
        }
    }
    violations
}

pub(crate) fn headless_violation(level: PolicyLevel, run: &HeadlessRun) -> Violation {
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

    /// A help or version invocation raises nothing at `warn` or `block`; the
    /// same tokens given as data still raise the headless rule (TSK-141 AC-3).
    #[test]
    fn a_help_invocation_passes_and_its_data_twin_is_reported_at_every_level() {
        for level in [PolicyLevel::Warn, PolicyLevel::Block] {
            let section = SecuritySection {
                headless_peer_runs: level,
                ..SecuritySection::default()
            };
            for (help, twin) in crate::security::guard_forms::HELP_PAIRS {
                assert!(evaluate(help, &section).is_empty(), "{level:?}: {help}");
                let v = evaluate(twin, &section);
                assert_eq!(v.len(), 1, "{level:?}: {twin}");
                assert_eq!(v[0].rule, "security.headless_peer_runs", "{twin}");
                assert_eq!(v[0].level, level, "{twin}");
                assert_eq!(any_blocking(&v), level == PolicyLevel::Block, "{twin}");
            }
        }
    }

    /// A peer run through a package runner is reported as its direct form
    /// at `warn` and `block`, and its help passes (TSK-141 AC-6).
    #[test]
    fn a_package_runner_peer_run_is_reported_as_its_direct_form_at_every_level() {
        for level in [PolicyLevel::Warn, PolicyLevel::Block] {
            let section = SecuritySection {
                headless_peer_runs: level,
                ..SecuritySection::default()
            };
            for (direct, runner) in crate::security::guard_forms::PACKAGE_RUNNER_PAIRS {
                let expected = evaluate(direct, &section);
                let v = evaluate(runner, &section);
                assert_eq!(v.len(), expected.len(), "{level:?}: {runner}");
                for (got, want) in v.iter().zip(&expected) {
                    assert_eq!(got.rule, want.rule, "{runner}");
                    assert_eq!(got.level, level, "{runner}");
                }
                assert_eq!(any_blocking(&v), any_blocking(&expected), "{runner}");
            }
        }
    }

    /// Each composed deletion is refused under `security.dangerous_commands`
    /// with the message its `rm -rf` equivalent gets, alone and nested; a
    /// project deletion raises nothing (TSK-141 AC-1, AC-2).
    #[test]
    fn a_composed_deletion_is_refused_as_its_rm_equivalent() {
        use crate::security::guard_forms::{COMPOSED_PAIRS, NESTINGS, PROJECT_DELETIONS};
        let section = SecuritySection::default();
        for (form, equivalent) in COMPOSED_PAIRS {
            let expected = evaluate(equivalent, &section);
            assert_eq!(expected.len(), 1, "{equivalent}");
            assert_eq!(expected[0].rule, "security.dangerous_commands");
            for nesting in NESTINGS {
                let nested = nesting.replace("{}", form);
                let v = evaluate(&nested, &section);
                let refused: Vec<_> = v
                    .iter()
                    .filter(|v| v.rule == "security.dangerous_commands")
                    .collect();
                assert_eq!(refused.len(), 1, "{nested}");
                assert!(any_blocking(&v), "{nested}");
                if *nesting == "{}" {
                    // A target reached on only some paths says so; the
                    // pattern it names is the equivalent's either way.
                    let pattern = |m: &str| m.rfind("(pattern").map(|at| m[at..].to_string());
                    if refused[0].message.contains("one of several values") {
                        assert_eq!(
                            pattern(&refused[0].message),
                            pattern(&expected[0].message),
                            "{form} as {equivalent}"
                        );
                    } else {
                        assert_eq!(
                            refused[0].message, expected[0].message,
                            "{form} as {equivalent}"
                        );
                    }
                }
            }
        }
        for command in PROJECT_DELETIONS {
            for nesting in NESTINGS {
                let nested = nesting.replace("{}", command);
                assert!(evaluate(&nested, &section).is_empty(), "{nested}");
            }
        }
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

    /// Grok review of TSK-171: blocking escalation by default must not
    /// refuse ordinary shell work.
    #[test]
    fn test_ordinary_shell_work_is_not_blocked_by_default() {
        for cmd in [
            "bash -c 'printf ok'",
            "source .venv/bin/activate",
            "LD_LIBRARY_PATH=/opt/lib cargo test",
        ] {
            let v = evaluate(cmd, &SecuritySection::default());
            assert!(!any_blocking(&v), "{cmd}: {v:?}");
        }
        for cmd in ["sudo id", "bash -c 'sudo id'", "LD_PRELOAD=/tmp/x.so ls"] {
            let v = evaluate(cmd, &SecuritySection::default());
            assert!(any_blocking(&v), "{cmd}");
            assert_eq!(v[0].rule, "security.privilege_escalation");
        }
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
