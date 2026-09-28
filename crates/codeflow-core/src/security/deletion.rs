//! Deletions composed from more than one word (TSK-141).
//!
//! The catastrophic floor in [`super::dangerous`] reads each `rm` on its own.
//! The same deletion reaches a protected target other ways: through
//! `find … -delete` or `find … -exec rm -r`, through `xargs rm -r` fed a
//! protected listing, through a relative path after `cd` into a protected
//! directory, through a variable assigned earlier in the line, or inside a
//! subshell, group or control-structure body. This module reads the line
//! the way git-guard does and judges each of those forms by the target it
//! reaches, with the same classification the `rm` check uses.

//!
//! The reader tracks, across the simple commands of one line in order, the
//! directory a `cd` moved to and the values a plain assignment or a `for`
//! list gave a variable. It over-approximates: a `cd` or an assignment
//! inside a subshell is kept for the rest of the line, and every value a
//! `for` list names is tried. That can refuse an unusual line whose
//! subshell moved to a protected directory and returned; it never lets a
//! deletion through that the plain reading would refuse.

use std::collections::HashMap;

use super::dangerous::{
    dangerous_rm_target, effective_invocation, normalize_path, program_name, recursive_rm_operands,
};
use crate::hooks::git_guard::{
    command_argv, expand_commands, strip_launchers, strip_reserved_words,
};

/// The protected target a composed deletion in `command` reaches, if any.
pub(super) fn composed_deletion(command: &str) -> Option<&'static str> {
    let piped_to_xargs = pipes_into_xargs(command);
    let mut shell = Shell::default();
    let mut previous: Option<Vec<String>> = None;
    for segment in expand_commands(command) {
        let mut words = command_argv(&segment);
        strip_reserved_words(&mut words);
        let Some(first) = words.first() else {
            continue;
        };
        if first == "for" {
            shell.record_for(&words);
            previous = None;
            continue;
        }
        if shell.record_assignments(&words) {
            continue;
        }
        let Some((program, args)) = strip_launchers(&words) else {
            continue;
        };
        let mut written = vec![program.to_string()];
        written.extend(args.iter().cloned());
        let invocation = effective_invocation(&written);
        let found = match program_name(&invocation[0]).as_str() {
            "cd" | "pushd" => {
                shell.cd(&invocation[1..]);
                None
            }
            "rm" => shell.rm_target(&invocation[1..]),
            "find" => shell.find_target(&invocation[1..], true),
            "xargs" => {
                let inner = after_xargs_options(&invocation[1..]);
                let removes = inner
                    .first()
                    .is_some_and(|program| program_name(program) == "rm")
                    && recursive_rm_operands(&inner[1..]).is_some();
                if removes {
                    shell.rm_target(&inner[1..]).or_else(|| {
                        previous
                            .as_deref()
                            .filter(|_| piped_to_xargs)
                            .and_then(|producer| shell.producer_target(producer))
                    })
                } else {
                    None
                }
            }
            _ => None,
        };
        if found.is_some() {
            return found;
        }
        previous = Some(invocation);
    }
    None
}

/// Whether the line pipes into `xargs`, so the command before an `xargs`
/// is its producer.
fn pipes_into_xargs(command: &str) -> bool {
    command.split('|').skip(1).any(|after| {
        after
            .split_whitespace()
            .next()
            .is_some_and(|program| program_name(program) == "xargs")
    })
}

/// The command `xargs` runs, after its own options.
fn after_xargs_options(args: &[String]) -> &[String] {
    const WITH_VALUE: &[&str] = &[
        "-I",
        "-L",
        "-n",
        "-P",
        "-s",
        "-d",
        "-E",
        "-a",
        "--arg-file",
        "--delimiter",
        "--eof",
        "--max-lines",
        "--max-args",
        "--max-procs",
        "--max-chars",
        "--replace",
    ];
    let mut at = 0;
    while let Some(arg) = args.get(at) {
        if arg == "--" {
            return &args[at + 1..];
        }
        if !arg.starts_with('-') || arg == "-" {
            break;
        }
        at += if WITH_VALUE.contains(&arg.as_str()) {
            2
        } else {
            1
        };
    }
    args.get(at..).unwrap_or_default()
}

