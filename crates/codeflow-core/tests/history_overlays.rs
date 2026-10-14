//! Judging history must refuse local parent overlays (TSK-287).
use codeflow_core::workgraph::{acceptance, history, lifecycle::Graph, line_adoption, readiness};
use git2::{Oid, Repository, Signature};

fn commit(repo: &Repository, parent: Option<Oid>, message: &str) -> Oid {
    let mut builder = repo.treebuilder(None).unwrap();
    if message == "source change" {
        let blob = repo.blob(b"unreviewed source change").unwrap();
        builder.insert("source.rs", blob, 0o100_644).unwrap();
    }
    let tree = builder.write().unwrap();
    let tree = repo.find_tree(tree).unwrap();
    let sig = Signature::now("History test", "history@example.invalid").unwrap();
    let parent = parent.map(|id| repo.find_commit(id).unwrap());
    repo.commit(
        None,
        &sig,
        &sig,
        message,
        &tree,
        &parent.iter().collect::<Vec<_>>(),
    )
    .unwrap()
}

fn overlaid(check: impl Fn(&std::path::Path, Oid, Oid) -> Result<(), String>) {
    for replace in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        let base = commit(&repo, None, "base");
        let source = commit(&repo, Some(base), "source change");
        let head = commit(&repo, Some(source), "revert");
        check(dir.path(), base, head).unwrap();
        if replace {
            let replacement = commit(&repo, Some(base), "replacement");
            repo.reference(&format!("refs/replace/{head}"), replacement, false, "test")
                .unwrap();
        } else {
            std::fs::create_dir_all(repo.path().join("info")).unwrap();
            std::fs::write(repo.path().join("info/grafts"), format!("{head} {base}\n")).unwrap();
        }
        let error = check(dir.path(), base, head).expect_err("overlaid history was accepted");
        assert!(error.contains("cannot verify"), "{error}");
    }
}

#[test]
fn journey_scope_refuses_grafts_and_replace_refs() {
    overlaid(|root, base, head| {
        acceptance::owned_paths(root, &head.to_string(), &[&base.to_string()]).map(|_| ())
    });
}

#[test]
fn epic_line_coverage_refuses_grafts_and_replace_refs() {
    overlaid(|root, base, head| {
        line_adoption::check_range(
            root,
            "integration/EPC-001-history",
            &base.to_string(),
            &base.to_string(),
            &head.to_string(),
        )
        .map(|_| ())
    });
}

#[test]
fn acceptance_binding_refuses_grafts_and_replace_refs() {
    overlaid(|root, base, head| {
        let repo = Repository::open(root).unwrap();
        acceptance::completions_in_range(
            &repo,
            &base.to_string(),
            &head.to_string(),
            None,
            &acceptance::Criteria::Frozen,
            &acceptance::RunBases::new([base]),
            None,
            &[],
        )
        .map(|_| ())
    });
}

#[test]
fn reopen_detection_refuses_grafts_and_replace_refs() {
    overlaid(|root, base, head| {
        let repo = Repository::open(root).unwrap();
        let after = Graph::from_revision(&repo, &head.to_string()).unwrap();
        acceptance::reopened_criteria(&repo, &base.to_string(), &head.to_string(), &after)
            .map(|_| ())
    });
}

#[test]
fn range_bases_refuse_grafts_and_replace_refs() {
    overlaid(|root, base, head| {
        readiness::selections_in_range(root, &base.to_string(), &head.to_string()).map(|_| ())
    });
}

#[test]
fn reader_refuses_each_overlay_and_reachable_shallow_boundary() {
    overlaid(|root, base, head| {
        let repo = Repository::open(root).unwrap();
        history::merge_base(&repo, base, head)
            .map(|_| ())
            .map_err(|error| error.to_string())
    });
    let dir = tempfile::tempdir().unwrap();
    let repo = Repository::init(dir.path()).unwrap();
    let base = commit(&repo, None, "base");
    let head = commit(&repo, Some(base), "head");
    std::fs::write(repo.path().join("shallow"), format!("{head}\n")).unwrap();
    let repo = Repository::open(dir.path()).unwrap();
    let error = history::parents(&repo, &repo.find_commit(head).unwrap()).unwrap_err();
    assert!(error.message().contains("cannot verify"), "{error}");
    assert!(history::merge_base(&repo, head, base)
        .unwrap_err()
        .message()
        .contains("cannot verify"));
    let mut walk = history::revwalk(&repo).unwrap();
    walk.push(head).unwrap();
    assert!(walk
        .next()
        .unwrap()
        .unwrap_err()
        .message()
        .contains("cannot verify"));
    // A boundary at a hidden base does not cut the judged span.
    std::fs::write(repo.path().join("shallow"), format!("{base}\n")).unwrap();
    let repo = Repository::open(dir.path()).unwrap();
    assert_eq!(history::merge_base(&repo, head, base).unwrap(), base);
    assert!(history::descendant_of(&repo, head, base).unwrap());
}
