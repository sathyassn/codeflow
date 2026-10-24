//! The coordination ledger's store, transitions and due rules (TSK-422,
//! ADR-0086), through the library: the lock, sequence, sync, idempotency,
//! tail repair, damage, discovery, privacy, streams and the permission
//! matrix, and the due and fulfilment rules over a fake observer.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use codeflow_core::coord::actor::RecordedBy;
use codeflow_core::coord::due::{self, Observer, PullView, TaskView};
use codeflow_core::coord::fixture;
use codeflow_core::coord::reference::{Link, LinkKind, Target};
use codeflow_core::coord::schema::{
    Data, Drop, Evidence, EvidenceKind, FollowUp, ItemUpdate, Source,
};
use codeflow_core::coord::store::{self, Draft, Paths, ReadOutcome, SubjectId};
use codeflow_core::coord::transition::ItemState;
use codeflow_core::coord::{self as coord, CoordError};

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    git(dir.path(), &["config", "user.name", "t"]);
    git(dir.path(), &["config", "user.email", "t@example.test"]);
    git(
        dir.path(),
        &["commit", "-q", "--allow-empty", "-m", "chore: start"],
    );
    dir
}

fn paths(dir: &Path) -> Paths {
    Paths::discover(dir).unwrap().0
}

const WAIT: Duration = Duration::from_secs(10);

fn item(title: &str, stream: &str, holder: &str) -> FollowUp {
    FollowUp {
        title: title.into(),
        description: format!("{title}, described for a cold reader"),
        done_when: "the answer is recorded".into(),
        source: Source::Coordinator,
        source_reference: None,
        stream: stream.into(),
        holder: holder.into(),
        links: Vec::new(),
        no_link_reason: Some("nothing to link yet".into()),
        next_check: coord::format_time(coord::now_secs() + 600),
        schedule: None,
        waits_on: Vec::new(),
        awaiting_operator: None,
        rejected_suggestions: Vec::new(),
        observed: Vec::new(),
    }
}

fn add(paths: &Paths, by: &RecordedBy, record: FollowUp) -> Result<String, CoordError> {
    let committed = store::commit(paths, by, WAIT, |_| {
        Ok(vec![Draft::new(
            SubjectId::NextFollowUp,
            Data::ItemAdded(record),
        )])
    })?;
    Ok(committed.envelopes[0].subject_id.clone())
}

fn act(paths: &Paths, by: &RecordedBy, id: &str, data: Data) -> Result<(), CoordError> {
    store::commit(paths, by, WAIT, |_| {
        Ok(vec![Draft::new(SubjectId::Given(id.into()), data)])
    })
    .map(|_| ())
}

fn ledger(paths: &Paths) -> codeflow_core::coord::transition::Folded {
    match store::read(paths) {
        ReadOutcome::Ledger(snapshot) => snapshot.folded,
        other => panic!("expected a ledger, got {other:?}"),
    }
}

/// One stream with a coordinator `c1` and one lane seated by `s1`.
fn bound(dir: &Path) -> (Paths, RecordedBy, RecordedBy) {
    let paths = paths(dir);
    fixture::open_stream(&paths, "WS-001", "c1").unwrap();
    let lane = fixture::open_lane(&paths, "WS-001", "c1", "s1").unwrap();
    (
        paths,
        fixture::coordinator("WS-001", "c1", 1),
        fixture::lane(&lane, "s1", 1),
    )
}

fn asserted() -> Evidence {
    Evidence {
        kind: EvidenceKind::Asserted,
        reference: "commit:abc".into(),
        source: "asserted by the closer".into(),
        repository: None,
        target: None,
        revision: None,
        observed_at: coord::format_time(coord::now_secs()),
    }
}

#[test]
fn the_store_lives_in_the_common_dir_with_its_files_and_one_sequence() {
    let dir = repo();
    let (paths, lead, lane) = bound(dir.path());
    let common = Path::new(&git(dir.path(), &["rev-parse", "--git-common-dir"])).to_path_buf();
    let common = if common.is_absolute() {
        common
    } else {
        dir.path().join(common)
    };
    assert_eq!(
        paths.dir.canonicalize().unwrap(),
        common.join("codeflow/coord").canonicalize().unwrap()
    );
    let first = add(
        &paths,
        &lead,
        item("ask the operator", "WS-001", "coordinator"),
    )
    .unwrap();
    let second = add(&paths, &lane, item("rerun the gate", "WS-001", "LN-001")).unwrap();
    assert_eq!((first.as_str(), second.as_str()), ("FU-001", "FU-002"));
    for file in [&paths.events, &paths.counters, &paths.lock, &paths.instance] {
        assert!(file.is_file(), "{} missing", file.display());
    }
    assert!(paths.status.join("c1.json").is_file());
    assert!(paths.status.join("s1.json").is_file());
    let instance: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&paths.instance).unwrap()).unwrap();
    assert!(uuid::Uuid::parse_str(instance["instance_id"].as_str().unwrap()).is_ok());
    let text = std::fs::read_to_string(&paths.events).unwrap();
    let sequences: Vec<u64> = text
        .lines()
        .map(|line| {
            serde_json::from_str::<serde_json::Value>(line).unwrap()["sequence"]
                .as_u64()
                .unwrap()
        })
        .collect();
    assert_eq!(sequences, (1..=sequences.len() as u64).collect::<Vec<_>>());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&paths.dir), 0o700);
        assert_eq!(mode(&paths.events), 0o600);
        assert_eq!(mode(&paths.counters), 0o600);
    }
}

