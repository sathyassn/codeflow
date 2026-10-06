//! The ceremony report (TSK-149): the process cost that does not change the
//! product, over a window of merged pull requests.
//!
//! Three measures, each from one source:
//!
//! | Measure | Source |
//! |---|---|
//! | Pull requests per logical change | merge commits in this clone's refs |
//! | Review rounds per pull request | the host, through `gh` (SPC-013 R-103) |
//! | Refusals hit by hooks and guards | this clone's refusals ledger |
//!
//! The review count is `codeflow`'s one host-backed read: when the host
//! call fails, or the host holds no review for the window, the report
//! prints `unknown` and never estimates. The other two measures are local
//! and always print.

mod history;
mod host;

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

pub use history::{merged_prs, MergedPr};
pub use host::{Gh, ReviewHost};

use crate::ledger::refusal;

/// The window a report covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Window {
    /// Pull requests numbered `first` to `last`, both included.
    Numbers {
        /// The first number.
        first: u64,
        /// The last number.
        last: u64,
    },
    /// Pull requests merged on `since` (a UTC `YYYY-MM-DD` date) or later,
    /// up to and including `until` when given.
    Dates {
        /// The first day.
        since: String,
        /// The last day.
        until: Option<String>,
    },
}

impl Window {
    /// A window of pull request numbers written `FIRST..LAST`.
    ///
    /// # Errors
    ///
    /// Returns a message for any other text, or a range that runs backwards.
    pub fn numbers(text: &str) -> Result<Self, String> {
        let bad = || format!("`{text}` is not a range of pull request numbers such as 568..644");
        let (first, last) = text.split_once("..").ok_or_else(bad)?;
        let first: u64 = first.trim_matches([' ', '\t']).parse().map_err(|_| bad())?;
        let last: u64 = last.trim_matches([' ', '\t']).parse().map_err(|_| bad())?;
        if first > last {
            return Err(bad());
        }
        Ok(Self::Numbers { first, last })
    }

    /// A window of merge dates.
    ///
    /// # Errors
    ///
    /// Returns a message for a date not written `YYYY-MM-DD`, or an `until`
    /// before `since`.
    pub fn dates(since: &str, until: Option<&str>) -> Result<Self, String> {
        check_date(since)?;
        if let Some(until) = until {
            check_date(until)?;
            if until < since {
                return Err(format!("--until {until} is before --since {since}"));
            }
        }
        Ok(Self::Dates {
            since: since.to_string(),
            until: until.map(str::to_string),
        })
    }

    /// The RFC 3339 bounds of a date window: its first second, and its last
    /// when it has an end.
    fn span_of_dates(&self) -> (String, Option<String>) {
        match self {
            Self::Dates { since, until } => (
                format!("{since}T00:00:00Z"),
                until.as_ref().map(|until| format!("{until}T23:59:59Z")),
            ),
            Self::Numbers { .. } => (String::new(), None),
        }
    }
}

fn check_date(text: &str) -> Result<(), String> {
    let bytes = text.as_bytes();
    let shaped = bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit());
    let in_range = shaped
        && matches!(text[5..7].parse::<u8>(), Ok(1..=12))
        && matches!(text[8..10].parse::<u8>(), Ok(1..=31));
    if in_range {
        Ok(())
    } else {
        Err(format!("`{text}` is not a date written YYYY-MM-DD"))
    }
}

/// Review rounds for the window's pull requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reviews {
    /// No merged pull request, so the host was not asked.
    NotAsked,
    /// The host call failed, or it holds no review for the window.
    Unknown(String),
    /// Submitted reviews per pull request that has any.
    Known(BTreeMap<u64, u64>),
}

/// Refusals recorded in the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusals {
    /// This clone cannot say.
    Unknown(String),
    /// Refusals by plane; `partial_since` is when recording began, when that
    /// is inside the window.
    Counted {
        /// Refusals by plane.
        by_plane: BTreeMap<String, usize>,
        /// When recording began, if inside the window.
        partial_since: Option<String>,
    },
}

/// A ceremony report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// The window it covers.
    pub window: Window,
    /// The merged pull requests in it, by number.
    pub prs: Vec<MergedPr>,
    /// Review rounds.
    pub reviews: Reviews,
    /// Refusals.
    pub refusals: Refusals,
}

