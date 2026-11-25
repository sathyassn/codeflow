//! How many jobs heavy runs on this host share, and each run's share
//! (TSK-1145).
//!
//! Heavy runs (full and essential gates, heavy quick targets, `codeflow
//! reserve`) never queue behind each other. The host has a job budget: its
//! logical cores, lowered when available memory cannot give each job
//! [`codeflow_limits::MEMORY_PER_JOB`], or the `execution.host_jobs` value a
//! project sets. A run that finds the host within capacity uses its own width
//! (its `execution.worker_budget`, else the whole budget); a run that finds
//! more runs active than the budget holds at that width starts at once with
//! the budget divided among the active runs, at least one job.
//!
//! The computed budget is guidance (ADR-0083). It sizes a run nobody sized,
//! and it never lowers a value the project set: an `execution.worker_budget`
//! above it is used as set, with a warning (TSK-1299).

/// What one heavy run gets from the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunShare {
    /// The jobs the run uses.
    pub jobs: usize,
    /// The budget the run was measured against.
    pub host: usize,
    /// Whether sharing the host lowered the run below the width it asked for.
    pub lowered: bool,
    /// The warning to report when a value the project set is above the
    /// memory-derived budget. The run goes ahead either way.
    pub warning: Option<&'static str>,
}

/// The share of a run with the project's `configured` host budget, its
/// `per_run` width, and `active` heavy runs on the host.
#[must_use]
pub fn run_share(configured: Option<usize>, per_run: Option<usize>, active: usize) -> RunShare {
    share_of(configured, host_jobs(None), per_run, active)
}

/// [`run_share`] against a `computed` budget. Only a budget the project set
/// bounds a width the project set.
fn share_of(
    configured: Option<usize>,
    computed: usize,
    per_run: Option<usize>,
    active: usize,
) -> RunShare {
    if let (None, Some(width)) = (configured, per_run) {
        let jobs = width.max(1);
        return RunShare {
            jobs,
            host: computed,
            lowered: false,
            warning: (jobs > computed).then_some(codeflow_limits::WORKER_BUDGET.warning),
        };
    }
    let host = configured.map_or(computed, |jobs| jobs.max(1));
    let jobs = run_jobs(host, per_run, active);
    RunShare {
        jobs,
        host,
        lowered: lowered(host, per_run, active),
        warning: (jobs > computed).then_some(codeflow_limits::HOST_JOBS.warning),
    }
}

/// The host's job budget: `configured` when set, else the logical cores,
/// lowered to the available memory divided by
/// [`codeflow_limits::MEMORY_PER_JOB`] when that can be read, and never below
/// one.
#[must_use]
pub fn host_jobs(configured: Option<usize>) -> usize {
    if let Some(jobs) = configured {
        return jobs.max(1);
    }
    let cores = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    budget_from(cores, available_memory())
}

/// The budget from `cores` and `memory` bytes available, when known.
#[must_use]
pub fn budget_from(cores: usize, memory: Option<u64>) -> usize {
    let by_memory = memory.map_or(usize::MAX, |bytes| {
        bytes
            .checked_div(codeflow_limits::MEMORY_PER_JOB.value())
            .map_or(usize::MAX, |jobs| {
                usize::try_from(jobs).unwrap_or(usize::MAX)
            })
    });
    cores.min(by_memory).max(1)
}

/// The jobs one run uses. Its width is `per_run` capped at `host_jobs`
/// (the whole budget when `per_run` is unset); while the `active` runs,
/// this one included, fit the host at that width it uses the width,
/// otherwise `host_jobs` divided by `active`, at least one. The result never
/// exceeds `per_run` or `host_jobs`, and no run count refuses a run.
#[must_use]
pub fn run_jobs(host_jobs: usize, per_run: Option<usize>, active: usize) -> usize {
    let host_jobs = host_jobs.max(1);
    let width = per_run.unwrap_or(host_jobs).clamp(1, host_jobs);
    let fits = (host_jobs / width).max(1);
    if active <= fits {
        width
    } else {
        (host_jobs / active.max(1)).clamp(1, width)
    }
}

/// Whether the run's job count is below the width it asked for: its
/// `per_run` is above the host budget, or it shares the host with more
/// runs than fit.
#[must_use]
pub fn lowered(host_jobs: usize, per_run: Option<usize>, active: usize) -> bool {
    run_jobs(host_jobs, per_run, active) < per_run.unwrap_or(host_jobs).max(1)
}

/// Bytes of memory available to new work, when the platform reports it.
#[must_use]
pub fn available_memory() -> Option<u64> {
    platform::available_memory()
}

#[cfg(target_os = "linux")]
mod platform {
    pub(super) fn available_memory() -> Option<u64> {
        let text = std::fs::read_to_string("/proc/meminfo").ok()?;
        let line = text
            .lines()
            .find(|line| line.starts_with("MemAvailable:"))?;
        let kib: u64 = line
            .trim_start_matches("MemAvailable:")
            .trim()
            .trim_end_matches("kB")
            .trim()
            .parse()
            .ok()?;
        kib.checked_mul(1024)
    }
}

