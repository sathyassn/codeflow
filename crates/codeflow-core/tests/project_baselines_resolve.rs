//! Every baseline commit id in this repository's `.codeflow/project.toml`
//! names a commit the repository holds (TSK-227).
//!
//! The first public history rewrote commit ids, and the baselines kept the
//! archive ids until every pull request failed the record checks (TSK-198).
//! Nothing noticed the ids had stopped resolving. This test reads the real
//! project file, collects every full commit id under a key that names a
//! baseline (`work_records_baseline`, `release_records_baseline`,
//! `release_rule_baseline` and any key added later with `baseline` in its
//! name), and fails naming each id and key the repository cannot resolve.
//!
//! The two release tables are the one stated exception. They are a one-time
//! bridge the release judge treats as write-once (SPC-013 R-120), so their
//! archive ids cannot be moved to public ids, and a public-only clone does
//! not hold the archive objects. The ten entries (five lines in each of the
//! two tables) are listed in [`ARCHIVE_ONLY`] by line and id. The list is
//! checked entry by entry in both directions: a listed entry the tables no
//! longer hold fails, a table entry the list does not name fails, an entry
//! at the wrong line fails, and an unresolvable id under any other key fails
//! too.
//!
//! Resolution needs the history the baselines name, which a shallow clone
//! does not hold. In a shallow clone the resolution test says so and skips;
//! the other tests and the control still run, and the hosted Linux gates and
//! any full clone run the resolution check.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use git2::{Oid, Repository};

/// The two release tables that may keep archive ids.
const RELEASE_TABLES: [&str; 2] = ["release_records_baseline", "release_rule_baseline"];

/// Archive commit ids the release tables keep (the retired epic lines'
/// cutoffs): (the line it is the cutoff of, full id). Each pair is allowed
/// only under the two release tables, at that line: five pairs in two tables
/// are the ten entries the exception covers.
const ARCHIVE_ONLY: &[(&str, &str)] = &[
    (
        "integration/EPC-014-public-docs",
        "d618075e229d49f706cf1c0e9ab5b10a3c9c69e3",
    ),
    (
        "integration/EPC-015-engineering-bar",
        "db55fc01c30f75d0eb1bd5f3df1de9dd8f9d1bff",
    ),
    (
        "integration/EPC-016-visual-guide",
        "51ee9374b506d9a359150ac15663c26c914b57cf",
    ),
    (
        "integration/EPC-018-autonomy-roster",
        "ccd56fa85160ac58d66828933e96c000357c4baa",
    ),
    (
        "integration/EPC-020-delivery-system",
        "2921df9f52a5e787f896146233a85201720591d3",
    ),
];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root resolves")
}

/// One baseline id found in the project file.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Found {
    /// The top-level key it sits under.
    key: String,
    /// The line (the table key) it is the cutoff of; empty for a list.
    line: String,
    /// The id as written.
    id: String,
}

