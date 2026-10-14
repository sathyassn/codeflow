//! Conservative literal inspection inside interpreter code, never ordinary
//! data, and of the arguments an interpreter is given (TSK-302).
use super::{actions, outward, pattern::matches_extended_glob};
use crate::hooks::{edit_guard, git_guard, PolicyLevel, SecuritySection, Violation};
use std::path::{Path, PathBuf};

pub(crate) fn evaluate(
    command: &str,
    levels: &SecuritySection,
    integrity: PolicyLevel,
    cwd: &Path,
    root: &Path,
    home: Option<&Path>,
) -> Vec<Violation> {
    let mut violations = Vec::new();
    if levels.secret_reads.is_active() {
        for args in outward::unwrapped_commands(command) {
            let name = args
                .first()
                .map_or("", |s| s.rsplit('/').next().unwrap_or(s));
            if matches!(
                name,
                "cat"
                    | "head"
                    | "tail"
                    | "less"
                    | "more"
                    | "bat"
                    | "sed"
                    | "awk"
                    | "grep"
                    | "rg"
                    | "cp"
                    | "base64"
                    | "Get-Content"
            ) {
                if let Some(path) = args.iter().skip(1).find(|word| secret_path(word, home)) {
                    violations.push(Violation::new(
                        "security.secret_reads",
                        levels.secret_reads,
                        format!("keychain or secret-store read names `{path}`"),
                        crate::remedy::OUTWARD_ACTION.remedy(),
                    ));
                }
            }
        }
    }
    for code in outward::interpreter_bodies(command) {
        if levels.headless_peer_runs.is_active() {
            if let Some(run) = super::headless::raw_run(&code) {
                violations.push(crate::hooks::exec_guard::headless_violation(
                    levels.headless_peer_runs,
                    &run,
                ));
            }
        }
        for word in outward::literal_words(&code) {
            if levels.secret_reads.is_active() && secret_path(&word, home) {
                violations.push(Violation::new(
                    "security.secret_reads",
                    levels.secret_reads,
                    format!("interpreter code names the secret store `{word}`"),
                    crate::remedy::OUTWARD_ACTION.remedy(),
                ));
            }
            // Only path-like literals need filesystem metadata. Ordinary code
            // identifiers cannot be enforcement paths.
            let authority = (word.contains('/') || word.starts_with('.'))
                && crate::hooks::edit_guard::repository_authority_target(
                    &cwd.join(&word),
                    root,
                    false,
                );
            if authority {
                violations.push(Violation::always_blocking("git.hook_integrity",
                    format!("interpreter code names policy authority metadata `{word}`"),
                    "the operator manages remote-tracking authority; use the configured remote's ordinary fetch"));
                continue;
            }
            if integrity.is_active() && (word.contains('/') || word.starts_with('.')) {
                match crate::hooks::edit_guard::enforcement_path(&word, cwd, root, home) {
                    Ok(false) => {}
                    result => violations.push(Violation::new(
                        "git.hook_integrity",
                        integrity,
                        match result {
                            Ok(true) => {
                                format!("interpreter code names the enforcement path `{word}`")
                            }
                            Err(why) => format!("cannot inspect interpreter path `{word}`: {why}"),
                            Ok(false) => unreachable!(),
                        },
                        crate::remedy::HOOK_INTEGRITY.remedy(),
                    )),
                }
            }
        }
    }
    for call in outward::interpreter_calls(command, cwd) {
        violations.extend(argument_violations(
            command, &call, integrity, cwd, root, home,
        ));
    }
    violations
}

/// The words of one interpreter argument that can name a file: the word
/// itself, or the value of an option written `--name=value`. A bare option,
/// stdin's `-` and `--` name none.
fn argument_value(word: &str) -> Option<&str> {
    if word.is_empty() || word == "-" || word == "--" {
        return None;
    }
    if word.starts_with('-') {
        return word
            .split_once('=')
            .map(|(_, value)| value)
            .filter(|v| !v.is_empty());
    }
    Some(word)
}

fn argument_violation(level: PolicyLevel, message: String) -> Violation {
    Violation::new(
        "git.hook_integrity",
        level,
        message,
        crate::remedy::INTERPRETER_ARGUMENT.remedy(),
    )
}

