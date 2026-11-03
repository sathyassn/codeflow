//! Writer preference, using stable read-only ownership locks and separate
//! closed records. Every publication, inspection and removal holds the same
//! queue mutex. None of the existing Windows sharing flags need to change.
use super::{
    describe_holder, holder_record, lock_anchor, os_lock, parse_record, try_open_and_lock,
    AcquiredFile, LockHeld, Slot,
};
use crate::contained::Tree;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub(super) const QUEUE_MUTEX: &str = "exclusive-queue.lock";
#[cfg(test)]
mod interleave {
    use std::cell::RefCell;
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(super) enum Point {
        Disabled,
        OwnerCreated,
        OwnerLocked,
        SequenceAllocated,
        QueueWritten,
        Published,
        RecordRemoved,
        OwnerClosed,
        OwnerRemoved,
    }
    type Step = Box<dyn FnMut(Point)>;
    thread_local! { static STEP: RefCell<Option<Step>> = const { RefCell::new(None) }; }
    pub(super) fn set(step: impl FnMut(Point) + 'static) {
        STEP.with(|slot| *slot.borrow_mut() = Some(Box::new(step)));
    }
    pub(super) fn clear() {
        STEP.with(|slot| *slot.borrow_mut() = None);
    }
    pub(super) fn at(point: Point) {
        STEP.with(|slot| {
            if let Some(step) = slot.borrow_mut().as_mut() {
                step(point);
            }
        });
    }
}

const MAX_ENTRIES: usize = 4096;
static RETAINED_OWNERS: Mutex<Vec<AcquiredFile>> = Mutex::new(Vec::new());

fn unavailable(dir: &Path, error: impl std::fmt::Display) -> LockHeld {
    LockHeld::unavailable(
        dir.join(QUEUE_MUTEX),
        format!("gate lock unavailable: exclusive queue: {error}"),
    )
}

fn blocked(dir: &Path, record: String) -> LockHeld {
    LockHeld {
        path: dir.join(QUEUE_MUTEX),
        holder: record,
        running_groups: Vec::new(),
        exclusive: true,
    }
}

struct Domain {
    tree: Tree,
    base: String,
}

impl Domain {
    fn open(dir: &Path) -> io::Result<Self> {
        let path = dir.join(QUEUE_MUTEX);
        let (anchor, relative) = lock_anchor(&path)?;
        std::fs::create_dir_all(anchor)?;
        let tree = Tree::open(anchor)?;
        let base = format!("{}exclusive", relative.strip_suffix(QUEUE_MUTEX).unwrap());
        tree.create_dir_all(&format!("{base}/queue"))?;
        tree.create_dir_all(&format!("{base}/owners"))?;
        Ok(Self { tree, base })
    }

    fn names(&self, folder: &str) -> io::Result<Vec<String>> {
        let mut names = Vec::new();
        for entry in std::fs::read_dir(self.tree.path().join(&self.base).join(folder))? {
            if names.len() == MAX_ENTRIES {
                return Err(io::Error::other("too many exclusive queue entries"));
            }
            let name = entry?
                .file_name()
                .into_string()
                .map_err(|_| io::Error::other("non-Unicode queue entry"))?;
            names.push(name);
        }
        names.sort();
        Ok(names)
    }

    fn owner(&self, id: &str) -> String {
        format!("{}/owners/{id}.lock", self.base)
    }
    fn record(&self, name: &str) -> String {
        format!("{}/queue/{name}", self.base)
    }

