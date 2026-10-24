//! The one bounded interface for everything the ledger reads or refreshes
//! outside its own log (ADR-0086, TSK-422 AC-13).
//!
//! Every outside observation goes through here: task records on their
//! landing lines, a commit's place on the work target, the landing lines
//! themselves, this clone's repository identity, `gh` and `git ls-remote`,
//! and the heartbeat refresh of a liveness file. Each observer has its own
//! limit ([`Limits`]); an observation that does not finish within it, or
//! cannot be made (offline, a failed read), is unknown, and a caller is
//! never held past the limit. Unknown never closes a follow-up and never
//! makes one due.
//!
//! Two shapes of bound:
//!
//! - [`command`] runs one subprocess in its own process group, drains its
//!   output on reader threads and kills the group at the deadline, so the
//!   process and any descendant holding its pipes end with the bound.
//! - [`within`] runs in-process work (the record readers, which open the
//!   repository and run several guarded Git reads, and the heartbeat
//!   write) on a worker thread and stops waiting at the deadline. The work
//!   is abandoned, not cancelled: a stalled Git read it started runs to its
//!   own end, outside the caller's time. A heartbeat abandoned this way may
//!   still land after the caller returned; a liveness file is a last-seen
//!   time, so a late write only refreshes it.
//!
//! An observation is kept for its freshness window ([`Cache`]) and made
//! again after it, unknown answers included, so one invocation never waits
//! twice on the same stalled read within the window. Nothing here writes
//! except the heartbeat, and only a bound mutation calls that: status reads
//! stay read-only.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// The limit of each observer and the freshness of a cached observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Reading task records on their landing lines (history, acceptance
    /// and binding checks for every task).
    pub records: Duration,
    /// One local Git observation: a commit's place on the work target, the
    /// landing lines, the repository identity.
    pub git: Duration,
    /// One network observation: `gh` or `git ls-remote`.
    pub network: Duration,
    /// Writing one liveness file.
    pub heartbeat: Duration,
    /// How long an observation is reused before it is made again.
    pub freshness: Duration,
}

/// Records: 10 seconds.
pub const RECORDS_TIMEOUT: Duration = Duration::from_secs(10);
/// One local Git observation: 10 seconds.
pub const GIT_TIMEOUT: Duration = Duration::from_secs(10);
/// One network observation: 20 seconds.
pub const NETWORK_TIMEOUT: Duration = Duration::from_secs(20);
/// One heartbeat write: 5 seconds.
pub const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(5);
/// Freshness of a cached observation: 60 seconds.
pub const FRESHNESS: Duration = Duration::from_secs(60);

impl Default for Limits {
    fn default() -> Self {
        Self {
            records: RECORDS_TIMEOUT,
            git: GIT_TIMEOUT,
            network: NETWORK_TIMEOUT,
            heartbeat: HEARTBEAT_TIMEOUT,
            freshness: FRESHNESS,
        }
    }
}

/// Run `work` on a worker thread and wait at most `limit` for it. `None`
/// when it does not finish in time, panics, or the thread cannot start;
/// the work is then abandoned (see the module notes).
pub fn within<T, F>(limit: Duration, work: F) -> Option<T>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("codeflow-observe".into())
        .spawn(move || {
            let _ = send.send(work());
        })
        .ok()?;
    receive.recv_timeout(limit).ok()
}

/// The most output one observation keeps from each stream.
const OUTPUT_LIMIT: u64 = 1 << 20;

/// Run `command` with its output captured, everything bounded by `limit`:
/// the process, its output being drained, and any descendant that inherited
/// its pipes. On unix the command runs in its own process group, which is
/// killed at the deadline; output beyond 1 MiB per stream is dropped. `None`
/// when it cannot start, fails to read, or does not finish in time.
#[must_use]
pub fn command(
    mut command: std::process::Command,
    limit: Duration,
) -> Option<std::process::Output> {
    use std::process::Stdio;
    use std::sync::mpsc;
    let deadline = Instant::now() + limit;
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let reader = |stream: Box<dyn std::io::Read + Send>| {
        let (send, receive) = mpsc::channel();
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let read = std::io::Read::read_to_end(
                &mut std::io::Read::take(stream, OUTPUT_LIMIT),
                &mut bytes,
            );
            let _ = send.send(read.map(|_| bytes));
        });
        receive
    };
    let stdout = reader(Box::new(child.stdout.take()?));
    let stderr = reader(Box::new(child.stderr.take()?));
    let stop = |child: &mut std::process::Child| {
        #[cfg(unix)]
        if let Ok(group) = i32::try_from(child.id()) {
            // SAFETY: kill has no memory preconditions; a negative pid names
            // the process group this command was started in.
            unsafe {
                libc::kill(-group, libc::SIGKILL);
            }
        }
        let _ = child.kill();
        let _ = child.wait();
    };
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            _ => {
                stop(&mut child);
                return None;
            }
        }
    };
    let collect = |receive: &mpsc::Receiver<std::io::Result<Vec<u8>>>| {
        let left = deadline.saturating_duration_since(Instant::now());
        receive.recv_timeout(left).ok()?.ok()
    };
    // A descendant holding the pipes past the deadline makes the observation
    // unknown and is killed with the group; a failed read is unknown too,
    // never an empty answer (which `ls-remote` would mean as "deleted").
    let (Some(out), Some(err)) = (collect(&stdout), collect(&stderr)) else {
        stop(&mut child);
        return None;
    };
    #[cfg(unix)]
    if let Ok(group) = i32::try_from(child.id()) {
        // SAFETY: as above; the group's leader has exited, so this only
        // reaches descendants left behind.
        unsafe {
            libc::kill(-group, libc::SIGKILL);
        }
    }
    Some(std::process::Output {
        status,
        stdout: out,
        stderr: err,
    })
}

