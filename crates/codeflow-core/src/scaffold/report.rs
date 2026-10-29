//! Scaffold reports — every file the engine touched, merged, skipped, or
//! refused to touch is reported. Legible degradation (charter principle 8):
//! nothing is ever silent.

use std::fmt;

use super::Shown;

/// What happened to one destination file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// File written fresh.
    Created,
    /// Missing managed file re-installed by update.
    Added,
    /// Unmodified managed file replaced with the new shipped version, or a
    /// managed region regenerated.
    Changed,
    /// 3-way merge applied cleanly, or structured JSON merge performed.
    Merged,
    /// Conflict: the existing file is untouched. General scaffold merging may
    /// write a sidecar; portal conflicts stop their entire transaction instead.
    Conflicted,
    /// User-owned JSON gained new default keys.
    KeysAdded,
    /// Existing file left alone (exists and not codeflow's to change).
    Skipped,
    /// Shipped asset not present in this build (parallel authoring) — warned.
    MissingAsset,
    /// Nothing to do; file already matches.
    Unchanged,
    /// User-modified file deliberately kept (no upstream change to merge).
    KeptUserModified,
    /// Overwritten because `--force` was passed.
    Forced,
    /// Unmodified managed file deleted because it is no longer shipped by the
    /// new manifest (an artifact removed or renamed upstream).
    Removed,
}

impl Action {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Added => "added",
            Self::Changed => "changed",
            Self::Merged => "merged",
            Self::Conflicted => "CONFLICT",
            Self::KeysAdded => "keys-added",
            Self::Skipped => "skipped",
            Self::MissingAsset => "missing-asset",
            Self::Unchanged => "unchanged",
            Self::KeptUserModified => "kept (user-modified)",
            Self::Forced => "forced",
            Self::Removed => "removed",
        }
    }
}

/// Report line for one destination file.
#[derive(Debug, Clone)]
pub struct FileReport {
    /// The destination path.
    pub dest: Shown,
    pub action: Action,
    pub notes: Vec<Shown>,
}

/// The full init/update report.
#[derive(Default, Clone)]
pub struct Report {
    /// Heading, e.g. `codeflow init (standard tier)`.
    pub title: Shown,
    pub files: Vec<FileReport>,
    /// Non-file findings: hook wiring, bootstrap notes, next steps.
    pub notes: Vec<Shown>,
    pub warnings: Vec<Shown>,
    /// A kept PR template whose mapping awaits the operator's decision
    /// (SPC-013 R-84); an interactive caller asks before it reports.
    pub pending_pr_template: Option<super::pr_template::KeptTemplate>,
}

impl std::fmt::Debug for Report {
    /// The report's own text, as [`Report::shown`] gives it: a derived form
    /// would print the kept PR template's fields (TSK-1029).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Report")
            .field("text", &self.shown().as_str())
            .field("pending_pr_template", &self.pending_pr_template)
            .finish_non_exhaustive()
    }
}

impl Report {
    #[must_use]
    pub fn new(title: Shown) -> Self {
        Self {
            title,
            ..Self::default()
        }
    }

    pub fn file(&mut self, dest: impl AsRef<std::path::Path>, action: Action) {
        self.files.push(FileReport {
            dest: Shown::path(dest),
            action,
            notes: vec![],
        });
    }

    pub fn file_with_notes(
        &mut self,
        dest: impl AsRef<std::path::Path>,
        action: Action,
        notes: Vec<Shown>,
    ) {
        self.files.push(FileReport {
            dest: Shown::path(dest),
            action,
            notes,
        });
    }

    #[must_use]
    pub fn count(&self, action: Action) -> usize {
        self.files.iter().filter(|f| f.action == action).count()
    }

    /// True when any file conflicted (callers may want a non-zero exit).
    #[must_use]
    pub fn has_conflicts(&self) -> bool {
        self.count(Action::Conflicted) > 0
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.block().as_str())
    }
}

impl Report {
    /// The text as a block for a terminal: each `writeln!` is one line
    /// under the block rule, so a quoted value cannot start a line of its
    /// own (TSK-388).
    #[must_use]
    pub fn block(&self) -> crate::git::TextBlock {
        let mut block = crate::git::TextBlock::new();
        let _ = self.write_block(&mut block);
        block
    }

    fn write_block(&self, f: &mut crate::git::TextBlock) -> fmt::Result {
        for line in self.lines() {
            writeln!(f, "{line}")?;
        }
        Ok(())
    }

    /// The report as text inside the display boundary ([`Shown`]), as the
    /// `--diff` export writes it (TSK-1029).
    #[must_use]
    pub fn shown(&self) -> Shown {
        let mut text = Shown::join(self.lines(), "\n");
        text.push(&Shown::fixed("\n"));
        text
    }

    /// The report's lines: every one is built from `Shown` fragments.
    fn lines(&self) -> Vec<Shown> {
        let mut lines = vec![self.title.clone()];
        for file in &self.files {
            lines.push(crate::shown!(
                "  {:<22} {}",
                Shown::fixed(file.action.label()),
                file.dest
            ));
            for note in &file.notes {
                lines.push(crate::shown!("      - {}", note));
            }
        }
        if !self.notes.is_empty() {
            lines.push(Shown::default());
            for note in &self.notes {
                lines.push(crate::shown!("  note: {}", note));
            }
        }
        if !self.warnings.is_empty() {
            lines.push(Shown::default());
            for warning in &self.warnings {
                lines.push(crate::shown!("  warning: {}", warning));
            }
        }
        let summary = [
            Action::Created,
            Action::Added,
            Action::Changed,
            Action::Merged,
            Action::Conflicted,
            Action::KeysAdded,
            Action::Skipped,
            Action::MissingAsset,
            Action::KeptUserModified,
            Action::Forced,
            Action::Removed,
            Action::Unchanged,
        ];
        let parts: Vec<Shown> = summary
            .into_iter()
            .map(|action| (action, self.count(action)))
            .filter(|(_, n)| *n > 0)
            .map(|(action, n)| {
                crate::shown!("{} {}", Shown::count(n), Shown::fixed(action.label()))
            })
            .collect();
        lines.push(Shown::default());
        lines.push(if parts.is_empty() {
            Shown::fixed("  no changes")
        } else {
            crate::shown!("  {}", Shown::join(parts, ", "))
        });
        lines
    }
}
