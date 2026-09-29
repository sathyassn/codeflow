//! Parsed action-family checks shared by the execution guard.
use super::actions::{table, PatternToken};
use super::headless::{
    after_runner_options, package_bin, shell_command_string, skip_assignments, skip_env,
    skip_options,
};
use crate::hooks::git_guard::{command_argv, expand_commands, strip_reserved_words};
use crate::hooks::policy::SecuritySection;
use crate::hooks::Violation;
use std::path::Path;

#[cfg(test)]
pub(crate) fn evaluate(command: &str, levels: &SecuritySection) -> Vec<Violation> {
    evaluate_at(command, levels, None)
}

pub(crate) fn evaluate_at(
    command: &str,
    levels: &SecuritySection,
    cwd: Option<&Path>,
) -> Vec<Violation> {
    let mut parsed = Parsed::default();
    visit(command, cwd, 0, &mut parsed);
    let mut embedded = Parsed::default();
    for code in &parsed.code {
        let words = literal_words(code);
        for at in 0..words.len() {
            argv(&words[at..], cwd, 0, &mut embedded);
        }
    }
    let mut families = parsed.families;
    families.extend(embedded.families);
    families.sort_unstable();
    families.dedup();
    families
        .into_iter()
        .filter_map(|id| {
            let level = match id {
                "privilege" => levels.privilege_escalation,
                "keychain" => levels.secret_reads,
                _ => levels.outward_actions,
            };
            level.is_active().then(|| {
                Violation::new(
                    match id {
                        "privilege" => "security.privilege_escalation",
                        "keychain" => "security.secret_reads",
                        _ => "security.outward_actions",
                    },
                    level,
                    format!(
                        "{id} action: {}",
                        table()
                            .families
                            .iter()
                            .find(|f| f.id == id)
                            .map_or("operator-owned action", |f| f.why.as_str())
                    ),
                    crate::remedy::OUTWARD_ACTION.remedy(),
                )
            })
        })
        .collect()
}

#[derive(Default)]
struct Parsed {
    families: Vec<&'static str>,
    code: Vec<String>,
    stdin_interpreter: usize,
    commands: Vec<Vec<String>>,
}

pub(crate) fn unwrapped_commands(command: &str) -> Vec<Vec<String>> {
    let mut parsed = Parsed::default();
    visit(command, None, 0, &mut parsed);
    parsed.commands
}

pub(crate) fn interpreter_bodies(command: &str) -> Vec<String> {
    let mut parsed = Parsed::default();
    visit(command, None, 0, &mut parsed);
    parsed.code
}

pub(crate) fn literal_words(code: &str) -> Vec<String> {
    code.split(|c: char| {
        !(c.is_alphanumeric() || matches!(c, '_' | '.' | '/' | '~' | '+' | '-' | '@' | '=' | ':'))
    })
    .filter(|word| !word.is_empty())
    .map(str::to_string)
    .collect()
}

fn visit(command: &str, cwd: Option<&Path>, depth: usize, out: &mut Parsed) {
    if depth > 16 {
        return;
    }
    let mut directories = vec![cwd.map(Path::to_path_buf)];
    for segment in expand_commands(command) {
        let mut words = command_argv(&segment);
        strip_reserved_words(&mut words);
        let args = skip_assignments(&words);
        let before = out.stdin_interpreter;
        for dir in &directories {
            argv(args, dir.as_deref(), depth + 1, out);
        }
        if out.stdin_interpreter > before && command.contains("<<") {
            if let Some((_, body)) = command.split_once('\n') {
                out.code.push(body.to_string());
            }
        }
        if args.first().is_some_and(|s| s == "cd") {
            if let Some(path) = args.get(1).filter(|p| !p.contains(['$', '`'])) {
                let next: Vec<_> = directories
                    .iter()
                    .filter_map(|dir| dir.as_ref().map(|dir| Some(dir.join(path))))
                    .collect();
                directories.extend(next);
                directories.sort();
                directories.dedup();
            }
        }
    }
}

