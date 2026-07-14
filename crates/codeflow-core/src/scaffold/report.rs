//! Scaffold reports — every file the engine touched, merged, skipped, or
//! refused to touch is reported. Legible degradation (charter principle 8):
//! nothing is ever silent.

use std::fmt;

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
    /// Merge conflict: `<path>.new` written, the file itself untouched.
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
    pub dest: String,
    pub action: Action,
    pub notes: Vec<String>,
}

/// The full init/update report.
#[derive(Debug, Default, Clone)]
pub struct Report {
    /// Heading, e.g. `codeflow init (standard tier)`.
    pub title: String,
    pub files: Vec<FileReport>,
    /// Non-file findings: hook wiring, bootstrap notes, next steps.
    pub notes: Vec<String>,
    pub warnings: Vec<String>,
}

impl Report {
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Self::default()
        }
    }

    pub fn file(&mut self, dest: impl Into<String>, action: Action) {
        self.files.push(FileReport {
            dest: dest.into(),
            action,
            notes: vec![],
        });
    }

    pub fn file_with_notes(&mut self, dest: impl Into<String>, action: Action, notes: Vec<String>) {
        self.files.push(FileReport {
            dest: dest.into(),
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
        writeln!(f, "{}", self.title)?;
        for file in &self.files {
            writeln!(f, "  {:<22} {}", file.action.label(), file.dest)?;
            for note in &file.notes {
                writeln!(f, "      - {note}")?;
            }
        }
        if !self.notes.is_empty() {
            writeln!(f)?;
            for note in &self.notes {
                writeln!(f, "  note: {note}")?;
            }
        }
        if !self.warnings.is_empty() {
            writeln!(f)?;
            for warning in &self.warnings {
                writeln!(f, "  warning: {warning}")?;
            }
        }
        let summary = [
            (Action::Created, self.count(Action::Created)),
            (Action::Added, self.count(Action::Added)),
            (Action::Changed, self.count(Action::Changed)),
            (Action::Merged, self.count(Action::Merged)),
            (Action::Conflicted, self.count(Action::Conflicted)),
            (Action::KeysAdded, self.count(Action::KeysAdded)),
            (Action::Skipped, self.count(Action::Skipped)),
            (Action::MissingAsset, self.count(Action::MissingAsset)),
            (
                Action::KeptUserModified,
                self.count(Action::KeptUserModified),
            ),
            (Action::Forced, self.count(Action::Forced)),
            (Action::Removed, self.count(Action::Removed)),
            (Action::Unchanged, self.count(Action::Unchanged)),
        ];
        let parts: Vec<String> = summary
            .iter()
            .filter(|(_, n)| *n > 0)
            .map(|(a, n)| format!("{} {}", n, a.label()))
            .collect();
        writeln!(f)?;
        writeln!(
            f,
            "  {}",
            if parts.is_empty() {
                "no changes".to_string()
            } else {
                parts.join(", ")
            }
        )
    }
}
