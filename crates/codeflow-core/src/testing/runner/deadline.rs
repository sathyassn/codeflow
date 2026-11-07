//! Target deadline decisions. Only parent-observed output and scoped CPU are activity.
use std::time::Duration;

use serde::{Deserialize, Serialize};

pub const HEAVY_TIMEOUT_MULTIPLIER: u64 = codeflow_limits::HEAVY_TIMEOUT_MULTIPLIER.default;
pub const DEFAULT_CLEANUP_GRACE_SECONDS: u64 = codeflow_limits::TARGET_CLEANUP_GRACE.default;
pub const MAX_CLEANUP_GRACE_SECONDS: u64 = codeflow_limits::TARGET_CLEANUP_GRACE.most();
pub const DEFAULT_STALL_SECONDS: u64 = codeflow_limits::TARGET_STALL_TIMEOUT.default;
pub const MAX_TIMEOUT_SECONDS: u64 = codeflow_limits::TARGET_TIMEOUT.most();

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetStatus {
    Passed,
    Failed,
    Stalled,
    TimedOut,
    CleanupIncomplete,
    NotRun,
}

impl TargetStatus {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Failed => "FAILED",
            Self::Stalled => "stalled",
            Self::TimedOut => "timed out",
            Self::CleanupIncomplete => "cleanup incomplete",
            Self::NotRun => "not run",
        }
    }

    #[must_use]
    pub fn prerequisite(self) -> &'static str {
        match self {
            Self::Stalled => "not run: prerequisite stalled",
            Self::TimedOut => "not run: prerequisite timed out",
            Self::CleanupIncomplete => "not run: prerequisite cleanup incomplete",
            _ => "not run: prerequisite failed",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct WorkerGrant {
    pub granted: usize,
    pub full: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CpuState {
    Unknown,
    Active,
    Idle,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Supervision {
    pub status: TargetStatus,
    pub heavy: bool,
    pub base_seconds: u64,
    pub ceiling_seconds: u64,
    pub stall_seconds: u64,
    pub workers: Option<WorkerGrant>,
    pub cpu_usec: Option<u64>,
    pub cpu_state: CpuState,
    pub cpu_source: Option<String>,
    pub early_stall_unavailable: Option<String>,
    pub target_pid: u32,
    pub cgroup: Option<String>,
    pub cgroup_identity: Option<(u64, u64)>,
    pub cleanup_details: Vec<String>,
}

impl Supervision {
    #[must_use]
    pub fn detail(&self) -> String {
        match self.status {
            TargetStatus::Stalled => {
                let workers = self.workers.map_or_else(
                    || "workers unknown".into(),
                    |w| format!("workers {} of {}", w.granted, w.full),
                );
                format!(
                    "stalled: no progress for {} s ({workers})",
                    self.stall_seconds
                )
            }
            TargetStatus::TimedOut if self.heavy => {
                format!("timed out at the ceiling after {} s", self.ceiling_seconds)
            }
            TargetStatus::TimedOut => format!(
                "TIMEOUT: exceeded timeout_seconds limit; timeout after {}s",
                self.ceiling_seconds
            ),
            other => other.label().into(),
        }
    }
}

#[must_use]
pub fn result_status(exit: i32, error: Option<&str>, info: Option<&Supervision>) -> TargetStatus {
    if let Some(info) = info {
        if info.status != TargetStatus::Passed && info.status != TargetStatus::Failed {
            return info.status;
        }
    }
    if exit == 0 && error.is_none() {
        TargetStatus::Passed
    } else {
        TargetStatus::Failed
    }
}

/// Elapsed durations make the boundary testable without long sleeps or distant Instants.
pub(super) struct Deadline {
    pub info: Supervision,
    pub ceiling: Duration,
    /// The target's `cleanup_grace_seconds`, default five seconds.
    pub cleanup_grace: Duration,
    last_activity: Duration,
    next_heartbeat: Duration,
}

impl Deadline {
    pub fn new(base: Duration, heavy: bool, stall: u64, workers: Option<WorkerGrant>) -> Self {
        Self {
            ceiling: base.saturating_mul(if heavy {
                u32::try_from(HEAVY_TIMEOUT_MULTIPLIER).expect("small multiplier")
            } else {
                1
            }),
            info: Supervision {
                status: TargetStatus::Passed,
                heavy,
                base_seconds: base.as_secs(),
                ceiling_seconds: base.as_secs() * if heavy { HEAVY_TIMEOUT_MULTIPLIER } else { 1 },
                stall_seconds: stall,
                workers,
                cpu_usec: None,
                cpu_state: CpuState::Unknown,
                cpu_source: None,
                early_stall_unavailable: None,
                target_pid: 0,
                cgroup: None,
                cgroup_identity: None,
                cleanup_details: Vec::new(),
            },
            cleanup_grace: Duration::from_secs(DEFAULT_CLEANUP_GRACE_SECONDS),
            last_activity: Duration::ZERO,
            next_heartbeat: codeflow_limits::RUNNER_HEARTBEAT_INTERVAL_SECONDS.wait(),
        }
    }

    pub fn output(&mut self, now: Duration) {
        self.last_activity = now;
    }

    pub fn cpu(&mut self, now: Duration, sample: Result<u64, String>) {
        match sample {
            Ok(value) => {
                self.info.cpu_state = if value > self.info.cpu_usec.unwrap_or(0) {
                    CpuState::Active
                } else {
                    CpuState::Idle
                };
                if self.info.cpu_usec.is_some_and(|old| value < old) {
                    self.unavailable("CPU counter regressed".into());
                    return;
                } else if value > self.info.cpu_usec.unwrap_or(0) {
                    self.last_activity = now;
                }
                self.info.cpu_usec = Some(value);
            }
            Err(reason) => self.unavailable(reason),
        }
    }

    pub fn unavailable(&mut self, reason: String) {
        self.info.cpu_state = CpuState::Unavailable;
        if self.info.early_stall_unavailable.is_none() {
            self.info.early_stall_unavailable = Some(reason);
        }
    }

    pub fn decision(&self, now: Duration) -> Option<TargetStatus> {
        if now >= self.ceiling {
            Some(TargetStatus::TimedOut)
        } else if self.info.heavy
            && self.info.early_stall_unavailable.is_none()
            && now.saturating_sub(self.last_activity)
                >= Duration::from_secs(self.info.stall_seconds)
        {
            Some(TargetStatus::Stalled)
        } else {
            None
        }
    }

    pub fn heartbeat(&mut self, now: Duration, tests: &str) -> Option<String> {
        if !self.info.heavy || now < self.next_heartbeat {
            return None;
        }
        self.next_heartbeat =
            now.saturating_add(codeflow_limits::RUNNER_HEARTBEAT_INTERVAL_SECONDS.wait());
        let observed = self
            .info
            .cpu_usec
            .map_or_else(|| "unknown".into(), |value| format!("{value} us"));
        let cpu = self.info.early_stall_unavailable.as_ref().map_or_else(
            || format!("{observed} (scoped accounting)"),
            |why| format!("early stall unavailable: {why}; observed {observed}"),
        );
        Some(format!(
            "tests done: {tests}; CPU state: {:?}; CPU: {cpu}; elapsed {} s",
            self.info.cpu_state,
            now.as_secs()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deadline_invariants_hold_across_windows_and_activity_times() {
        for base in 1..=32 {
            for stall in [1, base, 4 * base, 4 * base + 1] {
                for activity in [0, base, 4 * base - 1, 4 * base] {
                    let mut d = Deadline::new(Duration::from_secs(base), true, stall, None);
                    d.output(Duration::from_secs(activity));
                    assert_eq!(
                        d.decision(Duration::from_secs(4 * base)),
                        Some(TargetStatus::TimedOut)
                    );
                    d.unavailable("incomplete accounting".into());
                    for elapsed in [0, base, 4 * base - 1] {
                        assert_eq!(d.decision(Duration::from_secs(elapsed)), None);
                    }
                }
            }
        }
    }

    #[test]
    fn stall_detail_and_serialized_status_are_distinct_from_failure() {
        let mut d = Deadline::new(
            Duration::from_secs(10),
            true,
            3,
            Some(WorkerGrant {
                granted: 1,
                full: 9,
            }),
        );
        d.info.status = TargetStatus::Stalled;
        assert_eq!(
            d.info.detail(),
            "stalled: no progress for 3 s (workers 1 of 9)"
        );
        assert_eq!(
            TargetStatus::Stalled.prerequisite(),
            "not run: prerequisite stalled"
        );
        assert_eq!(TargetStatus::Failed.label(), "FAILED");
        let status = serde_json::to_value(d.info.status).unwrap();
        assert_eq!(status, "stalled");
    }

    #[test]
    fn activity_stall_ceiling_and_unavailability_have_distinct_boundaries() {
        let mut d = Deadline::new(Duration::from_secs(100), true, 5, None);
        d.cpu(Duration::from_secs(4), Ok(10));
        assert_eq!(d.decision(Duration::from_secs(8)), None);
        assert_eq!(
            d.decision(Duration::from_secs(9)),
            Some(TargetStatus::Stalled)
        );
        d.unavailable("lost accounting".into());
        assert_eq!(d.decision(Duration::from_secs(399)), None);
        d.output(Duration::from_secs(400));
        assert_eq!(
            d.decision(Duration::from_secs(400)),
            Some(TargetStatus::TimedOut)
        );
    }

    #[test]
    fn heartbeats_do_not_refresh_activity_and_show_unknown_counts() {
        let mut d = Deadline::new(Duration::from_secs(1000), true, 301, None);
        assert!(d.heartbeat(Duration::from_secs(299), "unknown").is_none());
        assert!(d
            .heartbeat(Duration::from_secs(300), "unknown")
            .unwrap()
            .contains("tests done: unknown"));
        assert_eq!(
            d.decision(Duration::from_secs(301)),
            Some(TargetStatus::Stalled)
        );
        assert!(d
            .heartbeat(Duration::from_secs(600), "12 of 20")
            .unwrap()
            .contains("12 of 20"));
        d.unavailable("no delegated cgroup".into());
        let line = d.heartbeat(Duration::from_secs(900), "unknown").unwrap();
        assert!(line.contains("no delegated cgroup"));
        assert!(line.contains("observed unknown"));
        assert!(!line.contains("None"));
        assert_eq!(
            d.decision(Duration::from_secs(4000)),
            Some(TargetStatus::TimedOut)
        );
    }

    #[test]
    fn unknown_or_regressing_cpu_never_authorizes_a_stall() {
        for sample in [Err("unreadable".into()), Ok(1)] {
            let mut d = Deadline::new(Duration::from_secs(10), true, 1, None);
            d.cpu(Duration::ZERO, Ok(100));
            d.cpu(Duration::ZERO, sample);
            assert_eq!(d.decision(Duration::from_secs(39)), None);
            assert_eq!(
                d.decision(Duration::from_secs(40)),
                Some(TargetStatus::TimedOut)
            );
        }
    }
}
