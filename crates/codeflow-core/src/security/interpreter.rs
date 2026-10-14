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
        assert_eq!(calls[0].args, ["a.py", "b"]);
        // Every word after the code or script boundary is an argument, a
        // second `-c` included; option values are kept, flags are not.
        let args = |line: &str| outward::interpreter_calls(line, cwd)[0].args.clone();
        assert_eq!(args("python3 -c 'x' -c s.json"), ["-c", "s.json"]);
        assert_eq!(args("python3 -Ic'x' a"), ["a"]);
        assert_eq!(
            args("python3 -W ignore -m json.tool a b"),
            ["ignore", "json.tool", "a", "b"]
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
            outward::interpreter_calls("ruby -C d -e 'x' f", cwd)[0]
                .chdir
                .as_deref(),
            Some("d")
        );
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
        // `cd` options are read past, and an `&&` chain runs a command in
        // the one directory its changes lead to.
        let dirs = |line: &str| -> Vec<Option<PathBuf>> {
            outward::interpreter_calls(line, cwd)
                .into_iter()
                .map(|c| c.cwd)
                .collect()
        };
        assert_eq!(
            dirs("cd -- sub && python3 x"),
            [Some(PathBuf::from("/w/sub"))]
        );
        assert_eq!(
            dirs("cd -P -L sub && python3 x"),
            [Some(PathBuf::from("/w/sub"))]
        );
        assert_eq!(
            dirs("cd a && cd b && python3 x"),
            [Some(PathBuf::from("/w/a/b"))]
        );
        assert_eq!(
            dirs("cd a; python3 x"),
            [Some(PathBuf::from("/w")), Some(PathBuf::from("/w/a"))]
        );
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
                "cd .claude && cd .. && python3 tool.py settings.json",
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
}
