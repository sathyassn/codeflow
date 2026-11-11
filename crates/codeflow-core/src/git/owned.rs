//! A child process group that cannot outlive its owner (TSK-327).
//!
//! The rule: every process a hook, guard, CI or doctor path starts has a
//! living owner until it has ended. An owner signals the whole group before
//! it reaps the group's leader, on every completion path. When an owner can
//! itself be killed by an outer deadline, cleanup of what it started belongs
//! to something that survives that kill.
//!
//! [`GroupOwner::prepare`] makes the command lead a new process group and
//! starts, inside that group, a watcher: `/bin/sh` reading one end of a
//! socket pair whose other end only the owning process holds. When the
//! owning process ends for any reason, SIGKILL included, the kernel closes
//! its end, the watcher reads end of file and kills its own group. Because
//! the watcher is a member, the group id stays reserved while anything in
//! the group lives, so neither the watcher's signal nor the owner's
//! [`GroupOwner::end`] can reach a reused id. This is the lifetime-pipe
//! pattern the test runner already uses for heavy targets
//! (`testing/runner/group_watch.rs`); the watcher here execs a shell, so the
//! exec closes every inherited descriptor at once instead of a loop over
//! the descriptor limit, which matters for reads that run many times a
//! hook.
//!
//! A process that leaves the group itself (`setsid`, or `setpgid` to a new
//! group, as an ssh `ControlPersist` master does) is out of reach of any
//! group-based design. Windows has no group here: [`GroupOwner::end`]
//! kills the child only, and its descendants are not ended.

use std::process::{Child, Command};

/// The owner of one child's process group; see the module docs.
pub struct GroupOwner {
    #[cfg(unix)]
    controller: Option<std::os::unix::net::UnixStream>,
    #[cfg(unix)]
    child_end: Option<std::os::unix::net::UnixStream>,
    group: Option<i32>,
}

impl GroupOwner {
    /// Make `command` lead its own process group, with a watcher that ends
    /// the group when this owner's process ends. A failure to start the
    /// watcher fails the spawn, so no child runs unowned.
    ///
    /// # Errors
    ///
    /// The socket pair could not be created.
    pub fn prepare(command: &mut Command) -> std::io::Result<Self> {
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd as _;
            use std::os::unix::process::CommandExt as _;
            // Both ends are close-on-exec, so no exec'd program keeps one.
            let (controller, child_end) = std::os::unix::net::UnixStream::pair()?;
            let reader = child_end.as_raw_fd();
            let shell = c"/bin/sh";
            let name = c"sh";
            let flag = c"-c";
            let script = c"while read -r _; do :; done; kill -9 0";
            let null = c"/dev/null";
            // SAFETY: after fork only async-signal-safe calls run: setpgid,
            // fork, waitpid, dup2, open, execve, kill and _exit. The argument
            // array lives on the stack; nothing allocates or takes a lock.
            unsafe {
                command.pre_exec(move || {
                    if libc::setpgid(0, 0) != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    let intermediate = libc::fork();
                    if intermediate < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    if intermediate == 0 {
                        // Detach the watcher from the command's children, so
                        // a command that waits for all of them never waits
                        // for it.
                        let watcher = libc::fork();
                        if watcher != 0 {
                            libc::_exit(i32::from(watcher < 0));
                        }
                        let quiet = libc::open(null.as_ptr(), libc::O_RDWR);
                        if libc::dup2(reader, 0) < 0
                            || quiet < 0
                            || libc::dup2(quiet, 1) < 0
                            || libc::dup2(quiet, 2) < 0
                        {
                            libc::kill(0, libc::SIGKILL);
                            libc::_exit(1);
                        }
                        let argv = [
                            name.as_ptr(),
                            flag.as_ptr(),
                            script.as_ptr(),
                            std::ptr::null(),
                        ];
                        let envp = [std::ptr::null::<libc::c_char>()];
                        libc::execve(shell.as_ptr(), argv.as_ptr(), envp.as_ptr());
                        // No watcher: end the group now, so the command fails
                        // instead of running unowned.
                        libc::kill(0, libc::SIGKILL);
                        libc::_exit(1);
                    }
                    let mut status = 0;
                    while libc::waitpid(intermediate, &raw mut status, 0) < 0 {
                        if std::io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
                            return Err(std::io::Error::last_os_error());
                        }
                    }
                    if !libc::WIFEXITED(status) || libc::WEXITSTATUS(status) != 0 {
                        return Err(std::io::Error::from_raw_os_error(libc::EIO));
                    }
                    Ok(())
                });
            }
            Ok(Self {
                controller: Some(controller),
                child_end: Some(child_end),
                group: None,
            })
        }
        #[cfg(not(unix))]
        {
            let _ = command;
            Ok(Self { group: None })
        }
    }

    /// Record the spawned child, whose pid is its group's id, and drop this
    /// process's copy of the watcher's end.
    pub fn spawned(&mut self, child: &Child) {
        #[cfg(unix)]
        drop(self.child_end.take());
        self.group = i32::try_from(child.id()).ok();
    }

    /// SIGKILL every process left in the group, the watcher included,
    /// without reaping the child. Once only: after it the group may empty
    /// and its id be reused, so a second call does nothing.
    pub fn signal(&mut self) {
        #[cfg(unix)]
        if let Some(group) = self.group.take() {
            // SAFETY: the watcher is a member until this signal, so `group`
            // is still this group's id; SIGKILL to it is best-effort.
            unsafe {
                libc::killpg(group, libc::SIGKILL);
            }
        }
        #[cfg(not(unix))]
        {
            self.group = None;
        }
    }

    /// End the whole group, then kill and reap the child.
    pub fn end(&mut self, child: &mut Child) {
        self.signal();
        let _ = child.kill();
        let _ = child.wait();
    }
}