fn argv(args: &[String], cwd: Option<&Path>, depth: usize, out: &mut Parsed) {
    if depth > 32 {
        return;
    }
    let Some(program) = args.first() else {
        return;
    };
    let name = program.rsplit('/').next().unwrap_or(program);
    out.commands.push(args.to_vec());
    let rest = &args[1..];
    record_interpreter(name, rest, out);
    direct_family(name, rest, cwd, out);
    let inner = match name {
        "env" => {
            if let Some(at) = rest
                .iter()
                .position(|a| a == "-C" || a == "--chdir" || a.starts_with("--chdir="))
            {
                let path = rest[at]
                    .strip_prefix("--chdir=")
                    .or_else(|| rest.get(at + 1).map(String::as_str));
                if let (Some(cwd), Some(path), Some(inner)) = (cwd, path, skip_env(rest)) {
                    argv(inner, Some(&cwd.join(path)), depth + 1, out);
                }
            }
            if let Some(at) = rest.iter().position(|a| a == "-S" || a == "--split-string") {
                if let Some(script) = rest.get(at + 1) {
                    visit(script, cwd, depth + 1, out);
                }
            }
            skip_env(rest)
        }
        "command" | "builtin" | "exec" | "nohup" | "setsid" | "caffeinate" | "unbuffer"
        | "time" => Some(skip_options(rest, &["-a"])),
        "nice" => Some(skip_options(rest, &["-n", "--adjustment"])),
        "stdbuf" => Some(skip_options(rest, &["-i", "-o", "-e"])),
        "timeout" => Some(
            skip_options(rest, &["-s", "--signal", "-k", "--kill-after"])
                .get(1..)
                .unwrap_or_default(),
        ),
        "xargs" => Some(skip_options(
            rest,
            &[
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
            ],
        )),
        "bash" | "sh" | "zsh" | "dash" | "ksh" | "ash" => {
            if let Some(script) = shell_command_string(rest) {
                visit(script, cwd, depth + 1, out);
            }
            None
        }
        "find" => {
            for (at, word) in rest.iter().enumerate() {
                if matches!(word.as_str(), "-exec" | "-execdir" | "-ok" | "-okdir") {
                    let tail = &rest[at + 1..];
                    let end = tail
                        .iter()
                        .position(|s| s == ";" || s == "+")
                        .unwrap_or(tail.len());
                    argv(&tail[..end], cwd, depth + 1, out);
                }
            }
            None
        }
        "npx" | "bunx" | "pnpx" | "pnx" => {
            for at in after_runner_options(rest) {
                let mut inner = rest.get(at..).unwrap_or_default().to_vec();
                if let Some(program) = inner.first_mut() {
                    *program = package_bin(program);
                }
                argv(&inner, cwd, depth + 1, out);
            }
            None
        }
        _ => None,
    };
    if let Some(inner) = inner {
        argv(inner, cwd, depth + 1, out);
    }
}

fn record_interpreter(name: &str, rest: &[String], out: &mut Parsed) {
    if matches!(
        name,
        "python" | "python3" | "node" | "nodejs" | "perl" | "ruby"
    ) {
        let flag = if name.starts_with("python") {
            "-c"
        } else {
            "-e"
        };
        let mut found = false;
        for (at, arg) in rest.iter().enumerate() {
            if arg == flag
                || (name.starts_with("node") && matches!(arg.as_str(), "--eval" | "-p" | "--print"))
            {
                if let Some(code) = rest.get(at + 1) {
                    out.code.push(code.clone());
                    found = true;
                }
            } else if let Some(code) = arg.strip_prefix(flag).filter(|code| !code.is_empty()) {
                out.code.push(code.to_string());
                found = true;
            } else if let Some(code) = arg.strip_prefix("--eval=") {
                out.code.push(code.to_string());
                found = true;
            }
        }
        if !found {
            out.stdin_interpreter += 1;
        }
    }
}