/// `find` tests that narrow what it matches to named entries; a deletion
/// under one of them is a selective cleanup, not the whole tree.
const FIND_NAME_TESTS: &[&str] = &[
    "-name",
    "-iname",
    "-path",
    "-ipath",
    "-wholename",
    "-iwholename",
    "-regex",
    "-iregex",
    "-lname",
    "-ilname",
];

/// What the line has set so far.
#[derive(Default)]
struct Shell {
    /// The directory a `cd` moved to, spelled absolute or from `~`; `None`
    /// while the line is still where it started (the project).
    cwd: Option<String>,
    /// The values a variable may hold.
    vars: HashMap<String, Vec<String>>,
}

impl Shell {
    /// `for NAME in WORDS`: the variable takes each word.
    fn record_for(&mut self, words: &[String]) {
        if let [_, name, keyword, values @ ..] = words {
            if keyword == "in" && is_name(name) {
                let values = values.iter().flat_map(|v| self.expand(v)).collect();
                self.vars.insert(name.clone(), values);
            }
        }
    }

    /// A line of only assignments (`D=/`, `export D=/`): record them.
    /// Returns `false` for any other command.
    fn record_assignments(&mut self, words: &[String]) -> bool {
        let words = match words.first().map(String::as_str) {
            Some("export" | "readonly" | "local" | "declare" | "typeset") => {
                let rest = &words[1..];
                let at = rest
                    .iter()
                    .position(|w| !w.starts_with('-'))
                    .unwrap_or(rest.len());
                &rest[at..]
            }
            _ => words,
        };
        let assignments: Option<Vec<(&str, &str)>> = words
            .iter()
            .map(|word| word.split_once('=').filter(|(name, _)| is_name(name)))
            .collect();
        let Some(assignments) = assignments.filter(|a| !a.is_empty()) else {
            return false;
        };
        for (name, value) in assignments {
            if value.contains(['`', '(']) {
                self.vars.remove(name); // a substitution: unknown
            } else {
                let values = self.expand(value);
                self.vars.insert(name.to_string(), values);
            }
        }
        true
    }

    /// `cd DIR`: move, as far as the line can tell.
    fn cd(&mut self, args: &[String]) {
        let target = args.iter().find(|a| !a.starts_with('-') || *a == "-");
        let Some(target) = target else {
            self.cwd = Some("~".to_string()); // `cd` alone goes home
            return;
        };
        if target == "-" {
            self.cwd = None;
            return;
        }
        let resolved = self
            .expand(target)
            .into_iter()
            .next()
            .map(|dir| self.resolve(&dir));
        // A relative move while still in the project stays in the project.
        self.cwd = resolved.filter(|dir| !is_relative(dir));
    }

