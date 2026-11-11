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

/// The forge repository `origin` names. Git reaches it through the
/// fixture's `ssh` stand-in (`core.sshCommand`), which serves the local
/// bare repository, so the hook reads a real forge identity from `origin`
/// and pushes land locally.
const ORIGIN: &str = "ssh://localhost/acme/widgets.git";
/// The forge repository as `owner/name`.
const NWO: &str = "acme/widgets";
/// The policy workflow the default branch holds, declaring the trusted
/// admission check.
const POLICY_WORKFLOW: &str = ".github/workflows/codeflow-policy.yml";
const POLICY_TEXT: &str = "name: codeflow-policy\non:\n  pull_request_target:\n    types: [opened, synchronize]\npermissions:\n  contents: read\njobs:\n  commit-lint:\n    name: \"trusted: commit standards\"\n    runs-on: ubuntu-24.04\n    steps:\n      - run: \"true\"\n";
/// The trusted admission check the policy workflow declares.
const ADMISSION: &str = "trusted: commit standards";
/// GitHub Actions' app id, the producer the admission check must have.
const ACTIONS_APP: u64 = 15368;

/// Where the recorded forge keeps its answers, and its `gh` and `ssh`
/// stand-ins.
fn forge_dir(root: &Path) -> std::path::PathBuf {
    root.join(".git/forge")
}

/// A `PATH` with the recorded forge's `gh` first.
fn forge_path(root: &Path) -> std::ffi::OsString {
    let mut paths = vec![forge_dir(root).join("bin")];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    std::env::join_paths(paths).unwrap()
}

