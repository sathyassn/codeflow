//! A kill-on-close job object for each target on Windows (TSK-142 AC-3).
//!
//! Unix runs each target in its own process group, and the gate's slot
//! counts as busy while that group outlives a killed gate (TSK-134,
//! TSK-1145). Windows has no such group, so a killed gate used to leave its
//! target running while the OS freed the lock, unseen by later runs.
//!
//! Here the runner starts the target suspended, places it in a new job
//! object and then lets it run. The job carries
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, so the OS ends every process in the
//! target's tree when the job's last handle closes. Children join the job as
//! they are created, and the target cannot escape it, because the job does
//! not allow breakaway. Starting suspended closes the window in which the
//! target could start a child before it joins.
//!
//! With no fence held, this process holds the only handle, so its end,
//! however it ends, ends the tree. The OS gives no order between that and
//! freeing the gate's fence, so while the gate holds a fence a watcher
//! outside the job holds it too, along with a copy of the job handle
//! ([`super::job_watch`], TSK-1318). The watcher starts while the target is
//! still suspended. When the gate ends, the watcher ends the job and frees
//! its fences once the job is empty. If the watcher is killed as well, the
//! last handle closes and the tree still ends, and the fence frees with the
//! gate.

use std::ffi::c_void;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::process::Child;

use super::AdoptError;

#[cfg(test)]
thread_local! {
    static FAIL_FOR_TEST: std::cell::RefCell<Option<std::path::PathBuf>> = const {
        std::cell::RefCell::new(None)
    };
}

#[cfg(test)]
pub(super) fn fail_adoption_for_test(pid_file: Option<std::path::PathBuf>) {
    FAIL_FOR_TEST.with(|slot| *slot.borrow_mut() = pid_file);
}

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};

/// Creation flag that starts the target suspended until [`TargetJob::adopt`].
///
/// A local constant, not a re-export: rustdoc resolves a link in a foreign
/// re-export's docs from the foreign crate and stops with an internal error.
pub const CREATE_SUSPENDED: u32 = windows_sys::Win32::System::Threading::CREATE_SUSPENDED;

/// The job holding one target's process tree, with the target's fence
/// watcher. Dropping it never waits. The watcher, signaled by
/// [`Self::terminate`] or at the latest by the drop, ends any process still
/// in the tree and exits once the job is empty; with no watcher, closing the
/// handle ends them.
pub struct TargetJob {
    job: OwnedHandle,
    watch: Option<super::job_watch::JobWatch>,
}

impl TargetJob {
    pub(super) fn accounting(&self) -> std::io::Result<(u64, u32)> {
        use windows_sys::Win32::System::JobObjects::{
            JobObjectBasicAccountingInformation, QueryInformationJobObject,
            JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
        };
        let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: the job is owned and the output buffer has the API's exact size.
        let ok = unsafe {
            QueryInformationJobObject(
                self.handle(),
                JobObjectBasicAccountingInformation,
                std::ptr::from_mut(&mut info).cast(),
                u32::try_from(std::mem::size_of_val(&info)).expect("accounting size fits"),
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error());
        }
        let user = u64::try_from(info.TotalUserTime).map_err(std::io::Error::other)?;
        let kernel = u64::try_from(info.TotalKernelTime).map_err(std::io::Error::other)?;
        let ticks = user
            .checked_add(kernel)
            .ok_or_else(|| std::io::Error::other("CPU total overflow"))?;
        Ok((ticks / 10, info.ActiveProcesses))
    }

    /// Place `child`, started with [`CREATE_SUSPENDED`], in a new
    /// kill-on-close job, start its fence watcher while this process holds
    /// a fence, then resume it. A job or watcher failure leaves the child
    /// suspended for the caller to end before it can run a command.
    ///
    /// # Errors
    ///
    /// Returns the OS error when the job cannot be created, configured or
    /// joined, when the watcher cannot start or hold the fences, or when the
    /// child cannot be resumed. The caller ends the child on any failure.
    pub fn adopt(child: &Child) -> Result<Self, AdoptError> {
        let mut job = Self::for_child(child).map_err(AdoptError::Job)?;
        job.watch = super::job_watch::JobWatch::start(job.handle()).map_err(AdoptError::Watch)?;
        resume(child.id()).map_err(AdoptError::Resume)?;
        Ok(job)
    }

