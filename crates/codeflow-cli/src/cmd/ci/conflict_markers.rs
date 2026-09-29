//! The conflict-marker row of `codeflow ci` (TSK-170, SPC-013 planning
//! resolution 26): the pre-commit hook's matcher, level, messages and
//! attribute rule, over the lines the range adds to every text path, with
//! `.gitattributes` read at the head. It catches what no pre-commit hook
//! sees: a marker left while resolving `git rebase --continue`, and a clean
//! merge of a branch that already holds one.

use std::path::Path;

use codeflow_core::hooks::conflict_markers::{self, AddedLines, AttrSource};
use codeflow_core::hooks::GitPolicy;

/// Run the check for `codeflow ci` over `base..head` and record its findings
/// and whether it ran. `base` is `None` when the range did not resolve; the
/// commit row has already reported that skip.
pub(super) fn dispatch(
    root: &Path,
    git: &GitPolicy,
    base: Option<&str>,
    head: &str,
    tagged: &mut Vec<super::TaggedViolation>,
    ran: &mut Vec<&str>,
) {
    let level = git.conflict_markers;
    if !level.is_active() {
        return;
    }
    let Some(base) = base else {
        return;
    };
    ran.push("conflict-markers");
    let findings = match range_findings(root, git, base, head) {
        Ok(findings) => findings,
        Err(error) => vec![conflict_markers::incomplete(
            level,
            &error,
            codeflow_core::remedy::CI_RANGE_UNREADABLE.remedy(),
        )],
    };
    tagged.extend(
        findings
            .into_iter()
            .map(|violation| super::TaggedViolation {
                sha: None,
                violation,
            }),
    );
}

/// The findings over the lines the range adds, each path judged at its
/// `conflict-marker-size` in the head's tree.
fn range_findings(
    root: &Path,
    git: &GitPolicy,
    base: &str,
    head: &str,
) -> Result<Vec<codeflow_core::hooks::Violation>, String> {
    let files = added_text_lines(root, base, head)?;
    let paths: Vec<&str> = files.keys().map(String::as_str).collect();
    let sizes = conflict_markers::marker_sizes(root, AttrSource::Revision(head), &paths)?;
    Ok(conflict_markers::check(
        git.conflict_markers,
        &files,
        &sizes,
    ))
}