/// Observations kept for a freshness window. An unknown answer is kept as
/// well, so a stalled read is waited on once per window, not once per
/// question.
#[derive(Debug)]
pub struct Cache<K, V> {
    freshness: Duration,
    entries: BTreeMap<K, (Instant, V)>,
}

impl<K: Ord, V: Clone> Cache<K, V> {
    /// An empty cache whose entries stay fresh for `freshness`.
    #[must_use]
    pub fn new(freshness: Duration) -> Self {
        Self {
            freshness,
            entries: BTreeMap::new(),
        }
    }

    /// The observation for `key`, made with `observe` when there is none
    /// or the kept one is older than the freshness window.
    pub fn get(&mut self, key: K, observe: impl FnOnce() -> V) -> V {
        self.get_at(key, Instant::now(), observe)
    }

    /// [`Cache::get`] at the time `now`.
    pub fn get_at(&mut self, key: K, now: Instant, observe: impl FnOnce() -> V) -> V {
        if let Some((made, value)) = self.entries.get(&key) {
            if now.saturating_duration_since(*made) <= self.freshness {
                return value.clone();
            }
        }
        // The time the observation finished: its window starts when the
        // answer is known.
        let value = observe();
        let made = now.max(Instant::now());
        self.entries.insert(key, (made, value.clone()));
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hung_observer_returns_unknown_within_its_bound() {
        let limit = Duration::from_millis(200);
        let start = Instant::now();
        let out: Option<u8> = within(limit, || {
            std::thread::sleep(Duration::from_secs(30));
            1
        });
        let elapsed = start.elapsed();
        assert_eq!(out, None);
        assert!(elapsed < limit + Duration::from_secs(2), "{elapsed:?}");
    }

    #[test]
    fn a_prompt_observer_returns_its_answer() {
        assert_eq!(within(Duration::from_secs(5), || 7_u8), Some(7));
    }

    #[test]
    fn a_panicking_observer_is_unknown() {
        let out: Option<u8> = within(Duration::from_secs(5), || panic!("a failed read"));
        assert_eq!(out, None);
    }

    #[cfg(unix)]
    #[test]
    fn a_hung_command_is_killed_at_its_bound() {
        let mut sleeper = std::process::Command::new("/bin/sh");
        sleeper.args(["-c", "sleep 30"]);
        let start = Instant::now();
        assert!(command(sleeper, Duration::from_millis(300)).is_none());
        assert!(start.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_fresh_observation_is_reused_and_a_stale_one_made_again() {
        let mut cache: Cache<&str, Option<u32>> = Cache::new(Duration::from_secs(60));
        let start = Instant::now();
        let mut calls = 0;
        let mut observe = |answer| {
            calls += 1;
            answer
        };
        assert_eq!(cache.get_at("TSK-001", start, || observe(Some(1))), Some(1));
        // Within the window the kept answer is used.
        let inside = start + Duration::from_secs(30);
        assert_eq!(
            cache.get_at("TSK-001", inside, || observe(Some(2))),
            Some(1)
        );
        // Past it the observation is made again.
        let after = start + Duration::from_secs(61);
        assert_eq!(cache.get_at("TSK-001", after, || observe(Some(3))), Some(3));
        assert_eq!(calls, 2);
    }

    #[test]
    fn an_unknown_answer_is_kept_for_its_window_too() {
        let mut cache: Cache<(), Option<u32>> = Cache::new(Duration::from_secs(60));
        let start = Instant::now();
        assert_eq!(cache.get_at((), start, || None), None);
        let inside = start + Duration::from_secs(1);
        assert_eq!(cache.get_at((), inside, || Some(9)), None);
    }
}
