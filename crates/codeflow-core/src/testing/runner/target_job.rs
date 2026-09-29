//! A kill-on-close job object for each target on Windows (TSK-142 AC-3).
//!
//! Unix runs each target in its own process group, and the gate lock stays
//! held while that group outlives a killed gate (TSK-134). Windows has no
//! such group, so a killed gate used to leave its target running while the
//! OS freed the lock, and a second full gate could start beside it.
//!
//! Here the runner starts the target suspended, places it in a new job
//! object whose only handle this process holds, and then lets it run. The
//! job carries `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, so when this process
//! ends, however it ends, the OS closes the handle and ends every process in
//! the target's tree. Children join the job as they are created, and the
//! target cannot escape it, because the job does not allow breakaway.
//! Starting suspended closes the window in which the target could start a
//! child before it joins.

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
pub use windows_sys::Win32::System::Threading::CREATE_SUSPENDED;

/// The job holding one target's process tree. Dropping it ends any process
/// still in the tree.
pub struct TargetJob(OwnedHandle);

impl TargetJob {
    /// Place `child`, started with [`CREATE_SUSPENDED`], in a new
    /// kill-on-close job, then resume it. A job failure leaves the child
    /// suspended for the caller to end before it can run a command.
    ///
    /// # Errors
    ///
    /// Returns the OS error when the job cannot be created, configured or
    /// joined, or when the child cannot be resumed. The caller ends the
    /// child on either failure.
    pub fn adopt(child: &Child) -> Result<Self, AdoptError> {
        let job = Self::for_child(child).map_err(AdoptError::Job)?;
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
        let job = Self(unsafe { OwnedHandle::from_raw_handle(raw as RawHandle) });
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

    /// End every process in the target's tree now (the timeout path).
    pub fn terminate(&self) {
        // SAFETY: the handle is valid; failure only means the tree has
        // already ended, and the caller has its own backstops.
        unsafe { TerminateJobObject(self.handle(), 1) };
    }

    fn handle(&self) -> HANDLE {
        self.0.as_raw_handle() as HANDLE
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
