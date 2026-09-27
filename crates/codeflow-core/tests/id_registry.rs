//! Journeys for the shared id registry (TSK-101, SPC-013 R-104): each with a
//! passing control and a fault that fails for the stated reason. Every
//! repository is a temporary clone of a local bare remote; no network.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use codeflow_core::ids::check::{self, registry_ref};
use codeflow_core::ids::issue::{self, Request};
use codeflow_core::ids::seed::{self, SeedMap};
use codeflow_core::ids::{
    new_uid, state, Git, IdsError, Kind, Ledger, RegId, Standing, REGISTRY_REF,
};

fn setup() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        // Hermetic git: no user or system configuration (signing, hooks).
        std::env::set_var("GIT_CONFIG_GLOBAL", "/dev/null");
        std::env::set_var("GIT_CONFIG_SYSTEM", "/dev/null");
        std::env::remove_var("GIT_DIR");
        std::env::remove_var("GIT_WORK_TREE");
    });
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?} in {}: {}",
        dir.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

struct World {
    dir: tempfile::TempDir,
}

impl World {
    /// A bare remote whose `main` holds one commit.
    fn new() -> World {
        setup();
        let world = World {
            dir: tempfile::tempdir().unwrap(),
        };
        git(
            world.dir.path(),
            &["init", "-q", "--bare", "-b", "main", "remote.git"],
        );
        let seed = world.clone_as("seed", "seed@example.test");
        std::fs::write(seed.join("README.md"), "fixture\n").unwrap();
        git(&seed, &["add", "."]);
        git(&seed, &["commit", "-q", "-m", "fixture"]);
        git(&seed, &["push", "-q", "origin", "main"]);
        world
    }

    fn bare(&self) -> PathBuf {
        self.dir.path().join("remote.git")
    }

    fn clone_as(&self, name: &str, email: &str) -> PathBuf {
        let path = self.dir.path().join(name);
        git(
            self.dir.path(),
            &["clone", "-q", self.bare().to_str().unwrap(), name],
        );
        git(&path, &["config", "user.email", email]);
        git(&path, &["config", "user.name", name]);
        path
    }

    fn hook(&self, name: &str, body: &str) {
        let path = self.bare().join("hooks").join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    fn remote_ledger(&self) -> Ledger {
        Ledger::read(&Git::new(&self.bare()), "refs/heads/codeflow/registry").unwrap()
    }
}

fn task(root: &Path, title: &str) -> Result<issue::Reservation, IdsError> {
    issue::reserve(root, &Request::issue(Kind::Tsk, title, "main"))
}

fn record_text(id: &str, uid: Option<&str>) -> String {
    let uid = uid.map_or(String::new(), |uid| format!("uid: {uid}\n"));
    format!("---\nid: {id}\n{uid}title: \"{id} record\"\nstatus: todo\n---\n\n# {id}\n")
}

fn write_record(root: &Path, id: &str, uid: Option<&str>) -> PathBuf {
    let path = root
        .join("project-management/tasks")
        .join(format!("{id}.md"));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, record_text(id, uid)).unwrap();
    path
}

fn commit_all(root: &Path, message: &str) -> String {
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", message]);
    git(root, &["rev-parse", "HEAD"])
}

fn seq(reservation: &issue::Reservation) -> u64 {
    reservation.id.seq().unwrap()
}

// --- AC-1: concurrent issue from two clones and two worktrees -------------

#[test]
fn concurrent_issuers_in_two_clones_and_two_worktrees_never_share_a_number() {
    let world = World::new();
    let a = world.clone_as("a", "a@example.test");
    let b = world.clone_as("b", "b@example.test");
    // The first issue on a repository without records creates the registry.
    let first = task(&a, "first").unwrap();
    assert_eq!(first.id.to_string(), "TSK-001");
    assert_eq!(first.standing, Standing::Reserved);
    git(&a, &["worktree", "add", "-q", "-b", "task/wt", "../a-wt"]);
    let worktree = world.dir.path().join("a-wt");

    let roots = [a.clone(), b.clone(), worktree.clone(), b.clone()];
    let handles: Vec<_> = roots
        .into_iter()
        .enumerate()
        .map(|(n, root)| {
            std::thread::spawn(move || {
                (0..2)
                    .map(|i| task(&root, &format!("t{n}-{i}")).unwrap())
                    .collect::<Vec<_>>()
            })
        })
        .collect();
    let issued: Vec<issue::Reservation> = handles
        .into_iter()
        .flat_map(|handle| handle.join().unwrap())
        .collect();
    let ids: BTreeSet<String> = issued.iter().map(|r| r.id.to_string()).collect();
    assert_eq!(ids.len(), 8, "every issue got its own number: {ids:?}");
    assert!(issued.iter().all(|r| r.standing == Standing::Reserved));
    let ledger = world.remote_ledger();
    for reservation in &issued {
        let entry = ledger
            .entry(&reservation.id)
            .expect("bound on the authority");
        assert_eq!(
            entry.uid, reservation.uid,
            "{} bound to its own uid",
            reservation.id
        );
    }
    assert!(!ledger.is_damaged() && ledger.violations.is_empty());
    assert_eq!(ledger.max_seq(Kind::Tsk), 9);
}

// --- AC-2: moved tip retried; other outcomes reported as themselves --------

