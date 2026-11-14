//! The loading limit of the instruction chain, checked for an overridden
//! `AGENTS.md` before update writes it (TSK-1302).
//!
//! Codex reads the instruction file of each directory from the project root
//! down, at most `project_doc_max_bytes` in all (32 KiB by default), and
//! silently drops the rest. An override replaces text early in the managed
//! block, so a long replacement moves every later instruction, the review and
//! git rules included, toward that cut. Every protected instruction sits in
//! the managed block, so the block's end must load whole: the bytes before
//! the block in this file and in the directories above it, plus the block,
//! must fit the budget.
//!
//! The budget is the 32 KiB default, lowered by a smaller
//! `project_doc_max_bytes` in the project's `.codex/config.toml` or the
//! user's Codex config, never raised by either. Codex's order, highest
//! first, is command line flags, project config, profile, user config,
//! cloud-managed config, system config; it skips an untrusted project's
//! config, and a nested project config can set its own value. So a flag or
//! a nested project config can replace the project value, a profile can
//! replace the user value, and an untrusted project's value does not apply:
//! a value above the default proves nothing about the limit a session runs
//! under. Flags, profiles, nested project configs, cloud-managed and system
//! config are not read; that is the named limit of this check.
//!
//! The check fails closed. When a config that could lower the budget cannot
//! be read, or the block's place in the file cannot be found, the override is
//! refused.

use std::path::Path;

use super::{dest_shown, file_refused};
use crate::scaffold::manifest::RegionFormat;
use crate::scaffold::rule_map::{
    ancestor_instruction_bytes, codex_home, instruction_budget, BudgetSource,
};
use crate::scaffold::state::read_beneath_root;
use crate::scaffold::{region, ScaffoldError, Shown};
use crate::shown;

/// Refuse the overridden `effective` rendering of the managed-region file
/// `dest` when the file update would write, with the project's current text
/// around the block, loads past the instruction limit.
///
/// # Errors
///
/// The refusal naming the file, the end of the block and the budget.
pub(super) fn check(
    root: &Path,
    dest: &str,
    effective: &str,
    format: RegionFormat,
) -> Result<(), ScaffoldError> {
    check_with(root, codex_home().as_deref(), dest, effective, format)
}

