//! A lifetime pipe ends a heavy command's group when its controller exits.
//!
//! The watcher starts before exec and keeps copies of the run's locks. A
//! sentinel it forks stays in the group, keeping the group's id reserved,
//! while the watcher moves to a group of its own, so its own signal does
//! not end it. When the controller exits the watcher kills the group, then
//! waits until no process of the group is left, or until every member left
//! is confirmed to be a zombie, before it exits and its lock copies close.
//! So the fence frees only after no process of the command's group can run
//! (TSK-1318).
use std::io::{self, Read};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::process::Command;

#[cfg(test)]
thread_local! {
    /// Seconds a test's watcher waits before it reads the lifetime pipe,
    /// standing in for the slow descriptor loop seen with a limit of 1048576
    /// on a loaded host.
    pub(super) static SLOW_WATCHER_START: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

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
        #[cfg(test)]
        let slow_start = SLOW_WATCHER_START.with(std::cell::Cell::get);
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
                    // After the loop, so only the watcher's own exit is slow,
                    // never the spawn that waits for exec.
                    #[cfg(test)]
                    libc::sleep(slow_start);
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
                        wait_until_group_ends(group, sentinel, read_members);
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

    pub(super) fn signal(&mut self) -> io::Result<()> {
        drop(self.child_end.take());
        self.controller.set_nonblocking(true)?;
        self.controller.shutdown(std::net::Shutdown::Write)
    }

    pub(super) fn finished(&mut self) -> io::Result<bool> {
        let mut byte = [0];
        match self.controller.read(&mut byte) {
            Ok(0) => Ok(true),
            Ok(_) => Ok(false),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                Ok(false)
            }
            Err(error) => Err(error),
        }
    }
}

/// What a read of a group's members found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Members {
    /// Every member was read and is a zombie, and there is at least one.
    Zombies,
    /// A member runs, or a member or the list could not be read.
    Live,
}

/// Reads the members of a group; injected so a test can stand in for it.
type ReadMembers = unsafe fn(libc::pid_t) -> Members;

/// Whether `group` has ended: no process of it is left (`ESRCH`), or a read
/// of its process states confirms that every member is a zombie. A zombie
/// runs no code and holds no files, so it cannot use a target folder, and
/// it can outlive the group for good where PID 1 does not reap (a container
/// without an init). `kill` alone cannot tell zombies apart: it succeeds on
/// them on Linux and is refused with `EPERM` on macOS, as it is for a live
/// member owned by another user. So only a state read counts, and a member
/// whose state cannot be read counts as alive.
///
/// # Safety
///
/// Calls only async-signal-safe libc operations and `read`, which must
/// too; allocates nothing.
unsafe fn group_has_ended(group: libc::pid_t, read: ReadMembers) -> bool {
    let gone = libc::kill(-group, 0) != 0
        && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH);
    gone || read(group) == Members::Zombies
}

/// Reap `sentinel`, then wait until `group` has ended, as
/// [`group_has_ended`] decides. Unix has no notice of a group's end, so the
/// group is checked every 5 ms, and its members' states are read every
/// 100 ms.
///
/// # Safety
///
/// Runs in the watcher after `fork`: it calls only async-signal-safe libc
/// operations and allocates nothing.
unsafe fn wait_until_group_ends(group: libc::pid_t, sentinel: libc::pid_t, read: ReadMembers) {
    let mut status = 0;
    while libc::waitpid(sentinel, &raw mut status, 0) < 0
        && io::Error::last_os_error().raw_os_error() == Some(libc::EINTR)
    {}
    let pause = libc::timespec {
        tv_sec: 0,
        tv_nsec: 5_000_000,
    };
    let mut checks: u32 = 0;
    loop {
        let gone = libc::kill(-group, 0) != 0
            && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH);
        if gone || (checks % 20 == 19 && group_has_ended(group, read)) {
            return;
        }
        checks = checks.wrapping_add(1);
        libc::nanosleep(&raw const pause, std::ptr::null_mut());
    }
}

/// The fields of a Linux `/proc/<pid>/stat` line that decide whether the
/// process is a zombie of a group.
#[cfg(any(target_os = "linux", test))]
#[derive(Debug, PartialEq, Eq)]
struct ProcStat {
    state: u8,
    group: libc::pid_t,
    threads: i64,
}

#[cfg(any(target_os = "linux", test))]
impl ProcStat {
    /// A zombie whose every thread has ended. A thread-group leader that
    /// has exited while other threads run also shows `Z`, with a thread
    /// count above one, and counts as alive.
    fn is_zombie(&self) -> bool {
        self.state == b'Z' && self.threads == 1
    }
}

