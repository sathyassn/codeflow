//! Linux CPU accounting in an already delegated subtree; never provisions delegation.
use std::ffi::CString;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{FileExt, MetadataExt};
use std::os::unix::process::CommandExt;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

struct Directory(File);

impl Directory {
    fn at(&self, name: &str, flags: i32) -> io::Result<File> {
        let name = CString::new(name).map_err(io::Error::other)?;
        // SAFETY: the parent is held open; the name is NUL-terminated. A leaf
        // link is never followed and the new descriptor is close-on-exec.
        let fd = unsafe {
            libc::openat(
                self.0.as_raw_fd(),
                name.as_ptr(),
                flags | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    fn open(path: &Path) -> io::Result<Self> {
        let mut dir = Self(File::open("/")?);
        for part in path.components() {
            match part {
                Component::RootDir => {}
                Component::Normal(name) => {
                    dir = Self(
                        dir.at(
                            name.to_str()
                                .ok_or_else(|| io::Error::other("non-UTF8 cgroup path"))?,
                            libc::O_RDONLY | libc::O_DIRECTORY,
                        )?,
                    );
                }
                _ => return Err(io::Error::other("unsafe cgroup path")),
            }
        }
        Ok(dir)
    }

    fn text(&self, name: &str) -> io::Result<String> {
        let mut text = String::new();
        self.at(name, libc::O_RDONLY)?
            .take(16_385)
            .read_to_string(&mut text)?;
        if text.len() > 16_384 {
            return Err(io::Error::other("cgroup counter too large"));
        }
        Ok(text)
    }
}

/// Exit-sensitive observations are provisional until the child is re-checked.
/// Positive path or identity mismatches remain evidence even after exit.
#[derive(Debug)]
pub(super) struct CgroupError {
    pub error: io::Error,
    pub membership_uncertain: bool,
    pub may_be_exit: bool,
}

impl CgroupError {
    fn membership(error: io::Error) -> Self {
        Self {
            error,
            membership_uncertain: true,
            may_be_exit: false,
        }
    }

    fn exit_observation(error: io::Error) -> Self {
        Self {
            error,
            membership_uncertain: true,
            may_be_exit: true,
        }
    }

    fn io(error: io::Error) -> Self {
        Self {
            error,
            membership_uncertain: false,
            may_be_exit: false,
        }
    }
}

impl std::fmt::Display for CgroupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.error, f)
    }
}

impl From<CgroupError> for io::Error {
    fn from(error: CgroupError) -> Self {
        error.error
    }
}

pub(super) struct Cgroup {
    parent: Directory,
    directory: Directory,
    name: String,
    pub path: PathBuf,
    launched: bool,
    events: File,
    membership: PathBuf,
}

pub(super) fn prepare(
    command: &mut Command,
    deadline: &mut super::deadline::Deadline,
) -> io::Result<Option<Cgroup>> {
    configure(Cgroup::discover(), command, deadline)
}

fn configure(
    group: io::Result<Cgroup>,
    command: &mut Command,
    deadline: &mut super::deadline::Deadline,
) -> io::Result<Option<Cgroup>> {
    match group {
        Ok(group) => {
            group.prepare(command)?;
            deadline.info.cgroup = Some(group.path.display().to_string());
            deadline.info.cpu_source = Some("cgroup v2 cpu.stat usage_usec".into());
            deadline.info.cgroup_identity = Some(group.identity()?);
            Ok(Some(group))
        }
        Err(error) => {
            deadline.unavailable(format!("no delegated cgroup ({error})"));
            Ok(None)
        }
    }
}

impl Cgroup {
    pub fn discover() -> io::Result<Self> {
        let membership = kernel_text("/proc/self/cgroup")?;
        let current = membership
            .split('\n')
            .find_map(|line| line.strip_prefix("0::"))
            .ok_or_else(|| io::Error::other("no cgroup v2 membership"))?;
        if current == "/" {
            return Err(io::Error::other(
                "cgroup root is not an explicit delegated subtree",
            ));
        }
        let mounts = kernel_text("/proc/self/mountinfo")?;
        let path = mounts
            .split('\n')
            .find_map(|line| {
                let (left, right) = line.split_once(" - ")?;
                if right.split_ascii_whitespace().next()? != "cgroup2" {
                    return None;
                }
                let fields: Vec<_> = left.split_ascii_whitespace().collect();
                let root = unescape(fields.get(3)?);
                let mount = unescape(fields.get(4)?);
                let relative = Path::new(current).strip_prefix(&root).ok()?;
                Some(PathBuf::from(mount).join(relative))
            })
            .ok_or_else(|| io::Error::other("no delegated cgroup v2 mount"))?;
        let mut group = Self::create(&path)?;
        group.membership = Path::new(current).join(&group.name);
        Ok(group)
    }

