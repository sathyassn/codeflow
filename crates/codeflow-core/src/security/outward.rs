//! Parsed action-family checks shared by the execution guard.
use super::actions::{table, PatternToken};
use super::headless::{
    after_runner_options, package_bin, shell_command_string, skip_assignments, skip_env,
    skip_options,
};
use crate::hooks::git_guard::{
    command_argv, expand_commands, strip_launchers, strip_reserved_words,
};
use crate::hooks::policy::SecuritySection;
use crate::hooks::Violation;
use std::path::{Path, PathBuf};

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
    let release_roots: Vec<_> = families
        .iter()
        .filter(|(id, _)| *id == "release")
        .map(|(_, root)| root.clone())
        .collect();
    families.retain(|(id, root)| *id != "configured_tag_push" || !release_roots.contains(root));
    families
        .into_iter()
        .filter_map(|(id, root)| {
            let authority = root.as_deref().map(crate::hooks::landed_policy::load).transpose();
            let authority = match authority {
                Ok(value) => value,
                Err(reason) => return Some(Violation::always_blocking("git.policy_authority", reason, "restore the named remote-tracking policy authority")),
            };
            let levels = authority.as_ref().map_or(levels, |a| &a.policy.security);
            let source = authority.as_ref().map_or(String::new(), |a| format!("; policy source: {}", a.source));
            let level = match id {
                "privilege" => levels.privilege_escalation,
                "keychain" => levels.secret_reads,
                _ => levels.outward_actions,
            };
            level.is_active().then(|| {
                if id == "configured_tag_push" {
                    return Violation::new(
                        "security.outward_actions",
                        level,
                        format!("release action: effective push.followTags config enables tag publication{source}"),
                        crate::remedy::PUSH_WITHOUT_FOLLOW_TAGS.remedy(),
                    );
                }
                Violation::new(
                    match id {
                        "privilege" => "security.privilege_escalation",
                        "keychain" => "security.secret_reads",
                        _ => "security.outward_actions",
                    },
                    level,
                    format!(
                        "{id} action: {}{source}",
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
    families: Vec<(&'static str, Option<PathBuf>)>,
    code: Vec<String>,
    stdin_interpreter: usize,
    commands: Vec<Vec<String>>,
    created_tags: Vec<(PathBuf, String)>,
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
    code.replace("${HOME}", "$HOME")
        .split(|c: char| {
            !(c.is_alphanumeric()
                || matches!(c, '_' | '.' | '/' | '~' | '+' | '-' | '@' | '=' | ':' | '$'))
        })
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect()
}

fn visit(command: &str, cwd: Option<&Path>, depth: usize, out: &mut Parsed) {
    if depth > 16 {
        return;
    }
    shell_inputs(command, out);
    let mut directories = vec![cwd.map(Path::to_path_buf)];
    for segment in expand_commands(command) {
        let mut words = command_argv(&segment);
        strip_reserved_words(&mut words);
        let mut assigned = skip_assignments(&words).to_vec();
        if let Some(setting) = words
            .iter()
            .take_while(|s| s.contains('='))
            .find(|s| s.starts_with("GIT_DIR="))
        {
            assigned.splice(0..0, ["env".to_string(), setting.clone()]);
        }
        let args = assigned.as_slice();
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

#[allow(clippy::too_many_lines)] // Keep each launcher dispatch beside the shared recursion bound.
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
    if name == "env" {
        if let (Some(dir), Some(inner)) = (
            rest.iter().find_map(|s| s.strip_prefix("GIT_DIR=")),
            skip_env(rest),
        ) {
            if inner.first().is_some_and(|s| s == "git") {
                let mut target = vec!["git".into(), format!("--git-dir={dir}")];
                target.extend_from_slice(&inner[1..]);
                argv(&target, cwd, depth + 1, out);
                return;
            }
        }
    }
    record_interpreter(name, rest, out);
    direct_family(name, rest, cwd, out);
    if name == "git" {
        git_alias(rest, cwd, depth, out);
    }
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
            let cluster = arg
                .strip_prefix('-')
                .filter(|s| !s.starts_with('-'))
                .is_some_and(|s| {
                    s.chars().all(|c| c.is_ascii_alphabetic())
                        && (!arg.starts_with(flag)
                            || arg == flag
                            || (name.starts_with("node") && s == "ep"))
                        && match name {
                            "perl" | "ruby" => s.contains(['e', 'E']),
                            "node" | "nodejs" => s.contains(['e', 'p']),
                            _ => s.ends_with('c'),
                        }
                });
            if let Some(code) = (name == "perl").then(|| perl_attached_code(arg)).flatten() {
                out.code.push(code.to_string());
                found = true;
            } else if cluster
                || arg == flag
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

// Only cross flags that do not consume a value. In particular, the `e` in
// `-Mfeature` or a backup suffix is not an evaluation switch.
fn perl_attached_code(arg: &str) -> Option<&str> {
    let flags = arg.strip_prefix('-')?;
    for (at, flag) in flags.char_indices() {
        match flag {
            'e' | 'E' => return flags.get(at + 1..).filter(|code| !code.is_empty()),
            'a' | 'c' | 'f' | 'l' | 'n' | 'p' | 's' | 't' | 'T' | 'u' | 'U' | 'w' | 'W' | 'X' => {}
            _ => return None,
        }
    }
    None
}

fn direct_family(name: &str, rest: &[String], cwd: Option<&Path>, out: &mut Parsed) {
    let source = (name == "git"
        && rest
            .iter()
            .any(|s| s == "-C" || s == "--git-dir" || s.starts_with("--git-dir=")))
    .then(|| git_policy_root(cwd, rest))
    .flatten();
    let mut canonical = vec![name.to_string()];
    canonical.extend(global_args(name, rest).iter().cloned());
    for family in &table().families {
        if family
            .codex
            .iter()
            .any(|rule| prefix(&canonical, &rule.pattern))
        {
            out.families.push((family_id(&family.id), source.clone()));
        }
    }
    let sub = canonical.get(1).map_or("", String::as_str);
    let body = canonical.get(2..).unwrap_or_default();
    if name == "git" && sub == "tag" {
        let tag_args = skip_options(
            body,
            &["-m", "--message", "-F", "--file", "-u", "--local-user"],
        );
        if !body.iter().any(|a| {
            matches!(
                a.as_str(),
                "-d" | "--delete" | "-l" | "--list" | "-v" | "--verify"
            )
        }) {
            if let (Some(dir), Some(tag)) = (git_directory(cwd, rest), tag_args.first()) {
                out.created_tags.push((dir, tag.clone()));
            }
        }
    }
    if name == "git" && sub == "update-ref" {
        if let (Some(dir), Some(tag)) = (
            git_directory(cwd, rest),
            body.iter().find_map(|a| a.strip_prefix("refs/tags/")),
        ) {
            out.created_tags.push((dir, tag.to_owned()));
        }
    }
    if name == "git" && sub == "config" && enables_follow_tags(body) {
        out.families.push(("release", source.clone()));
    }
    if name == "git" && sub == "push" {
        if body.iter().any(|a| a == "--mirror") {
            out.families.push(("account", source.clone()));
        }
        if body.iter().any(|a| {
            matches!(a.as_str(), "--tags" | "--follow-tags" | "tag") || a.contains("refs/tags/")
        }) || tag_push(body, cwd, rest, &out.created_tags)
        {
            out.families.push(("release", source.clone()));
        } else if follow_tags(cwd, rest) {
            out.families.push(("configured_tag_push", source.clone()));
        }
    }
    if name == "gh" {
        if sub == "repo"
            && body.first().is_some_and(|s| s == "edit")
            && body
                .iter()
                .any(|s| s == "--visibility" || s.starts_with("--visibility="))
        {
            out.families.push(("account", source.clone()));
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
            out.families.push(("account", source.clone()));
        }
    }
    if name.starts_with("git-credential-") {
        out.families.push(("account", source.clone()));
    }
    if matches!(name, "runuser" | "gsudo.exe" | "runas.exe")
        || (name == "osascript" && rest.join(" ").contains("with administrator privileges"))
    {
        out.families.push(("privilege", source.clone()));
    }
    if name.eq_ignore_ascii_case("Set-ExecutionPolicy")
        || (name == "crontab"
            && !rest
                .iter()
                .any(|a| matches!(a.as_str(), "-l" | "--help" | "-h")))
    {
        out.families.push(("persistence", source.clone()));
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

fn git_policy_root(cwd: Option<&Path>, globals: &[String]) -> Option<PathBuf> {
    let mut dir = cwd?.to_path_buf();
    let mut git_dir = None;
    let args = global_args("git", globals);
    let end = globals.len() - args.len();
    let mut at = 0;
    while at < end {
        match globals[at].as_str() {
            "-C" => {
                dir = dir.join(globals.get(at + 1)?);
                at += 1;
            }
            "--git-dir" => {
                git_dir = Some(globals.get(at + 1)?.clone());
                at += 1;
            }
            value => {
                if let Some(path) = value.strip_prefix("--git-dir=") {
                    git_dir = Some(path.to_owned());
                }
            }
        }
        at += 1;
    }
    let repo = if let Some(path) = git_dir {
        git2::Repository::open(dir.join(path))
    } else {
        git2::Repository::discover(dir)
    }
    .ok()?;
    Some(repo.workdir().unwrap_or(repo.path()).to_path_buf())
}

fn git_directory(cwd: Option<&Path>, globals: &[String]) -> Option<PathBuf> {
    let root = git_policy_root(cwd, globals)?;
    git2::Repository::discover(root)
        .ok()?
        .commondir()
        .canonicalize()
        .ok()
}

fn tag_push(
    args: &[String],
    cwd: Option<&Path>,
    globals: &[String],
    created: &[(PathBuf, String)],
) -> bool {
    let Some(dir) = git_directory(cwd, globals) else {
        return false;
    };
    let Ok(repo) = git2::Repository::open(&dir) else {
        return false;
    };
    args.iter().filter(|arg| !arg.starts_with('-')).any(|arg| {
        arg.trim_start_matches('+').split(':').any(|part| {
            let shorthand = part
                .strip_prefix("tags/")
                .or_else(|| part.strip_prefix("refs/tags/"));
            [Some(part), shorthand]
                .into_iter()
                .flatten()
                .any(|tag_name| {
                    !tag_name.is_empty()
                        && (repo
                            .find_reference(&format!("refs/tags/{tag_name}"))
                            .is_ok()
                            || created
                                .iter()
                                .any(|(at, tag)| at == &dir && tag == tag_name))
                })
        })
    })
}

fn truthy(value: &str) -> bool {
    !matches!(
        value.to_ascii_lowercase().as_str(),
        "false" | "no" | "off" | "0"
    )
}

fn enables_follow_tags(args: &[String]) -> bool {
    if args.iter().any(|a| {
        matches!(
            a.as_str(),
            "--get"
                | "--get-all"
                | "--get-regexp"
                | "--list"
                | "-l"
                | "--unset"
                | "--unset-all"
                | "get"
                | "list"
                | "unset"
        )
    }) {
        return false;
    }
    args.windows(2)
        .any(|p| p[0].eq_ignore_ascii_case("push.followTags") && truthy(&p[1]))
}

fn follow_tags(cwd: Option<&Path>, globals: &[String]) -> bool {
    let sub = global_args("git", globals);
    if let Some(option) = sub
        .iter()
        .rev()
        .find(|s| matches!(s.as_str(), "--follow-tags" | "--no-follow-tags"))
    {
        return option == "--follow-tags";
    }
    let end = globals.len() - sub.len();
    let override_value = globals[..end]
        .windows(2)
        .filter(|p| p[0] == "-c")
        .map(|p| p[1].split_once('=').unwrap_or((&p[1], "true")))
        .filter(|(key, _)| key.eq_ignore_ascii_case("push.followTags"))
        .map(|(_, value)| truthy(value))
        .next_back();
    override_value.unwrap_or_else(|| {
        git_directory(cwd, globals)
            .and_then(|dir| git2::Repository::open(dir).ok())
            .and_then(|repo| repo.config().ok())
            .and_then(|config| config.get_bool("push.followTags").ok())
            .unwrap_or(false)
    })
}

fn git_alias(rest: &[String], cwd: Option<&Path>, depth: usize, out: &mut Parsed) {
    let sub = global_args("git", rest);
    let Some(name) = sub.first() else {
        return;
    };
    if crate::hooks::git_guard::GIT_BUILTINS.contains(&name.as_str()) {
        return;
    }
    let Some(dir) = git_directory(cwd, rest) else {
        return;
    };
    let end = rest.len() - sub.len();
    let inline = rest[..end]
        .windows(2)
        .filter(|p| p[0] == "-c")
        .filter_map(|p| p[1].split_once('='))
        .rfind(|(key, _)| *key == format!("alias.{name}"))
        .map(|(_, value)| value.to_string());
    let value = inline.or_else(|| {
        git2::Repository::open(&dir)
            .ok()?
            .config()
            .ok()?
            .get_string(&format!("alias.{name}"))
            .ok()
    });
    if let Some(value) = value.filter(|v| !v.starts_with('!')) {
        let mut args = vec!["git".to_string()];
        args.extend_from_slice(&rest[..end]);
        args.extend(command_argv(&value));
        args.extend_from_slice(&sub[1..]);
        argv(&args, cwd, depth + 1, out);
    }
}

// Keep pipeline boundaries and quoted producer text. Only a shell that reads
// stdin makes that text code; ordinary pipes and separate commands stay data.
fn shell_inputs(command: &str, out: &mut Parsed) {
    let mut quote = None;
    let mut escaped = false;
    let mut start = 0;
    let mut producers = String::new();
    for (at, ch) in command
        .char_indices()
        .chain(std::iter::once((command.len(), ';')))
    {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && quote != Some('\'') {
            escaped = true;
            continue;
        }
        if let Some(q) = quote {
            if ch == q {
                quote = None;
            }
            continue;
        }
        if matches!(ch, '\'' | '"') {
            quote = Some(ch);
            continue;
        }
        if !matches!(ch, '|' | ';' | '&' | '\n') {
            continue;
        }
        let segment = &command[start..at];
        let words = command_argv(segment);
        if let Some((program, rest)) = strip_launchers(&words) {
            let name = program.rsplit('/').next().unwrap_or(program);
            if matches!(name, "sh" | "bash" | "zsh" | "dash" | "ksh" | "ash") {
                if let Some(script) = shell_command_string(rest) {
                    if script.contains("$(") || script.contains('`') {
                        out.code.push(script.to_string());
                    }
                } else if !producers.is_empty()
                    && (rest.iter().all(|a| a.starts_with('-'))
                        || rest.iter().take_while(|a| a.as_str() != "--").any(|a| {
                            a.strip_prefix('-')
                                .is_some_and(|flags| !flags.starts_with('-') && flags.contains('s'))
                        }))
                {
                    out.code.push(producers.clone());
                }
            }
        }
        if ch == '|' && !command[at..].starts_with("||") {
            producers.push_str(segment);
            producers.push(' ');
        } else {
            producers.clear();
        }
        start = at + ch.len_utf8();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::actions::{table, PatternToken};

    #[test]
    fn n1_tag_shorthand_matches_existing_and_new_tags() {
        let temp = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(temp.path()).unwrap();
        let id = repo.blob(b"tag target").unwrap();
        repo.tag_lightweight("v1.2.3", &repo.find_object(id, None).unwrap(), false)
            .unwrap();
        for command in [
            "git push origin tags/v1.2.3",
            "git push origin +tags/v1.2.3",
            "git push origin refs/tags/v1.2.3",
            "git tag v9 && git push origin tags/v9",
            "git tag v9 && git push origin HEAD:tags/v9",
        ] {
            assert!(
                !evaluate_at(command, &SecuritySection::default(), Some(temp.path())).is_empty(),
                "{command}"
            );
        }
        for command in [
            "git push origin tags-x",
            "git push origin tags/task/x",
            "git tag v9 && git push origin tags-x",
        ] {
            assert!(
                evaluate_at(command, &SecuritySection::default(), Some(temp.path())).is_empty(),
                "{command}"
            );
        }
    }

    #[test]
    fn n2_valueless_follow_tags_is_true() {
        for command in [
            "git -c push.followTags push origin task/x",
            "git -c PUSH.FOLLOWTAGS push origin task/x",
        ] {
            assert!(
                !evaluate(command, &SecuritySection::default()).is_empty(),
                "{command}"
            );
        }
        for command in [
            "git -c push.followTags=false push origin task/x",
            "git -c push.followTags -c push.followTags=false push origin task/x",
            "git -c push.followTags push --no-follow-tags origin task/x",
        ] {
            assert!(
                evaluate(command, &SecuritySection::default()).is_empty(),
                "{command}"
            );
        }
    }

    #[test]
    fn n5_perl_code_attached_to_clusters() {
        for command in [
            r#"perl -pe'system("cargo publish")'"#,
            r#"perl -E'system("cargo publish")'"#,
            r#"perl -lwe'system("cargo publish")'"#,
        ] {
            assert!(
                !evaluate(command, &SecuritySection::default()).is_empty(),
                "{command}"
            );
        }
        for command in [
            "perl -pe's/a/b/' README.md",
            "perl -lwe'print 1'",
            "perl -Mfeature -pe'print' README.md",
            "perl -i.backup -pe's/a/b/' README.md",
        ] {
            assert!(
                evaluate(command, &SecuritySection::default()).is_empty(),
                "{command}"
            );
        }
    }

    #[test]
    fn n6_effective_follow_tags_names_cause_and_route() {
        let temp = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(temp.path()).unwrap();
        repo.config()
            .unwrap()
            .set_bool("push.followTags", true)
            .unwrap();
        let findings = evaluate_at(
            "git push origin task/x",
            &SecuritySection::default(),
            Some(temp.path()),
        );
        assert_eq!(findings.len(), 1);
        assert!(
            findings[0].message.contains("effective")
                && findings[0].message.contains("push.followTags"),
            "{findings:?}"
        );
        assert!(
            findings[0].remedy.contains("--no-follow-tags"),
            "{findings:?}"
        );
        assert!(evaluate_at(
            "git push --no-follow-tags origin task/x",
            &SecuritySection::default(),
            Some(temp.path())
        )
        .is_empty());
        let direct = evaluate_at(
            "git push origin refs/tags/v1",
            &SecuritySection::default(),
            Some(temp.path()),
        );
        assert!(!direct[0].remedy.contains("--no-follow-tags"), "{direct:?}");
    }

    #[test]
    fn f1_tags_created_earlier_are_release_refspecs() {
        let temp = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(temp.path()).unwrap();
        repo.config().unwrap().set_str("alias.mark", "tag").unwrap();
        for command in [
            "git tag v9 && git push origin v9",
            "git tag -a v9 -m x && git push origin +v9",
            "git mark v9 && git push origin v9",
            "git -c alias.mark=tag mark v9 && git push origin v9",
            "git tag v9 && bash -c 'git push origin v9'",
        ] {
            assert!(
                !evaluate_at(command, &SecuritySection::default(), Some(temp.path())).is_empty(),
                "{command}"
            );
        }
        assert!(evaluate_at(
            "git tag v9 && git push origin task/x",
            &SecuritySection::default(),
            Some(temp.path())
        )
        .is_empty());
        let other = temp.path().join("other");
        git2::Repository::init(&other).unwrap();
        for command in [
            "git -C other tag v9 && git -C other push origin v9",
            "cd other && git tag v9 && git push origin v9",
        ] {
            assert!(
                !evaluate_at(command, &SecuritySection::default(), Some(temp.path())).is_empty(),
                "{command}"
            );
        }
        assert!(evaluate_at(
            "git -C other tag v9 && git push origin v9",
            &SecuritySection::default(),
            Some(temp.path())
        )
        .is_empty());
    }

    #[test]
    fn f2_shell_code_from_stdin_and_substitution() {
        for command in [
            "echo 'cargo publish' | sh",
            "printf 'npm publish' | bash",
            "echo 'cargo publish' | env sh -s arg",
            r#"bash -c "$(printf 'cargo publish')""#,
        ] {
            assert!(
                !evaluate(command, &SecuritySection::default()).is_empty(),
                "{command}"
            );
        }
        for command in [
            "echo hello | sh -c 'cat'",
            "echo 'cargo publish' | cat",
            "echo 'cargo publish'; sh",
            "echo 'cargo publish' | sh script.sh",
        ] {
            assert!(
                evaluate(command, &SecuritySection::default()).is_empty(),
                "{command}"
            );
        }
    }

    #[test]
    fn f3_follow_tags_configuration() {
        for command in [
            "git -c push.followTags=true push origin task/x",
            "git -c PUSH.FOLLOWTAGS=on push origin task/x",
            "git config push.followTags true",
            "git config --global push.followTags true",
        ] {
            assert!(
                !evaluate(command, &SecuritySection::default()).is_empty(),
                "{command}"
            );
        }
        for command in [
            "git -c push.followTags=false push origin task/x",
            "git config --get push.followTags",
            "git config push.followTags",
            "git config push.followTags false",
        ] {
            assert!(
                evaluate(command, &SecuritySection::default()).is_empty(),
                "{command}"
            );
        }
        let temp = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(temp.path()).unwrap();
        repo.config()
            .unwrap()
            .set_bool("push.followTags", true)
            .unwrap();
        assert!(!evaluate_at(
            "git push origin task/x",
            &SecuritySection::default(),
            Some(temp.path())
        )
        .is_empty());
        assert!(evaluate_at(
            "git -c push.followTags=false push origin task/x",
            &SecuritySection::default(),
            Some(temp.path())
        )
        .is_empty());
    }

    #[test]
    fn f5_interpreter_clusters() {
        for flag in [
            "perl -le",
            "perl -E",
            "perl -ne",
            "perl -pe",
            "ruby -ne",
            "node -pe",
            "python3 -Ic",
        ] {
            let command = format!("{flag} 'run(\"cargo publish\")'");
            assert!(
                !evaluate(&command, &SecuritySection::default()).is_empty(),
                "{command}"
            );
        }
        assert!(evaluate("perl -ne 'print' file", &SecuritySection::default()).is_empty());
    }

    #[test]
    fn f6_crontab_stdin_and_read_controls() {
        for command in ["crontab", "echo '* * * * * x' | crontab"] {
            assert!(
                !evaluate(command, &SecuritySection::default()).is_empty(),
                "{command}"
            );
        }
        for command in ["crontab -l", "crontab -h", "crontab --help"] {
            assert!(
                evaluate(command, &SecuritySection::default()).is_empty(),
                "{command}"
            );
        }
    }

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
