//! The fence watcher on Windows (TSK-1318).
//!
//! A Windows file lock belongs to the process that took it, so a target
//! cannot inherit the gate's fence as it does on Unix. When the gate is
//! killed the OS frees the gate's locks and closes its job handle, which
//! starts the end of the target's tree, with no order between the two, and
//! an older binary could start beside the tree while it is still ending.
//!
//! So a gate that holds fences starts a watcher for each target, outside
//! the target's job, before the target runs. The watcher is this binary run
//! with [`WATCH_ARG`]. It copies the target's job handle out of the gate,
//! takes every fence the gate holds shared for itself, and links the job to
//! a completion port. When the gate ends, however it ends, or signals the
//! release event, the watcher ends the job and waits until the job reports
//! no active process (`JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO`, with the job's
//! `ActiveProcesses` count as a backstop). Only then does it exit, and its
//! fences free. A watcher that cannot start, or cannot hold the fences,
//! stops the target before it runs.
//!
//! The gate waits for the watcher only within the target's cleanup grace, as
//! it does for the tree. A process that cannot end keeps the watcher running
//! past that: the gate reports the target's termination as unconfirmed and
//! goes on, and the watcher keeps the fences, even after the gate exits,
//! until the job is empty.
//!
//! Only a binary that names itself with [`designate`] and serves
//! [`WATCH_ARG`] through [`serve`] starts watchers; the `codeflow` binary
//! does both. A watcher that is itself killed closes its job handle, so the
//! gate's kill-on-close handle still ends the tree when the gate dies, and
//! the fence then frees with the gate as it did before TSK-1318.
//!
//! The watcher starts in a new process group with no console window, and
//! ignores console control events, so a Ctrl+C, Ctrl+Break or console
//! close that ends the gate does not end the watcher. A kill that reaches
//! the watcher itself, such as `taskkill /T` on the gate or the end of a job
//! that holds the gate (a CI runner's job, a test harness timeout), still
//! ends the watcher with the gate, and the fence then frees with the gate.

use std::ffi::{c_void, OsString};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, ExitCode, Stdio};
use std::sync::OnceLock;

use windows_sys::Win32::Foundation::{
    DuplicateHandle, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
};
use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;
use windows_sys::Win32::System::JobObjects::{
    JobObjectAssociateCompletionPortInformation, JobObjectBasicAccountingInformation,
    QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
    JOBOBJECT_ASSOCIATE_COMPLETION_PORT, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
};
use windows_sys::Win32::System::SystemServices::{
    JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO, JOB_OBJECT_QUERY, JOB_OBJECT_SET_ATTRIBUTES,
    JOB_OBJECT_TERMINATE,
};
use windows_sys::Win32::System::Threading::{
    CreateEventW, GetCurrentProcess, OpenProcess, SetEvent, WaitForMultipleObjects,
    CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW, EVENT_MODIFY_STATE, INFINITE, PROCESS_DUP_HANDLE,
    PROCESS_SYNCHRONIZE, SYNCHRONIZATION_SYNCHRONIZE,
};
use windows_sys::Win32::System::IO::{
    CreateIoCompletionPort, GetQueuedCompletionStatus, PostQueuedCompletionStatus, OVERLAPPED,
};

/// The first argument that makes this binary a fence watcher.
pub const WATCH_ARG: &str = "__codeflow-fence-watch";

/// The binary started as the fence watcher, named by [`designate`].
static WATCHER: OnceLock<PathBuf> = OnceLock::new();

#[cfg(test)]
thread_local! {
    static WATCHER_FOR_TEST: std::cell::RefCell<Option<PathBuf>> = const {
        std::cell::RefCell::new(None)
    };
}

/// The test binary and the arguments that select its stand-in watcher, for
/// every thread: the runner starts dependency targets on their own threads.
#[cfg(test)]
static STAND_IN_FOR_TEST: std::sync::Mutex<Option<(PathBuf, Vec<String>)>> =
    std::sync::Mutex::new(None);

/// Start `binary` as the fence watcher in this thread only, for a test.
#[cfg(test)]
pub(super) fn watcher_for_test(binary: Option<PathBuf>) {
    WATCHER_FOR_TEST.with(|slot| *slot.borrow_mut() = binary);
}

/// Start `binary` with `leading_args` before the watcher's own arguments as
/// the fence watcher in this whole process, for a test.
#[cfg(test)]
pub(super) fn stand_in_for_test(stand_in: Option<(PathBuf, Vec<String>)>) {
    *STAND_IN_FOR_TEST
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = stand_in;
}