/// Build the report for the repository at `repo_root`. `prefixes` are the
/// policy's branch prefixes.
///
/// # Errors
///
/// Returns a message when the repository's history cannot be read.
pub fn build(
    repo_root: &Path,
    prefixes: &[String],
    window: Window,
    host: &dyn ReviewHost,
) -> Result<Report, String> {
    let prs = merged_prs(repo_root, prefixes, &window)?;
    let reviews = reviews(repo_root, &prs, host);
    let refusals = refusals(repo_root, &window, &prs);
    Ok(Report {
        window,
        prs,
        reviews,
        refusals,
    })
}

fn reviews(repo_root: &Path, prs: &[MergedPr], host: &dyn ReviewHost) -> Reviews {
    if prs.is_empty() {
        return Reviews::NotAsked;
    }
    let numbers: Vec<u64> = prs.iter().map(|pr| pr.number).collect();
    match host.rounds(repo_root, &numbers) {
        Err(error) => Reviews::Unknown(format!("the host call failed: {error}")),
        Ok(rounds) if rounds.is_empty() => Reviews::Unknown(format!(
            "the host holds no submitted review for these {} pull requests",
            prs.len()
        )),
        Ok(rounds) => Reviews::Known(rounds),
    }
}

fn refusals(repo_root: &Path, window: &Window, prs: &[MergedPr]) -> Refusals {
    let (start, end) = match window {
        Window::Dates { .. } => window.span_of_dates(),
        Window::Numbers { .. } => {
            let (Some(start), Some(end)) = (
                prs.iter().map(|pr| pr.started_at.clone()).min(),
                prs.iter().map(|pr| pr.merged_at.clone()).max(),
            ) else {
                return Refusals::Unknown(
                    "no merged pull request in the window gives it a time span".to_string(),
                );
            };
            (start, Some(end))
        }
    };
    let info = match crate::hooks::RepoInfo::discover(repo_root) {
        Ok(Some(info)) => info,
        Ok(None) => return Refusals::Unknown("not a git repository".to_string()),
        Err(error) => return Refusals::Unknown(format!("cannot read repository: {error}")),
    };
    let log = match refusal::read(&info.ledger_dir()) {
        Ok(log) => log,
        Err(error) => {
            return Refusals::Unknown(format!("the refusals ledger is unreadable: {error}"))
        }
    };
    let Some(since) = log.since else {
        return Refusals::Unknown(
            "this clone has recorded none; hooks and guards record from their first run of \
             this version"
                .to_string(),
        );
    };
    if end.as_deref().is_some_and(|end| end < since.as_str()) {
        return Refusals::Unknown(format!(
            "this clone began recording at {since}, after this window"
        ));
    }
    let mut by_plane = BTreeMap::new();
    for recorded in &log.refusals {
        let at = recorded.timestamp.as_str();
        if at >= start.as_str() && end.as_deref().is_none_or(|end| at <= end) {
            *by_plane.entry(recorded.plane.clone()).or_insert(0) += 1;
        }
    }
    Refusals::Counted {
        by_plane,
        partial_since: (start < since).then_some(since),
    }
}

/// Pull requests and logical changes of one row.
#[derive(Default)]
struct Row {
    prs: usize,
    changes: std::collections::BTreeSet<String>,
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.window {
            Window::Numbers { first, last } => {
                writeln!(f, "Ceremony report: pull requests {first} to {last}")?;
            }
            Window::Dates { since, until: None } => {
                writeln!(f, "Ceremony report: pull requests merged from {since}")?;
            }
            Window::Dates {
                since,
                until: Some(until),
            } => writeln!(
                f,
                "Ceremony report: pull requests merged from {since} to {until}"
            )?,
        }
        let (Some(first), Some(last)) = (
            self.prs.iter().map(|pr| &pr.merged_at).min(),
            self.prs.iter().map(|pr| &pr.merged_at).max(),
        ) else {
            writeln!(
                f,
                "  no merged pull request in this clone's refs is in the window"
            )?;
            writeln!(f)?;
            return self.fmt_measures(f);
        };
        writeln!(
            f,
            "  {} merged, {first} to {last}, read from this clone's refs",
            self.prs.len()
        )?;
        writeln!(f)?;
        writeln!(f, "Pull requests per logical change")?;
        let mut rows: BTreeMap<&str, Row> = BTreeMap::new();
        let mut status_only: Vec<u64> = Vec::new();
        let mut all = Row::default();
        for pr in &self.prs {
            all.prs += 1;
            if pr.status_only {
                status_only.push(pr.number);
                continue;
            }
            let row = rows.entry(pr.kind.as_str()).or_default();
            row.prs += 1;
            row.changes.insert(pr.change.clone());
            all.changes.insert(pr.change.clone());
        }
        writeln!(
            f,
            "  {:<16}{:>14}{:>17}{:>12}",
            "kind", "pull requests", "logical changes", "per change"
        )?;
        for (kind, row) in &rows {
            table_row(f, kind, row.prs, Some(row.changes.len()))?;
        }
        table_row(f, "record status", status_only.len(), None)?;
        table_row(f, "all", all.prs, Some(all.changes.len()))?;
        writeln!(
            f,
            "  A task's pull requests are one change. A record status pull request changes\n  \
             only a record's status, acceptance evidence or closeout."
        )?;
        if !status_only.is_empty() {
            let numbers: Vec<String> = status_only.iter().map(u64::to_string).collect();
            writeln!(f, "  Record status pull requests: {}", numbers.join(", "))?;
        }
        writeln!(f)?;
        self.fmt_measures(f)
    }
}