fn direct_family(name: &str, rest: &[String], cwd: Option<&Path>, out: &mut Parsed) {
    let mut canonical = vec![name.to_string()];
    canonical.extend(global_args(name, rest).iter().cloned());
    for family in &table().families {
        if family
            .codex
            .iter()
            .any(|rule| prefix(&canonical, &rule.pattern))
        {
            out.families.push(family_id(&family.id));
        }
    }
    let sub = canonical.get(1).map_or("", String::as_str);
    let body = canonical.get(2..).unwrap_or_default();
    if name == "git" && sub == "push" {
        if body.iter().any(|a| a == "--mirror") {
            out.families.push("account");
        }
        if body.iter().any(|a| {
            matches!(a.as_str(), "--tags" | "--follow-tags" | "tag") || a.contains("refs/tags/")
        }) || tag_push(body, cwd, rest)
        {
            out.families.push("release");
        }
    }
    if name == "gh" {
        if sub == "repo"
            && body.first().is_some_and(|s| s == "edit")
            && body
                .iter()
                .any(|s| s == "--visibility" || s.starts_with("--visibility="))
        {
            out.families.push("account");
        }
        if sub == "api"
            && body.iter().enumerate().any(|(i, a)| {
                a.eq_ignore_ascii_case("-XDELETE")
                    || a.eq_ignore_ascii_case("--method=DELETE")
                    || ((a == "-X" || a == "--method")
                        && body
                            .get(i + 1)
                            .is_some_and(|v| v.eq_ignore_ascii_case("DELETE")))
            })
        {
            out.families.push("account");
        }
    }
    if name.starts_with("git-credential-") {
        out.families.push("account");
    }
    if matches!(name, "runuser" | "gsudo.exe" | "runas.exe")
        || (name == "osascript" && rest.join(" ").contains("with administrator privileges"))
    {
        out.families.push("privilege");
    }
    if name.eq_ignore_ascii_case("Set-ExecutionPolicy")
        || (name == "crontab"
            && !rest
                .iter()
                .any(|a| matches!(a.as_str(), "-l" | "--help" | "-h"))
            && !rest.is_empty())
    {
        out.families.push("persistence");
    }
}

fn family_id(id: &str) -> &'static str {
    match id {
        "privilege" => "privilege",
        "publish" => "publish",
        "release" => "release",
        "account" => "account",
        "keychain" => "keychain",
        "persistence" => "persistence",
        _ => unreachable!("action table family lacks guard routing"),
    }
}

// Only options before the subcommand are skipped. Data after an ordinary
// subcommand (for example cargo test --features publish) is never an action.
fn global_args<'a>(name: &str, rest: &'a [String]) -> &'a [String] {
    let values: &[&str] = match name {
        "git" => &[
            "-C",
            "-c",
            "--git-dir",
            "--work-tree",
            "--namespace",
            "--config-env",
        ],
        "gh" => &["-R", "--repo", "--hostname"],
        "cargo" => &[
            "--manifest-path",
            "--config",
            "--color",
            "--target-dir",
            "-Z",
        ],
        "npm" | "pnpm" | "yarn" | "uv" => &[
            "--prefix",
            "--registry",
            "--cwd",
            "--dir",
            "-C",
            "--project",
            "--directory",
            "--cache-dir",
        ],
        _ => &[],
    };
    if !matches!(
        name,
        "git"
            | "gh"
            | "cargo"
            | "npm"
            | "pnpm"
            | "yarn"
            | "uv"
            | "systemctl"
            | "security"
            | "defaults"
            | "launchctl"
            | "twine"
            | "gem"
    ) {
        return rest;
    }
    let mut at = 0;
    while let Some(word) = rest.get(at) {
        if name == "cargo" && word.starts_with('+') {
            at += 1;
        } else if word == "--" {
            at += 1;
            break;
        } else if values.contains(&word.as_str()) {
            at += 2;
        } else if word.starts_with('-') {
            at += 1;
        } else {
            break;
        }
    }
    rest.get(at..).unwrap_or_default()
}

fn prefix(args: &[String], pattern: &[PatternToken]) -> bool {
    args.len() >= pattern.len()
        && args.iter().zip(pattern).all(|(arg, token)| match token {
            PatternToken::Word(word) => arg == word,
            PatternToken::AnyOf(words) => words.contains(arg),
        })
}