/// [`check`] with the Codex home whose user config applies given.
fn check_with(
    root: &Path,
    home: Option<&Path>,
    dest: &str,
    effective: &str,
    format: RegionFormat,
) -> Result<(), ScaffoldError> {
    let undetermined = |why: Shown| {
        file_refused(shown!(
            "{} cannot be shown to load within the instruction limit: {}",
            dest_shown(dest),
            why
        ))
    };
    let (limit, source) =
        instruction_budget(root, home).map_err(|reason| undetermined(Shown::fixed(reason)))?;
    let current = read_beneath_root(root, dest)
        .map_err(|_| undetermined(Shown::fixed("the file cannot be read")))?;
    let block = region::extract_block(effective, format)
        .ok_or_else(|| undetermined(Shown::fixed("its managed block is not found")))?;
    let next = match current.as_deref() {
        Some(current) => region::upsert_block(current, &block, format).0,
        None => effective.to_string(),
    };
    let end = region::managed_range(&next)
        .ok_or_else(|| undetermined(Shown::fixed("its managed block is not found")))?
        .end;
    let above = ancestor_instruction_bytes(root, dest).map_err(|_| {
        undetermined(Shown::fixed(
            "the instruction files above it cannot be inspected",
        ))
    })?;
    let loaded = above.saturating_add(end);
    if loaded <= limit {
        return Ok(());
    }
    let (source, raised) = match source {
        BudgetSource::Default => (
            "Codex's default",
            "; a configured project_doc_max_bytes above the default is not honoured, because it may not be the limit a session runs under",
        ),
        BudgetSource::ProjectConfig => ("project_doc_max_bytes in .codex/config.toml", ""),
        BudgetSource::UserConfig => ("project_doc_max_bytes in the Codex user config", ""),
    };
    Err(file_refused(shown!(
        "the overrides would put the end of the managed instructions of {} at byte {}, past the {}-byte instruction limit ({}){}; Codex would cut the review and git rules that follow the overridden text, so shorten the replacements",
        dest_shown(dest),
        Shown::count(loaded),
        Shown::count(limit),
        Shown::fixed(source),
        Shown::fixed(raised)
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scaffold::rule_map::codex_home_from;

    const BLOCK: &str =
        "<!-- codeflow:managed:begin scaffold=1 -->\nrules\n<!-- codeflow:managed:end -->";

    fn write(root: &Path, rel: &str, bytes: usize) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "a".repeat(bytes)).unwrap();
    }

    fn checked(root: &Path, dest: &str) -> Result<(), String> {
        checked_in(root, None, dest)
    }

    /// `dest` checked with the user config of `home`, the Codex home.
    fn checked_in(root: &Path, home: Option<&Path>, dest: &str) -> Result<(), String> {
        check_with(root, home, dest, BLOCK, RegionFormat::Markdown)
            .map_err(|error| error.to_string())
    }

    fn config(dir: &Path, rel: &str, text: &str) {
        std::fs::create_dir_all(dir.join(rel).parent().unwrap()).unwrap();
        std::fs::write(dir.join(rel), text).unwrap();
    }

    /// The instruction files of the directories above `dest` load before it,
    /// the override file standing in for `AGENTS.md` where it has bytes.
    #[test]
    fn the_directories_above_the_file_count_toward_its_block() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "AGENTS.md", 32 * 1024 - BLOCK.len() - 10);
        assert_eq!(checked(root, "AGENTS.md"), Ok(()));
        assert_eq!(checked(root, "sub/AGENTS.md"), Ok(()));
        write(root, "sub/AGENTS.md", 20);
        let error = checked(root, "sub/deep/AGENTS.md").expect_err("the chain is over");
        assert!(
            error.contains("past the 32768-byte instruction limit"),
            "{error}"
        );
        // The override file replaces `AGENTS.md` in its directory.
        write(root, "AGENTS.override.md", 100);
        assert_eq!(checked(root, "sub/deep/AGENTS.md"), Ok(()));
        // A configured limit applies to the whole chain.
        std::fs::create_dir_all(root.join(".codex")).unwrap();
        std::fs::write(
            root.join(".codex/config.toml"),
            "project_doc_max_bytes = 120\n",
        )
        .unwrap();
        let error = checked(root, "sub/deep/AGENTS.md").expect_err("over the configured limit");
        assert!(
            error.contains("120-byte instruction limit (project_doc_max_bytes"),
            "{error}"
        );
    }

    /// An ancestor whose instruction file cannot be inspected, or a limit
    /// that cannot be read, refuses instead of passing.
    #[test]
    fn what_cannot_be_measured_refuses() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".codex")).unwrap();
        std::fs::write(
            root.join(".codex/config.toml"),
            "project_doc_max_bytes = 'x'\n",
        )
        .unwrap();
        let error = checked(root, "AGENTS.md").expect_err("no positive integer");
        assert!(
            error.contains("cannot be shown to load within the instruction limit"),
            "{error}"
        );
        std::fs::write(root.join(".codex/config.toml"), "").unwrap();
        assert_eq!(checked(root, "AGENTS.md"), Ok(()));
        // No managed block in the rendering: its place cannot be found.
        let error = check_with(
            root,
            None,
            "AGENTS.md",
            "no block\n",
            RegionFormat::Markdown,
        )
        .expect_err("no block")
        .to_string();
        assert!(error.contains("its managed block is not found"), "{error}");
    }

    /// A configured value only lowers the budget: a project value above the
    /// default, which a higher-priority setting or an untrusted project
    /// would not honour, leaves the 32 KiB default in force.
    #[test]
    fn a_configured_limit_never_raises_the_budget() {
        let dir = tempfile::tempdir().unwrap();
        let (root, home) = (dir.path().join("project"), dir.path().join("codex"));
        write(&root, "AGENTS.md", 32 * 1024 - BLOCK.len() + 1);
        for raised in [
            "project_doc_max_bytes = 131072\n",
            "project_doc_max_bytes = 32769\n",
        ] {
            config(&root, ".codex/config.toml", raised);
            let error = checked(&root, "AGENTS.md").expect_err("the default still holds");
            assert!(
                error.contains("past the 32768-byte instruction limit (Codex's default)"),
                "{error}"
            );
        }
        config(&home, "config.toml", "project_doc_max_bytes = 999999\n");
        let error = checked_in(&root, Some(&home), "AGENTS.md").expect_err("user raise ignored");
        assert!(error.contains("32768-byte"), "{error}");
        // A value equal to the default is no source of the budget.
        config(
            &root,
            ".codex/config.toml",
            "project_doc_max_bytes = 32768\n",
        );
        let error = checked(&root, "AGENTS.md").expect_err("over");
        assert!(error.contains("(Codex's default)"), "{error}");
    }

    /// The user's Codex config lowers the budget, the lowest of the project
    /// and user values wins, and an unusable user config refuses.
    #[test]
    fn the_user_config_lowers_the_budget_and_an_unusable_one_refuses() {
        let dir = tempfile::tempdir().unwrap();
        let (root, home) = (dir.path().join("project"), dir.path().join("codex"));
        write(&root, "AGENTS.md", 1000);
        assert_eq!(checked_in(&root, Some(&home), "AGENTS.md"), Ok(()));
        config(&home, "config.toml", "project_doc_max_bytes = 500\n");
        let error = checked_in(&root, Some(&home), "AGENTS.md").expect_err("user limit");
        assert!(
            error.contains(
                "past the 500-byte instruction limit (project_doc_max_bytes in the Codex user config)"
            ),
            "{error}"
        );
        // Without the home the file is not read.
        assert_eq!(checked_in(&root, None, "AGENTS.md"), Ok(()));
        // The lower of the two applies, from either side.
        config(&root, ".codex/config.toml", "project_doc_max_bytes = 300\n");
        let error = checked_in(&root, Some(&home), "AGENTS.md").expect_err("project is lower");
        assert!(
            error.contains(
                "300-byte instruction limit (project_doc_max_bytes in .codex/config.toml)"
            ),
            "{error}"
        );
        config(&root, ".codex/config.toml", "project_doc_max_bytes = 900\n");
        let error = checked_in(&root, Some(&home), "AGENTS.md").expect_err("user is lower");
        assert!(error.contains("500-byte"), "{error}");
        // A user config that cannot be used refuses, even when the project
        // config is fine.
        for bad in [
            "project_doc_max_bytes = \"big\"\n",
            "project_doc_max_bytes = 0\n",
            "= not toml\n",
        ] {
            config(&home, "config.toml", bad);
            let error = checked_in(&root, Some(&home), "AGENTS.md").expect_err("unusable");
            assert!(error.contains("the user's Codex config file"), "{error}");
        }
        std::fs::remove_file(home.join("config.toml")).unwrap();
        std::fs::create_dir(home.join("config.toml")).unwrap();
        let error = checked_in(&root, Some(&home), "AGENTS.md").expect_err("a directory");
        assert!(error.contains("not a readable regular file"), "{error}");
    }

    /// `CODEX_HOME` names the Codex home when it is set and not empty, else
    /// `.codex` under the user's home.
    #[test]
    fn the_codex_home_is_resolved_like_codex_does() {
        use std::ffi::OsString;
        use std::path::PathBuf;
        let user = Some(PathBuf::from("/home/u"));
        assert_eq!(
            codex_home_from(Some(OsString::from("/x/codex")), user.clone()),
            Some(PathBuf::from("/x/codex"))
        );
        for unset in [None, Some(OsString::new())] {
            assert_eq!(
                codex_home_from(unset, user.clone()),
                Some(PathBuf::from("/home/u/.codex"))
            );
        }
        assert_eq!(codex_home_from(None, None), None);
    }
}
