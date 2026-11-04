//! A lifetime pipe ends a heavy command's group when its controller exits.
//!
//! The watcher starts before exec and keeps copies of the run's locks. A
//! sentinel it forks stays in the group, keeping the group's id reserved,
//! while the watcher moves to a group of its own, so its own signal does
//! not end it. When the controller exits the watcher kills the group, then
//! waits until no process of the group is left, zombies included, before
//! it exits and its lock copies close. So the fence frees only after the
//! command's whole group is gone (TSK-1318).
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
    /// With `hand_over`, the command itself also keeps the lock copies
    /// across exec and finds their numbers in `RESERVED_LOCKS_ENV`, so a
    /// nested `codeflow` run can use the reservation (TSK-1128).
    pub(super) fn prepare(command: &mut Command, hand_over: bool) -> io::Result<Self> {
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
                    // The sentinel keeps this group id alive, so it cannot be
                    // reused between command exit and the signal below. It holds
                    // no descriptor and only waits to be killed.
                    let sentinel = libc::fork();
                    if sentinel == 0 {
                        libc::close(reader);
                        for &fd in &lock_descriptors {
                            libc::close(fd);
                        }
                        loop {
                            libc::pause();
                        }
                    }
                    // Without a sentinel, or when the watcher cannot leave the
                    // group, it stays in the group and its signal ends it too,
                    // as before TSK-1318.
                    let waits = sentinel > 0 && libc::setpgid(0, 0) == 0;
                    let mut byte = 0_u8;
                    loop {
                        if libc::read(reader, (&raw mut byte).cast(), 1) == 0 {
                            break;
                        }
                    }
                    libc::kill(-group, libc::SIGKILL);
                    if waits {
                        wait_until_group_ends(group, sentinel);
                    }
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
        })
    }

    pub(super) fn spawned(&mut self) {
        // The watcher retains its copies until it ends the group. This covers
        // controller death before the runner can record the target's group.
        self.locks.clear();
        drop(self.child_end.take());
    }
}

/// Reap `sentinel`, then wait until no process of `group` is left. A group
/// whose only members are zombies still counts: `kill` then succeeds on
/// Linux and is refused with `EPERM` on macOS, and only `ESRCH` means the
/// group is empty. Unix has no notice of a group's end, so the group is
/// checked every 5 ms.
///
/// # Safety
///
/// Runs in the watcher after `fork`: it calls only async-signal-safe libc
/// operations and allocates nothing.
unsafe fn wait_until_group_ends(group: libc::pid_t, sentinel: libc::pid_t) {
    let mut status = 0;
    while libc::waitpid(sentinel, &raw mut status, 0) < 0
        && io::Error::last_os_error().raw_os_error() == Some(libc::EINTR)
    {}
    let pause = libc::timespec {
        tv_sec: 0,
        tv_nsec: 5_000_000,
    };
    while libc::kill(-group, 0) == 0
        || io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
    {
        libc::nanosleep(&raw const pause, std::ptr::null_mut());
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

#[cfg(test)]
mod tests {
    use super::GroupWatch;
    use fs2::FileExt;
    use std::os::unix::process::CommandExt;
    use std::process::Command;
    use std::time::Duration;

    /// Whether an older binary's exclusive take of the fence at `path`
    /// would succeed now; a take that succeeds is released at once.
    fn older_take(path: &std::path::Path) -> bool {
        let file = std::fs::File::open(path).unwrap();
        let taken = file.try_lock_exclusive().is_ok();
        if taken {
            file.unlock().unwrap();
        }
        taken
    }

    /// TSK-1318: when the controller ends, the watcher's copy of the fence
    /// stays locked until no process of the command's group is left. A
    /// member this test starts and does not reap stays in the group as a
    /// zombie after the watcher kills the group, so the fence must stay
    /// held until the test reaps it. Before TSK-1318 the watcher ended by
    /// its own signal and the fence freed while that member was left.
    #[test]
    fn the_fence_frees_only_after_the_whole_group_has_ended() {
        let home = tempfile::tempdir().unwrap();
        let locks = home.path().join("locks");
        std::fs::create_dir_all(&locks).unwrap();
        let lock = crate::testing::gate_guard::acquire_gate_lock(
            std::slice::from_ref(&locks),
            home.path(),
            "quick",
        )
        .unwrap();
        let fence = locks.join(crate::testing::gate_guard::LOCK_FILE);
        let mut command = Command::new("sleep");
        command.arg("30").process_group(0);
        let mut watch = GroupWatch::prepare(&mut command, false).unwrap();
        let mut leader = command.spawn().unwrap();
        watch.spawned();
        let group = i32::try_from(leader.id()).unwrap();
        let mut member = Command::new("sleep")
            .arg("30")
            .process_group(group)
            .spawn()
            .unwrap();
        // The gate's own fence closes, as when the gate is killed: only the
        // watcher's copy keeps older binaries out from here.
        drop(lock);
        assert!(!older_take(&fence), "the watcher keeps the fence");
        // The controller ends; the watcher kills the group and then waits
        // for it, so this drop returns only once the group is empty.
        let controller = std::thread::spawn(move || drop(watch));
        leader.wait().unwrap();
        // Wait for the member's death without reaping it: it stays a zombie.
        // SAFETY: siginfo_t is plain data; waitid fills it.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let member_pid = libc::id_t::from(member.id());
        // SAFETY: waits for the member to exit without reaping it (WNOWAIT).
        let waited = unsafe {
            libc::waitid(
                libc::P_PID,
                member_pid,
                &raw mut info,
                libc::WEXITED | libc::WNOWAIT,
            )
        };
        assert_eq!(waited, 0, "{}", std::io::Error::last_os_error());
        std::thread::sleep(Duration::from_millis(300));
        assert!(
            !older_take(&fence),
            "the fence freed while a process of the killed group was left"
        );
        assert!(!controller.is_finished(), "the watcher ended early");
        member.wait().unwrap();
        controller.join().unwrap();
        assert!(older_take(&fence), "the fence frees once the group is gone");
    }
}
