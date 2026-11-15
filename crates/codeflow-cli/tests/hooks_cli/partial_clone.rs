//! Partial-clone push journeys. Every transport stays under the fixture root.
use super::*;
use git2::{Oid, Repository, Signature};

struct Fixture {
    dir: tempfile::TempDir,
    clone: std::path::PathBuf,
    upstream_url: String,
    destination_url: String,
    head: Oid,
}

impl Fixture {
    fn new(merge: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "--bare", "upstream.git"]);
        git(dir.path(), &["init", "--bare", "destination.git"]);
        let upstream = Repository::open_bare(dir.path().join("upstream.git")).unwrap();
        upstream
            .config()
            .unwrap()
            .set_bool("uploadpack.allowFilter", true)
            .unwrap();
        upstream
            .config()
            .unwrap()
            .set_bool("uploadpack.allowAnySHA1InWant", true)
            .unwrap();
        let sig = Signature::now("Fixture", "fixture@example.invalid").unwrap();
        let commit = |tree, parents: &[Oid]| {
            let tree = upstream.find_tree(tree).unwrap();
            let parents: Vec<_> = parents
                .iter()
                .map(|id| upstream.find_commit(*id).unwrap())
                .collect();
            upstream
                .commit(
                    None,
                    &sig,
                    &sig,
                    "fix: partial clone fixture",
                    &tree,
                    &parents.iter().collect::<Vec<_>>(),
                )
                .unwrap()
        };
        let empty = upstream.treebuilder(None).unwrap().write().unwrap();
        let base = commit(empty, &[]);
        let ordinary = upstream.blob(b"ordinary contents\n").unwrap();
        let side = upstream.blob(b"other side contents\n").unwrap();
        let mut tree = upstream.treebuilder(None).unwrap();
        tree.insert("ordinary.txt", ordinary, 0o100_644).unwrap();
        let left = commit(tree.write().unwrap(), &[base]);
        let head = if merge {
            let mut other = upstream.treebuilder(None).unwrap();
            other.insert("side.txt", side, 0o100_644).unwrap();
            let right = commit(other.write().unwrap(), &[base]);
            tree.insert("side.txt", side, 0o100_644).unwrap();
            commit(tree.write().unwrap(), &[left, right])
        } else {
            left
        };
        upstream
            .reference("refs/heads/fixture", head, true, "fixture")
            .unwrap();
        upstream
            .reference("refs/heads/main", base, true, "fixture")
            .unwrap();
        upstream.set_head("refs/heads/main").unwrap();
        // Seed only the advertised base in the second bare remote. No source
        // repository or worktree from the developer's checkout is consulted.
        let destination = Repository::open_bare(dir.path().join("destination.git")).unwrap();
        let source_objects = upstream.odb().unwrap();
        for id in [empty, base] {
            let object = source_objects.read(id).unwrap();
            destination
                .odb()
                .unwrap()
                .write(object.kind(), object.data())
                .unwrap();
        }
        destination
            .reference("refs/heads/main", base, true, "fixture")
            .unwrap();
        destination.set_head("refs/heads/main").unwrap();
        let clone = dir.path().join("clone");
        git(dir.path(), &["init", "-b", "feat/partial", "clone"]);
        let repo = Repository::open(&clone).unwrap();
        let upstream_url = format!("file://{}", upstream.path().display());
        let destination_url = format!("file://{}", destination.path().display());
        repo.remote("origin", &upstream_url).unwrap();
        repo.remote("destination", &destination_url).unwrap();
        repo.config()
            .unwrap()
            .set_bool("remote.origin.promisor", true)
            .unwrap();
        repo.config()
            .unwrap()
            .set_str("extensions.partialClone", "origin")
            .unwrap();
        git(
            &clone,
            &["fetch", "--filter=blob:none", "origin", "fixture"],
        );
        repo.reference("refs/heads/feat/partial", head, true, "fixture")
            .unwrap();
        repo.reference("refs/remotes/origin/main", base, true, "fixture")
            .unwrap();
        repo.reference_symbolic(
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
            true,
            "fixture",
        )
        .unwrap();
        assert!(repo.find_blob(ordinary).is_err());
        if merge {
            assert!(repo.find_blob(side).is_err());
        }
        let hooks = clone.join(".codeflow/git-hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        let shim = hooks.join("pre-push");
        std::fs::write(
            &shim,
            include_str!("../../../../assets/base/git-hooks/pre-push"),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
        repo.config()
            .unwrap()
            .set_str("core.hooksPath", hooks.to_str().unwrap())
            .unwrap();
        Self {
            dir,
            clone,
            upstream_url,
            destination_url,
            head,
        }
    }

    fn push_check(&self, trace: &Path) -> Output {
        let input = format!(
            "refs/heads/feat/partial {} refs/heads/feat/partial {}\n",
            self.head,
            "0".repeat(40)
        );
        run_with_stdin(
            codeflow()
                .current_dir(&self.clone)
                .env_remove("GIT_NO_LAZY_FETCH")
                .env("GIT_TRACE", trace)
                .args(["git-hook", "pre-push", "destination", &self.destination_url]),
            &input,
        )
    }
}

