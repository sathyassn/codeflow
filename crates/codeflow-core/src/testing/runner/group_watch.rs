//! A lifetime pipe ends a heavy command's group when its controller exits.
//! The watcher starts before exec and stays in the group, keeping its id reserved.
//!
//! A *keeper* (TSK-1256) is the same process for a target that may outlive
//! its gate: it holds copies of the gate's locks, the fence that keeps older
//! binaries out included, while the target's group lives, and never ends the
//! group. It leaves the group, so the group ends when the target's own
//! processes do, and it exits then, or at once when the controller releases
//! it after a normal run.
use std::io::{self, Read};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::process::Command;

pub(super) struct GroupWatch {
    controller: UnixStream,
    child_end: Option<UnixStream>,
    locks: Vec<std::fs::File>,
    /// A keeper: released with [`RELEASE`] instead of ending the group.
    keep: bool,
}

/// The byte a controller sends a keeper when its target ended normally, so
/// the keeper drops its lock copies at once. A keeper whose pipe closes
/// without it holds them until the target's group is gone.
const RELEASE: u8 = b'r';

/// How often a keeper checks whether the target's group still has a process.
const KEEPER_POLL_MS: libc::c_int = 200;

impl GroupWatch {
    /// With `hand_over`, the command itself also keeps the lock copies
    /// across exec and finds their numbers in `RESERVED_LOCKS_ENV`, so a
    /// nested `codeflow` run can use the reservation (TSK-1128). With `keep`
    /// the process is a keeper (see the module docs) and never ends the group.
    pub(super) fn prepare(command: &mut Command, hand_over: bool, keep: bool) -> io::Result<Self> {
        // SAFETY: sysconf reads the descriptor limit and changes no process state.
        let limit = i32::try_from(unsafe { libc::sysconf(libc::_SC_OPEN_MAX) })
            .ok()
            .filter(|limit| *limit > 0)
            .ok_or_else(|| io::Error::other("cannot read the open descriptor limit"))?;
        // Rust creates both endpoints close-on-exec. Other execs cannot retain them.
        let (controller, child_end) = UnixStream::pair()?;
        let locks = crate::testing::gate_guard::inherited_gate_locks()?;
        let lock_descriptors: Vec<_> = locks.iter().map(AsRawFd::as_raw_fd).collect();
        if hand_over && !lock_descriptors.is_empty() {
            let names: Vec<_> = lock_descriptors.iter().map(ToString::to_string).collect();
            command.env(
                crate::testing::gate_guard::RESERVED_LOCKS_ENV,
                names.join(","),
            );
        }
        let writer = controller.as_raw_fd();
        let reader = child_end.as_raw_fd();
        // SAFETY: the closure uses only async-signal-safe libc operations and
        // constructs errors from errno. No allocation, locks or Rust I/O run after fork.
        unsafe {
            command.pre_exec(move || {
                let group = libc::getpid();
                if group <= 1 || libc::setpgid(0, 0) != 0 {
                    return Err(io::Error::from_raw_os_error(libc::EINVAL));
                }
                let intermediate = libc::fork();
                if intermediate < 0 {
                    return Err(io::Error::last_os_error());
                }
                if intermediate == 0 {
                    // Detach the watcher from the command's child list. A command
                    // that waits for all its children must not wait for its watcher.
                    let watcher = libc::fork();
                    if watcher < 0 {
                        libc::_exit(1);
                    }
                    if watcher > 0 {
                        libc::_exit(0);
                    }
                    for fd in 0..limit {
                        if fd != reader && !lock_descriptors.contains(&fd) {
                            libc::close(fd);
                        }
                    }
                    let mut byte = 0_u8;
                    if keep {
                        // Leave the group, so it ends with the target's own
                        // processes, and hold the lock copies until it does.
                        if libc::setpgid(0, 0) != 0 {
                            libc::_exit(1);
                        }
                        let mut open = true;
                        loop {
                            if open {
                                let mut poll = libc::pollfd {
                                    fd: reader,
                                    events: libc::POLLIN,
                                    revents: 0,
                                };
                                if libc::poll(&raw mut poll, 1, KEEPER_POLL_MS) > 0 {
                                    match libc::read(reader, (&raw mut byte).cast(), 1) {
                                        1 if byte == RELEASE => libc::_exit(0),
                                        // The controller died: keep holding.
                                        0 => open = false,
                                        _ => {}
                                    }
                                }
                            } else {
                                libc::poll(std::ptr::null_mut(), 0, KEEPER_POLL_MS);
                            }
                            if libc::kill(-group, 0) != 0
                                && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
                            {
                                libc::_exit(0);
                            }
                        }
                    }
                    loop {
                        if libc::read(reader, (&raw mut byte).cast(), 1) == 0 {
                            break;
                        }
                    }
                    // The watcher itself keeps this group id alive, so it cannot
                    // be reused between command exit and this signal. It ends too.
                    libc::kill(-group, libc::SIGKILL);
                    libc::_exit(0);
                }
                let mut status = 0;
                loop {
                    if libc::waitpid(intermediate, &raw mut status, 0) >= 0 {
                        break;
                    }
                    if io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
                        return Err(io::Error::last_os_error());
                    }
                }
                if !libc::WIFEXITED(status) || libc::WEXITSTATUS(status) != 0 {
                    return Err(io::Error::from_raw_os_error(libc::EIO));
                }
                libc::close(reader);
                libc::close(writer);
                if hand_over {
                    // Only this command keeps the copies across exec; the
                    // receiving `codeflow` makes them close-on-exec again.
                    for &fd in &lock_descriptors {
                        let flags = libc::fcntl(fd, libc::F_GETFD);
                        if flags < 0
                            || libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) != 0
                        {
                            return Err(io::Error::last_os_error());
                        }
                    }
                }
                Ok(())
            });
        }
        Ok(Self {
            controller,
            child_end: Some(child_end),
            locks,
            keep,
        })
    }

    pub(super) fn spawned(&mut self) {
        // The watcher retains its copies until it ends the group. This covers
        // controller death before the runner can record the target's group.
        self.locks.clear();
        drop(self.child_end.take());
    }
}

impl Drop for GroupWatch {
    fn drop(&mut self) {
        // Close our spare child endpoint even when spawn failed. Half-close the
        // lifetime pipe, then wait for the watcher to close its end as it exits.
        drop(self.child_end.take());
        if self.keep {
            // The target ended under this controller, which still holds the
            // locks itself: the keeper lets go of its copies now.
            let _ = std::io::Write::write_all(&mut self.controller, &[RELEASE]);
        }
        let _ = self.controller.shutdown(std::net::Shutdown::Write);
        let mut byte = [0];
        loop {
            match self.controller.read(&mut byte) {
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                _ => break,
            }
        }
    }
}
