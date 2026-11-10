//! A landing candidate branch is pushable and judged against its line
//! (TSK-1351). The fixture has the shape of coord/ci's refused push of
//! `candidate/epc-022-c4a` (2026-11-05): a bound line, two task branches
//! each completed at its reviewed head, a planning branch that amends
//! another task's criteria, and member heads the destination advertises,
//! each containing the line, as decoy bases. Every push goes through the
//! installed pre-push shim with the built binary.
use super::*;

const LINE: &str = "integration/EPC-001-line-work";
const CANDIDATE: &str = "candidate/epc-001-c1";
const MEMBERS: [&str; 3] = [
    "task/TSK-001-one",
    "task/TSK-002-two",
    "plan/EPC-001-amend-three",
];
/// TSK-003's criteria after the planning branch's amendment.
const AMENDED: &str = "- AC-1 When run, the system shall work.\n- AC-2 (journey) On a fresh project, the command shall succeed.\n- AC-3 When rerun, the system shall still work.\n";

/// `git` with the shipped pre-push shim installed and the built binary as
/// its hook binary.
fn hooked_git(root: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("CODEFLOW_HOOK_BINARY", env!("CARGO_BIN_EXE_codeflow"))
        .env("CODEFLOW_HOME", isolated_home())
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("CODEFLOW_PR_BODY")
        .env_remove("GITHUB_EVENT_NAME")
        .env_remove("GITHUB_HEAD_REF")
        .env_remove("GITHUB_BASE_REF")
        .env_remove("CI_PIPELINE_SOURCE")
        .env_remove("CI_MERGE_REQUEST_IID")
        .env_remove("BITBUCKET_PR_ID")
        .env_remove("CLAUDECODE")
        .env_remove("CODEX_SESSION_ID")
        .env_remove("CODEX_THREAD_ID")
        .env_remove("CODEX_CI")
        .env_remove("GROK_SESSION_ID");
    cmd
}