fn tag_push(args: &[String], cwd: Option<&Path>, globals: &[String]) -> bool {
    let Some(cwd) = cwd else {
        return false;
    };
    let mut dir = cwd.to_path_buf();
    for pair in globals.windows(2) {
        if pair[0] == "-C" {
            dir = dir.join(&pair[1]);
        }
    }
    let Ok(repo) = git2::Repository::discover(dir) else {
        return false;
    };
    // A refspec's source and destination can both name a tag; a leading +
    // changes force semantics, not the reference namespace.
    args.iter().filter(|arg| !arg.starts_with('-')).any(|arg| {
        arg.trim_start_matches('+').split(':').any(|part| {
            !part.is_empty() && repo.find_reference(&format!("refs/tags/{part}")).is_ok()
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::actions::{table, PatternToken};

    const WRAPPERS: &[&str] = &[
        "{}",
        "env -i {}",
        "env -C /tmp {}",
        "env -S '{}'",
        "command {}",
        "nohup {}",
        "timeout 2 {}",
        "xargs -n 1 {}",
        "find . -exec {} \";\"",
        "npx --yes {}",
        "bash -lc '{}'",
        "python3 -c 'run(\"{}\")'",
        "node -e 'run(\"{}\")'",
        "perl -e 'run(\"{}\")'",
        "ruby -e 'run(\"{}\")'",
    ];

    #[test]
    fn table_families_are_refused_through_each_wrapper() {
        for family in &table().families {
            for rule in &family.codex {
                let mut forms = vec![Vec::<String>::new()];
                for token in &rule.pattern {
                    let alternatives = match token {
                        PatternToken::Word(word) => vec![word.clone()],
                        PatternToken::AnyOf(words) => words.clone(),
                    };
                    forms = forms
                        .into_iter()
                        .flat_map(|prefix| {
                            alternatives.iter().map(move |word| {
                                let mut next = prefix.clone();
                                next.push(word.clone());
                                next
                            })
                        })
                        .collect();
                }
                for form in forms {
                    let direct = form.join(" ");
                    for wrapper in WRAPPERS {
                        let command = wrapper.replace("{}", &direct);
                        let found = evaluate(&command, &SecuritySection::default());
                        assert!(
                            found.iter().any(|v| v.message.contains(&family.id)),
                            "{}: {command}: {found:?}",
                            family.id
                        );
                        assert!(
                            found.iter().all(|v| v.remedy.contains("operator")),
                            "{command}"
                        );
                        for level in [
                            crate::hooks::PolicyLevel::Warn,
                            crate::hooks::PolicyLevel::Off,
                        ] {
                            let levels = SecuritySection {
                                privilege_escalation: level,
                                outward_actions: level,
                                secret_reads: level,
                                ..SecuritySection::default()
                            };
                            let findings = evaluate(&command, &levels);
                            if level.is_active() {
                                assert!(!findings.is_empty(), "{command}");
                                assert!(findings.iter().all(|v| v.level == level), "{command}");
                            } else {
                                assert!(findings.is_empty(), "{command}");
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn bare_tags_are_resolved_in_the_command_repository() {
        let temp = tempfile::tempdir().unwrap();
        let repo_dir = temp.path().join("other");
        let repo = git2::Repository::init(&repo_dir).unwrap();
        let id = repo.blob(b"tag target").unwrap();
        let object = repo.find_object(id, None).unwrap();
        repo.tag_lightweight("release-candidate", &object, false)
            .unwrap();
        for command in [
            "git -C other push origin release-candidate",
            "cd other && git push origin release-candidate",
            "env -C other git push origin release-candidate",
        ] {
            let findings = evaluate_at(command, &SecuritySection::default(), Some(temp.path()));
            assert!(
                findings.iter().any(|v| v.message.contains("release")),
                "{command}: {findings:?}"
            );
        }
        assert!(evaluate_at(
            "git -C other push origin task/x",
            &SecuritySection::default(),
            Some(temp.path())
        )
        .is_empty());
    }

    #[test]
    fn leading_flags_and_non_prefix_actions_are_refused() {
        for command in [
            "cargo +stable publish",
            "cargo --manifest-path Cargo.toml publish",
            "npm --ignore-scripts publish",
            "gh -R o/r release create v1",
            "git push origin refs/tags/v1",
            "git push origin --follow-tags",
            "git push origin --mirror",
            "gh repo edit --visibility public",
            "gh api repos/o/r -X DELETE",
            "crontab jobs.txt",
            "osascript -e 'do shell script x with administrator privileges'",
        ] {
            assert!(
                !evaluate(command, &SecuritySection::default()).is_empty(),
                "{command}"
            );
        }
    }

    #[test]
    fn ordinary_actions_pass_direct_and_wrapped() {
        for command in [
            "cargo test",
            "git push origin task/x",
            "gh release view v1",
            "cargo package",
            "grep -rn sudo docs/",
            "cargo +nightly test --features publish",
            "gh auth status",
            "defaults read domain",
            "crontab -l",
        ] {
            for wrapper in &WRAPPERS[..11] {
                let command = wrapper.replace("{}", command);
                assert!(
                    evaluate(&command, &SecuritySection::default()).is_empty(),
                    "{command}"
                );
            }
        }
    }
}