#[test]
fn a_moved_tip_is_retried_and_a_permission_or_transport_error_is_never_taken() {
    let world = World::new();
    let a = world.clone_as("a", "a@example.test");
    let b = world.clone_as("b", "b@example.test");
    task(&a, "first").unwrap();
    // Prepare a competing reservation as a ref on the host, then make the
    // host move the registry tip to it while B's push is in flight: the
    // host reports "incorrect old value", the loser of a simultaneous race.
    task(&b, "warm up").unwrap();
    let racer = {
        let git_b = Git::new(&b);
        let tip = git_b.rev(REGISTRY_REF).unwrap();
        let entry = codeflow_core::ids::Entry::issued(
            RegId::parse("TSK-003").unwrap(),
            new_uid(),
            "racer",
            "racer@example.test",
            "main",
        );
        let blob = git_b.write_blob(entry.render().as_bytes()).unwrap();
        let commit = git_b
            .commit_files(
                Some(&tip),
                &[("ids/TSK/003.toml".to_string(), blob)],
                "issue: TSK-003",
            )
            .unwrap();
        git(
            &b,
            &[
                "push",
                "-q",
                "origin",
                &format!("{commit}:refs/heads/racer"),
            ],
        );
        commit
    };
    world.hook(
        "pre-receive",
        "if [ ! -f race.done ]; then touch race.done; env -u GIT_QUARANTINE_PATH git update-ref refs/heads/codeflow/registry \"$(git rev-parse refs/heads/racer)\"; fi\nexit 0",
    );
    let retried = task(&b, "after the race").unwrap();
    assert!(world.bare().join("race.done").exists(), "the race happened");
    assert_eq!(
        retried.id.to_string(),
        "TSK-004",
        "recomputed past the racer"
    );
    assert!(
        world
            .remote_ledger()
            .first_add(&RegId::parse("TSK-003").unwrap())
            .unwrap()
            .commit
            == racer
    );

    // Permission: the host refuses; nothing is reserved or kept pending.
    world.hook(
        "pre-receive",
        "echo 'error: GH013: Repository rule violations found' >&2\nexit 1",
    );
    let before = world.remote_ledger().tip.clone();
    match task(&a, "refused") {
        Err(IdsError::Permission(reason)) => assert!(reason.contains("GH013"), "{reason}"),
        other => panic!("expected a permission error, got {other:?}"),
    }
    assert_eq!(world.remote_ledger().tip, before);
    assert!(!issue::has_pending(&a), "a refusal leaves nothing pending");

    // Transport: the fetch works but the push cannot reach the host.
    world.hook("pre-receive", "exit 0");
    git(
        &a,
        &["config", "remote.origin.pushurl", "/nonexistent/remote.git"],
    );
    match task(&a, "unreachable push") {
        Err(IdsError::Transport(reason)) => assert!(reason.contains("nonexistent"), "{reason}"),
        other => panic!("expected a transport error, got {other:?}"),
    }
    assert_eq!(world.remote_ledger().tip, before);
    git(&a, &["config", "--unset", "remote.origin.pushurl"]);
}

#[test]
fn a_tip_that_keeps_moving_stops_after_the_bounded_retries() {
    let world = World::new();
    let a = world.clone_as("a", "a@example.test");
    task(&a, "first").unwrap();
    // Every push loses: the host always reports a moved tip.
    world.hook(
        "pre-receive",
        "echo 'cannot lock ref refs/heads/codeflow/registry: is at x but expected y' >&2\nexit 1",
    );
    match task(&a, "contended") {
        Err(IdsError::Contended(attempts, reason)) => {
            assert_eq!(attempts, codeflow_core::ids::PUSH_ATTEMPTS);
            assert!(reason.contains("cannot lock ref"), "{reason}");
        }
        other => panic!("expected contention, got {other:?}"),
    }
}

// --- AC-3: lost acknowledgement read back by uid ---------------------------

#[test]
fn a_lost_acknowledgement_is_reserved_only_when_read_back_binds_our_uid() {
    let world = World::new();
    let a = world.clone_as("a", "a@example.test");
    task(&a, "first").unwrap();
    // The host commits the ref, then the connection dies before the reply.
    world.hook(
        "reference-transaction",
        "[ \"$1\" = committed ] || exit 0\ngrep -q refs/heads/codeflow/registry || exit 0\nkill -9 $PPID",
    );
    let lost = task(&a, "lost ack").unwrap();
    assert_eq!(lost.standing, Standing::Reserved);
    assert!(
        lost.note
            .as_deref()
            .unwrap_or_default()
            .contains("read back as reserved"),
        "{:?}",
        lost.note
    );
    assert_eq!(world.remote_ledger().entry(&lost.id).unwrap().uid, lost.uid);
    std::fs::remove_file(world.bare().join("hooks/reference-transaction")).unwrap();

    // Fault control: the connection dies before the ref moves; read-back
    // finds no binding, so the number is reported as not taken.
    world.hook("pre-receive", "kill -9 $PPID");
    let before = world.remote_ledger().tip.clone();
    match task(&a, "died before update") {
        Err(IdsError::Transport(reason)) => assert!(reason.contains("not taken"), "{reason}"),
        other => panic!("expected a transport error, got {other:?}"),
    }
    assert_eq!(world.remote_ledger().tip, before);
}

// --- AC-4: offline, sync and retarget ---------------------------------------