/// Judge the words an interpreter call is given as its code words are
/// judged (issue 173): an argument naming policy authority metadata always
/// refuses, and one naming an enforcement path refuses under
/// `git.hook_integrity`. An argument the guard cannot read, filled in at
/// run time or used from a directory it cannot follow, takes git-guard's
/// rule for writer operands: it refuses when its spelled part could name an
/// enforcement path or when the line names one. Arguments `xargs` or
/// `find -exec` supplies are judged the same way, `find`'s by the
/// enforcement files its starting points reach. A program that builds a
/// path internally is past this reading; the OS sandbox is the backstop.
fn argument_violations(
    line: &str,
    call: &outward::InterpreterCall,
    integrity: PolicyLevel,
    payload_cwd: &Path,
    root: &Path,
    home: Option<&Path>,
) -> Vec<Violation> {
    let mut found = Vec::new();
    // Literal values, with the directory they are read from.
    let mut literal: Vec<(&Path, &str)> = Vec::new();
    for word in &call.args {
        // `find` and `xargs -I {}` fill in `{}`; what they supply is
        // judged below.
        if call.supplier.is_some() && word.contains("{}") {
            continue;
        }
        let Some(value) = argument_value(word) else {
            continue;
        };
        // `~user` and `~-` are filled in by the shell too.
        let unresolved = git_guard::unresolved_word(value)
            || (value.starts_with('~') && !value.starts_with("~/") && value != "~");
        let absolute = value.starts_with('/') || value.starts_with("~/") || value == "~";
        match (
            unresolved,
            call.cwd.as_deref().or(absolute.then_some(payload_cwd)),
        ) {
            (false, Some(dir)) => literal.push((dir, value)),
            _ if integrity.is_active() => found.extend(unreadable_argument(
                line,
                value,
                call.cwd.as_deref(),
                unresolved,
                integrity,
                payload_cwd,
            )),
            _ => {}
        }
    }
    let mut names = None;
    for (dir, value) in literal {
        if integrity.is_active() && (git_guard::has_glob(value) || value.contains('{')) {
            if let Some(p) = git_guard::token_integrity_path(value, dir, payload_cwd) {
                found.push(argument_violation(
                    integrity,
                    format!("an interpreter argument `{value}` reaches the enforcement path `{p}`"),
                ));
                continue;
            }
        }
        // The full checks read every directory on the path; a value that
        // passes through no protected name is cleared first.
        let names = names.get_or_insert_with(|| ProtectedNames::load(root, home));
        let target = literal_target(value, dir, home);
        if !names.may_reach(&target) {
            continue;
        }
        if edit_guard::repository_authority_target(&target, root, false) {
            found.push(Violation::always_blocking(
                "git.hook_integrity",
                format!("an interpreter argument names policy authority metadata `{value}`"),
                "the operator manages remote-tracking authority; use the configured remote's ordinary fetch",
            ));
            continue;
        }
        if !integrity.is_active() {
            continue;
        }
        match edit_guard::enforcement_path(value, dir, root, home) {
            Ok(false) => {}
            Ok(true) => found.push(argument_violation(
                integrity,
                format!("an interpreter argument names the enforcement path `{value}`"),
            )),
            Err(why) => found.push(argument_violation(
                integrity,
                format!("cannot inspect interpreter argument `{value}`: {why}"),
            )),
        }
    }
    if integrity.is_active() {
        found.extend(supplied_violation(line, call, integrity, payload_cwd));
    }
    found
}

/// The file a literal argument names from `dir`.
fn literal_target(value: &str, dir: &Path, home: Option<&Path>) -> PathBuf {
    match (
        value.strip_prefix("~/").or((value == "~").then_some("")),
        home,
    ) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => dir.join(value),
    }
}

/// A name as compared here: Unicode NFC, lower case, so a spelling a
/// case-insensitive or normalizing file system reads as the same name
/// matches. `None` for a name that is not valid UTF-8, which the caller
/// sends to the full checks.
fn folded(name: &std::ffi::OsStr) -> Option<String> {
    use unicode_normalization::UnicodeNormalization as _;
    Some(name.to_str()?.nfc().collect::<String>().to_lowercase())
}

