//! Journey (TSK-106 AC-9, SPC-013 R-93 and R-96).
//!
//! - An adopter project, scaffolded and then updated by the binary Cargo
//!   built, receives a managed CI file with no `CodeFlow` release job, at every
//!   tier.
//! - A project that adopted `CodeFlow`'s release calculator the way this
//!   repository does (`release.backend = "codeflow"`, `scripts/release.py`,
//!   the path-set table and a release configuration) gets a warning from its
//!   real pre-push hook when a behaviour change carries no pending entry, is
//!   never blocked by it, is blocked when the push breaks the release tree,
//!   and the pull request job's `check-pr` blocks the same change.
//!
//! The hooks `init` installs resolve `codeflow` to the built binary through
//! `PATH`; `release.py` and the path-set table are this checkout's own.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The published baseline the adopted fixture records: the version of the
/// binary under test, whose `init` stamps the scaffold with it.
const BASE: &str = env!("CARGO_PKG_VERSION");

/// The patch release the journey prepares on top of the baseline.
fn next() -> String {
    let mut parts = BASE.split('.').map(|part| part.parse::<u64>().unwrap());
    let (major, minor, patch) = (
        parts.next().unwrap(),
        parts.next().unwrap(),
        parts.next().unwrap(),
    );
    format!("{major}.{minor}.{}", patch + 1)
}

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn isolated_home() -> &'static Path {
    static HOME: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    HOME.get_or_init(|| tempfile::tempdir().expect("home tempdir"))
        .path()
}