fn is_full_id(text: &str) -> bool {
    text.len() == 40 && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Collects every string under `value` as an id of `key`; a string that is
/// not a full 40-hex id is reported, never skipped.
fn collect(
    key: &str,
    line: &str,
    value: &toml::Value,
    found: &mut Vec<Found>,
    malformed: &mut Vec<String>,
) {
    match value {
        toml::Value::String(text) => {
            if is_full_id(text) {
                found.push(Found {
                    key: key.to_string(),
                    line: line.to_string(),
                    id: text.clone(),
                });
            } else {
                malformed.push(format!(
                    "{key}: `{text}` is not a full 40-character commit id"
                ));
            }
        }
        toml::Value::Array(items) => {
            for item in items {
                collect(key, line, item, found, malformed);
            }
        }
        toml::Value::Table(table) => {
            for (name, item) in table {
                collect(key, name, item, found, malformed);
            }
        }
        other => malformed.push(format!("{key}: `{other}` is not a commit id")),
    }
}

fn baseline_ids(root: &Path) -> (Vec<Found>, Vec<String>) {
    let text = std::fs::read_to_string(root.join(".codeflow/project.toml"))
        .expect("the project file is readable");
    let table: toml::Table = text.parse().expect("the project file is valid TOML");
    let mut found = Vec::new();
    let mut malformed = Vec::new();
    for (key, value) in &table {
        if key.contains("baseline") {
            collect(key, "", value, &mut found, &mut malformed);
        }
    }
    (found, malformed)
}

fn resolves(repo: &Repository, id: &str) -> bool {
    Oid::from_str(id)
        .ok()
        .is_some_and(|oid| repo.find_commit(oid).is_ok())
}

/// Whether a baseline entry is one of the ten listed archive-only entries:
/// a listed (line, id) pair under one of the two release tables.
fn is_archive_only(found: &Found) -> bool {
    RELEASE_TABLES.contains(&found.key.as_str())
        && ARCHIVE_ONLY
            .iter()
            .any(|(line, id)| found.line == *line && found.id == *id)
}

/// The ten (table, line, id) entries the exception covers.
fn expected_exception() -> BTreeSet<(String, String, String)> {
    RELEASE_TABLES
        .iter()
        .flat_map(|table| {
            ARCHIVE_ONLY.iter().map(move |(line, id)| {
                ((*table).to_string(), (*line).to_string(), (*id).to_string())
            })
        })
        .collect()
}

/// Where the release tables and the exception list disagree, entry by entry:
/// a listed entry the tables no longer hold, or a table entry (anywhere
/// under a `release_` key, at any line) the list does not name.
fn exception_drift(found: &[Found]) -> Vec<String> {
    let expected = expected_exception();
    let actual: BTreeSet<(String, String, String)> = found
        .iter()
        .filter(|f| f.key.starts_with("release_"))
        .map(|f| (f.key.clone(), f.line.clone(), f.id.clone()))
        .collect();
    let mut drift = Vec::new();
    for (table, line, id) in expected.difference(&actual) {
        drift.push(format!(
            "{table}: {line} = {id} is listed in ARCHIVE_ONLY but the table does not hold it; remove the stale entry or restore the table"
        ));
    }
    for (table, line, id) in actual.difference(&expected) {
        drift.push(format!(
            "{table}: {line} = {id} is not listed in ARCHIVE_ONLY; list it there with its line, or remove it from the table (the tables are a write-once bridge, SPC-013 R-120)"
        ));
    }
    drift
}

#[test]
fn the_project_file_names_baselines_and_every_id_is_a_full_commit_id() {
    let (found, malformed) = baseline_ids(&repo_root());
    assert!(malformed.is_empty(), "malformed baselines: {malformed:#?}");
    let keys: BTreeSet<&str> = found.iter().map(|f| f.key.as_str()).collect();
    for expected in [
        "work_records_baseline",
        "release_records_baseline",
        "release_rule_baseline",
    ] {
        assert!(
            keys.contains(expected),
            "the scan found no ids under `{expected}`; found keys: {keys:?}"
        );
    }
}

/// The ids the repository cannot resolve and the exception does not cover.
fn unresolved(repo: &Repository, found: &[Found]) -> Vec<String> {
    found
        .iter()
        .filter(|f| !resolves(repo, &f.id) && !is_archive_only(f))
        .map(|f| {
            format!(
                "{}: {} does not resolve to a commit in this repository",
                f.key, f.id
            )
        })
        .collect()
}

#[test]
fn every_baseline_commit_id_resolves_in_the_repository() {
    let root = repo_root();
    // A source archive has no history to resolve against; the check belongs
    // to a clone.
    let Ok(repo) = Repository::discover(&root) else {
        eprintln!(
            "no git repository above {}: nothing to resolve",
            root.display()
        );
        return;
    };
    if repo.is_shallow() {
        eprintln!(
            "skipped: this clone is shallow and holds none of the baseline history; run `git fetch --unshallow` to check resolution"
        );
        return;
    }
    let (found, _) = baseline_ids(&root);
    let unresolved = unresolved(&repo, &found);
    assert!(
        unresolved.is_empty(),
        "baseline ids this repository cannot resolve (a rewritten or archive-only id?): {unresolved:#?}"
    );
}

/// The failure TSK-198 repaired: a work records baseline naming an id the
/// repository does not hold is reported, a held id is not, and only the
/// listed (table, line, id) entries of the release tables are tolerated.
#[test]
fn an_id_the_repository_does_not_hold_is_reported() {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = Repository::init(dir.path()).expect("init");
    let signature = git2::Signature::now("test", "test@example.invalid").expect("signature");
    let tree = repo
        .find_tree(
            repo.treebuilder(None)
                .expect("builder")
                .write()
                .expect("tree"),
        )
        .expect("tree");
    let held = repo
        .commit(Some("HEAD"), &signature, &signature, "root", &tree, &[])
        .expect("commit")
        .to_string();
    let rewritten = "375c68585aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string();
    let (line, archive) = ARCHIVE_ONLY[0];
    let (other_line, _) = ARCHIVE_ONLY[1];
    let entry = |key: &str, line: &str, id: &str| Found {
        key: key.into(),
        line: line.into(),
        id: id.into(),
    };
    let found = vec![
        entry("work_records_baseline", "", &held),
        entry("work_records_baseline", "", &rewritten),
        entry("work_records_baseline", "", archive),
        entry("release_rule_baseline", line, archive),
        entry("release_records_baseline", line, archive),
        entry("release_rule_baseline", other_line, archive),
        entry(
            "release_rule_baseline",
            line,
            "ffffffffffffffffffffffffffffffffffffffff",
        ),
    ];
    let reported = unresolved(&repo, &found);
    assert_eq!(
        reported.len(),
        4,
        "a rewritten id, an archive id under the work baseline, a listed id at the wrong line and an unlisted release id: {reported:#?}"
    );
    assert!(reported.iter().any(|text| text.contains(&rewritten)));
    assert!(reported
        .iter()
        .any(|text| text.starts_with("work_records_baseline") && text.contains(archive)));
    assert!(reported
        .iter()
        .any(|text| text.starts_with("release_rule_baseline") && text.contains(archive)));
    assert!(reported.iter().any(|text| text.contains("ffffffff")));
}

#[test]
fn the_work_records_baseline_never_relies_on_the_archive_exception() {
    let (found, _) = baseline_ids(&repo_root());
    let archive_ids: BTreeSet<&str> = ARCHIVE_ONLY.iter().map(|(_, id)| *id).collect();
    let reliant: Vec<&Found> = found
        .iter()
        .filter(|f| f.key == "work_records_baseline" && archive_ids.contains(f.id.as_str()))
        .collect();
    assert!(
        reliant.is_empty(),
        "work_records_baseline must name public ids; it names archive-only ones: {reliant:#?}"
    );
}

#[test]
fn the_archive_exception_lists_exactly_the_release_table_entries_it_covers() {
    let (found, _) = baseline_ids(&repo_root());
    let drift = exception_drift(&found);
    assert!(
        drift.is_empty(),
        "the release tables and ARCHIVE_ONLY disagree: {drift:#?}"
    );
    assert_eq!(
        found
            .iter()
            .filter(|f| f.key.starts_with("release_") && is_archive_only(f))
            .count(),
        10,
        "the exception covers five lines in two tables"
    );
}

/// The exception check sees a removed entry, an entry moved to the wrong
/// line and an extra entry, each by its (table, line, id).
#[test]
fn the_exception_check_reports_a_removed_moved_or_extra_entry() {
    let mut found: Vec<Found> = expected_exception()
        .into_iter()
        .map(|(key, line, id)| Found { key, line, id })
        .collect();
    assert!(
        exception_drift(&found).is_empty(),
        "the control starts clean"
    );

    let removed = found.remove(0);
    let drift = exception_drift(&found);
    assert_eq!(drift.len(), 1, "a removed entry: {drift:#?}");
    assert!(drift[0].contains(&removed.id) && drift[0].contains("does not hold it"));

    found.insert(0, removed);
    let moved = found
        .iter()
        .position(|f| f.line == ARCHIVE_ONLY[1].0)
        .expect("a listed line is present");
    found[moved].line = ARCHIVE_ONLY[2].0.to_string();
    let drift = exception_drift(&found);
    assert_eq!(drift.len(), 2, "an entry at the wrong line: {drift:#?}");

    found[moved].line = ARCHIVE_ONLY[1].0.to_string();
    found.push(Found {
        key: "release_rule_baseline".into(),
        line: "integration/EPC-099-new".into(),
        id: "1111111111111111111111111111111111111111".into(),
    });
    let drift = exception_drift(&found);
    assert_eq!(drift.len(), 1, "an extra entry: {drift:#?}");
    assert!(drift[0].contains("not listed in ARCHIVE_ONLY"));
}