    // Existing owners must be opened without create. A missing file cannot
    // be manufactured as evidence of a dead requester. The read-only opener
    // also avoids Windows creation handles retaining DELETE access.
    fn inspect_owner(&self, id: &str) -> io::Result<Option<AcquiredFile>> {
        let relative = self.owner(id);
        let file = self.tree.open_regular(&relative)?;
        match os_lock::try_lock(&file) {
            Ok(()) => {
                self.check_owner(id, &file)?;
                Ok(Some(AcquiredFile(file)))
            }
            Err(error) if os_lock::is_contended(&error) => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn check_owner(&self, id: &str, file: &std::fs::File) -> io::Result<()> {
        let current = self.tree.open_regular(&self.owner(id))?;
        if crate::bounded_file::FileIdentity::of(file)?
            == crate::bounded_file::FileIdentity::of(&current)?
        {
            Ok(())
        } else {
            Err(io::Error::other("ownership path changed while locking"))
        }
    }

    fn remove_owner(&self, id: &str, owner: AcquiredFile) -> io::Result<()> {
        // Both unlock AND close must precede deletion on Windows.
        drop(owner);
        #[cfg(test)]
        interleave::at(interleave::Point::OwnerClosed);
        self.tree.remove(&self.owner(id))?;
        #[cfg(test)]
        interleave::at(interleave::Point::OwnerRemoved);
        Ok(())
    }

    fn head(&self, trusted: &[String]) -> io::Result<Option<(String, String)>> {
        let mut first = None;
        let mut referenced = std::collections::BTreeSet::new();
        for name in self.names("queue")? {
            if let Some(id) = name.strip_prefix(".tmp-") {
                validate_id(id)?;
                continue;
            }
            let id = record_id(&name)?;
            referenced.insert(id.to_string());
            let bytes = self.tree.read(&self.record(&name), 8192)?;
            let text = std::str::from_utf8(&bytes).map_err(io::Error::other)?;
            let record = parse_record(text, trusted).map_err(io::Error::other)?;
            if let Some(owner) = self.inspect_owner(id)? {
                if let Err(error) = self.tree.remove(&self.record(&name)) {
                    retain(owner);
                    return Err(error);
                }
                #[cfg(test)]
                interleave::at(interleave::Point::RecordRemoved);
                self.remove_owner(id, owner)?;
            } else if first.is_none() {
                first = Some((name, record));
            }
        }
        // Crashes after creation or after record removal can leave only an
        // owner file. The mutex excludes publication and teardown races.
        for name in self.names("owners")? {
            let id = name
                .strip_suffix(".lock")
                .ok_or_else(|| io::Error::other("invalid ownership name"))?;
            validate_id(id)?;
            if referenced.contains(id) {
                continue;
            }
            if let Some(owner) = self.inspect_owner(id)? {
                let temporary = self.record(&format!(".tmp-{id}"));
                self.tree.remove(&temporary)?;
                self.remove_owner(id, owner)?;
            }
        }
        Ok(first)
    }
}

fn validate_id(id: &str) -> io::Result<()> {
    if id.len() == 32
        && id
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        Ok(())
    } else {
        Err(io::Error::other("invalid ownership identifier"))
    }
}

fn record_id(name: &str) -> io::Result<&str> {
    let (sequence, id) = name
        .split_once('-')
        .ok_or_else(|| io::Error::other("invalid queue record name"))?;
    if sequence.len() != 20
        || !sequence.bytes().all(|b| b.is_ascii_digit())
        || sequence.parse::<u64>().is_err()
    {
        return Err(io::Error::other("invalid queue sequence"));
    }
    validate_id(id)?;
    Ok(id)
}

fn retain(owner: AcquiredFile) {
    RETAINED_OWNERS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(owner);
}

/// Held until the caller has taken all its shared leases and slots. The
/// publication mutex closes the check-to-acquire window for ordinary arrivals.
pub(super) fn shared(dirs: &[PathBuf], trusted: &[String]) -> Result<Vec<AcquiredFile>, LockHeld> {
    // Serialize the short metadata transaction without treating another
    // shared arrival as an exclusive writer. This bound matches the existing
    // holder-record serialization bound; no host lease is held while waiting.
    let locks = bounded_mutexes(dirs, Instant::now() + Duration::from_secs(5))?;
    for dir in dirs {
        let domain = Domain::open(dir).map_err(|error| unavailable(dir, error))?;
        if let Some((_, record)) = domain
            .head(trusted)
            .map_err(|error| unavailable(dir, error))?
        {
            return Err(blocked(dir, record));
        }
    }
    Ok(locks)
}

fn mutexes(dirs: &[PathBuf]) -> Result<Vec<AcquiredFile>, LockHeld> {
    let mut locks = Vec::new();
    // All attempts are non-blocking; partial mutexes drop before a retry.
    for dir in dirs {
        match try_open_and_lock(&dir.join(QUEUE_MUTEX), false)? {
            Slot::Taken(file) => locks.push(AcquiredFile(file)),
            Slot::Busy => return Err(blocked(dir, String::new())),
        }
    }
    Ok(locks)
}

fn wait_mutexes(dirs: &[PathBuf], deadline: Instant) -> Result<Vec<AcquiredFile>, String> {
    bounded_mutexes(dirs, deadline).map_err(|held| held.to_string())
}

fn bounded_mutexes(dirs: &[PathBuf], deadline: Instant) -> Result<Vec<AcquiredFile>, LockHeld> {
    loop {
        match mutexes(dirs) {
            Ok(locks) => return Ok(locks),
            Err(held) if held.exclusive && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(held) => return Err(held),
        }
    }
}

struct Owned {
    dir: PathBuf,
    id: String,
    record: Option<String>,
    owner: Option<AcquiredFile>,
}

/// Stable ownership survives both queueing and the exclusive target's run.
pub(super) struct Ticket {
    owned: Vec<Owned>,
    deadline: Instant,
}

impl std::fmt::Debug for Ticket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExclusiveTicket")
            .field("domains", &self.owned.len())
            .finish_non_exhaustive()
    }
}