    /// The protected target a recursive `rm` with these arguments reaches.
    fn rm_target(&self, args: &[String]) -> Option<&'static str> {
        recursive_rm_operands(args)?
            .into_iter()
            .find_map(|operand| self.target_of(operand, ""))
    }

    /// The protected target a `find` deletion reaches: `-delete`, or
    /// `-exec`/`-execdir`/`-ok`/`-okdir` running a recursive `rm`, over a
    /// start that is protected and without a name test. With
    /// `deleting` false it judges `find` as a producer for `xargs`.
    fn find_target(&self, args: &[String], deleting: bool) -> Option<&'static str> {
        let mut at = 0;
        while let Some(arg) = args.get(at) {
            match arg.as_str() {
                "-H" | "-L" | "-P" => at += 1,
                "-D" => at += 2,
                _ if arg.starts_with("-O") => at += 1,
                _ => break,
            }
        }
        let rest = args.get(at..).unwrap_or_default();
        let starts_end = rest
            .iter()
            .position(|a| a.starts_with('-') || matches!(a.as_str(), "(" | "!" | ","))
            .unwrap_or(rest.len());
        let (starts, expression) = rest.split_at(starts_end);
        if expression
            .iter()
            .any(|a| FIND_NAME_TESTS.contains(&a.as_str()))
        {
            return None;
        }
        if deleting && !deletes(expression) {
            return None;
        }
        let dot = [".".to_string()];
        let starts = if starts.is_empty() { &dot[..] } else { starts };
        starts.iter().find_map(|start| self.target_of(start, ""))
    }

    /// The protected target an `xargs rm -r` reaches through the command
    /// that feeds it: a listing (`ls`), a `find`, or words it prints.
    fn producer_target(&self, producer: &[String]) -> Option<&'static str> {
        let args = &producer[1..];
        let operands = || args.iter().filter(|a| !a.starts_with('-'));
        match program_name(&producer[0]).as_str() {
            // `ls DIR` feeds DIR's entries: the equivalent is `DIR/*`.
            "ls" => {
                let dirs: Vec<&String> = operands().collect();
                if dirs.is_empty() {
                    self.target_of(".", "/*")
                } else {
                    dirs.into_iter().find_map(|dir| self.target_of(dir, "/*"))
                }
            }
            "find" => self.find_target(args, false),
            "echo" | "printf" => operands().find_map(|word| self.target_of(word, "")),
            _ => None,
        }
    }

    /// The protected target `operand` names, with `suffix` appended, for
    /// every value its variables may hold.
    fn target_of(&self, operand: &str, suffix: &str) -> Option<&'static str> {
        self.expand(operand).into_iter().find_map(|value| {
            let path = self.resolve(&value);
            let path = if suffix.is_empty() || path.ends_with('*') {
                path
            } else {
                format!("{}{suffix}", path.trim_end_matches('/'))
            };
            dangerous_rm_target(&path)
        })
    }

    /// Each value `word` may take once the recorded variables are
    /// substituted; unknown variables are left as written.
    fn expand(&self, word: &str) -> Vec<String> {
        let mut values = vec![word.to_string()];
        for (name, choices) in &self.vars {
            let braced = format!("${{{name}}}");
            let plain = format!("${name}");
            let mut next = Vec::new();
            for value in &values {
                if value.contains(&braced) || mentions(value, &plain) {
                    for choice in choices {
                        let replaced = value.replace(&braced, choice);
                        next.push(replace_plain(&replaced, &plain, choice));
                    }
                } else {
                    next.push(value.clone());
                }
            }
            next.truncate(64);
            values = next;
        }
        values
    }

    /// `path` as the line reaches it: absolute, or from `~`; a relative
    /// path while the line is still in the project stays relative.
    fn resolve(&self, path: &str) -> String {
        let home_rest = ["~", "$HOME", "${HOME}"].iter().find_map(|home| {
            path.strip_prefix(home)
                .filter(|rest| rest.is_empty() || rest.starts_with('/'))
        });
        if let Some(rest) = home_rest {
            return from_home(rest);
        }
        if path.starts_with('/') {
            return path.to_string();
        }
        match &self.cwd {
            Some(cwd) if cwd.starts_with('~') => from_home(&format!("{}/{path}", &cwd[1..])),
            Some(cwd) => normalize_path(&format!("{cwd}/{path}")),
            None => path.to_string(),
        }
    }
}

/// Whether a `find` expression deletes: `-delete`, or an exec-style action
/// running a recursive `rm`.
fn deletes(expression: &[String]) -> bool {
    if expression.iter().any(|a| a == "-delete") {
        return true;
    }
    expression.iter().enumerate().any(|(at, action)| {
        if !matches!(action.as_str(), "-exec" | "-execdir" | "-ok" | "-okdir") {
            return false;
        }
        let body = &expression[at + 1..];
        let end = body
            .iter()
            .position(|a| a == ";" || a == "+")
            .unwrap_or(body.len());
        let body = &body[..end];
        body.first()
            .is_some_and(|program| program_name(program) == "rm")
            && recursive_rm_operands(&body[1..]).is_some()
    })
}

