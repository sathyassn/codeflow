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
        for unwrapped in outward::unwrapped_commands(command, cwd, home) {
            let args = &unwrapped.args;
            let name = args
                .first()
                .map_or("", |s| super::unresolved::writer_name(s));
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
                // Only a word the shell expands names files it never spells;
                // a quoted search pattern such as `'^/etc/.*'` is text.
                let expands = |at: usize| {
                    unwrapped
                        .expands
                        .as_ref()
                        .is_none_or(|flags| flags.get(at).copied().unwrap_or(true))
                };
                let reader_cwd = unwrapped.cwd.as_deref().unwrap_or(cwd);
                if let Some((_, path)) = args
                    .iter()
                    .enumerate()
                    .skip(1)
                    .find(|(at, word)| secret_path(word, reader_cwd, home, Some(expands(*at))))
                {
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
            if levels.secret_reads.is_active() && secret_path(&word, cwd, home, None) {
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

/// `reader` is `Some` for a reader's argument, saying whether the shell
/// expands it as a pattern, and `None` for a literal in interpreter code.
fn secret_path(word: &str, cwd: &Path, home: Option<&Path>, reader: Option<bool>) -> bool {
    let expanded_word = word
        .strip_prefix("$HOME/")
        .or_else(|| word.strip_prefix("${HOME}/"))
        .map(|suffix| format!("~/{suffix}"));
    let word = expanded_word.as_deref().unwrap_or(word);
    // A named home may be another user's, but the configured store suffix
    // remains sensitive. Never let a leading slash replace the joined home.
    let tilde_word = word
        .strip_prefix('~')
        .and_then(|tail| tail.split_once('/'))
        .map(|(_, suffix)| format!("~/{}", suffix.trim_start_matches('/')));
    let word = tilde_word.as_deref().unwrap_or(word);
    // Keep the literal spelling for home patterns even when HOME is unknown.
    // Also judge relative store names from the payload cwd, without reading
    // files or resolving symlinks. Do not reduce paths to their basename:
    // auth.json is protected in its store, not in every project directory.
    let absolute = if let Some(suffix) = word.strip_prefix("~/") {
        home.map(|home| outward::lexical_join(home, Path::new(suffix)))
    } else {
        Some(outward::lexical_join(cwd, Path::new(word)))
    };
    secret_path_spelling(word, home, reader)
        || absolute.as_deref().is_some_and(|path| {
            secret_path_spelling(&crate::portable_path::slashed(path), home, reader)
        })
}

fn secret_path_spelling(word: &str, home: Option<&Path>, reader: Option<bool>) -> bool {
    let word = word.to_ascii_lowercase();
    let home_relative = home.and_then(|home| {
        let home = crate::portable_path::slashed(home).to_ascii_lowercase();
        word.strip_prefix(home.trim_end_matches('/'))
            .filter(|tail| tail.starts_with('/'))
            .map(|tail| format!("~{tail}"))
    });
    let word = home_relative.as_deref().unwrap_or(&word);
    actions::table().sandbox_read_denies.iter().any(|path| {
        word == path
            || word
                .strip_prefix(path)
                .is_some_and(|tail| tail.starts_with('/'))
            || reader.is_some_and(|expands| secret_prefix(word, path, expands))
    }) || actions::table().claude_read_groups.iter().any(|group| {
        // This group has no carve-outs; source-name and env-file groups do.
        group.carveouts.is_empty()
            && group.denies.iter().any(|rule| {
                let Some(pattern) = rule.strip_prefix("Read(").and_then(|s| s.strip_suffix(')'))
                else {
                    return false;
                };
                matches_extended_glob(word, pattern)
                    || reader.is_some_and(|expands| secret_prefix(word, pattern, expands))
            })
    })
}

/// A reader can traverse a named directory or expand a pattern without naming
/// the protected leaf. Only a nonempty literal prefix supplies that evidence.
/// A word the shell does not expand is all literal: its wildcards are text.
fn secret_prefix(word: &str, store: &str, expands: bool) -> bool {
    let wildcard = |c| matches!(c, '*' | '?' | '[' | '{');
    let prefix = if expands {
        word.split(wildcard).next().unwrap_or("")
    } else {
        word
    };
    if prefix.is_empty() {
        return false;
    }
    let patterned = prefix.len() != word.len();
    if let Some(basename) = store.strip_prefix("**/") {
        let stem = basename.split(wildcard).next().unwrap_or("");
        let name = prefix.rsplit('/').next().unwrap_or("");
        return patterned && !name.is_empty() && !stem.is_empty() && stem.starts_with(name);
    }
    let stem = store.split(wildcard).next().unwrap_or("");
    if patterned {
        // A wildcard at the home root names no store; `*.md` stays ordinary.
        prefix != "~/" && stem.starts_with(prefix)
    } else {
        stem.strip_prefix(prefix.trim_end_matches('/'))
            .is_some_and(|tail| tail.starts_with('/'))
    }
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
    fn hard_secret_filename_spellings_receive_the_same_verdict() {
        for name in [
            "id_rsa",
            "id_rsa.pub",
            "id_ed25519",
            "id_ed25519.pub",
            "certificate.pem",
            "private.key",
            "bundle.p12",
            "bundle.pfx",
            ".netrc",
        ] {
            for prefix in ["", "./", "nested/", "/fixture/", "~/", "$HOME/", "${HOME}/"] {
                let path = format!("{prefix}{name}");
                for command in [
                    format!("cat {path}"),
                    format!("python3 -c 'open(\"{path}\")'"),
                ] {
                    assert!(
                        check(&command, &SecuritySection::default())
                            .iter()
                            .any(|v| v.rule == "security.secret_reads"),
                        "{command}"
                    );
                }
            }
        }
    }

    #[test]
    fn configured_secret_stores_match_relative_to_the_session_directory() {
        let table = actions::table();
        let stores = table.sandbox_read_denies.iter().cloned().chain(
            table
                .claude_read_groups
                .iter()
                .filter(|group| group.carveouts.is_empty())
                .flat_map(|group| &group.denies)
                .filter_map(|rule| {
                    rule.strip_prefix("Read(~/")
                        .and_then(|rule| rule.strip_suffix(')'))
                })
                .map(|path| {
                    format!(
                        "~/{}",
                        path.replace("**/", "nested/")
                            .replace("**", "entry")
                            .replace('*', "fixture")
                    )
                }),
        );
        for store in stores {
            let suffix = store.strip_prefix("~/").unwrap();
            let home = Path::new("/fixture-home");
            let absolute = home.join(suffix);
            let parent = absolute.parent().unwrap();
            let basename = absolute.file_name().unwrap().to_str().unwrap();
            for (cwd, spelling) in [
                (parent, basename.to_string()),
                (parent, format!("./{basename}")),
                (home, suffix.to_string()),
                (home, crate::portable_path::slashed(&absolute)),
                (home, store.clone()),
                (home, format!("$HOME/{suffix}")),
                (home, format!("${{HOME}}/{suffix}")),
            ] {
                let findings = evaluate(
                    &format!("cat {spelling}"),
                    &SecuritySection::default(),
                    PolicyLevel::Block,
                    cwd,
                    Path::new("/fixture"),
                    Some(home),
                );
                assert!(
                    findings.iter().any(|v| v.rule == "security.secret_reads"),
                    "{cwd:?}: {spelling}"
                );
            }
        }
    }

    #[test]
    fn ordinary_names_and_carveout_groups_keep_their_shell_verdict() {
        for name in [
            "README.md",
            "auth.json",
            "./auth.json",
            "id_ordinary",
            "identity_rsa",
            "certificate.pem.txt",
            "private.key.rs",
            ".netrc-notes",
            "~/.ssh-notes/file",
            "~/.codex/auth.json.old",
            "~/.password-store-notes/item",
            ".env",
            ".env.production",
            ".env.example",
            ".env.sample",
            ".env.template",
            ".env.dist",
            "secret.txt",
            "credentials.json",
            "secret_scan.rs",
            "credentials.py",
        ] {
            assert!(
                !check(&format!("cat {name}"), &SecuritySection::default())
                    .iter()
                    .any(|v| v.rule == "security.secret_reads"),
                "{name}"
            );
        }
    }

    fn assert_secret_commands(commands: &[&str], cwd: &Path) {
        for command in commands {
            assert!(
                evaluate(
                    command,
                    &SecuritySection::default(),
                    PolicyLevel::Block,
                    cwd,
                    Path::new("/fixture"),
                    Some(Path::new("/fixture-home"))
                )
                .iter()
                .any(|v| v.rule == "security.secret_reads"),
                "{command}"
            );
        }
    }

    #[test]
    fn review_f1_home_aliases_cannot_hide_stores() {
        assert_secret_commands(
            &[
                "cat ~//.codex/auth.json",
                "cat $HOME//.codex/auth.json",
                "cat ${HOME}///.codex/auth.json",
                "cat ~someone/.codex/auth.json",
                "cat ~$USER/.codex/auth.json",
                "cat ~someone/.aws/credentials",
            ],
            Path::new("/fixture"),
        );
    }

    #[test]
    fn review_f2_secret_spelling_is_ascii_case_insensitive() {
        assert_secret_commands(
            &[
                "cat ID_RSA",
                "cat ~/.Codex/auth.json",
                "cat ~/.codex/AUTH.JSON",
                "cat ~/.AWS/credentials",
                "cat .PYPIRC",
            ],
            Path::new("/fixture-home"),
        );
        for store in &actions::table().sandbox_read_denies {
            assert_secret_commands(
                &[&format!("cat {}", store.to_ascii_uppercase())],
                Path::new("/fixture"),
            );
        }
    }

    #[test]
    fn review_f3_patterns_directories_and_input_redirects_reach_stores() {
        assert_secret_commands(
            &[
                "cat ~/.codex/auth.*",
                "cat ~/.codex/auth.jso?",
                "cat ~/.pypir?",
                "cat id_rs?",
                "cat ~/.codex/auth.{json}",
                "cat ~/.codex/*",
                "grep -r token ~/.codex",
                "cat ~/.ssh",
                "cat < ~/.ssh/id_rsa",
                "sh -c 'cat < ~/.codex/auth.json'",
                "wc 0<~/.codex/auth.json",
                "cat auth.*",
                "cat auth.jso?",
                "cat auth.{json}",
            ],
            Path::new("/fixture-home/.codex"),
        );
    }

    #[test]
    fn review_f4_reader_paths_follow_directory_changes() {
        assert_secret_commands(
            &[
                "cd ~/.codex && cat auth.json",
                "cd ~/.gem && cat credentials",
                "sh -c 'cd ~/.codex && cat auth.json'",
                "cd /fixture-home && cd .codex && cat auth.json",
                "cd $HOME/.codex && cat < auth.json",
            ],
            Path::new("/fixture"),
        );
    }

    #[test]
    fn review_controls_keep_patterns_and_redirect_data_readable() {
        for command in [
            "cat src/*.rs",
            "cat *.md",
            "cat src/{a,b}.rs",
            "cat ~/.codex-notes/*",
            "cat ~/.codex/auth.json.old",
            "cat < README.md",
            "echo '< ~/.codex/auth.json'",
            "cat <<'~/.codex/auth.json'",
            "cat <<< '~/.codex/auth.json'",
            "cd src && cat auth.json",
            "cat ID_ORDINARY",
        ] {
            assert!(
                !check(command, &SecuritySection::default())
                    .iter()
                    .any(|v| v.rule == "security.secret_reads"),
                "{command}"
            );
        }
    }

    #[test]
    fn a_quoted_wildcard_is_text_and_an_unquoted_one_expands() {
        let reads = |command: &str| {
            evaluate(
                command,
                &SecuritySection::default(),
                PolicyLevel::Block,
                Path::new("/fixture"),
                Path::new("/fixture"),
                Some(Path::new("/fixture-home")),
            )
            .iter()
            .any(|v| v.rule == "security.secret_reads")
        };
        for command in [
            "rg '^/etc/.*' docs",
            "grep -e '^/etc/.*' docs",
            "rg --files | xargs rg '^/etc/.*'",
            "cat 'id_rs?'",
            "cat \"~/.codex/auth.*\"",
        ] {
            assert!(!reads(command), "{command}");
        }
        for command in [
            "cat /etc/.*",
            "cat id_rs?",
            "cat ~/.codex/auth.*",
            "rg x ~/.codex/*",
        ] {
            assert!(reads(command), "{command}");
        }
    }

    #[test]
    fn reader_prefixes_do_not_turn_code_fragments_into_store_reads() {
        for command in ["cat *.md", "cat src/*.rs", r#"python3 -c 'print("/")'"#] {
            assert!(
                !evaluate(
                    command,
                    &SecuritySection::default(),
                    PolicyLevel::Block,
                    Path::new("/fixture-home"),
                    Path::new("/fixture"),
                    Some(Path::new("/fixture-home"))
                )
                .iter()
                .any(|v| v.rule == "security.secret_reads"),
                "{command}"
            );
        }
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
        // These placeholders occur unquoted, inside shell quotes and inside
        // code literals. Use slash paths so shell escape removal cannot
        // change their target. The Windows matrix below quotes native forms.
        let shell_path = |path: &Path| {
            crate::portable_path::slashed(&crate::portable_path::without_verbatim(
                path.to_path_buf(),
            ))
        };
        let command = command
            .replace("@ROOT@", &shell_path(&repo.root))
            .replace("@LINKED@", &shell_path(&repo.linked));
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

    fn argument_refusal(findings: &[Violation]) -> bool {
        integrity_refusal(findings).is_some_and(|v| {
            v.message.contains("interpreter") && !v.message.contains("interpreter code")
        })
    }

    /// Every command refuses under `git.hook_integrity` from `dir`, each
    /// with a finding about an interpreter argument, not its code.
    fn assert_refused(repo: &Repo, dir: &Path, root: &Path, commands: &[&str]) {
        let mut wrong = Vec::new();
        for command in commands {
            let findings = judge_in(repo, dir, root, command, PolicyLevel::Block);
            if !argument_refusal(&findings) {
                wrong.push(format!("should refuse: {command}: {findings:?}"));
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
                "node tool.js .worktrees/w/.git",
                "python3 tool.py ~/repo/.worktrees/w/.git",
                "python3 tool.py .claude",
                "python3 tool.py .worktrees/w",
                "python3 tool.py .worktrees",
            ],
        );
        assert_refused(&repo, &repo.linked, &repo.linked, &["python3 tool.py .git"]);
        std::fs::write(repo.root.join("case-probe"), "probe").unwrap();
        let folds_case = repo.root.join("CASE-PROBE").exists();
        let findings = judge_in(
            &repo,
            &repo.root,
            &repo.root,
            "python3 tool.py .WORKTREES/W/.GIT",
            PolicyLevel::Block,
        );
        assert_eq!(argument_refusal(&findings), folds_case, "{findings:?}");
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
                "env -C '{}' python3 tool.py away/w",
                other.root.display()
            )],
        );
    }

    #[test]
    fn unquoted_shell_patterns_reach_protected_paths() {
        let repo = repo();
        assert_refused(
            &repo,
            &repo.root,
            &repo.root,
            &[
                "python3 tool.py .worktrees/w/*(D)",
                "python3 tool.py .claude/*(D)",
                "python3 tool.py .cla(ude)/settings.json",
                "python3 tool.py .worktrees/w/(.git|x)",
                "python3 tool.py .worktrees/w/.g(i)t",
                "python3 tool.py .worktrees/w/@(.git|.claude)",
                "python3 tool.py .worktrees/w/^src(D)",
                "python3 tool.py *(D)",
                "python3 tool.py .worktrees/w/<1-9>",
                "env python3 tool.py \".worktrees/w/\"*(D)",
                "nice python3 tool.py .worktrees/w/*(D)",
            ],
        );
    }

    #[test]
    fn interpreter_project_locations_stay_allowed() {
        let repo = repo();
        let standalone = repo.root.parent().unwrap().join("standalone");
        git2::Repository::init(&standalone).unwrap();
        let mut wrong = Vec::new();
        for root in [&standalone, &repo.root, &repo.linked] {
            let discovered = crate::hooks::repo::open(root).unwrap().unwrap();
            assert!(
                crate::hooks::edit_guard::is_own_checkout_location(root, root, None).unwrap(),
                "checkout {root:?}, discovered workdir {:?}",
                discovered.workdir()
            );
            std::fs::create_dir_all(root.join("src")).unwrap();
            for (dir, command) in [
                (root.clone(), "python3 tool.py ."),
                (root.clone(), "python3 -m pytest ."),
                (root.clone(), "python3 -m black ."),
                (root.clone(), "python3 -m pip install -e ."),
                (root.clone(), "python3 -m ruff check ."),
                (root.clone(), "node x.js ."),
                (root.join("src"), "python3 tool.py .."),
                (root.join("src"), "python3 tool.py ."),
            ] {
                let findings = judge_in(&repo, &dir, root, command, PolicyLevel::Block);
                if !findings.is_empty() {
                    wrong.push(format!("{dir:?}: {command}: {findings:?}"));
                }
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    #[test]
    fn code_tilde_is_literal_when_home_contains_the_checkout() {
        let mut repo = repo();
        repo.home = repo.root.parent().unwrap().to_path_buf();
        assert_allowed(&repo, &[r#"python3 -c 'print("~")'"#]);
        assert_refused(&repo, &repo.root, &repo.root, &["python3 tool.py ~"]);
        assert_allowed(
            &repo,
            &[
                r#"python3 -c 'import os; open(os.path.expanduser("~")+"/."+"zsh"+"rc","a")'"#,
                r#"node -e 'require("fs").rmSync(require("os").homedir()+"/repo/.worktrees/w", {recursive:true})'"#,
            ],
        );
    }

    #[test]
    fn code_tilde_paths_judge_literal_and_home_readings() {
        let mut repo = repo();
        repo.home = repo.root.parent().unwrap().to_path_buf();
        assert_refused(
            &repo,
            &repo.root,
            &repo.root,
            &[
                r#"python3 -c 'import shutil,os; shutil.rmtree(os.path.expanduser("~/repo/.worktrees/w"))'"#,
                r#"python3 -c 'import shutil,os; shutil.rmtree(os.path.expanduser("~/repo/.worktrees"))'"#,
                r#"python3 -c 'import shutil,os; shutil.rmtree(os.path.expanduser("~/repo/.claude"))'"#,
            ],
        );
        // The literal reading still protects a repository beneath a directory
        // actually named `~`, even when its home-expanded counterpart is absent.
        let nested = repo.root.join("~/literal");
        std::fs::create_dir(nested.parent().unwrap()).unwrap();
        std::fs::rename(&repo.linked, &nested).unwrap();
        std::fs::write(
            repo.root.join(".git/worktrees/w/gitdir"),
            format!("{}\n", nested.join(".git").display()),
        )
        .unwrap();
        assert_refused(
            &repo,
            &repo.root,
            &repo.root,
            &[r#"python3 -c 'open("~/literal/.claude/settings.json", "w")'"#],
        );
    }

    #[test]
    fn escaped_native_code_paths_keep_checkout_identity() {
        let repo = repo();
        for (path, refuse) in [
            (repo.linked.clone(), true),
            (repo.root.join("src/app.py"), false),
        ] {
            for literal in [
                serde_json::to_string(path.to_str().unwrap()).unwrap(),
                format!("\"{}\"", path.display()),
            ] {
                let command = format!("python3 -c 'open({literal}, \"w\")'");
                let findings =
                    judge_in(&repo, &repo.root, &repo.root, &command, PolicyLevel::Block);
                assert_eq!(
                    integrity_refusal(&findings).is_some(),
                    refuse,
                    "{command}: {findings:?}"
                );
            }
        }
    }

    #[test]
    fn linked_checkout_ancestors_refuse() {
        let repo = repo();
        let commands = [
            "python3 tool.py ../..".to_string(),
            "python3 tool.py \"@ROOT@\"".to_string(),
            "python3 tool.py ../../.worktrees".to_string(),
            "python3 tool.py ..".to_string(),
        ];
        #[cfg(unix)]
        let commands = {
            let mut commands = Vec::from(commands);
            let alias = repo.root.parent().unwrap().join("ancestor-alias");
            std::os::unix::fs::symlink(&repo.root, &alias).unwrap();
            commands.push(format!("python3 tool.py \"{}\"", alias.display()));
            commands
        };
        assert_refused(
            &repo,
            &repo.linked,
            &repo.linked,
            &commands.iter().map(String::as_str).collect::<Vec<_>>(),
        );
    }

    #[cfg(unix)]
    #[test]
    fn external_aliases_cannot_hide_linked_git_pointers() {
        let repo = repo();
        let base = repo.root.parent().unwrap();
        let alias = base.join("alias");
        let plain = base.join("plain");
        std::fs::create_dir(&plain).unwrap();
        std::os::unix::fs::symlink(&repo.root, &alias).unwrap();
        std::os::unix::fs::symlink(&repo.linked, plain.join("x")).unwrap();
        for path in [
            alias.join(".worktrees/w/.git"),
            alias.join(".worktrees/w/.g*"),
            alias.join(".worktrees/*/.git"),
            plain.join("x/.git"),
        ] {
            assert_refused(
                &repo,
                &repo.root,
                &repo.root,
                &[&format!("python3 tool.py {}", path.display())],
            );
            for command in [
                format!("cp x {}", path.display()),
                format!("echo hi > {}", path.display()),
            ] {
                let policy = crate::hooks::policy::GitPolicy::default();
                let context = crate::hooks::git_guard::GuardContext {
                    policy: &policy,
                    current_branch: "task/alias",
                    integrate_token: false,
                    pr_base_lookup: None,
                    dir_target_lookup: None,
                    alias_lookup: None,
                    discard_lookup: None,
                    branch_lookup: None,
                    root_checkout: None,
                };
                let findings =
                    crate::hooks::git_guard::evaluate_report_at(&command, &context, &repo.root)
                        .violations;
                assert!(
                    integrity_refusal(&findings).is_some(),
                    "{command}: {findings:?}"
                );
            }
        }
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

    /// Large ordinary globs allow only with a safe enclosing directory;
    /// an escape or a held checkout still reports the entry budget.
    #[test]
    fn interpreter_large_glob_requires_a_safe_prefix() {
        let repo = repo();
        let many = repo.root.join("many");
        std::fs::create_dir(&many).unwrap();
        for n in 0..5000 {
            std::fs::write(many.join(format!("f{n}")), "").unwrap();
        }
        let mut wrong = Vec::new();
        let mut check = |command: &str, refuse: bool| {
            let findings = judge_in(&repo, &repo.root, &repo.root, command, PolicyLevel::Block);
            let correct = if refuse {
                integrity_refusal(&findings)
                    .is_some_and(|v| v.message.contains("more than 4096 entries"))
            } else {
                findings.is_empty()
            };
            if !correct {
                wrong.push(format!("{command}: refuse={refuse}: {findings:?}"));
            }
        };
        check("python3 tool.py many/*", false);
        check("python3 tool.py many/*/../../.claude/settings.json", true);
        check("python3 tool.py many/*/../.claude/x", true);
        let moved = many.join("linked");
        std::fs::rename(&repo.linked, &moved).unwrap();
        std::fs::write(
            repo.root.join(".git/worktrees/w/gitdir"),
            format!("{}\n", moved.join(".git").display()),
        )
        .unwrap();
        check("python3 tool.py many/*", true);
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    /// Code string fragments do not acquire recursive checkout-operand
    /// semantics. Shell arguments after the code still take those checks.
    #[test]
    fn interpreter_code_fragments_are_not_checkout_operands() {
        let repo = repo();
        assert_allowed(
            &repo,
            &[
                r#"python3 -c 'import os; open(os.path.expanduser("~")+"/."+"zsh"+"rc","a")'"#,
                r#"python3 -c 'print("/.")'"#,
                r#"python3 -c 'print("/..")'"#,
            ],
        );
        assert_refused(
            &repo,
            &repo.linked,
            &repo.linked,
            &[
                "python3 -c 'pass' ../..",
                "python3 tool.py ../..",
                "python3 tool.py .git",
                "python3 tool.py .claude/*(D)",
            ],
        );
        assert_refused(
            &repo,
            &repo.root,
            &repo.root,
            &["python3 -c 'open(\"@LINKED@\", \"w\")'"],
        );
        let main = repo.root.to_string_lossy().replace('\\', "/");
        let parent = repo
            .root
            .parent()
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let src = repo.root.join("src");
        let mut wrong = Vec::new();
        for (cwd, paths) in [
            (
                &repo.linked,
                vec!["../..", "..", "../../..", "../../.worktrees", &main],
            ),
            (&repo.root, vec!["..", &parent]),
            (&src, vec!["../.."]),
        ] {
            for path in paths {
                for command in [
                    format!(r#"python3 -c 'import shutil; shutil.rmtree("{path}")'"#),
                    format!(r#"node -e 'require("fs").rmSync("{path}",{{recursive:true}})'"#),
                    format!(r#"perl -e 'use File::Path qw(remove_tree); remove_tree("{path}")'"#),
                    format!(r#"ruby -e 'require "fileutils"; FileUtils.rm_rf("{path}")'"#),
                ] {
                    let findings = judge_in(&repo, cwd, cwd, &command, PolicyLevel::Block);
                    if integrity_refusal(&findings).is_none() {
                        wrong.push(format!("should refuse in {cwd:?}: {command}: {findings:?}"));
                    }
                }
            }
        }
        // Code roots remain fragments, including normalized dot components.
        let filesystem_root = repo.root.ancestors().last().unwrap().to_string_lossy();
        let plain_root = filesystem_root.trim_start_matches(r"\\?\");
        #[cfg(windows)]
        let native_roots = [plain_root.to_string(), format!(r"\\?\{plain_root}")];
        #[cfg(not(windows))]
        let native_roots = [plain_root.to_string()];
        for cwd in [&repo.root, &repo.linked, &src] {
            // Exercise native normalization directly for verbatim roots too.
            for root in &native_roots {
                for suffix in ["", ".", ".."] {
                    let path = format!("{root}{suffix}");
                    assert!(
                        crate::hooks::edit_guard::is_own_checkout_location(
                            Path::new(&path),
                            cwd,
                            Some(&path),
                        )
                        .unwrap(),
                        "code root {path:?} in {cwd:?}"
                    );
                    assert!(
                        !crate::hooks::edit_guard::is_own_checkout_location(
                            Path::new(&path),
                            cwd,
                            None,
                        )
                        .unwrap(),
                        "argv root {path:?} in {cwd:?}"
                    );
                }
            }
            for suffix in ["", ".", ".."] {
                let path = format!("{}{suffix}", plain_root.replace('\\', "/"));
                let command = format!(r#"python3 -c 'print("{path}")'"#);
                let findings = judge_in(&repo, cwd, cwd, &command, PolicyLevel::Block);
                if !findings.is_empty() {
                    wrong.push(format!("should allow in {cwd:?}: {command}: {findings:?}"));
                }
            }
            let command =
                r#"python3 -c 'import os; open(os.path.expanduser("~")+"/."+"zsh"+"rc","a")'"#;
            let findings = judge_in(&repo, cwd, cwd, command, PolicyLevel::Block);
            if !findings.is_empty() {
                wrong.push(format!("should allow in {cwd:?}: {command}: {findings:?}"));
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
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
        let paths = crate::hooks::git_guard::glob_paths(&word, &repo.root, &repo.root).unwrap();
        for name in [".git", ".claude"] {
            let expected = crate::portable_path::canonicalize(&repo.linked.join(name)).unwrap();
            assert!(
                paths.contains(&expected),
                "expected {expected:?}: {paths:?}"
            );
        }
        assert_refused(
            &repo,
            &repo.root,
            &repo.root,
            &[
                "python3 tool.py @LINKED@/.*",
                "python3 tool.py \"@LINKED@/\".*",
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
                // Preserve the native spelling through POSIX double quotes.
                // In particular, `\\` must reach the path reader as two
                // separators in a verbatim namespace prefix.
                let quoted = spelling.replace('\\', "\\\\");
                let ordinary = format!(r#"W="{quoted}"; python3 tool.py "$W/src/app.py""#);
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
                    let command = format!(r#"python3 tool.py "{quoted}/"{suffix}"#);
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
            refusal.message.contains(".codeflow/policy.json"),
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
                "python3 tool.py \"my file(1).txt\"",
                "python3 tool.py 'notes (draft).md'",
                "node x.js --filter \"(a|b)\"",
                "python3 tool.py .",
                "python3 -m pytest .",
                "python3 -m black .",
                "python3 -m pip install -e .",
                "python3 -m ruff check .",
                "node x.js .",
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
                "for f in src/*.py; do python3 lint.py \"$f\"; done",
                "ls | xargs python3 edit.py",
                "cat > notes.md <<'EOF'\nexample: python3 edit.py .claude/settings.json\nEOF",
                "printf '%s\\n' 'python3 edit.py .claude/settings.json' > notes.md",
                "cd ./src && python3 app.py notes.txt",
                "python3 edit.py \"@ROOT@/src/app.py\"",
            ],
        );
    }

    /// TSK-397 (operator decision): after `cd`, the guard keeps judging
    /// interpreter arguments from the starting directory too. Where a `cd`
    /// lands depends on shell state the guard cannot see, so narrowing
    /// cannot be made sound; a parent path that is legitimate from the
    /// destination still refuses when it leaves the checkout from the start.
    #[test]
    fn cd_keeps_the_starting_directory() {
        let repo = repo();
        std::fs::create_dir_all(repo.root.join("lib")).unwrap();
        assert_refused(
            &repo,
            &repo.root,
            &repo.root,
            &[
                "cd src && python3 tool.py ..",
                "cd src && python3 tool.py ../.claude/settings.json",
                "cd src && python3.13 tool.py ..",
                "cd src && node tool.js ..",
                "cd src && ruby tool.rb ..",
                "cd src && perl tool.pl ..",
                "cd -P src && python3 tool.py ..",
                "cd -- src && python3 tool.py ..",
                // A bare name may be found through a `CDPATH` only the shell
                // holds, so its destination is unknown (Codex finding 3).
                "cd src && python3 app.py settings.json",
                // Any `cd` may run a function or hook first (Codex round 3).
                "cd ./src && python3 app.py settings.json",
                "cd /tmp && python3 app.py settings.json",
            ],
        );
        assert_allowed(
            &repo,
            &[
                "cd ./src && python3 tool.py ../lib",
                "cd ./src && python3 app.py notes.txt",
            ],
        );
    }

    #[test]
    fn other_cd_joins_keep_the_starting_directory() {
        let repo = repo();
        assert_refused(
            &repo,
            &repo.root,
            &repo.root,
            &[
                "cd src; python3 tool.py ..",
                "cd src || python3 tool.py ..",
                "(cd src) && python3 tool.py ..",
                "cd src && true || python3 tool.py ..",
                "cd src && true; python3 tool.py ..",
                "cd src & python3 tool.py ..",
                "cd src | python3 tool.py ..",
                "! cd src && python3 tool.py ..",
                "python3 tool.py .. && cd src",
                "cd src && popd && python3 tool.py ..",
                "cd src && eval popd && python3 tool.py ..",
                "cd src && cd .. && python3 tool.py ..",
            ],
        );
    }

    #[test]
    fn line_local_cdpath_keeps_directory_uncertainty() {
        let repo = repo();
        assert_refused(
            &repo,
            &repo.root,
            &repo.root,
            &[
                "CDPATH=.claude; cd src && python3 tool.py ../settings.json",
                "CDPATH=$SEARCH cd src && python3 tool.py ../settings.json",
                // TSK-397 security review F2: every assignment form.
                "CDPATH+=.claude && cd src && python3 tool.py ../settings.json",
                "cdpath=.claude && cd src && python3 tool.py ../settings.json",
                "cdpath=(.claude) && cd src && python3 tool.py ../settings.json",
                "cdpath+=(.claude); cd src && python3 tool.py ../settings.json",
                "export CDPATH=.claude; cd src && python3 tool.py ../settings.json",
                "declare -x CDPATH=.claude; cd src && python3 tool.py ../settings.json",
                "typeset -a cdpath=(.claude); cd src && python3 tool.py ../settings.json",
                "set -A cdpath .claude; cd src && python3 tool.py ../settings.json",
                "printf -v CDPATH .claude; cd src && python3 tool.py ../settings.json",
                "read CDPATH <<< .claude; cd src && python3 tool.py ../settings.json",
            ],
        );
    }

    /// TSK-397 review fixture: `src/lnk` and `safe/lnk` point at `.claude`,
    /// `hidden` is `.claude` under another name, and `bridge` is
    /// `.claude/deep`, so `bridge/..` is `.claude` when read physically.
    #[cfg(unix)]
    fn linked_repo() -> Repo {
        let repo = repo();
        for dir in ["safe", ".claude/src", ".claude/deep"] {
            std::fs::create_dir_all(repo.root.join(dir)).unwrap();
        }
        for (at, to) in [
            ("src/lnk", "../.claude"),
            ("safe/lnk", "../.claude"),
            ("hidden", ".claude"),
            ("bridge", ".claude/deep"),
        ] {
            std::os::unix::fs::symlink(to, repo.root.join(at)).unwrap();
        }
        repo
    }

    /// Grok finding 1: from a symlinked hook cwd, `cd ..` may land in the
    /// link target's parent; a `..` makes the directory unknown.
    #[cfg(unix)]
    #[test]
    fn a_symlinked_cwd_keeps_both_parents() {
        let repo = linked_repo();
        assert_refused(
            &repo,
            &repo.root.join("src/lnk"),
            &repo.root,
            &[
                "cd .. && python3 .claude/settings.json",
                "cd .. && python3 .codex/config.toml",
                "cd -P .. && python3 .claude/settings.json",
            ],
        );
    }

    /// Grok finding 2: `ruby -C` changes directory physically, so
    /// `lnk/..` is the link target's parent, with or without a `cd`.
    #[cfg(unix)]
    #[test]
    fn ruby_chdir_resolves_links_before_parents() {
        let repo = linked_repo();
        assert_refused(
            &repo,
            &repo.root,
            &repo.root,
            &[
                "cd safe && ruby -C lnk/.. .claude/settings.json",
                "cd safe && ruby -Clnk/.. .codex/config.toml",
                "cd safe && ruby -C ./lnk/.. .claude/settings.json",
                "ruby -C safe/lnk/.. .claude/settings.json",
                "cd safe && ruby -C .. .claude/settings.json",
            ],
        );
    }

    /// Grok finding 3: the shell removes quotes and backslashes and
    /// expands parameters before it reads a name, so a spelled-apart
    /// `CDPATH` still makes the directory unknown.
    #[cfg(unix)]
    #[test]
    fn a_spelled_apart_cdpath_is_still_cdpath() {
        let repo = linked_repo();
        assert_refused(
            &repo,
            &repo.root,
            &repo.root,
            &[
                "export CD''PATH=hidden && cd src && python3 ../settings.json",
                "export CD\\PATH=hidden && cd src && python3 ../settings.json",
                "export CD\"\"PATH=hidden && cd src && python3 ../settings.json",
                "export CD${E}PATH=hidden && cd src && python3 ../settings.json",
                "printf -v CD''PATH hidden && cd src && python3 ../settings.json",
            ],
        );
    }

    /// TSK-397 security review F4: an encoded or expanded `CDPATH` name
    /// cannot be read from the text, so any expansion on a line whose `cd`
    /// CDPATH could redirect makes the directory unknown.
    #[cfg(unix)]
    #[test]
    fn an_encoded_cdpath_makes_the_directory_unknown() {
        let repo = linked_repo();
        assert_refused(
            &repo,
            &repo.root,
            &repo.root,
            &[
                r"export $'\x43DPATH'=hidden && cd src && python3 tool.py ../settings.json",
                r"export $'\103DPATH'=hidden && cd src && python3 tool.py ../settings.json",
                r"export ${X:-CDPATH}=hidden && cd src && python3 tool.py ../settings.json",
                r"export CD$'\x50'ATH=hidden && cd src && python3 tool.py ../settings.json",
                r"printf -v $'\x43DPATH' hidden && cd src && python3 tool.py ../settings.json",
                // F6: backtick substitution spells it without a `$`.
                r"export `printf '\103DPATH'`=hidden && cd src && python3 tool.py ../settings.json",
                r"export `rev <<< HTAPDC`=hidden && cd src && python3 tool.py ../settings.json",
                r"export `echo HTAPDC | rev`=hidden && cd src && python3 tool.py ../settings.json",
            ],
        );
    }

    /// Grok finding 4: a path-named interpreter may be a wrapper, and an
    /// option before the script may load code that changes directory first.
    /// These were bypasses of the withdrawn narrowing; they stay refusals.
    #[cfg(unix)]
    #[test]
    fn interpreters_that_load_code_first_keep_the_wider_set() {
        let repo = linked_repo();
        assert_refused(
            &repo,
            &repo.root,
            &repo.root,
            &[
                "cd safe && ./python3 .claude/settings.json",
                "cd safe && python3 -m evil .claude/settings.json",
                "cd safe && python3 -I x.py .claude/settings.json",
                "cd safe && node -r ./ch.js ./read.js .claude/settings.json",
                "cd safe && node --require ./ch.js ./read.js .claude/settings.json",
                "cd safe && node --import ./ch.mjs read.js .claude/settings.json",
                "cd safe && ruby -r ./ch.rb read.rb .claude/settings.json",
                "cd safe && perl -Mevil read.pl .claude/settings.json",
                "cd safe && NODE_OPTIONS=--require=./ch.js node read.js .claude/settings.json",
            ],
        );
    }

    /// TSK-397 Codex review round 2: shell functions, `chpwd` hooks and
    /// `PATH` wrappers (1), the shell's logical `PWD` (2), a `CDPATH` only
    /// the shell holds (3), zsh's two-operand `cd` (6) and `cdable_vars` (7)
    /// all move the shell where the guard cannot follow, so each refuses.
    /// Finding 4 is pinned by `cdpath_entries_resolve_links_before_parents`
    /// and finding 5 by `an_encoded_cdpath_makes_the_directory_unknown`.
    #[cfg(unix)]
    #[test]
    fn shell_state_the_guard_cannot_see_keeps_the_wider_set() {
        let repo = linked_repo();
        assert_refused(
            &repo,
            &repo.root,
            &repo.root,
            &[
                "cd src && python3 tool.py .claude/settings.json",
                "cd src && python3 tool.py ../settings.json",
                "cd destination && python3 settings.json",
                "cd destination && python3 .claude/settings.json",
                // Security review F2: forms the CDPATH rule never refused,
                // so these lines show the unresolved-`cd` fix on its own.
                "cd ./destination && python3 x.py settings.json",
            ],
        );
        assert_refused(
            &repo,
            &repo.root.join("src"),
            &repo.root,
            &[
                "cd .. && python3 settings.json",
                "cd src hidden && python3 settings.json",
                "cd @ROOT@/src @ROOT@/hidden && python3 settings.json",
                "cd -- src hidden && python3 settings.json",
            ],
        );
        let set = |line: &str, cwd: &Path| outward::line_directories(line, cwd, None);
        assert!(set("cd src hidden", &repo.root.join("src")).unknown);
        assert!(set("cd destination", &repo.root).unknown);
        assert!(set("cd ..", &repo.root.join("src")).unknown);
        assert!(set("cd src", &repo.root).unknown);
        assert!(set("cd ./src", &repo.root).unknown);
        assert!(
            set(
                &format!("cd {}", repo.root.join("src").display()),
                &repo.root
            )
            .unknown
        );
    }

    /// TSK-397 security review F1: after a `cd`, a word the guard does not
    /// model may be a user alias or function that returns to the start
    /// directory (oh-my-zsh `-` and `1`, zoxide `z`).
    #[test]
    fn unmodeled_commands_after_cd_keep_the_starting_directory() {
        let repo = repo();
        assert_refused(
            &repo,
            &repo.root,
            &repo.root,
            &[
                "cd src && - && python3 tool.py .claude/settings.json",
                "cd src && 1 && python3 tool.py .claude/settings.json",
                "cd src && .. && python3 tool.py .claude/settings.json",
                "cd src && up && python3 tool.py .claude/settings.json",
                "cd src && z repo && python3 tool.py .claude/settings.json",
                "cd src && - && python3 tool.py .claude/settings.local.json",
                "cd src && - && python3 tool.py .codex/config.toml",
                "cd src && - && python3 tool.py .claude",
                "cd src && true && python3 tool.py .claude/settings.json",
                "cd src && python3 tool.py .. && python3 tool.py .claude/settings.json",
            ],
        );
    }

    /// Codex review round 3 findings 2 and 3: `popd`, a bare `pushd` and
    /// `pushd +N` use a directory stack the guard never saw, and a bare `cd`
    /// uses the shell's `HOME`. Every shell directory change, in any
    /// spelling, makes the directory unknown.
    #[test]
    fn every_shell_directory_change_is_unknown() {
        let repo = repo();
        assert_refused(
            &repo,
            &repo.root,
            &repo.root,
            &[
                "popd && python3 tool.py settings.json",
                "pushd && python3 tool.py settings.json",
                "pushd +1 && python3 tool.py settings.json",
                "pushd ./src && python3 tool.py settings.json",
                "cd && python3 tool.py settings.json",
                "cd ~ && python3 tool.py settings.json",
                "chdir ./src && python3 tool.py settings.json",
                "builtin cd ./src && python3 tool.py settings.json",
                "command cd ./src && python3 tool.py settings.json",
                "eval cd ./src && python3 tool.py settings.json",
                "cd ./src; python3 tool.py settings.json",
            ],
        );
        assert_allowed(
            &repo,
            &[
                "python3 tool.py settings.json",
                "cd ./src && python3 tool.py notes.txt",
                "pushd ./src >/dev/null && python3 tool.py out.json && popd",
            ],
        );
    }

    /// Security review F1: a `trap` action or an `alias` value is code the
    /// shell runs later, and a directory change inside its quoted text is
    /// never split out, so either makes the directory unknown.
    #[test]
    fn trap_actions_and_aliases_are_unknown() {
        let repo = repo();
        assert_refused(
            &repo,
            &repo.root,
            &repo.root,
            &[
                "trap 'builtin cd ./hidden' DEBUG; python3 x.py settings.json",
                "trap 'cd ./hidden' ERR; false; python3 x.py settings.json",
                "trap \"pushd ./hidden\" DEBUG; python3 x.py settings.json",
                "shopt -s expand_aliases\nalias up='cd ./hidden'\nup\npython3 x.py settings.json",
                "unalias up; python3 x.py settings.json",
            ],
        );
        assert_allowed(
            &repo,
            &["trap 'rm -f out.tmp' EXIT; python3 x.py notes.txt"],
        );
    }

    /// Security review SF-1 to SF-3 and Codex review round 4: a program
    /// word the guard does not model may be a shell function (zoxide's `z`,
    /// one made with `autoload` or the zsh `functions` table), a word the
    /// shell expands into `cd`, or a command that installs a hook or an
    /// option that changes directory. Any such command before an
    /// interpreter call makes the directory unknown.
    #[test]
    fn unmodeled_commands_before_an_interpreter_are_unknown() {
        let repo = repo();
        assert_refused(
            &repo,
            &repo.root,
            &repo.root,
            &[
                // SF-1: a program word that expands to `cd`.
                "${X:-cd} .claude && python3 ../src/w.py settings.json",
                "\"${X:-cd}\" .claude && python3 x.py settings.json",
                "{cd,.claude}; python3 x.py settings.json",
                "x=c d=d $x$d .claude && python3 x.py settings.json",
                "${=X} && python3 x.py settings.json",
                // SF-2: functions the line defines.
                "autoload -Uz f && f && python3 x.py settings.json",
                "functions[f]='builtin cd ./hidden'; f; python3 x.py settings.json",
                "aliases[up]='cd .claude'; up; python3 x.py settings.json",
                // SF-3: a shell function from the profile.
                "z .claude; python3 x.py settings.json",
                "make && python3 x.py settings.json",
                // Codex round 4: options and hooks.
                "setopt autocd; hidden; python3 x.py settings.json",
                "shopt -s autocd; hidden; python3 x.py settings.json",
                "PROMPT_COMMAND='builtin cd ./hidden'; python3 x.py settings.json",
                "PROMPT_COMMAND[0]='builtin cd ./hidden'; python3 x.py settings.json",
                "precmd_functions+=(f); python3 x.py settings.json",
                "chpwd_functions=(f); python3 x.py settings.json",
                // SF-6: a loop runs its condition again after its body.
                "while python3 /tmp/w.py settings.json; do cd .claude; done",
                "until python3 /tmp/w.py settings.json; do cd .claude; done",
                "while python3 w.py settings.json; do builtin cd .claude; done",
                "while python3 w.py settings.json; do pushd .claude; done",
                "while python3 w.py settings.json; do z .claude; done",
                "while python3 w.py settings.json { cd .claude }",
                "while python3 x.py settings.json; do cd ./hidden || break; done",
                "while command python3 w.py settings.json; do builtin cd .claude; done",
                // Codex review round 5: a current-shell substitution runs
                // during expansion, before the interpreter starts.
                "python3 x.py settings.json ${ cd ./hidden; }",
                "python3 x.py settings.json <<EOF\n${ cd ./hidden; }\nEOF",
                // Codex review round 6: quoted or assigned, the substitution
                // still runs in this shell.
                "python3 x.py settings.json \"${ cd ./hidden; }\"",
                "v=\"${ cd ./hidden; }\"; python3 x.py settings.json",
                "python3 x.py settings.json \"${| cd ./hidden; }\"",
                "python3 x.py settings.json > \"${ cd ./hidden; }\"",
                // Security review SR2-1: words after the substitution, or
                // joined to it, are still judged.
                "python3 x.py ${ cd ./hidden; } settings.json",
                "python3 x.py settings.json${ cd ./hidden; }",
                "python3 x.py settings.json <<< \"${ cd ./hidden; }\"",
                // Codex review round 7: no word is skipped by its shape, a
                // continuation does not hide `${`, and a `$PWD` form refuses
                // whatever follows it.
                "s python3; python3 x.py settings.json",
                "123 python3; python3 x.py settings.json",
                "s -n 5 python3; python3 x.py settings.json",
                "python3 x.py settings.json \"$\\\n{ cd ./hidden; }\"",
                "v=\"$\\\n{ cd ./hidden; }\"; python3 x.py settings.json",
                "python3 x.py settings.json > \"$\\\n{ cd ./hidden; }\"",
                "z; python3 /abs/read_dir.py \"$PWD/src/..\"",
                "z; python3 /abs/read_dir.py ${PWD}/src/..",
                "z; python3 /abs/read_dir.py $(pwd)/src/..",
                // Security review SR2-2: from a directory the guard cannot
                // follow, `.` or an ancestor may be a protected directory.
                "while python3 /abs/w.py .; do cd ./hidden || break; done",
                "while python3 /abs/w.py ./; do cd ./hidden || break; done",
                "until python3 /abs/w.py .; do cd ./hidden; done",
                "for i in 1 2; do python3 /abs/w.py .; cd ./hidden; done",
                "f() { cd ./hidden; }; while python3 /abs/w.py .; do f; done",
                "cd ./src && python3 /abs/w.py ..",
                "cd ./src && python3 /abs/w.py x/..",
                "cd ./src && python3 /abs/w.py $PWD",
                "cd ./src && python3 /abs/w.py ${PWD}",
                "cd ./src && python3 /abs/w.py $(pwd)",
                "cd ./src && python3 /abs/w.py `pwd`",
            ],
        );
        assert_allowed(
            &repo,
            &[
                "python3 x.py settings.json",
                "make && python3 x.py notes.txt",
                "nice -n 5 python3 x.py settings.json",
                "timeout 5 node x.js settings.json",
                "env -u X python3 x.py settings.json",
                "cd web && node build.js",
            ],
        );
    }

    /// Codex review round 8 and security review round 4: one lexer with the
    /// shell's own states reads every word. A quoted or escaped word is
    /// never `!`, a reserved word or an assignment, a digit-led name is not
    /// an assignment, `$'...'` keeps its escaped quote, and any operand
    /// filled in at run time refuses where the directory is unknown.
    #[test]
    fn the_shell_lexer_decides_command_position_and_taint() {
        let repo = repo();
        assert_refused(
            &repo,
            &repo.root,
            &repo.root,
            &[
                // C8-2, SR4-1: command position.
                "1=x python3; python3 x.py settings.json",
                "\"a=b\" python3; python3 x.py settings.json",
                "a\\=b python3; python3 x.py settings.json",
                "'s=x' python3; python3 x.py settings.json",
                "s\\=x python3; python3 x.py settings.json",
                "\"!\" python3; python3 x.py settings.json",
                "'!' python3; python3 x.py settings.json",
                "\"then\" python3; python3 x.py settings.json",
                "\\then python3; python3 x.py settings.json",
                "\"do\" python3; python3 x.py settings.json",
                "'do' python3; python3 x.py settings.json",
                "\"if\" python3; python3 x.py settings.json",
                "'if' python3; python3 x.py settings.json",
                "\"while\" python3; python3 x.py settings.json",
                // C8-1, SR4-2: ANSI-C quoting before a continuation.
                "v=$'\\''; python3 x.py settings.json \"$\\\n{ cd ./hidden; }\"",
                "v=$'\\''; v=\"$\\\n{ cd ./hidden; }\"; python3 x.py settings.json",
                "v=$'\\''; python3 x.py settings.json > \"$\\\n{ cd ./hidden; printf %s x; }\"",
                "v=$'\\''; python3 x.py settings.json <<EOF\n$\\\n{ cd ./hidden; }\nEOF",
                "v=$'a\\'b'; python3 x.py settings.json \"$\\\n{ cd ./hidden; }\"",
                // C8-3, SR4-3: values derived from the working directory.
                "z; python3 /abs/read_dir.py \"$( pwd)/src/..\"",
                "z; python3 /abs/read_dir.py \"$(command pwd)/src/..\"",
                "z; python3 /abs/read_dir.py \"$(builtin pwd)/src/..\"",
                "z; python3 /abs/read_dir.py \"` pwd`/src/..\"",
                "z; python3 /abs/read_dir.py ~+",
                "z; python3 /abs/read_dir.py ~0",
                "z; python3 /abs/read_dir.py ~+/src/..",
                "z; python3 /abs/read_dir.py ~-",
            ],
        );
        assert_allowed(
            &repo,
            &[
                "python3 x.py notes.txt",
                "nice -n 5 python3 x.py settings.json",
                "X=1 python3 x.py settings.json",
                "z; python3 x.py notes.txt",
            ],
        );
    }

    /// Security review round 5: a tilde anywhere in a word is a tilde, an
    /// assignment counts only as a whole command in the main shell, and the
    /// lexer's unmodeled corners are opaque.
    #[test]
    fn assignments_and_lexer_corners_fail_closed() {
        let repo = repo();
        assert_refused(
            &repo,
            &repo.root,
            &repo.root,
            &[
                // SR5-1: a tilde after `=` or `:`.
                "x=~+; z; python3 /abs/r.py \"$x\"",
                "x=a:~+; z; python3 /abs/r.py \"$x\"",
                "export x=~+; z; python3 /abs/r.py \"$x\"",
                "a=(~+); z; python3 /abs/r.py \"${a[0]}\"",
                "z; python3 /abs/r.py d=~+/src/..",
                // SR5-2: assignments that do not reach the main shell, or
                // whose value the line does not know.
                "(x=notes.txt); z; python3 /abs/r.py \"$x\"",
                "x=notes.txt | true; z; python3 /abs/r.py \"$x\"",
                "y=$(x=notes.txt); z; python3 /abs/r.py \"$x\"",
                "x+=notes.txt; z; python3 /abs/r.py \"$x\"",
                "for x; do z; python3 /abs/r.py \"$x\"; done",
                "declare -n x=PWD; z; python3 /abs/r.py \"$x\"",
                "case x in *) z; python3 /abs/r.py \"$x\";; esac",
                "HOME=.claude; z; python3 /abs/r.py \"$HOME\"",
                "printf -v x %s notes.txt; z; python3 /abs/r.py \"$x\"",
                // SR5-3 to SR5-5: corners the lexer does not model.
                "python3 x.py settings.json <<$'E'\nE",
                "python3 x.py settings.json <<$(x)\n$(x)",
                "python3 x.py settings.json \"$(( '1' ))\"",
                "python3 x.py settings.json \"$((echo 1) )\"",
                "python3 x.py settings.json \"${x:-'}'}\"",
                // SR4-2 as round 4 wrote it, with `$'\''` as an operand.
                "python3 x.py settings.json $'\\'' \"$\\\n{ cd ./src/lnk; }\"",
            ],
        );
        // Codex review round 10: a function may change `HOME`, so even a
        // plain `~` is untrusted once the directory is unknown.
        assert_refused(
            &repo,
            &repo.root,
            &repo.root,
            &["z; python3 /abs/r.py ~/notes.txt"],
        );
        assert_allowed(&repo, &["python3 /abs/r.py ~/notes.txt"]);
    }

    /// Codex review round 9: no variable is trusted once the directory is
    /// unknown, a quoted or expanded interpreter name keeps its operands
    /// judged, and non-ASCII input is read without a panic.
    #[test]
    fn no_variable_is_trusted_and_no_operand_is_dropped() {
        let repo = repo();
        assert_refused(
            &repo,
            &repo.root,
            &repo.root,
            &[
                // C9-1: the program word is quoted, escaped or expanded.
                "z; \"python3\" /abs/read_dir.py \"$PWD/src/..\"",
                "z; \\python3 /abs/read_dir.py \"$PWD/src/..\"",
                "prog=python3; z; \"$prog\" /abs/read_dir.py \"$PWD/src/..\"",
                // C9-2: every assignment path, now untrusted.
                "z; p=$(pwd); p+=/src/..; python3 /abs/read_dir.py \"$p\"",
                "z; p=$(pwd); export p+=/src/..; python3 /abs/read_dir.py \"$p\"",
                "z; p=$(pwd); declare p+=/src/..; python3 /abs/read_dir.py \"$p\"",
                "z; p=$(pwd); (p=safe); python3 /abs/read_dir.py \"$p\"",
                "z; p=$(pwd); p=safe python3 /abs/read_dir.py \"$p\"",
                "z; p=$(pwd); case p in x) ;; esac; python3 /abs/read_dir.py \"$p\"",
                "z; p=$(pwd); p+=; python3 /abs/read_dir.py \"$p\"",
                "p=safe; z; python3 /abs/read_dir.py \"$p\"",
                "z; p=$(pwd); export p=safe; python3 /abs/read_dir.py \"$p\"",
                "z; p=$(pwd); printf -v p %s safe; python3 /abs/read_dir.py \"$p\"",
                // The accepted cost: a variable after an unmodeled command.
                "printf -v p '%s%s' 'no' 'tes.txt'; python3 tool.py \"$p\"",
                "export a=notes.txt; python3 tool.py \"$a\"",
                "x=notes.txt; z; python3 /abs/r.py \"$x\"",
                // C10-1: a launcher wrapping a quoted interpreter name, or
                // an unsupported wrapper, keeps the operands judged.
                "z; env \"python3\" /abs/read_dir.py \"$PWD/src/..\"",
                "z; command \"python3\" /abs/read_dir.py \"$PWD/src/..\"",
                "z; nice -n 5 \"python3\" /abs/read_dir.py \"$PWD/src/..\"",
                "z; timeout 5 \"python3\" /abs/read_dir.py \"$PWD/src/..\"",
                "z; builtin command python3 /abs/read_dir.py \"$PWD/src/..\"",
                "z; env -u X \"python3\" /abs/read_dir.py \"$PWD/src/..\"",
                // C10-2: `~` after a command that may change `HOME`.
                "z; python3 x.py ~/settings.json",
                "z\npython3 x.py ~/settings.json",
                // SR6-1, SR6-2.
                "x=$PWD; z; python3 /abs/r.py $=x/src/..",
                "x=$PWD; z; python3 /abs/r.py $^x/src/..",
                "for x do cd ..; done; python3 x.py settings.json",
                // C9-3: non-ASCII words.
                "é; python3 x.py settings.json",
                "export x=é; python3 x.py settings.json",
                "é=1; python3 x.py settings.json",
            ],
        );
        assert_allowed(
            &repo,
            &[
                "x=café; python3 x.py notes.txt",
                "printf -v p %s notes.txt; python3 x.py notes.txt",
                "p=notes.txt; python3 x.py \"$p\"",
            ],
        );
        // C9-4: a NUL byte refuses before anything is read.
        let found = crate::hooks::exec_guard::evaluate_at(
            "python3 x.py settings.json \"$\0{ cd ./hidden; }\"",
            &SecuritySection::default(),
            PolicyLevel::Block,
            &repo.root,
            &repo.root,
        );
        assert!(found.iter().any(|v| v.message.contains("NUL")), "{found:?}");
    }

    /// Codex review round 3 finding 1: a `cd` function or a zsh `chpwd`
    /// hook in the shell moves it after `cd ./src`. The real shells land in
    /// `.claude`, and the guard refuses the line.
    #[cfg(unix)]
    #[test]
    fn a_cd_hook_moves_the_shell_after_a_followed_cd() {
        let repo = repo();
        let claude = std::fs::canonicalize(repo.root.join(".claude")).unwrap();
        for (shell, args) in [
            (
                "bash",
                [
                    "-c",
                    r#"cd() { builtin cd "$@" && builtin cd "$R/.claude"; }; cd ./src && /bin/pwd -P"#,
                ],
            ),
            (
                "zsh",
                [
                    "-fc",
                    r#"chpwd() { builtin cd -q "$R/.claude"; }; cd ./src && /bin/pwd -P"#,
                ],
            ),
        ] {
            let Ok(output) = std::process::Command::new(shell)
                .args(args)
                .env("R", &repo.root)
                .env_remove("CDPATH")
                .current_dir(&repo.root)
                .output()
            else {
                eprintln!("skipped: {shell} is not installed");
                continue;
            };
            assert!(output.status.success(), "{shell}: {output:?}");
            let printed = String::from_utf8(output.stdout).unwrap();
            assert_eq!(Path::new(printed.trim()), claude, "{shell}");
        }
        assert_refused(
            &repo,
            &repo.root,
            &repo.root,
            &["cd ./src && python3 tool.py settings.json"],
        );
    }

    #[test]
    fn inherited_cdpath_includes_search_destinations() {
        const CHILD_ROOT: &str = "CODEFLOW_TEST_397_CDPATH_ROOT";
        if let Some(root) = std::env::var_os(CHILD_ROOT) {
            let root = PathBuf::from(root);
            let line = "cd src && python3 tool.py ../settings.json";
            let findings = evaluate(
                line,
                &SecuritySection::default(),
                PolicyLevel::Block,
                &root,
                &root,
                None,
            );
            assert!(argument_refusal(&findings), "{findings:?}");
            assert!(outward::line_directories(line, &root, None).unknown);
            return;
        }
        let repo = repo();
        std::fs::create_dir(repo.root.join(".claude/src")).unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "security::interpreter::tests::inherited_cdpath_includes_search_destinations",
                "--nocapture",
            ])
            .env(CHILD_ROOT, &repo.root)
            .env("CDPATH", repo.root.join(".claude"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// Codex review round 2 finding 4: a `CDPATH` entry such as `bridge/..`
    /// reaches `.claude/src`; the `cd` makes the directory unknown.
    #[cfg(unix)]
    #[test]
    fn cdpath_entries_resolve_links_before_parents() {
        const CHILD_ROOT: &str = "CODEFLOW_TEST_397_CDPATH_LINK_ROOT";
        if let Some(root) = std::env::var_os(CHILD_ROOT) {
            let root = PathBuf::from(root);
            let line = "cd -P src && python3 tool.py ../settings.json";
            let findings = evaluate(
                line,
                &SecuritySection::default(),
                PolicyLevel::Block,
                &root,
                &root,
                None,
            );
            assert!(argument_refusal(&findings), "{findings:?}");
            assert!(outward::line_directories(line, &root, None).unknown);
            return;
        }
        let repo = linked_repo();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "security::interpreter::tests::cdpath_entries_resolve_links_before_parents",
                "--nocapture",
            ])
            .env(CHILD_ROOT, &repo.root)
            .env("CDPATH", "bridge/..")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// A `cd` through a symbolic link makes the directory unknown, like
    /// every shell directory change.
    #[cfg(unix)]
    #[test]
    fn a_cd_through_a_link_is_unknown() {
        let repo = repo();
        std::os::unix::fs::symlink(repo.root.join("src"), repo.root.join("link")).unwrap();
        let dirs = outward::line_directories(
            "cd link && python3 tool.py ..",
            &repo.root,
            Some(&repo.home),
        );
        assert!(dirs.unknown, "{dirs:?}");
        assert!(dirs.dirs.contains(&repo.root), "{dirs:?}");
        std::os::unix::fs::symlink(&repo.home, repo.root.join("outside")).unwrap();
        assert_refused(
            &repo,
            &repo.root,
            &repo.root,
            &[
                "cd link && python3 tool.py ..",
                "cd link && python3 tool.py ../.claude/settings.json",
                "cd outside && python3 tool.py ../repo/.claude/settings.json",
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
        // The line's directory set. A process's own directory option is
        // followed, beside the starting directory; a shell directory change,
        // in every spelling, makes the set unknown and keeps the starting
        // directory (TSK-397).
        let set = |line: &str| outward::line_directories(line, cwd, Some(Path::new("/h")));
        let has = |line: &str, dir: &str| set(line).dirs.contains(&PathBuf::from(dir));
        for line in [
            "env -C sub python3 x",
            "env -Csub python3 x",
            "env --chdir=sub python3 x",
            "git -C sub status; python3 x",
        ] {
            assert!(has(line, "/w/sub"), "{line}: {:?}", set(line));
            assert!(has(line, "/w"), "{line}: {:?}", set(line));
        }
        for line in [
            "cd sub && python3 x",
            "cd -- sub && python3 x",
            "cd -P -L sub && python3 x",
            "cd -q sub; python3 x",
            "command cd sub && python3 x",
            "command -p cd sub && python3 x",
            "builtin cd sub && python3 x",
            "! cd sub; python3 x",
            "pushd sub >/dev/null && python3 x",
            "chdir sub && python3 x",
            "cd a && cd b && python3 x",
            "cd a && cd .. && python3 x",
            "cd && python3 x",
            "cd \"$D\" && python3 x",
            "cd - && python3 x",
            "cd ~u && python3 x",
        ] {
            assert!(set(line).unknown, "{line}: {:?}", set(line));
            assert!(has(line, "/w"), "{line}: {:?}", set(line));
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
    /// level. Unresolved shell input near a checkout is covered by TSK-406.
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
                    "python3 tool.py .{claude,git}/*",
                    "python3 tool.py \"$(echo .git/config)\"",
                ],
                level,
            );
            for command in [
                "ls | xargs python3 tool.py",
                "for f in src/*.py; do python3 lint.py \"$f\"; done",
                "find src -exec python3 tool.py {} \\;",
            ] {
                let findings = judge_in(&repo, &repo.root, &repo.root, command, level);
                assert!(findings.is_empty(), "{level:?}: {command}: {findings:?}");
            }
        }
    }

    /// TSK-406 AC-3 supersedes the old allowances for unresolved shell input.
    #[test]
    fn unresolved_shell_expansions_refuse_near_a_checkout() {
        let repo = repo();
        assert_public_refused(
            &repo,
            &[
                "python3 tool.py \"$INPUT\"",
                "python3 tool.py \"$ROOT/$NAME\"",
                "python3 tool.py \"$ROOT/notes.txt\"",
                "ROOT=$(git rev-parse --show-toplevel); python3 \"$ROOT/src/app.py\"",
                "while read f; do python3 lint.py \"$f\"; done < files.txt",
                "python3 lint.py \"$(echo src/*.py)\"",
                "python3 lint.py \"$(echo ok src/*.py)\"",
                "python3 -c 'import sys; print(sys.argv[1:])' \"$config\"",
                "cd \"$D\" && python3 -c 'import sys; open(sys.argv[1])' \"$IN\"",
            ],
            false,
        );
    }

    /// TSK-302 review F4: a line with several suppliers reads what the line
    /// names once, so five `xargs` interpreters stay within the hook's
    /// 10-second budget. TSK-453 counts the reads instead of timing them,
    /// so host load cannot fail the test.
    #[test]
    fn several_suppliers_read_the_line_once() {
        use super::super::arguments::{LineReads, LINE_READS};
        let repo = repo();
        let line = (0..5)
            .map(|n| format!("ls | xargs python3 check{n}.py"))
            .collect::<Vec<_>>()
            .join("; ");
        let none = LineReads {
            asked: 0,
            enforcement: 0,
            authority: 0,
        };
        LINE_READS.with(|reads| reads.set(none));
        assert!(judge_in(&repo, &repo.root, &repo.root, &line, PolicyLevel::Block).is_empty());
        // Each of the five calls asks for both readings, and one enforcement
        // reading and one authority reading answer all of them.
        assert_eq!(
            LINE_READS.with(std::cell::Cell::get),
            LineReads {
                asked: 10,
                enforcement: 1,
                authority: 1,
            }
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
                "cd ./src && python3 app.py notes.txt",
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
                "echo -e \"python3 tool.py .claude/settings.json\"",
                "print -r '.cla\\x75de/settings.json'",
                "IFS=; arr=('no' 'tes.txt'); python3 tool.py \"${arr[*]}\"",
                "arr=('no' 'tes.txt'); python3 tool.py \"${arr[*]}\"",
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
                "IFS=; arr=('no' 'tes.txt'); python3 tool.py \"${arr[*]}\"",
                "a=no; b=\"${a}tes.txt\"; python3 tool.py \"$b\"",
                "f='%s%s\\n'; printf \"$f\" no tes.txt | python3 -c 'import sys; print(sys.stdin.read())'",
                "echo -ne 'notes.txt' | python3 -c 'import sys; print(sys.stdin.read())'",
                "find src -name '*.py' -exec sh -c 'python3 lint.py \"$1\"' sh {} \\;",
                "for f in src/*.py; do python3 lint.py \"$f\"; done",
                // Program output is outside text (operator boundary, 2026-10-15).
                "echo \"Done $(date)\"; python3 src/app.py",
                // Ordinary globs an output builtin writes.
                "echo src/*.py | python3 -c 'import sys; print(sys.stdin.read())'",
                "echo *.txt; python3 src/app.py",
                "echo ok src/*.py | python3 -c 'import sys; print(sys.stdin.read())'",
                "echo \"$TAG\" *.txt; python3 src/app.py",
                "echo notes$(/bin/cat /dev/null).txt | python3 -c 'import sys; print(sys.stdin.read())'",
                "echo notes.txt \"$(/bin/cat /dev/null)\"; python3 src/app.py",
                // Round 16: a quoted parameter named like a protected
                // component is a name, not spelled text.
                "echo \"$config\" | python3 -c 'import sys; print(sys.stdin.read())'",
                "echo \"$hooks\" | python3 -c 'import sys; print(sys.stdin.read())'",
                "echo \"$policy.json\" | python3 -c 'import sys; print(sys.stdin.read())'",
                "echo \"$settings.json\" | python3 -c 'import sys; print(sys.stdin.read())'",
            ],
        );
        assert_public_allowed(
            &repo,
            &["source venv/bin/activate && echo ok && python3 src/app.py"],
        );
    }
}
