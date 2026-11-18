//! AC-9: counts at the Git boundary, including the merge rule's added text.

use super::{check, git::Git, r22_fixture::*};

const OTHER: &[u8] = b"---\nid: TSK-001\nuid: 00000000-0000-4000-8000-000000000002\n---\n";

fn fixture(tips: usize) -> (tempfile::TempDir, git2::Repository, String, String) {
    let (dir, repo, base) = repository(b"README.md", b"base");
    repo.reference("refs/heads/codeflow/registry", base, true, "fixture")
        .unwrap();
    let head = crate::git::add_commit(&repo, &[(TASK, TEXT)]);
    for n in 0..tips {
        let text = if n % 2 == 0 { OTHER } else { TEXT };
        let tip = crate::git::add_commit(
            &repo,
            &[(TASK, text), (b"README.md", n.to_string().as_bytes())],
        );
        repo.reference(&format!("refs/heads/topic-{n:03}"), tip, true, "fixture")
            .unwrap();
    }
    // Only the topic tips hold records. The range head need not have a ref.
    repo.reference("refs/heads/main", base, true, "fixture")
        .unwrap();
    (dir, repo, base.to_string(), head.to_string())
}

fn measured(tips: usize, merge: bool) -> (usize, usize) {
    let (dir, repo, base, head) = fixture(tips);
    let git = Git::new(dir.path());
    let report = if merge {
        check::merge_rule(&git, &base, &head).unwrap()
    } else {
        check::check(&git, None).unwrap()
    };
    if merge {
        let expected: Vec<_> = (0..tips)
            .filter(|n| n % 2 == 0)
            .map(|n| format!("uniqueness: TSK-001 also exists on topic-{n:03} as a different record (project-management/tasks/TSK-001.md); renumber this one with `codeflow ids retarget TSK-001`"))
            .collect();
        assert_eq!(
            report
                .blocks
                .iter()
                .filter(|line| line.starts_with("uniqueness:"))
                .cloned()
                .collect::<Vec<_>>(),
            expected
        );
        assert!(!report.passed());
    } else {
        assert!(report.passed(), "{report:?}");
    }
    let reads = git.reads.borrow();
    let children = reads
        .children
        .iter()
        .filter(|args| args.starts_with("ls-tree ") || args.starts_with("cat-file "))
        .count();
    let record_blobs = [
        repo.blob(TEXT).unwrap().to_string(),
        repo.blob(OTHER).unwrap().to_string(),
    ];
    let blobs: Vec<_> = reads
        .blobs
        .iter()
        .filter(|oid| record_blobs.contains(oid))
        .collect();
    eprintln!(
        "AC-9 merge={merge} tips={tips} listing/read children={children} record blob reads={}",
        blobs.len()
    );
    (children, blobs.len())
}

#[test]
fn ac9_merge_rule_has_fixed_children_and_reads_each_blob_once() {
    let small = measured(5, true);
    let large = measured(50, true);
    assert_eq!(small, large);
    assert_eq!(large.1, 2);
}

#[test]
fn ac9_ids_check_has_fixed_children_and_reads_each_blob_once() {
    let small = measured(5, false);
    let large = measured(50, false);
    assert_eq!(small, large);
    assert_eq!(large.1, 2);
}

#[test]
fn ac9_missing_record_blob_keeps_the_refusal() {
    let (dir, repo, base, head) = fixture(5);
    let oid = repo.blob(OTHER).unwrap();
    remove_blob(&repo, OTHER);
    let expected = format!(
        "git cat-file: requested blob {oid} is missing; repair the repository objects, then retry"
    );
    for result in [
        check::merge_rule(&Git::new(dir.path()), &base, &head),
        check::check(&Git::new(dir.path()), None),
    ] {
        assert!(result.unwrap_err().to_string().contains(&expected));
    }
}