#[test]
fn offline_issue_is_pending_and_sync_publishes_or_names_retarget() {
    let world = World::new();
    let a = world.clone_as("a", "a@example.test");
    let b = world.clone_as("b", "b@example.test");
    task(&a, "first").unwrap();
    git(&b, &["fetch", "-q", "origin"]);
    let url = world.bare().to_str().unwrap().to_string();
    git(
        &b,
        &["remote", "set-url", "origin", "/nonexistent/remote.git"],
    );

    // Offline: a local commit, "pending, not unique yet".
    let pending = task(&b, "offline one").unwrap();
    assert_eq!(pending.standing, Standing::Pending);
    assert_eq!(pending.id.to_string(), "TSK-002");
    assert!(issue::has_pending(&b));
    // Control: nobody else took TSK-002, so sync publishes it as is.
    git(&b, &["remote", "set-url", "origin", &url]);
    let synced = issue::sync(&b).unwrap();
    assert_eq!(synced.published, vec![pending.id.clone()]);
    assert_eq!(
        world.remote_ledger().entry(&pending.id).unwrap().uid,
        pending.uid
    );
    assert!(!issue::has_pending(&b));
    // A second sync finds nothing and treats the same uid as reserved.
    assert!(issue::sync(&b).unwrap().published.is_empty());

    // Fault: B takes TSK-003 offline while A takes TSK-003 online.
    git(
        &b,
        &["remote", "set-url", "origin", "/nonexistent/remote.git"],
    );
    let clash = task(&b, "offline two").unwrap();
    assert_eq!(clash.id.to_string(), "TSK-003");
    let winner = task(&a, "online").unwrap();
    assert_eq!(winner.id.to_string(), "TSK-003");
    git(&b, &["remote", "set-url", "origin", &url]);
    match issue::sync(&b) {
        Err(IdsError::Clash(message)) => {
            assert!(
                message.contains("codeflow ids retarget TSK-003"),
                "{message}"
            );
        }
        other => panic!("expected a clash, got {other:?}"),
    }

    // Retarget renumbers B's unmerged record, rewrites links, records the
    // old id, and replaces the pending reservation.
    git(&b, &["checkout", "-q", "-b", "task/TSK-003-offline"]);
    write_record(&b, "TSK-003", Some(&clash.uid));
    let linking = b.join("project-management/tasks/TSK-002.md");
    std::fs::write(
        &linking,
        "---\nid: TSK-002\ndepends_on: [TSK-003]\n---\nsee TSK-003 and TSK-003-001\n",
    )
    .unwrap();
    commit_all(&b, "records");
    let moved = seed::retarget(&b, &RegId::parse("TSK-003").unwrap()).unwrap();
    assert_eq!(moved.to.to_string(), "TSK-004");
    assert_eq!(moved.standing, Standing::Reserved);
    let text = std::fs::read_to_string(b.join("project-management/tasks/TSK-004.md")).unwrap();
    assert!(
        text.contains("id: TSK-004") && text.contains("former_ids: [TSK-003]"),
        "{text}"
    );
    assert!(
        text.contains(&format!("uid: {}", clash.uid)),
        "the uid is kept"
    );
    assert!(!b.join("project-management/tasks/TSK-003.md").exists());
    let linked = std::fs::read_to_string(&linking).unwrap();
    assert!(
        linked.contains("[TSK-004]") && linked.contains("see TSK-004 and TSK-003-001"),
        "{linked}"
    );
    let ledger = world.remote_ledger();
    assert_eq!(ledger.entry(&moved.to).unwrap().uid, clash.uid);
    assert_eq!(
        ledger.entry(&RegId::parse("TSK-003").unwrap()).unwrap().uid,
        winner.uid
    );
    assert!(
        !issue::has_pending(&b),
        "the clashing pending reservation is gone"
    );
    issue::sync(&b).unwrap();
}

// --- AC-5: admission and crash between reserve and write -------------------

#[test]
fn admit_reserves_a_forks_number_or_the_next_one_and_never_reissues() {
    let world = World::new();
    let a = world.clone_as("maintainer", "maintainer@example.test");
    task(&a, "first").unwrap();
    let fork_uid = new_uid();
    let admitted = issue::admit(
        &a,
        &RegId::parse("TSK-050").unwrap(),
        &fork_uid,
        "fork",
        "main",
    )
    .unwrap();
    assert_eq!(admitted.reserved.to_string(), "TSK-050");
    let entry = world
        .remote_ledger()
        .entry(&admitted.reserved)
        .cloned()
        .unwrap();
    assert_eq!(entry.uid, fork_uid);
    assert_eq!(entry.issuer, "admit:maintainer@example.test");
    // Admitting again is idempotent.
    assert!(
        issue::admit(
            &a,
            &RegId::parse("TSK-050").unwrap(),
            &fork_uid,
            "fork",
            "main"
        )
        .unwrap()
        .already
    );
    // A second fork record claiming TSK-050 gets the next free number.
    let other = issue::admit(
        &a,
        &RegId::parse("TSK-050").unwrap(),
        &new_uid(),
        "fork 2",
        "main",
    )
    .unwrap();
    assert_eq!(other.reserved.to_string(), "TSK-051");
    // An ordinary issue continues after the admitted numbers.
    assert_eq!(task(&a, "after").unwrap().id.to_string(), "TSK-052");
}

#[test]
fn resume_writes_only_the_callers_own_reservation() {
    let world = World::new();
    let a = world.clone_as("a", "a@example.test");
    task(&a, "first").unwrap();
    let mut request = Request::issue(Kind::Tsk, "crashed", "main");
    request.resume = Some(serde_json::json!({ "title": "crashed" }));
    let reserved = issue::reserve(&a, &request).unwrap();
    // The process died before writing the record: resume finds it.
    let unwritten = issue::resume(&a, &reserved.id).unwrap();
    assert_eq!(unwritten.uid, reserved.uid);
    // Another issuer in the same clone is refused.
    git(&a, &["config", "user.email", "someone@example.test"]);
    let refused = issue::resume(&a, &reserved.id).unwrap_err().to_string();
    assert!(refused.contains("reserved by a@example.test"), "{refused}");
    git(&a, &["config", "user.email", "a@example.test"]);
    // A reservation that never landed is refused, and so is an unknown id.
    let git_a = Git::new(&a);
    state::update(&git_a, |state| {
        state.unwritten.push(state::Unwritten {
            id: "TSK-099".into(),
            uid: new_uid(),
            issuer: "a@example.test".into(),
            request: serde_json::json!({}),
        });
    })
    .unwrap();
    let never = issue::resume(&a, &RegId::parse("TSK-099").unwrap())
        .unwrap_err()
        .to_string();
    assert!(never.contains("not reserved"), "{never}");
    assert!(issue::resume(&a, &RegId::parse("TSK-098").unwrap()).is_err());
    issue::written(&a, &reserved.id).unwrap();
    assert!(
        issue::resume(&a, &reserved.id).is_err(),
        "written clears the pending write"
    );
}

// --- AC-7, AC-13: damage, the append-only range, and typed restore ---------