fn with_env(command: &mut Command) -> &mut Command {
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_codeflow"));
    let path = std::env::join_paths(exe.parent().map(Path::to_path_buf).into_iter().chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .expect("joinable PATH");
    command
        .env("CODEFLOW_HOME", isolated_home())
        // The PR job names the binary that reads the body, as CI does after
        // building it; pre-push passes its own (TSK-147 F4).
        .env("CODEFLOW_BIN", &exe)
        .env("PATH", path)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Journey")
        .env("GIT_AUTHOR_EMAIL", "journey@example.test")
        .env("GIT_COMMITTER_NAME", "Journey")
        .env("GIT_COMMITTER_EMAIL", "journey@example.test")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("CODEFLOW_INTEGRATE_TOKEN")
        .env_remove("CODEFLOW_HUMAN_OVERRIDE")
        .env_remove("CODEFLOW_PR_BODY")
        .env_remove("CODEFLOW_PR_DRAFT")
        .env_remove("GITHUB_BASE_REF")
        .env_remove("GITHUB_HEAD_REF")
        .env_remove("GITHUB_EVENT_NAME")
}

fn codeflow(root: &Path, args: &[&str]) -> Output {
    with_env(&mut Command::new(env!("CARGO_BIN_EXE_codeflow")))
        .args(args)
        .current_dir(root)
        .output()
        .expect("codeflow runs")
}

fn run(root: &Path, program: &str, args: &[&str]) -> Output {
    with_env(&mut Command::new(program))
        .args(args)
        .current_dir(root)
        .output()
        .expect("command runs")
}

fn git(root: &Path, args: &[&str]) -> String {
    let out = run(root, "git", args);
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn ok(out: &Output, what: &str) -> String {
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        out.status.success(),
        "{what} failed:\n{stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    stdout
}

fn write(root: &Path, relative: &str, text: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn commit(root: &Path, message: &str) -> String {
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", message]);
    git(root, &["rev-parse", "HEAD"])
}

/// Record fixture history on `main` as already landed. The installed hooks
/// refuse commits on a protected branch, so only this setup, which stands
/// for merged pull requests, runs without them; every journey step on a
/// work branch runs the real hooks.
fn landed(root: &Path, message: &str) -> String {
    git(root, &["add", "-A"]);
    git(
        root,
        &[
            "-c",
            "core.hooksPath=/dev/null",
            "commit",
            "-q",
            "-m",
            message,
        ],
    );
    git(root, &["rev-parse", "HEAD"])
}

fn python_available() -> bool {
    Command::new("python3").arg("--version").output().is_ok()
}

const RELEASE_JOBS: [&str; 5] = [
    "release-impact",
    "release-state",
    "release impact",
    "scripts/release.py",
    "check-pr",
];

/// TSK-106 AC-8 and AC-9: the managed CI file an adopter receives, on init
/// and after an update by the built binary, carries no `CodeFlow` release job,
/// and no `CodeFlow`-only release workflow is installed.
#[test]
fn an_updated_adopter_ci_file_carries_no_codeflow_release_job() {
    for tier in ["--minimal", "--standard", "--full"] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("adopter");
        std::fs::create_dir(&root).unwrap();
        ok(&codeflow(&root, &["init", "--yes", tier]), "init");
        ok(&codeflow(&root, &["update"]), "update");
        let workflows = root.join(".github/workflows");
        let ci = std::fs::read_to_string(workflows.join("codeflow-ci.yml"))
            .unwrap_or_else(|e| panic!("{tier}: managed CI file: {e}"));
        for job in RELEASE_JOBS {
            assert!(!ci.contains(job), "{tier}: managed CI carries {job}");
        }
        assert!(
            !workflows.join("codeflow-release.yml").exists(),
            "{tier}: CodeFlow's own release workflow is not scaffolded"
        );
        for entry in std::fs::read_dir(&workflows).unwrap() {
            let text = std::fs::read_to_string(entry.unwrap().path()).unwrap();
            assert!(!text.contains("scripts/release.py"), "{tier}: {text}");
        }
    }
}

/// `CodeFlow`'s own copy of the managed file keeps no release job either, so a
/// three-way update merge has none to carry, and its release jobs run from
/// the `CodeFlow`-only workflow on pull requests, main and integration lines.
#[test]
fn codeflow_release_jobs_live_outside_the_managed_ci_file() {
    let root = workspace();
    for managed in [
        ".github/workflows/codeflow-ci.yml",
        "assets/base/ci/codeflow-ci.yml",
    ] {
        let text = std::fs::read_to_string(root.join(managed)).unwrap();
        for job in RELEASE_JOBS {
            assert!(!text.contains(job), "{managed} carries {job}");
        }
    }
    let own = std::fs::read_to_string(root.join(".github/workflows/codeflow-release.yml")).unwrap();
    for required in [
        "branches: [main, \"integration/**\"]",
        "types: [opened, synchronize, reopened, edited]",
        "name: release impact",
        "python3 scripts/release.py check-pr",
        // The body is read by the codeflow built from the same checkout.
        "cargo build --release --locked -p codeflow-cli",
        "--codeflow-bin target/release/codeflow",
        "name: release state",
        "python3 scripts/release.py check-state --ref \"$GITHUB_SHA\"",
    ] {
        assert!(
            own.contains(required),
            "codeflow-release.yml lacks {required}"
        );
    }
    let manifest =
        std::fs::read_to_string(root.join("assets/base/scaffold-manifest.toml")).unwrap();
    assert!(!manifest.contains("codeflow-release.yml"));
}

/// A project that adopted the calculator the way this repository does.
struct Adopted {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl Adopted {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        std::fs::create_dir(&root).unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        ok(
            &codeflow(&root, &["init", "--yes", "--full"]),
            "init --full",
        );
        let project = std::fs::read_to_string(root.join(".codeflow/project.toml")).unwrap();
        let project = match project.find("[release]") {
            Some(at) => format!("{}[release]\nbackend = \"codeflow\"\n", &project[..at]),
            None => format!("{project}\n[release]\nbackend = \"codeflow\"\n"),
        };
        write(&root, ".codeflow/project.toml", &project);
        let checkout = workspace();
        for file in [
            "scripts/release.py",
            "crates/codeflow-core/src/workgraph/path_sets.toml",
        ] {
            write(
                &root,
                file,
                &std::fs::read_to_string(checkout.join(file)).unwrap(),
            );
        }
        write(
            &root,
            "Cargo.toml",
            &format!("[workspace]\nmembers = []\n\n[workspace.package]\nversion = \"{BASE}\"\n"),
        );
        let packages: String = ["codeflow-cli", "codeflow-core", "codeflow-present"]
            .iter()
            .fold(String::new(), |mut out, name| {
                let _ = write!(
                    out,
                    "[[package]]\nname = \"{name}\"\nversion = \"{BASE}\"\n"
                );
                out
            });
        write(&root, "Cargo.lock", &format!("version = 4\n\n{packages}"));
        write(
            &root,
            "CHANGELOG.md",
            &format!("# Changelog\n\n## [{BASE}] - 2026-01-01\n\n- public\n"),
        );
        let baseline = landed(&root, "chore: published baseline");
        git(&root, &["tag", &format!("v{BASE}")]);
        let tree = git(&root, &["rev-parse", "HEAD^{tree}"]);
        let snapshot = Command::new("python3")
            .args([
                "-c",
                &format!(
                    "import sys, hashlib; sys.path.insert(0, 'scripts'); import release; \
                     text = open('CHANGELOG.md').read(); \
                     print(hashlib.sha256(release.published_snapshot(text, '{BASE}').encode()).hexdigest())"
                ),
            ])
            .current_dir(&root)
            .output()
            .unwrap();
        let snapshot = String::from_utf8_lossy(&snapshot.stdout).trim().to_string();
        let config = serde_json::json!({
            "schema_version": 2,
            "release_unit": "codeflow",
            "main_branch": "main",
            "bootstrap": {
                "comparison": {"tag": format!("v{BASE}"), "commit": baseline, "tree": tree},
                "published": {
                    "changelog_sha256": snapshot,
                    "version": BASE,
                    "source_commit": baseline,
                    "release_target_commit": baseline,
                    "source_archive_sha256": "a".repeat(64),
                },
            },
            "legacy_pending_group": {"version": "9.0.0", "impact": "major", "id": "none"},
            "required_publication_checks": ["release state"],
            "watched_contract_paths": [".codeflow/**", "project-management/templates/**"],
        });
        write(
            &root,
            ".release/config.json",
            &format!("{}\n", serde_json::to_string_pretty(&config).unwrap()),
        );
        landed(&root, "chore: adopt the release calculator");
        let mut host = serde_json::json!({
            "schema_version": 1,
            "drafts_visible": false,
            "tags": {},
            "releases": [{
                "tag": format!("v{BASE}"), "draft": false, "prerelease": false, "target": baseline,
                "body": "", "assets": [{"name": "source.tar.gz", "digest": format!("sha256:{}", "a".repeat(64))}],
            }],
        });
        host["tags"][format!("v{BASE}").as_str()] = baseline.clone().into();
        write(
            &root,
            ".git/host.json",
            &serde_json::to_string(&host).unwrap(),
        );
        let remote = dir.path().join("remote.git");
        git(
            &root,
            &["clone", "-q", "--bare", ".", remote.to_str().unwrap()],
        );
        git(
            &root,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git(&root, &["fetch", "-q", "origin"]);
        Self { _dir: dir, root }
    }

    /// Push `branch` with its tip not checked out, so the push set's tree
    /// checks (which read the working checkout) leave it to CI and the
    /// journey sees only the git-data checks, the release preflight among
    /// them.
    fn push(&self, branch: &str, draft: Option<&Path>) -> Output {
        git(&self.root, &["switch", "-q", "main"]);
        let mut command = Command::new("git");
        // No CODEFLOW_BIN: the hook passes itself to release.py, and the
        // draft below is read only if it does.
        with_env(&mut command).env_remove("CODEFLOW_BIN");
        if let Some(draft) = draft {
            command.env("CODEFLOW_PR_DRAFT", draft);
        }
        command
            .args(["push", "-q", "origin", branch])
            .current_dir(&self.root)
            .output()
            .expect("git push runs")
    }

    fn check_pr(&self, head: &str, body: &str) -> Output {
        write(&self.root, ".git/body.md", body);
        run(
            &self.root,
            "python3",
            &[
                "-B",
                "scripts/release.py",
                "check-pr",
                "--base",
                "origin/main",
                "--head",
                head,
                "--target-ref",
                "origin/main",
                "--body-file",
                ".git/body.md",
                "--host-state",
                ".git/host.json",
            ],
        )
    }
}

fn body(impact: &str) -> String {
    format!(
        "## Summary\n\nStart the tool.\n\n## Release impact\n\n- Impact: {impact}\n- Breaking: no\n\
         - Rationale: The tool is new.\n- Migration: none\n- Unit: codeflow\n- Evidence: CHANGELOG.md entry.\n"
    )
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

/// TSK-106 AC-4 and AC-9: a behaviour change with no pending entry warns in
/// the real pre-push hook and is never blocked by it; the pull request job
/// blocks it; a push that breaks the release tree is blocked locally.
#[test]
#[allow(clippy::too_many_lines)] // One journey, in the order a change lives it.
fn a_behaviour_change_without_an_entry_warns_locally_and_blocks_in_the_pr_job() {
    if !python_available() {
        return;
    }
    let project = Adopted::new();
    let root = &project.root;

    // A work-in-progress push of a behaviour change with no entry: a
    // warning, never a block.
    git(root, &["switch", "-q", "-c", "feat/tool"]);
    write(root, "src/tool.rs", "pub fn tool() {}\n");
    let head = commit(root, "fix: start the tool");
    let pushed = project.push("feat/tool", None);
    let text = stderr(&pushed);
    assert!(
        pushed.status.success(),
        "a missing entry never blocks:\n{text}"
    );
    assert!(text.contains("release preflight (feat/tool)"), "{text}");
    assert!(
        text.contains("behaviour paths changed (src/tool.rs)"),
        "{text}"
    );
    assert!(text.contains("not checked against the host"), "{text}");

    // The pull request job's full check blocks the same change.
    let no_block = project.check_pr(&head, "## Summary\n\nStart the tool.\n");
    assert_eq!(no_block.status.code(), Some(2), "{}", stderr(&no_block));
    assert!(stderr(&no_block).contains("exactly one '## Release impact' section"));
    let floor = project.check_pr(&head, &body("none"));
    assert_eq!(floor.status.code(), Some(2));
    assert!(
        stderr(&floor).contains("below marker floor patch"),
        "{}",
        stderr(&floor)
    );
    let no_entry = project.check_pr(&head, &body("patch"));
    assert_eq!(no_entry.status.code(), Some(2));
    assert!(
        stderr(&no_entry).contains("must equal the impact"),
        "{}",
        stderr(&no_entry)
    );

    // A declared `Impact: none` intent in the local PR draft silences the
    // warning for a change that has no release impact.
    git(root, &["switch", "-q", "-c", "chore/internal", "main"]);
    write(root, "src/internal.rs", "pub fn internal() {}\n");
    commit(root, "chore: an internal helper");
    let draft = root.join(".git/draft.md");
    write(root, ".git/draft.md", &body("none"));
    let quiet = project.push("chore/internal", Some(&draft));
    let text = stderr(&quiet);
    assert!(quiet.status.success(), "{text}");
    assert!(!text.contains("behaviour paths changed"), "{text}");

    // A labelled pending entry with its stamps: no warning, and the pull
    // request job accepts it.
    git(root, &["switch", "-q", "feat/tool"]);
    let changelog = std::fs::read_to_string(root.join("CHANGELOG.md")).unwrap();
    let next = next();
    write(
        root,
        "CHANGELOG.md",
        &changelog.replace(
            &format!("## [{BASE}] - 2026-01-01"),
            &format!("## [{next}]\n\n<!-- codeflow:release-impact patch -->\n- **Start the tool.** A new helper.\n\n## [{BASE}] - 2026-01-01"),
        ),
    );
    for path in ["Cargo.toml", "Cargo.lock"] {
        let text = std::fs::read_to_string(root.join(path)).unwrap();
        write(
            root,
            path,
            &text.replace(
                &format!("version = \"{BASE}\""),
                &format!("version = \"{next}\""),
            ),
        );
    }
    let broken = commit(root, "docs(changelog): add the tool entry");
    // The scaffold stamps are still the baseline: this push breaks a release tree
    // its base kept valid, so the preflight blocks it.
    let blocked = project.push("feat/tool", None);
    let text = stderr(&blocked);
    assert!(
        !blocked.status.success(),
        "a broken release tree blocks:\n{text}"
    );
    assert!(text.contains("breaks the release tree"), "{text}");
    let broken_pr = project.check_pr(&broken, &body("patch"));
    assert_eq!(broken_pr.status.code(), Some(2));

    git(root, &["switch", "-q", "feat/tool"]);
    for path in [
        ".codeflow/project.toml",
        ".codeflow/manifest.json",
        "AGENTS.md",
        "CLAUDE.md",
    ] {
        let text = std::fs::read_to_string(root.join(path)).unwrap();
        let text = text
            .replace(
                &format!("scaffold_version = \"{BASE}\""),
                &format!("scaffold_version = \"{next}\""),
            )
            .replace(
                &format!("\"scaffold_version\": \"{BASE}\""),
                &format!("\"scaffold_version\": \"{next}\""),
            )
            .replace(
                &format!("scaffold={BASE} -->"),
                &format!("scaffold={next} -->"),
            );
        write(root, path, &text);
    }
    let fixed = commit(root, &format!("chore(release): stamp {next}"));
    let pushed = project.push("feat/tool", None);
    let text = stderr(&pushed);
    assert!(pushed.status.success(), "{text}");
    assert!(!text.contains("behaviour paths changed"), "{text}");
    assert!(text.contains("release tree valid"), "{text}");
    let accepted = project.check_pr(&fixed, &body("patch"));
    assert!(
        accepted.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&accepted.stdout),
        stderr(&accepted)
    );
}