    fn create(parent_path: &Path) -> io::Result<Self> {
        let parent = Directory::open(parent_path)?;
        // Refuse ordinary directories or a v1 hierarchy, even in tests/config.
        let mut kind = std::mem::MaybeUninit::<libc::statfs>::uninit();
        if unsafe { libc::fstatfs(parent.0.as_raw_fd(), kind.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        if unsafe { kind.assume_init() }.f_type != libc::CGROUP2_SUPER_MAGIC {
            return Err(io::Error::other("not a cgroup v2 filesystem"));
        }
        let name = format!("codeflow-{}", ulid::Ulid::new());
        let c_name = CString::new(name.as_str()).map_err(io::Error::other)?;
        if unsafe { libc::mkdirat(parent.0.as_raw_fd(), c_name.as_ptr(), 0o700) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let directory = match parent.at(&name, libc::O_RDONLY | libc::O_DIRECTORY) {
            Ok(file) => Directory(file),
            Err(error) => {
                // No workload was launched. Only remove the empty name we created.
                unsafe {
                    libc::unlinkat(parent.0.as_raw_fd(), c_name.as_ptr(), libc::AT_REMOVEDIR);
                }
                return Err(error);
            }
        };
        let events = match directory.at("cgroup.events", libc::O_RDONLY) {
            Ok(events) => events,
            Err(error) => {
                unsafe {
                    libc::unlinkat(parent.0.as_raw_fd(), c_name.as_ptr(), libc::AT_REMOVEDIR);
                }
                return Err(error);
            }
        };
        Ok(Self {
            events,
            membership: PathBuf::new(),
            path: parent_path.join(&name),
            parent,
            directory,
            name,
            launched: false,
        })
    }

    pub fn mark_launched(&mut self, pid: u32) -> Result<(), CgroupError> {
        self.launched = true;
        // Acknowledge the runner's own populated transition. Exit-sensitive
        // errors remain provisional until the controller re-checks its child.
        self.verify().map_err(CgroupError::membership)?;
        let populated = self.read_events().map_err(CgroupError::exit_observation)?;
        validate_population(false, populated).map_err(CgroupError::exit_observation)?;
        self.check_membership(pid)
    }

    fn read_events(&self) -> io::Result<bool> {
        let mut bytes = [0_u8; 1024];
        let count = self.events.read_at(&mut bytes, 0)?;
        if count == bytes.len() {
            return Err(io::Error::other("cgroup events too large"));
        }
        let text = std::str::from_utf8(&bytes[..count]).map_err(io::Error::other)?;
        match counter(text, "populated")? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(io::Error::other("invalid cgroup populated value")),
        }
    }

    fn population_changed(&self) -> io::Result<bool> {
        let mut fd = libc::pollfd {
            fd: self.events.as_raw_fd(),
            events: libc::POLLPRI,
            revents: 0,
        };
        // kernfs retains a notification until the held events descriptor is
        // read, including an empty-to-populated transition between samples.
        // SAFETY: fd describes one live descriptor; poll only writes its revents.
        if unsafe { libc::poll(&raw mut fd, 1, 0) } < 0 {
            return Err(io::Error::last_os_error());
        }
        if fd.revents & (libc::POLLNVAL | libc::POLLHUP) != 0 {
            return Err(io::Error::other("cgroup population observation lost"));
        }
        Ok(fd.revents & (libc::POLLPRI | libc::POLLERR) != 0)
    }

    fn check_membership(&self, pid: u32) -> Result<(), CgroupError> {
        let text =
            kernel_text(&format!("/proc/{pid}/cgroup")).map_err(CgroupError::exit_observation)?;
        let actual = text.split('\n').find_map(|line| line.strip_prefix("0::"));
        membership_observation(&self.membership, actual)
    }

    pub fn sample(&self, pid: u32) -> Result<u64, CgroupError> {
        self.verify().map_err(CgroupError::membership)?;
        let changed = self.population_changed().map_err(CgroupError::membership)?;
        let populated = self.read_events().map_err(CgroupError::exit_observation)?;
        validate_population(changed, populated).map_err(CgroupError::exit_observation)?;
        self.check_membership(pid)?;
        self.cpu()
    }

    pub fn identity(&self) -> io::Result<(u64, u64)> {
        let metadata = self.directory.0.metadata()?;
        Ok((metadata.dev(), metadata.ino()))
    }

    fn verify(&self) -> io::Result<()> {
        let named = self
            .parent
            .at(&self.name, libc::O_RDONLY | libc::O_DIRECTORY)?
            .metadata()?;
        let held = self.directory.0.metadata()?;
        if (named.dev(), named.ino()) != (held.dev(), held.ino()) {
            return Err(io::Error::other("cgroup identity changed"));
        }
        Ok(())
    }

    /// Runs after the watcher pre-exec hook. Only the inert launch child moves;
    /// successful enrollment precedes exec and every workload fork.
    pub fn prepare(&self, command: &mut Command) -> io::Result<()> {
        self.verify()?;
        let procs = self.directory.at("cgroup.procs", libc::O_WRONLY)?;
        unsafe {
            command.pre_exec(move || {
                // Async-signal-safe, allocation-free write of this child's PID (0).
                if libc::write(procs.as_raw_fd(), b"0".as_ptr().cast(), 1) != 1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        Ok(())
    }

    fn cpu(&self) -> Result<u64, CgroupError> {
        self.verify().map_err(CgroupError::membership)?;
        let text = self.directory.text("cpu.stat").map_err(CgroupError::io)?;
        counter(&text, "usage_usec").map_err(CgroupError::io)
    }

    pub fn populated(&self) -> io::Result<bool> {
        self.verify()?;
        match counter(&self.directory.text("cgroup.events")?, "populated")? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(io::Error::other("invalid cgroup populated value")),
        }
    }

    pub fn kill(&self) -> Result<(), CgroupError> {
        self.verify().map_err(CgroupError::membership)?;
        self.directory
            .at("cgroup.kill", libc::O_WRONLY)
            .and_then(|mut file| file.write_all(b"1"))
            .map_err(CgroupError::io)
    }

    pub fn remove_empty(&self) -> io::Result<()> {
        if self.populated()? {
            return Err(io::Error::other("cgroup remains populated"));
        }
        let name = CString::new(self.name.as_str()).map_err(io::Error::other)?;
        if unsafe { libc::unlinkat(self.parent.0.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR) }
            != 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

impl Drop for Cgroup {
    fn drop(&mut self) {
        // Spawn failure can leave an empty owned group. Never remove a populated
        // or replaced group; it is retained as termination evidence.
        if !self.launched {
            let _ = self.remove_empty();
        }
    }
}

fn validate_population(changed: bool, populated: bool) -> io::Result<()> {
    if !populated {
        return Err(io::Error::other(
            "cgroup became unpopulated while child remained live",
        ));
    }
    if changed {
        return Err(io::Error::other(
            "cgroup population changed outside the runner while child remained live",
        ));
    }
    Ok(())
}

fn membership_observation(expected: &Path, actual: Option<&str>) -> Result<(), CgroupError> {
    validate_membership(expected, actual).map_err(|error| {
        if actual.is_some() {
            CgroupError::membership(error)
        } else {
            CgroupError::exit_observation(error)
        }
    })
}

fn validate_membership(expected: &Path, actual: Option<&str>) -> io::Result<()> {
    if expected.as_os_str().is_empty()
        || !actual.is_some_and(|path| Path::new(path).starts_with(expected))
    {
        return Err(io::Error::other("live child left its target cgroup"));
    }
    Ok(())
}

fn counter(text: &str, wanted: &str) -> io::Result<u64> {
    let mut found = None;
    for line in text.split('\n') {
        let mut parts = line.split_ascii_whitespace();
        if parts.next() == Some(wanted) {
            if found.is_some() {
                return Err(io::Error::other("duplicate cgroup counter"));
            }
            found = Some(
                parts
                    .next()
                    .ok_or_else(|| io::Error::other("missing cgroup counter"))?
                    .parse::<u64>()
                    .map_err(io::Error::other)?,
            );
            if parts.next().is_some() {
                return Err(io::Error::other("invalid cgroup counter"));
            }
        }
    }
    found.ok_or_else(|| io::Error::other("missing cgroup counter"))
}

fn kernel_text(path: &str) -> io::Result<String> {
    let mut text = String::new();
    File::open(path)?.take(65_537).read_to_string(&mut text)?;
    if text.len() > 65_536 {
        return Err(io::Error::other("cgroup discovery data too large"));
    }
    Ok(text)
}

fn unescape(text: &str) -> String {
    text.replace("\\040", " ")
        .replace("\\011", "\t")
        .replace("\\012", "\n")
        .replace("\\134", "\\")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn counters_reject_missing_duplicate_and_tampered_values() {
        assert_eq!(
            counter("usage_usec 42\nuser_usec 10\n", "usage_usec").unwrap(),
            42
        );
        for text in [
            "",
            "usage_usec -1",
            "usage_usec 1 extra",
            "usage_usec 1\nusage_usec 2",
        ] {
            assert!(counter(text, "usage_usec").is_err());
        }
    }
    #[test]
    fn ordinary_writable_directory_is_not_delegation() {
        let dir = tempfile::tempdir().unwrap();
        assert!(Cgroup::create(dir.path()).is_err());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn population_and_membership_anomalies_disable_early_stalls() {
        use super::super::deadline::{Deadline, TargetStatus};
        use std::time::Duration;
        assert!(validate_population(false, true).is_ok());
        let wrong =
            membership_observation(Path::new("/delegate/target"), Some("/delegate/sibling"))
                .unwrap_err();
        assert!(wrong.membership_uncertain);
        assert!(
            !wrong.may_be_exit,
            "an observed wrong group is not a natural-exit race"
        );
        let missing = membership_observation(Path::new("/delegate/target"), None).unwrap_err();
        assert!(
            missing.may_be_exit,
            "missing process data requires an exit re-check"
        );
        assert!(
            validate_membership(Path::new("/delegate/target"), Some("/delegate/target")).is_ok()
        );
        assert!(validate_membership(
            Path::new("/delegate/target"),
            Some("/delegate/target/worker")
        )
        .is_ok());
        for observation in [
            validate_population(false, false),
            validate_population(true, true),
            validate_membership(Path::new("/delegate/target"), Some("/delegate/sibling")),
            validate_membership(
                Path::new("/delegate/target"),
                Some("/delegate/target-other"),
            ),
            validate_membership(Path::new("/delegate/target"), None),
        ] {
            let reason = observation.unwrap_err().to_string();
            let mut deadline = Deadline::new(Duration::from_secs(10), true, 1, None);
            deadline.cpu(Duration::ZERO, Ok(10));
            deadline.cpu(Duration::from_secs(2), Err(reason.clone()));
            deadline.cpu(Duration::from_secs(3), Ok(10));
            assert_eq!(
                deadline.info.early_stall_unavailable.as_deref(),
                Some(reason.as_str())
            );
            assert_eq!(deadline.decision(Duration::from_secs(3)), None);
            assert_eq!(
                deadline.decision(Duration::from_secs(40)),
                Some(TargetStatus::TimedOut)
            );
        }
    }

    /// Run on an existing delegation only. This never creates a delegation or
    /// changes host configuration; unavailable hosts report the missing proof.
    #[test]
    fn delegated_children_keep_cpu_then_stall_and_clean_up() {
        use super::super::deadline::{Deadline, TargetStatus};
        use std::time::{Duration, Instant};
        let group = match Cgroup::discover() {
            Ok(group) => group,
            Err(error) => {
                eprintln!("SKIP native delegated cgroup journey: {error}");
                return;
            }
        };
        let path = group.path.clone();
        // Both waited children and SIGCHLD-ignored children exit between
        // observations. Their CPU must remain charged to the target group.
        let script = r"
import os, signal, time
for ignored in (False, True):
 signal.signal(signal.SIGCHLD, signal.SIG_IGN if ignored else signal.SIG_DFL)
 end=time.monotonic()+1.5
 while time.monotonic()<end:
  pid=os.fork()
  if pid == 0:
   if ignored and os.fork() != 0: os._exit(0)
   end_child=time.process_time()+0.08
   while time.process_time()<end_child: pass
   os._exit(0)
  if ignored: time.sleep(0.1)
  else: os.waitpid(pid, 0)
time.sleep(10)
";
        let mut command = Command::new("python3");
        command.arg("-c").arg(script);
        let mut deadline = Deadline::new(Duration::from_secs(5), true, 1, None);
        let mut group = configure(Ok(group), &mut command, &mut deadline)
            .unwrap()
            .unwrap();
        assert!(deadline.info.early_stall_unavailable.is_none());
        let mut child = command.spawn().unwrap();
        let result = (|| -> io::Result<()> {
            group.mark_launched(child.id())?;
            let start = Instant::now();
            let mut increased = false;
            loop {
                let now = start.elapsed();
                let cpu = group.sample(child.id())?;
                increased |= deadline.info.cpu_usec.is_some_and(|old| cpu > old);
                deadline.cpu(now, Ok(cpu));
                if let Some(status) = deadline.decision(now) {
                    if status != TargetStatus::Stalled || now < Duration::from_secs(3) || !increased
                    {
                        return Err(io::Error::other("child CPU failed to delay the idle stall"));
                    }
                    break;
                }
                if now >= Duration::from_secs(8) {
                    return Err(io::Error::other("idle target did not stall"));
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Ok(())
        })();
        let killed = group.kill();
        let _ = child.kill();
        child.wait().unwrap();
        let until = Instant::now() + Duration::from_secs(5);
        while group.populated().unwrap() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(20));
        }
        let removed = group.remove_empty();
        result.unwrap();
        killed.unwrap();
        removed.unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn delegated_population_transition_is_retained_between_samples() {
        let mut group = match Cgroup::discover() {
            Ok(group) => group,
            Err(error) => {
                eprintln!("SKIP native cgroup migration journey: {error}");
                return;
            }
        };
        let mut command = Command::new("sleep");
        command.arg("10");
        group.prepare(&mut command).unwrap();
        let mut child = command.spawn().unwrap();
        let result = (|| -> io::Result<String> {
            group.mark_launched(child.id())?;
            group.sample(child.id())?;
            let pid = child.id().to_string();
            group
                .parent
                .at("cgroup.procs", libc::O_WRONLY)?
                .write_all(pid.as_bytes())?;
            group
                .directory
                .at("cgroup.procs", libc::O_WRONLY)?
                .write_all(pid.as_bytes())?;
            match group.sample(child.id()) {
                Ok(_) => Err(io::Error::other(
                    "unobserved population transition accepted",
                )),
                Err(error) => {
                    let reason = error.to_string();
                    group.sample(child.id())?;
                    if !error.membership_uncertain {
                        return Err(io::Error::other(
                            "migration was classified as a counter error",
                        ));
                    }
                    Ok(reason)
                }
            }
        })();
        let _ = child.kill();
        child.wait().unwrap();
        let removed = group.remove_empty();
        assert!(result.unwrap().contains("population changed"));
        removed.unwrap();
    }

    #[test]
    fn unavailable_delegation_explicitly_disables_stalls_in_ci() {
        use super::super::deadline::{Deadline, TargetStatus};
        use std::time::Duration;
        let mut deadline = Deadline::new(Duration::from_secs(10), true, 1, None);
        let group = configure(
            Err(io::Error::from(io::ErrorKind::PermissionDenied)),
            &mut Command::new("sh"),
            &mut deadline,
        )
        .unwrap();
        assert!(group.is_none());
        assert!(deadline
            .info
            .early_stall_unavailable
            .as_deref()
            .unwrap()
            .starts_with("no delegated cgroup"));
        assert_eq!(deadline.decision(Duration::from_secs(39)), None);
        assert_eq!(
            deadline.decision(Duration::from_secs(40)),
            Some(TargetStatus::TimedOut)
        );
    }
}
