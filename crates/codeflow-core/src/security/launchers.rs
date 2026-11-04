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