/// Push a commit that deletes `ids/TSK/<n>.toml` straight to the host.
fn push_deletion(world: &World, number: &str) {
    static COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = COUNT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let work = world.clone_as(&format!("vandal-{number}-{n}"), "vandal@example.test");
    git(&work, &["fetch", "-q", "origin", "codeflow/registry"]);
    git(&work, &["checkout", "-q", "-b", "reg", "FETCH_HEAD"]);
    git(&work, &["rm", "-q", &format!("ids/TSK/{number}.toml")]);
    git(&work, &["commit", "-q", "-m", "remove"]);
    git(&work, &["push", "-q", "origin", "reg:codeflow/registry"]);
}

#[test]
fn damage_refuses_issue_until_a_typed_restore_and_never_reissues_the_top() {
    let world = World::new();
    let a = world.clone_as("a", "a@example.test");
    task(&a, "one").unwrap();
    let top = task(&a, "two").unwrap();
    push_deletion(&world, "002");

    // History allocation: a deleted top is never reissued, and issue stops.
    match task(&a, "blocked") {
        Err(IdsError::Damaged(damage)) => {
            assert!(damage[0].contains("ids/TSK/002.toml"), "{damage:?}");
            assert!(
                damage[0].contains("codeflow ids restore TSK-002"),
                "{damage:?}"
            );
        }
        other => panic!("expected damage, got {other:?}"),
    }
    let git_a = Git::new(&a);
    let report = check::check(&git_a, None).unwrap();
    assert!(!report.passed());
    assert!(
        report
            .blocks
            .iter()
            .any(|b| b.starts_with("current damage:") && b.contains("deletes ids/TSK/002.toml")),
        "{:?}",
        report.blocks
    );
    // The binding stands in history even though the tip lost the file.
    let ledger = Ledger::read(&git_a, &registry_ref(&git_a).unwrap()).unwrap();
    assert_eq!(ledger.entry(&top.id).unwrap().uid, top.uid);

    let restored = issue::restore(&a, std::slice::from_ref(&top.id)).unwrap();
    assert_eq!(restored, vec![top.id.clone()]);
    let report = check::check(&git_a, None).unwrap();
    assert!(report.passed(), "{:?}", report.blocks);
    assert!(report.warns.iter().any(|w| w.starts_with("history (repaired):") && w.contains("deletes ids/TSK/002.toml")), "{:?}", report.warns);
    let next = task(&a, "healthy again").unwrap();
    assert_eq!(
        next.id.to_string(),
        "TSK-003",
        "the deleted top is not reissued"
    );
    // Restoring an intact file is refused as nothing to do.
    assert!(issue::restore(&a, &[top.id]).is_err());
}

#[test]
fn a_rebinding_restore_is_refused_by_the_ledger_rule() {
    let world = World::new();
    let a = world.clone_as("a", "a@example.test");
    let bound = task(&a, "one").unwrap();
    push_deletion(&world, "001");
    // A hand-made "restore" that re-adds TSK-001 with another uid.
    let work = world.clone_as("forger", "forger@example.test");
    git(&work, &["fetch", "-q", "origin", "codeflow/registry"]);
    git(&work, &["checkout", "-q", "-b", "reg", "FETCH_HEAD"]);
    let forged =
        codeflow_core::ids::Entry::issued(bound.id.clone(), new_uid(), "forged", "x", "main");
    std::fs::create_dir_all(work.join("ids/TSK")).unwrap();
    std::fs::write(work.join("ids/TSK/001.toml"), forged.render()).unwrap();
    git(&work, &["add", "."]);
    git(&work, &["commit", "-q", "-m", "restore: TSK-001"]);
    let ledger = Ledger::read(&Git::new(&work), "HEAD").unwrap();
    assert!(
        ledger.violations.iter().any(|v| v
            .message
            .contains("restore would bind TSK-001 to another uid")),
        "{:?}",
        ledger.violations
    );
    assert!(
        ledger.is_damaged(),
        "the tip still differs from the first addition"
    );
}

/// A registry work tree on the host's current tip, for hand-made commits
/// that bypass the hooks (the counterfeit a broken client could push).
fn forge_checkout(world: &World, name: &str) -> PathBuf {
    let work = world.clone_as(name, "forger@example.test");
    git(&work, &["fetch", "-q", "origin", "codeflow/registry"]);
    git(&work, &["checkout", "-q", "-b", "reg", "FETCH_HEAD"]);
    std::fs::create_dir_all(work.join("ids/TSK")).unwrap();
    work
}

fn forged_entry(number: &str) -> String {
    codeflow_core::ids::Entry::issued(
        RegId::parse(&format!("TSK-{number}")).unwrap(),
        new_uid(),
        "forged",
        "forger@example.test",
        "main",
    )
    .render()
}

#[test]
fn a_number_an_invalid_commit_introduced_is_never_issued_again() {
    let world = World::new();
    let a = world.clone_as("a", "a@example.test");
    task(&a, "one").unwrap();
    let work = forge_checkout(&world, "forger");
    // A counterfeit restore: it names TSK-002 but adds a file never added.
    std::fs::write(work.join("ids/TSK/002.toml"), forged_entry("002")).unwrap();
    git(&work, &["add", "."]);
    git(&work, &["commit", "-q", "-m", "restore: TSK-002"]);
    // A merge whose tree alone introduces TSK-003.
    git(&work, &["checkout", "-q", "-b", "side"]);
    git(&work, &["commit", "-q", "--allow-empty", "-m", "side"]);
    git(&work, &["checkout", "-q", "reg"]);
    git(&work, &["commit", "-q", "--allow-empty", "-m", "main line"]);
    git(&work, &["merge", "-q", "--no-ff", "--no-commit", "side"]);
    std::fs::write(work.join("ids/TSK/003.toml"), forged_entry("003")).unwrap();
    git(&work, &["add", "."]);
    git(&work, &["commit", "-q", "-m", "merge side"]);
    git(&work, &["push", "-q", "origin", "reg:codeflow/registry"]);

    let ledger = world.remote_ledger();
    for number in ["002", "003"] {
        let id = RegId::parse(&format!("TSK-{number}")).unwrap();
        assert!(
            ledger.holds(&id),
            "TSK-{number} is used although its commit broke the rule"
        );
    }
    assert_eq!(ledger.max_seq(Kind::Tsk), 3);
    assert!(
        ledger.violations.iter().any(|v| v
            .message
            .contains("ids/TSK/002.toml, which was never added")),
        "{:?}",
        ledger.violations
    );
    assert!(
        ledger
            .violations
            .iter()
            .any(|v| v.message.contains("merge commit")),
        "{:?}",
        ledger.violations
    );
    assert_eq!(
        task(&a, "after the counterfeit").unwrap().id.to_string(),
        "TSK-004",
        "neither number becomes issuable again"
    );
}