impl Report {
    fn fmt_measures(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Review rounds per pull request: ")?;
        match &self.reviews {
            Reviews::NotAsked => writeln!(f, "unknown (no pull request to ask about)")?,
            Reviews::Unknown(why) => writeln!(f, "unknown ({why})")?,
            Reviews::Known(rounds) => {
                let total: u64 = rounds.values().sum();
                let known = rounds.len();
                #[allow(clippy::cast_precision_loss)]
                let mean = total as f64 / known as f64;
                let missing = self.prs.len().saturating_sub(known);
                write!(
                    f,
                    "{mean:.2} over the {known} pull requests with host review data"
                )?;
                if missing > 0 {
                    write!(f, "; unknown for the other {missing}")?;
                }
                writeln!(f)?;
            }
        }
        write!(f, "Refusals hit by this clone's hooks and guards: ")?;
        match &self.refusals {
            Refusals::Unknown(why) => writeln!(f, "unknown ({why})"),
            Refusals::Counted {
                by_plane,
                partial_since,
            } => {
                let total: usize = by_plane.values().sum();
                write!(f, "{total}")?;
                if let Some(since) = partial_since {
                    write!(
                        f,
                        " since recording began at {since}, inside the window; earlier refusals are unknown"
                    )?;
                }
                if !by_plane.is_empty() {
                    let planes: Vec<String> = by_plane
                        .iter()
                        .map(|(plane, count)| format!("{plane} {count}"))
                        .collect();
                    write!(f, " ({})", planes.join(", "))?;
                }
                writeln!(f)
            }
        }
    }
}

fn table_row(
    f: &mut fmt::Formatter<'_>,
    kind: &str,
    prs: usize,
    changes: Option<usize>,
) -> fmt::Result {
    let (changes, per) = match changes {
        Some(changes) if changes > 0 => {
            #[allow(clippy::cast_precision_loss)]
            let per = prs as f64 / changes as f64;
            (changes.to_string(), format!("{per:.2}"))
        }
        Some(changes) => (changes.to_string(), "-".to_string()),
        None => ("-".to_string(), "-".to_string()),
    };
    writeln!(f, "  {kind:<16}{prs:>14}{changes:>17}{per:>12}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_number_window_reads_first_and_last() {
        assert_eq!(
            Window::numbers("568..644").unwrap(),
            Window::Numbers {
                first: 568,
                last: 644
            }
        );
        assert!(Window::numbers("644..568").is_err());
        assert!(Window::numbers("568").is_err());
        assert!(Window::numbers("a..b").is_err());
    }

    #[test]
    fn a_date_window_takes_only_written_dates_in_order() {
        assert!(Window::dates("2026-09-20", Some("2026-09-28")).is_ok());
        assert!(Window::dates("2026-9-20", None).is_err());
        assert!(Window::dates("2026-13-01", None).is_err());
        assert!(Window::dates("2026-09-28", Some("2026-09-20")).is_err());
        let window = Window::dates("2026-09-20", Some("2026-09-28")).unwrap();
        assert_eq!(
            window.span_of_dates(),
            (
                "2026-09-20T00:00:00Z".to_string(),
                Some("2026-09-28T23:59:59Z".to_string())
            )
        );
    }
}