#[cfg(target_os = "macos")]
mod platform {
    fn sysctl<T: Copy + Default>(name: &std::ffi::CStr) -> Option<T> {
        let mut value = T::default();
        let mut size = std::mem::size_of::<T>();
        // SAFETY: the name is NUL-terminated, the output buffer is a `T`
        // whose size is passed, and no new value is set.
        let status = unsafe {
            libc::sysctlbyname(
                name.as_ptr(),
                (&raw mut value).cast(),
                &raw mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        (status == 0 && size == std::mem::size_of::<T>()).then_some(value)
    }

    pub(super) fn available_memory() -> Option<u64> {
        // The kernel's memory pressure view: the percentage of memory
        // available, free and reclaimable together.
        let total: u64 = sysctl(c"hw.memsize")?;
        let level: u32 = sysctl(c"kern.memorystatus_level")?;
        (level <= 100).then(|| total / 100 * u64::from(level))
    }
}

#[cfg(windows)]
mod platform {
    pub(super) fn available_memory() -> Option<u64> {
        use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
        // SAFETY: MEMORYSTATUSEX is plain data; its length field is set as
        // the call requires before the call fills the rest.
        unsafe {
            let mut status: MEMORYSTATUSEX = std::mem::zeroed();
            status.dwLength = u32::try_from(std::mem::size_of::<MEMORYSTATUSEX>()).ok()?;
            (GlobalMemoryStatusEx(&raw mut status) != 0).then_some(status.ullAvailPhys)
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
mod platform {
    pub(super) fn available_memory() -> Option<u64> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_budget_is_cores_lowered_by_available_memory() {
        assert_eq!(budget_from(16, None), 16);
        let per_job = codeflow_limits::MEMORY_PER_JOB.default;
        assert_eq!(budget_from(16, Some(8 * per_job)), 8);
        assert_eq!(budget_from(16, Some(64 * per_job)), 16);
        assert_eq!(budget_from(16, Some(0)), 1);
        assert_eq!(host_jobs(Some(6)), 6);
        assert_eq!(host_jobs(Some(0)), 1);
        assert!(host_jobs(None) >= 1);
    }

    #[test]
    fn runs_within_capacity_keep_their_width() {
        // Sixteen jobs at four per run fit four runs.
        for active in 1..=4 {
            assert_eq!(run_jobs(16, Some(4), active), 4);
            assert!(!lowered(16, Some(4), active));
        }
        // A run with no width of its own uses the whole budget when alone.
        assert_eq!(run_jobs(16, None, 1), 16);
    }

    #[test]
    fn a_per_run_width_above_the_host_budget_is_capped_by_it() {
        // R483-05: a run alone never takes more than the host's jobs.
        assert_eq!(run_jobs(2, Some(9), 1), 2);
        assert!(lowered(2, Some(9), 1));
        assert_eq!(run_jobs(2, None, 1), 2);
        assert!(!lowered(2, None, 1));
        assert_eq!(run_jobs(4, Some(9), 3), 1);
        // Past any run count a run still starts with one job.
        assert_eq!(run_jobs(4, Some(2), 1000), 1);
    }

    #[test]
    fn an_explicit_worker_budget_above_the_computed_budget_is_used_as_set() {
        // Memory suggests two jobs; the project asked for nine.
        let share = share_of(None, 2, Some(9), 1);
        assert_eq!(share.jobs, 9);
        assert!(!share.lowered);
        assert_eq!(share.warning, Some(codeflow_limits::WORKER_BUDGET.warning));
        // Other runs on the host do not lower it either.
        assert_eq!(share_of(None, 2, Some(9), 5).jobs, 9);
        // Within the computed budget there is nothing to warn about.
        assert_eq!(share_of(None, 16, Some(4), 1).warning, None);
    }

    #[test]
    fn a_large_host_budget_runs_with_a_warning() {
        let share = share_of(Some(64), 2, None, 1);
        assert_eq!(share.jobs, 64);
        assert_eq!(share.warning, Some(codeflow_limits::HOST_JOBS.warning));
    }

    #[test]
    fn a_host_budget_the_project_set_still_bounds_a_shared_run() {
        let share = share_of(Some(4), 64, Some(2), 4);
        assert_eq!(share.jobs, 1);
        assert!(share.lowered);
        assert_eq!(share.warning, None);
    }

    #[test]
    fn a_run_nobody_sized_takes_the_computed_budget_without_a_warning() {
        let share = share_of(None, 2, None, 1);
        assert_eq!(share.jobs, 2);
        assert_eq!(share.warning, None);
    }

    #[test]
    fn runs_past_capacity_divide_the_budget_instead_of_waiting() {
        assert_eq!(run_jobs(16, Some(4), 5), 3);
        assert_eq!(run_jobs(16, Some(4), 8), 2);
        assert_eq!(run_jobs(16, Some(4), 40), 1);
        assert_eq!(run_jobs(16, None, 2), 8);
        assert_eq!(run_jobs(4, Some(4), 2), 2);
        assert!(lowered(4, Some(4), 2));
        // Never more than the run's own width.
        assert_eq!(run_jobs(16, Some(2), 9), 1);
        assert_eq!(run_jobs(16, Some(2), 3), 2);
    }

    #[test]
    fn available_memory_is_reported_where_the_platform_has_it() {
        if cfg!(any(target_os = "linux", target_os = "macos", windows)) {
            assert!(available_memory().is_some_and(|bytes| bytes > 0));
        }
    }
}