/// Run a local command to completion as [`Command::output`] does, with
/// `input` on its stdin when given, as an owned group: it waits for the
/// child's exit without reaping it, ends what is left of its group, then
/// reaps it and returns its output. For a short local read whose caller can
/// exit on its own deadline, such as a session guard (TSK-327); it adds no
/// deadline of its own.
///
/// # Errors
///
/// The child could not be started, waited for or read.
pub fn owned_output(
    command: &mut Command,
    input: Option<&[u8]>,
) -> std::io::Result<std::process::Output> {
    use std::io::Read as _;
    use std::process::Stdio;
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut owner = GroupOwner::prepare(command)?;
    let (mut child, writer) = match input {
        Some(input) => {
            let (child, writer) =
                crate::git::spawn_with_input_stopping(command, input.to_vec(), |child| {
                    owner.spawned(child);
                    owner.signal();
                    let _ = child.kill();
                })?;
            (child, Some(writer))
        }
        None => (command.stdin(Stdio::null()).spawn()?, None),
    };
    owner.spawned(&child);
    let pipe = |pipe: Option<Box<dyn std::io::Read + Send>>| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            if let Some(mut pipe) = pipe {
                pipe.read_to_end(&mut bytes)?;
            }
            Ok::<_, std::io::Error>(bytes)
        })
    };
    let stdout = pipe(
        child
            .stdout
            .take()
            .map(|out| Box::new(out) as Box<dyn std::io::Read + Send>),
    );
    let stderr = pipe(
        child
            .stderr
            .take()
            .map(|err| Box::new(err) as Box<dyn std::io::Read + Send>),
    );
    let exited = wait_unreaped(&mut child);
    // The group ends before the child is reaped, which also closes any pipe
    // a descendant held, so the readers below finish.
    owner.signal();
    let _ = child.kill();
    let status = child.wait();
    exited?;
    let status = status?;
    let joined = |reader: std::thread::JoinHandle<std::io::Result<Vec<u8>>>| {
        reader
            .join()
            .unwrap_or_else(|_| Err(std::io::Error::other("an output reader panicked")))
    };
    let stdout = joined(stdout)?;
    let stderr = joined(stderr)?;
    if let Some(writer) = writer {
        let _ = writer.finish_within(std::time::Duration::from_millis(250));
    }
    Ok(std::process::Output {
        status,
        stdout,
        stderr,
    })
}

/// Block until `child` exits, without reaping it on Unix.
fn wait_unreaped(child: &mut Child) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let pid: libc::id_t = child.id();
        loop {
            // SAFETY: a zeroed `siginfo_t` is a valid value for `waitid` to
            // fill; `WNOWAIT` leaves the child waitable for the later wait.
            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
            let done = unsafe {
                libc::waitid(
                    libc::P_PID,
                    pid,
                    &raw mut info,
                    libc::WEXITED | libc::WNOWAIT,
                )
            };
            if done == 0 {
                return Ok(());
            }
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
    }
    #[cfg(not(unix))]
    {
        child.wait().map(drop)
    }
}

impl Drop for GroupOwner {
    /// An owner dropped without [`GroupOwner::end`] still ends its group:
    /// closing the controller is the watcher's end of file.
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            drop(self.child_end.take());
            drop(self.controller.take());
        }
    }
}
