//! How long each check of `codeflow ci` took (TSK-1306).
//!
//! A run records one lap per check, in the order the checks run, and
//! reports them on request: `--timings` prints them on stderr after the
//! verdict, and the pre-push hook's hidden `--timings-out <FILE>` writes
//! them to a file the hook names in its over-budget note. A lap is the
//! wall time since the previous lap, so a check's lap holds the setup it
//! shares with nothing else. The record only reports; no verdict reads it.

use std::path::Path;
use std::time::{Duration, Instant};

/// The laps of one run.
pub(super) struct Timings {
    started: Instant,
    last: Instant,
    laps: Vec<(&'static str, Duration)>,
}

impl Timings {
    pub(super) fn start() -> Self {
        let now = Instant::now();
        Self {
            started: now,
            last: now,
            laps: Vec::new(),
        }
    }

    /// Close the lap for `check`: the time since the previous lap. A check
    /// timed twice keeps one row with both laps added.
    pub(super) fn lap(&mut self, check: &'static str) {
        let now = Instant::now();
        let spent = now.duration_since(self.last);
        self.last = now;
        match self.laps.iter_mut().find(|(name, _)| *name == check) {
            Some((_, total)) => *total += spent,
            None => self.laps.push((check, spent)),
        }
    }

    /// The laps in run order, then the run's whole time under `total`.
    pub(super) fn rows(&self) -> Vec<(&'static str, Duration)> {
        let mut rows = self.laps.clone();
        rows.push(("total", self.started.elapsed()));
        rows
    }

    /// Print the rows on stderr, one line per check.
    pub(super) fn print(&self) {
        for (check, spent) in self.rows() {
            eprintln!("codeflow ci: time: {check} {:.2}s", spent.as_secs_f64());
        }
    }

    /// Write the rows to `out`, one `<check>\t<microseconds>` line each.
    /// Best effort: a file that cannot be written leaves the hook without a
    /// breakdown, and the hook still reports its own step times.
    pub(super) fn write(&self, out: &Path) {
        let text: String = self
            .rows()
            .iter()
            .map(|(check, spent)| format!("{check}\t{}\n", spent.as_micros()))
            .collect();
        let _ = std::fs::write(out, text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn laps_keep_run_order_add_repeats_and_end_with_the_total() {
        let mut timings = Timings::start();
        timings.lap("commit");
        std::thread::sleep(Duration::from_millis(5));
        timings.lap("work-records");
        timings.lap("commit");
        let rows = timings.rows();
        let names: Vec<&str> = rows.iter().map(|(name, _)| *name).collect();
        assert_eq!(names, ["commit", "work-records", "total"]);
        assert!(rows[1].1 >= Duration::from_millis(5), "{rows:?}");
        let laps: Duration = rows[..2].iter().map(|(_, spent)| *spent).sum();
        assert!(rows[2].1 >= laps, "{rows:?}");
    }

    #[test]
    fn the_written_rows_are_tab_separated_microseconds() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("timings");
        let mut timings = Timings::start();
        timings.lap("commit");
        timings.write(&out);
        let text = std::fs::read_to_string(&out).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "{text}");
        for (line, name) in lines.iter().zip(["commit", "total"]) {
            let (check, micros) = line.split_once('\t').unwrap();
            assert_eq!(check, name);
            micros.parse::<u128>().unwrap();
        }
    }
}
