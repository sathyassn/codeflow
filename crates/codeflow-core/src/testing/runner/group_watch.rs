//! A lifetime pipe ends a heavy command's group when its controller exits.
//! The watcher starts before exec and stays in the group, keeping its id reserved.
use std::io::{self, Read};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::process::Command;

pub(super) struct GroupWatch {
    controller: UnixStream,
    child_end: Option<UnixStream>,
    locks: Vec<std::fs::File>,
}

impl GroupWatch {
    pub(super) fn prepare(command: &mut Command) -> io::Result<Self> {
        // SAFETY: sysconf reads the descriptor limit and changes no process state.
        let limit = i32::try_from(unsafe { libc::sysconf(libc::_SC_OPEN_MAX) })
            .ok()
            .filter(|limit| *limit > 0)
            .ok_or_else(|| io::Error::other("cannot read the open descriptor limit"))?;
        // Rust creates both endpoints close-on-exec. Other execs cannot retain them.
        let (controller, child_end) = UnixStream::pair()?;
        let locks = crate::testing::gate_guard::inherited_gate_locks()?;
        let lock_descriptors: Vec<_> = locks.iter().map(AsRawFd::as_raw_fd).collect();
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
                Ok(())
            });
        }
        Ok(Self {
            controller,
            child_end: Some(child_end),
            locks,
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