/// Every line the range adds to a text path, from the merge-base of `base`
/// and `head`. Unlike the dash check's diff it is scoped to no tree and
/// skips no scaffold file; like it, `--text` stops an attribute from hiding
/// a text addition, and a file is skipped as binary only by the content of
/// its new-side blob. Without rename detection a moved file's lines count as
/// added, as they do in the hook's staged diff.
fn added_text_lines(root: &Path, base: &str, head: &str) -> Result<AddedLines, String> {
    let merge_base = super::git_stdout(root, &["merge-base", base, head])?;
    let diff = super::git_stdout(
        root,
        &[
            "-c",
            "core.quotepath=off",
            "diff-tree",
            "-r",
            "-p",
            "--no-renames",
            "--unified=0",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "--text",
            "--full-index",
            "--src-prefix=a/",
            "--dst-prefix=b/",
            merge_base.trim(),
            head,
        ],
    )?;
    let lines = super::parse_added_lines(&diff);
    let blobs: std::collections::BTreeSet<&str> =
        lines.iter().filter_map(|l| l.blob.as_deref()).collect();
    let contents = super::read_blobs(root, &blobs.into_iter().collect::<Vec<_>>())?;
    let mut files = AddedLines::new();
    for added in lines {
        let binary = added
            .blob
            .as_ref()
            .and_then(|blob| contents.get(blob))
            .is_some_and(|content| conflict_markers::is_binary(content));
        if !binary {
            files
                .entry(added.path)
                .or_default()
                .push((added.line, added.text));
        }
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use codeflow_core::hooks::git_hook;

    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .output()
            .expect("git runs");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn write(dir: &Path, rel: &str, content: &str) {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    /// A marker line built at run time, so this file holds none itself.
    fn marker(fill: char, size: usize, label: &str) -> String {
        format!("{}{label}", fill.to_string().repeat(size))
    }

    fn messages(violations: &[codeflow_core::hooks::Violation]) -> Vec<String> {
        let mut out: Vec<String> = violations
            .iter()
            .filter(|v| v.rule == conflict_markers::RULE)
            .map(|v| format!("{} | {} | {}", v.level, v.message, v.remedy))
            .collect();
        out.sort();
        out
    }

    #[test]
    fn the_hook_and_ci_report_the_same_findings_for_one_staged_diff() {
        // TSK-170 AC-4: one staged diff, fed to the hook's git2 walk and,
        // once committed, to this row's parser.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q", "-b", "main"]);
        git(root, &["config", "user.email", "t@example.com"]);
        git(root, &["config", "user.name", "t"]);
        write(root, "keep.md", "one\ntwo\nthree\n");
        write(root, "gone.md", &format!("{}\n", marker('<', 7, " old")));
        git(root, &["add", "."]);
        git(root, &["commit", "-q", "-m", "chore: seed"]);
        git(root, &["switch", "-q", "-c", "feat/x"]);
        let conflict = format!(
            "{}\nours\n{}\nbase\n{}\ntheirs\n{}\n",
            marker('<', 7, " HEAD"),
            marker('|', 7, " base"),
            marker('=', 7, ""),
            marker('>', 7, " feat/y")
        );
        // Two hunks in one file: the opening in the first, the rest later.
        write(
            root,
            "keep.md",
            &format!(
                "{}\none\ntwo\nthree\n{}\nlast\n{}\n",
                marker('<', 7, " HEAD"),
                marker('=', 7, ""),
                marker('>', 7, " b")
            ),
        );
        write(root, "src/new.rs", &conflict);
        write(root, "crlf.txt", &conflict.replace('\n', "\r\n"));
        write(
            root,
            "README.md",
            &format!("Title\n{}\n", marker('=', 7, "")),
        );
        write(root, "blob.bin", &format!("\0{conflict}"));
        write(root, "fixtures/a.txt", &conflict);
        write(
            root,
            "fixtures/b.txt",
            &format!("{}\n{}\n", marker('<', 32, " x"), marker('>', 32, " y")),
        );
        write(
            root,
            ".gitattributes",
            "fixtures/** conflict-marker-size=32\n",
        );
        git(root, &["add", "-A"]);
        git(root, &["rm", "-q", "gone.md"]);

        let policy = GitPolicy::default();
        let hook = git_hook::pre_commit(root, &policy, false).unwrap();
        git(root, &["commit", "-q", "-m", "feat: add the files"]);
        let ci = range_findings(root, &policy, "main", "HEAD").unwrap();

        let hook = messages(&hook.violations);
        assert_eq!(hook, messages(&ci));
        // keep.md 3, src/new.rs 4, crlf.txt 4, fixtures/b.txt 2.
        assert_eq!(hook.len(), 13, "{hook:#?}");
        assert!(hook.iter().any(|m| m.contains("keep.md:5 ")), "{hook:#?}");
        assert!(!hook.iter().any(|m| m.contains("README.md")));
        assert!(!hook.iter().any(|m| m.contains("blob.bin")));
        assert!(!hook.iter().any(|m| m.contains("fixtures/a.txt")));
        assert!(!hook.iter().any(|m| m.contains("gone.md")));
    }

    #[test]
    fn an_unreadable_range_is_a_finding_not_a_pass() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q", "-b", "main"]);
        let mut tagged = Vec::new();
        let mut ran = Vec::new();
        dispatch(
            root,
            &GitPolicy::default(),
            Some("main"),
            "HEAD",
            &mut tagged,
            &mut ran,
        );
        assert_eq!(ran, ["conflict-markers"]);
        assert_eq!(tagged.len(), 1);
        assert!(tagged[0].violation.message.contains("incomplete"));
    }
}