/// A path below `~` with its `.` and `..` resolved; leaving the home
/// directory resolves from `HOME` instead.
fn from_home(rest: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in rest.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());
                    return normalize_path(&format!("{home}/{rest}"));
                }
            }
            _ => parts.push(part),
        }
    }
    if parts.is_empty() {
        "~".to_string()
    } else {
        format!("~/{}", parts.join("/"))
    }
}

/// Whether `value` holds `$NAME` not followed by a name character.
fn mentions(value: &str, plain: &str) -> bool {
    value.match_indices(plain).any(|(at, _)| {
        !value[at + plain.len()..].starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_')
    })
}

/// Replace each whole `$NAME` in `value` with `choice`.
fn replace_plain(value: &str, plain: &str, choice: &str) -> String {
    let mut out = String::new();
    let mut rest = value;
    while let Some(at) = rest.find(plain) {
        let after = &rest[at + plain.len()..];
        out.push_str(&rest[..at]);
        if after.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_') {
            out.push_str(plain);
        } else {
            out.push_str(choice);
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

/// A shell variable name.
fn is_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !name.starts_with(|c: char| c.is_ascii_digit())
}

fn is_relative(path: &str) -> bool {
    !path.starts_with(['/', '~', '$'])
}

/// Each AC-1 form beside the `rm -rf` it is equivalent to; both are refused
/// with the same target (TSK-141 AC-1). The last rows are forms refused
/// before this task, kept as they are.
#[cfg(test)]
pub(crate) const COMPOSED_PAIRS: &[(&str, &str)] = &[
    ("find / -delete", "rm -rf /"),
    ("find ~ -delete", "rm -rf ~"),
    ("find /etc -delete", "rm -rf /etc"),
    ("find -L / -delete", "rm -rf /"),
    ("find / -exec rm -rf {} +", "rm -rf /"),
    (r"find / -exec rm -rf {} \;", "rm -rf /"),
    (
        "find /Users/alice -execdir rm -r {} +",
        "rm -rf /Users/alice",
    ),
    ("find / -print0 | xargs -0 rm -rf", "rm -rf /"),
    ("ls / | xargs rm -rf", "rm -rf /*"),
    ("ls ~ | xargs -n 1 rm -rf", "rm -rf ~/*"),
    ("echo / | xargs rm -rf", "rm -rf /"),
    ("ls /etc | xargs -I{} rm -rf /etc/{}", "rm -rf /etc/*"),
    ("xargs rm -rf / < /dev/null", "rm -rf /"),
    ("rm -rf /Users/alice", "rm -rf ~"),
    ("rm -rf /home/alice/", "rm -rf ~"),
    ("rm -rf /Users/alice/*", "rm -rf ~/*"),
    ("rm -rf /home/alice/proj/..", "rm -rf ~"),
    ("cd ~ && rm -rf *", "rm -rf ~/*"),
    ("cd; rm -rf ./*", "rm -rf ~/*"),
    ("cd $HOME && rm -rf .", "rm -rf ~"),
    ("cd / && rm -rf *", "rm -rf /*"),
    ("cd /etc; rm -rf .", "rm -rf /etc"),
    ("cd /Users && rm -rf alice", "rm -rf /Users/alice"),
    ("cd ~/code && rm -rf ..", "rm -rf ~"),
    ("cd / && find . -delete", "rm -rf /"),
    ("cd / && ls | xargs rm -rf", "rm -rf /*"),
    ("D=/; rm -rf $D", "rm -rf /"),
    ("D=/etc && rm -rf \"$D\"", "rm -rf /etc"),
    ("export D=~; rm -rf ${D}/*", "rm -rf ~/*"),
    ("D=/; find $D -delete", "rm -rf /"),
    ("D=/usr; cd $D && rm -rf *", "rm -rf /usr"),
    ("for d in / ~; do rm -rf \"$d\"; done", "rm -rf /"),
    ("(rm -rf /)", "rm -rf /"),
    ("{ rm -rf /; }", "rm -rf /"),
    ("if true; then rm -rf /; fi", "rm -rf /"),
    ("for i in 1 2; do rm -rf /; done", "rm -rf /"),
    ("while true; do rm -rf /; done", "rm -rf /"),
    ("until false; do rm -rf /; done", "rm -rf /"),
    ("case a in a) rm -rf /;; esac", "rm -rf /"),
    ("(cd ~ && rm -rf *)", "rm -rf ~/*"),
    ("sudo find / -delete", "rm -rf /"),
    ("rm -rf /*", "rm -rf /*"),
    ("rm -rf --no-preserve-root /", "rm -rf /"),
];

/// Deletions that stay inside the project: allowed, alone and nested
/// (TSK-141 AC-2).
#[cfg(test)]
pub(crate) const PROJECT_DELETIONS: &[&str] = &[
    "rm -rf ./build",
    "rm -rf target",
    "find . -name '*.o' -delete",
    "find ./build -delete",
    "find . -name node_modules -exec rm -rf {} +",
    "find ~ -name .DS_Store -delete",
    "D=./build; rm -rf $D",
    "D=build && find $D -delete",
    "cd build && rm -rf *",
    "cd ~/code/app && rm -rf target",
    "rm -rf ~/code/app/target",
    "rm -rf /Users/alice/project/target",
    "ls ./build | xargs rm -rf",
    "ls ~/code/app/build | xargs rm -rf",
    "find ./dist -print0 | xargs -0 rm -rf",
    "for d in build dist; do rm -rf \"$d\"; done",
    "cd /; ls",
    "find / -name '*.log'",
    "echo / | xargs ls",
    "rm -r /tmp/scratch",
];

/// The control structures and sequences a form is nested in: each `{}` is
/// replaced by the form.
#[cfg(test)]
pub(crate) const NESTINGS: &[&str] = &[
    "{}",
    "( {} )",
    "{ {}; }",
    "if true; then {}; fi",
    "if false; then :; else {}; fi",
    "for i in 1; do {}; done",
    "while true; do {}; break; done",
    "until false; do {}; break; done",
    "case a in a) {};; esac",
    "true && {}",
    "true; {}",
    "false || {}",
    "echo start\n{}",
    "( if true; then {}; fi )",
];

#[cfg(test)]
mod tests {
    use super::super::dangerous::DangerousModule;
    use super::*;
    use crate::security::{CheckContext, SecurityModule, SecurityPolicy};

    fn refused(command: &str) -> Option<String> {
        let policy = SecurityPolicy::defaults();
        DangerousModule
            .check(&CheckContext {
                command,
                sandbox_bypass: false,
                current_branch: "",
                policy: &policy,
            })
            .map(|verdict| verdict.pattern)
    }

    #[test]
    fn each_composed_form_is_refused_as_its_rm_equivalent() {
        for (form, equivalent) in COMPOSED_PAIRS {
            let expected = refused(equivalent);
            assert!(expected.is_some(), "the equivalent {equivalent} is refused");
            assert_eq!(refused(form), expected, "{form} as {equivalent}");
        }
    }

    #[test]
    fn a_project_deletion_is_allowed() {
        for command in PROJECT_DELETIONS {
            assert_eq!(refused(command), None, "{command}");
        }
        // A form named as data is not run.
        for command in [
            "git commit -m 'cd / && rm -rf *'",
            "echo 'find / -delete'",
            "grep -rn 'ls / | xargs rm -rf' docs",
            "printf '%s' 'D=/; rm -rf $D'",
        ] {
            assert_eq!(refused(command), None, "{command}");
        }
    }

    /// The gap class is composition: every form in every nesting is
    /// refused, and every project deletion in every nesting is allowed.
    #[test]
    fn nesting_changes_no_verdict() {
        for nesting in NESTINGS {
            for (form, _) in COMPOSED_PAIRS {
                let nested = nesting.replace("{}", form);
                assert!(refused(&nested).is_some(), "{nested}");
            }
            for command in PROJECT_DELETIONS {
                let nested = nesting.replace("{}", command);
                assert_eq!(refused(&nested), None, "{nested}");
            }
        }
    }
}
