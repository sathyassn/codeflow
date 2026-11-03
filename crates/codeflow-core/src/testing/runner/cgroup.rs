//! Linux CPU accounting in an already delegated subtree; never provisions delegation.
use std::ffi::CString;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::MetadataExt;
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

pub(super) struct Cgroup {
    parent: Directory,
    directory: Directory,
    name: String,
    pub path: PathBuf,
    launched: bool,
}

impl Cgroup {
    pub fn discover() -> io::Result<Self> {
        let membership = kernel_text("/proc/self/cgroup")?;
        let current = membership
            .lines()
            .find_map(|line| line.strip_prefix("0::"))
            .ok_or_else(|| io::Error::other("no cgroup v2 membership"))?;
        if current == "/" {
            return Err(io::Error::other(
                "cgroup root is not an explicit delegated subtree",
            ));
        }
        let mounts = kernel_text("/proc/self/mountinfo")?;
        let path = mounts
            .lines()
            .find_map(|line| {
                let (left, right) = line.split_once(" - ")?;
                if right.split_whitespace().next()? != "cgroup2" {
                    return None;
                }
                let fields: Vec<_> = left.split_whitespace().collect();
                let root = unescape(fields.get(3)?);
                let mount = unescape(fields.get(4)?);
                let relative = Path::new(current).strip_prefix(&root).ok()?;
                Some(PathBuf::from(mount).join(relative))
            })
            .ok_or_else(|| io::Error::other("no delegated cgroup v2 mount"))?;
        Self::create(&path)
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
        Ok(Self {
            path: parent_path.join(&name),
            parent,
            directory,
            name,
            launched: false,
        })
    }

    pub fn mark_launched(&mut self) {
        self.launched = true;
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

    pub fn cpu(&self) -> io::Result<u64> {
        self.verify()?;
        counter(&self.directory.text("cpu.stat")?, "usage_usec")
    }

    pub fn populated(&self) -> io::Result<bool> {
        self.verify()?;
        match counter(&self.directory.text("cgroup.events")?, "populated")? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(io::Error::other("invalid cgroup populated value")),
        }
    }

    pub fn kill(&self) -> io::Result<()> {
        self.verify()?;
        self.directory
            .at("cgroup.kill", libc::O_WRONLY)?
            .write_all(b"1")
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

fn counter(text: &str, wanted: &str) -> io::Result<u64> {
    let mut found = None;
    for line in text.lines() {
        let mut parts = line.split_whitespace();
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
}