/// `path` with the symbolic links on its existing part resolved: the
/// longest prefix that exists, canonicalized, then the rest as spelled.
/// `None` when that cannot be read.
fn resolved_existing(path: &Path) -> Option<PathBuf> {
    let mut existing = path.to_path_buf();
    let mut rest = Vec::new();
    while std::fs::symlink_metadata(&existing).is_err() {
        rest.push(existing.file_name()?.to_owned());
        if !existing.pop() {
            return None;
        }
    }
    let mut resolved = std::fs::canonicalize(&existing).ok()?;
    resolved.extend(rest.iter().rev());
    Some(resolved)
}

/// The names a path passes through when it is an enforcement path or
/// policy authority metadata: the first component of every protected
/// pattern in the action table, the repository's common directory, and
/// the git configuration files. A path that passes through none of them,
/// read as spelled and with its symbolic links resolved, is neither, so
/// the full checks, which read every directory on the path, are skipped
/// for it. An argument list is judged in about the time one path takes
/// (TSK-302).
struct ProtectedNames {
    names: Vec<String>,
    /// Git configuration files named by full path (`XDG_CONFIG_HOME`,
    /// `GIT_CONFIG_GLOBAL`), each folded, as spelled and resolved.
    configs: Vec<String>,
    /// A protected pattern this reading does not know: every path takes
    /// the full checks.
    unknown: bool,
}

impl ProtectedNames {
    fn load(root: &Path, home: Option<&Path>) -> Self {
        let mut unknown = false;
        let mut names: Vec<String> = Vec::new();
        for rule in &actions::table().claude_edit_denies {
            let first = rule
                .strip_prefix("Edit(")
                .and_then(|s| s.strip_suffix(')'))
                .and_then(|s| s.strip_prefix("~/").or_else(|| s.strip_prefix('/')))
                .and_then(|s| s.split('/').next())
                .filter(|s| !s.is_empty() && !git_guard::has_glob(s));
            match first.and_then(|first| folded(std::ffi::OsStr::new(first))) {
                Some(first) => names.push(first),
                None => unknown = true,
            }
        }
        for name in [".git", ".gitconfig", "gitconfig"] {
            names.push(name.to_string());
        }
        match crate::hooks::repo::open(root) {
            Ok(Some(repo)) => match repo.commondir().components().next_back() {
                Some(std::path::Component::Normal(name)) => match folded(name) {
                    Some(name) => names.push(name),
                    None => unknown = true,
                },
                _ => unknown = true,
            },
            Ok(None) => {}
            Err(_) => unknown = true,
        }
        let mut configs: Vec<PathBuf> = crate::portable_path::user_homes()
            .into_iter()
            .chain(home.map(Path::to_path_buf))
            .map(|home| home.join(".config/git/config"))
            .collect();
        if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
            configs.push(PathBuf::from(xdg).join("git/config"));
        }
        for name in ["GIT_CONFIG_GLOBAL", "GIT_CONFIG_SYSTEM"] {
            if let Some(path) = std::env::var_os(name).filter(|s| !s.is_empty()) {
                configs.push(root.join(path));
            }
        }
        let mut folded_configs = Vec::new();
        for path in configs.iter().flat_map(|path| {
            [
                edit_guard::normalized(path, false).ok(),
                resolved_existing(path),
            ]
        }) {
            match path.as_deref().map(Path::as_os_str).map(folded) {
                Some(Some(path)) => folded_configs.push(path),
                // A configuration path that cannot be read sends every
                // path to the full checks.
                _ => unknown = true,
            }
        }
        Self {
            names,
            configs: folded_configs,
            unknown,
        }
    }

    /// Whether `target` may be an enforcement path or authority metadata,
    /// so the full checks must judge it.
    fn may_reach(&self, target: &Path) -> bool {
        // `..` after a symbolic link leaves where the spelling says; the
        // full checks read it.
        if self.unknown
            || target
                .components()
                .any(|c| c == std::path::Component::ParentDir)
        {
            return true;
        }
        let Some((lexical, resolved)) = edit_guard::normalized(target, false)
            .ok()
            .and_then(|lexical| resolved_existing(&lexical).map(|resolved| (lexical, resolved)))
        else {
            return true;
        };
        [lexical, resolved].iter().any(|path| {
            path.components().any(|component| match component {
                std::path::Component::Normal(name) => {
                    folded(name).is_none_or(|name| self.names.contains(&name))
                }
                _ => false,
            }) || folded(path.as_os_str()).is_none_or(|path| self.configs.contains(&path))
        })
    }
}