    fn for_child(child: &Child) -> std::io::Result<Self> {
        #[cfg(test)]
        if let Some(path) = FAIL_FOR_TEST.with(|slot| slot.borrow().clone()) {
            std::fs::write(path, child.id().to_string())?;
            return Err(std::io::Error::from_raw_os_error(5));
        }
        // SAFETY: null attributes give a default, non-inheritable handle,
        // and a null name gives an unnamed job; the handle is owned below.
        let raw = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if raw.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: `raw` is a valid job handle that nothing else owns.
        let job = Self {
            job: unsafe { OwnedHandle::from_raw_handle(raw as RawHandle) },
            watch: None,
        };
        // SAFETY: an all-zero JOBOBJECT_EXTENDED_LIMIT_INFORMATION is valid
        // (no limits); only the kill-on-close flag is set.
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let size = u32::try_from(std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>())
            .expect("the limit structure fits in u32");
        // SAFETY: the pointer and size describe `limits`, which outlives
        // the call.
        let set = unsafe {
            SetInformationJobObject(
                job.handle(),
                JobObjectExtendedLimitInformation,
                std::ptr::from_ref(&limits).cast::<c_void>(),
                size,
            )
        };
        if set == 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: both handles are valid for the duration of the call.
        if unsafe { AssignProcessToJobObject(job.handle(), child.as_raw_handle() as HANDLE) } == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(job)
    }

    /// End every process in the target's tree now and tell the watcher to
    /// exit once the job is empty.
    pub fn terminate(&self) {
        // SAFETY: the handle is valid; failure only means the tree has
        // already ended, and the caller has its own backstops.
        unsafe { TerminateJobObject(self.handle(), 1) };
        if let Some(watch) = &self.watch {
            watch.release();
        }
    }

    /// Whether the job reads empty and the watcher, if any, has exited with
    /// its fences. A count that cannot be read counts as not ended.
    pub(super) fn ended(&mut self) -> bool {
        self.accounting().is_ok_and(|(_, active)| active == 0)
            && self
                .watch
                .as_mut()
                .is_none_or(super::job_watch::JobWatch::finished)
    }

    fn handle(&self) -> HANDLE {
        self.job.as_raw_handle() as HANDLE
    }
}

/// Resume every thread of the suspended process `pid`. A process started
/// with `CREATE_SUSPENDED` has exactly one, its primary thread; std does
/// not expose that thread's handle, so it is found by enumeration.
fn resume(pid: u32) -> std::io::Result<()> {
    // SAFETY: the snapshot handle is checked, used only here and closed.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error());
    }
    let mut resumed = 0_usize;
    // SAFETY: THREADENTRY32 is plain data; dwSize is set before use.
    let mut entry: THREADENTRY32 = unsafe { std::mem::zeroed() };
    entry.dwSize =
        u32::try_from(std::mem::size_of::<THREADENTRY32>()).expect("the thread entry fits in u32");
    // SAFETY: `entry` is a valid, sized THREADENTRY32 for each call.
    let mut more = unsafe { Thread32First(snapshot, &raw mut entry) } != 0;
    while more {
        if entry.th32OwnerProcessID == pid {
            // SAFETY: the thread handle is checked and closed after use.
            let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
            if !thread.is_null() {
                // SAFETY: `thread` is a valid handle with resume access.
                if unsafe { ResumeThread(thread) } != u32::MAX {
                    resumed += 1;
                }
                // SAFETY: closing the handle opened above.
                unsafe { CloseHandle(thread) };
            }
        }
        // SAFETY: as for Thread32First.
        more = unsafe { Thread32Next(snapshot, &raw mut entry) } != 0;
    }
    // SAFETY: closing the snapshot opened above.
    unsafe { CloseHandle(snapshot) };
    if resumed == 0 {
        return Err(std::io::Error::other(format!(
            "no thread of process {pid} could be resumed"
        )));
    }
    Ok(())
}