#[test]
fn a_first_uid_backfill_must_be_the_registry_binding() {
    let world = World::new();
    let a = world.clone_as("a", "a@example.test");
    write_record(&a, "TSK-001", None);
    commit_all(&a, "legacy record");
    git(&a, &["push", "-q", "origin", "main"]);
    seed::seed(&a, None).unwrap();
    let git_a = Git::new(&a);
    let bound = Ledger::read(&git_a, &registry_ref(&git_a).unwrap())
        .unwrap()
        .entry(&RegId::parse("TSK-001").unwrap())
        .unwrap()
        .uid
        .clone();

    // Control: a text edit keeps the uid-free legacy allowance.
    git(&a, &["checkout", "-q", "-b", "task/edit", "main"]);
    std::fs::write(
        a.join("project-management/tasks/TSK-001.md"),
        record_text("TSK-001", None) + "more\n",
    )
    .unwrap();
    commit_all(&a, "edit");
    let edit = check::merge_rule(&git_a, "main", "HEAD").unwrap();
    assert!(edit.passed(), "{:?}", edit.blocks);

    // The correct backfill binds.
    git(&a, &["checkout", "-q", "-b", "task/backfill", "main"]);
    write_record(&a, "TSK-001", Some(&bound));
    commit_all(&a, "backfill");
    let right = check::merge_rule(&git_a, "main", "HEAD").unwrap();
    assert!(right.passed(), "{:?}", right.blocks);
    assert!(
        right
            .info
            .iter()
            .any(|l| l == &format!("bound: TSK-001 -> {bound}")),
        "{:?}",
        right.info
    );

    // A wrong first uid blocks before it lands.
    git(&a, &["checkout", "-q", "-b", "task/wrong", "main"]);
    write_record(&a, "TSK-001", Some(&new_uid()));
    commit_all(&a, "wrong backfill");
    let wrong = check::merge_rule(&git_a, "main", "HEAD").unwrap();
    assert!(
        wrong
            .blocks
            .iter()
            .any(|b| b.contains("the registry binds it to uid") && b.contains(&bound)),
        "{:?}",
        wrong.blocks
    );
}

#[test]
fn quoted_record_paths_are_still_judged() {
    let world = World::new();
    let a = world.clone_as("a", "a@example.test");
    task(&a, "creates the registry").unwrap();
    git(&a, &["checkout", "-q", "-b", "task/names", "main"]);
    let names = [
        ("ADR-0040", "ADR-0040-caf\u{e9}.md"),
        ("ADR-0041", "ADR-0041-a\"quote.md"),
        ("ADR-0042", "ADR-0042-a\ttab.md"),
    ];
    std::fs::create_dir_all(a.join("docs/decisions")).unwrap();
    for (id, file) in names {
        std::fs::write(
            a.join("docs/decisions").join(file),
            record_text(id, Some(&new_uid())),
        )
        .unwrap();
    }
    commit_all(&a, "decisions with quoted names");
    let git_a = Git::new(&a);
    let report = check::merge_rule(&git_a, "main", "HEAD").unwrap();
    for (id, _) in names {
        assert!(
            report
                .blocks
                .iter()
                .any(|b| b.starts_with(&format!("{id}: not reserved"))),
            "{id}: {:?}",
            report.blocks
        );
    }
    assert_eq!(
        codeflow_core::ids::inventory::max_seq_on_refs(&git_a, Kind::Adr).unwrap(),
        42
    );
    let introduced = codeflow_core::ids::inventory::introductions(&git_a, "HEAD").unwrap();
    assert!(names
        .iter()
        .all(|(id, _)| introduced.contains_key(&RegId::parse(id).unwrap())));
    let next = issue::reserve(&a, &Request::issue(Kind::Adr, "next decision", "main")).unwrap();
    assert_eq!(next.id.to_string(), "ADR-0043");
}

// --- AC-8: rewrite detection on a host without rules ------------------------

#[test]
fn a_rewritten_registry_is_detected_against_the_last_verified_tip() {
    let world = World::new();
    let a = world.clone_as("a", "a@example.test");
    let first = task(&a, "one").unwrap();
    task(&a, "two").unwrap();
    // The host (no branch rules) is rewound to the first reservation.
    let first_commit = world
        .remote_ledger()
        .first_add(&first.id)
        .unwrap()
        .commit
        .clone();
    git(
        &world.bare(),
        &["update-ref", "refs/heads/codeflow/registry", &first_commit],
    );
    // A non-forced fetch refuses the rewind.
    match task(&a, "after rewind") {
        Err(IdsError::Rewritten(reason)) => {
            assert!(reason.contains("non-forced fetch"), "{reason}");
        }
        other => panic!("expected rewrite detection, got {other:?}"),
    }
    // Even after a forced fetch moves the tracking ref, the last verified
    // tip catches it.
    git(
        &a,
        &[
            "fetch",
            "-q",
            "origin",
            "+refs/heads/codeflow/registry:refs/remotes/origin/codeflow/registry",
        ],
    );
    match task(&a, "after forced fetch") {
        Err(IdsError::Rewritten(reason)) => {
            assert!(reason.contains("last verified tip"), "{reason}");
        }
        other => panic!("expected rewrite detection, got {other:?}"),
    }
}

