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
//! with [`WATCH_ARG`]. It takes every fence the gate holds shared for
//! itself, keeps a handle to the target's job, and links the job to a
//! completion port. When the gate ends, however it ends, or closes the
//! watcher's input, the watcher ends the job and waits until the job
//! reports no active process (`JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO`, with
//! the job's `ActiveProcesses` count as a backstop). Only then does it
//! exit, and its fences free. A watcher that cannot start, or cannot hold
//! the fences, stops the target before it runs.
//!
//! Only a binary that names itself with [`designate`] and serves
//! [`WATCH_ARG`] through [`serve`] starts watchers; the `codeflow` binary
//! does both. A watcher that is itself killed closes its job handle, so the
//! gate's kill-on-close handle still ends the tree when the gate dies, and
//! the fence then frees with the gate as it did before TSK-1318.

use std::ffi::c_void;
use std::io::{BufRead, BufReader, Write};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, ExitCode, Stdio};
use std::sync::OnceLock;

use windows_sys::Win32::Foundation::{DuplicateHandle, HANDLE, INVALID_HANDLE_VALUE};
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
    GetCurrentProcess, WaitForSingleObject, INFINITE, PROCESS_SYNCHRONIZE,
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

/// Start `binary` as the fence watcher in this thread only, for a test.
#[cfg(test)]
pub(super) fn watcher_for_test(binary: Option<PathBuf>) {
    WATCHER_FOR_TEST.with(|slot| *slot.borrow_mut() = binary);
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
    WATCHER.get().cloned()
}

/// The completion key of the job's own messages.
const JOB_KEY: usize = 1;
/// The completion key posted when the gate ends or closes the input.
const END_KEY: usize = 2;
/// How often the job's active count is read while the watcher waits for
/// the job's last process, in milliseconds, should a message be missed.
const BACKSTOP_MS: u32 = 1000;

/// A running fence watcher for one target's job. Dropping it closes the
/// watcher's input, so the watcher ends the job, and waits for the watcher
/// to exit, which it does once the job is empty.
pub(super) struct JobWatch {
    input: Option<ChildStdin>,
    child: Child,
}

impl JobWatch {
    /// Start a watcher for `job` that holds every fence this process
    /// holds, and return once it holds them. `None` when this process
    /// holds no fence, or no watcher binary is designated.
    ///
    /// # Errors
    ///
    /// When the watcher cannot be started, cannot be given the job, or
    /// cannot hold a fence; the watcher is then ended.
    pub(super) fn start(job: HANDLE) -> std::io::Result<Option<Self>> {
        let fences = crate::testing::gate_guard::watched_fence_paths();
        if fences.is_empty() {
            return Ok(None);
        }
        let Some(binary) = watcher() else {
            return Ok(None);
        };
        let mut paths = String::new();
        for fence in &fences {
            let text = fence.to_str().ok_or_else(|| {
                std::io::Error::other(format!(
                    "the fence path {} is not valid Unicode",
                    fence.display()
                ))
            })?;
            paths.push_str(text);
            paths.push('\n');
        }
        let mut child = Command::new(&binary)
            .arg(WATCH_ARG)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| {
                std::io::Error::other(format!("{} could not start: {error}", binary.display()))
            })?;
        match hand_over(&mut child, job, &paths) {
            Ok(input) => Ok(Some(Self {
                input: Some(input),
                child,
            })),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                Err(error)
            }
        }
    }
}

impl Drop for JobWatch {
    fn drop(&mut self) {
        drop(self.input.take());
        let _ = self.child.wait();
    }
}

