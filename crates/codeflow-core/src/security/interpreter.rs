//! Conservative literal inspection inside interpreter code, never ordinary
//! data, and of the arguments an interpreter is given (TSK-302).
use super::{actions, outward, pattern::matches_extended_glob};
use crate::hooks::{PolicyLevel, SecuritySection, Violation};
use std::path::Path;

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
    violations.extend(super::arguments::evaluate(
        command, integrity, cwd, root, home,
    ));
    violations
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
    use std::path::PathBuf;
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
        // A scaffolded hook directory, which `find -execdir` can run in.
        std::fs::create_dir_all(root.join(".codeflow/git-hooks")).unwrap();
        std::fs::write(root.join(".codeflow/git-hooks/pre-commit"), "#!/bin/sh\n").unwrap();
        std::fs::write(root.join("src/notes.txt"), "\n").unwrap();
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

    /// Every resolved reading uses the writer's directory and checkout
    /// protection, including literal and brace-expanded operands.
    #[test]
    fn interpreter_directory_and_checkout_arguments_refuse() {
        let mut repo = repo();
        repo.home = repo.root.parent().unwrap().to_path_buf();
        assert_refused(
            &repo,
            &repo.root,
            &repo.root,
            &[
                "python3 tool.py .worktrees/w/{.git,.claude}",
                "python3 tool.py .worktrees/{w,x}/.git",
                "python3 tool.py .worktrees/w/.git{,}",
                "python3 tool.py .worktrees/w/.git",
                "python3 tool.py .WORKTREES/W/.GIT",
                "node tool.js .worktrees/w/.git",
                "python3 tool.py ~/repo/.worktrees/w/.git",
                "python3 tool.py .claude",
                "python3 tool.py .worktrees/w",
                "python3 tool.py .worktrees",
                "python3 tool.py .",
            ],
        );
        assert_refused(&repo, &repo.linked, &repo.linked, &["python3 tool.py .git"]);
        // A launcher can run in a different repository. Its registered
        // checkout roots must qualify for the same candidate checks.
        let other = self::repo();
        let moved = other.root.join("away/w");
        std::fs::create_dir(moved.parent().unwrap()).unwrap();
        std::fs::rename(&other.linked, &moved).unwrap();
        std::fs::write(
            other.root.join(".git/worktrees/w/gitdir"),
            format!("{}\n", moved.join(".git").display()),
        )
        .unwrap();
        assert_refused(
            &repo,
            &repo.root,
            &repo.root,
            &[&format!(
                "env -C \"{}\" python3 tool.py away/w",
                other.root.display()
            )],
        );
    }

    /// An expanded directory operand cannot be followed as literal text.
    #[test]
    fn interpreter_pattern_directory_changes_are_unknown() {
        let repo = repo();
        assert_refused(
            &repo,
            &repo.root,
            &repo.root,
            &[
                "cd .cla* && python3 tool.py settings.json",
                "cd .c?aude && python3 tool.py settings.json",
                "cd .[c]laude && python3 tool.py settings.json",
                "cd {.claude,x} && python3 tool.py settings.json",
                "pushd .cla* && python3 tool.py settings.json",
                "env -C .cla* python3 tool.py settings.json",
                "ruby -C .cla* tool.rb settings.json",
                "cd .wor* && python3 tool.py w/.g*",
            ],
        );
    }

    /// An incomplete ordinary glob refuses with the actual entry budget.
    #[test]
    fn interpreter_incomplete_glob_reports_limit() {
        let repo = repo();
        let many = repo.root.join("many");
        std::fs::create_dir(&many).unwrap();
        for n in 0..5000 {
            std::fs::write(many.join(format!("f{n}")), "").unwrap();
        }
        let findings = judge_in(
            &repo,
            &repo.root,
            &repo.root,
            "python3 tool.py many/*",
            PolicyLevel::Block,
        );
        assert!(
            integrity_refusal(&findings)
                .is_some_and(|refusal| refusal.message.contains("more than 4096 entries")),
            "{findings:?}"
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

    #[test]
    fn interpreter_hidden_worktree_glob_refuses() {
        let repo = repo();
        let word = format!("{}/.*", repo.linked.display());
        let paths = crate::hooks::git_guard::glob_paths(&word, &repo.root).unwrap();
        assert!(paths.contains(&repo.linked.join(".git")), "{paths:?}");
        assert!(paths.contains(&repo.linked.join(".claude")), "{paths:?}");
        assert_refused(
            &repo,
            &repo.root,
            &repo.root,
            &[
                "python3 tool.py @LINKED@/.*",
                "python3 tool.py \"@LINKED@/.*\"",
                "python3 tool.py @LINKED@/.cla*",
                "python3 tool.py @LINKED@/.g*",
                "python3 tool.py @ROOT@/.worktrees/*",
            ],
        );
        for command in ["python3 tool.py src/*", "python3 tool.py src/missing*"] {
            let findings = judge_in(&repo, &repo.root, &repo.root, command, PolicyLevel::Block);
            assert!(findings.is_empty(), "{command}: {findings:?}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_repository_spellings_keep_ordinary_arguments_and_enforcement_refusals() {
        let repo = repo();
        let plain = crate::portable_path::canonicalize(&repo.root).unwrap();
        let verbatim = repo.root.canonicalize().unwrap();
        let spellings = [
            plain.display().to_string(),
            verbatim.display().to_string(),
            plain.display().to_string().replace('\\', "/"),
            plain.display().to_string().to_uppercase(),
        ];
        let mut results = Vec::new();
        let mut mismatches = Vec::new();
        for root in [&plain, &verbatim] {
            for spelling in &spellings {
                let ordinary = format!(r#"W={spelling}; python3 tool.py "$W/src/app.py""#);
                let findings = judge_in(&repo, root, root, &ordinary, PolicyLevel::Block);
                let result = format!("cwd={root:?}; {ordinary}: {findings:?}");
                results.push(result.clone());
                if !findings.is_empty() {
                    mismatches.push(result);
                }
                for suffix in [
                    ".claude/settings.json",
                    ".worktrees/w/.claude/settings.json",
                    ".worktrees/w/.claude/*",
                    ".worktrees/w/.*",
                ] {
                    let command = format!(r#"python3 tool.py "{spelling}/{suffix}""#);
                    let findings = judge_in(&repo, root, root, &command, PolicyLevel::Block);
                    let result = format!("cwd={root:?}; {command}: {findings:?}");
                    results.push(result.clone());
                    if integrity_refusal(&findings).is_none() {
                        mismatches.push(result);
                    }
                }
            }
        }
        println!("{}", results.join("\n"));
        assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
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
                "python3 tool.py \"foo (bar)\"",
                "python3 tool.py \"x (y)\"",
                "python3 tool.py \"x (y)\" notes.txt",
                "python3 tool.py \"print(x)\"",
                "node x.js --filter \"(a|b)\"",
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
        assert_eq!(calls[0].args, ["a.py", "b"]);
        // Every word after the code or script boundary is an argument, a
        // second `-c` included; option values are kept, flags are not.
        let args = |line: &str| outward::interpreter_calls(line, cwd)[0].args.clone();
        assert_eq!(args("python3 -c 'x' -c s.json"), ["-c", "s.json"]);
        assert_eq!(args("python3 -Ic'x' a"), ["a"]);
        // `-m` is a module name, not a path; the words after it are argv.
        assert_eq!(
            args("python3 -W ignore -m json.tool a b"),
            ["ignore", "a", "b"]
        );
        assert_eq!(args("python3 -B - a"), ["-", "a"]);
        assert_eq!(args("python3 -- s.py -o"), ["s.py", "-o"]);
        assert_eq!(args("node -pe 'x' a -e b"), ["a", "-e", "b"]);
        assert_eq!(
            args("node --require ./r.js app.js a"),
            ["./r.js", "app.js", "a"]
        );
        assert_eq!(args("perl -i.bak -pe 's/a/b/' -e 'c' f"), ["f"]);
        assert_eq!(
            args("perl -I lib -Mstrict -e 'x' f"),
            ["lib", "strict", "f"]
        );
        assert_eq!(args("ruby -C d -e 'x' f")[..], ["f"]);
        assert_eq!(
            outward::interpreter_calls("ruby -C d -C e -e 'x' f", cwd)[0].chdir,
            ["d", "e"]
        );
        assert_eq!(
            outward::interpreter_calls("node --eval=code x", cwd)[0].args,
            ["x"]
        );
        // The line's directory set: every directory a change may reach,
        // whether or not it succeeds, through every spelling.
        let set = |line: &str| outward::line_directories(line, cwd, Some(Path::new("/h")));
        let has = |line: &str, dir: &str| set(line).dirs.contains(&PathBuf::from(dir));
        for (line, dir) in [
            ("cd sub && python3 x", "/w/sub"),
            ("cd -- sub && python3 x", "/w/sub"),
            ("cd -P -L sub && python3 x", "/w/sub"),
            ("cd -q sub; python3 x", "/w/sub"),
            ("command cd sub && python3 x", "/w/sub"),
            ("command -p cd sub && python3 x", "/w/sub"),
            ("builtin cd sub && python3 x", "/w/sub"),
            ("! cd sub; python3 x", "/w/sub"),
            ("pushd sub >/dev/null && python3 x", "/w/sub"),
            ("chdir sub && python3 x", "/w/sub"),
            ("env -C sub python3 x", "/w/sub"),
            ("env -Csub python3 x", "/w/sub"),
            ("env --chdir=sub python3 x", "/w/sub"),
            ("git -C sub status; python3 x", "/w/sub"),
            ("cd a && cd b && python3 x", "/w/a/b"),
            ("cd a && cd .. && python3 x", "/w/a"),
            ("cd && python3 x", "/h"),
        ] {
            assert!(has(line, dir), "{line}: {:?}", set(line));
            assert!(has(line, "/w"), "{line}: {:?}", set(line));
        }
        for line in [
            "cd \"$D\" && python3 x",
            "cd - && python3 x",
            "cd ~u && python3 x",
        ] {
            assert!(set(line).unknown, "{line}");
        }
        assert!(outward::interpreter_calls("rustc x.rs", cwd).is_empty());
        // A version suffix is read past; a multi-byte name ending is not cut
        // inside a character.
        assert_eq!(outward::interpreter_calls("ruby3.3 x", cwd).len(), 1);
        assert!(outward::interpreter_calls("gre\u{a0}1 x", cwd).is_empty());
    }

    /// Every command refuses with a finding about an interpreter argument
    /// that keeps its level fixed (authority metadata) at `level`.
    fn assert_authority(repo: &Repo, commands: &[&str], level: PolicyLevel) {
        let mut wrong = Vec::new();
        for command in commands {
            let findings = judge_in(repo, &repo.root, &repo.root, command, level);
            if !findings
                .iter()
                .any(|v| v.rule == "git.hook_integrity" && v.level_fixed)
            {
                wrong.push(format!(
                    "should refuse at {level:?}: {command}: {findings:?}"
                ));
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    /// TSK-302 review F1: a word after the interpreter's code or script
    /// boundary is an argument, a second `-c` included, and a short
    /// option's attached value is read as a path; ordinary options pass.
    #[test]
    fn arguments_after_code_and_attached_values_refuse() {
        let repo = repo();
        assert_refused(
            &repo,
            &repo.root,
            &repo.root,
            &[
                "cd .claude && python3 -c 'import sys; open(sys.argv[2], \"a\").write(\"\")' -c settings.json",
                "python3 -c 'import argparse; p=argparse.ArgumentParser(); p.add_argument(\"-o\"); open(p.parse_args().o,\"a\").write(\"\")' -o.claude/settings.json",
                "python3 -c 'x' -- .claude/settings.json",
                "python3 - -o.codeflow/policy.json <<'PY'\nimport sys\nPY",
                "python3 tool.py -o.claude/settings.json",
                "python3 -m tool .codeflow/policy.json",
                "node app.js -o.claude/settings.json",
                "perl -e 'x' -- -o.claude/settings.json",
                "ruby -C .claude -e 'x' settings.json",
            ],
        );
        assert_allowed(
            &repo,
            &[
                "python3 -c 'x' -c data.json",
                "python3 tool.py -v -n3 --level=2 -o out.json",
                "python3 -W ignore -X dev tool.py data.json",
                "node --require ./setup.js app.js data.json",
                "perl -I lib -e 'x' notes.txt",
                "ruby -C src -e 'x' app.rb",
            ],
        );
    }

    /// TSK-302 review F2: `cd` options are read past, an `&&` chain runs in
    /// the directory its changes lead to, and `find -execdir` runs its
    /// command in each directory it visits.
    #[test]
    fn the_directory_an_interpreter_runs_in_is_followed() {
        let repo = repo();
        assert_refused(
            &repo,
            &repo.root,
            &repo.root,
            &[
                "cd -- .claude && python3 -c 'import sys; open(sys.argv[1], \"a\").write(\"\")' settings.json",
                "cd -P .claude && python3 -c 'import sys; open(sys.argv[1], \"a\").write(\"\")' settings.json",
                "cd -L -- .claude && python3 tool.py settings.json",
                "cd .claude || true; python3 tool.py settings.json",
                "find . -name '*.json' -execdir python3 tool.py settings.json \\;",
                "cd \"$D\" && python3 tool.py policy.json",
            ],
        );
        assert_allowed(
            &repo,
            &[
                "cd src && python3 tool.py out.json",
                "find src -name '*.py' -execdir python3 -m py_compile {} +",
                "find src -name '*.py' -execdir python3 lint.py out.txt \\;",
            ],
        );
    }

    /// TSK-302 review F3: authority metadata refuses in every operand form,
    /// supplied, filled in at run time or globbed, whatever the integrity
    /// level; ordinary unresolved input stays allowed.
    #[test]
    fn authority_refuses_in_every_operand_form_at_every_level() {
        let repo = repo();
        for level in [
            PolicyLevel::Block,
            PolicyLevel::Warn,
            PolicyLevel::Allow,
            PolicyLevel::Off,
        ] {
            assert_authority(
                &repo,
                &[
                    "python3 tool.py .git/config",
                    "echo .git/config | xargs python3 tool.py",
                    "find .git -name config -exec python3 tool.py {} \\;",
                    "P=.git/config; python3 tool.py \"$P\"",
                    "python3 tool.py .git/con*",
                    "python3 tool.py \"$(echo .git/config)\"",
                ],
                level,
            );
            for command in [
                "python3 tool.py \"$INPUT\"",
                "ls | xargs python3 tool.py",
                "find src -exec python3 tool.py {} \\;",
                "for f in src/*.py; do python3 lint.py \"$f\"; done",
            ] {
                let findings = judge_in(&repo, &repo.root, &repo.root, command, level);
                assert!(findings.is_empty(), "{level:?}: {command}: {findings:?}");
            }
        }
    }

    /// TSK-302 review F4: a line with several suppliers reads what the line
    /// names once, so five `xargs` interpreters stay within the hook's
    /// 10-second budget with a wide margin.
    #[test]
    fn several_suppliers_read_the_line_once() {
        let repo = repo();
        let line = (0..5)
            .map(|n| format!("ls | xargs python3 check{n}.py"))
            .collect::<Vec<_>>()
            .join("; ");
        let started = std::time::Instant::now();
        assert!(judge_in(&repo, &repo.root, &repo.root, &line, PolicyLevel::Block).is_empty());
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
    }

    /// The findings the public exec-guard boundary gives `command` from
    /// `dir` at `level`, after it merges the findings of each rule.
    fn public(repo: &Repo, dir: &Path, command: &str, level: PolicyLevel) -> Vec<Violation> {
        let command = command
            .replace("@ROOT@", &repo.root.display().to_string())
            .replace("@LINKED@", &repo.linked.display().to_string());
        crate::hooks::exec_guard::evaluate_in(
            &command,
            true,
            &SecuritySection::default(),
            level,
            dir,
            &repo.root,
        )
    }

    fn integrity_finding(findings: &[Violation]) -> Option<&Violation> {
        findings.iter().find(|v| v.rule == "git.hook_integrity")
    }

    /// Each command refuses under `git.hook_integrity` through the public
    /// boundary at block; with `fixed`, it also keeps a fixed block at warn,
    /// allow and off.
    fn assert_public_refused(repo: &Repo, commands: &[&str], fixed: bool) {
        let mut wrong = Vec::new();
        for command in commands {
            for level in [
                PolicyLevel::Block,
                PolicyLevel::Warn,
                PolicyLevel::Allow,
                PolicyLevel::Off,
            ] {
                if !fixed && level != PolicyLevel::Block {
                    continue;
                }
                let findings = public(repo, &repo.root, command, level);
                let ok = integrity_finding(&findings)
                    .is_some_and(|v| v.level == PolicyLevel::Block && (!fixed || v.level_fixed));
                if !ok {
                    wrong.push(format!(
                        "should refuse at {level:?}: {command}: {findings:?}"
                    ));
                }
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    fn assert_public_allowed(repo: &Repo, commands: &[&str]) {
        let mut wrong = Vec::new();
        for command in commands {
            let findings = public(repo, &repo.root, command, PolicyLevel::Block);
            if let Some(v) = integrity_finding(&findings) {
                wrong.push(format!("should allow: {command}: {}", v.message));
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    /// TSK-302 review round 2, Codex R2-1: the fixed authority block
    /// survives the merge of findings, whatever came first, and a broad
    /// unresolved glob that can reach `.git/config` is authority too.
    #[test]
    fn the_authority_block_survives_merged_findings() {
        let repo = repo();
        assert_public_refused(
            &repo,
            &[
                "python3 tool.py .claude/settings.json .git/config",
                "python3 tool.py .git/config .claude/settings.json",
                "python3 tool.py .claude/settings.json; python3 tool.py .git/config",
                "python3 -c 'open(\".claude/settings.json\")' .git/config",
                "W=.; python3 tool.py \"$W/.git/\"*",
                "W=.; python3 tool.py \"$W/.git/con\"*",
            ],
            true,
        );
    }

    /// Codex R2-2 and Grok F2: every directory change a line can make,
    /// whatever spelling and whether or not it succeeds, is a directory the
    /// interpreter may run in.
    #[test]
    fn every_directory_change_spelling_is_followed() {
        let repo = repo();
        assert_public_refused(
            &repo,
            &[
                "cd .claude && ! cd nonexistent && python3 tool.py settings.json",
                "ruby -C .claude -C . -e 'File.write(ARGV[0], \"\")' settings.json",
                "find ../repo -name '*.json' -execdir python3 tool.py settings.json \\;",
                "find .claude -name settings.json -execdir ruby -C ../.codeflow -e 'File.write(ARGV[0], \"\")' policy.json \\;",
                "command cd .claude && python3 tool.py settings.json",
                "builtin cd .claude && python3 tool.py settings.json",
                "command -p cd .claude && python3 tool.py settings.json",
                "pushd .claude >/dev/null && python3 tool.py settings.json",
                "chdir .claude && python3 tool.py settings.json",
                "cd -q .claude && python3 tool.py settings.json",
                "env -C.claude python3 tool.py settings.json",
                "cd .claude && command cd .. && python3 tool.py .claude/settings.json",
                "cd .claude && builtin cd .. && python3 tool.py .claude/settings.json",
                "cd .claude && pushd .. >/dev/null && python3 tool.py .claude/settings.json",
                "cd .claude && cd -q .. && python3 tool.py .claude/settings.json",
                // Known conservative refusals: the set keeps `.claude`.
                "cd .claude && cd .. && python3 tool.py settings.json",
                "find . -execdir python3 lint.py notes.txt \\;",
            ],
            false,
        );
        assert_public_refused(
            &repo,
            &[
                "command cd .git && python3 tool.py config",
                "builtin cd .git && python3 tool.py config",
                "pushd .git >/dev/null && python3 tool.py config",
                "env -C.git python3 tool.py config",
                "cd \"$D\" && python3 tool.py config",
            ],
            true,
        );
        assert_public_allowed(
            &repo,
            &[
                "cd src && python3 app.py settings.json",
                "find . -name '*.py' -execdir python3 -m py_compile {} +",
                "find . -name '*.rs' -execdir python3 lint.py {} +",
                "find src -execdir python3 lint.py notes.txt \\;",
                "pushd src >/dev/null && python3 tool.py out.json && popd",
            ],
        );
    }

    /// Grok F1: code text is judged from every directory the line may run
    /// it in, bare names included.
    #[test]
    fn code_text_is_judged_from_every_directory() {
        let repo = repo();
        assert_public_refused(
            &repo,
            &[
                "cd .claude && python3 -c 'open(\"settings.json\",\"w\").write(\"\")'",
                "cd .claude && python3 -c 'open(\"./settings.json\",\"w\").write(\"\")'",
                "cd -- .claude && python3 -c 'open(\"./settings.json\",\"w\").write(\"\")'",
                "cd -P .claude && python3 -c 'open(\"./settings.json\",\"w\").write(\"\")'",
                "cd .codeflow && python3 -c 'open(\"policy.json\",\"w\").write(\"\")'",
                "cd .claude && python3 -c 'open(\"settings.local.json\",\"w\").write(\"\")'",
                "cd .claude && node -e 'require(\"fs\").writeFileSync(\"settings.json\",\"\")'",
                "cd .claude && perl -e 'open F, \">\", \"settings.json\"'",
                "ruby -C .claude -e 'File.write(\"settings.json\",\"\")'",
                "ruby -C.claude -e 'File.write(\"./settings.json\",\"\")'",
            ],
            false,
        );
        assert_public_refused(
            &repo,
            &[
                "cd .git && python3 -c 'open(\"config\",\"w\").write(\"\")'",
                "cd .git && python3 -c 'open(\"./config\",\"a\").write(\"\")'",
            ],
            true,
        );
        assert_public_allowed(
            &repo,
            &[
                "cd .claude && python3 -c 'print(1)'",
                "cd src && python3 -c 'open(\"out.txt\",\"w\")'",
                "python3 -c 'import json, sys\\; print(json.dumps({\"a\": 1}))'",
            ],
        );
    }

    /// Codex R2-3: an attached value after a short-option cluster is read.
    #[test]
    fn an_attached_value_after_an_option_cluster_is_read() {
        let repo = repo();
        assert_public_refused(
            &repo,
            &[
                "python3 -c 'import argparse\\; p=argparse.ArgumentParser(); p.add_argument(\"-v\",action=\"store_true\"); p.add_argument(\"-o\"); open(p.parse_args().o,\"a\").write(\"\")' -vo.claude/settings.json",
                "python3 tool.py -xvo.codeflow/policy.json",
            ],
            false,
        );
        assert_public_allowed(
            &repo,
            &[
                "python3 tool.py -vvv -xz out.json",
                "python3 tool.py -vo out.json",
                "python3 tool.py -n3",
            ],
        );
    }

    /// TSK-302 review round 3: within the modeled grammar the directory set
    /// is exact, and anything outside it makes the set unknown, so known
    /// words are judged by their trailing components; it never fails open.
    #[test]
    fn launchers_and_unmodeled_grammar_never_fail_open() {
        let repo = repo();
        let go = repo.root.join("go.sh");
        std::fs::write(&go, "cd .claude\n").unwrap();
        let go = go.display().to_string();
        let source = |rest: &str| format!("source {go} && {rest}");
        let dot = |rest: &str| format!(". {go} && {rest}");
        let owned = [
            source("python3 tool.py settings.json"),
            source("python3 -c 'open(\"settings.json\",\"w\").write(\"\")'"),
            dot("python3 tool.py settings.json"),
        ];
        let owned: Vec<&str> = owned.iter().map(String::as_str).collect();
        assert_public_refused(&repo, &owned, false);
        let authority = source("python3 tool.py config");
        assert_public_refused(&repo, &[authority.as_str()], true);
        assert_public_allowed(&repo, &[source("python3 tool.py notes.txt").as_str()]);
        // Codex R3-1: options that consume values, attached spellings,
        // clusters and outer launchers.
        assert_public_refused(
            &repo,
            &[
                "env -u FOO -C.claude python3 -c 'import sys; open(sys.argv[1],\"w\")' settings.json",
                "nice env -C.claude python3 -c 'import sys; open(sys.argv[1],\"w\")' settings.json",
                "env -iC.claude python3 tool.py settings.json",
                "timeout 5 env -C.claude python3 tool.py settings.json",
                "nohup env --chdir .claude python3 tool.py settings.json",
                "stdbuf -oL env -C .claude python3 tool.py settings.json",
                "env -S 'python3 tool.py' settings.json",
                "env --frobnicate -C.claude python3 tool.py settings.json",
            ],
            false,
        );
        assert_public_refused(
            &repo,
            &["env -u FOO -C.git python3 -c 'import sys; open(sys.argv[1],\"w\")' config"],
            true,
        );
        assert_public_allowed(
            &repo,
            &[
                "nice python3 tool.py data.json",
                "nice -n 5 python3 tool.py data.json",
                "timeout 5 node x.js out.json",
                "timeout -s KILL 5 python3 app.py",
                "env FOO=1 python3 app.py",
                "env -u FOO python3 app.py data.json",
                "stdbuf -oL python3 app.py out.txt",
                "nohup python3 worker.py",
                "command python3 app.py out.json",
                "time python3 app.py",
            ],
        );
    }

    /// Codex R3-2: a code literal whose directory is unknown is judged by
    /// its trailing components; authority keeps its fixed level.
    #[test]
    fn code_literals_in_an_unknown_directory_are_judged() {
        let repo = repo();
        assert_public_refused(
            &repo,
            &[
                "D=.claude; cd \"$D\" && python3 -c 'open(\"settings.json\",\"w\")'",
                "D=.claude; cd \"$D\" && python3 tool.py settings.json",
            ],
            false,
        );
        assert_public_refused(
            &repo,
            &[
                "D=.git; cd \"$D\" && python3 -c 'open(\"config\",\"w\")'",
                "D=.git; cd \"$D\" && python3 tool.py config",
            ],
            true,
        );
        assert_public_allowed(
            &repo,
            &[
                "D=src; cd \"$D\" && python3 -c 'print(1)'",
                "D=src; cd \"$D\" && python3 -c 'open(\"out.txt\",\"w\")'",
                "cd \"$D\" && python3 -c 'import sys; open(sys.argv[1])' \"$IN\"",
            ],
        );
    }

    /// Codex R3-3 and Grok F1: `find -execdir` starts from every directory
    /// in the line's set, a file start runs in its parent, and `-path`
    /// sees find's own spelling.
    #[test]
    fn execdir_follows_the_line_set_and_find_spelling() {
        let repo = repo();
        assert_public_refused(
            &repo,
            &[
                "find . -path '*cla*json' -execdir python3 -c 'import sys; open(sys.argv[1],\"w\")' settings.json \\;",
                "find . -ipath '*CLA*JSON' -execdir python3 tool.py settings.json \\;",
                "find .claude/settings.json -execdir python3 tool.py settings.json \\;",
                "command cd .claude && find settings.json -execdir python3 tool.py settings.json \\;",
                "builtin cd .claude && find settings.json -execdir python3 tool.py settings.json \\;",
                "pushd .claude >/dev/null && find settings.json -execdir python3 tool.py settings.json \\;",
                "chdir .claude && find settings.json -execdir python3 tool.py settings.json \\;",
                "cd -q .claude && find settings.json -execdir python3 tool.py settings.json \\;",
                "env -C.claude find settings.json -execdir python3 tool.py settings.json \\;",
                "bash -c 'command cd .claude && find settings.json -execdir python3 tool.py settings.json \\;'",
                "command cd .codeflow && find policy.json -execdir python3 tool.py policy.json \\;",
            ],
            false,
        );
        assert_public_refused(
            &repo,
            &[
                "find . -path '*git*config' -execdir python3 -c 'import sys; open(sys.argv[1],\"w\")' config \\;",
                "command cd .git && find config -execdir python3 tool.py config \\;",
            ],
            true,
        );
        assert_public_allowed(
            &repo,
            &[
                "command cd .claude && find settings.json -execdir python3 tool.py notes.txt \\;",
                "find src -execdir python3 lint.py notes.txt \\;",
                "find . -name '*.py' -execdir python3 -m py_compile {} +",
                "find . -path './src/*' -execdir python3 lint.py notes.txt \\;",
            ],
        );
    }

    /// Codex R3-4: only string literals in code are read as file names, so
    /// identifiers and numbers in a directory-wide protected path pass.
    #[test]
    fn code_identifiers_are_not_file_names() {
        let repo = repo();
        assert_public_allowed(
            &repo,
            &[
                "cd .codex && python3 -c 'print(1)'",
                "cd .codeflow/git-hooks && python3 -c 'print(1)'",
                "cd .codeflow/git-hooks && node -e 'console.log(process.version)'",
            ],
        );
        assert_public_refused(
            &repo,
            &[
                "cd .codeflow/git-hooks && python3 -c 'open(\"pre-commit\",\"w\")'",
                "cd .claude && python3 -c 'open(\"settings.json\",\"w\")'",
            ],
            false,
        );
    }

    /// TSK-302 review round 4 (Grok F1, F2): an unmodeled word followed by
    /// a directory-change token makes the directory unknown, never
    /// unchanged; zsh precommands, `builtin --`, `caffeinate`, `xcrun`,
    /// `ionice` and the GNU-prefixed coreutils names are read as launchers.
    #[test]
    fn unmodeled_launcher_words_never_hide_a_directory_change() {
        let repo = repo();
        assert_public_refused(
            &repo,
            &[
                "noglob cd .claude && python3 tool.py settings.json",
                "noglob cd .claude && python3 -c 'open(\"settings.json\",\"w\").write(\"\")'",
                "nocorrect cd .claude && python3 tool.py settings.json",
                "bash -c 'builtin -- cd .claude && python3 tool.py settings.json'",
                "caffeinate env -C.claude python3 tool.py settings.json",
                "caffeinate env -C .claude python3 tool.py settings.json",
                "caffeinate env -C.claude python3 -c 'open(\"settings.json\",\"w\").write(\"\")'",
                "genv -C.claude python3 tool.py settings.json",
                "gtimeout 5 env -C.claude python3 tool.py settings.json",
                "ionice -c 3 env -C.claude python3 tool.py settings.json",
                "frobnicate cd .claude && python3 tool.py settings.json",
                "frobnicate env -C.claude x && python3 tool.py settings.json",
            ],
            false,
        );
        assert_public_refused(
            &repo,
            &[
                "noglob cd .git && python3 tool.py config",
                "true; - cd .git && python3 tool.py config",
                "bash -c 'builtin -- cd .git && python3 tool.py config'",
                "caffeinate env -C.git python3 tool.py config",
                "xcrun env -C.git python3 tool.py config",
                "genv -C.git python3 tool.py config",
                "gtimeout 5 env -C.git python3 tool.py config",
            ],
            true,
        );
        assert_public_allowed(
            &repo,
            &[
                "noglob cd .claude && python3 tool.py notes.txt",
                "caffeinate python3 tool.py data.json",
                "xcrun python3 tool.py data.json",
                "gtimeout 5 node x.js out.json",
                "genv FOO=1 python3 app.py",
                "echo done && python3 tool.py data.json",
            ],
        );
    }

    /// After review round 4: an unmodeled program followed by a modeled
    /// launcher or an interpreter in a command position may launch it, so
    /// the rest of the segment is judged as a call whose directory is
    /// unknown. Quoted arguments, words after `--` and `-m`/`-c` values
    /// never start a command.
    #[test]
    fn an_unknown_program_that_launches_an_interpreter_is_judged() {
        let repo = repo();
        assert_public_refused(
            &repo,
            &[
                "frobnicate env -C.claude python3 tool.py settings.json",
                "frobnicate python3 tool.py settings.json",
                "cd .claude && frobnicate python3 -c 'open(\"settings.json\",\"w\")'",
                "frobnicate --flag nice python3 tool.py settings.json",
            ],
            false,
        );
        assert_public_refused(
            &repo,
            &[
                "frobnicate env -C.git python3 tool.py config",
                "cd .git && frobnicate python3 -c 'open(\"config\",\"w\")'",
            ],
            true,
        );
        assert_public_allowed(
            &repo,
            &[
                "make test",
                "git commit -m \"python3 x\"",
                "cargo run -- python3",
                "cargo run -- python3 tool.py notes.txt",
                "npm run x -- --flag",
                "npm run build",
                "docker run img python3 app.py data.json",
                "frobnicate -m python3 settings.json",
                "frobnicate 'python3 tool.py notes.txt'",
                "frobnicate env -C.claude python3 tool.py notes.txt",
            ],
        );
    }

    /// Grok round 5 F1: a bare `--` after an unmodeled program does not
    /// hide the interpreter it runs (`sandbox-exec ... --`, `taskpolicy --`).
    /// The call is judged with an unknown directory, so a bare protected
    /// tail after `--` refuses too (`cargo run -- python3 tool.py
    /// settings.json`), as after `cd "$DIR"`.
    #[test]
    fn a_double_dash_does_not_hide_the_interpreter_a_program_runs() {
        let repo = repo();
        assert_public_refused(
            &repo,
            &[
                "sandbox-exec -p '(version 1)(allow default)' -- python3 tool.py .claude/settings.json",
                "sandbox-exec -p '(version 1)(allow default)' -- python3 -c 'open(\"settings.json\",\"w\").write(\"\")'",
                "taskpolicy -- python3 tool.py .claude/settings.json",
                "docker run --rm -v .:/w -w /w -- python:3 python3 tool.py .claude/settings.json",
                "frobnicate -- python3 tool.py settings.json",
                "cargo run -- python3 tool.py settings.json",
            ],
            false,
        );
        assert_public_refused(
            &repo,
            &[
                "sandbox-exec -p '(version 1)(allow default)' -- python3 tool.py .git/config",
                "taskpolicy -- python3 tool.py .git/config",
                "frobnicate -- python3 tool.py .git/config",
                "frobnicate -- env -C.git python3 tool.py config",
            ],
            true,
        );
        assert_public_allowed(
            &repo,
            &[
                "sandbox-exec -p '(version 1)(allow default)' -- python3 tool.py notes.txt",
                "taskpolicy -- python3 tool.py notes.txt",
                "cargo run -- python3",
                "cargo run -- python3 tool.py notes.txt",
                "git commit -m \"python3 x\"",
                "npm run x -- --flag",
                "make test",
                "npm run build",
            ],
        );
    }

    /// Grok round 6 F1: a script string an unmodeled program runs, its
    /// `-c` or `-e` value or any quoted word, is read as a command line
    /// with an unknown directory (`expect -c`, `osascript -e`, `ssh host`).
    /// `echo` and `printf` only print, so their operands stay data.
    #[test]
    fn a_script_string_an_unknown_program_runs_is_judged() {
        let repo = repo();
        assert_public_refused(
            &repo,
            &[
                "expect -c 'spawn python3 tool.py .claude/settings.json'",
                "osascript -e 'do shell script \"python3 tool.py .claude/settings.json\"'",
                "frobnicate -c 'python3 tool.py .claude/settings.json'",
                "frobnicate 'python3 tool.py settings.json'",
                "frobnicate -- 'python3 tool.py settings.json'",
                "ssh host 'cd .claude && python3 tool.py settings.json'",
                "frobnicate -m 'python3 tool.py .claude/settings.json'",
            ],
            false,
        );
        assert_public_refused(
            &repo,
            &[
                "expect -c 'spawn python3 tool.py .git/config'",
                "osascript -e 'do shell script \"python3 tool.py .git/config\"'",
                "frobnicate -c 'python3 tool.py .git/config'",
            ],
            true,
        );
        assert_public_allowed(
            &repo,
            &[
                "echo -e 'hello'",
                "echo -e \"python3 tool.py .claude/settings.json\"",
                "printf '%s\\n' 'python3 edit.py .claude/settings.json' > notes.md",
                "npm run x -- --flag",
                "cargo test -- --nocapture",
                "git commit -m \"python3 x\"",
                "cargo run -- python3 -c 'print(1)'",
                "make test",
                "expect -c 'spawn python3 tool.py notes.txt'",
                "osascript -e 'display notification \"done\"'",
                "frobnicate -m python3 settings.json",
                "awk '{print $1}' data.txt",
                "sed -e 's/a/b/' notes.txt",
                "grep -e 'python3 tool.py' notes.txt",
            ],
        );
    }

    /// Grok round 7 F1: a shell or interpreter that takes its code from a
    /// pipe, a here-string, a process substitution or a `-c` value holding a
    /// substitution runs code the guard cannot read in full, so every word
    /// on the line is judged by its trailing components. F2: a word loses
    /// one `{}` pair before Tcl runs it.
    #[test]
    fn code_the_guard_cannot_read_makes_every_word_judged() {
        let repo = repo();
        assert_public_refused(
            &repo,
            &[
                "python3 -c \"$(printf '%s\\n' 'open(\".claude/settings.json\",\"w\").write(\"x\")')\"",
                "bash <(printf '%s\\n' 'python3 tool.py .claude/settings.json')",
                "python3 <(printf '%s\\n' 'open(\".claude/settings.json\",\"w\").write(\"x\")')",
                "sh <<< 'python3 tool.py .claude/settings.json'",
                "bash <<< 'python3 tool.py .claude/settings.json'",
                "python3 <<< 'open(\".claude/settings.json\",\"w\").write(\"x\")'",
                "python3 - <<< 'open(\".claude/settings.json\",\"w\").write(\"x\")'",
                "node <<< 'require(\"fs\").writeFileSync(\".claude/settings.json\",\"\")'",
                "echo 'open(\".claude/settings.json\",\"w\").write(\"x\")' | python3",
                "sh <<< 'cd .claude && python3 tool.py settings.json'",
                "expect -c 'spawn python3 tool.py {.claude/settings.json}'",
            ],
            false,
        );
        assert_public_refused(
            &repo,
            &[
                "bash <(printf '%s\\n' 'python3 tool.py .git/config')",
                "python3 <(printf '%s\\n' 'open(\".git/config\").read()')",
                "sh <<< 'python3 tool.py .git/config'",
                "python3 <<< 'open(\".git/config\").read()'",
                "echo 'open(\".git/config\").read()' | python3",
                "expect -c 'spawn python3 tool.py {.git/config}'",
            ],
            true,
        );
        assert_public_allowed(
            &repo,
            &[
                "python3 <<< 'print(1)'",
                "echo 'print(1)' | python3",
                "echo -e \"python3 tool.py .claude/settings.json\"",
                "printf '%s\\n' 'python3 edit.py .claude/settings.json' > notes.md",
                "sh <<< 'python3 tool.py notes.txt'",
                "bash <(printf '%s\\n' 'python3 tool.py notes.txt')",
                "echo 'open(\"notes.txt\").read()' | python3",
                "expect -c 'spawn python3 tool.py {notes.txt}'",
                "cat .codeflow/policy.json | python3 -c 'import sys; print(sys.stdin.read())'",
                "python3 -c \"$(printf '%s\\n' 'print(1)')\"",
                "diff <(sort a.txt) <(sort b.txt)",
            ],
        );
    }

    /// Grok round 8: where a shell (`source` and `.` included) or an
    /// interpreter runs, every quoted string on the line is judged as
    /// possible code (F1 descriptors, `source <(...)`, fifos; F2 a literal
    /// `-c` that runs its stdin), and a descriptor program is code the
    /// guard cannot read. F3: the bare word `x` is not a remote ref.
    #[test]
    fn quoted_strings_are_judged_where_a_shell_or_interpreter_runs() {
        let repo = repo();
        assert_public_refused(
            &repo,
            &[
                "python3 /dev/stdin <<< 'open(\".claude/settings.json\",\"w\").write(\"x\")'",
                "python3 /dev/fd/3 3<<< 'open(\".claude/settings.json\",\"w\").write(\"x\")'",
                "exec 3<<< 'open(\".claude/settings.json\",\"w\").write(\"x\")'; python3 /dev/fd/3",
                "source <(echo 'python3 tool.py .claude/settings.json')",
                ". <(echo 'python3 tool.py .claude/settings.json')",
                "mkfifo p; echo 'open(\".claude/settings.json\",\"w\").write(\"x\")' > p & python3 p",
                "echo 'open(\".claude/settings.json\",\"w\").write(\"x\")' | python3 -c 'exec(sys.stdin.read())'",
                "python3 -c 'exec(sys.stdin.read())' <<< 'open(\".claude/settings.json\",\"w\").write(\"x\")'",
                "python3 -c 'import sys; exec(sys.stdin.read())' <<< 'open(\".claude/settings.json\",\"a\").write(\"ok\")'",
                "node -e 'eval(require(\"fs\").readFileSync(0,\"utf8\"))' <<< 'require(\"fs\").writeFileSync(\".claude/settings.json\",\"\")'",
                "python3 /proc/self/fd/0 <<< 'open(\".claude/settings.json\",\"w\").write(\"x\")'",
            ],
            false,
        );
        assert_public_refused(
            &repo,
            &[
                "python3 /dev/stdin <<< 'open(\".git/config\").read()'",
                "python3 /dev/fd/3 3<<< 'open(\".git/config\").read()'",
                "exec 3<<< 'open(\".git/config\").read()'; python3 /dev/fd/3",
                "source <(echo 'python3 tool.py .git/config')",
                ". <(echo 'python3 tool.py .git/config')",
                "mkfifo p; echo 'open(\".git/config\").read()' > p & python3 p",
                "echo 'open(\".git/config\").read()' | python3 -c 'exec(sys.stdin.read())'",
                "python3 -c 'exec(sys.stdin.read())' <<< 'open(\".git/config\").read()'",
                "python3 -c 'import sys; exec(sys.stdin.read())' <<< 'open(\".git/config\").read()'",
                "python3 <<< 'open(\".git/refs/remotes/origin/main\").read()'",
            ],
            true,
        );
        assert_public_allowed(
            &repo,
            &[
                "python3 /dev/stdin <<< 'print(1)'",
                "python3 /dev/fd/3 3<<< 'print(1)'",
                "source <(echo 'python3 tool.py notes.txt')",
                "python3 -c 'import sys; exec(sys.stdin.read())' <<< 'print(1)'",
                "cat .codeflow/policy.json | python3 -c 'import sys; print(sys.stdin.read())'",
                "python3 <<< 'print(x)'",
                "echo x | python3",
                "python3 <<< 'print(y)'",
                "echo -e \"python3 tool.py .claude/settings.json\"",
                "printf '%s\\n' 'python3 edit.py .claude/settings.json' > notes.md",
                "env python3 <<< 'print(1)'",
                "xargs python3 tool.py <<< 'notes.txt'",
                "python3 -c 'print(1)' | cat",
                "cat notes.txt | python3 -c 'import sys; print(len(sys.stdin.read()))'",
                "echo -e \"python3 tool.py .claude/settings.json\" | cat",
            ],
        );
    }

    /// Grok round 9: `$'...'` is decoded before a here-string body or a
    /// quoted string is split (F2); adjacent quoted pieces are judged as the
    /// one shell word they form, and a `printf` of `%s` pieces as the value
    /// it joins (F1, shell part).
    #[test]
    fn shell_quoting_and_printf_joins_are_judged_as_the_shell_builds_them() {
        let repo = repo();
        assert_public_refused(
            &repo,
            &[
                "python3 <<< $'open(\".cla\\x75de/settings.json\",\"w\").write(\"x\")'",
                "python3 <<< $'open(\"\\056claude/settings.json\",\"w\").write(\"x\")'",
                "printf -v p '%s%s' '.cla' 'ude/settings.json'; python3 -c \"open(\\\"$p\\\",\\\"w\\\").write(\\\"x\\\")\"",
                "printf -vp '%s%s' '.cla' 'ude/settings.json'; python3 tool.py \"$p\"",
                "echo '.cla''ude/settings.json' | python3 -c 'import sys; open(sys.stdin.read().strip(),\"w\")'",
                "python3 tool.py '.cla''ude/settings.json'",
                "python3 tool.py \".cla\"'ude/settings.json'",
                "x=\".cla\"'ude/settings.json'; python3 -c \"open('$x','w')\"",
            ],
            false,
        );
        assert_public_refused(
            &repo,
            &[
                "python3 <<< $'open(\".gi\\x74/config\").read()'",
                "printf -v p '%s%s' '.gi' 't/config'; python3 -c \"print(open(\\\"$p\\\").read())\"",
                "python3 tool.py '.gi''t/config'",
                "echo \".gi\"'t/config' | python3 -c 'import sys; print(open(sys.stdin.read().strip()).read())'",
            ],
            true,
        );
        assert_public_allowed(
            &repo,
            &[
                "python3 <<< $'print(17330\\x32)'",
                "python3 -c $'print(1)'",
                "printf -v p '%s%s' 'no' 'tes.txt'; python3 tool.py \"$p\"",
                "printf -v p '%s%s' '.cla' 'ude/settings.json'",
                "printf '%s\\n' 'python3 edit.py .claude/settings.json' > notes.md",
                "python3 tool.py 'no''tes.txt'",
                "cat .codeflow/policy.json | python3 -c 'import sys; print(sys.stdin.read())'",
                "python3 -c 'print(\"173\" \"302\")'",
            ],
        );
    }

    /// Grok round 10: a shell construct that composes a path from pieces
    /// the guard sees apart. Exact where cheap (printf escapes and `%b`,
    /// `echo -e`/`print` decoding, empty-separator joins of on-line
    /// assignments); cannot-verify otherwise on a line that runs a shell
    /// or interpreter.
    #[test]
    fn composed_shell_text_is_resolved_or_refused_as_cannot_verify() {
        let repo = repo();
        assert_public_refused(
            &repo,
            &[
                "printf '%b' '.cla\\x75de/settings.json' | python3 -c 'import sys; open(sys.stdin.read().strip(),\"w\")'",
                "printf -v p '%b' '.cla\\x75de/settings.json'; python3 tool.py \"$p\"",
                "printf '%s%b' '.cla' '\\x75de/settings.json' | python3 -c 'import sys; open(sys.stdin.read().strip(),\"w\")'",
                "printf '.cla\\x75de/settings.json\\n' | python3 -c 'import sys; open(sys.stdin.read().strip(),\"w\")'",
                "printf '%q%s' '.cla' 'ude/settings.json' | python3 -c 'import sys; open(sys.stdin.read().strip(),\"w\")'",
                "IFS=; arr=('.cla' 'ude/settings.json'); python3 -c \"open(\\\"${arr[*]}\\\",\\\"w\\\")\"",
                "a='.cla'; b='ude/settings.json'; python3 -c \"open(\\\"${a}${b}\\\",\\\"w\\\")\"",
                "arr=('.cla' 'ude/settings.json'); python3 -c \"open(\\\"${(j::)arr}\\\",\\\"w\\\")\"",
                "echo -e '.cla\\x75de/settings.json' | python3 -c 'import sys; open(sys.stdin.read().strip(),\"w\")'",
                "print '.cla\\x75de/settings.json' | python3 -c 'import sys; open(sys.stdin.read().strip(),\"w\")'",
            ],
            false,
        );
        assert_public_refused(
            &repo,
            &[
                "printf '%b' '.gi\\x74/config'; python3 tool.py \"$p\"",
                "printf '.gi\\x74/config\\n' | python3 -c 'import sys; print(open(sys.stdin.read().strip()).read())'",
                "IFS=; arr=('.gi' 't/config'); python3 -c \"print(open(\\\"${arr[*]}\\\").read())\"",
                "a='.gi'; b='t/config'; python3 -c \"print(open(\\\"${a}${b}\\\").read())\"",
                "echo -e '.gi\\x74/config' | python3 -c 'import sys; print(open(sys.stdin.read().strip()).read())'",
            ],
            true,
        );
        // A text-composing construct the guard cannot resolve, on a line
        // that runs an interpreter: cannot-verify.
        assert_public_refused(
            &repo,
            &[
                "printf '%d' 1 | python3 -c 'import sys; print(sys.stdin.read())'",
                "p=.claude; python3 -c \"open('${p/clau/cla}')\"",
                "x=abcdef; python3 -c \"print('${x:0:3}')\"",
                "arr=(a b); python3 -c \"print('${(U)arr}')\"",
            ],
            false,
        );
        assert_public_allowed(
            &repo,
            &[
                "printf '%b' 'notes.txt' | python3 -c 'import sys; print(sys.stdin.read())'",
                "printf '%s\\n' 'python3 edit.py .claude/settings.json' > notes.md",
                "printf -v p '%s%s' 'no' 'tes.txt'; python3 tool.py \"$p\"",
                "echo -e \"python3 tool.py .claude/settings.json\"",
                "print -r '.cla\\x75de/settings.json'",
                "IFS=; arr=('no' 'tes.txt'); python3 tool.py \"${arr[*]}\"",
                "arr=('no' 'tes.txt'); python3 tool.py \"${arr[*]}\"",
                "python3 tool.py \"$ROOT/$NAME\"",
                "python3 tool.py \"$ROOT/notes.txt\"",
            ],
        );
    }

    /// Grok round 11: layer 1 is closed-world. One payload per construct
    /// family it used to resolve wrongly composes an enforcement path on a
    /// line where an interpreter runs, and refuses, resolved exactly or as
    /// cannot-verify; the same constructs over ordinary files stay allowed.
    #[test]
    fn each_composing_family_resolves_exactly_or_refuses() {
        let repo = repo();
        assert_public_refused(
            &repo,
            &[
                // An assignment whose value holds an expansion.
                "a=.cla; b=\"${a}ude/settings.json\"; python3 -c \"open('$b','w')\"",
                // A declaration builtin, and an indexed element.
                "export a=.cla; python3 -c \"open('${a}ude/settings.json','w')\"",
                "a[0]=.cla; python3 -c \"open('${a}ude/settings.json','w')\"",
                // An echo option cluster that decodes.
                "echo -ne '.cla\\x75de/settings.json' | python3 -c 'import sys; open(sys.stdin.read(),\"w\")'",
                // A printf format given through a variable.
                "f='.cla\\x75de/settings.json'; printf \"$f\" | python3 -c 'import sys; open(sys.stdin.read(),\"w\")'",
                // A unicode escape.
                "printf '.cla\\u0075de/settings.json' | python3 -c 'import sys; open(sys.stdin.read(),\"w\")'",
                // Indirect expansion.
                "n=p; p=.claude/settings.json; python3 -c \"open('${!n}','w')\"",
                // A command substitution over a modelled builtin.
                "python3 -c \"open('$(printf .cla)ude/settings.json','w')\"",
                // Round 12: a path-qualified program's output is outside
                // text, never its spelled arguments, so a fixed absolute
                // argument no longer hides the path the shell builds.
                "python3 -c \"open('$(/bin/cat /tmp/dir.txt)/.claude/settings.json','w')\"",
                // Round 13: a glob an output builtin writes is judged by
                // its pattern, directly and through a substitution.
                "echo .cl?ude/settings.js?n | python3 -c 'import sys; open(sys.stdin.read().strip(),\"w\")'",
                "echo .cla* | python3 -c 'import sys; open(sys.stdin.read().strip()+\"/settings.json\",\"w\")'",
                "python3 -c \"open('$(echo .[c]laude)'+'/settings.json','w')\"",
                // Round 14: the glob in a later field, beside outside text,
                // through a substitution, and as a later printf argument.
                "echo ok .cla* | python3 -c 'import sys; open(sys.stdin.read().split()[1]+\"/settings.json\",\"w\")'",
                "echo \"$TAG\" .cl?ude/settings.js?n | python3 -c 'import sys; open(sys.stdin.read().split()[-1],\"w\")'",
                "python3 -c \"open('$(echo ok .[c]laude)'.split()[1]+'/settings.json','w')\"",
                "printf '%s %s\\n' ok .cla* | python3 -c 'import sys; open(sys.stdin.read().split()[1]+\"/settings.json\",\"w\")'",
                // Round 15: an empty expansion inside one written word
                // joins the spelled directory and final component.
                "echo .claude/$(/usr/bin/true)settings.json | python3 -c 'import sys; print(sys.stdin.read())'",
                "print -r -- .claude/$(/bin/cat /dev/null)settings.json | python3 -c 'import sys; print(sys.stdin.read())'",
            ],
            false,
        );
        // The same join naming policy authority refuses at every level.
        assert_public_refused(
            &repo,
            &[
                "echo .git/$(/bin/cat /dev/null)config | python3 -c 'import sys; print(sys.stdin.read())'",
                "echo .git/${nope}config | python3 -c 'import sys; print(sys.stdin.read())'",
            ],
            true,
        );
        assert_public_allowed(
            &repo,
            &[
                "printf '%s\\n' 'python3 edit.py .claude/settings.json' > notes.md",
                "echo -e \"python3 tool.py .claude/settings.json\"",
                "print -r '.cla\\x75de/settings.json'",
                "python3 tool.py \"$ROOT/$NAME\"",
                "IFS=; arr=('no' 'tes.txt'); python3 tool.py \"${arr[*]}\"",
                "a=no; b=\"${a}tes.txt\"; python3 tool.py \"$b\"",
                "export a=notes.txt; python3 tool.py \"$a\"",
                "f='%s%s\\n'; printf \"$f\" no tes.txt | python3 -c 'import sys; print(sys.stdin.read())'",
                "echo -ne 'notes.txt' | python3 -c 'import sys; print(sys.stdin.read())'",
                "for f in src/*.py; do python3 lint.py \"$f\"; done",
                "find src -name '*.py' -exec sh -c 'python3 lint.py \"$1\"' sh {} \\;",
                // Program output is outside text (operator boundary, 2026-10-15).
                "ROOT=$(git rev-parse --show-toplevel); python3 \"$ROOT/src/app.py\"",
                "source venv/bin/activate && echo ok && python3 src/app.py",
                "echo \"Done $(date)\"; python3 src/app.py",
                "while read f; do python3 lint.py \"$f\"; done < files.txt",
                // Ordinary globs an output builtin writes.
                "echo src/*.py | python3 -c 'import sys; print(sys.stdin.read())'",
                "echo *.txt; python3 src/app.py",
                "python3 lint.py \"$(echo src/*.py)\"",
                "echo ok src/*.py | python3 -c 'import sys; print(sys.stdin.read())'",
                "echo \"$TAG\" *.txt; python3 src/app.py",
                "python3 lint.py \"$(echo ok src/*.py)\"",
                "echo notes$(/bin/cat /dev/null).txt | python3 -c 'import sys; print(sys.stdin.read())'",
                "echo notes.txt \"$(/bin/cat /dev/null)\"; python3 src/app.py",
                // Round 16: a quoted parameter named like a protected
                // component is a name, not spelled text.
                "echo \"$config\" | python3 -c 'import sys; print(sys.stdin.read())'",
                "python3 -c 'import sys; print(sys.argv[1:])' \"$config\"",
                "echo \"$hooks\" | python3 -c 'import sys; print(sys.stdin.read())'",
                "echo \"$policy.json\" | python3 -c 'import sys; print(sys.stdin.read())'",
                "echo \"$settings.json\" | python3 -c 'import sys; print(sys.stdin.read())'",
            ],
        );
    }
}