/// A real `git push origin <refspec>` through the installed hook.
fn push(root: &Path, refspec: &str) -> (i32, String) {
    let out = hooked_git(root)
        .args(["push", "origin", refspec])
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

/// Push without hooks, to set up the destination.
fn seed_push(root: &Path, refs: &[&str]) {
    let mut args = vec!["-c", "core.hooksPath=/dev/null", "push", "-q", "origin"];
    args.extend_from_slice(refs);
    git(root, &args);
}

/// The commit `origin/<branch>` names, or `None` when the destination does
/// not hold the branch.
fn remote_tip(root: &Path, branch: &str) -> Option<String> {
    let listed = git_out(
        root,
        &["ls-remote", "origin", &format!("refs/heads/{branch}")],
    );
    listed.split_whitespace().next().map(str::to_string)
}

struct Fixture {
    dir: tempfile::TempDir,
    /// The line's tip, which every member and the candidate are cut from.
    line_tip: String,
}

impl Fixture {
    fn root(&self) -> &Path {
        self.dir.path()
    }

    /// The candidate cut from the line's tip with every member merged by a
    /// `--no-ff` merge, checked out; returns its head.
    fn candidate(&self, name: &str) -> String {
        let root = self.root();
        git(root, &["switch", "-q", "-C", name, &self.line_tip]);
        for member in MEMBERS {
            git(
                root,
                &[
                    "merge",
                    "-q",
                    "--no-ff",
                    "-m",
                    &format!("chore(merge): land {member}"),
                    member,
                ],
            );
        }
        head(root)
    }
}

/// The c4a-shaped repository: `main` plans TSK-001 to TSK-003 on the line;
/// TSK-001 and TSK-002 are built and completed on their own branches,
/// `plan/EPC-001-amend-three` amends TSK-003's criteria, and the destination
/// holds `main`, the line and every member head.
fn fixture() -> Fixture {
    let dir = repo(
        &[
            ("TSK-001", OWN_JOURNEY),
            ("TSK-002", OWN_JOURNEY),
            ("TSK-003", OWN_JOURNEY),
        ],
        ", \"test_gate_on_push\": \"block\"",
    );
    let root = dir.path();
    let on_line = |id: &str, criteria: &str, closeout: &str| {
        task(id, "todo", criteria, closeout).replace(
            "integration_target: main",
            &format!("integration_target: {LINE}"),
        )
    };
    for id in ["TSK-001", "TSK-002", "TSK-003"] {
        write(
            root,
            &record_path(id),
            &on_line(id, OWN_JOURNEY, "Pending.\n"),
        );
    }
    let line_tip = commit(root, "docs(records): target the line");
    git(root, &["branch", LINE]);
    let remote = root.join(".git/destination.git");
    git(
        root,
        &[
            "init",
            "--bare",
            "-q",
            "-b",
            "main",
            remote.to_str().unwrap(),
        ],
    );
    git(root, &["remote", "add", "origin", remote.to_str().unwrap()]);
    seed_push(root, &["main", LINE]);

    for (id, branch, path) in [
        ("TSK-001", MEMBERS[0], "src/one.rs"),
        ("TSK-002", MEMBERS[1], "src/two.rs"),
    ] {
        git(root, &["switch", "-q", "-c", branch, LINE]);
        write(root, path, &format!("// {id}\n"));
        let reviewed = commit(root, &format!("feat: build {id}"));
        let done = on_line(id, OWN_JOURNEY, &valid_block(&reviewed))
            .replace("status: todo", "status: complete");
        write(root, &record_path(id), &done);
        commit(root, &format!("docs(records): complete {id}"));
    }
    git(root, &["switch", "-q", "-c", MEMBERS[2], LINE]);
    write(
        root,
        &record_path("TSK-003"),
        &on_line("TSK-003", AMENDED, "Pending.\n"),
    );
    commit(root, "docs(records): amend TSK-003");
    seed_push(root, &MEMBERS);
    git(root, &["switch", "-q", "main"]);

    let seed = codeflow()
        .args(["ids", "seed"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        seed.status.success(),
        "{}",
        String::from_utf8_lossy(&seed.stderr)
    );
    let hooks = root.join(".git/codeflow-hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    let shim = hooks.join("pre-push");
    std::fs::write(
        &shim,
        include_str!("../../../../assets/base/git-hooks/pre-push"),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    git(root, &["config", "core.hooksPath", hooks.to_str().unwrap()]);
    Fixture { dir, line_tip }
}

/// The push is refused by the candidate binding naming `cause`, runs no
/// `codeflow ci` against the default branch, and the destination does not
/// take `branch`'s new tip.
fn refused_by_binding(pushed: &(i32, String), what: &str, cause: &str) {
    assert_ne!(pushed.0, 0, "{what} must be refused: {}", pushed.1);
    for needle in ["work.landing_candidate", cause] {
        assert!(
            pushed.1.contains(needle),
            "{what}: missing {needle:?} in {}",
            pushed.1
        );
    }
    assert!(
        !pushed.1.contains("--into=main"),
        "{what} must never be judged against the default branch: {}",
        pushed.1
    );
}

/// `codeflow ci` with the pre-push hook's hand-off for the candidate
/// `name` at `head`, from `base`, into the line.
fn hand_off(fixture: &Fixture, name: &str, head: &str, base: &str) -> (i32, String) {
    let root = fixture.root();
    let remote = root.join(".git/destination.git");
    let url = remote.to_str().unwrap();
    let main = git_out(root, &["rev-parse", "main"]);
    let listed = git_out(
        root,
        &[
            "ls-remote",
            "--symref",
            url,
            "HEAD",
            "refs/heads/*",
            "refs/tags/*",
        ],
    );
    let mut child = codeflow()
        .args([
            "ci",
            "--base",
            base,
            "--head",
            head,
            "--branch",
            name,
            "--pushed-ref",
            &format!("refs/heads/{name}"),
            "--run-level",
            "block",
            "--policy-from",
            &main,
            "--commits-from",
            &main,
            "--destination",
            url,
            "--advertisement-stdin",
            &format!("--into={LINE}"),
        ])
        .current_dir(root)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    std::io::Write::write_all(
        &mut child.stdin.take().unwrap(),
        format!("{listed}\n").as_bytes(),
    )
    .unwrap();
    let out = child.wait_with_output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

/// A criteria change to TSK-001 that no member's reviewed head made.
fn foreign_criteria(root: &Path) {
    let record = std::fs::read_to_string(root.join(record_path("TSK-001"))).unwrap();
    write(
        root,
        &record_path("TSK-001"),
        &record.replace("shall work.", "shall mostly work."),
    );
}

/// AC-1, AC-3, AC-4 (first output): the c4a-shaped candidate pushes, judged
/// with base the line's tip and target the line, never with a member head
/// as base or `main` as target, and the destination holds it.
#[test]
fn a_landing_candidate_pushes_and_is_judged_against_its_line() {
    let fixture = fixture();
    let root = fixture.root();
    let candidate = fixture.candidate(CANDIDATE);
    let pushed = push(root, CANDIDATE);
    println!("{}", pushed.1);
    assert_eq!(pushed.0, 0, "the candidate push: {}", pushed.1);
    for needle in [
        format!("'{CANDIDATE}' is a landing candidate of '{LINE}': 3 member merge(s)"),
        format!("--base {} --head {candidate}", fixture.line_tip),
        format!("--into={LINE}"),
        "TSK-001 completion bound at".to_string(),
        "TSK-002 completion bound at".to_string(),
    ] {
        assert!(
            pushed.1.contains(&needle),
            "missing {needle:?}: {}",
            pushed.1
        );
    }
    assert!(!pushed.1.contains("--into=main"), "{}", pushed.1);
    assert_eq!(remote_tip(root, CANDIDATE), Some(candidate));
}

/// AC-3 and the builder checks: `codeflow ci` binds the candidate again
/// from its own reads and judges it as the line's next tip, with the line's
/// direct-commit rule and amendable criteria, so the planning merge's
/// amendment of TSK-003 is a note, never a block. A hand-off whose base is a
/// member head is refused by `codeflow ci` itself.
#[test]
fn codeflow_ci_binds_the_candidate_again_and_never_trusts_the_hand_off() {
    let fixture = fixture();
    let root = fixture.root();
    let candidate = fixture.candidate(CANDIDATE);
    let judged = hand_off(&fixture, CANDIDATE, &candidate, &fixture.line_tip);
    println!("{}", judged.1);
    assert_passes(&judged, "the bound candidate");
    for needle in [
        format!("landing candidate: '{CANDIDATE}' is bound to '{LINE}'"),
        format!("line tip: the head is judged as the next tip of '{LINE}' (a bound landing candidate's push)"),
        "line tip: no unadopted direct commit".to_string(),
        format!("acceptance binding: amendable (branch '{LINE}'"),
    ] {
        assert!(judged.1.contains(&needle), "missing {needle:?}: {}", judged.1);
    }
    assert!(!judged.1.contains("work.criteria_frozen"), "{}", judged.1);
    let decoy = git_out(root, &["rev-parse", MEMBERS[1]]);
    assert_blocks(
        &hand_off(&fixture, CANDIDATE, &candidate, &decoy),
        "a member head as base",
        &[
            "work.landing_candidate",
            &format!(
                "the range's base {} is not the line's fetched tip",
                &decoy[..9]
            ),
        ],
    );
}

/// AC-1 (negative), AC-4 (second output): a foreign criteria change that
/// enters by a direct commit on the candidate is refused, by the hook and by
/// `codeflow ci`, naming the commit.
#[test]
fn a_foreign_criteria_change_by_a_direct_commit_is_refused() {
    let fixture = fixture();
    let root = fixture.root();
    fixture.candidate(CANDIDATE);
    foreign_criteria(root);
    let direct = commit(root, "docs(records): loosen TSK-001");
    let pushed = push(root, CANDIDATE);
    println!("{}", pushed.1);
    refused_by_binding(
        &pushed,
        "a direct commit",
        &format!("{} on its first-parent chain", &direct[..9]),
    );
    assert_eq!(remote_tip(root, CANDIDATE), None);
    assert_blocks(
        &hand_off(&fixture, CANDIDATE, &direct, &fixture.line_tip),
        "the hand-off of a direct commit",
        &["work.landing_candidate", "is a direct commit"],
    );
}

/// AC-4 (third output): a foreign criteria change that enters by an evil
/// merge, a member merge whose tree differs from the clean merge of its
/// parents, is refused, naming the merge and the path.
#[test]
fn a_foreign_criteria_change_by_an_evil_merge_is_refused() {
    let fixture = fixture();
    let root = fixture.root();
    git(root, &["switch", "-q", "-C", CANDIDATE, &fixture.line_tip]);
    for member in &MEMBERS[..2] {
        git(
            root,
            &["merge", "-q", "--no-ff", "-m", "chore(merge): land", member],
        );
    }
    git(root, &["merge", "-q", "--no-ff", "--no-commit", MEMBERS[2]]);
    foreign_criteria(root);
    let evil = commit(root, "chore(merge): land plan/EPC-001-amend-three");
    let pushed = push(root, CANDIDATE);
    println!("{}", pushed.1);
    refused_by_binding(
        &pushed,
        "an evil merge",
        &format!(
            "merge {} is not the clean merge of its parents (it changes {})",
            &evil[..9],
            record_path("TSK-001")
        ),
    );
    assert_eq!(remote_tip(root, CANDIDATE), None);
}

/// AC-3: a name without the prefix is judged exactly as before: a `feat/`
/// branch of the same shape gets no candidate binding, so it is judged
/// against `main` and refused for the members' records.
#[test]
fn a_feat_branch_of_the_same_shape_is_judged_as_today() {
    let fixture = fixture();
    let root = fixture.root();
    fixture.candidate("feat/epc-001-c1");
    let pushed = push(root, "feat/epc-001-c1");
    println!("{}", pushed.1);
    assert_ne!(pushed.0, 0, "{}", pushed.1);
    assert!(!pushed.1.contains("landing candidate"), "{}", pushed.1);
    assert!(!pushed.1.contains("work.landing_candidate"), "{}", pushed.1);
    assert!(pushed.1.contains("--into=main"), "{}", pushed.1);
    assert!(pushed.1.contains("work.criteria_frozen"), "{}", pushed.1);
}

/// AC-3: a binding input that cannot be read refuses with its cause and
/// runs no `codeflow ci` against the default branch: here the line was
/// never fetched into this clone.
#[test]
fn an_unreadable_binding_input_refuses() {
    let fixture = fixture();
    let root = fixture.root();
    fixture.candidate(CANDIDATE);
    git(
        root,
        &["update-ref", "-d", &format!("refs/remotes/origin/{LINE}")],
    );
    let pushed = push(root, CANDIDATE);
    println!("{}", pushed.1);
    refused_by_binding(
        &pushed,
        "an unfetched line",
        "its first-parent chain reaches no fetched epic line tip",
    );
    assert!(!pushed.1.contains("codeflow ci --base"), "{}", pushed.1);
    assert_eq!(remote_tip(root, CANDIDATE), None);
}

/// AC-5: the line moves to the reviewed candidate in one fast-forward push,
/// and every member head is then on the line.
#[test]
fn the_line_moves_to_a_bound_candidate_in_one_push() {
    let fixture = fixture();
    let root = fixture.root();
    let candidate = fixture.candidate(CANDIDATE);
    assert_passes(&push(root, CANDIDATE), "the candidate");
    let landed = push(root, &format!("{CANDIDATE}:refs/heads/{LINE}"));
    println!("{}", landed.1);
    assert_passes(&landed, "the line push");
    assert!(
        landed.1.contains(&format!(
            "the push of '{LINE}' lands the landing candidate '{CANDIDATE}': a fast-forward from {} by 3 member merge(s)",
            &fixture.line_tip[..9]
        )),
        "{}",
        landed.1
    );
    assert_eq!(remote_tip(root, LINE), Some(candidate.clone()));
    for member in MEMBERS {
        git(root, &["merge-base", "--is-ancestor", member, &candidate]);
    }
}

/// AC-5: a fast-forward of the line to a candidate tip the binding refuses
/// (an evil merge) is refused with its cause, and the line stays.
#[test]
fn a_line_push_to_an_unbound_candidate_tip_is_refused() {
    let fixture = fixture();
    let root = fixture.root();
    git(
        root,
        &["switch", "-q", "-C", "candidate/evil", &fixture.line_tip],
    );
    git(root, &["merge", "-q", "--no-ff", "--no-commit", MEMBERS[0]]);
    foreign_criteria(root);
    let evil = commit(root, "chore(merge): land task/TSK-001-one");
    let landed = push(root, &format!("candidate/evil:refs/heads/{LINE}"));
    println!("{}", landed.1);
    refused_by_binding(
        &landed,
        "the line push of an evil candidate",
        &format!("lands the candidate 'candidate/evil', which is not a bound landing candidate of the line: merge {} is not the clean merge", &evil[..9]),
    );
    assert_eq!(remote_tip(root, LINE), Some(fixture.line_tip.clone()));
    // The same commit pushed from a ref no candidate names is judged as
    // any line push is, by the line's own rules.
    git(root, &["switch", "-q", "-c", "feat/evil", &evil]);
    git(root, &["branch", "-D", "candidate/evil"]);
    let ordinary = push(root, &format!("feat/evil:refs/heads/{LINE}"));
    assert!(
        !ordinary.1.contains("work.landing_candidate"),
        "{}",
        ordinary.1
    );
}

/// AC-5: every other line push is judged exactly as before: an ordinary
/// merge pushed to the line passes with no candidate binding, and a direct
/// commit pushed to the line keeps the line's `work.epic_line` refusal.
#[test]
fn an_ordinary_line_push_is_judged_as_today() {
    let fixture = fixture();
    let root = fixture.root();
    git(
        root,
        &["switch", "-q", "-C", "feat/land-one", &fixture.line_tip],
    );
    git(
        root,
        &[
            "merge",
            "-q",
            "--no-ff",
            "-m",
            "chore(merge): land",
            MEMBERS[0],
        ],
    );
    let merged = head(root);
    let landed = push(root, &format!("feat/land-one:refs/heads/{LINE}"));
    println!("{}", landed.1);
    assert_passes(&landed, "an ordinary line push");
    assert!(!landed.1.contains("landing candidate"), "{}", landed.1);
    assert_eq!(remote_tip(root, LINE), Some(merged));
    write(root, "src/direct.rs", "// direct\n");
    let direct = commit(root, "feat: a direct commit");
    let refused = push(root, &format!("feat/land-one:refs/heads/{LINE}"));
    assert_ne!(refused.0, 0, "{}", refused.1);
    assert!(refused.1.contains("work.epic_line"), "{}", refused.1);
    assert!(refused.1.contains(&direct[..9]), "{}", refused.1);
    assert!(
        !refused.1.contains("work.landing_candidate"),
        "{}",
        refused.1
    );
}
