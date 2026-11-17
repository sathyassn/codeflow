//! AC-9 at the CLI boundary. Trace only the command under test.

#[path = "support/line_adoption.rs"]
mod fixture;

const PATH: &str = "project-management/tasks/TSK-900.md";
const TEXT: &str = "---\nid: TSK-900\nuid: 00000000-0000-4000-8000-000000000900\n---\n";

fn refs(tips: usize) -> fixture::Line {
    let f = fixture::Line::untracked();
    f.git(&["rm", fixture::EPIC_PATH]);
    let base = f.commit("docs: empty the record tree");
    f.git(&["update-ref", "refs/heads/main", &base]);
    f.git(&["branch", "codeflow/registry"]);
    f.write(PATH, TEXT);
    f.commit("docs: add record");
    for n in 0..tips {
        f.write("README.md", &format!("tip {n}\n"));
        f.commit("chore: distinct tip");
        f.git(&["branch", &format!("topic-{n:03}")]);
    }
    // The checked-out line already aliases the final topic, so precisely
    // `tips` distinct code-ref tips hold the record tree.
    f
}

fn measured(tips: usize) -> usize {
    let f = refs(tips);
    let trace = f.root().join("trace.json");
    let out = f
        .command(env!("CARGO_BIN_EXE_codeflow"))
        .args(["ids", "check"])
        .env("GIT_TRACE2_EVENT", &trace)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", fixture::output(&out));
    let children = std::fs::read_to_string(trace)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .filter(|event| event["event"] == "start")
        .filter(|event| {
            event["argv"]
                .as_array()
                .is_some_and(|args| args.iter().any(|arg| arg == "ls-tree" || arg == "cat-file"))
        })
        .count();
    eprintln!("AC-9 CLI tips={tips} listing/read children={children}");
    assert!(children > 0, "the trace must observe the batch read");
    children
}

fn treeish_tip_keeps_findings_and_exit(annotated: bool) {
    let f = refs(1);
    let repo = git2::Repository::open(f.root()).unwrap();
    let commit = repo.head().unwrap().peel_to_commit().unwrap();
    let signature = git2::Signature::now("Test", "test@example.test").unwrap();
    let tip = if annotated {
        repo.tag("annotated", commit.as_object(), &signature, "tag", false)
            .unwrap()
    } else {
        commit.tree_id()
    };
    let branch = "refs/heads/treeish";
    repo.reference(branch, commit.id(), true, "fixture")
        .unwrap();
    let expected = f
        .command(env!("CARGO_BIN_EXE_codeflow"))
        .args(["ids", "check"])
        .output()
        .unwrap();
    assert_eq!(
        expected.status.code(),
        Some(0),
        "{}",
        fixture::output(&expected)
    );
    repo.reference(branch, tip, true, "fixture").unwrap();
    let actual = f
        .command(env!("CARGO_BIN_EXE_codeflow"))
        .args(["ids", "check"])
        .output()
        .unwrap();
    assert_eq!(
        actual.status.code(),
        expected.status.code(),
        "{}",
        fixture::output(&actual)
    );
    assert_eq!(actual.stdout, expected.stdout);
    assert_eq!(actual.stderr, expected.stderr);
}

#[test]
fn ac9_ids_check_cli_annotated_tag_tip_keeps_findings_and_exit() {
    treeish_tip_keeps_findings_and_exit(true);
}

#[test]
fn ac9_ids_check_cli_tree_tip_keeps_findings_and_exit() {
    treeish_tip_keeps_findings_and_exit(false);
}

#[test]
fn ac9_ids_check_cli_has_fixed_children() {
    assert_eq!(measured(5), measured(50));
}

#[test]
fn ac9_ids_check_cli_missing_blob_keeps_the_refusal() {
    let f = refs(5);
    let repo = git2::Repository::open(f.root()).unwrap();
    let oid = repo.blob(TEXT.as_bytes()).unwrap().to_string();
    std::fs::remove_file(repo.path().join("objects").join(&oid[..2]).join(&oid[2..])).unwrap();
    let out = f
        .command(env!("CARGO_BIN_EXE_codeflow"))
        .args(["ids", "check"])
        .output()
        .unwrap();
    assert_ne!(out.status.code(), Some(0));
    assert!(fixture::output(&out).contains(&format!("requested blob {oid} is missing")));
}