impl Ticket {
    pub(super) fn publish(
        dirs: &[PathBuf],
        root: &Path,
        deadline: Instant,
    ) -> Result<Self, String> {
        let mut ticket = Self {
            owned: Vec::new(),
            deadline,
        };
        let locks = wait_mutexes(dirs, deadline)?;
        let id = uuid::Uuid::new_v4().simple().to_string();
        let result = (|| -> io::Result<()> {
            for dir in dirs {
                let domain = Domain::open(dir)?;
                domain.tree.create_new(&domain.owner(&id), b"")?;
                #[cfg(test)]
                interleave::at(interleave::Point::OwnerCreated);
                // create_new closed its creation handle. Reopen read only
                // before locking; never retain its Windows DELETE access.
                let file = domain.tree.open_regular(&domain.owner(&id))?;
                os_lock::try_lock(&file)?;
                let owner = AcquiredFile(file);
                #[cfg(test)]
                interleave::at(interleave::Point::OwnerLocked);
                domain.check_owner(&id, &owner.0)?;
                ticket.owned.push(Owned {
                    dir: dir.clone(),
                    id: id.clone(),
                    record: None,
                    owner: Some(owner),
                });
                let counter = format!("{}/sequence", domain.base);
                let sequence =
                    if crate::absence::proven_absent(&domain.tree.path().join(&counter))? {
                        0
                    } else {
                        let bytes = domain.tree.read(&counter, 32)?;
                        std::str::from_utf8(&bytes)
                            .map_err(io::Error::other)?
                            .parse::<u64>()
                            .map_err(io::Error::other)?
                    }
                    .checked_add(1)
                    .ok_or_else(|| io::Error::other("exclusive queue sequence exhausted"))?;
                domain
                    .tree
                    .write(&counter, sequence.to_string().as_bytes())?;
                #[cfg(test)]
                interleave::at(interleave::Point::SequenceAllocated);
                let temporary = domain.record(&format!(".tmp-{id}"));
                domain
                    .tree
                    .create_new(&temporary, holder_record(root, "exclusive").as_bytes())?;
                #[cfg(test)]
                interleave::at(interleave::Point::QueueWritten);
                let name = format!("{sequence:020}-{id}");
                domain.tree.rename(&temporary, &domain.record(&name))?;
                #[cfg(test)]
                interleave::at(interleave::Point::Published);
                ticket.owned.last_mut().unwrap().record = Some(name);
            }
            Ok(())
        })();
        drop(locks);
        result.map_err(|error| {
            format!("gate lock unavailable: publishing exclusive ticket: {error}")
        })?;
        Ok(ticket)
    }