// --- AC-6: the merge rule and the uniqueness scan --------------------------

#[test]
fn the_merge_rule_binds_added_records_and_the_scan_catches_other_lines() {
    let world = World::new();
    let a = world.clone_as("a", "a@example.test");
    let bound = task(&a, "bound").unwrap();
    git(&a, &["checkout", "-q", "-b", "integration/one"]);
    git(&a, &["push", "-q", "origin", "integration/one"]);
    git(&a, &["checkout", "-q", "-b", "task/TSK-001-bound"]);
    write_record(&a, "TSK-001", Some(&bound.uid));
    commit_all(&a, "bound record");
    let git_a = Git::new(&a);
    let pass = check::merge_rule(&git_a, "integration/one", "HEAD").unwrap();
    assert!(pass.passed(), "{:?}", pass.blocks);
    assert!(pass
        .info
        .iter()
        .any(|line| line == &format!("bound: TSK-001 -> {}", bound.uid)));

    // The registry moves between the check and the merge: still bound.
    task(&a, "moves the tip").unwrap();
    assert!(check::merge_rule(&git_a, "integration/one", "HEAD")
        .unwrap()
        .passed());

    // An unbound record (a fork) blocks and names admission.
    write_record(&a, "TSK-040", Some(&new_uid()));
    commit_all(&a, "fork record");
    let blocked = check::merge_rule(&git_a, "integration/one", "HEAD").unwrap();
    assert!(
        blocked
            .blocks
            .iter()
            .any(|b| b.contains("TSK-040: not reserved") && b.contains("codeflow ids admit")),
        "{:?}",
        blocked.blocks
    );
    git(&a, &["reset", "-q", "--hard", "HEAD~1"]);

    // An edited uid blocks.
    git(&a, &["checkout", "-q", "integration/one"]);
    git(
        &a,
        &["merge", "-q", "--no-ff", "-m", "land", "task/TSK-001-bound"],
    );
    git(&a, &["push", "-q", "origin", "integration/one"]);
    git(&a, &["checkout", "-q", "-b", "task/edit-uid"]);
    let path = a.join("project-management/tasks/TSK-001.md");
    let text = std::fs::read_to_string(&path)
        .unwrap()
        .replace(&bound.uid, &new_uid());
    std::fs::write(&path, text).unwrap();
    commit_all(&a, "edit uid");
    let edited = check::merge_rule(&git_a, "integration/one", "HEAD").unwrap();
    assert!(
        edited
            .blocks
            .iter()
            .any(|b| b.contains("edits the uid of an existing record")),
        "{:?}",
        edited.blocks
    );

    // Two integration lines take the same number with different uids: the
    // second is refused by the binding and by the scan.
    git(&a, &["checkout", "-q", "-b", "integration/two", "main"]);
    write_record(&a, "TSK-001", Some(&new_uid()));
    commit_all(&a, "same number, other record");
    let second = check::merge_rule(&git_a, "main", "HEAD").unwrap();
    assert!(
        second
            .blocks
            .iter()
            .any(|b| b.contains("the registry binds it to uid")),
        "{:?}",
        second.blocks
    );
    assert!(
        second
            .blocks
            .iter()
            .any(|b| b.starts_with("uniqueness: TSK-001 also exists on integration/one")),
        "{:?}",
        second.blocks
    );
}

#[test]
fn the_uniqueness_scan_holds_without_a_registry() {
    let world = World::new();
    let a = world.clone_as("a", "a@example.test");
    git(&a, &["checkout", "-q", "-b", "task/one"]);
    write_record(&a, "TSK-007", Some(&new_uid()));
    commit_all(&a, "one");
    git(&a, &["push", "-q", "origin", "task/one"]);
    git(&a, &["checkout", "-q", "-b", "task/two", "main"]);
    write_record(&a, "TSK-007", Some(&new_uid()));
    commit_all(&a, "two");
    let git_a = Git::new(&a);
    let report = check::merge_rule(&git_a, "main", "HEAD").unwrap();
    assert!(
        report
            .blocks
            .iter()
            .any(|b| b.contains("no `codeflow/registry` was fetched")),
        "{:?}",
        report.blocks
    );
    assert!(
        report
            .blocks
            .iter()
            .any(|b| b.starts_with("uniqueness: TSK-007 also exists on task/one")),
        "{:?}",
        report.blocks
    );
    // Control: the same record on a stacked branch is not a collision.
    git(&a, &["checkout", "-q", "-b", "task/stacked", "task/one"]);
    std::fs::write(a.join("notes.md"), "more\n").unwrap();
    commit_all(&a, "stacked");
    let stacked = check::merge_rule(&git_a, "main", "HEAD").unwrap();
    assert!(
        !stacked
            .blocks
            .iter()
            .any(|b| b.contains("exists on task/one")),
        "{:?}",
        stacked.blocks
    );
}

// --- AC-9, AC-10: seed, backfill and admission of legacy and mapped ids ----