/// Parse a `/proc/<pid>/stat` line: state is field 3, the process group
/// field 5 and the thread count field 20. The command name, field 2, may
/// hold any byte, so the fields are read after its last `)`.
#[cfg(any(target_os = "linux", test))]
fn parse_stat(line: &[u8]) -> Option<ProcStat> {
    fn number(field: &[u8]) -> Option<i64> {
        let (negative, digits) = match field.split_first() {
            Some((b'-', rest)) => (true, rest),
            _ => (false, field),
        };
        if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
            return None;
        }
        let mut value: i64 = 0;
        for digit in digits {
            value = value
                .checked_mul(10)?
                .checked_add(i64::from(digit - b'0'))?;
        }
        Some(if negative { -value } else { value })
    }
    let close = line.iter().rposition(|byte| *byte == b')')?;
    let mut fields = line[close + 1..]
        .split(|byte| *byte == b' ' || *byte == b'\n')
        .filter(|field| !field.is_empty());
    let state = match fields.next()? {
        [state] => *state,
        _ => return None,
    };
    fields.next()?;
    let group = libc::pid_t::try_from(number(fields.next()?)?).ok()?;
    let threads = number(fields.nth(14)?)?;
    Some(ProcStat {
        state,
        group,
        threads,
    })
}

/// What a read of one `/proc/<pid>/stat` found.
#[cfg(target_os = "linux")]
enum StatRead {
    /// The process has gone since the directory was listed.
    Gone,
    Unreadable,
    Read(ProcStat),
}

/// Read `/proc/<name>/stat`, `name` being a process ID's digits.
///
/// # Safety
///
/// Async-signal-safe: raw `open`, `read` and `close`, stack buffers only.
#[cfg(target_os = "linux")]
unsafe fn read_stat(name: &[u8]) -> StatRead {
    let gone = |error: io::Error| matches!(error.raw_os_error(), Some(libc::ENOENT | libc::ESRCH));
    let mut path = [0_u8; 48];
    let (prefix, suffix) = (b"/proc/", b"/stat\0");
    if prefix.len() + name.len() + suffix.len() > path.len() {
        return StatRead::Unreadable;
    }
    path[..prefix.len()].copy_from_slice(prefix);
    path[prefix.len()..prefix.len() + name.len()].copy_from_slice(name);
    path[prefix.len() + name.len()..prefix.len() + name.len() + suffix.len()]
        .copy_from_slice(suffix);
    let file = libc::open(path.as_ptr().cast(), libc::O_RDONLY | libc::O_CLOEXEC);
    if file < 0 {
        return if gone(io::Error::last_os_error()) {
            StatRead::Gone
        } else {
            StatRead::Unreadable
        };
    }
    let mut line = [0_u8; 1024];
    let mut length = 0;
    let outcome = loop {
        let read = libc::read(
            file,
            line[length..].as_mut_ptr().cast(),
            line.len() - length,
        );
        if read == 0 || (read > 0 && length + read.unsigned_abs() == line.len()) {
            length += read.unsigned_abs();
            break None;
        }
        if read > 0 {
            length += read.unsigned_abs();
            continue;
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::EINTR) {
            break Some(if gone(error) {
                StatRead::Gone
            } else {
                StatRead::Unreadable
            });
        }
    };
    libc::close(file);
    if let Some(outcome) = outcome {
        return outcome;
    }
    parse_stat(&line[..length]).map_or(StatRead::Unreadable, StatRead::Read)
}

/// Read the state of every process in `/proc` and report whether every
/// member of `group` is a zombie. A listing or a stat that cannot be read
/// counts as a live member.
///
/// # Safety
///
/// Async-signal-safe: raw `open`, `getdents64`, `read` and `close`, stack
/// buffers only.
#[cfg(target_os = "linux")]
unsafe fn read_members(group: libc::pid_t) -> Members {
    let directory = libc::open(
        c"/proc".as_ptr(),
        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
    );
    if directory < 0 {
        return Members::Live;
    }
    let found = scan_proc(directory, group);
    libc::close(directory);
    found
}

