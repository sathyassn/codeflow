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
//! must fit the limit the harness applies.
//!
//! The check fails closed. When the limit cannot be determined, or the
//! block's place in the file cannot be found, the override is refused.

use std::path::Path;

use super::{dest_shown, file_refused};
use crate::scaffold::manifest::RegionFormat;
use crate::scaffold::rule_map::{
    ancestor_instruction_bytes, strict_instruction_limit, LimitSource,
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
/// The refusal naming the file, the end of the block and the limit.
pub(super) fn check(
    root: &Path,
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
        strict_instruction_limit(root).map_err(|reason| undetermined(Shown::fixed(reason)))?;
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
    let source = Shown::fixed(match source {
        LimitSource::Default => "Codex's default",
        LimitSource::ProjectConfig => "project_doc_max_bytes in .codex/config.toml",
    });
    Err(file_refused(shown!(
        "the overrides would put the end of the managed instructions of {} at byte {}, past the {}-byte instruction limit ({}); Codex would cut the review and git rules that follow the overridden text, so shorten the replacements",
        dest_shown(dest),
        Shown::count(loaded),
        Shown::count(limit),
        source
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLOCK: &str =
        "<!-- codeflow:managed:begin scaffold=1 -->\nrules\n<!-- codeflow:managed:end -->";

    fn write(root: &Path, rel: &str, bytes: usize) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "a".repeat(bytes)).unwrap();
    }

    fn checked(root: &Path, dest: &str) -> Result<(), String> {
        check(root, dest, BLOCK, RegionFormat::Markdown).map_err(|error| error.to_string())
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
        let error = check(root, "AGENTS.md", "no block\n", RegionFormat::Markdown)
            .expect_err("no block")
            .to_string();
        assert!(error.contains("its managed block is not found"), "{error}");
    }
}