/// A repository whose records exercise every seed decision: a landed
/// record, a legacy nested id, a merged branch (replica), a cherry-pick, a
/// squash, and two unrelated unlanded records sharing an id. Returns the
/// maintainer clone and the squash branch's introducing commit.
fn seed_fixture(world: &World) -> (PathBuf, String, String) {
    let m = world.clone_as("maintainer", "maintainer@example.test");
    // A landed record, a legacy nested id, and a record merged from a branch.
    write_record(&m, "TSK-001", None);
    let legacy = m.join("project-management/epics/EPC-002/tasks/TSK-002-001.md");
    std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
    std::fs::write(&legacy, record_text("TSK-002-001", None)).unwrap();
    commit_all(&m, "landed records");
    git(&m, &["checkout", "-q", "-b", "task/replica"]);
    write_record(&m, "TSK-003", None);
    let replica_intro = commit_all(&m, "replica");
    git(&m, &["checkout", "-q", "main"]);
    git(
        &m,
        &[
            "merge",
            "-q",
            "--no-ff",
            "-m",
            "merge replica",
            "task/replica",
        ],
    );
    // A cherry-picked record: identical file, different commit.
    git(&m, &["checkout", "-q", "-b", "task/picked", "main~1"]);
    write_record(&m, "TSK-004", None);
    let picked = commit_all(&m, "picked");
    git(&m, &["checkout", "-q", "main"]);
    git(&m, &["cherry-pick", "-x", &picked]);
    // A squash: the branch copy differs from what landed.
    git(&m, &["checkout", "-q", "-b", "task/squashed", "main"]);
    write_record(&m, "TSK-005", None);
    let squash_intro = commit_all(&m, "squash source");
    std::fs::write(m.join("extra.md"), "x\n").unwrap();
    commit_all(&m, "more");
    git(&m, &["checkout", "-q", "main"]);
    std::fs::write(
        m.join("project-management/tasks/TSK-005.md"),
        record_text("TSK-005", None) + "edited in the squash\n",
    )
    .unwrap();
    commit_all(&m, "squash");
    // Two unrelated, unlanded records that share an id.
    git(&m, &["checkout", "-q", "-b", "task/x", "main"]);
    write_record(&m, "TSK-006", None);
    commit_all(&m, "x");
    git(&m, &["checkout", "-q", "-b", "task/y", "main"]);
    std::fs::write(
        m.join("project-management/tasks/TSK-006.md"),
        record_text("TSK-006", None).replace("record", "other"),
    )
    .unwrap();
    commit_all(&m, "y");
    git(&m, &["checkout", "-q", "main"]);
    git(&m, &["push", "-q", "origin", "--all"]);

    (m, squash_intro, replica_intro)
}

#[test]
#[allow(clippy::too_many_lines)] // one journey: seed, stop, map, CI before backfill, backfill
fn seed_decides_replicas_by_commit_stops_on_the_undecidable_and_accepts_a_map() {
    let world = World::new();
    let (m, squash_intro, replica_intro) = seed_fixture(&world);
    // Seed stops, names both undecidable ids, and writes nothing.
    let stop = seed::seed(&m, None).unwrap_err().to_string();
    assert!(
        stop.contains("TSK-005") && stop.contains("TSK-006"),
        "{stop}"
    );
    assert!(
        !stop.contains("TSK-003") && !stop.contains("TSK-004"),
        "replica and pick are decided: {stop}"
    );
    assert!(world.remote_ledger().tip.is_none(), "nothing was written");

    // The unrelated pair is retargeted away by deleting one line; the squash
    // is mapped explicitly.
    git(&m, &["push", "-q", "origin", "--delete", "task/y"]);
    git(&m, &["branch", "-q", "-D", "task/y"]);
    git(&m, &["fetch", "-q", "--prune", "origin"]);
    let main_intro = git(
        &m,
        &[
            "log",
            "--format=%H",
            "-1",
            "--diff-filter=A",
            "main",
            "--",
            "project-management/tasks/TSK-005.md",
        ],
    );
    let map = SeedMap::parse(&format!(
        "[ids.\"TSK-005\"]\ncopies = [\"{squash_intro}\", \"{main_intro}\"]\n"
    ))
    .unwrap();
    let seeded = seed::seed(&m, Some(&map)).unwrap();
    assert_eq!(seeded.registered.len(), 6, "{:?}", seeded.registered);
    let ledger = world.remote_ledger();
    let legacy_id = RegId::parse("TSK-002-001").unwrap();
    assert!(
        ledger.entry(&legacy_id).is_some(),
        "legacy id registered at ids/TSK/002-001.toml"
    );
    assert_eq!(
        ledger.max_seq(Kind::Tsk),
        6,
        "the legacy id never raises the high-water mark"
    );
    let squash = ledger.entry(&RegId::parse("TSK-005").unwrap()).unwrap();
    assert_eq!(squash.mapped, vec![squash_intro.clone()]);
    assert!(
        squash.mapped_by.starts_with("maintainer@example.test "),
        "{}",
        squash.mapped_by
    );
    let replica = ledger.entry(&RegId::parse("TSK-003").unwrap()).unwrap();
    assert!(
        replica.introduced.starts_with(&replica_intro),
        "{}",
        replica.introduced
    );
    assert_eq!(
        replica.landed, replica_intro,
        "a merged branch commit landed as itself"
    );
    // Seeding is idempotent.
    assert!(seed::seed(&m, None).unwrap().registered.is_empty());
    // Allocation continues after every seeded id.
    assert_eq!(task(&m, "next").unwrap().id.to_string(), "TSK-007");

    // A fresh checkout before backfill: CI admits the mapped squash copy by
    // provenance alone.
    let fresh = world.clone_as("ci", "ci@example.test");
    git(
        &fresh,
        &[
            "fetch",
            "-q",
            "origin",
            "refs/heads/codeflow/registry:refs/remotes/origin/codeflow/registry",
        ],
    );
    let git_ci = Git::new(&fresh);
    let squash_pr = check::merge_rule(&git_ci, "origin/main", "origin/task/squashed").unwrap();
    assert!(squash_pr.passed(), "{:?}", squash_pr.blocks);
    assert!(
        squash_pr
            .info
            .iter()
            .any(|l| l.contains("bound by provenance: TSK-005")),
        "{:?}",
        squash_pr.info
    );
    let reconcile = check::check(&git_ci, None).unwrap();
    assert!(reconcile.passed(), "{:?}", reconcile.blocks);

    // Fault: a uid-less TSK-005 written independently, from before the
    // squash landed, matches no introduced, mapped or landed commit.
    let before = format!("{main_intro}^");
    git(&fresh, &["checkout", "-q", "-b", "task/impostor", &before]);
    std::fs::write(
        fresh.join("project-management/tasks/TSK-005.md"),
        record_text("TSK-005", None).replace("record", "impostor"),
    )
    .unwrap();
    commit_all(&fresh, "impostor");
    let impostor = check::merge_rule(&git_ci, &before, "HEAD").unwrap();
    assert!(
        impostor
            .blocks
            .iter()
            .any(|b| b.contains("TSK-005") && b.contains("provenance does not match")),
        "{:?}",
        impostor.blocks
    );
    git(&fresh, &["checkout", "-q", "main"]);

    // Backfill copies registered uids into this line's records and never
    // generates one.
    git(&fresh, &["checkout", "-q", "main"]);
    let filled = seed::backfill(&fresh).unwrap();
    assert!(filled.refused.is_empty(), "{:?}", filled.refused);
    assert_eq!(filled.written.len(), 5, "{:?}", filled.written);
    let text = std::fs::read_to_string(fresh.join("project-management/tasks/TSK-005.md")).unwrap();
    assert!(text.contains(&format!("uid: {}", squash.uid)), "{text}");
    let legacy_text = std::fs::read_to_string(
        fresh.join("project-management/epics/EPC-002/tasks/TSK-002-001.md"),
    )
    .unwrap();
    assert!(legacy_text.contains(&format!("uid: {}", ledger.entry(&legacy_id).unwrap().uid)));
    assert!(
        seed::backfill(&fresh).unwrap().written.is_empty(),
        "idempotent"
    );
}

