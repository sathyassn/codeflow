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

#[path = "support/forged_graph.rs"]
mod forged_graph;

#[test]
fn security_forged_commit_graph_refuses_ancestry_and_merge_base() {
    let dir = tempfile::tempdir().unwrap();
    let repo = Repository::init(dir.path()).unwrap();
    let base = commit(&repo, None, "base");
    let middle = commit(&repo, Some(base), "source change");
    let forged = commit(&repo, Some(middle), "revert");
    let head = commit(&repo, Some(forged), "tip");
    repo.reference("refs/heads/main", head, true, "fixture")
        .unwrap();
    assert!(history::descendant_of(&repo, head, middle).unwrap());
    assert_eq!(history::merge_base(&repo, head, middle).unwrap(), middle);
    forged_graph::plant(dir.path(), &forged.to_string(), &base.to_string());
    let output = history::command()
        .arg("-C")
        .arg(dir.path())
        .args(["rev-list", "--count", &format!("{base}..HEAD")])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "3");
    let repo = Repository::open(dir.path()).unwrap();
    for result in [
        history::descendant_of(&repo, head, middle).map(|_| ()),
        history::merge_base(&repo, head, middle).map(|_| ()),
    ] {
        assert!(result.unwrap_err().message().contains("cannot verify"));
    }
}

#[test]
fn security_graph_chain_and_alternate_graphs_are_refused() {
    for alternate in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        let head = commit(&repo, None, "base");
        let objects = if alternate {
            let objects = dir.path().join("alternate-objects");
            std::fs::create_dir_all(objects.join("info")).unwrap();
            std::fs::create_dir_all(repo.path().join("objects/info")).unwrap();
            std::fs::write(
                repo.path().join("objects/info/alternates"),
                "../../alternate-objects\n",
            )
            .unwrap();
            objects
        } else {
            repo.path().join("objects")
        };
        std::fs::create_dir_all(objects.join("info/commit-graphs")).unwrap();
        std::fs::write(
            objects.join("info/commit-graphs/commit-graph-chain"),
            "planted\n",
        )
        .unwrap();
        assert!(history::parents(&repo, &repo.find_commit(head).unwrap())
            .unwrap_err()
            .message()
            .contains("cannot verify"));
    }
}

#[test]
fn security_ordinary_graphs_and_split_chains_remain_allowed() {
    for split in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        let base = commit(&repo, None, "base");
        let middle = commit(&repo, Some(base), "middle");
        repo.reference("refs/heads/main", middle, true, "fixture")
            .unwrap();
        let write = || {
            let mut command = std::process::Command::new("git");
            command
                .arg("-C")
                .arg(dir.path())
                .args(["commit-graph", "write", "--reachable"]);
            if split {
                command.arg("--split=no-merge");
            }
            let out = command.output().unwrap();
            assert!(out.status.success(), "{out:?}");
        };
        write();
        let head = commit(&repo, Some(middle), "tip");
        repo.reference("refs/heads/main", head, true, "fixture")
            .unwrap();
        write();
        let repo = Repository::open(dir.path()).unwrap();
        assert!(history::descendant_of(&repo, head, base).unwrap());
        assert_eq!(history::merge_base(&repo, middle, head).unwrap(), middle);
        history::check_command(dir.path()).unwrap();
    }
}

#[test]
fn security_registry_spawned_history_reads_refuse_grafts() {
    let dir = tempfile::tempdir().unwrap();
    let repo = Repository::init(dir.path()).unwrap();
    let base = commit(&repo, None, "base");
    let head = commit(&repo, Some(base), "head");
    repo.reference("refs/heads/main", head, true, "fixture")
        .unwrap();
    std::fs::create_dir_all(repo.path().join("info")).unwrap();
    std::fs::write(repo.path().join("info/grafts"), format!("{head} {base}\n")).unwrap();
    let git = codeflow_core::ids::Git::new(dir.path());
    for query in [
        vec!["rev-list", "HEAD"],
        vec!["log", "--format=%H"],
        vec!["merge-base", "HEAD", "HEAD"],
        vec!["diff-tree", "HEAD"],
    ] {
        assert!(git
            .run(&query)
            .unwrap_err()
            .to_string()
            .contains("cannot verify"));
    }
}

#[test]
fn security_forged_graph_in_an_alternate_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let repo = Repository::init(dir.path()).unwrap();
    let base = commit(&repo, None, "base");
    let middle = commit(&repo, Some(base), "middle");
    let forged = commit(&repo, Some(middle), "forged");
    let head = commit(&repo, Some(forged), "tip");
    repo.reference("refs/heads/main", head, true, "fixture")
        .unwrap();
    forged_graph::plant(dir.path(), &forged.to_string(), &base.to_string());
    drop(repo);
    let objects = dir.path().join(".git/objects");
    let alternate = dir.path().join("alternate");
    std::fs::rename(&objects, &alternate).unwrap();
    std::fs::create_dir_all(objects.join("info")).unwrap();
    std::fs::write(objects.join("info/alternates"), "../../alternate\n").unwrap();
    let repo = Repository::open(dir.path()).unwrap();
    assert!(history::descendant_of(&repo, head, middle)
        .unwrap_err()
        .message()
        .contains("cannot verify"));
    assert!(history::merge_base(&repo, head, middle).is_err());
}

#[test]
fn security_ordinary_octopus_graph_remains_allowed() {
    let dir = tempfile::tempdir().unwrap();
    let repo = Repository::init(dir.path()).unwrap();
    let base = commit(&repo, None, "base");
    let ids: Vec<Oid> = ["first", "second", "third"]
        .iter()
        .map(|name| commit(&repo, Some(base), name))
        .collect();
    let parents: Vec<_> = ids
        .iter()
        .map(|id| repo.find_commit(*id).unwrap())
        .collect();
    let sig = Signature::now("History test", "history@example.invalid").unwrap();
    let head = repo
        .commit(
            None,
            &sig,
            &sig,
            "octopus",
            &parents[0].tree().unwrap(),
            &parents.iter().collect::<Vec<_>>(),
        )
        .unwrap();
    repo.reference("refs/heads/main", head, true, "fixture")
        .unwrap();
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir.path())
        .args(["commit-graph", "write", "--reachable"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let repo = Repository::open(dir.path()).unwrap();
    assert!(history::descendant_of(&repo, head, base).unwrap());
    assert_eq!(
        history::parents(&repo, &repo.find_commit(head).unwrap()).unwrap(),
        ids
    );
}
