//! The conflict-marker row of `codeflow ci` (TSK-170, SPC-013 planning
//! resolution 26): the pre-commit hook's matcher, level, messages and
//! attribute rule, over the lines the range adds to every text path, with
//! `.gitattributes` read at the head. It catches what no pre-commit hook
//! sees: a marker left while resolving `git rebase --continue`, and a clean
//! merge of a branch that already holds one.

use std::path::Path;

use std::collections::BTreeSet;

use codeflow_core::hooks::conflict_markers::{self, AddedLines, AttrError, AttrSource};
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
        Err(error) => {
            // Fetching cannot add an option to an old git; upgrading can.
            let remedy = match &error {
                AttrError::SourceUnsupported(_) => {
                    codeflow_core::remedy::GIT_ATTR_SOURCE_UNSUPPORTED.remedy()
                }
                AttrError::Failed(_) => codeflow_core::remedy::CI_RANGE_UNREADABLE.remedy(),
            };
            vec![conflict_markers::incomplete(
                level,
                &error.to_string(),
                remedy,
            )]
        }
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
) -> Result<Vec<codeflow_core::hooks::Violation>, AttrError> {
    let files = added_text_lines(root, base, head).map_err(AttrError::Failed)?;
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
/// added, as they do in the hook's staged diff. Gitlinks are skipped before
/// any blob is read: their object is a commit in the submodule's repository.
/// `--ignore-submodules=none` lists every gitlink without reading
/// `.gitmodules`, which git would otherwise parse and refuse when it holds a
/// leftover marker.
fn added_text_lines(root: &Path, base: &str, head: &str) -> Result<AddedLines, String> {
    let merge_base = super::git_stdout(root, &["merge-base", base, head])?;
    let gitlinks = gitlinks(root, merge_base.trim(), head)?;
    let diff = super::git_stdout(
        root,
        &[
            "-c",
            "core.quotepath=off",
            "diff-tree",
            "-r",
            "-p",
            "--no-renames",
            "--ignore-submodules=none",
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
    let lines: Vec<super::AddedLine> = super::parse_added_lines(&diff)
        .into_iter()
        .filter(|added| !gitlinks.contains(&added.path))
        .collect();
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

/// The paths the diff from `from` to `to` leaves as gitlinks (mode 160000),
/// read from `diff-tree --raw -z`: a `:old new old-id new-id status` record,
/// then the path.
fn gitlinks(root: &Path, from: &str, to: &str) -> Result<BTreeSet<String>, String> {
    let raw = super::git_stdout(
        root,
        &[
            "diff-tree",
            "-r",
            "--no-renames",
            "--ignore-submodules=none",
            "--raw",
            "-z",
            from,
            to,
        ],
    )?;
    let mut out = BTreeSet::new();
    let mut fields = raw.split('\0');
    while let Some(header) = fields.next() {
        let Some(header) = header.strip_prefix(':') else {
            continue;
        };
        let Some(path) = fields.next() else {
            break;
        };
        if header.split(' ').nth(1) == Some("160000") {
            out.insert(path.to_string());
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use codeflow_core::hooks::git_hook;

    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        let out = codeflow_core::git::command()
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

    fn stdout(dir: &Path, args: &[&str]) -> String {
        let out = codeflow_core::git::command()
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .output()
            .expect("git runs");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn repo(root: &Path, seed: &str) {
        std::fs::create_dir_all(root).unwrap();
        git(root, &["init", "-q", "-b", "main"]);
        git(root, &["config", "user.email", "t@example.com"]);
        git(root, &["config", "user.name", "t"]);
        write(root, "seed.txt", seed);
        git(root, &["add", "."]);
        git(root, &["commit", "-q", "-m", "chore: seed"]);
    }

    /// TSK-170 review P2: a submodule added and then updated. The gitlink's
    /// commit lives only in the child repository, so neither plane may read
    /// it as a blob; `.gitmodules` is still judged, on both planes alike.
    #[test]
    fn gitlinks_are_skipped_and_gitmodules_judged_on_both_planes() {
        let dir = tempfile::tempdir().unwrap();
        let child = dir.path().join("child");
        repo(&child, "child one\n");
        let first = stdout(&child, &["rev-parse", "HEAD"]);
        write(&child, "seed.txt", "child two\n");
        git(&child, &["commit", "-q", "-am", "chore: move on"]);
        let second = stdout(&child, &["rev-parse", "HEAD"]);
        let root = dir.path().join("super");
        repo(&root, "superproject\n");
        git(&root, &["switch", "-q", "-c", "feat/x"]);
        let policy = GitPolicy::default();
        for oid in [&first, &second] {
            let absent = codeflow_core::git::command()
                .args(["cat-file", "-e", oid])
                .current_dir(&root)
                .env_remove("GIT_DIR")
                .env_remove("GIT_INDEX_FILE")
                .status()
                .unwrap();
            assert!(
                !absent.success(),
                "{oid} must be absent from the superproject"
            );
        }

        // Addition: a gitlink and a .gitmodules that still holds a marker.
        write(
            &root,
            ".gitmodules",
            &format!(
                "{}\n[submodule \"vendor\"]\n\tpath = vendor\n\turl = {}\n",
                marker('<', 7, " HEAD"),
                child.display()
            ),
        );
        git(&root, &["add", ".gitmodules"]);
        let cacheinfo = format!("160000,{first},vendor");
        git(&root, &["update-index", "--add", "--cacheinfo", &cacheinfo]);
        let hook = messages(
            &git_hook::pre_commit(&root, &policy, false)
                .unwrap()
                .violations,
        );
        git(&root, &["commit", "-q", "-m", "feat: add the dependency"]);
        let ci = messages(&range_findings(&root, &policy, "main", "HEAD").unwrap());
        assert_eq!(hook, ci);
        assert_eq!(hook.len(), 1, "{hook:#?}");
        assert!(
            hook[0].contains(".gitmodules:1 adds an unresolved opening"),
            "{hook:#?}"
        );

        // Update: the gitlink moves to another child commit.
        let cacheinfo = format!("160000,{second},vendor");
        git(&root, &["update-index", "--cacheinfo", &cacheinfo]);
        let hook = messages(
            &git_hook::pre_commit(&root, &policy, false)
                .unwrap()
                .violations,
        );
        assert!(hook.is_empty(), "{hook:#?}");
        git(
            &root,
            &["commit", "-q", "-m", "feat: update the dependency"],
        );
        let ci = messages(&range_findings(&root, &policy, "HEAD~1", "HEAD").unwrap());
        assert!(ci.is_empty(), "{ci:#?}");
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
