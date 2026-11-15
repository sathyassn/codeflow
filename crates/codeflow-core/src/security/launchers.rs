//! The closed list of command launchers the text guards model (TSK-1002
//! Fable review 16; the operator's decision of 2026-10-29 to narrow the
//! text guard's promise where a class cannot be closed soundly).
//!
//! A launcher runs another command it is given as words, or hands a
//! command paths it finds. The guards read the command, the directory it
//! runs in and the paths it receives only for the launchers listed here,
//! the shells and interpreters, and Git through its runner reader. A word
//! naming a secret or an enforcement path is judged as any program's
//! operand whatever launches it. A launcher outside this list that moves
//! its command to another directory or hands it paths it finds is outside
//! the text guard's promise; the qualified shell parser (TSK-1073, 4.0.0)
//! owns that residual. `docs/policy-reference.md` lists these names under
//! "Command launchers the guards model", held equal to this module by test.
//!
//! Each reader keeps its own option grammar for the launchers it reads;
//! the tests below hold every reader's list inside this one and the
//! policy reference's list equal to it, so outside tests these lists are
//! the stated promise rather than a table any reader consults.
#![cfg_attr(not(test), allow(dead_code))]

/// Programs that run the command after their own options. The GNU names
/// with a `g` prefix (`gtimeout`, `genv`) are read as the plain name.
pub(crate) const LAUNCHERS: &[&str] = &[
    "-",
    "builtin",
    "busybox",
    "caffeinate",
    "chrt",
    "command",
    "doas",
    "env",
    "exec",
    "gsudo",
    "ionice",
    "nice",
    "nocorrect",
    "noglob",
    "nohup",
    "pkexec",
    "setsid",
    "stdbuf",
    "sudo",
    "taskset",
    "time",
    "timeout",
    "unbuffer",
    "xcrun",
];

/// Programs that hand the command they run paths or words they find:
/// `find -exec`, `fd -x`, `xargs`, `parallel` and `watch`.
pub(crate) const SUPPLIERS: &[&str] = &["fd", "fdfind", "find", "parallel", "watch", "xargs"];

/// Package runners that run a package's program with the words after it.
pub(crate) const PACKAGE_RUNNERS: &[&str] = &["bunx", "npx", "pnpx", "pnx"];

/// Shells, whose `-c` string and script input are read as command lines.
pub(crate) const SHELLS: &[&str] = &[
    "ash",
    "bash",
    "cmd",
    "csh",
    "dash",
    "fish",
    "ksh",
    "lksh",
    "mksh",
    "oksh",
    "posh",
    "powershell",
    "pwsh",
    "rbash",
    "sh",
    "tcsh",
    "yash",
    "zsh",
];

/// Interpreters, whose inline code and arguments are read.
pub(crate) const INTERPRETERS: &[&str] = &[
    "deno",
    "make",
    "node",
    "nodejs",
    "osascript",
    "perl",
    "python",
    "ruby",
];