/// The walk of `/proc` for [`read_members`].
///
/// # Safety
///
/// As [`read_members`]; `directory` is an open `/proc`.
#[cfg(target_os = "linux")]
unsafe fn scan_proc(directory: libc::c_int, group: libc::pid_t) -> Members {
    // linux_dirent64: d_ino (8), d_off (8), d_reclen (2), d_type (1), d_name.
    const NAME: usize = 19;
    // Eight-byte words keep each record aligned as the kernel writes it.
    let mut entries = [0_u64; 1024];
    let mut zombies = 0_usize;
    loop {
        let read = libc::syscall(
            libc::SYS_getdents64,
            directory,
            entries.as_mut_ptr(),
            std::mem::size_of_val(&entries),
        );
        if read < 0 {
            return Members::Live;
        }
        if read == 0 {
            break;
        }
        let Ok(read) = usize::try_from(read) else {
            return Members::Live;
        };
        // SAFETY: the kernel wrote `read` bytes into `entries`.
        let bytes = std::slice::from_raw_parts(entries.as_ptr().cast::<u8>(), read);
        let mut at = 0;
        while at < bytes.len() {
            if at + NAME > bytes.len() {
                return Members::Live;
            }
            let length = usize::from(u16::from_ne_bytes([bytes[at + 16], bytes[at + 17]]));
            if length <= NAME || at + length > bytes.len() {
                return Members::Live;
            }
            let name = &bytes[at + NAME..at + length];
            let name = &name[..name
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(name.len())];
            at += length;
            if name.is_empty() || !name.iter().all(u8::is_ascii_digit) {
                continue;
            }
            match read_stat(name) {
                StatRead::Unreadable => return Members::Live,
                StatRead::Read(stat) if stat.group == group => {
                    if !stat.is_zombie() {
                        return Members::Live;
                    }
                    zombies += 1;
                }
                // Gone since the listing, or in another group.
                StatRead::Gone | StatRead::Read(_) => {}
            }
        }
    }
    if zombies > 0 {
        Members::Zombies
    } else {
        Members::Live
    }
}

/// Read `group`'s members with `sysctl(KERN_PROC_PGRP)`, which lists
/// zombies too, and report whether every one is a zombie (`SZOMB`). The
/// `libc` crate has no `kinfo_proc`, so the record's size and field offsets
/// are those of the LP64 macOS headers; a reply of another shape counts as
/// a live member, as does a group of more than 24 processes, which does
/// not fit the buffer (`ENOMEM`) and so ends the wait only on `ESRCH`.
///
/// # Safety
///
/// Async-signal-safe: one `sysctl` call into a stack buffer.
#[cfg(target_os = "macos")]
unsafe fn read_members(group: libc::pid_t) -> Members {
    const RECORD: usize = 648;
    const STATE: usize = 36;
    const GROUP: usize = 564;
    const ZOMBIE: u8 = 5;
    let mut records = [0_u64; RECORD * 24 / 8];
    let mut length = std::mem::size_of_val(&records);
    let mut name = [libc::CTL_KERN, libc::KERN_PROC, libc::KERN_PROC_PGRP, group];
    let done = libc::sysctl(
        name.as_mut_ptr(),
        4,
        records.as_mut_ptr().cast(),
        &raw mut length,
        std::ptr::null_mut(),
        0,
    );
    if done != 0 || length == 0 || length % RECORD != 0 {
        return Members::Live;
    }
    // SAFETY: sysctl wrote `length` bytes into `records`.
    let bytes = std::slice::from_raw_parts(records.as_ptr().cast::<u8>(), length);
    let all_zombies = bytes.chunks_exact(RECORD).all(|record| {
        let member = [
            record[GROUP],
            record[GROUP + 1],
            record[GROUP + 2],
            record[GROUP + 3],
        ];
        libc::pid_t::from_ne_bytes(member) == group && record[STATE] == ZOMBIE
    });
    if all_zombies {
        Members::Zombies
    } else {
        Members::Live
    }
}

/// Elsewhere no state read is built, so only `ESRCH` ends the wait.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
unsafe fn read_members(_group: libc::pid_t) -> Members {
    Members::Live
}