#[test]
fn partial_clone_push_refuses_offline_and_passes_after_named_commands() {
    let mut failures = Vec::new();
    for merge in [false, true] {
        let fixture = Fixture::new(merge);
        let repo = Repository::open(&fixture.clone).unwrap();
        repo.config()
            .unwrap()
            .set_str(
                "remote.origin.url",
                &format!("file://{}/unreachable.git", fixture.dir.path().display()),
            )
            .unwrap();
        let trace = fixture.dir.path().join("offline.trace");
        let output = fixture.push_check(&trace);
        let message = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        eprintln!("push merge={merge}:\n{message}");
        assert!(!output.status.success(), "{message}");
        let text = std::fs::read_to_string(&trace).unwrap();
        // The push check reads the second local bare destination's ref
        // advertisement. That upload-pack is expected; a promisor transport
        // or any fetch child is not.
        let destination = fixture.destination_url.strip_prefix("file://").unwrap();
        let fetch_trace = text
            .lines()
            .filter(|line| {
                line.contains("fetch ")
                    || (line.contains("upload-pack") && !line.contains(destination))
            })
            .collect::<Vec<_>>()
            .join("\n");
        eprintln!("push fetch trace merge={merge}:\n{fetch_trace}");
        if !fetch_trace.is_empty()
            || !message.contains("partial clone")
            || !message.contains("ordinary.txt")
            || (merge && !message.contains("side.txt"))
        {
            failures.push(format!(
                "merge={merge}: missing partial-clone guidance or a fetch child was started"
            ));
            continue;
        }
        repo.config()
            .unwrap()
            .set_str("remote.origin.url", &fixture.upstream_url)
            .unwrap();
        // Interpret only the fixed remediation grammar and typed object IDs.
        // No diagnostic is passed to a shell or executed as arbitrary text.
        let mut fetched = std::collections::BTreeSet::new();
        for tail in message
            .split("GIT_NO_LAZY_FETCH=0 git cat-file -e ")
            .skip(1)
        {
            let id = tail
                .get(..40)
                .and_then(|id| id.parse::<Oid>().ok())
                .expect("typed object ID in fetch command");
            if fetched.insert(id) {
                let output = git_command()
                    .current_dir(&fixture.clone)
                    .env("GIT_NO_LAZY_FETCH", "0")
                    .args(["cat-file", "-e", &id.to_string()])
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }
        assert!(!fetched.is_empty(), "{message}");
        let repaired = fixture.push_check(&fixture.dir.path().join("repaired.trace"));
        assert!(
            repaired.status.success(),
            "{}{}",
            String::from_utf8_lossy(&repaired.stdout),
            String::from_utf8_lossy(&repaired.stderr)
        );
        repo.config()
            .unwrap()
            .remove("extensions.partialclone")
            .unwrap();
        repo.config()
            .unwrap()
            .set_bool("remote.origin.promisor", false)
            .unwrap();
        let full = fixture.push_check(&fixture.dir.path().join("full.trace"));
        assert_eq!(
            repaired.status.code(),
            full.status.code(),
            "full-clone judgment differs"
        );
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