    pub(super) fn waiting_for(&self, trusted: &[String]) -> Result<Option<String>, String> {
        let dirs: Vec<_> = self.owned.iter().map(|owner| owner.dir.clone()).collect();
        let _locks = wait_mutexes(&dirs, self.deadline)?;
        for owned in &self.owned {
            let domain = Domain::open(&owned.dir)
                .map_err(|error| unavailable(&owned.dir, error).to_string())?;
            let first = domain
                .head(trusted)
                .map_err(|error| unavailable(&owned.dir, error).to_string())?;
            match first {
                Some((name, _)) if owned.record.as_ref() == Some(&name) => {}
                Some((_, record)) => return Ok(Some(describe_holder(&record))),
                None => {
                    return Err("gate lock unavailable: own exclusive ticket disappeared".into())
                }
            }
        }
        Ok(None)
    }

    /// Unpublish only after the exclusive host lease and its holder record
    /// exist. Ownership stays locked until the target's confirmed completion.
    pub(super) fn admitted(&mut self) -> Result<(), String> {
        let dirs: Vec<_> = self.owned.iter().map(|owner| owner.dir.clone()).collect();
        let _locks = wait_mutexes(&dirs, self.deadline)?;
        for owned in &mut self.owned {
            if let Some(name) = owned.record.as_ref() {
                let domain = Domain::open(&owned.dir).map_err(|e| e.to_string())?;
                domain
                    .tree
                    .remove(&domain.record(name))
                    .map_err(|e| e.to_string())?;
                #[cfg(test)]
                interleave::at(interleave::Point::RecordRemoved);
                owned.record = None;
            }
        }
        Ok(())
    }