impl Drop for GroupWatch {
    fn drop(&mut self) {
        // A Drop must never block the controller on a surviving descendant.
        // Explicit cleanup polls finished() under the shared cleanup deadline.
        drop(self.child_end.take());
        let _ = self.controller.shutdown(std::net::Shutdown::Write);
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
    /// stays locked until no process of the command's group can run. A
    /// member this test starts and does not reap stays in the group as a
    /// zombie after the watcher kills the group. A zombie runs nothing, so
    /// once its state confirms it, the group counts as ended and the fence
    /// frees while the zombie is still unreaped (AC-7 as amended).
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
        // for it, and its pipe reports the end only once it exits.
        watch.signal().unwrap();
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
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut finished = watch.finished().unwrap();
        while !finished && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
            finished = watch.finished().unwrap();
        }
        assert!(
            finished,
            "a group left with only a zombie still holds the fence"
        );
        drop(watch);
        // The watcher's exit closes its pipe end and its lock copies
        // together, in no set order, so the take may need a moment.
        let mut taken = older_take(&fence);
        while !taken && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
            taken = older_take(&fence);
        }
        assert!(taken, "the fence frees once only a zombie is left");
        member.wait().unwrap();
    }

    /// Start a process alone in a new group and leave it a zombie: exited
    /// and not reaped.
    fn zombie_group() -> std::process::Child {
        let child = Command::new("true").process_group(0).spawn().unwrap();
        // SAFETY: siginfo_t is plain data; waitid fills it.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        // SAFETY: waits for the child to exit without reaping it (WNOWAIT).
        let waited = unsafe {
            libc::waitid(
                libc::P_PID,
                libc::id_t::from(child.id()),
                &raw mut info,
                libc::WEXITED | libc::WNOWAIT,
            )
        };
        assert_eq!(waited, 0, "{}", std::io::Error::last_os_error());
        child
    }

    /// A group whose only member is a zombie has ended once the state read
    /// confirms it, though `kill` does not report `ESRCH`.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn a_group_of_only_zombies_has_ended() {
        let mut child = zombie_group();
        let group = i32::try_from(child.id()).unwrap();
        // SAFETY: signal 0 only checks the group.
        let probed = unsafe { libc::kill(-group, 0) };
        let esrch =
            probed != 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH);
        assert!(!esrch, "the zombie keeps the group's id");
        // SAFETY: the read uses stack buffers and raw syscalls only.
        assert_eq!(
            unsafe { super::read_members(group) },
            super::Members::Zombies
        );
        // SAFETY: as above.
        assert!(unsafe { super::group_has_ended(group, super::read_members) });
        child.wait().unwrap();
    }

    /// A member whose state cannot be confirmed keeps the group alive, so
    /// the fence stays held: an `EPERM` or a successful `kill` alone never
    /// ends the wait.
    #[test]
    fn an_unconfirmed_member_keeps_the_group_alive() {
        unsafe fn unreadable(_group: libc::pid_t) -> super::Members {
            super::Members::Live
        }
        let mut child = zombie_group();
        let group = i32::try_from(child.id()).unwrap();
        // SAFETY: the stand-in reader does nothing.
        assert!(!unsafe { super::group_has_ended(group, unreadable) });
        child.wait().unwrap();
    }

    /// A running member is read as alive.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn a_running_member_is_read_as_alive() {
        let mut child = Command::new("sleep")
            .arg("30")
            .process_group(0)
            .spawn()
            .unwrap();
        let group = i32::try_from(child.id()).unwrap();
        // SAFETY: the read uses stack buffers and raw syscalls only.
        assert!(!unsafe { super::group_has_ended(group, super::read_members) });
        child.kill().unwrap();
        child.wait().unwrap();
    }

    /// The Linux stat parser reads the state, group and thread count after
    /// the command name's last `)`, and fails closed on a short line.
    #[test]
    fn a_stat_line_parses_after_the_last_parenthesis() {
        // Fields 9 to 19, so the next field is the thread count (20).
        let fields = "0 0 0 0 0 0 0 0 0 0 0";
        let zombie = format!("42 (sleep) Z 1 40 40 0 -1 {fields} 1 0 0\n");
        let stat = super::parse_stat(zombie.as_bytes()).unwrap();
        assert_eq!((stat.state, stat.group, stat.threads), (b'Z', 40, 1));
        assert!(stat.is_zombie());
        let leader = format!("42 (sleep) Z 1 40 40 0 -1 {fields} 3 0 0\n");
        assert!(!super::parse_stat(leader.as_bytes()).unwrap().is_zombie());
        let running = format!("42 (sleep) S 1 40 40 0 -1 {fields} 1 0 0\n");
        assert!(!super::parse_stat(running.as_bytes()).unwrap().is_zombie());
        let tricky = format!("42 (a) Z 1 40 40) R 1 7 7 0 -1 {fields} 1 0 0\n");
        let stat = super::parse_stat(tricky.as_bytes()).unwrap();
        assert_eq!((stat.state, stat.group), (b'R', 7));
        assert!(super::parse_stat(b"42 (sleep) Z 1 40").is_none());
        assert!(super::parse_stat(b"42 sleep Z 1 40 40").is_none());
    }
}
