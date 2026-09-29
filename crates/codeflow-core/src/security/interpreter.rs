//! Conservative literal inspection inside interpreter code, never ordinary data.
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
    violations
}

fn secret_path(word: &str, home: Option<&Path>) -> bool {
    actions::table().sandbox_read_denies.iter().any(|pattern| {
        let expanded = pattern
            .strip_prefix("~/")
            .and_then(|p| home.map(|home| home.join(p).to_string_lossy().into_owned()));
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
                    .and_then(|p| home.map(|home| home.join(p).to_string_lossy().into_owned()));
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
}