#[test]
fn seed_creates_an_orphan_root_that_adds_only_ids_files() {
    let world = World::new();
    let m = world.clone_as("maintainer", "maintainer@example.test");
    write_record(&m, "TSK-010", None);
    commit_all(&m, "record");
    git(&m, &["push", "-q", "origin", "main"]);
    seed::seed(&m, None).unwrap();
    let bare = world.bare();
    let roots = git(&bare, &["rev-list", "--max-parents=0", "codeflow/registry"]);
    assert_eq!(roots.lines().count(), 1);
    let files = git(
        &bare,
        &["ls-tree", "-r", "--name-only", "codeflow/registry"],
    );
    assert_eq!(files, "ids/TSK/010.toml");
    let unrelated = Command::new("git")
        .args(["merge-base", "main", "codeflow/registry"])
        .current_dir(&bare)
        .output()
        .unwrap();
    assert!(
        !unrelated.status.success(),
        "the registry shares no history with code"
    );
    let ledger = world.remote_ledger();
    assert!(ledger.violations.is_empty() && !ledger.is_damaged());
}

// --- AC-11 (library part): a no-remote repository ---------------------------

#[test]
fn a_repository_without_a_remote_keeps_its_own_registry() {
    setup();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["config", "user.email", "solo@example.test"]);
    git(root, &["config", "user.name", "solo"]);
    write_record(root, "TSK-004", None);
    commit_all(root, "existing record");
    // The first issue seeds the local registry, then continues after it.
    let issued = task(root, "local").unwrap();
    assert_eq!(issued.standing, Standing::Local);
    assert_eq!(issued.id.to_string(), "TSK-005");
    let ledger = Ledger::read(&Git::new(root), REGISTRY_REF).unwrap();
    assert!(ledger.entry(&RegId::parse("TSK-004").unwrap()).is_some());
    assert_eq!(ledger.entry(&issued.id).unwrap().uid, issued.uid);
    assert!(check::check(&Git::new(root), None).unwrap().passed());
}

// --- Property: allocation exceeds every number ever added ------------------

#[test]
fn property_issued_numbers_exceed_every_number_ever_added_and_never_repeat() {
    let world = World::new();
    let a = world.clone_as("a", "a@example.test");
    let b = world.clone_as("b", "b@example.test");
    task(&a, "root").unwrap();
    let mut issued: BTreeSet<u64> = BTreeSet::from([1]);
    let mut high = 1;
    // A small deterministic generator over adds, deletions with restore,
    // records on refs with high numbers, and interleaved issuers.
    let mut state_: u64 = 0x2545_F491_4F6C_DD1D;
    let mut next = || {
        state_ ^= state_ << 13;
        state_ ^= state_ >> 7;
        state_ ^= state_ << 17;
        state_
    };
    for step in 0..24 {
        match next() % 4 {
            0 | 1 => {
                let root = if next() % 2 == 0 { &a } else { &b };
                let reservation = task(root, &format!("p{step}")).unwrap();
                let n = seq(&reservation);
                assert!(n > high, "step {step}: issued {n} not above {high}");
                assert!(issued.insert(n), "step {step}: {n} reissued");
                high = n;
            }
            2 => {
                // A record on a pushed branch with a higher number.
                let jump = high + 1 + next() % 3;
                let id = format!("TSK-{jump:03}");
                git(
                    &a,
                    &["checkout", "-q", "-b", &format!("task/p{step}"), "main"],
                );
                write_record(&a, &id, Some(&new_uid()));
                commit_all(&a, "ref record");
                git(&a, &["push", "-q", "origin", &format!("task/p{step}")]);
                git(&a, &["checkout", "-q", "main"]);
                git(&b, &["fetch", "-q", "origin"]);
                high = high.max(jump);
            }
            _ => {
                // Delete the top reservation on the host, then restore it.
                let top = world.remote_ledger().max_seq(Kind::Tsk);
                if issued.contains(&top) {
                    push_deletion(&world, &format!("{top:03}"));
                    assert!(matches!(task(&a, "damaged"), Err(IdsError::Damaged(_))));
                    issue::restore(&b, &[RegId::canonical(Kind::Tsk, top)]).unwrap();
                }
            }
        }
    }
    assert!(
        issued.len() > 5,
        "the walk issued enough numbers: {issued:?}"
    );
}