/// A fence watcher that holds the fences and signals ready like the real
/// one and then never exits while the gate runs. Run by a test as this test
/// binary with the watcher's arguments; without them it returns at once.
#[cfg(test)]
pub(super) fn run_stand_in_for_test() {
    let args: Vec<String> = std::env::args().collect();
    let Some(at) = args.iter().position(|arg| arg == WATCH_ARG) else {
        return;
    };
    let gate_id: u32 = args[at + 1].parse().unwrap();
    let ready_value: usize = args[at + 4].parse().unwrap();
    // SAFETY: opens the gate, which waits for this watcher and so still runs.
    let raw = unsafe { OpenProcess(PROCESS_DUP_HANDLE | PROCESS_SYNCHRONIZE, 0, gate_id) };
    assert!(!raw.is_null(), "the stand-in could not open the gate");
    // SAFETY: `raw` is a valid handle owned here exactly once.
    let gate = unsafe { OwnedHandle::from_raw_handle(raw as RawHandle) };
    let ready = copy_from(
        gate.as_raw_handle() as HANDLE,
        ready_value,
        EVENT_MODIFY_STATE,
    )
    .unwrap();
    let _held = args[at + 5..]
        .iter()
        .map(|path| crate::testing::gate_guard::hold_fence_for_watcher(std::path::Path::new(path)))
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    // SAFETY: the event is open with EVENT_MODIFY_STATE.
    assert_ne!(unsafe { SetEvent(ready.as_raw_handle() as HANDLE) }, 0);
    let handles = [gate.as_raw_handle() as HANDLE];
    // SAFETY: the handle is open for the call. Ending with the gate leaves
    // no process behind the test.
    unsafe { WaitForMultipleObjects(1, handles.as_ptr(), 0, INFINITE) };
    std::process::exit(0);
}

/// Name the binary to start as the fence watcher: one that calls
/// [`serve`] first thing in `main`. The first name given stays.
pub fn designate(binary: PathBuf) {
    let _ = WATCHER.set(binary);
}

fn watcher() -> Option<PathBuf> {
    #[cfg(test)]
    if let Some(binary) = WATCHER_FOR_TEST.with(|slot| slot.borrow().clone()) {
        return Some(binary);
    }
    #[cfg(test)]
    if let Some((binary, _)) = STAND_IN_FOR_TEST
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
    {
        return Some(binary.clone());
    }
    WATCHER.get().cloned()
}

/// The completion key of the job's own messages.
const JOB_KEY: usize = 1;
/// The completion key posted when the gate ends or signals the release.
const END_KEY: usize = 2;
/// How often the job's active count is read while the watcher waits for
/// the job's last process, in milliseconds, should a message be missed.
const BACKSTOP_MS: u32 = 1000;

/// A running fence watcher for one target's job. Releasing or dropping it
/// signals the release, so the watcher ends the job and exits once the job
/// is empty. Neither waits: the runner polls [`Self::finished`] under the
/// target's cleanup grace, and a watcher still running past that keeps its
/// fences.
pub(super) struct JobWatch {
    release: OwnedHandle,
    child: Child,
}

impl JobWatch {
    /// Start a watcher for `job` that holds every fence this process
    /// holds, and return once it holds them. `None` when this process
    /// holds no fence.
    ///
    /// # Errors
    ///
    /// When this process holds a fence but no watcher binary is designated,
    /// when the watcher cannot be started, or when it ends before it holds
    /// the fences; the watcher is then ended.
    pub(super) fn start(job: HANDLE) -> std::io::Result<Option<Self>> {
        let fences = crate::testing::gate_guard::watched_fence_paths();
        if fences.is_empty() {
            return Ok(None);
        }
        let Some(binary) = watcher() else {
            return Err(std::io::Error::other(
                "this process holds a fence but no fence watcher binary is designated",
            ));
        };
        let release = new_event()?;
        let ready = new_event()?;
        let mut command = Command::new(&binary);
        #[cfg(test)]
        if let Some((_, leading)) = STAND_IN_FOR_TEST
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
        {
            command.args(leading);
        }
        let mut child = command
            .arg(WATCH_ARG)
            .arg(std::process::id().to_string())
            .arg((job as usize).to_string())
            .arg((release.as_raw_handle() as usize).to_string())
            .arg((ready.as_raw_handle() as usize).to_string())
            .args(&fences)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            // Outside the gate's console group and console, so the gate's
            // Ctrl+C, Ctrl+Break or console close does not reach it.
            .creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW)
            .spawn()
            .map_err(|error| {
                std::io::Error::other(format!("{} could not start: {error}", binary.display()))
            })?;
        // The watcher copies the handles out of this process, which stays
        // alive here, so its process ID cannot be reused meanwhile.
        let handles = [
            ready.as_raw_handle() as HANDLE,
            child.as_raw_handle() as HANDLE,
        ];
        // SAFETY: both handles are open for the call.
        let woke = unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, INFINITE) };
        if woke == WAIT_OBJECT_0 {
            return Ok(Some(Self { release, child }));
        }
        let failed = (woke != WAIT_OBJECT_0 + 1).then(std::io::Error::last_os_error);
        let _ = child.kill();
        let status = child.wait();
        Err(std::io::Error::other(match (failed, status) {
            (Some(error), _) => format!("the wait for it failed: {error}"),
            (None, Ok(status)) => format!(
                "it ended before it held the fence ({status}); its message, if any, is above"
            ),
            (None, Err(error)) => format!("it ended before it held the fence: {error}"),
        }))
    }

    /// Signal the release, so the watcher ends the job and exits once the
    /// job is empty. Signaling again changes nothing.
    pub(super) fn release(&self) {
        // SAFETY: the event is open; a failure leaves the gate's own exit
        // as the watcher's signal.
        unsafe { SetEvent(self.release.as_raw_handle() as HANDLE) };
    }

    /// Whether the watcher has exited, so it holds no fence. A status that
    /// cannot be read counts as still running.
    pub(super) fn finished(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(Some(_)))
    }
}