/// Install the recorded forge: an `ssh` stand-in that runs the git service
/// asked for on `remote`, and a `gh` stand-in that answers `gh api <path>`
/// from a file per path (a missing one answers 404).
fn record_forge(root: &Path, remote: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let forge = forge_dir(root);
    std::fs::create_dir_all(forge.join("bin")).unwrap();
    std::fs::create_dir_all(forge.join("answers")).unwrap();
    let ssh = forge.join("ssh");
    std::fs::write(
        &ssh,
        format!(
            "#!/bin/sh\n# Test stand-in for ssh: run the git service asked for on the fixture's bare repository.\nfor last; do :; done\ncase \"$last\" in\n  git-upload-pack*) exec git upload-pack '{0}' ;;\n  git-receive-pack*) exec git receive-pack '{0}' ;;\nesac\necho \"unexpected ssh command: $*\" >&2\nexit 1\n",
            remote.display()
        ),
    )
    .unwrap();
    let gh = forge.join("bin/gh");
    std::fs::write(
        &gh,
        format!(
            "#!/bin/sh\n# Test stand-in for gh: answer `gh api <path>` from the recorded forge.\nfor last; do :; done\nkey=$(printf '%s' \"$last\" | tr -c 'A-Za-z0-9' '_')\nif [ -f '{0}'/\"$key\" ]; then cat '{0}'/\"$key\"; exit 0; fi\necho \"HTTP 404: Not Found ($last)\" >&2\nexit 1\n",
            forge.join("answers").display()
        ),
    )
    .unwrap();
    for script in [&ssh, &gh] {
        std::fs::set_permissions(script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    git(root, &["config", "core.sshCommand", ssh.to_str().unwrap()]);
}

/// Record the forge's answer for the API `path`.
fn answer(root: &Path, path: &str, body: &str) {
    let key: String = path
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    std::fs::write(forge_dir(root).join("answers").join(key), body).unwrap();
}

/// The forge holds one open pull request into the line per head, numbered
/// from 1, each with the admission check green at its head.
fn admit(root: &Path, heads: &[&str]) {
    let pulls: Vec<String> = heads
        .iter()
        .zip(1..)
        .map(|(head, number)| {
            format!(
                "{{\"number\":{number},\"state\":\"open\",\"head\":{{\"sha\":\"{head}\"}},\"base\":{{\"ref\":\"{LINE}\"}}}}"
            )
        })
        .collect();
    answer(
        root,
        &format!("repos/{NWO}/pulls?state=open&base={LINE}&per_page=100&page=1"),
        &format!("[{}]", pulls.join(",")),
    );
    for head in heads {
        answer(
            root,
            &format!("repos/{NWO}/commits/{head}/check-runs?per_page=100&page=1"),
            &format!(
                "{{\"total_count\":1,\"check_runs\":[{{\"id\":1,\"name\":\"{ADMISSION}\",\"status\":\"completed\",\"conclusion\":\"success\",\"app\":{{\"id\":{ACTIONS_APP}}}}}]}}"
            ),
        );
    }
}

/// `git` with the shipped pre-push shim installed and the built binary as
/// its hook binary.
fn hooked_git(root: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.current_dir(root)
        .env("PATH", forge_path(root))
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
    push_many(root, &[refspec])
}

/// One real `git push origin <refspec>...` of every refspec through the
/// installed hook.
fn push_many(root: &Path, refspecs: &[&str]) -> (i32, String) {
    let out = hooked_git(root)
        .args(["push", "origin"])
        .args(refspecs)
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
    write(root, POLICY_WORKFLOW, POLICY_TEXT);
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
    record_forge(root, &remote);
    git(root, &["remote", "add", "origin", ORIGIN]);
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
    // Each member is the head of an open pull request into the line with
    // the trusted admission check green there.
    let heads: Vec<String> = MEMBERS
        .iter()
        .map(|member| git_out(root, &["rev-parse", member]))
        .collect();
    let admitted: Vec<&str> = heads.iter().map(String::as_str).collect();
    admit(root, &admitted);

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
    hand_off_with(fixture, name, head, base, &Claims::default())
}

/// The claims a hand-off makes, each as the pre-push hook makes it unless
/// set: `None` in an outer `Option` keeps the hook's value, `Some(None)`
/// leaves the claim out.
#[derive(Default)]
struct Claims<'a> {
    /// `--policy-from`; the hook passes `main`'s tip.
    policy_from: Option<Option<&'a str>>,
    /// `--into`; the hook passes the line.
    into: Option<Option<&'a str>>,
    /// Leave out `--destination` and `--advertisement-stdin`.
    no_advertisement: bool,
}

/// [`hand_off`] with `claims` in place of the hook's own.
fn hand_off_with(
    fixture: &Fixture,
    name: &str,
    head: &str,
    base: &str,
    claims: &Claims<'_>,
) -> (i32, String) {
    let root = fixture.root();
    let url = ORIGIN;
    let main = git_out(root, &["rev-parse", "main"]);
    let listed = git_out(
        root,
        &[
            "ls-remote",
            "--symref",
            "origin",
            "HEAD",
            "refs/heads/*",
            "refs/tags/*",
        ],
    );
    let pushed_ref = format!("refs/heads/{name}");
    let mut args: Vec<String> = [
        "ci",
        "--base",
        base,
        "--head",
        head,
        "--branch",
        name,
        "--pushed-ref",
        &pushed_ref,
        "--run-level",
        "block",
        "--commits-from",
        &main,
    ]
    .iter()
    .map(|arg| (*arg).to_string())
    .collect();
    if let Some(policy_from) = claims.policy_from.unwrap_or(Some(&main)) {
        args.extend(["--policy-from".to_string(), policy_from.to_string()]);
    }
    if !claims.no_advertisement {
        args.extend([
            "--destination".to_string(),
            url.to_string(),
            "--advertisement-stdin".to_string(),
        ]);
    }
    if let Some(into) = claims.into.unwrap_or(Some(LINE)) {
        args.push(format!("--into={into}"));
    }
    let mut child = codeflow()
        .args(&args)
        .env("PATH", forge_path(root))
        .current_dir(root)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    // Without the advertisement flag, stdin is never read: closing it
    // unwritten keeps the run from waiting on it.
    let mut stdin = child.stdin.take().unwrap();
    if !claims.no_advertisement {
        std::io::Write::write_all(&mut stdin, format!("{listed}\n").as_bytes()).unwrap();
    }
    drop(stdin);
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
        "each member is the admitted head of its open pull request (#1, #2, #3): admission at the head, not a person's review".to_string(),
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

/// AC-3: `codeflow ci` establishes every destination tip itself and
/// refuses a hand-off claim that is missing or conflicts with it: no
/// advertisement, no policy authority, an authority that is not the
/// advertised default tip, no `--into`, and an `--into` naming another
/// line. Each is refused by the candidate binding, never judged.
#[test]
fn codeflow_ci_refuses_missing_or_conflicting_hand_off_claims() {
    let fixture = fixture();
    let root = fixture.root();
    let candidate = fixture.candidate(CANDIDATE);
    let base = fixture.line_tip.clone();
    let member = git_out(root, &["rev-parse", MEMBERS[0]]);
    let not_default = format!(
        "the hand-off's policy authority {} is not the tip of 'main' the destination advertises",
        &member[..9]
    );
    let cases: [(&str, Claims<'_>, &str); 5] = [
        (
            "no advertisement",
            Claims {
                no_advertisement: true,
                ..Claims::default()
            },
            "the run holds no destination advertisement",
        ),
        (
            "no policy authority",
            Claims {
                policy_from: Some(None),
                ..Claims::default()
            },
            "the hand-off names no policy authority",
        ),
        (
            "an authority that is not the advertised default tip",
            Claims {
                policy_from: Some(Some(&member)),
                ..Claims::default()
            },
            &not_default,
        ),
        (
            "no --into",
            Claims {
                into: Some(None),
                ..Claims::default()
            },
            "the hand-off names no line (`--into`)",
        ),
        (
            "an --into naming another line",
            Claims {
                into: Some(Some("integration/EPC-002-other")),
                ..Claims::default()
            },
            "the hand-off names 'integration/EPC-002-other' with `--into`, but the candidate is bound to",
        ),
    ];
    for (what, claims, cause) in cases {
        let judged = hand_off_with(&fixture, CANDIDATE, &candidate, &base, &claims);
        println!("{what}: {}", judged.1);
        assert_blocks(&judged, what, &["work.landing_candidate", cause]);
        assert!(
            !judged.1.contains("landing candidate: '"),
            "{what} must not bind: {}",
            judged.1
        );
    }
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

/// AC-1 (negative), through the real hook and `codeflow ci`: a member whose
/// head is not the head of an admitted open pull request into the line is
/// refused, the candidate push and the line push alike, and neither ref
/// moves. Here the forge holds pull requests for the first two members only.
#[test]
fn a_candidate_with_an_unadmitted_member_is_refused() {
    let fixture = fixture();
    let root = fixture.root();
    let heads: Vec<String> = MEMBERS[..2]
        .iter()
        .map(|member| git_out(root, &["rev-parse", member]))
        .collect();
    let admitted: Vec<&str> = heads.iter().map(String::as_str).collect();
    admit(root, &admitted);
    let candidate = fixture.candidate(CANDIDATE);
    let planning = git_out(root, &["rev-parse", MEMBERS[2]]);
    let cause = format!("member head {} (merged by", &planning[..9]);
    let pushed = push(root, CANDIDATE);
    println!("{}", pushed.1);
    refused_by_binding(&pushed, "an unadmitted member", &cause);
    let unadmitted = format!("is not the head of an open pull request into '{LINE}'");
    assert!(pushed.1.contains(&unadmitted), "{}", pushed.1);
    assert_eq!(remote_tip(root, CANDIDATE), None);
    assert_blocks(
        &hand_off(&fixture, CANDIDATE, &candidate, &fixture.line_tip),
        "the hand-off of an unadmitted member",
        &["work.landing_candidate", &cause],
    );
    let landed = push(root, &format!("{CANDIDATE}:refs/heads/{LINE}"));
    println!("{}", landed.1);
    refused_by_binding(&landed, "the line push of an unadmitted member", &cause);
    assert_eq!(remote_tip(root, LINE), Some(fixture.line_tip.clone()));
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
        &format!("'origin/{LINE}' is missing"),
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

/// AC-5: a line update is checked against every candidate destination in
/// the same push. One push from `feat/x` to a new candidate ref and to the
/// line at the same commit lands that candidate, though the destination
/// does not advertise the candidate yet and no local branch names it; the
/// same push to a different line is refused, since the candidate is bound
/// to its own line, and that line is not created.
#[test]
fn a_line_update_is_checked_against_candidate_refs_in_the_same_push() {
    let fixture = fixture();
    let root = fixture.root();
    let candidate = fixture.candidate("feat/x");
    let other = "integration/EPC-001-other-line";
    let pushed = push_many(
        root,
        &[
            &format!("feat/x:refs/heads/{other}"),
            "feat/x:refs/heads/candidate/new",
        ],
    );
    println!("{}", pushed.1);
    refused_by_binding(
        &pushed,
        "a push to a new candidate and another line",
        &format!(
            "the push of '{other}' lands the candidate 'candidate/new', which is not a bound landing candidate of the line: its first-parent chain reaches the tip of '{LINE}', not of '{other}'"
        ),
    );
    assert_eq!(remote_tip(root, other), None);
    let landed = push_many(
        root,
        &[
            &format!("feat/x:refs/heads/{LINE}"),
            "feat/x:refs/heads/candidate/new",
        ],
    );
    println!("{}", landed.1);
    assert_passes(&landed, "a push to a new candidate and its line");
    assert!(
        landed
            .1
            .contains(&format!("the push of '{LINE}' lands the landing candidate 'candidate/new'")),
        "{}",
        landed.1
    );
    assert_eq!(remote_tip(root, LINE), Some(candidate));
}

/// AC-5: a line push whose new tip is a bound candidate is refused when it
/// is not a fast-forward of the line to that candidate: here the push
/// claims the line is at an older commit than the tip the candidate is cut
/// from. Git itself withholds a non-fast-forward update from the hook and
/// a forced one meets the force-push rule first, so the hook is driven
/// directly with the update it would be handed.
#[test]
fn a_non_fast_forward_line_push_to_a_candidate_is_refused() {
    let fixture = fixture();
    let root = fixture.root();
    let candidate = fixture.candidate(CANDIDATE);
    let older = git_out(root, &["rev-parse", &format!("{}^", fixture.line_tip)]);
    let mut child = codeflow()
        .args(["git-hook", "pre-push", "origin", ORIGIN])
        .env("PATH", forge_path(root))
        .current_dir(root)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    std::io::Write::write_all(
        &mut child.stdin.take().unwrap(),
        format!("refs/heads/{CANDIDATE} {candidate} refs/heads/{LINE} {older}\n").as_bytes(),
    )
    .unwrap();
    let out = child.wait_with_output().unwrap();
    let refused = (
        out.status.code().unwrap_or(-1),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    );
    println!("{}", refused.1);
    refused_by_binding(
        &refused,
        "a non-fast-forward line push",
        &format!(
            "the push is not a fast-forward of the line to the candidate: the candidate is cut from {}, and the destination's line is at {}",
            &fixture.line_tip[..9],
            &older[..9]
        ),
    );
    assert_eq!(remote_tip(root, LINE), Some(fixture.line_tip.clone()));
}
