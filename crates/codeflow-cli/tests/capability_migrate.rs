//! TSK-1137: `capability migrate` converts a legacy fence catalog into
//! records in one run (AC-1), refuses an ambiguous prose span before any
//! write (AC-2), and leaves a catalog `validate --docs` accepts (AC-3).

use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command, Output};

const ENTRIES: usize = 17;
/// Entries that already own a page under docs/capabilities/.
const PAGES: [usize; 2] = [5, 12];
const PAGE: &str =
    "# A page of its own\n\nIntro to the page.\n\n## Anchor one\n\nDetail kept as written.\n";

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn cli(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_codeflow"))
        .current_dir(root)
        .args(args)
        .output()
        .unwrap()
}

fn write(root: &Path, path: &str, content: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn id(n: usize) -> String {
    format!("CAP-{n:03}")
}

fn name(n: usize) -> String {
    format!("entry-{n}")
}

/// The fence body of entry `n`, varied across statuses and evidence.
fn fence(n: usize) -> String {
    let (area, status, evidence) = match n % 4 {
        0 => (
            "engine",
            "shipped",
            "verified_by: [\"test: tests/flow.rs::real_case\"]\n",
        ),
        1 => (
            "scaffold",
            "shipped",
            "verified_by: [\"gap: test: no test yet\"]\n",
        ),
        2 => ("engine", "building", ""),
        _ => ("scaffold", "planned", ""),
    };
    let extra = if n == 3 {
        "summary: \"A summary the fence already states\"\nkind: non-functional\nquality: reliability\nmeasure: \"Every run finishes in a minute\"\n"
    } else {
        ""
    };
    format!(
        "id: {}\nname: {}\narea: {area}\nstatus: {status}\n{evidence}{extra}",
        id(n),
        name(n)
    )
}

/// The prose that follows entry `n`'s fence, up to the next heading of
/// equal or higher depth.
fn prose(n: usize) -> String {
    match n % 3 {
        0 => format!("\nEntry {n} does one thing well. It does it twice.\n\n"),
        1 => format!(
            "\n- entry {n} as a list\n- with a [link](other.md#anchor)\n\n##### A deeper heading stays\n\nInside entry {n}.\n\n"
        ),
        _ => format!("\n| Field | Value |\n|---|---|\n| n | {n} |\n\nTrailing  spaces and `code` in {n}.  \n\n"),
    }
}

fn catalog() -> String {
    let mut text = String::from(
        "# Capabilities\n\n<!-- an example inside a comment\n#### example\n```yaml\nid: CAP-999\n```\n-->\n\n## Concept\n\nWhy the catalog exists.\n\n## Technical\n\n### First group\n\n",
    );
    for n in 1..=ENTRIES {
        if n == 10 {
            text.push_str("### Second group\n\nGroup notes stay in the overview.\n\n");
        }
        text.push_str(
            &[
                "#### ",
                &name(n),
                "\n\n```yaml\n",
                &fence(n),
                "```\n",
                &prose(n),
            ]
            .concat(),
        );
    }
    text.push_str("### Catalog record schema\n\nTrailing overview text.\n");
    text
}

fn repo(legacy: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["config", "user.name", "Catalog fixture"]);
    git(root, &["config", "user.email", "catalog@example.test"]);
    write(
        root,
        ".codeflow/project.toml",
        "schema_version = 1\ntier = 'standard'\nscaffold_version = '4.0.0'\nstack = 'rust'\nareas = ['engine', 'scaffold']\npolicy_armed = false\ngit_hooks = 'unwired'\npermission_preset = 'default'\n",
    );
    write(root, "tests/flow.rs", "#[test]\nfn real_case() {}\n");
    write(root, "docs/capabilities.md", legacy);
    write(root, "docs/other.md", "# Other\n\n## Anchor\n");
    for n in PAGES {
        write(
            root,
            &format!("docs/capabilities/{}-{}.md", id(n), name(n)),
            PAGE,
        );
    }
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "chore: seed the fixture"]);
    dir
}

fn success(output: &Output) -> String {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout.clone()).unwrap()
}

/// Split a record into its frontmatter mapping and the bytes after it.
fn split(record: &str) -> (serde_yaml::Mapping, String) {
    let rest = record.strip_prefix("---\n").expect("frontmatter");
    let (yaml, body) = rest.split_once("\n---\n").expect("closed frontmatter");
    (serde_yaml::from_str(yaml).unwrap(), body.to_string())
}