/// An argument the guard cannot read: part of the value, or the directory
/// it is read from, is filled in at run time. Its spelled part is read by
/// its names, then the line is read for an enforcement path, as git-guard
/// reads a writer's operand.
fn unreadable_argument(
    line: &str,
    value: &str,
    dir: Option<&Path>,
    unresolved: bool,
    integrity: PolicyLevel,
    payload_cwd: &Path,
) -> Option<Violation> {
    let shown = git_guard::shown_word(value);
    let reach = match dir {
        Some(dir) => git_guard::token_integrity_path(value, dir, payload_cwd),
        None => git_guard::word_could_name(value),
    };
    if let Some(p) = reach {
        let why = if unresolved {
            "part of it is filled in at run time"
        } else {
            "it is used from a directory the guard cannot follow"
        };
        return Some(argument_violation(
            integrity,
            format!(
                "an interpreter argument `{shown}` could name the enforcement path `{p}`: {why}"
            ),
        ));
    }
    if !unresolved {
        return None;
    }
    let p = git_guard::line_names(line, dir.unwrap_or(payload_cwd), payload_cwd)?;
    Some(argument_violation(
        integrity,
        format!(
            "an interpreter argument `{shown}` is filled in at run time on a line that names the enforcement path `{p}`"
        ),
    ))
}

/// The arguments `xargs` or `find -exec` gives an interpreter: `xargs`
/// reads them at run time, so they refuse on a line that names an
/// enforcement path; `find` gives each path its starting points reach and
/// its `-name` tests let through, so they refuse when one is an
/// enforcement path.
fn supplied_violation(
    line: &str,
    call: &outward::InterpreterCall,
    integrity: PolicyLevel,
    payload_cwd: &Path,
) -> Option<Violation> {
    match call.supplier.as_ref()? {
        outward::Supplier::Xargs => {
            let dir = call.cwd.as_deref().unwrap_or(payload_cwd);
            let p = git_guard::line_names(line, dir, payload_cwd)?;
            Some(argument_violation(
                integrity,
                format!("`xargs` gives an interpreter arguments it reads at run time, on a line that names the enforcement path `{p}`"),
            ))
        }
        outward::Supplier::Find {
            starts,
            before,
            cwd,
        } => {
            // `find` hands its paths over as `{}`, or to a shell `-c`
            // string that passes them on as its positional parameters.
            if !call
                .args
                .iter()
                .any(|word| word.contains("{}") || git_guard::unresolved_word(word))
            {
                return None;
            }
            let dir = cwd.as_deref().unwrap_or(payload_cwd);
            if let Some(p) = git_guard::line_names(line, dir, payload_cwd) {
                return Some(argument_violation(
                    integrity,
                    format!("`find` gives an interpreter paths on a line that names the enforcement path `{p}`"),
                ));
            }
            let filter = git_guard::find_name_filter(before);
            for start in starts {
                let path = match cwd {
                    Some(dir) if !git_guard::unresolved_word(start) => dir.join(start),
                    // The line check above covers a start the guard cannot read.
                    _ => continue,
                };
                let candidates = match edit_guard::find_candidates(&path, payload_cwd) {
                    Ok(candidates) => candidates,
                    Err(error) => {
                        return Some(argument_violation(
                            integrity,
                            format!("cannot read the paths `find` gives an interpreter: {error}"),
                        ))
                    }
                };
                // The candidates are the enforcement paths and checkouts
                // under the start, and the directories leading to them.
                if let Some(candidate) = candidates.iter().find(|candidate| {
                    git_guard::find_name_matches(filter.as_deref(), candidate)
                        && edit_guard::repository_enforcement_target(candidate, payload_cwd, false)
                            .unwrap_or(true)
                }) {
                    return Some(argument_violation(
                        integrity,
                        format!(
                            "`find` can give an interpreter the enforcement path `{}`",
                            candidate.display()
                        ),
                    ));
                }
            }
            None
        }
    }
}

