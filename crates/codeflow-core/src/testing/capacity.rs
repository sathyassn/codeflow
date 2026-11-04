//! How many jobs heavy runs on this host share, and each run's share
//! (TSK-1145).
//!
//! Heavy runs (full and essential gates, heavy quick targets, `codeflow
//! reserve`) never queue behind each other. The host has a job budget: its
//! logical cores, lowered when available memory cannot give each job
//! [`MEMORY_PER_JOB`], or the `execution.host_jobs` value a project sets.
//! A run that finds the host within capacity uses its own width (its
//! `execution.worker_budget`, else the whole budget); a run that finds more
//! runs active than the budget holds at that width starts at once with the
//! budget divided among the active runs, at least one job.

/// Available memory one compile or test job is assumed to need when the
/// host budget is computed.
pub const MEMORY_PER_JOB: u64 = 1 << 30;

/// A caller's Cargo cap, resolved once so relative CPU counts cannot change
/// between target admissions. This limits both compile and nextest workers.
#[derive(Debug, Clone, Copy)]
pub struct CallerCap {
    pub jobs: Option<usize>,
    cores: usize,
}

impl CallerCap {
    /// Read and validate the caller's cap before any target starts.
    /// # Errors
    /// Refuses non-Unicode or invalid Cargo job counts, naming their source.
    pub fn read() -> Result<Self, String> {
        let cores = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
        let jobs = match std::env::var("CARGO_BUILD_JOBS") {
            Ok(value) => Some(
                parse_cap(&value, cores)
                    .map_err(|cause| format!("caller CARGO_BUILD_JOBS: {cause}"))?,
            ),
            Err(std::env::VarError::NotPresent) => None,
            Err(std::env::VarError::NotUnicode(_)) => {
                return Err("caller CARGO_BUILD_JOBS must be Unicode".into())
            }
        };
        Ok(Self { jobs, cores })
    }

    #[must_use]
    pub fn width(self, configured: Option<usize>) -> Option<usize> {
        match (configured, self.jobs) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// Resolve a target override without permitting it to raise the caller cap.
    /// # Errors
    /// Refuses a malformed target cap before scheduling any target.
    pub fn target(self, target: &super::config::TargetConfig) -> Result<Option<usize>, String> {
        let Some(value) = target.env.get("CARGO_BUILD_JOBS") else {
            return Ok(self.jobs);
        };
        let jobs = parse_cap(value, self.cores)
            .map_err(|cause| format!("target '{}': CARGO_BUILD_JOBS: {cause}", target.name))?;
        Ok(Some(self.jobs.map_or(jobs, |caller| caller.min(jobs))))
    }
}

/// Normalize the documented Cargo forms; the signed range is explicit and
/// arithmetic never overflows, even at the most negative accepted value.
fn parse_cap(value: &str, cores: usize) -> Result<usize, String> {
    if value == "default" {
        return Ok(cores.max(1));
    }
    let digits = value.strip_prefix(['+', '-']).unwrap_or(value);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err("expected a nonzero signed 32-bit integer or default".into());
    }
    let parsed = value
        .parse::<i32>()
        .map_err(|_| "expected a nonzero signed 32-bit integer or default".to_string())?;
    if parsed == 0 {
        return Err("zero is not a job cap".into());
    }
    if parsed > 0 {
        Ok(parsed.unsigned_abs() as usize)
    } else {
        Ok(cores.saturating_sub(parsed.unsigned_abs() as usize).max(1))
    }
}

/// The host's job budget: `configured` when set, else the logical cores,
/// lowered to the available memory divided by [`MEMORY_PER_JOB`] when that
/// can be read, and never below one.
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
        usize::try_from(bytes / MEMORY_PER_JOB).unwrap_or(usize::MAX)
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
    fn cargo_cap_forms_have_a_fixed_normalization_contract() {
        for (text, jobs) in [
            ("2", 2),
            ("+2", 2),
            ("002", 2),
            ("-2", 6),
            ("-8", 1),
            ("-9", 1),
            ("default", 8),
            ("-2147483648", 1),
            ("2147483647", 2_147_483_647),
        ] {
            assert_eq!(parse_cap(text, 8).unwrap(), jobs, "{text}");
        }
        for text in [
            "",
            "0",
            "+0",
            "-0",
            " 2",
            "2 ",
            "many",
            "2147483648",
            "-2147483649",
            "--2",
            "+-2",
        ] {
            assert!(parse_cap(text, 8).is_err(), "{text:?}");
        }
    }

    #[test]
    fn caller_caps_bound_every_admitted_share() {
        let cap = CallerCap {
            jobs: Some(2),
            cores: 8,
        };
        for host in [1, 2, 4, 10, 100] {
            for active in 1..=20 {
                for configured in [None, Some(1), Some(9)] {
                    assert!(run_jobs(host, cap.width(configured), active) <= 2);
                }
            }
        }
    }

    #[test]
    fn the_budget_is_cores_lowered_by_available_memory() {
        assert_eq!(budget_from(16, None), 16);
        assert_eq!(budget_from(16, Some(8 * MEMORY_PER_JOB)), 8);
        assert_eq!(budget_from(16, Some(64 * MEMORY_PER_JOB)), 16);
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