    fn cancel(&mut self) -> Result<(), String> {
        let dirs: Vec<_> = self.owned.iter().map(|owner| owner.dir.clone()).collect();
        let _locks = wait_mutexes(&dirs, Instant::now() + Duration::from_secs(2))?;
        for owned in &mut self.owned {
            let domain = Domain::open(&owned.dir).map_err(|e| e.to_string())?;
            if let Some(name) = owned.record.as_ref() {
                domain
                    .tree
                    .remove(&domain.record(name))
                    .map_err(|e| e.to_string())?;
                #[cfg(test)]
                interleave::at(interleave::Point::RecordRemoved);
                owned.record = None;
            }
            domain
                .tree
                .remove(&domain.record(&format!(".tmp-{}", owned.id)))
                .map_err(|e| e.to_string())?;
            if let Some(owner) = owned.owner.take() {
                domain
                    .remove_owner(&owned.id, owner)
                    .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
}

impl Drop for Ticket {
    fn drop(&mut self) {
        if let Err(error) = self.cancel() {
            eprintln!("[codeflow test] exclusive queue cleanup refused: {error}; ownership retained until process exit");
            for owned in &mut self.owned {
                if let Some(owner) = owned.owner.take() {
                    retain(owner);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::open_lock_file;
    use super::interleave::{self, Point};
    use super::*;
    use std::process::{Child, Stdio};

    fn domain(tmp: &Path) -> Vec<PathBuf> {
        vec![tmp.join("repo/locks")]
    }
    fn deadline() -> Instant {
        Instant::now() + Duration::from_secs(10)
    }

    fn lifecycle() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = domain(tmp.path());
        let mut ticket = Ticket::publish(&dirs, tmp.path(), deadline()).unwrap();
        let owner_path = {
            let domain = Domain::open(&dirs[0]).unwrap();
            domain.tree.path().join(domain.owner(&ticket.owned[0].id))
        };
        let owner = open_lock_file(&owner_path).unwrap();
        assert!(
            os_lock::try_lock(&owner).is_err(),
            "the creation handle did not interfere with opening a live owner"
        );
        drop(owner);
        assert!(ticket.waiting_for(&[]).unwrap().is_none());
        assert!(
            shared(&dirs, &[]).is_err(),
            "ordinary arrivals respect the queued writer"
        );
        ticket.admitted().unwrap();
        assert!(owner_path.exists(), "ownership lasts through the run");
        assert!(Domain::open(&dirs[0])
            .unwrap()
            .names("queue")
            .unwrap()
            .is_empty());
        assert!(
            shared(&dirs, &[]).is_ok(),
            "the host lease, rather than a stale ticket, protects an admitted writer"
        );
        drop(ticket);
        assert!(
            !owner_path.exists(),
            "normal exit closes before removing ownership"
        );
    }

    #[test]
    fn stable_ownership_and_closed_records_cover_the_whole_lifecycle() {
        lifecycle();
    }

    #[test]
    fn queued_writers_keep_fifo_order_and_block_sustained_shared_arrivals() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = domain(tmp.path());
        let first = Ticket::publish(&dirs, tmp.path(), deadline()).unwrap();
        let second = Ticket::publish(&dirs, tmp.path(), deadline()).unwrap();
        for _ in 0..20 {
            assert!(shared(&dirs, &[]).is_err());
            assert!(second.waiting_for(&[]).unwrap().is_some());
            assert!(first.waiting_for(&[]).unwrap().is_none());
        }
        drop(first);
        assert!(second.waiting_for(&[]).unwrap().is_none());
        drop(second);
        assert!(shared(&dirs, &[]).is_ok());
    }

    struct ChildOwner(Child);
    impl Drop for ChildOwner {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    // A native test subprocess, not a shell mock: killing it releases the
    // actual OS ownership and queue-mutex locks on Windows as well as Unix.
    #[test]
    fn owner_child() {
        let Some(root) = std::env::var_os("CODEFLOW_QUEUE_CHILD_ROOT") else {
            return;
        };
        let root = PathBuf::from(root);
        let point = std::env::var("CODEFLOW_QUEUE_CHILD_POINT").unwrap();
        let action = std::env::var("CODEFLOW_QUEUE_CHILD_ACTION").unwrap_or_default();
        let ready = root.join("ready");
        interleave::set(move |at| {
            if format!("{at:?}") == point {
                std::fs::write(&ready, b"ready").unwrap();
                loop {
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        });
        if let Some(label) = action.strip_prefix("host-") {
            // Record the ticket order while publication still holds its
            // mutex. The records are closed before the callback returns.
            let published_root = root.clone();
            let published_label = label.to_string();
            interleave::set(move |at| {
                if at == Point::Published {
                    let domain = Domain::open(&domain(&published_root)[0]).unwrap();
                    let mine = format!("pid={}\n", std::process::id());
                    for name in domain.names("queue").unwrap() {
                        if name.starts_with(".tmp-") {
                            continue;
                        }
                        let bytes = domain.tree.read(&domain.record(&name), 8192).unwrap();
                        if String::from_utf8(bytes).unwrap().starts_with(&mine) {
                            std::fs::write(
                                published_root.join(format!("sequence-{published_label}")),
                                name,
                            )
                            .unwrap();
                        }
                    }
                }
            });
            let shared = super::super::acquire_gate_lock(&domain(&root), &root, "full").unwrap();
            std::fs::write(root.join(format!("shared-{label}")), "").unwrap();
            wait_file(&root.join("request-exclusive"));
            let exclusive = super::super::hold_host_exclusive_in(
                &domain(&root),
                &root,
                Duration::from_secs(10),
            )
            .unwrap();
            std::fs::write(root.join(format!("exclusive-{label}")), "").unwrap();
            wait_file(&root.join(format!("release-{label}")));
            drop(exclusive);
            drop(shared);
            interleave::clear();
            return;
        } else if action == "unconfirmed" {
            let hold = super::super::hold_host_exclusive_in(
                &domain(&root),
                &root,
                Duration::from_secs(10),
            )
            .unwrap();
            let mut exclusive = Some((0, hold));
            let mut result = crate::testing::runner::TargetRunResult {
                target_name: "incomplete-cleanup".into(),
                termination_unconfirmed: true,
                exit_code: 0,
                error: None,
                stdout: String::new(),
                stderr: String::new(),
                stdout_truncated: false,
                stderr_truncated: false,
                duration_ms: 0,
                report_path: None,
                coverage_path: None,
                evidence: None,
            };
            assert!(crate::testing::runner::retain_if_unconfirmed(
                &root,
                &mut result,
                &mut exclusive
            ));
            assert_ne!(result.exit_code, 0);
            std::fs::write(root.join("ready"), b"retained").unwrap();
            loop {
                std::thread::sleep(Duration::from_millis(10));
            }
        } else if action == "reclaim" {
            let _ = shared(&domain(&root), &[]).unwrap();
        } else {
            let ticket = Ticket::publish(&domain(&root), &root, deadline()).unwrap();
            drop(ticket);
        }
        panic!("the requested interruption point was not reached");
    }

    fn spawn_owner(root: &Path, point: Point, action: &str) -> ChildOwner {
        let child = crate::git::process(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "testing::gate_guard::queue::tests::owner_child",
                "--nocapture",
            ])
            .env("CODEFLOW_QUEUE_CHILD_ROOT", root)
            .env("CODEFLOW_QUEUE_CHILD_POINT", format!("{point:?}"))
            .env("CODEFLOW_QUEUE_CHILD_ACTION", action)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        ChildOwner(child)
    }

    fn wait_file(path: &Path) {
        let until = deadline();
        while !path.exists() {
            assert!(Instant::now() < until, "missing {}", path.display());
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn stopped_owner(root: &Path, point: Point, action: &str) -> ChildOwner {
        let _ = std::fs::remove_file(root.join("ready"));
        let child = spawn_owner(root, point, action);
        let mut child = child;
        let until = deadline();
        while !root.join("ready").exists() {
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "owner helper exited early"
            );
            assert!(
                Instant::now() < until,
                "owner helper did not reach {point:?}"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        child
    }

    fn dead_first_waiter() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = domain(tmp.path());
        let mut child = stopped_owner(tmp.path(), Point::Published, "");
        // Child is paused under the publication mutex. Kill then reap before
        // checking recovery; a live owner must never be inferred dead by PID.
        child.0.kill().unwrap();
        child.0.wait().unwrap();
        assert!(shared(&dirs, &[]).is_ok());
        assert!(Domain::open(&dirs[0])
            .unwrap()
            .names("owners")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn interrupted_publication_and_cancellation_recover_without_skipping_live_owners() {
        for point in [
            Point::OwnerCreated,
            Point::OwnerLocked,
            Point::SequenceAllocated,
            Point::QueueWritten,
            Point::Published,
            Point::RecordRemoved,
            Point::OwnerClosed,
            Point::OwnerRemoved,
        ] {
            let tmp = tempfile::tempdir().unwrap();
            let dirs = domain(tmp.path());
            let mut child = stopped_owner(tmp.path(), point, "");
            child.0.kill().unwrap();
            child.0.wait().unwrap();
            let live = Ticket::publish(&dirs, tmp.path(), deadline()).unwrap();
            assert!(live.waiting_for(&[]).unwrap().is_none(), "{point:?}");
            assert!(
                shared(&dirs, &[]).is_err(),
                "{point:?}: live owner must survive cleanup"
            );
            drop(live);
            assert!(shared(&dirs, &[]).is_ok(), "{point:?}");
        }
    }

    #[test]
    fn interrupted_dead_ticket_reclamation_is_recoverable() {
        for point in [
            Point::RecordRemoved,
            Point::OwnerClosed,
            Point::OwnerRemoved,
        ] {
            let tmp = tempfile::tempdir().unwrap();
            let mut first = stopped_owner(tmp.path(), Point::Published, "");
            first.0.kill().unwrap();
            first.0.wait().unwrap();
            let mut reclaimer = stopped_owner(tmp.path(), point, "reclaim");
            reclaimer.0.kill().unwrap();
            reclaimer.0.wait().unwrap();
            assert!(shared(&domain(tmp.path()), &[]).is_ok(), "{point:?}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_publishes_records_while_stable_ownership_stays_locked() {
        lifecycle();
    }

    #[cfg(windows)]
    #[test]
    fn windows_live_owner_inspection_keeps_read_only_access_semantics() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = domain(tmp.path());
        let ticket = Ticket::publish(&dirs, tmp.path(), deadline()).unwrap();
        let domain = Domain::open(&dirs[0]).unwrap();
        assert!(domain.inspect_owner(&ticket.owned[0].id).unwrap().is_none());
        assert!(
            domain
                .tree
                .remove(&domain.owner(&ticket.owned[0].id))
                .is_err(),
            "held ownership still denies delete sharing"
        );
        drop(ticket);
    }

    #[cfg(windows)]
    #[test]
    fn windows_cancellation_closes_before_removing_ownership() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = domain(tmp.path());
        let mut ticket = Ticket::publish(&dirs, tmp.path(), deadline()).unwrap();
        ticket.cancel().unwrap();
        assert!(Domain::open(&dirs[0])
            .unwrap()
            .names("owners")
            .unwrap()
            .is_empty());
        assert!(shared(&dirs, &[]).is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn windows_dead_ticket_reclamation_removes_record_before_owner() {
        dead_first_waiter();
    }

    #[test]
    fn a_dead_first_waiter_is_reclaimed() {
        dead_first_waiter();
    }

    #[test]
    fn corrupt_sequence_refuses_instead_of_resetting_arrival_order() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = domain(tmp.path());
        drop(Ticket::publish(&dirs, tmp.path(), deadline()).unwrap());
        let domain = Domain::open(&dirs[0]).unwrap();
        domain
            .tree
            .write(&format!("{}/sequence", domain.base), b"corrupt")
            .unwrap();
        assert!(Ticket::publish(&dirs, tmp.path(), deadline()).is_err());
        assert!(domain.names("queue").unwrap().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn a_dangling_sequence_link_is_not_treated_as_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = domain(tmp.path());
        drop(Ticket::publish(&dirs, tmp.path(), deadline()).unwrap());
        let domain = Domain::open(&dirs[0]).unwrap();
        let relative = format!("{}/sequence", domain.base);
        domain.tree.remove(&relative).unwrap();
        let outside = tmp.path().join("outside-counter");
        std::os::unix::fs::symlink(&outside, domain.tree.path().join(&relative)).unwrap();
        assert!(Ticket::publish(&dirs, tmp.path(), deadline()).is_err());
        assert!(!outside.exists());
    }

    #[cfg(unix)]
    #[test]
    fn queue_creation_in_a_read_only_folder_refuses() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let dirs = domain(tmp.path());
        std::fs::create_dir_all(&dirs[0]).unwrap();
        std::fs::set_permissions(&dirs[0], std::fs::Permissions::from_mode(0o500)).unwrap();
        let refused = shared(&dirs, &[]);
        std::fs::set_permissions(&dirs[0], std::fs::Permissions::from_mode(0o700)).unwrap();
        let error = refused.unwrap_err();
        assert!(
            error.to_string().contains("gate lock unavailable"),
            "{error}"
        );
        assert!(error.path.ends_with(QUEUE_MUTEX));
    }

    #[test]
    fn two_gates_release_their_slots_and_run_in_ticket_order() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let mut a = spawn_owner(root, Point::Disabled, "host-a");
        let mut b = spawn_owner(root, Point::Disabled, "host-b");
        wait_file(&root.join("shared-a"));
        wait_file(&root.join("shared-b"));
        std::fs::write(root.join("request-exclusive"), "").unwrap();
        wait_file(&root.join("sequence-a"));
        wait_file(&root.join("sequence-b"));
        let order_a = std::fs::read_to_string(root.join("sequence-a")).unwrap();
        let order_b = std::fs::read_to_string(root.join("sequence-b")).unwrap();
        let (first, second) = if order_a < order_b {
            ("a", "b")
        } else {
            ("b", "a")
        };
        wait_file(&root.join(format!("exclusive-{first}")));
        assert!(!root.join(format!("exclusive-{second}")).exists());
        std::fs::write(root.join(format!("release-{first}")), "").unwrap();
        wait_file(&root.join(format!("exclusive-{second}")));
        std::fs::write(root.join(format!("release-{second}")), "").unwrap();
        assert!(a.0.wait().unwrap().success());
        assert!(b.0.wait().unwrap().success());
    }

    #[test]
    fn incomplete_cleanup_keeps_the_host_until_gate_exit() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = domain(tmp.path());
        let mut child = stopped_owner(tmp.path(), Point::Disabled, "unconfirmed");
        let refusal = super::super::acquire_gate_lock(&dirs, tmp.path(), "full").unwrap_err();
        assert!(refusal.exclusive, "{refusal}");
        child.0.kill().unwrap();
        child.0.wait().unwrap();
        let admitted = super::super::acquire_gate_lock(&dirs, tmp.path(), "full").unwrap();
        drop(admitted);
    }
}