fn secret_path(word: &str, home: Option<&Path>) -> bool {
    let expanded_word = word
        .strip_prefix("$HOME/")
        .or_else(|| word.strip_prefix("${HOME}/"))
        .map(|suffix| format!("~/{suffix}"));
    let word = expanded_word.as_deref().unwrap_or(word);
    actions::table().sandbox_read_denies.iter().any(|pattern| {
        let expanded = pattern
            .strip_prefix("~/")
            .and_then(|p| home.and_then(|home| home.join(p).to_str().map(str::to_string)));
        let matches = [Some(pattern.as_str()), expanded.as_deref()]
            .into_iter()
            .flatten()
            .any(|path| {
                word == path
                    || word
                        .strip_prefix(path)
                        .is_some_and(|tail| tail.starts_with('/'))
            });
        matches
    }) || actions::table().claude_read_groups.iter().any(|group| {
        // This group has no carve-outs; source-name and env-file groups do.
        group.carveouts.is_empty()
            && group.denies.iter().any(|rule| {
                let Some(pattern) = rule.strip_prefix("Read(").and_then(|s| s.strip_suffix(')'))
                else {
                    return false;
                };
                let expanded = pattern
                    .strip_prefix("~/")
                    .and_then(|p| home.and_then(|home| home.join(p).to_str().map(str::to_string)));
                matches_extended_glob(word, pattern)
                    || expanded.is_some_and(|p| matches_extended_glob(word, &p))
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn check(code: &str, levels: &SecuritySection) -> Vec<Violation> {
        evaluate(
            code,
            levels,
            PolicyLevel::Block,
            Path::new("/fixture"),
            Path::new("/fixture"),
            Some(Path::new("/fixture-home")),
        )
    }

    #[test]
    fn f15_home_variable_secret_spellings() {
        for command in [
            r"cat $HOME/.codex/auth.json",
            r"cat ${HOME}/.codex/auth.json",
            r#"python3 -c 'open("$HOME/.codex/auth.json")'"#,
            r#"python3 -c 'open("${HOME}/.codex/auth.json")'"#,
        ] {
            assert!(
                check(command, &SecuritySection::default())
                    .iter()
                    .any(|v| v.rule == "security.secret_reads"),
                "{command}"
            );
        }
        assert!(check("cat $HOME/notes.md", &SecuritySection::default()).is_empty());
    }

    #[test]
    fn interpreter_headless_paths_and_secret_pairs() {
        for command in [
            "python3 -c 'run(\"claude\", \"-p\", \"x\")'",
            "node -e 'exec(\"codex exec x\")'",
            "ruby -e 'system(\"grok -p x\")'",
            "perl -e 'system(\"claude -p x\")'",
            "python3 <<'PY'\nrun('codex exec x')\nPY",
        ] {
            let findings = check(command, &SecuritySection::default());
            assert!(
                findings
                    .iter()
                    .any(|v| v.rule == "security.headless_peer_runs"),
                "{command}: {findings:?}"
            );
            let off = SecuritySection {
                headless_peer_runs: PolicyLevel::Off,
                ..SecuritySection::default()
            };
            assert!(check(command, &off).is_empty(), "{command}");
        }
        for store in &actions::table().sandbox_read_denies {
            let command = format!("python3 -c 'open(\"{store}\")'");
            assert!(
                check(&command, &SecuritySection::default())
                    .iter()
                    .any(|v| v.rule == "security.secret_reads"),
                "{command}"
            );
        }
        for command in [
            "cat ~/.codex/auth.json",
            "nohup cat ~/.grok/auth.json",
            "bash -c 'cat ~/.pypirc'",
        ] {
            assert!(
                check(command, &SecuritySection::default())
                    .iter()
                    .any(|v| v.rule == "security.secret_reads"),
                "{command}"
            );
        }
        assert!(check(
            "node -e 'write(\".codeflow/policy.json\")'",
            &SecuritySection::default()
        )
        .iter()
        .any(|v| v.rule == "git.hook_integrity"));
        for command in [
            "cat README.md",
            "cat .env.example",
            "rg 'claude -p' assets/",
            "python3 -c 'print(\"hello\")'",
            "cat <<'EOF'\nclaude -p x\nEOF",
        ] {
            assert!(
                check(command, &SecuritySection::default()).is_empty(),
                "{command}"
            );
        }
    }

    /// A repository with enforcement files, a linked worktree at
    /// `.worktrees/w` and a home, at canonical paths.
    struct Repo {
        _temp: tempfile::TempDir,
        root: PathBuf,
        linked: PathBuf,
        home: PathBuf,
    }

    fn repo() -> Repo {
        let temp = tempfile::tempdir().unwrap();
        let base = std::fs::canonicalize(temp.path()).unwrap();
        let root = base.join("repo");
        let home = base.join("home");
        for dir in [".claude", ".codeflow", "src"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(root.join(".claude/settings.json"), "{}\n").unwrap();
        std::fs::write(root.join(".codeflow/policy.json"), "{}\n").unwrap();
        std::fs::write(root.join("src/app.py"), "\n").unwrap();
        let git = git2::Repository::init(&root).unwrap();
        // A linked worktree registered the way `git worktree add` does.
        let linked = root.join(".worktrees/w");
        let admin = git.path().join("worktrees/w");
        std::fs::create_dir_all(&admin).unwrap();
        std::fs::create_dir_all(linked.join(".claude")).unwrap();
        std::fs::write(
            admin.join("gitdir"),
            format!("{}\n", linked.join(".git").display()),
        )
        .unwrap();
        std::fs::write(admin.join("commondir"), "../..\n").unwrap();
        std::fs::write(admin.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::write(
            linked.join(".git"),
            format!("gitdir: {}\n", admin.display()),
        )
        .unwrap();
        std::fs::write(linked.join(".claude/settings.json"), "{}\n").unwrap();
        Repo {
            _temp: temp,
            root,
            linked,
            home,
        }
    }

    /// The findings for `command` run from `dir` in the checkout `root`.
    fn judge_in(
        repo: &Repo,
        dir: &Path,
        root: &Path,
        command: &str,
        integrity: PolicyLevel,
    ) -> Vec<Violation> {
        let command = command
            .replace("@ROOT@", &repo.root.display().to_string())
            .replace("@LINKED@", &repo.linked.display().to_string());
        evaluate(
            &command,
            &SecuritySection::default(),
            integrity,
            dir,
            root,
            Some(&repo.home),
        )
    }

    fn integrity_refusal(findings: &[Violation]) -> Option<&Violation> {
        findings.iter().find(|v| v.rule == "git.hook_integrity")
    }

    /// Every command refuses under `git.hook_integrity` from `dir`, each
    /// with a finding about an interpreter argument, not its code.
    fn assert_refused(repo: &Repo, dir: &Path, root: &Path, commands: &[&str]) {
        let mut wrong = Vec::new();
        for command in commands {
            let findings = judge_in(repo, dir, root, command, PolicyLevel::Block);
            match integrity_refusal(&findings) {
                Some(v)
                    if v.message.contains("interpreter")
                        && !v.message.contains("interpreter code") => {}
                _ => wrong.push(format!("should refuse: {command}: {findings:?}")),
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    fn assert_allowed(repo: &Repo, commands: &[&str]) {
        let mut wrong = Vec::new();
        for command in commands {
            let findings = judge_in(repo, &repo.root, &repo.root, command, PolicyLevel::Block);
            if !findings.is_empty() {
                wrong.push(format!("should allow: {command}: {findings:?}"));
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    /// TSK-302 AC-1 (issue 173): an argument naming an enforcement path
    /// refuses for every interpreter, after inline code, after stdin `-`,
    /// as a script's argument and in a linked worktree. The heredoc code
    /// here names no path, so the argument alone decides.
    #[test]
    fn interpreter_arguments_naming_an_enforcement_path_refuse() {
        let repo = repo();
        assert_refused(
            &repo,
            &repo.root,
            &repo.root,
            &[
                "python3 -I -B - \"@ROOT@/.claude/settings.json\" <<'PY'\nimport sys\nopen(sys.argv[1], 'a')\nPY",
                "python3 - \"@ROOT@/.codeflow/policy.json\" <<'PY'\nimport sys\nPY",
                "python3 edit.py \"@ROOT@/.claude/settings.json\"",
                "python3 edit.py .codeflow/policy.json",
                "python edit.py .claude/settings.json",
                "python3.13 edit.py .claude/settings.json",
                "python3 -c 'import sys; open(sys.argv[1], \"a\")' .claude/settings.json",
                "python3 -c 'import sys' -- .claude/settings.json",
                "node -e 'require(\"fs\").appendFileSync(process.argv[1], \"\")' .claude/settings.json",
                "node --eval='x' .codeflow/policy.json",
                "nodejs script.js .codeflow/policy.json",
                "perl -i -pe 's/a/b/' .claude/settings.json",
                "perl -e 'print' .codeflow/policy.json",
                "ruby -e 'File.write(ARGV[0], \"\")' .codeflow/policy.json",
                "ruby -i -pe 'x' .claude/settings.json",
                "python3 tool.py --out=.codeflow/policy.json",
                "python3 edit.py ~/.claude/settings.json",
                "python3 edit.py .claude/*.json",
                "python3 edit.py .codeflow/{policy,other}.json",
                "python3 edit.py \"@LINKED@/.claude/settings.json\"",
            ],
        );
        // From inside the linked worktree, its own files and the main
        // checkout's are enforcement paths.
        assert_refused(
            &repo,
            &repo.linked,
            &repo.linked,
            &[
                "python3 edit.py .claude/settings.json",
                "python3 edit.py \"@ROOT@/.claude/settings.json\"",
                "python3 edit.py ../../.codeflow/policy.json",
            ],
        );
    }

    /// TSK-302 AC-2: the interpreter is found through the wrappers the
    /// parser reads, and `xargs` and `find -exec` supply its path.
    #[test]
    fn wrapped_interpreter_arguments_refuse() {
        let repo = repo();
        assert_refused(
            &repo,
            &repo.root,
            &repo.root,
            &[
                "env python3 edit.py .claude/settings.json",
                "env FOO=1 python3 edit.py .claude/settings.json",
                "env -S 'python3 edit.py .claude/settings.json'",
                "command python3 edit.py .claude/settings.json",
                "nohup node x.js .codeflow/policy.json",
                "timeout 5 python3 edit.py .claude/settings.json",
                "bash -c 'python3 edit.py .claude/settings.json'",
                "sh -c 'python3 edit.py \"$1\"' sh .codeflow/policy.json",
                "echo .claude/settings.json | xargs python3 edit.py",
                "printf '%s\\n' \"@LINKED@/.claude/settings.json\" | xargs -I {} python3 edit.py {}",
                "find .claude -name settings.json -exec python3 edit.py {} \\;",
                "find . -name 'settings.json' -exec python3 edit.py {} +",
                "find . -name 'set*' -exec node x.js {} \\;",
                "find . -name settings.json -exec sh -c 'python3 edit.py \"$1\"' sh {} \\;",
                "cd .claude && python3 edit.py settings.json",
                "cd .codeflow; ruby x.rb policy.json",
                "cd \"$D\" && python3 edit.py .claude/settings.json",
                "cd \"$D\" && python3 edit.py policy.json",
            ],
        );
    }

    /// TSK-302 AC-3: policy authority metadata refuses even with
    /// `git.hook_integrity` off; an enforcement path then passes.
    #[test]
    fn interpreter_arguments_naming_authority_refuse_at_every_level() {
        let repo = repo();
        for command in [
            "python3 -c 'import sys' .git/config",
            "python3 edit.py .git/refs/remotes/origin/main",
        ] {
            let findings = judge_in(&repo, &repo.root, &repo.root, command, PolicyLevel::Off);
            assert!(
                findings
                    .iter()
                    .any(|v| v.rule == "git.hook_integrity" && v.level_fixed),
                "{command}: {findings:?}"
            );
        }
        assert!(judge_in(
            &repo,
            &repo.root,
            &repo.root,
            "python3 edit.py .claude/settings.json",
            PolicyLevel::Off
        )
        .is_empty());
    }

    /// TSK-302 AC-4: a value the guard cannot read takes git-guard's rule
    /// for writer operands, and the refusal names the sanctioned route.
    #[test]
    fn unreadable_interpreter_arguments_take_the_writer_rule() {
        let repo = repo();
        assert_refused(
            &repo,
            &repo.root,
            &repo.root,
            &[
                // The issue's command, its main checkout and linked forms.
                "W=@ROOT@; python3 -I -B - \"$W/.claude/settings.json\" <<'PY'\nimport json, sys\np = sys.argv[1]\nopen(p, 'w').write('')\nPY",
                "W=@LINKED@; python3 - \"$W/.claude/settings.json\" <<'PY'\nimport sys\nPY",
                "W=@ROOT@; python3 - \"$W/.codeflow/policy.json\"",
                "python3 edit.py \"$REPO/.claude/settings.json\"",
                "python3 edit.py \"${REPO}/.codeflow/policy.json\"",
                "python3 edit.py \"$HOME/.claude/settings.json\"",
                "P=.codeflow/policy.json; python3 edit.py \"$P\"",
                "python3 edit.py \"$(echo .codeflow/policy.json)\"",
            ],
        );
        let findings = judge_in(
            &repo,
            &repo.root,
            &repo.root,
            "P=.codeflow/policy.json; python3 edit.py \"$P\"",
            PolicyLevel::Block,
        );
        let refusal = integrity_refusal(&findings).unwrap();
        assert!(
            refusal.message.contains("filled in at run time"),
            "{}",
            refusal.message
        );
        assert!(
            refusal
                .remedy
                .contains("redirect it into the interpreter's stdin"),
            "{}",
            refusal.remedy
        );
        assert_allowed(
            &repo,
            &[
                "python3 tool.py \"$INPUT\"",
                "python3 tool.py \"$HOME/notes.md\"",
                "for f in src/*.py; do python3 lint.py \"$f\"; done",
                "W=@ROOT@; python3 tool.py \"$W/src/app.py\"",
            ],
        );
    }

    /// TSK-302 AC-5: ordinary data and output files, reading an
    /// enforcement file with a reader or on stdin, and text that only
    /// shows such a command stay allowed.
    #[test]
    fn ordinary_interpreter_arguments_stay_allowed() {
        let repo = repo();
        assert_allowed(
            &repo,
            &[
                "python3 edit.py notes.txt out.json",
                "python3 -m json.tool data.json out.json",
                "python3 -W ignore src/app.py",
                "node build.js --out=dist/app.js",
                "python3 - <<'PY'\nprint(1)\nPY",
                "python3 -c 'import json, sys; print(json.load(sys.stdin))' < .codeflow/policy.json",
                "cat .codeflow/policy.json | python3 -c 'import sys; print(sys.stdin.read())'",
                "jq . .codeflow/policy.json",
                "find src -name '*.py' -exec python3 -m py_compile {} +",
                "find . -name '*.py' -exec python3 -m py_compile {} +",
                "find src -name '*.py' -exec sh -c 'python3 lint.py \"$1\"' sh {} \\;",
                "ls | xargs python3 edit.py",
                "cat > notes.md <<'EOF'\nexample: python3 edit.py .claude/settings.json\nEOF",
                "printf '%s\\n' 'python3 edit.py .claude/settings.json' > notes.md",
                "cd src && python3 app.py settings.json",
                "python3 edit.py \"@ROOT@/src/app.py\"",
            ],
        );
    }

    /// The arguments the parser hands over: inline code is left out, and
    /// the directory follows `cd`, including one it cannot follow.
    #[test]
    fn interpreter_calls_leave_out_code_and_follow_cd() {
        let cwd = Path::new("/w");
        let calls = outward::interpreter_calls("python3 -c 'x = 1' a.py b", cwd);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].args, ["-c", "a.py", "b"]);
        assert_eq!(
            outward::interpreter_calls("node --eval=code x", cwd)[0].args,
            ["x"]
        );
        let dirs: Vec<_> = outward::interpreter_calls("cd sub && python3 x", cwd)
            .into_iter()
            .map(|c| c.cwd)
            .collect();
        assert!(dirs.contains(&Some(PathBuf::from("/w/sub"))), "{dirs:?}");
        let dirs: Vec<_> = outward::interpreter_calls("cd \"$D\" && python3 x", cwd)
            .into_iter()
            .map(|c| c.cwd)
            .collect();
        assert!(dirs.contains(&None), "{dirs:?}");
        assert!(outward::interpreter_calls("rustc x.rs", cwd).is_empty());
        // A version suffix is read past; a multi-byte name ending is not cut
        // inside a character.
        assert_eq!(outward::interpreter_calls("ruby3.3 x", cwd).len(), 1);
        assert!(outward::interpreter_calls("gre\u{a0}1 x", cwd).is_empty());
    }
}