/// The names in backticks on the `- {label}:` bullet, continuation lines
/// included, of the `docs/policy-reference.md` section that starts with
/// `heading`: the closed lists the text guards promise.
#[cfg(test)]
pub(crate) fn policy_reference_list(heading: &str, label: &str) -> Vec<&'static str> {
    let page: &'static str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/policy-reference.md"
    ));
    let start = page.find(heading).unwrap_or_else(|| panic!("{heading}"));
    let section = &page[start + 3..];
    let section = &section[..section.find("\n#").unwrap_or(section.len())];
    let bullet: Vec<&'static str> = section
        .lines()
        .skip_while(|line| !line.starts_with(&format!("- {label}:")))
        .enumerate()
        .take_while(|(at, line)| *at == 0 || line.starts_with("  "))
        .map(|(_, line)| line)
        .collect();
    assert!(!bullet.is_empty(), "the {label} line");
    bullet
        .iter()
        .flat_map(|line| line.split('`').skip(1).step_by(2))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed(name: &str) -> bool {
        LAUNCHERS.contains(&name)
            || SUPPLIERS.contains(&name)
            || PACKAGE_RUNNERS.contains(&name)
            || SHELLS.contains(&name)
            || INTERPRETERS.contains(&name)
    }

    #[test]
    fn every_reader_list_stays_inside_the_closed_list() {
        for name in crate::security::outward::MODELED_LAUNCHERS {
            assert!(listed(name), "outward launcher {name}");
        }
        for name in crate::security::composed::LAUNCHERS {
            let name = crate::hooks::git_guard::program_name(name);
            assert!(listed(name), "composed launcher {name}");
        }
        for name in crate::security::composed::SUPPLIERS {
            assert!(SUPPLIERS.contains(name), "composed supplier {name}");
        }
        for name in crate::hooks::git_guard::GNU_LAUNCHERS {
            assert!(listed(name), "GNU launcher {name}");
        }
        for name in SHELLS {
            assert!(crate::hooks::git_guard::is_shell(name), "shell {name}");
        }
        for name in INTERPRETERS {
            assert!(
                crate::security::outward::interpreter_family(name).is_some(),
                "interpreter {name}"
            );
        }
        for name in ["noglob", "nocorrect", "-"] {
            assert!(crate::hooks::git_guard::is_shell_modifier(name));
            assert!(LAUNCHERS.contains(&name));
        }
    }

    /// TSK-1490 AC-3: the inventory documented in the option reader's module
    /// is the one both guards read, line for line; the rows read by
    /// git-guard's own launcher reader are its command carriers; and every
    /// program and spelling of every row reaches both guards.
    #[test]
    fn every_option_carrier_is_read_by_both_guards() {
        use crate::hooks::git_guard::COMMAND_CARRIERS;
        use crate::security::option_commands::{Runs, OPTION_COMMANDS};

        let source = include_str!("option_commands.rs");
        let documented: Vec<&str> = source
            .lines()
            .skip_while(|line| !line.starts_with("//! The inventory, one line per row"))
            .skip_while(|line| !line.starts_with("//! - "))
            .take_while(|line| line.starts_with("//! - "))
            .map(|line| &line[4..])
            .collect();
        let rendered: Vec<String> = OPTION_COMMANDS.iter().map(render_row).collect();
        assert_eq!(documented, rendered);

        for row in OPTION_COMMANDS {
            for program in row.programs {
                assert_eq!(
                    COMMAND_CARRIERS.contains(program),
                    row.runs == Runs::OwnReader,
                    "{program}"
                );
            }
        }

        // Both guards judge in an owned, unborn repository, never this
        // source checkout.
        let dir = tempfile::tempdir().unwrap();
        git2::Repository::init(dir.path()).unwrap();
        let root = dir.path().to_path_buf();
        let policy = crate::hooks::policy::GitPolicy::default();
        let ctx = crate::hooks::git_guard::GuardContext {
            policy: &policy,
            current_branch: "feat/x",
            integrate_token: false,
            pr_base_lookup: None,
            dir_target_lookup: None,
            alias_lookup: None,
            branch_lookup: None,
            discard_lookup: Some(&|_| Ok(None)),
            root_checkout: None,
        };
        let section = crate::hooks::policy::SecuritySection::default();
        let git_rules = |line: &str| -> Vec<String> {
            crate::hooks::git_guard::evaluate_report_at(line, &ctx, &root)
                .violations
                .into_iter()
                .map(|v| v.rule)
                .collect()
        };
        let exec_rules = |line: &str| -> Vec<String> {
            crate::hooks::exec_guard::evaluate_at(
                line,
                &section,
                crate::hooks::policy::PolicyLevel::Block,
                &root,
                &root,
            )
            .into_iter()
            .map(|v| v.rule)
            .collect()
        };
        let git_command = "git config core.hooksPath /tmp/h";
        let exec_command = "cat ~/.ssh/id_rsa";
        let mut failed = Vec::new();
        for row in OPTION_COMMANDS {
            let (git_rule, exec_rule) = match row.runs {
                Runs::Language | Runs::CargoConfig => ("git.hook_integrity", "git.hook_integrity"),
                _ => ("git.hook_integrity", "security.secret_reads"),
            };
            for (command, rule, guard) in [
                (
                    git_command,
                    git_rule,
                    &git_rules as &dyn Fn(&str) -> Vec<String>,
                ),
                (exec_command, exec_rule, &exec_rules),
            ] {
                for line in probes(row, command) {
                    if !guard(&line).iter().any(|r| r == rule) {
                        failed.push(format!("{rule}: {line}"));
                    }
                }
            }
        }
        assert!(
            failed.is_empty(),
            "{} rows:\n{}",
            failed.len(),
            failed.join("\n")
        );
    }

    /// One inventory row as its line in the option reader's module docs.
    fn render_row(row: &crate::security::option_commands::Carrier) -> String {
        use crate::security::option_commands::Runs;
        let names: Vec<String> = if row.subcommands.is_empty() {
            row.programs.iter().map(|p| format!("`{p}`")).collect()
        } else {
            row.programs
                .iter()
                .flat_map(|p| {
                    row.subcommands
                        .iter()
                        .map(move |words| format!("`{p} {}`", words.join(" ")))
                })
                .collect()
        };
        let mut options: Vec<String> = row.short.iter().map(|c| format!("`-{c}`")).collect();
        options.extend(row.long.iter().map(|l| format!("`{l}`")));
        if row.plus {
            options.push("`+`".to_string());
        }
        if row.runs == Runs::CloneArguments {
            options.push("`--`".to_string());
        }
        format!("- {}: {}", names.join(", "), options.join(", "))
    }

    /// One probe line per program and spelling of `row`, carrying `command`
    /// as the row runs a value.
    fn probes(row: &crate::security::option_commands::Carrier, command: &str) -> Vec<String> {
        use crate::security::option_commands::Runs;
        let value = match row.runs {
            Runs::SshOption => format!("ProxyCommand={command}"),
            Runs::Checkpoint => format!("exec={command}"),
            Runs::Language => "x".to_string(),
            Runs::CargoConfig => "target.x.runner=y".to_string(),
            Runs::Wgetrc => format!("use_askpass={command}"),
            _ => command.to_string(),
        };
        // A value as one word, or as the command words a `Words` row takes.
        let word = if row.runs == Runs::Words {
            value.clone()
        } else {
            format!("'{value}'")
        };
        let mut spellings: Vec<String> = Vec::new();
        for letter in row.short {
            spellings.push(if row.attached {
                format!("-{letter}{word}")
            } else {
                format!("-{letter} {word}")
            });
        }
        for long in row.long {
            spellings.push(format!("{long}={word}"));
            if !row.attached {
                spellings.push(format!("{long} {word}"));
            }
            if row.glued && !long.starts_with("--") {
                spellings.push(format!("{long}{word}"));
            }
        }
        if row.plus {
            spellings.push(format!("'+{value}'"));
        }
        if row.runs == Runs::CloneArguments {
            spellings.push(format!("o/r -- -u {word}"));
        }
        let mut lines = Vec::new();
        for program in row.programs {
            let heads: Vec<String> = if row.subcommands.is_empty() {
                vec![(*program).to_string()]
            } else {
                row.subcommands
                    .iter()
                    .map(|words| format!("{program} {}", words.join(" ")))
                    .collect()
            };
            for head in &heads {
                for spelling in &spellings {
                    lines.push(match *program {
                        "flock" => format!("{head} /tmp/l {spelling}"),
                        "script" => format!("{head} {spelling} /dev/null"),
                        _ => format!("{head} {spelling}"),
                    });
                }
            }
        }
        lines
    }

    /// The policy reference lists exactly these names, in the section that
    /// states the promise.
    #[test]
    fn the_policy_reference_lists_the_closed_list() {
        for (label, names) in [
            ("Launchers", LAUNCHERS),
            ("Suppliers", SUPPLIERS),
            ("Package runners", PACKAGE_RUNNERS),
            ("Shells", SHELLS),
            ("Interpreters", INTERPRETERS),
        ] {
            let mut listed = policy_reference_list("### Command launchers the guards model", label);
            listed.sort_unstable();
            let mut expected = names.to_vec();
            expected.sort_unstable();
            assert_eq!(listed, expected, "{label}");
        }
    }
}