#[test]
fn migrate_registers_then_writes_seventeen_records_byte_for_byte() {
    let dir = repo(&catalog());
    let root = dir.path();
    let stdout = success(&cli(root, &["capability", "migrate"]));
    assert!(stdout.contains("migrated 17 capabilities"), "{stdout}");

    for n in 1..=ENTRIES {
        let file = format!("docs/capabilities/{}-{}.md", id(n), name(n));
        let record = std::fs::read_to_string(root.join(&file)).unwrap();
        let (fields, body) = split(&record);

        // Every fence field is carried unchanged; only the uid and the
        // missing required facets are added.
        let fence: serde_yaml::Mapping = serde_yaml::from_str(&fence(n)).unwrap();
        for (key, value) in &fence {
            assert_eq!(fields.get(key), Some(value), "{file}: {key:?}");
        }
        let added: Vec<&str> = fields
            .keys()
            .filter(|key| !fence.contains_key(*key))
            .map(|key| key.as_str().unwrap())
            .collect();
        for key in &added {
            assert!(
                ["uid", "summary", "kind", "audience", "stage"].contains(key),
                "{file}: unexpected {key}"
            );
        }
        if n == 3 {
            assert_eq!(fields["summary"], "A summary the fence already states");
            assert_eq!(fields["kind"], "non-functional");
        }

        // The registry binds the legacy number to the record's minted uid.
        let uid = fields["uid"].as_str().unwrap();
        let binding = git(
            root,
            &["show", &format!("codeflow/registry:ids/CAP/{n:03}.toml")],
        );
        assert!(binding.contains(uid), "{file}: {binding}");

        // Prose equals the text that followed the fence, byte for byte; a page
        // keeps its own text, anchors included, ahead of it.
        let expected = if PAGES.contains(&n) {
            format!("{PAGE}{}", prose(n))
        } else {
            format!("\n# {}: {}\n{}", id(n), name(n), prose(n))
        };
        assert_eq!(body, expected, "{file}");
    }

    let legacy = std::fs::read_to_string(root.join("docs/capabilities.md")).unwrap();
    assert_eq!(
        legacy,
        "# Capabilities\n\n<!-- an example inside a comment\n#### example\n```yaml\nid: CAP-999\n```\n-->\n\n## Concept\n\nWhy the catalog exists.\n\n## Technical\n\n### First group\n\n### Second group\n\nGroup notes stay in the overview.\n\n### Catalog record schema\n\nTrailing overview text.\n"
    );
}

/// Every file outside .git, and every ref, so a refusal can prove it wrote nothing.
fn snapshot(root: &Path) -> (BTreeMap<String, Vec<u8>>, String) {
    fn walk(root: &Path, dir: &Path, files: &mut BTreeMap<String, Vec<u8>>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.file_name().is_some_and(|name| name == ".git") {
                continue;
            }
            if path.is_dir() {
                walk(root, &path, files);
            } else {
                let relative = path.strip_prefix(root).unwrap().display().to_string();
                files.insert(relative, std::fs::read(&path).unwrap());
            }
        }
    }
    let mut files = BTreeMap::new();
    walk(root, root, &mut files);
    (files, git(root, &["for-each-ref"]))
}

fn assert_refused_without_writes(legacy: &str, expected: &str) {
    let dir = repo(legacy);
    let root = dir.path();
    let before = snapshot(root);
    let output = cli(root, &["capability", "migrate"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(stderr.contains(expected), "{stderr}");
    assert_eq!(snapshot(root), before, "a refusal wrote something");
    assert!(!before.1.contains("codeflow/registry"));
}

#[test]
fn text_between_a_heading_and_its_fence_refuses_before_any_write() {
    let legacy = catalog().replacen(
        "#### entry-4\n\n```yaml",
        "#### entry-4\n\nWhose sentence is this?\n\n```yaml",
        1,
    );
    assert_refused_without_writes(
        &legacy,
        "ambiguous prose span for CAP-004: text between its heading and its fence",
    );
}

#[test]
fn two_fences_under_one_heading_refuse_before_any_write() {
    let legacy = catalog().replacen("#### entry-9\n\n", "", 1);
    assert_refused_without_writes(
        &legacy,
        "ambiguous prose span for CAP-008: another capability fence follows",
    );
}

#[test]
fn a_fence_with_no_heading_refuses_before_any_write() {
    let legacy = format!(
        "```yaml\nid: CAP-018\nname: orphan\narea: engine\nstatus: planned\n```\n\nOrphan prose.\n\n{}",
        catalog()
    );
    assert_refused_without_writes(&legacy, "ambiguous prose span for CAP-018: no heading");
}

#[test]
fn a_page_under_another_name_refuses_before_any_write() {
    let dir = repo(&catalog());
    let root = dir.path();
    write(root, "docs/capabilities/CAP-005-renamed.md", PAGE);
    let before = snapshot(root);
    let output = cli(root, &["capability", "migrate"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(stderr.contains("ambiguous page for CAP-005"), "{stderr}");
    assert_eq!(snapshot(root), before);
}

#[test]
fn an_unknown_fence_key_refuses_before_any_write() {
    let legacy = catalog().replacen("name: entry-6\n", "name: entry-6\nowner: someone\n", 1);
    assert_refused_without_writes(&legacy, "fence key `owner` is not a record field");
}

/// AC-3 journey: the built binary converts a 17-entry catalog in a temporary
/// repository and `validate --docs` accepts the result.
#[test]
fn journey_migrate_then_validate_docs_is_green() {
    let dir = repo(&catalog());
    let root = dir.path();
    success(&cli(root, &["validate", "--docs"]));
    success(&cli(root, &["capability", "migrate"]));
    let records = std::fs::read_dir(root.join("docs/capabilities"))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("CAP-"))
        .count();
    assert_eq!(records, ENTRIES);
    success(&cli(root, &["validate", "--docs"]));
}