#[test]
fn counters_are_reserved_ahead_never_lowered_and_gaps_are_allowed() {
    let dir = repo();
    let (paths, lead, _) = bound(dir.path());
    std::fs::write(&paths.counters, r#"{"follow_up": 41, "lane": 1}"#).unwrap();
    assert_eq!(
        add(&paths, &lead, item("a", "WS-001", "coordinator")).unwrap(),
        "FU-042"
    );
    // A refused add after reservation leaves a gap, never a reused number.
    let mut bad = item("b", "WS-002", "coordinator");
    bad.stream = "WS-002".into();
    assert!(add(&paths, &lead, bad).is_err());
    std::fs::write(&paths.counters, r#"{"follow_up": 3, "lane": 1}"#).unwrap();
    assert_eq!(
        add(&paths, &lead, item("c", "WS-001", "coordinator")).unwrap(),
        "FU-043"
    );
}

#[test]
fn a_retried_operation_with_the_same_key_is_recorded_once() {
    let dir = repo();
    let (paths, lead, _) = bound(dir.path());
    let draft = || Draft {
        idempotency_key: Some("op-1".into()),
        ..Draft::new(
            SubjectId::NextFollowUp,
            Data::ItemAdded(item("once", "WS-001", "coordinator")),
        )
    };
    let first = store::commit(&paths, &lead, WAIT, |_| Ok(vec![draft()])).unwrap();
    let again = store::commit(&paths, &lead, WAIT, |_| Ok(vec![draft()])).unwrap();
    assert!(again.envelopes.is_empty());
    assert_eq!(again.duplicate_of, Some(first.envelopes[0].sequence));
    assert_eq!(ledger(&paths).ledger.items.len(), 1);
}

#[test]
fn a_torn_tail_is_quarantined_before_the_next_append() {
    let dir = repo();
    let (paths, lead, _) = bound(dir.path());
    add(&paths, &lead, item("a", "WS-001", "coordinator")).unwrap();
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&paths.events)
        .unwrap();
    std::io::Write::write_all(&mut file, br#"{"schema_version":1,"event":"item_add"#).unwrap();
    // A reader sees the committed prefix and is not read-only.
    let read = ledger(&paths);
    assert!(read.damage.is_none());
    assert!(read.tail > 0);
    let committed = store::commit(&paths, &lead, WAIT, |_| {
        Ok(vec![Draft::new(
            SubjectId::NextFollowUp,
            Data::ItemAdded(item("b", "WS-001", "coordinator")),
        )])
    })
    .unwrap();
    let kept = committed
        .repaired_tail
        .expect("the torn tail was moved aside");
    assert!(std::fs::read(&kept)
        .unwrap()
        .starts_with(b"{\"schema_version\""));
    let read = ledger(&paths);
    assert_eq!((read.tail, read.damage), (0, None));
    assert_eq!(read.ledger.items.len(), 2);
}

fn plant(paths: &Paths, edit: impl FnOnce(Vec<String>) -> Vec<String>) {
    let text = std::fs::read_to_string(&paths.events).unwrap();
    let lines = edit(text.lines().map(str::to_string).collect());
    std::fs::write(&paths.events, lines.join("\n") + "\n").unwrap();
}

fn assert_read_only(paths: &Paths, lead: &RecordedBy, needle: &str) {
    let read = ledger(paths);
    let damage = read.damage.expect("the ledger is read-only");
    assert!(damage.contains(needle), "{damage}");
    let error = add(paths, lead, item("blocked", "WS-001", "coordinator")).unwrap_err();
    assert!(matches!(error, CoordError::ReadOnly(_)), "{error:?}");
}

#[test]
fn interior_damage_makes_the_ledger_read_only_and_repair_quarantines_the_suffix() {
    let dir = repo();
    let (paths, lead, _) = bound(dir.path());
    add(&paths, &lead, item("a", "WS-001", "coordinator")).unwrap();
    add(&paths, &lead, item("b", "WS-001", "coordinator")).unwrap();
    plant(&paths, |mut lines| {
        lines.insert(lines.len() - 1, "not json".into());
        lines
    });
    assert_read_only(&paths, &lead, "not JSON");
    let repaired = store::repair(&paths, WAIT).unwrap();
    assert!(repaired.quarantined.unwrap().is_file());
    let read = ledger(&paths);
    assert!(read.damage.is_none());
    add(&paths, &lead, item("c", "WS-001", "coordinator")).unwrap();
}

#[test]
fn a_duplicate_or_decreasing_sequence_makes_the_ledger_read_only() {
    for shift in [0_i64, -2] {
        let dir = repo();
        let (paths, lead, _) = bound(dir.path());
        add(&paths, &lead, item("a", "WS-001", "coordinator")).unwrap();
        plant(&paths, |mut lines| {
            let mut last: serde_json::Value = serde_json::from_str(lines.last().unwrap()).unwrap();
            let sequence = last["sequence"].as_i64().unwrap() + shift;
            last["sequence"] = sequence.into();
            last["subject_id"] = "FU-009".into();
            lines.push(last.to_string());
            lines
        });
        assert_read_only(&paths, &lead, "sequence");
    }
}

#[test]
fn a_broken_transition_or_a_newer_schema_makes_the_ledger_read_only() {
    let dir = repo();
    let (paths, lead, _) = bound(dir.path());
    let id = add(&paths, &lead, item("a", "WS-001", "coordinator")).unwrap();
    act(&paths, &lead, &id, Data::ItemClosed(asserted())).unwrap();
    // A second close of a closed item, planted past the rules.
    plant(&paths, |mut lines| {
        let mut last: serde_json::Value = serde_json::from_str(lines.last().unwrap()).unwrap();
        last["sequence"] = (last["sequence"].as_u64().unwrap() + 1).into();
        lines.push(last.to_string());
        lines
    });
    assert_read_only(&paths, &lead, "reopen it first");

    let dir = repo();
    let (paths, lead, _) = bound(dir.path());
    plant(&paths, |mut lines| {
        let mut last: serde_json::Value = serde_json::from_str(lines.last().unwrap()).unwrap();
        last["sequence"] = (last["sequence"].as_u64().unwrap() + 1).into();
        last["schema_version"] = 2.into();
        lines.push(last.to_string());
        lines
    });
    assert_read_only(&paths, &lead, "schema_version 2 is newer");
}

#[test]
fn a_malformed_event_is_refused_on_input_and_on_replay() {
    let dir = repo();
    let (paths, lead, _) = bound(dir.path());
    let mut bad = item("a", "WS-001", "coordinator");
    bad.no_link_reason = None;
    let error = add(&paths, &lead, bad).unwrap_err();
    assert!(
        format!("{error}").contains("typed link or a no_link_reason"),
        "{error}"
    );
    let id = add(&paths, &lead, item("a", "WS-001", "coordinator")).unwrap();
    plant(&paths, |mut lines| {
        let mut last: serde_json::Value = serde_json::from_str(lines.last().unwrap()).unwrap();
        last["sequence"] = (last["sequence"].as_u64().unwrap() + 1).into();
        last["event"] = "item_updated".into();
        last["subject_id"] = id.clone().into();
        last["data"] = serde_json::json!({"source": "operator"});
        lines.push(last.to_string());
        lines
    });
    assert_read_only(&paths, &lead, "unknown field `source`");
}

#[test]
fn a_held_lock_reports_busy_after_the_bounded_wait() {
    use fs2::FileExt;
    let dir = repo();
    let (paths, lead, _) = bound(dir.path());
    let holder = std::fs::OpenOptions::new()
        .write(true)
        .open(&paths.lock)
        .unwrap();
    holder.lock_exclusive().unwrap();
    let started = std::time::Instant::now();
    let error = store::commit(
        &paths,
        &lead,
        Duration::from_millis(300),
        |_| Ok(Vec::new()),
    )
    .unwrap_err();
    assert_eq!(error, CoordError::Busy);
    assert!(started.elapsed() >= Duration::from_millis(300));
    // A reader is not blocked by the writer's lock and reports the ledger,
    // which is distinct from busy and from empty.
    assert!(matches!(store::read(&paths), ReadOutcome::Ledger(_)));
    FileExt::unlock(&holder).unwrap();
    assert!(store::commit(&paths, &lead, WAIT, |_| Ok(Vec::new())).is_ok());
}

#[test]
fn concurrent_adds_of_one_source_identity_record_exactly_one() {
    let dir = repo();
    let (paths, lead, _) = bound(dir.path());
    let handles: Vec<_> = (0..4)
        .map(|n| {
            let paths = paths.clone();
            let lead = lead.clone();
            std::thread::spawn(move || {
                let mut record = item(&format!("same ask {n}"), "WS-001", "coordinator");
                record.source = Source::Review;
                record.source_reference = Some("review-7 finding 3".into());
                add(&paths, &lead, record)
            })
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(
        results.iter().filter(|r| r.is_ok()).count(),
        1,
        "{results:?}"
    );
    for error in results.iter().filter_map(|r| r.as_ref().err()) {
        assert!(format!("{error}").contains("exact duplicate"), "{error}");
    }
    // A similar item with another identity is accepted.
    let mut similar = item("same ask again", "WS-001", "coordinator");
    similar.source_reference = Some("review-7 finding 4".into());
    add(&paths, &lead, similar).unwrap();
}

#[test]
fn a_linked_worktree_and_a_relocated_git_dir_share_their_clones_ledger() {
    let dir = repo();
    let linked = dir.path().join("wt");
    git(
        dir.path(),
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "side",
            linked.to_str().unwrap(),
        ],
    );
    let (main_paths, lead, _) = bound(dir.path());
    add(&main_paths, &lead, item("shared", "WS-001", "coordinator")).unwrap();
    assert_eq!(paths(&linked), main_paths);
    assert_eq!(ledger(&paths(&linked)).ledger.items.len(), 1);

    let outer = tempfile::tempdir().unwrap();
    let work = outer.path().join("work");
    let gitdir = outer.path().join("elsewhere.git");
    std::fs::create_dir(&work).unwrap();
    git(
        &work,
        &["init", "-q", "--separate-git-dir", gitdir.to_str().unwrap()],
    );
    let relocated = paths(&work);
    assert_eq!(
        relocated
            .dir
            .canonicalize()
            .unwrap_or(relocated.dir.clone())
            .parent()
            .map(Path::to_path_buf),
        Some(gitdir.canonicalize().unwrap().join("codeflow"))
    );
    fixture::open_stream(&relocated, "WS-001", "c1").unwrap();
    assert!(gitdir.join("codeflow/coord/events.jsonl").is_file());
    assert!(!work.join("codeflow").exists());
}

#[cfg(unix)]
#[test]
fn a_read_only_ledger_reads_and_reports_a_write_as_unwritten() {
    use std::os::unix::fs::PermissionsExt;
    let dir = repo();
    let (paths, lead, _) = bound(dir.path());
    add(&paths, &lead, item("a", "WS-001", "coordinator")).unwrap();
    std::fs::set_permissions(&paths.events, std::fs::Permissions::from_mode(0o400)).unwrap();
    std::fs::set_permissions(&paths.dir, std::fs::Permissions::from_mode(0o500)).unwrap();
    let read = ledger(&paths);
    assert_eq!(read.ledger.items.len(), 1);
    let error = add(&paths, &lead, item("b", "WS-001", "coordinator")).unwrap_err();
    std::fs::set_permissions(&paths.dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::set_permissions(&paths.events, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(matches!(error, CoordError::Unwritten(_)), "{error:?}");
    // A denied read is unavailable, distinct from empty.
    std::fs::set_permissions(&paths.events, std::fs::Permissions::from_mode(0o000)).unwrap();
    let outcome = store::read(&paths);
    std::fs::set_permissions(&paths.events, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(
        matches!(outcome, ReadOutcome::Unavailable(_)),
        "{outcome:?}"
    );
}

#[cfg(unix)]
#[test]
fn a_symlinked_ledger_directory_is_refused() {
    let dir = repo();
    let paths = paths(dir.path());
    let elsewhere = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(paths.dir.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(elsewhere.path(), &paths.dir).unwrap();
    let error = fixture::open_stream(&paths, "WS-001", "c1").unwrap_err();
    assert!(format!("{error}").contains("symbolic link"), "{error}");
    assert!(std::fs::read_dir(elsewhere.path())
        .unwrap()
        .next()
        .is_none());
    assert!(matches!(store::read(&paths), ReadOutcome::Unavailable(_)));
}

#[test]
fn no_ledger_reads_as_empty_and_a_read_creates_nothing() {
    let dir = repo();
    let paths = paths(dir.path());
    assert!(matches!(store::read(&paths), ReadOutcome::Empty));
    assert!(!paths.dir.exists());
    assert!(!paths.dir.parent().unwrap().exists());
}

#[test]
fn the_permission_matrix_holds_for_every_action() {
    let dir = repo();
    let (paths, lead, lane) = bound(dir.path());
    let other_lane_id = fixture::open_lane(&paths, "WS-001", "c1", "s2").unwrap();
    let other = fixture::lane(&other_lane_id, "s2", 1);
    let mine = add(&paths, &lane, item("lane item", "WS-001", "LN-001")).unwrap();
    // Any bound actor adds; only the holder updates, checks, hands over, closes.
    for data in [
        Data::ItemUpdated(ItemUpdate {
            title: Some("x".into()),
            ..ItemUpdate::default()
        }),
        Data::ItemClosed(asserted()),
    ] {
        let error = act(&paths, &other, &mine, data).unwrap_err();
        assert!(format!("{error}").contains("only the holder"), "{error}");
    }
    // The stream's coordinator may drop with a reason; a drop needs one.
    let error = act(
        &paths,
        &lead,
        &mine,
        Data::ItemDropped(Drop {
            reason: " ".into(),
            disposition: "none".into(),
        }),
    )
    .unwrap_err();
    assert!(format!("{error}").contains("reason"), "{error}");
    act(
        &paths,
        &lead,
        &mine,
        Data::ItemDropped(Drop {
            reason: "superseded".into(),
            disposition: "nothing left".into(),
        }),
    )
    .unwrap();
    // A stale generation or a replaced seat is refused.
    let stale = fixture::lane("LN-001", "s1", 2);
    let error = add(&paths, &stale, item("stale", "WS-001", "LN-001")).unwrap_err();
    assert!(format!("{error}").contains("stale"), "{error}");
    let impostor = fixture::coordinator("WS-001", "c9", 1);
    assert!(add(&paths, &impostor, item("x", "WS-001", "coordinator")).is_err());
    // An unbound role cannot write at all.
    let unbound = fixture::lane("LN-099", "s9", 1);
    assert!(add(&paths, &unbound, item("x", "WS-001", "coordinator")).is_err());
}

#[test]
fn streams_hold_their_items_and_a_cross_stream_wait_is_read_only() {
    let dir = repo();
    let (paths, lead_one, _) = bound(dir.path());
    fixture::open_stream(&paths, "WS-002", "c2").unwrap();
    let lead_two = fixture::coordinator("WS-002", "c2", 1);
    let theirs = add(&paths, &lead_two, item("theirs", "WS-002", "coordinator")).unwrap();
    // An operator ask goes to the intake list until a coordinator takes it.
    let ask = add(
        &paths,
        &fixture::operator(),
        item("operator ask", "intake", "coordinator"),
    )
    .unwrap();
    let mut waiting = item("waits on theirs", "WS-001", "coordinator");
    waiting.waits_on = vec![theirs.clone()];
    let mine = add(&paths, &lead_one, waiting).unwrap();
    // Waiting by id never grants a write to the other stream's item.
    let error = act(&paths, &lead_one, &theirs, Data::ItemClosed(asserted())).unwrap_err();
    assert!(format!("{error}").contains("only the holder"), "{error}");
    // A stream id that no fixture opened is refused at the store.
    let error = add(&paths, &lead_one, item("nowhere", "WS-003", "coordinator")).unwrap_err();
    assert!(format!("{error}").contains("WS-003 is not open"), "{error}");
    // A coordinator takes the intake item into its stream.
    act(
        &paths,
        &lead_one,
        &ask,
        Data::ItemHandedOver(codeflow_core::coord::schema::HandOver {
            to_holder: "coordinator".into(),
            to_stream: Some("WS-001".into()),
        }),
    )
    .unwrap();
    let folded = ledger(&paths).ledger;
    assert_eq!(folded.items[&ask].record.stream, "WS-001");
    assert_eq!(folded.items[&mine].record.waits_on, vec![theirs]);
}

#[test]
fn an_update_that_introduces_a_cycle_is_refused() {
    let dir = repo();
    let (paths, lead, _) = bound(dir.path());
    let first = add(&paths, &lead, item("first", "WS-001", "coordinator")).unwrap();
    let mut second = item("second", "WS-001", "coordinator");
    second.waits_on = vec![first.clone()];
    let second = add(&paths, &lead, second).unwrap();
    let error = act(
        &paths,
        &lead,
        &first,
        Data::ItemUpdated(ItemUpdate {
            links: Some(vec![Link {
                kind: LinkKind::Prerequisite,
                target: Target::FollowUp { id: second },
            }]),
            ..ItemUpdate::default()
        }),
    )
    .unwrap_err();
    assert!(format!("{error}").contains("cycle"), "{error}");
}

#[test]
fn a_stale_expected_revision_is_refused() {
    let dir = repo();
    let (paths, lead, _) = bound(dir.path());
    let id = add(&paths, &lead, item("a", "WS-001", "coordinator")).unwrap();
    let update = |revision| {
        store::commit(&paths, &lead, WAIT, |_| {
            Ok(vec![Draft {
                expected_revision: Some(revision),
                ..Draft::new(
                    SubjectId::Given(id.clone()),
                    Data::ItemUpdated(ItemUpdate {
                        title: Some("b".into()),
                        ..ItemUpdate::default()
                    }),
                )
            }])
        })
    };
    update(1).unwrap();
    let error = update(1).unwrap_err();
    assert!(
        format!("{error}").contains("changed since it was read"),
        "{error}"
    );
}

#[test]
fn purge_removes_an_items_text_from_the_live_log() {
    let dir = repo();
    let (paths, lead, _) = bound(dir.path());
    let mut record = item("private matter", "WS-001", "coordinator");
    record.description = "a private detail to purge".into();
    let id = add(&paths, &lead, record).unwrap();
    add(&paths, &lead, item("kept", "WS-001", "coordinator")).unwrap();
    let purged = store::purge(&paths, &lead, &id, WAIT).unwrap();
    assert_eq!(purged.lines, 1);
    let text = std::fs::read_to_string(&paths.events).unwrap();
    assert!(!text.contains("private detail"));
    assert!(!text.contains("private matter"));
    let folded = ledger(&paths);
    assert!(folded.damage.is_none());
    assert_eq!(folded.ledger.items[&id].record.title, "[purged]");
    assert_eq!(folded.ledger.items.len(), 2);
}

#[test]
fn a_secret_in_any_free_text_field_is_refused_without_echo() {
    let dir = repo();
    let (paths, lead, _) = bound(dir.path());
    let token = format!("ghp_{}", "z".repeat(36));
    let mut record = item("a", "WS-001", "coordinator");
    record.done_when = format!("when {token} rotates");
    let error = add(&paths, &lead, record).unwrap_err();
    let shown = format!("{error}");
    assert!(
        shown.contains("secret") && !shown.contains(&token),
        "{shown}"
    );
    assert!(!std::fs::read_to_string(&paths.events)
        .unwrap()
        .contains(&token));
}

// ---------------------------------------------------------------------------
// Due and fulfilment, over a fake observer
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Fake {
    tasks: std::collections::BTreeMap<String, TaskView>,
    pull: Option<PullView>,
    branch: Option<bool>,
    repository: Option<String>,
    now: u64,
}

impl Observer for Fake {
    fn task(&mut self, id: &str) -> Option<TaskView> {
        self.tasks.get(id).cloned()
    }
    fn pull_request(&mut self, _: &str, _: u64) -> Option<PullView> {
        self.pull.clone()
    }
    fn branch_deleted(&mut self, _: &str, _: &str) -> Option<bool> {
        self.branch
    }
    fn commit_on_target(&mut self, _: &str) -> Option<(bool, String, String)> {
        None
    }
    fn now(&self) -> u64 {
        self.now
    }
    fn repository(&self) -> Option<String> {
        self.repository.clone()
    }
}

fn task_view(status: &str, claimed: bool, tip: &str) -> TaskView {
    TaskView {
        status: status.into(),
        uid: Some("u-1".into()),
        target: "main".into(),
        reference: "origin/main".into(),
        tip: tip.into(),
        claimed,
        accepted: status == "complete",
    }
}

fn linked(kind: LinkKind, target: Target) -> FollowUp {
    let mut record = item("linked", "WS-001", "coordinator");
    record.links = vec![Link { kind, target }];
    record.no_link_reason = None;
    record
}

fn task_target() -> Target {
    Target::Record {
        id: "TSK-001".into(),
        uid: Some("u-1".into()),
        repository: "local".into(),
    }
}

/// Add `record` with its links observed by `fake`, then return the folded
/// item.
fn observed_item(
    paths: &Paths,
    lead: &RecordedBy,
    mut record: FollowUp,
    fake: &mut Fake,
) -> codeflow_core::coord::transition::Item {
    let folded = ledger(paths).ledger;
    let probe = codeflow_core::coord::transition::Item {
        id: "FU-000".into(),
        record: record.clone(),
        state: ItemState::Open,
        revision: 0,
        added_at: String::new(),
        added_by: lead.clone(),
        last_check: None,
        proofs: Vec::new(),
        reopened: Vec::new(),
        purged: Vec::new(),
        baseline: Vec::new(),
        identity_purged: false,
        last_sequence: 0,
    };
    record.observed = due::observe_item(&probe, &folded, &[], fake);
    let id = add(paths, lead, record).unwrap();
    ledger(paths).ledger.items[&id].clone()
}

#[test]
fn a_prerequisite_task_changing_state_makes_the_item_due_and_never_closes_it() {
    let dir = repo();
    let (paths, lead, _) = bound(dir.path());
    let mut fake = Fake {
        now: coord::now_secs(),
        ..Fake::default()
    };
    fake.tasks
        .insert("TSK-001".into(), task_view("todo", false, "a"));
    let item = observed_item(
        &paths,
        &lead,
        linked(LinkKind::Prerequisite, task_target()),
        &mut fake,
    );
    let folded = ledger(&paths).ledger;
    let reasons = |fake: &mut Fake| {
        due::due_reasons(
            &item,
            fake.now,
            &due::observe_item(&item, &folded, &[], fake),
        )
    };
    assert!(reasons(&mut fake).is_empty());
    for (status, claimed, expect) in [
        ("todo", true, "claimed"),
        ("complete", false, "complete"),
        ("blocked", false, "blocked"),
        ("cancelled", false, "cancelled"),
    ] {
        fake.tasks
            .insert("TSK-001".into(), task_view(status, claimed, "b"));
        let found = reasons(&mut fake);
        assert!(
            found.iter().any(|reason| reason.contains(expect)),
            "{status}: {found:?}"
        );
        // A prerequisite never closes the item.
        assert!(due::fulfilment(&item, &ledger(&paths).ledger, &mut fake).is_none());
    }
}

#[test]
#[allow(clippy::too_many_lines)]
fn a_pull_request_a_follow_up_a_lane_and_a_time_make_the_item_due() {
    let dir = repo();
    let (paths, lead, lane) = bound(dir.path());
    let mut fake = Fake {
        now: coord::now_secs(),
        ..Fake::default()
    };
    // A pull request merged, closed or its checks finished.
    fake.pull = Some(PullView {
        state: "open".into(),
        checks: "pending".into(),
        base: "main".into(),
        merge_commit: None,
    });
    let pr = observed_item(
        &paths,
        &lead,
        linked(
            LinkKind::Prerequisite,
            Target::PullRequest {
                number: 4,
                repository: "github.com/a/b".into(),
            },
        ),
        &mut fake,
    );
    for (state, checks) in [("open", "passed"), ("merged", "passed"), ("closed", "none")] {
        fake.pull = Some(PullView {
            state: state.into(),
            checks: checks.into(),
            base: "main".into(),
            merge_commit: Some("m".into()),
        });
        let folded = ledger(&paths).ledger;
        let found = due::due_reasons(
            &pr,
            fake.now,
            &due::observe_item(&pr, &folded, &[], &mut fake),
        );
        assert!(!found.is_empty(), "{state} {checks}");
    }
    // Another follow-up closing.
    let other = add(&paths, &lead, item("other", "WS-001", "coordinator")).unwrap();
    let waits = observed_item(
        &paths,
        &lead,
        linked(
            LinkKind::Prerequisite,
            Target::FollowUp { id: other.clone() },
        ),
        &mut fake,
    );
    act(&paths, &lead, &other, Data::ItemClosed(asserted())).unwrap();
    let folded = ledger(&paths).ledger;
    let found = due::due_reasons(
        &waits,
        fake.now,
        &due::observe_item(&waits, &folded, &[], &mut fake),
    );
    assert!(
        found.iter().any(|r| r.contains("open -> closed")),
        "{found:?}"
    );
    // A lane reporting.
    let lane_item = observed_item(
        &paths,
        &lead,
        linked(
            LinkKind::Prerequisite,
            Target::Lane {
                id: "LN-001".into(),
            },
        ),
        &mut fake,
    );
    act(
        &paths,
        &lane,
        "LN-001",
        Data::LaneReported(codeflow_core::coord::schema::Report {
            summary: "done".into(),
        }),
    )
    .unwrap();
    let folded = ledger(&paths).ledger;
    let found = due::due_reasons(
        &lane_item,
        fake.now,
        &due::observe_item(&lane_item, &folded, &[], &mut fake),
    );
    assert!(
        found.iter().any(|r| r.contains("idle -> reported")),
        "{found:?}"
    );
    // A lane going stale: its seat's liveness is past the timeout.
    let stale = vec![store::Liveness {
        recorded_by: lane.clone(),
        last_seen: coord::format_time(fake.now - coord::HEARTBEAT_TIMEOUT_SECS - 60),
        sequence: 1,
    }];
    let found = due::due_reasons(
        &lane_item,
        fake.now,
        &due::observe_item(&lane_item, &folded, &stale, &mut fake),
    );
    assert!(found.iter().any(|r| r.contains("stale")), "{found:?}");
    // A time passing.
    let at = coord::format_time(fake.now + 60);
    let timed = observed_item(
        &paths,
        &lead,
        linked(LinkKind::Prerequisite, Target::Time { at }),
        &mut fake,
    );
    fake.now += 120;
    let folded = ledger(&paths).ledger;
    let found = due::due_reasons(
        &timed,
        fake.now,
        &due::observe_item(&timed, &folded, &[], &mut fake),
    );
    assert!(
        found.iter().any(|r| r.contains("pending -> passed")),
        "{found:?}"
    );
    // The next check passing.
    fake.now += 3600;
    let found = due::due_reasons(&timed, fake.now, &[]);
    assert!(found.iter().any(|r| r.contains("check due")), "{found:?}");
}

#[test]
fn only_a_fulfils_link_done_on_the_target_closes_and_a_revert_reopens() {
    let dir = repo();
    let (paths, lead, _) = bound(dir.path());
    let mut fake = Fake {
        now: coord::now_secs(),
        ..Fake::default()
    };
    fake.tasks
        .insert("TSK-001".into(), task_view("todo", true, "a"));
    let item = observed_item(
        &paths,
        &lead,
        linked(LinkKind::Fulfils, task_target()),
        &mut fake,
    );
    // Completion on a feature branch leaves the target's record as it was:
    // the observer reads the target tip only, so nothing closes.
    assert!(due::fulfilment(&item, &ledger(&paths).ledger, &mut fake).is_none());
    // A context link never closes either.
    let context = observed_item(
        &paths,
        &lead,
        linked(
            LinkKind::Context,
            Target::Record {
                id: "TSK-002".into(),
                uid: None,
                repository: "local".into(),
            },
        ),
        &mut fake,
    );
    fake.tasks
        .insert("TSK-002".into(), task_view("complete", false, "a"));
    assert!(due::fulfilment(&context, &ledger(&paths).ledger, &mut fake).is_none());
    // Complete on the target closes, recording source, repository, target,
    // revision and time.
    fake.tasks
        .insert("TSK-001".into(), task_view("complete", false, "c0ffee"));
    let proof = due::fulfilment(&item, &ledger(&paths).ledger, &mut fake).unwrap();
    assert_eq!(proof.kind, EvidenceKind::MachineVerified);
    assert_eq!(proof.target.as_deref(), Some("main"));
    assert_eq!(proof.revision.as_deref(), Some("c0ffee"));
    assert_eq!(proof.repository.as_deref(), Some("local"));
    assert!(proof.source.contains("origin/main"));
    act(&paths, &lead, &item.id, Data::ItemClosed(proof.clone())).unwrap();
    let closed = ledger(&paths).ledger.items[&item.id].clone();
    assert!(due::reverted(&closed, &ledger(&paths).ledger, &mut fake).is_none());
    // The task is reverted on the target: the proof no longer holds; the
    // reopen keeps the old proof in history.
    fake.tasks
        .insert("TSK-001".into(), task_view("todo", false, "d"));
    let reason = due::reverted(&closed, &ledger(&paths).ledger, &mut fake).unwrap();
    act(
        &paths,
        &lead,
        &item.id,
        Data::ItemReopened(codeflow_core::coord::schema::Reason { reason }),
    )
    .unwrap();
    let reopened = ledger(&paths).ledger.items[&item.id].clone();
    assert_eq!(reopened.state, ItemState::Open);
    assert_eq!(reopened.proofs, vec![proof]);
}

#[test]
fn a_branch_deletion_stays_open_until_deletion_evidence_exists() {
    let dir = repo();
    let (paths, lead, _) = bound(dir.path());
    let mut fake = Fake {
        now: coord::now_secs(),
        ..Fake::default()
    };
    let item = observed_item(
        &paths,
        &lead,
        linked(
            LinkKind::Fulfils,
            Target::Branch {
                name: "task/x".into(),
                remote: "origin".into(),
                repository: "local".into(),
            },
        ),
        &mut fake,
    );
    // Unknown (offline, never asked) and present both keep it open.
    for observed in [None, Some(false)] {
        fake.branch = observed;
        assert!(due::fulfilment(&item, &ledger(&paths).ledger, &mut fake).is_none());
    }
    fake.branch = Some(true);
    let proof = due::fulfilment(&item, &ledger(&paths).ledger, &mut fake).unwrap();
    assert_eq!(proof.revision.as_deref(), Some("absent"));
}

#[test]
fn a_cancelled_task_makes_its_follow_ups_due_for_relinking_or_a_drop() {
    let dir = repo();
    let (paths, lead, _) = bound(dir.path());
    let mut fake = Fake {
        now: coord::now_secs(),
        ..Fake::default()
    };
    fake.tasks
        .insert("TSK-001".into(), task_view("todo", false, "a"));
    let item = observed_item(
        &paths,
        &lead,
        linked(LinkKind::Fulfils, task_target()),
        &mut fake,
    );
    fake.tasks
        .insert("TSK-001".into(), task_view("cancelled", false, "b"));
    let folded = ledger(&paths).ledger;
    let found = due::due_reasons(
        &item,
        fake.now,
        &due::observe_item(&item, &folded, &[], &mut fake),
    );
    assert!(
        found
            .iter()
            .any(|r| r.contains("relink the follow-up or drop it")),
        "{found:?}"
    );
}

#[test]
fn a_dependency_wait_is_rechecked_at_least_daily() {
    let dir = repo();
    let (paths, lead, _) = bound(dir.path());
    let mut fake = Fake {
        now: coord::now_secs(),
        ..Fake::default()
    };
    let item = observed_item(
        &paths,
        &lead,
        linked(LinkKind::Prerequisite, task_target()),
        &mut fake,
    );
    let far = fake.now + 30 * 86_400;
    assert_eq!(
        due::capped_next_check(&item, far, fake.now),
        fake.now + coord::MAX_RECHECK_SECS
    );
}

/// The 10,000-event read path (no compaction): fold once, with a large
/// open set. The figure is printed for the record; the bound only catches
/// a regression of an order of magnitude.
#[test]
fn ten_thousand_events_fold_once_within_the_recorded_bound() {
    let dir = repo();
    let (paths, lead, _) = bound(dir.path());
    let start_sequence = ledger(&paths).ledger.last_sequence;
    let mut text = std::fs::read_to_string(&paths.events).unwrap();
    let now = coord::format_time(coord::now_secs());
    for n in 0..10_000_u64 {
        let id = format!("FU-{:05}", n / 2 + 1);
        let (event, data) = if n % 2 == 0 {
            (
                "item_added",
                serde_json::to_value(item(&format!("item {n}"), "WS-001", "coordinator")).unwrap(),
            )
        } else {
            (
                "item_checked",
                serde_json::json!({"observed": "still waiting", "next_check": now}),
            )
        };
        let line = serde_json::json!({
            "schema_version": 1, "event": event, "subject_id": id,
            "sequence": start_sequence + n + 1, "recorded_at": now,
            "recorded_by": lead, "data": data,
        });
        text.push_str(&line.to_string());
        text.push('\n');
    }
    std::fs::write(&paths.events, text).unwrap();
    let started = std::time::Instant::now();
    let read = ledger(&paths);
    let elapsed = started.elapsed();
    assert!(read.damage.is_none(), "{:?}", read.damage);
    assert_eq!(read.ledger.items.len(), 5_000);
    println!(
        "coord fold: 10000 events, 5000 open, {} ms",
        elapsed.as_millis()
    );
    assert!(elapsed < Duration::from_secs(30), "fold took {elapsed:?}");
}

#[test]
fn every_event_kind_is_documented_in_the_schema_reference() {
    let doc = include_str!("../../../docs/coordination-ledger.md");
    for kind in codeflow_core::coord::schema::EventKind::ALL {
        assert!(
            doc.contains(&format!("### `{}`", kind.as_str())),
            "docs/coordination-ledger.md has no section for {}",
            kind.as_str()
        );
    }
    for record in [
        "Follow-up",
        "Lane",
        "Seat",
        "Attempt",
        "Coordinator claim",
        "Work stream",
    ] {
        assert!(
            doc.contains(&format!("### {record}")),
            "no {record} record section"
        );
    }
}

// ---------------------------------------------------------------------------
// Review round 1 (Codex F1 to F10, security F1 to F7): each reproduction
// failed at 998dadb1e and passes here.
// ---------------------------------------------------------------------------

fn scope_change(lane: &str, text: &str) -> Draft {
    Draft::new(
        SubjectId::Given(lane.into()),
        Data::LaneScopeChanged(codeflow_core::coord::schema::Scope {
            slug: None,
            scope: vec![text.into()],
        }),
    )
}

/// Codex F1: an operation of two events interrupted after the first
/// complete line is finished by its retry, exactly once.
#[test]
fn an_interrupted_operation_is_finished_by_its_retry() {
    let dir = repo();
    let (paths, lead, _) = bound(dir.path());
    let operation = || {
        let mut first = scope_change("LN-001", "first");
        first.idempotency_key = Some("op-7".into());
        vec![first, scope_change("LN-001", "second")]
    };
    let before = std::fs::read_to_string(&paths.events).unwrap();
    store::commit(&paths, &lead, WAIT, |_| Ok(operation())).unwrap();
    let after = std::fs::read_to_string(&paths.events).unwrap();
    let mut lines = after[before.len()..].lines();
    let (one, two) = (lines.next().unwrap(), lines.next().unwrap());
    // The crash image: the first event whole, 15 bytes of the second.
    std::fs::write(&paths.events, format!("{before}{one}\n{}", &two[..15])).unwrap();
    let retried = store::commit(&paths, &lead, WAIT, |_| Ok(operation())).unwrap();
    assert_eq!(
        retried.envelopes.len(),
        1,
        "only the missing event is written"
    );
    let folded = ledger(&paths).ledger;
    assert_eq!(folded.lanes["LN-001"].scope, vec!["second".to_string()]);
    assert!(folded.keys.contains_key("op-7#0") && folded.keys.contains_key("op-7#1"));
    // A second retry writes nothing.
    let again = store::commit(&paths, &lead, WAIT, |_| Ok(operation())).unwrap();
    assert!(again.envelopes.is_empty() && again.duplicate_of.is_some());
}

/// Security F5: a reused idempotency key with another payload or actor is
/// refused, never silently dropped.
#[test]
fn a_reused_key_with_another_payload_or_actor_is_refused() {
    let dir = repo();
    let (paths, lead, lane) = bound(dir.path());
    let keyed = |title: &str| Draft {
        idempotency_key: Some("review-R1-F3".into()),
        ..Draft::new(
            SubjectId::NextFollowUp,
            Data::ItemAdded(item(title, "WS-001", "coordinator")),
        )
    };
    store::commit(&paths, &lead, WAIT, |_| Ok(vec![keyed("decoy")])).unwrap();
    let error = store::commit(&paths, &lead, WAIT, |_| {
        Ok(vec![keyed("rotate the deploy key")])
    })
    .unwrap_err();
    assert!(matches!(error, CoordError::KeyConflict(_)), "{error:?}");
    let error = store::commit(&paths, &lane, WAIT, |_| Ok(vec![keyed("decoy")])).unwrap_err();
    assert!(matches!(error, CoordError::KeyConflict(_)), "{error:?}");
    let same = store::commit(&paths, &lead, WAIT, |_| Ok(vec![keyed("decoy")])).unwrap();
    assert!(same.duplicate_of.is_some());
}

/// Codex F2: completion without a passing acceptance block, a pinned UID
/// missing or changed on the target, another repository, or a prerequisite
/// still open never closes as machine-verified.
#[test]
fn only_accepted_identified_work_with_its_prerequisites_met_closes() {
    let dir = repo();
    let (paths, lead, _) = bound(dir.path());
    let mut fake = Fake {
        now: coord::now_secs(),
        ..Fake::default()
    };
    fake.tasks
        .insert("TSK-001".into(), task_view("todo", false, "a"));
    let fulfils = observed_item(
        &paths,
        &lead,
        linked(LinkKind::Fulfils, task_target()),
        &mut fake,
    );
    let mut unaccepted = task_view("complete", false, "b");
    unaccepted.accepted = false;
    fake.tasks.insert("TSK-001".into(), unaccepted);
    assert!(due::fulfilment(&fulfils, &ledger(&paths).ledger, &mut fake).is_none());
    let mut no_uid = task_view("complete", false, "b");
    no_uid.uid = None;
    fake.tasks.insert("TSK-001".into(), no_uid);
    assert!(due::fulfilment(&fulfils, &ledger(&paths).ledger, &mut fake).is_none());
    fake.tasks
        .insert("TSK-001".into(), task_view("complete", false, "b"));
    fake.repository = Some("github.com/other/repo".into());
    assert!(due::fulfilment(&fulfils, &ledger(&paths).ledger, &mut fake).is_none());
    fake.repository = None;
    assert!(due::fulfilment(&fulfils, &ledger(&paths).ledger, &mut fake).is_some());
    // A condition that asks for more than the fulfilled task: an open
    // prerequisite follow-up keeps the item open.
    let other = add(
        &paths,
        &lead,
        item("release notes reviewed", "WS-001", "coordinator"),
    )
    .unwrap();
    let mut record = linked(LinkKind::Fulfils, task_target());
    record.links.push(Link {
        kind: LinkKind::Prerequisite,
        target: Target::FollowUp { id: other.clone() },
    });
    let both = observed_item(&paths, &lead, record, &mut fake);
    assert!(due::fulfilment(&both, &ledger(&paths).ledger, &mut fake).is_none());
    act(&paths, &lead, &other, Data::ItemClosed(asserted())).unwrap();
    let both = ledger(&paths).ledger.items[&both.id].clone();
    assert!(due::fulfilment(&both, &ledger(&paths).ledger, &mut fake).is_some());
}

/// Codex F3: a closure on a branch deletion, a commit or a pull request is
/// reopened when that proof is definitely invalidated, and not when the
/// observation is unknown.
#[test]
fn every_invalidated_machine_proof_reopens_and_unknown_does_not() {
    let dir = repo();
    let (paths, lead, _) = bound(dir.path());
    let mut fake = Fake {
        now: coord::now_secs(),
        branch: Some(true),
        ..Fake::default()
    };
    let branch = observed_item(
        &paths,
        &lead,
        linked(
            LinkKind::Fulfils,
            Target::Branch {
                name: "task/x".into(),
                remote: "origin".into(),
                repository: "local".into(),
            },
        ),
        &mut fake,
    );
    let proof = due::fulfilment(&branch, &ledger(&paths).ledger, &mut fake).unwrap();
    act(&paths, &lead, &branch.id, Data::ItemClosed(proof)).unwrap();
    let closed = ledger(&paths).ledger.items[&branch.id].clone();
    fake.branch = None;
    assert!(
        due::reverted(&closed, &ledger(&paths).ledger, &mut fake).is_none(),
        "unknown is not invalidation"
    );
    fake.branch = Some(false);
    let reason = due::reverted(&closed, &ledger(&paths).ledger, &mut fake).unwrap();
    assert!(reason.contains("present"), "{reason}");
    // A pull request reopened after its merge, and one merged into a work
    // branch, are unmet.
    fake.pull = Some(PullView {
        state: "merged".into(),
        checks: "passed".into(),
        base: "main".into(),
        merge_commit: Some("m".into()),
    });
    let pr = observed_item(
        &paths,
        &lead,
        linked(
            LinkKind::Fulfils,
            Target::PullRequest {
                number: 9,
                repository: "local".into(),
            },
        ),
        &mut fake,
    );
    let proof = due::fulfilment(&pr, &ledger(&paths).ledger, &mut fake).unwrap();
    act(&paths, &lead, &pr.id, Data::ItemClosed(proof)).unwrap();
    let closed = ledger(&paths).ledger.items[&pr.id].clone();
    fake.pull = Some(PullView {
        state: "open".into(),
        checks: "pending".into(),
        base: "main".into(),
        merge_commit: None,
    });
    assert!(due::reverted(&closed, &ledger(&paths).ledger, &mut fake).is_some());
    fake.pull = Some(PullView {
        state: "merged".into(),
        checks: "passed".into(),
        base: "feature/x".into(),
        merge_commit: Some("m".into()),
    });
    let reopened = ledger(&paths).ledger.items[&pr.id].clone();
    assert!(
        due::reverted(&reopened, &ledger(&paths).ledger, &mut fake).is_some(),
        "a work-branch merge is no proof"
    );
}

/// Codex F4, security F4: purge removes every free-text field of the item
/// from the live log, its keys and quarantine, records who purged it, and a
/// second item with its own source reference can be purged too.
#[test]
fn purge_removes_every_free_text_field_and_keeps_items_distinct() {
    let dir = repo();
    let (paths, lead, _) = bound(dir.path());
    let mut record = item("PRIVATE-TITLE", "WS-001", "coordinator");
    record.description = "PRIVATE-DESCRIPTION".into();
    record.source = Source::Review;
    record.source_reference = Some("PRIVATE-REF-1".into());
    record.rejected_suggestions = vec![codeflow_core::coord::schema::Rejected {
        reference: "PRIVATE-REJREF".into(),
        reason: "PRIVATE-REJREASON".into(),
    }];
    let first = store::commit(&paths, &lead, WAIT, |_| {
        Ok(vec![Draft {
            idempotency_key: Some("PRIVATE-KEY".into()),
            comment: Some("PRIVATE-COMMENT".into()),
            ..Draft::new(SubjectId::NextFollowUp, Data::ItemAdded(record))
        }])
    })
    .unwrap()
    .envelopes[0]
        .subject_id
        .clone();
    act(
        &paths,
        &lead,
        &first,
        Data::ItemChecked(codeflow_core::coord::schema::ItemCheck {
            observed: "PRIVATE-OBSERVED".into(),
            next_check: coord::format_time(coord::now_secs() + 60),
            observations: vec![codeflow_core::coord::schema::Observation {
                link: "PRIVATE-OBSLINK".into(),
                state: "PRIVATE-STATE".into(),
                version: None,
            }],
            answer_source: Some("PRIVATE-ANSWERSRC".into()),
        }),
    )
    .unwrap();
    let mut evidence = asserted();
    evidence.reference = "https://example.invalid/PRIVATE-EVIDENCE-URL".into();
    act(&paths, &lead, &first, Data::ItemClosed(evidence)).unwrap();
    // A torn fragment of the item in quarantine.
    std::fs::create_dir_all(&paths.quarantine).unwrap();
    std::fs::write(
        paths.quarantine.join("torn-1-x.bin"),
        format!("{{\"subject_id\":\"{first}\",\"data\":\"PRIVATE-TORN\""),
    )
    .unwrap();
    let mut second = item("second", "WS-001", "coordinator");
    second.source = Source::Review;
    second.source_reference = Some("PRIVATE-REF-2".into());
    let second = add(&paths, &lead, second).unwrap();
    let purged = store::purge(&paths, &lead, &first, WAIT).unwrap();
    assert_eq!(purged.quarantine_removed, 1);
    store::purge(&paths, &lead, &second, WAIT).unwrap();
    let mut everything = String::new();
    for entry in walk(&paths.dir) {
        everything.push_str(&String::from_utf8_lossy(&std::fs::read(entry).unwrap()));
    }
    assert!(!everything.contains("PRIVATE"), "text left behind");
    let folded = ledger(&paths);
    assert!(folded.damage.is_none(), "{:?}", folded.damage);
    assert_eq!(folded.ledger.items[&first].purged.len(), 1);
    assert_eq!(folded.ledger.items[&first].purged[0].1, "coordinator");
}

fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap().filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            out.extend(walk(&path));
        } else {
            out.push(path);
        }
    }
    out
}

/// Codex F5: purge is judged under the lock; a holder that handed the item
/// over can no longer purge it, and its text is unchanged.
#[test]
fn purge_by_a_former_holder_is_refused_under_the_lock() {
    let dir = repo();
    let (paths, lead, lane) = bound(dir.path());
    let id = add(&paths, &lane, item("lane item", "WS-001", "LN-001")).unwrap();
    act(
        &paths,
        &lane,
        &id,
        Data::ItemHandedOver(codeflow_core::coord::schema::HandOver {
            to_holder: "coordinator".into(),
            to_stream: None,
        }),
    )
    .unwrap();
    let before = std::fs::read(&paths.events).unwrap();
    let error = store::purge(&paths, &lane, &id, WAIT).unwrap_err();
    assert!(format!("{error}").contains("only the holder"), "{error}");
    assert_eq!(before, std::fs::read(&paths.events).unwrap());
    store::purge(&paths, &lead, &id, WAIT).unwrap();
}

/// Codex F8: an existing public ledger becomes private on the next write.
#[cfg(unix)]
#[test]
fn an_existing_public_ledger_is_made_private_on_write() {
    use std::os::unix::fs::PermissionsExt;
    let dir = repo();
    let (paths, lead, _) = bound(dir.path());
    let set = |path: &Path, mode| {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    };
    set(&paths.dir, 0o755);
    set(&paths.events, 0o644);
    set(&paths.counters, 0o644);
    add(&paths, &lead, item("after", "WS-001", "coordinator")).unwrap();
    let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&paths.dir), 0o700);
    assert_eq!(mode(&paths.events), 0o600);
    assert_eq!(mode(&paths.counters), 0o600);
}

/// Codex F7, security F6: a refusal or a damage diagnostic never echoes a
/// secret-shaped value.
#[test]
fn refusals_and_damage_never_echo_a_secret() {
    let dir = repo();
    let (paths, lead, _) = bound(dir.path());
    let token = format!("ghp_{}", "q".repeat(36));
    let resolver = codeflow_core::coord::reference::Resolver::new(dir.path());
    for written in [
        format!("context:{token}"),
        format!("context:https://x:{token}@github.com/o/r"),
        format!("{token}:TSK-001"),
        format!("prerequisite:time:{token}"),
    ] {
        let error = resolver.link(&written).unwrap_err();
        assert!(!error.contains(&token), "echoed: {error}");
    }
    add(&paths, &lead, item("a", "WS-001", "coordinator")).unwrap();
    plant(&paths, |mut lines| {
        let mut last: serde_json::Value = serde_json::from_str(lines.last().unwrap()).unwrap();
        last["sequence"] = (last["sequence"].as_u64().unwrap() + 1).into();
        last["subject_id"] = "FU-002".into();
        last["data"]["waits_on"] = serde_json::json!([token.clone()]);
        lines.push(last.to_string());
        let mut bad: serde_json::Value = serde_json::from_str(lines.last().unwrap()).unwrap();
        bad["event"] = token.clone().into();
        lines.push(bad.to_string());
        lines
    });
    let error = add(&paths, &lead, item("b", "WS-001", "coordinator")).unwrap_err();
    assert!(!format!("{error}").contains(&token), "echoed: {error}");
}

/// Security F7: a branch link replayed with a remote the clone does not
/// configure is never contacted.
#[test]
fn an_unconfigured_remote_is_never_contacted() {
    let dir = repo();
    let mut observer = due::RepoObserver::new(dir.path(), true);
    let started = std::time::Instant::now();
    assert_eq!(
        observer.branch_deleted("https://attacker.example/x", "main"),
        None
    );
    assert!(started.elapsed() < Duration::from_secs(2));
}

/// AC-13: a configured remote that cannot be reached is unknown, within the
/// network bound, never "deleted".
#[test]
fn an_offline_remote_is_unknown_within_its_bound() {
    let dir = repo();
    // Nothing listens on the discard port: the connection is refused.
    git(
        dir.path(),
        &["remote", "add", "origin", "https://127.0.0.1:9/project.git"],
    );
    let limits = codeflow_core::coord::observe::Limits {
        network: Duration::from_secs(5),
        ..codeflow_core::coord::observe::Limits::default()
    };
    let mut observer = due::RepoObserver::with_limits(dir.path(), true, limits);
    let started = std::time::Instant::now();
    assert_eq!(observer.branch_deleted("origin", "task/gone"), None);
    assert!(started.elapsed() < limits.network + Duration::from_secs(2));
}

/// Security F3: a task completed on a work branch that the record itself
/// names as its target is never read as complete; only a landing line is.
#[test]
fn a_work_branch_named_as_target_is_never_a_target() {
    let dir = repo();
    let root = dir.path();
    let record = |status: &str, target: &str| {
        format!("---\nid: TSK-001\nuid: u-1\nepic_id: null\nstandalone_reason: x\nintegration_target: \"{target}\"\ntitle: t\nstatus: {status}\nwork_type: feat\nspecs: []\ndepends_on: []\ncreated: 2026-10-01\n---\n\n## Description\nx\n")
    };
    std::fs::create_dir_all(root.join("project-management/tasks")).unwrap();
    std::fs::write(
        root.join("project-management/tasks/TSK-001.md"),
        record("todo", "main"),
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "docs: plan"]);
    let origin = tempfile::tempdir().unwrap();
    git(origin.path(), &["init", "-q", "--bare", "-b", "main"]);
    git(
        root,
        &["remote", "add", "origin", origin.path().to_str().unwrap()],
    );
    git(root, &["push", "-q", "-u", "origin", "main"]);
    git(root, &["checkout", "-qb", "fix/TSK-001-fixture"]);
    std::fs::write(
        root.join("project-management/tasks/TSK-001.md"),
        record("complete", "fix/TSK-001-fixture"),
    )
    .unwrap();
    git(root, &["commit", "-qam", "docs: complete on the branch"]);
    let tasks = codeflow_core::workgraph::readiness::target_tasks(root).unwrap();
    assert_eq!(
        tasks.get("TSK-001").map(|task| task.status.as_str()),
        Some("todo")
    );
    assert!(!codeflow_core::workgraph::readiness::is_landing_target(
        "fix/TSK-001-fixture"
    ));
    assert!(codeflow_core::workgraph::readiness::is_landing_target(
        "integration/EPC-025-x"
    ));
}

// ---------------------------------------------------------------------------
// Review round 2 (Codex R2, R3, R7); each failed at 03bc5a13f.
// ---------------------------------------------------------------------------

/// Codex R2: a pull request merged into `fix/main` is not merged into a
/// landing line.
#[test]
fn a_pull_request_into_a_feature_branch_named_like_main_is_no_proof() {
    let dir = repo();
    let (paths, lead, _) = bound(dir.path());
    let mut fake = Fake {
        now: coord::now_secs(),
        ..Fake::default()
    };
    fake.pull = Some(PullView {
        state: "merged".into(),
        checks: "passed".into(),
        base: "fix/main".into(),
        merge_commit: Some("a".repeat(40)),
    });
    let item = observed_item(
        &paths,
        &lead,
        linked(
            LinkKind::Fulfils,
            Target::PullRequest {
                number: 1,
                repository: "local".into(),
            },
        ),
        &mut fake,
    );
    assert!(due::fulfilment(&item, &ledger(&paths).ledger, &mut fake).is_none());
    assert!(!codeflow_core::workgraph::readiness::is_landing_target(
        "fix/main"
    ));
}

/// Codex R3: reopening a follow-up the closure waited on reopens the
/// dependent closure.
#[test]
fn reopening_a_required_follow_up_reopens_the_dependent_closure() {
    let dir = repo();
    let (paths, lead, _) = bound(dir.path());
    let mut fake = Fake {
        now: coord::now_secs(),
        branch: Some(true),
        ..Fake::default()
    };
    let prior = add(
        &paths,
        &lead,
        item("prior condition", "WS-001", "coordinator"),
    )
    .unwrap();
    act(&paths, &lead, &prior, Data::ItemClosed(asserted())).unwrap();
    let mut record = linked(
        LinkKind::Fulfils,
        Target::Branch {
            name: "absent".into(),
            remote: "origin".into(),
            repository: "local".into(),
        },
    );
    record.waits_on = vec![prior.clone()];
    let dependent = observed_item(&paths, &lead, record, &mut fake);
    let proof = due::fulfilment(&dependent, &ledger(&paths).ledger, &mut fake).unwrap();
    act(&paths, &lead, &dependent.id, Data::ItemClosed(proof)).unwrap();
    let closed = ledger(&paths).ledger.items[&dependent.id].clone();
    assert!(due::reverted(&closed, &ledger(&paths).ledger, &mut fake).is_none());
    act(
        &paths,
        &lead,
        &prior,
        Data::ItemReopened(codeflow_core::coord::schema::Reason {
            reason: "invalidated".into(),
        }),
    )
    .unwrap();
    let reason = due::reverted(&closed, &ledger(&paths).ledger, &mut fake).unwrap();
    assert!(
        reason.contains(&format!("{prior} is open again")),
        "{reason}"
    );
}

/// Codex R7: a key reused for a check with another observation is refused;
/// the user's observation is content, not a recomputed field.
#[test]
fn a_reused_key_with_another_observation_is_refused() {
    let dir = repo();
    let (paths, lead, _) = bound(dir.path());
    let id = add(&paths, &lead, item("watch result", "WS-001", "coordinator")).unwrap();
    let next = coord::format_time(coord::now_secs() + 300);
    let check = |observed: &str| Draft {
        idempotency_key: Some("check-key".into()),
        ..Draft::new(
            SubjectId::Given(id.clone()),
            Data::ItemChecked(codeflow_core::coord::schema::ItemCheck {
                observed: observed.into(),
                next_check: next.clone(),
                observations: vec![],
                answer_source: None,
            }),
        )
    };
    store::commit(&paths, &lead, WAIT, |_| Ok(vec![check("passed")])).unwrap();
    let error = store::commit(&paths, &lead, WAIT, |_| {
        Ok(vec![check("FAILED; operator action required")])
    })
    .unwrap_err();
    assert!(matches!(error, CoordError::KeyConflict(_)), "{error:?}");
    let again = store::commit(&paths, &lead, WAIT, |_| Ok(vec![check("passed")])).unwrap();
    assert!(again.duplicate_of.is_some());
}