/// Give the watcher `child` the job and this process, then the fence
/// paths, and wait until it holds the fences.
fn hand_over(child: &mut Child, job: HANDLE, paths: &str) -> std::io::Result<ChildStdin> {
    let process = child.as_raw_handle() as HANDLE;
    let job_copy = duplicate(
        job,
        process,
        JOB_OBJECT_QUERY | JOB_OBJECT_SET_ATTRIBUTES | JOB_OBJECT_TERMINATE,
    )?;
    // SAFETY: GetCurrentProcess returns a pseudo handle that needs no close;
    // its duplicate in the watcher is a real handle to this process.
    let gate_copy = duplicate(unsafe { GetCurrentProcess() }, process, PROCESS_SYNCHRONIZE)?;
    let mut input = child
        .stdin
        .take()
        .ok_or_else(|| std::io::Error::other("the watcher has no input"))?;
    write!(
        input,
        "{} {}\n{paths}\n",
        job_copy as usize, gate_copy as usize
    )?;
    input.flush()?;
    let output = child
        .stdout
        .take()
        .ok_or_else(|| std::io::Error::other("the watcher has no output"))?;
    let mut line = String::new();
    BufReader::new(output).read_line(&mut line)?;
    match line.trim_end() {
        "ready" => Ok(input),
        "" => Err(std::io::Error::other(
            "the watcher ended before it held the fence",
        )),
        message => Err(std::io::Error::other(message.to_string())),
    }
}

/// A copy of `handle`, owned by `process`, with `access` only. The copy is
/// not inheritable, so nothing the watcher starts receives it.
fn duplicate(handle: HANDLE, process: HANDLE, access: u32) -> std::io::Result<HANDLE> {
    let mut copy: HANDLE = std::ptr::null_mut();
    // SAFETY: both process handles and `handle` are valid for the call,
    // and `copy` is written only on success.
    let done = unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            handle,
            process,
            &raw mut copy,
            access,
            0,
            0,
        )
    };
    if done == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(copy)
}

/// Run as the fence watcher when this process was started with
/// [`WATCH_ARG`], returning its exit code; `None` otherwise. A binary that
/// [`designate`]s itself calls this first thing in `main`.
#[must_use]
pub fn serve() -> Option<ExitCode> {
    if std::env::args_os().nth(1).as_deref() != Some(std::ffi::OsStr::new(WATCH_ARG)) {
        return None;
    }
    Some(match watch() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            // Before `ready`, the gate reads this line as the cause.
            println!("fence watcher: {message}");
            ExitCode::FAILURE
        }
    })
}

/// The watcher's whole life: read the handles and fences, hold the
/// fences, report `ready`, then wait for the gate's end and the job's.
fn watch() -> Result<(), String> {
    let mut input = std::io::stdin().lock();
    let mut line = String::new();
    input
        .read_line(&mut line)
        .map_err(|error| format!("its input could not be read: {error}"))?;
    let mut values = line.split_whitespace().map(str::parse::<usize>);
    let (Some(Ok(job)), Some(Ok(gate)), None) = (values.next(), values.next(), values.next())
    else {
        return Err(format!("its input named no job and gate: {line:?}"));
    };
    // SAFETY: the gate duplicated both handles into this process for it
    // alone; each is owned here exactly once.
    let job = unsafe { OwnedHandle::from_raw_handle(job as RawHandle) };
    // SAFETY: as above.
    let gate = unsafe { OwnedHandle::from_raw_handle(gate as RawHandle) };
    let mut held = Vec::new();
    loop {
        line.clear();
        input
            .read_line(&mut line)
            .map_err(|error| format!("its input could not be read: {error}"))?;
        let path = line.trim_end_matches(['\r', '\n']);
        if path.is_empty() {
            break;
        }
        held.push(crate::testing::gate_guard::hold_fence_for_watcher(
            std::path::Path::new(path),
        )?);
    }
    drop(input);
    // SAFETY: a new port with no file; the handle stays open until exit,
    // so the threads below may post to it.
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
    let mut output = std::io::stdout().lock();
    writeln!(output, "ready")
        .and_then(|()| output.flush())
        .map_err(|error| format!("it could not report ready: {error}"))?;
    drop(output);
    let port_value = port as usize;
    std::thread::spawn(move || {
        // SAFETY: the handle is open while this thread owns it.
        unsafe { WaitForSingleObject(gate.as_raw_handle() as HANDLE, INFINITE) };
        post_end(port_value);
    });
    std::thread::spawn(move || {
        // The gate closes the input when its target ends, or as it exits.
        let _ = std::io::copy(&mut std::io::stdin(), &mut std::io::sink());
        post_end(port_value);
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

/// Post the end of the gate, or of its input, to the port `port`.
fn post_end(port: usize) {
    // SAFETY: the port stays open for the life of the process.
    unsafe { PostQueuedCompletionStatus(port as HANDLE, 0, END_KEY, std::ptr::null()) };
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