impl Drop for JobWatch {
    fn drop(&mut self) {
        // A Drop must never block the gate on a process that cannot end. The
        // watcher stays alive after this handle closes and keeps the fences.
        self.release();
    }
}

/// A new unnamed, manual-reset event, not signaled and not inheritable.
fn new_event() -> std::io::Result<OwnedHandle> {
    // SAFETY: null attributes and name give a private, non-inheritable event.
    let raw = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
    if raw.is_null() {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `raw` is a valid handle that nothing else owns.
    Ok(unsafe { OwnedHandle::from_raw_handle(raw as RawHandle) })
}

/// A copy of the handle `value` of the process `source`, owned by this
/// process, with `access` only and not inheritable.
fn copy_from(source: HANDLE, value: usize, access: u32) -> Result<OwnedHandle, String> {
    let mut copy: HANDLE = std::ptr::null_mut();
    // SAFETY: `source` is open with PROCESS_DUP_HANDLE; `copy` is written
    // only on success.
    let done = unsafe {
        DuplicateHandle(
            source,
            value as HANDLE,
            GetCurrentProcess(),
            &raw mut copy,
            access,
            0,
            0,
        )
    };
    if done == 0 {
        return Err(format!(
            "a handle could not be copied from the gate: {}",
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: the copy is a valid handle owned here exactly once.
    Ok(unsafe { OwnedHandle::from_raw_handle(copy as RawHandle) })
}

/// Run as the fence watcher when this process was started with
/// `WATCH_ARG`, returning its exit code; `None` otherwise. A binary that
/// [`designate`]s itself calls this first thing in `main`.
#[must_use]
pub fn serve() -> Option<ExitCode> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new(WATCH_ARG)) {
        return None;
    }
    // Ignore console control events, so only the gate's end or its release
    // ends the watcher. A failure leaves the new process group and the
    // missing console window as the guard.
    // SAFETY: a null handler with TRUE only sets this process to ignore
    // Ctrl+C; no callback is registered.
    unsafe { SetConsoleCtrlHandler(None, 1) };
    Some(match watch(&args.collect::<Vec<_>>()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("codeflow fence watcher: {message}");
            ExitCode::FAILURE
        }
    })
}

/// The watcher's whole life: copy the handles, hold the fences, signal
/// ready, then wait for the gate's end and the job's.
fn watch(args: &[OsString]) -> Result<(), String> {
    let number = |index: usize| -> Result<usize, String> {
        args.get(index)
            .and_then(|arg| arg.to_str())
            .and_then(|text| text.parse().ok())
            .ok_or_else(|| format!("argument {} is not a number", index + 1))
    };
    let gate_id = u32::try_from(number(0)?).map_err(|_| "the gate ID is too large")?;
    // SAFETY: opens the gate, which waits for this watcher and so still runs.
    let raw = unsafe { OpenProcess(PROCESS_DUP_HANDLE | PROCESS_SYNCHRONIZE, 0, gate_id) };
    if raw.is_null() {
        return Err(format!(
            "the gate could not be opened: {}",
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: `raw` is a valid handle owned here exactly once.
    let gate = unsafe { OwnedHandle::from_raw_handle(raw as RawHandle) };
    let source = gate.as_raw_handle() as HANDLE;
    let job = copy_from(
        source,
        number(1)?,
        JOB_OBJECT_QUERY | JOB_OBJECT_SET_ATTRIBUTES | JOB_OBJECT_TERMINATE,
    )?;
    let release = copy_from(source, number(2)?, SYNCHRONIZATION_SYNCHRONIZE)?;
    let ready = copy_from(source, number(3)?, EVENT_MODIFY_STATE)?;
    let held = args[4..]
        .iter()
        .map(|path| crate::testing::gate_guard::hold_fence_for_watcher(std::path::Path::new(path)))
        .collect::<Result<Vec<_>, _>>()?;
    // SAFETY: a new port with no file; the handle stays open until exit,
    // so the thread below may post to it.
    let port = unsafe { CreateIoCompletionPort(INVALID_HANDLE_VALUE, std::ptr::null_mut(), 0, 1) };
    if port.is_null() {
        return Err(format!(
            "its completion port could not be created: {}",
            std::io::Error::last_os_error()
        ));
    }
    let association = JOBOBJECT_ASSOCIATE_COMPLETION_PORT {
        CompletionKey: std::ptr::without_provenance_mut(JOB_KEY),
        CompletionPort: port,
    };
    let size = u32::try_from(std::mem::size_of::<JOBOBJECT_ASSOCIATE_COMPLETION_PORT>())
        .expect("the port structure fits in u32");
    // SAFETY: the pointer and size describe `association`, which outlives
    // the call.
    let linked = unsafe {
        SetInformationJobObject(
            job.as_raw_handle() as HANDLE,
            JobObjectAssociateCompletionPortInformation,
            std::ptr::from_ref(&association).cast::<c_void>(),
            size,
        )
    };
    if linked == 0 {
        return Err(format!(
            "the job could not be linked to its port: {}",
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: the event is open with EVENT_MODIFY_STATE.
    if unsafe { SetEvent(ready.as_raw_handle() as HANDLE) } == 0 {
        return Err(format!(
            "it could not signal ready: {}",
            std::io::Error::last_os_error()
        ));
    }
    let port_value = port as usize;
    std::thread::spawn(move || {
        let handles = [
            gate.as_raw_handle() as HANDLE,
            release.as_raw_handle() as HANDLE,
        ];
        // SAFETY: both handles are open while this thread owns them.
        unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, INFINITE) };
        // SAFETY: the port stays open for the life of the process.
        unsafe { PostQueuedCompletionStatus(port_value as HANDLE, 0, END_KEY, std::ptr::null()) };
    });
    let job = job.as_raw_handle() as HANDLE;
    loop {
        match next_message(port, INFINITE) {
            // The tree ended by itself: nothing is left to keep out.
            Some((JOB_KEY, JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO)) => return Ok(()),
            Some((END_KEY, _)) => break,
            Some(_) => {}
            None => std::thread::sleep(std::time::Duration::from_millis(50)),
        }
    }
    // SAFETY: the handle carries JOB_OBJECT_TERMINATE.
    unsafe { TerminateJobObject(job, 1) };
    while !job_is_empty(job) {
        match next_message(port, BACKSTOP_MS) {
            Some((JOB_KEY, JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO)) => break,
            Some(_) => {}
            // A timeout, or a failed wait: read the count again, after a
            // pause so that a wait that fails at once cannot spin.
            None => std::thread::sleep(std::time::Duration::from_millis(50)),
        }
    }
    // The tree is gone: the fences free as this process exits.
    drop(held);
    Ok(())
}

/// The next message on `port` within `wait_ms`, as its key and message.
fn next_message(port: HANDLE, wait_ms: u32) -> Option<(usize, u32)> {
    let mut message = 0_u32;
    let mut key = 0_usize;
    let mut overlapped: *mut OVERLAPPED = std::ptr::null_mut();
    // SAFETY: every out pointer is valid for the call.
    let got = unsafe {
        GetQueuedCompletionStatus(
            port,
            &raw mut message,
            &raw mut key,
            &raw mut overlapped,
            wait_ms,
        )
    };
    (got != 0).then_some((key, message))
}

/// Whether the job `job` has no active process. A count that cannot be
/// read counts as not empty, so the fences stay held.
fn job_is_empty(job: HANDLE) -> bool {
    let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
    let size = u32::try_from(std::mem::size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>())
        .expect("the accounting structure fits in u32");
    // SAFETY: the pointer and size describe `info`; the handle carries
    // JOB_OBJECT_QUERY.
    let read = unsafe {
        QueryInformationJobObject(
            job,
            JobObjectBasicAccountingInformation,
            std::ptr::from_mut(&mut info).cast::<c_void>(),
            size,
            std::ptr::null_mut(),
        )
    };
    read != 0 && info.ActiveProcesses == 0
}
