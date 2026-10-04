//! The target-side binary pin on every scaffolded CI platform besides the
//! GitHub workflow (TSK-095; SPC-013 R-113): `ci-generic.sh`, the GitLab job
//! and the Bitbucket pipeline install the `codeflow` release the target pins,
//! verified against its published `sha256.sum`, test a raised candidate
//! without enforcing with it, judge the change with the target's binary, and
//! fail a lowered pin.
//!
//! Each platform's shipped script runs against a local `file://` release. The
//! pinned release is a wrapper around this build of `codeflow`, so `codeflow
//! ci`, `test` and `validate` really run; a raised candidate is a stub that
//! stands for a newer binary. Every wrapper logs its version and arguments.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const GENERIC: &str = include_str!("../../../assets/base/ci/ci-generic.sh");
const GITLAB: &str = include_str!("../../../assets/base/ci/.gitlab-ci.yml");
const BITBUCKET: &str = include_str!("../../../assets/base/ci/bitbucket-pipelines.yml");
const BEGIN: &str = "# >>> codeflow pinned run";
const END: &str = "# <<< codeflow pinned run";
/// A pull request body in the shipped template's shape. GitLab passes the
/// merge request description as `CODEFLOW_PR_BODY`; the other platforms read
/// it from the same variable when a project supplies it.
const BODY: &str = "## Summary\n\nAdds a file.\n\n- a file\n\nTask: pin probe\n\n## Changes\n\n- add a file\n\n## Testing\n\nThe probe target passes.\n\n## Reviews\n\nNone: reviewed by the team.\n\n## Release impact\n\n- Impact: minor\n- Breaking: no\n- Rationale: new file.\n- Migration: none\n";

#[derive(Clone, Copy, Debug)]
enum Platform {
    Generic,
    GitLab,
    Bitbucket,
}

const PLATFORMS: [Platform; 3] = [Platform::Generic, Platform::GitLab, Platform::Bitbucket];

/// The release triple this machine's pinned install asks for.
fn triple() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        other => panic!("no codeflow release for {other:?}"),
    }
}

/// The literal block of the last `- |` item in a YAML script list, with its
/// indentation removed.
fn yaml_script(workflow: &str) -> String {
    let lines: Vec<&str> = workflow.lines().collect();
    let start = lines
        .iter()
        .rposition(|l| l.trim() == "- |")
        .expect("a `- |` script item");
    let indent = lines[start + 1].len() - lines[start + 1].trim_start().len();
    lines[start + 1..]
        .iter()
        .take_while(|l| l.trim().is_empty() || l.len() - l.trim_start().len() >= indent)
        .map(|l| l.get(indent..).unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

/// The shared pinned run inside a script, from its begin to its end marker.
fn pinned_run(script: &str) -> &str {
    let start = script.find(BEGIN).expect("begin marker");
    let end = script.find(END).expect("end marker") + END.len();
    &script[start..end]
}

fn script(platform: Platform) -> String {
    match platform {
        Platform::Generic => GENERIC.to_string(),
        Platform::GitLab => yaml_script(GITLAB),
        Platform::Bitbucket => yaml_script(BITBUCKET),
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("CODEFLOW_INTEGRATE_TOKEN", "test")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn sha256(path: &Path) -> String {
    let out = Command::new("shasum")
        .args(["-a", "256"])
        .arg(path)
        .output()
        .expect("shasum runs");
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()
        .unwrap()
        .to_string()
}

/// What a published release's binary does.
enum Binary {
    /// This build of codeflow, reporting `version`.
    Real,
    /// A newer binary the fixture cannot build: it logs and succeeds.
    Stub,
}

struct Fixture {
    dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
        }
    }
    fn releases(&self) -> PathBuf {
        self.dir.path().join("releases")
    }
    fn log(&self) -> PathBuf {
        self.dir.path().join("calls.log")
    }
    fn repo(&self) -> PathBuf {
        self.dir.path().join("repo")
    }
    fn home(&self) -> PathBuf {
        self.dir.path().join("home")
    }

    /// Publish `version` locally: the archive holding a logging wrapper and
    /// `sha256.sum` listing it. Returns the release directory.
    fn publish(&self, version: &str, binary: &Binary) -> PathBuf {
        let asset = format!("codeflow-cli-{}", triple());
        let dir = self.releases().join(format!("v{version}"));
        let stage = self
            .releases()
            .join(format!("stage-{version}"))
            .join(&asset);
        std::fs::create_dir_all(&stage).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        let run = match binary {
            Binary::Real => format!("exec '{}' \"$@\"", env!("CARGO_BIN_EXE_codeflow")),
            Binary::Stub => "exit 0".to_string(),
        };
        let bin = stage.join("codeflow");
        std::fs::write(
            &bin,
            format!(
                "#!/bin/sh\necho \"{version} $*\" >> '{}'\nif [ \"$1\" = --version ]; then echo \"codeflow {version}\"; exit 0; fi\n{run}\n",
                self.log().display()
            ),
        )
        .unwrap();
        Command::new("chmod").arg("755").arg(&bin).status().unwrap();
        let archive = dir.join(format!("{asset}.tar.xz"));
        let status = Command::new("tar")
            .arg("-cJf")
            .arg(&archive)
            .arg("-C")
            .arg(stage.parent().unwrap())
            .arg(&asset)
            .status()
            .unwrap();
        assert!(status.success());
        std::fs::write(
            dir.join("sha256.sum"),
            format!(
                "{}  {asset}.tar.xz\n0000  codeflow-cli-installer.sh\n",
                sha256(&archive)
            ),
        )
        .unwrap();
        dir
    }

    /// A scaffolded project whose `main` pins `version`, with one test
    /// target, and a `feat/x` branch checked out. Returns the target commit.
    fn project(&self, version: &str) -> String {
        let repo = self.repo();
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.email", "t@example.com"]);
        git(&repo, &["config", "user.name", "t"]);
        let init = self
            .codeflow(&repo)
            .args(["init", "--minimal", "--yes"])
            .output()
            .unwrap();
        assert!(init.status.success(), "{}", text(&init));
        // The fixture judges the CI scripts, not the local hooks.
        git(&repo, &["config", "core.hooksPath", "/dev/null"]);
        set_pin(&repo, version);
        std::fs::write(
            repo.join(".codeflow/test-config.json"),
            r#"{"schema_version":"1.0","targets":[{"name":"probe","enabled":true,"runner":"custom","modes":{"quick":{"command":"true"},"full":{"command":"true"}}}]}"#,
        )
        .unwrap();
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-q", "-m", "chore: scaffold"]);
        let target = git(&repo, &["rev-parse", "HEAD"]);
        git(&repo, &["checkout", "-q", "-b", "feat/x"]);
        target
    }

    /// Commit the working tree on `feat/x` and return the head.
    fn commit(&self, message: &str) -> String {
        let repo = self.repo();
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-q", "-m", message]);
        git(&repo, &["rev-parse", "HEAD"])
    }

    fn codeflow(&self, cwd: &Path) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_codeflow"));
        command
            .current_dir(cwd)
            .env("CODEFLOW_HOME", self.home())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null");
        command
    }

    /// `platform`'s shipped script, run from the fixture repository with a
    /// clean environment and the local release.
    fn script_command(&self, platform: Platform) -> Command {
        // Scratch space inside the fixture, so the scripts' temporary
        // directories go away with it.
        let scratch = self.dir.path().join("tmp");
        std::fs::create_dir_all(&scratch).unwrap();
        let mut command = Command::new("sh");
        command
            .current_dir(self.repo())
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap())
            .env("HOME", self.home())
            .env("TMPDIR", &scratch)
            .env("CODEFLOW_HOME", self.home())
            .env("CODEFLOW_PR_BODY", BODY)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env(
                "CODEFLOW_RELEASE_URL",
                format!("file://{}", self.releases().display()),
            );
        command.arg("-c").arg(script(platform));
        if let Platform::Generic = platform {
            command.arg("ci-generic.sh");
        }
        command
    }

    /// Run a prepared script command, logging the calls afresh.
    fn output(&self, command: &mut Command) -> Output {
        let _ = std::fs::remove_file(self.log());
        command.output().expect("sh runs")
    }

    /// Run `platform`'s shipped script for the change `target..head`, as that
    /// platform presents it. GitLab gets a merged results pipeline, which
    /// names the target branch's commit. `None` for the target leaves it
    /// unset.
    fn run(&self, platform: Platform, target: Option<&str>, head: &str) -> Output {
        let mut command = self.script_command(platform);
        match platform {
            Platform::Generic => {
                if let Some(target) = target {
                    command.arg(target).arg(head);
                }
            }
            Platform::GitLab => {
                command
                    .env("CI_PIPELINE_SOURCE", "merge_request_event")
                    .env("CI_COMMIT_SHA", head)
                    .env("CI_MERGE_REQUEST_SOURCE_BRANCH_NAME", "feat/x");
                if let Some(target) = target {
                    command
                        .env("CI_MERGE_REQUEST_EVENT_TYPE", "merged_result")
                        .env("CI_MERGE_REQUEST_TARGET_BRANCH_SHA", target)
                        .env("CI_MERGE_REQUEST_DIFF_BASE_SHA", target);
                }
            }
            Platform::Bitbucket => {
                command
                    .env("BITBUCKET_COMMIT", head)
                    .env("BITBUCKET_BRANCH", "feat/x");
                if let Some(target) = target {
                    command.env("BITBUCKET_PR_DESTINATION_COMMIT", target);
                }
            }
        }
        self.output(&mut command)
    }

    /// The logged calls, one `<version> <args>` per line.
    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.log())
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn set_pin(repo: &Path, version: &str) {
    let path = repo.join(".codeflow/project.toml");
    let state = std::fs::read_to_string(&path).unwrap();
    let pinned: Vec<String> = state
        .lines()
        .map(|line| {
            if line.starts_with("scaffold_version") {
                format!("scaffold_version = \"{version}\"")
            } else {
                line.to_string()
            }
        })
        .collect();
    std::fs::write(&path, pinned.join("\n") + "\n").unwrap();
}

fn set_commit_format(repo: &Path, level: &str) {
    let path = repo.join(".codeflow/policy.json");
    let mut value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    value["git"]["commit_format"] = serde_json::json!(level);
    std::fs::write(&path, serde_json::to_string_pretty(&value).unwrap()).unwrap();
}

/// A change branched from `base`, after which the target advanced to
/// `target`: the source never saw the target's newer pin or policy.
struct Diverged {
    base: String,
    target: String,
    head: String,
}

/// `main` pins 1.2.3 with commit format off at the fork point, then raises
/// the pin to 1.2.4 and blocks bad commit subjects; `feat/x` branches at the
/// fork point and commits `message`. Both releases are this build.
fn diverged(fx: &Fixture, message: &str) -> Diverged {
    let repo = fx.repo();
    fx.project("1.2.3");
    git(&repo, &["checkout", "-q", "main"]);
    set_commit_format(&repo, "off");
    let base = fx.commit("chore: relax the commit format");
    set_pin(&repo, "1.2.4");
    set_commit_format(&repo, "block");
    let target = fx.commit("chore: raise the pin and block bad subjects");
    git(&repo, &["checkout", "-q", "-B", "feat/x", &base]);
    std::fs::write(repo.join("a.txt"), "a\n").unwrap();
    let head = fx.commit(message);
    fx.publish("1.2.3", &Binary::Real);
    fx.publish("1.2.4", &Binary::Real);
    Diverged { base, target, head }
}

/// A bare copy of the fixture repository at `path` whose `main` is `main`.
fn bare_copy(fx: &Fixture, path: &Path, main: &str) {
    git(
        fx.dir.path(),
        &[
            "clone",
            "-q",
            "--bare",
            &fx.repo().display().to_string(),
            &path.display().to_string(),
        ],
    );
    git(path, &["update-ref", "refs/heads/main", main]);
}

/// The ways each platform presents a merge request whose target advanced
/// after the source branched, with the diff base still at the fork point.
fn diverged_runs(fx: &Fixture, d: &Diverged) -> Vec<(String, Output, Vec<String>)> {
    let repo = fx.repo();
    let gitlab = |extra: &[(&str, &str)]| {
        let mut command = fx.script_command(Platform::GitLab);
        command
            .env("CI_PIPELINE_SOURCE", "merge_request_event")
            .env("CI_MERGE_REQUEST_SOURCE_BRANCH_NAME", "feat/x")
            .env("CI_MERGE_REQUEST_TARGET_BRANCH_NAME", "main")
            .env("CI_MERGE_REQUEST_DIFF_BASE_SHA", &d.base)
            .env("CI_PROJECT_ID", "7")
            .env("CI_MERGE_REQUEST_PROJECT_ID", "7")
            .env("CI_COMMIT_SHA", &d.head);
        for (key, value) in extra {
            command.env(key, value);
        }
        command
    };
    let mut runs = Vec::new();
    let mut record = |label: &str, out: Output| runs.push((label.to_string(), out, fx.calls()));
    record(
        "generic",
        fx.output(
            fx.script_command(Platform::Generic)
                .arg(&d.target)
                .arg(&d.head),
        ),
    );
    let mut bitbucket = fx.script_command(Platform::Bitbucket);
    bitbucket
        .env("BITBUCKET_COMMIT", &d.head)
        .env("BITBUCKET_BRANCH", "feat/x")
        .env("BITBUCKET_PR_DESTINATION_COMMIT", &d.target);
    record("bitbucket", fx.output(&mut bitbucket));

    // An ordinary (detached) merge request pipeline: GitLab names no target
    // commit and fetches only the pipeline ref, so the job fetches the
    // target branch from the project's own remote.
    let origin = fx.dir.path().join("origin.git");
    bare_copy(fx, &origin, &d.target);
    git(
        &repo,
        &["remote", "add", "origin", &origin.display().to_string()],
    );
    record(
        "gitlab detached",
        fx.output(&mut gitlab(&[("CI_MERGE_REQUEST_EVENT_TYPE", "detached")])),
    );

    // A Bitbucket pull request step without the destination commit
    // variable, which Atlassian's variable table does not list: the step
    // fetches the destination branch from `origin` (TSK-183).
    let mut bitbucket = fx.script_command(Platform::Bitbucket);
    bitbucket
        .env("BITBUCKET_COMMIT", &d.head)
        .env("BITBUCKET_BRANCH", "feat/x")
        .env("BITBUCKET_PR_DESTINATION_BRANCH", "main");
    record("bitbucket by branch", fx.output(&mut bitbucket));

    // A fork's pipeline: `origin` is the fork, whose `main` is stale; the
    // target branch comes from the merge request's project.
    let fork = fx.dir.path().join("fork.git");
    bare_copy(fx, &fork, &d.base);
    git(
        &repo,
        &["remote", "set-url", "origin", &fork.display().to_string()],
    );
    let parent = fx.dir.path().join("parent");
    bare_copy(fx, &fx.dir.path().join("parent.git"), &d.target);
    record(
        "gitlab fork",
        fx.output(&mut gitlab(&[
            ("CI_MERGE_REQUEST_EVENT_TYPE", "detached"),
            ("CI_PROJECT_ID", "8"),
            (
                "CI_MERGE_REQUEST_PROJECT_URL",
                &parent.display().to_string(),
            ),
        ])),
    );
    git(
        &repo,
        &["remote", "set-url", "origin", &origin.display().to_string()],
    );

    // A merged results pipeline builds the merge of the source into the
    // target and names the target commit it merged with.
    git(&repo, &["checkout", "-q", "--detach", &d.target]);
    git(
        &repo,
        &[
            "merge",
            "-q",
            "--no-ff",
            "-m",
            "Merge branch 'feat/x' into 'main'",
            &d.head,
        ],
    );
    let merged = git(&repo, &["rev-parse", "HEAD"]);
    record(
        "gitlab merged results",
        fx.output(&mut gitlab(&[
            ("CI_MERGE_REQUEST_EVENT_TYPE", "merged_result"),
            ("CI_MERGE_REQUEST_TARGET_BRANCH_SHA", &d.target),
            ("CI_MERGE_REQUEST_SOURCE_BRANCH_SHA", &d.head),
            ("CI_COMMIT_SHA", &merged),
        ])),
    );
    git(&repo, &["checkout", "-q", "feat/x"]);
    runs
}

/// Whether a call by `version` ran `args`.
fn ran(calls: &[String], version: &str, args: &str) -> bool {
    calls
        .iter()
        .any(|c| c.starts_with(&format!("{version} {args}")))
}

#[test]
fn every_platform_runs_the_same_pinned_script() {
    let generic = pinned_run(GENERIC);
    for platform in [Platform::GitLab, Platform::Bitbucket] {
        assert_eq!(pinned_run(&script(platform)), generic, "{platform:?}");
    }
    assert!(generic.contains("sha256.sum"));
}

#[test]
fn no_shipped_ci_file_installs_or_suggests_the_latest_release() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/base/ci");
    let mut checked = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            !text.contains("releases/latest"),
            "{} names releases/latest",
            path.display()
        );
        assert!(!text.contains("PLACEHOLDER"), "{}", path.display());
        checked += 1;
    }
    assert!(checked >= 7, "read every shipped CI file");
}

#[test]
fn the_pinned_install_verifies_the_release_and_fails_closed() {
    for platform in PLATFORMS {
        let fx = Fixture::new();
        let target = fx.project("1.2.3");
        let release = fx.publish("1.2.3", &Binary::Real);
        let asset = release.join(format!("codeflow-cli-{}.tar.xz", triple()));

        let ok = fx.run(platform, Some(&target), &target);
        assert!(ok.status.success(), "{platform:?}: {}", text(&ok));
        assert!(text(&ok).contains("codeflow 1.2.3 installed and verified"));

        // A tampered asset installs nothing and runs nothing.
        let original = std::fs::read(&asset).unwrap();
        let mut tampered = original.clone();
        tampered.extend_from_slice(b"tampered");
        std::fs::write(&asset, &tampered).unwrap();
        let bad = fx.run(platform, Some(&target), &target);
        assert!(!bad.status.success(), "{platform:?}");
        assert!(text(&bad).contains("checksum mismatch"), "{}", text(&bad));
        assert!(fx.calls().is_empty(), "{platform:?}: {:?}", fx.calls());
        std::fs::write(&asset, &original).unwrap();

        // A missing checksum file, or one without this asset, fails closed.
        let sums = release.join("sha256.sum");
        let listed = std::fs::read_to_string(&sums).unwrap();
        std::fs::remove_file(&sums).unwrap();
        let missing = fx.run(platform, Some(&target), &target);
        assert!(!missing.status.success());
        assert!(
            text(&missing).contains("no published checksum file"),
            "{}",
            text(&missing)
        );
        std::fs::write(&sums, "0000  codeflow-cli-installer.sh\n").unwrap();
        let unlisted = fx.run(platform, Some(&target), &target);
        assert!(!unlisted.status.success());
        assert!(
            text(&unlisted).contains("lists no checksum"),
            "{}",
            text(&unlisted)
        );
        std::fs::write(&sums, listed).unwrap();
        assert!(fx.calls().is_empty(), "{platform:?}: {:?}", fx.calls());

        // A version with no release fails naming it.
        set_pin(&fx.repo(), "7.7.7");
        let unpublished_target = fx.commit("chore: pin an unpublished release");
        let unpublished = fx.run(platform, Some(&unpublished_target), &unpublished_target);
        assert!(!unpublished.status.success());
        assert!(
            text(&unpublished).contains("codeflow 7.7.7 has no published checksum file"),
            "{}",
            text(&unpublished)
        );

        // No pin at all fails naming the file.
        let state = std::fs::read_to_string(fx.repo().join(".codeflow/project.toml")).unwrap();
        let unpinned = state
            .lines()
            .filter(|l| !l.starts_with("scaffold_version"))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        std::fs::write(fx.repo().join(".codeflow/project.toml"), unpinned).unwrap();
        let bare = fx.commit("chore: drop the pin");
        let none = fx.run(platform, Some(&bare), &bare);
        assert!(!none.status.success());
        assert!(
            text(&none).contains("no scaffold_version pinned"),
            "{}",
            text(&none)
        );
    }
}

/// Append a `[scaffold_sha256]` table naming `table_version` and, when
/// given, this machine's archive digest to the fixture's state.
fn pin_digest(repo: &Path, table_version: &str, digest: Option<&str>) {
    let path = repo.join(".codeflow/project.toml");
    let state = std::fs::read_to_string(&path).unwrap();
    let state = state
        .split("\n[scaffold_sha256]\n")
        .next()
        .unwrap()
        .trim_end()
        .to_string();
    let entry = digest.map_or_else(String::new, |d| format!("{} = \"{d}\"\n", triple()));
    std::fs::write(
        &path,
        format!("{state}\n\n[scaffold_sha256]\nversion = \"{table_version}\"\n{entry}"),
    )
    .unwrap();
}

/// sathyassn/codeflow#47: the shared pinned run takes the archive digest the
/// target pins beside the version, refuses a release whose archive and
/// `sha256.sum` were both replaced, fails closed on a stale or partial
/// table, and warns when no digest is pinned.
#[test]
fn the_shared_run_requires_the_pinned_release_digest() {
    for platform in PLATFORMS {
        let fx = Fixture::new();
        let unpinned = fx.project("1.2.3");
        let release = fx.publish("1.2.3", &Binary::Real);
        let asset = release.join(format!("codeflow-cli-{}.tar.xz", triple()));
        let reviewed = sha256(&asset);

        let warned = fx.run(platform, Some(&unpinned), &unpinned);
        assert!(warned.status.success(), "{platform:?}: {}", text(&warned));
        assert!(
            text(&warned).contains("codeflow: warning: no release digest is pinned"),
            "{platform:?}: {}",
            text(&warned)
        );
        assert!(text(&warned).contains("installed and verified against sha256.sum"));

        pin_digest(&fx.repo(), "1.2.3", Some(&reviewed));
        let target = fx.commit("chore: pin the release digest");
        let ok = fx.run(platform, Some(&target), &target);
        assert!(ok.status.success(), "{platform:?}: {}", text(&ok));
        assert!(
            text(&ok).contains(
                "codeflow 1.2.3 installed and verified against the digest pinned in .codeflow/project.toml and sha256.sum"
            ),
            "{platform:?}: {}",
            text(&ok)
        );
        assert!(
            !text(&ok).contains("no release digest is pinned"),
            "{platform:?}: {}",
            text(&ok)
        );

        // Both release files replaced: the pinned digest refuses it.
        let original = std::fs::read(&asset).unwrap();
        let listed = std::fs::read_to_string(release.join("sha256.sum")).unwrap();
        let mut replaced = original.clone();
        replaced.extend_from_slice(b"replaced");
        std::fs::write(&asset, &replaced).unwrap();
        std::fs::write(
            release.join("sha256.sum"),
            format!("{}  codeflow-cli-{}.tar.xz\n", sha256(&asset), triple()),
        )
        .unwrap();
        let swapped = fx.run(platform, Some(&target), &target);
        assert!(!swapped.status.success(), "{platform:?}");
        assert!(
            text(&swapped).contains("does not match the digest pinned in .codeflow/project.toml"),
            "{platform:?}: {}",
            text(&swapped)
        );
        assert!(fx.calls().is_empty(), "{platform:?}: {:?}", fx.calls());
        std::fs::write(&asset, &original).unwrap();
        std::fs::write(release.join("sha256.sum"), listed).unwrap();

        // A table from another version, or without this triple, fails closed.
        pin_digest(&fx.repo(), "1.2.2", Some(&reviewed));
        let stale_target = fx.commit("chore: stale digest");
        let stale = fx.run(platform, Some(&stale_target), &stale_target);
        assert!(!stale.status.success(), "{platform:?}");
        assert!(
            text(&stale).contains("pins the digests of codeflow 1.2.2, not 1.2.3"),
            "{platform:?}: {}",
            text(&stale)
        );
        pin_digest(&fx.repo(), "1.2.3", None);
        let partial_target = fx.commit("chore: no digest for this triple");
        let partial = fx.run(platform, Some(&partial_target), &partial_target);
        assert!(!partial.status.success(), "{platform:?}");
        assert!(
            text(&partial).contains(&format!("lists no digest for {}", triple())),
            "{platform:?}: {}",
            text(&partial)
        );
        assert!(fx.calls().is_empty(), "{platform:?}: {:?}", fx.calls());
    }
}

/// A table written another way, or declared or keyed twice, fails the shared
/// run closed even against a replaced archive and `sha256.sum`; it never
/// reads as absent.
#[test]
fn the_shared_run_refuses_a_digest_table_it_does_not_read() {
    for platform in PLATFORMS {
        let fx = Fixture::new();
        fx.project("1.2.3");
        let release = fx.publish("1.2.3", &Binary::Real);
        let asset = release.join(format!("codeflow-cli-{}.tar.xz", triple()));
        let reviewed = sha256(&asset);
        let mut replaced = std::fs::read(&asset).unwrap();
        replaced.extend_from_slice(b"replaced");
        std::fs::write(&asset, &replaced).unwrap();
        std::fs::write(
            release.join("sha256.sum"),
            format!("{}  codeflow-cli-{}.tar.xz\n", sha256(&asset), triple()),
        )
        .unwrap();
        let path = fx.repo().join(".codeflow/project.toml");
        let base = std::fs::read_to_string(&path).unwrap();
        let base = base
            .split("\n[scaffold_sha256]\n")
            .next()
            .unwrap()
            .trim_end()
            .to_string();
        let t = triple();
        let table =
            format!("{base}\n\n[scaffold_sha256]\nversion = \"1.2.3\"\n{t} = \"{reviewed}\"\n");
        let form = "is written in a form the CI installers do not read";
        for (state, reason) in [
            (format!("{base}\nscaffold_sha256 = {{ version = \"1.2.3\", {t} = \"{reviewed}\" }}\n"), form.to_string()),
            (format!("{base}\n\n[\"scaffold_sha256\"]\nversion = \"1.2.3\"\n{t} = \"{reviewed}\"\n"), form.to_string()),
            (format!("{base}\nscaffold_sha256.version = \"1.2.3\"\nscaffold_sha256.{t} = \"{reviewed}\"\n"), form.to_string()),
            (format!("{table}[other]\n[scaffold_sha256]\n"), "is declared twice".to_string()),
            (format!("{table}{t} = \"{reviewed}\"\n"), format!("lists {t} twice")),
            (
                format!("{base}\n\n[\"\\u0073caffold_sha256\"]\nversion = \"1.2.3\"\n{t} = \"{reviewed}\"\n"),
                "may be hidden behind an escaped key".to_string(),
            ),
        ] {
            std::fs::write(&path, &state).unwrap();
            let unread = fx.commit("chore: another table form");
            let out = fx.run(platform, Some(&unread), &unread);
            assert!(!out.status.success(), "{platform:?}: {state}");
            assert!(
                text(&out).contains(&format!("the [scaffold_sha256] table in .codeflow/project.toml at {unread} {reason}")),
                "{platform:?}: {state}: {}",
                text(&out)
            );
            assert!(fx.calls().is_empty(), "{platform:?}: {:?}", fx.calls());
        }
    }
}

/// sathyassn/codeflow#46: the shared pinned run sources a project-owned
/// `.codeflow/ci-setup.sh` after the install and before `codeflow test`, so
/// its exports reach the test gate; a failing hook stops the run before the
/// gate.
#[test]
fn the_shared_run_sources_the_project_setup_hook_before_the_test_gate() {
    for platform in PLATFORMS {
        let fx = Fixture::new();
        fx.project("1.2.3");
        fx.publish("1.2.3", &Binary::Real);
        // The test target passes only when the hook's export reaches it.
        std::fs::write(
            fx.repo().join(".codeflow/test-config.json"),
            r#"{"schema_version":"1.0","targets":[{"name":"probe","enabled":true,"runner":"custom","modes":{"quick":{"command":"test \"$HOOK_VALUE\" = set"},"full":{"command":"test \"$HOOK_VALUE\" = set"}}}]}"#,
        )
        .unwrap();
        std::fs::write(
            fx.repo().join(".codeflow/ci-setup.sh"),
            format!(
                "echo hook >> '{}'\nexport HOOK_VALUE=set\ncd /\n",
                fx.log().display()
            ),
        )
        .unwrap();
        let head = fx.commit("ci: add the project setup hook");
        let ok = fx.run(platform, Some(&head), &head);
        assert!(ok.status.success(), "{platform:?}: {}", text(&ok));
        let calls = fx.calls();
        let hook = calls
            .iter()
            .position(|c| c == "hook")
            .expect("the hook ran");
        let test = calls
            .iter()
            .position(|c| c.starts_with("1.2.3 test --strict"))
            .expect("the gate ran");
        let ci = calls
            .iter()
            .position(|c| c.starts_with("1.2.3 ci "))
            .expect("the range was judged");
        assert!(ci < hook && hook < test, "{platform:?}: {calls:?}");

        std::fs::write(
            fx.repo().join(".codeflow/ci-setup.sh"),
            format!("echo hook >> '{}'\nfalse\n", fx.log().display()),
        )
        .unwrap();
        let failing = fx.commit("ci: break the project setup hook");
        let failed = fx.run(platform, Some(&failing), &failing);
        assert!(!failed.status.success(), "{platform:?}: {}", text(&failed));
        assert!(
            !fx.calls().iter().any(|c| c.starts_with("1.2.3 test")),
            "{platform:?}: {:?}",
            fx.calls()
        );
    }
}

#[test]
fn a_run_without_the_target_commit_is_refused() {
    for platform in PLATFORMS {
        let fx = Fixture::new();
        let target = fx.project("1.2.3");
        fx.publish("1.2.3", &Binary::Real);
        let out = fx.run(platform, None, &target);
        assert!(!out.status.success(), "{platform:?}");
        assert!(
            text(&out).contains("no target commit"),
            "{platform:?}: {}",
            text(&out)
        );
        assert!(fx.calls().is_empty(), "{platform:?}");
    }
}

/// Equal, raised and lowered pins, and an update whose raised pin has not
/// landed: the target's binary always judges.
#[test]
fn the_target_pin_judges_every_change_to_the_pin() {
    for platform in PLATFORMS {
        let fx = Fixture::new();
        let target = fx.project("1.2.3");
        fx.publish("1.2.3", &Binary::Real);
        fx.publish("1.2.4", &Binary::Stub);
        fx.publish("1.0.0", &Binary::Stub);

        // Equal pins: the target's binary runs every gate, and nothing else.
        std::fs::write(fx.repo().join("a.txt"), "a\n").unwrap();
        let head = fx.commit("feat: add a file");
        let equal = fx.run(platform, Some(&target), &head);
        assert!(equal.status.success(), "{platform:?}: {}", text(&equal));
        let calls = fx.calls();
        for gate in ["ci --base", "test --strict", "validate --docs"] {
            assert!(ran(&calls, "1.2.3", gate), "{platform:?} {gate}: {calls:?}");
        }
        assert!(calls.iter().all(|c| c.starts_with("1.2.3 ")), "{calls:?}");

        // A raised pin and nothing else: the target's binary judges, and the
        // candidate is only tested.
        set_pin(&fx.repo(), "1.2.4");
        let raised = fx.commit("chore: raise the codeflow pin");
        let out = fx.run(platform, Some(&target), &raised);
        assert!(out.status.success(), "{platform:?}: {}", text(&out));
        let calls = fx.calls();
        assert!(ran(&calls, "1.2.4", "--version"), "{calls:?}");
        assert!(ran(&calls, "1.2.4", "validate --docs"), "{calls:?}");
        assert!(!ran(&calls, "1.2.4", "ci"), "{calls:?}");
        assert!(!ran(&calls, "1.2.4", "test"), "{calls:?}");
        for gate in ["ci --base", "test --strict", "validate --docs"] {
            assert!(ran(&calls, "1.2.3", gate), "{platform:?} {gate}: {calls:?}");
        }

        // The update carried before the raised pin landed: the target's
        // binary cannot read the new key and names the two-step order.
        let policy_path = fx.repo().join(".codeflow/policy.json");
        let policy = std::fs::read_to_string(&policy_path).unwrap();
        let mut value: serde_json::Value = serde_json::from_str(&policy).unwrap();
        value["git"]["future_key"] = serde_json::json!("block");
        std::fs::write(&policy_path, serde_json::to_string_pretty(&value).unwrap()).unwrap();
        let update = fx.commit("chore: run codeflow update");
        let early = fx.run(platform, Some(&target), &update);
        assert!(!early.status.success(), "{platform:?}");
        assert!(
            text(&early).contains("Upgrades take two pull requests, in order"),
            "{platform:?}: {}",
            text(&early)
        );
        assert!(ran(&fx.calls(), "1.2.3", "ci --base"), "{:?}", fx.calls());

        // A lowered pin: the target's binary still judges, then the run fails.
        std::fs::write(&policy_path, policy).unwrap();
        set_pin(&fx.repo(), "1.0.0");
        let lowered = fx.commit("chore: lower the codeflow pin");
        let out = fx.run(platform, Some(&target), &lowered);
        assert!(!out.status.success(), "{platform:?}");
        assert!(
            text(&out).contains("lowers scaffold_version from 1.2.3 to 1.0.0"),
            "{platform:?}: {}",
            text(&out)
        );
        let calls = fx.calls();
        assert!(ran(&calls, "1.2.3", "ci --base"), "{calls:?}");
        assert!(calls.iter().all(|c| !c.starts_with("1.0.0 ")), "{calls:?}");

        // A setup hook in the head cannot clear the refusal: the pin is
        // checked before the project's own code runs.
        std::fs::write(
            fx.repo().join(".codeflow/ci-setup.sh"),
            format!(
                "echo hook >> '{}'\nlowered=\ncodeflow_fail() {{ :; }}\n",
                fx.log().display()
            ),
        )
        .unwrap();
        let cleared = fx.commit("ci: a hook that clears the refusal");
        let out = fx.run(platform, Some(&target), &cleared);
        assert!(!out.status.success(), "{platform:?}: {}", text(&out));
        assert!(
            text(&out).contains("lowers scaffold_version from 1.2.3 to 1.0.0"),
            "{platform:?}: {}",
            text(&out)
        );
        let calls = fx.calls();
        assert!(
            !calls.iter().any(|c| c == "hook"),
            "{platform:?}: {calls:?}"
        );
        assert!(!ran(&calls, "1.2.3", "test"), "{platform:?}: {calls:?}");
    }
}

/// The range is judged by the target's policy, not a relaxed one the head
/// carries.
#[test]
fn the_head_cannot_relax_the_policy_that_judges_it() {
    for platform in PLATFORMS {
        let fx = Fixture::new();
        let target = fx.project("1.2.3");
        fx.publish("1.2.3", &Binary::Real);
        let policy_path = fx.repo().join(".codeflow/policy.json");
        let policy = std::fs::read_to_string(&policy_path).unwrap();
        let mut value: serde_json::Value = serde_json::from_str(&policy).unwrap();
        value["git"]["commit_format"] = serde_json::json!("off");
        std::fs::write(&policy_path, serde_json::to_string_pretty(&value).unwrap()).unwrap();
        let head = fx.commit("Relax every check");
        let out = fx.run(platform, Some(&target), &head);
        assert!(!out.status.success(), "{platform:?}: {}", text(&out));
        assert!(
            text(&out).contains("git.commit_format"),
            "{platform:?}: {}",
            text(&out)
        );
    }
}

/// The target advanced after the source branched: its newer pin and policy
/// judge the change on every platform, never the fork point's (review
/// C095-1).
#[test]
fn the_current_target_judges_a_branch_that_predates_it() {
    let fx = Fixture::new();
    let d = diverged(&fx, "invalid message");
    for (label, out, calls) in diverged_runs(&fx, &d) {
        assert!(!out.status.success(), "{label}: {}", text(&out));
        assert!(
            text(&out).contains("git.commit_format"),
            "{label}: {}",
            text(&out)
        );
        assert!(
            ran(&calls, "1.2.4", &format!("ci --base {}", d.target)),
            "{label}: {calls:?}"
        );
        assert!(
            calls.iter().all(|c| c.starts_with("1.2.4 ")),
            "{label}: {calls:?}"
        );
    }
}

/// GitLab fails closed when the merge request's target branch cannot be
/// fetched, and never falls back to the diff base.
#[test]
fn gitlab_refuses_a_target_it_cannot_fetch() {
    let fx = Fixture::new();
    let d = diverged(&fx, "invalid message");
    let origin = fx.dir.path().join("origin.git");
    bare_copy(&fx, &origin, &d.target);
    git(
        &fx.repo(),
        &["remote", "add", "origin", &origin.display().to_string()],
    );
    let mut command = fx.script_command(Platform::GitLab);
    command
        .env("CI_PIPELINE_SOURCE", "merge_request_event")
        .env("CI_MERGE_REQUEST_EVENT_TYPE", "detached")
        .env("CI_MERGE_REQUEST_SOURCE_BRANCH_NAME", "feat/x")
        .env("CI_MERGE_REQUEST_TARGET_BRANCH_NAME", "gone")
        .env("CI_MERGE_REQUEST_DIFF_BASE_SHA", &d.base)
        .env("CI_PROJECT_ID", "7")
        .env("CI_MERGE_REQUEST_PROJECT_ID", "7")
        .env("CI_COMMIT_SHA", &d.head);
    let out = fx.output(&mut command);
    assert!(!out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains("cannot fetch the merge request's target branch gone"),
        "{}",
        text(&out)
    );
    assert!(fx.calls().is_empty(), "{:?}", fx.calls());
}

/// Bitbucket without the destination commit fails closed when the
/// destination branch cannot be fetched, and never falls back to the head
/// or a merge base (TSK-183 AC-2).
#[test]
fn bitbucket_refuses_a_destination_it_cannot_fetch() {
    let fx = Fixture::new();
    let d = diverged(&fx, "invalid message");
    let origin = fx.dir.path().join("origin.git");
    bare_copy(&fx, &origin, &d.target);
    git(
        &fx.repo(),
        &["remote", "add", "origin", &origin.display().to_string()],
    );
    let mut command = fx.script_command(Platform::Bitbucket);
    command
        .env("BITBUCKET_COMMIT", &d.head)
        .env("BITBUCKET_BRANCH", "feat/x")
        .env("BITBUCKET_PR_DESTINATION_BRANCH", "gone");
    let out = fx.output(&mut command);
    assert!(!out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains("cannot fetch the pull request's destination branch gone"),
        "{}",
        text(&out)
    );
    assert!(fx.calls().is_empty(), "{:?}", fx.calls());
}

/// Bitbucket's destination commit, full or abbreviated to 12 characters,
/// is the target, and the destination branch is then not fetched
/// (TSK-183 AC-3).
#[test]
fn bitbucket_takes_the_destination_commit_when_it_is_set() {
    let fx = Fixture::new();
    let target = fx.project("1.2.3");
    fx.publish("1.2.3", &Binary::Real);
    std::fs::write(fx.repo().join("a.txt"), "a\n").unwrap();
    let head = fx.commit("feat: add a file");
    for named in [target.as_str(), &target[..12]] {
        let mut command = fx.script_command(Platform::Bitbucket);
        command
            .env("BITBUCKET_COMMIT", &head)
            .env("BITBUCKET_BRANCH", "feat/x")
            .env("BITBUCKET_PR_DESTINATION_COMMIT", named)
            // No `origin` exists: a fetch would fail the step.
            .env("BITBUCKET_PR_DESTINATION_BRANCH", "main");
        let out = fx.output(&mut command);
        assert!(out.status.success(), "{named}: {}", text(&out));
        assert!(
            ran(&fx.calls(), "1.2.3", &format!("ci --base {target}")),
            "{named}: {:?}",
            fx.calls()
        );
    }
}

/// A branch that started before the target raised its pin, and never
/// touched the pin, lowers nothing: merging it keeps the target's pin, and
/// the target's binary judges it.
#[test]
fn a_branch_that_predates_a_raised_pin_does_not_lower_it() {
    let fx = Fixture::new();
    let d = diverged(&fx, "feat: add a file");
    for (label, out, calls) in diverged_runs(&fx, &d) {
        assert!(out.status.success(), "{label}: {}", text(&out));
        assert!(!text(&out).contains("lowers"), "{label}: {}", text(&out));
        for gate in ["ci --base", "test --strict", "validate --docs"] {
            assert!(ran(&calls, "1.2.4", gate), "{label} {gate}: {calls:?}");
        }
        assert!(
            calls.iter().all(|c| c.starts_with("1.2.4 ")),
            "{label}: {calls:?}"
        );
    }
}

/// Run git in `dir` with the author and committer date set to `date`.
fn git_at(dir: &Path, date: &str, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_DATE", date)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// How a criss-cross head is shaped and dated (Fable's review of the round
/// 1 fix). `main` raises the pin from 1.2.3 to 1.2.4 on branch B while a
/// feature C starts from the same 1.2.3 commit.
#[derive(Clone, Copy, Debug)]
enum CrissCross {
    /// Both land on `main` by no-ff merges (D1 then D2); the head merges
    /// D1 into C with `-s ours`, keeping 1.2.3, and adds a commit. C is
    /// dated after D1, so `git merge-base` picks C, the 1.2.3 base.
    NoFfFeatureNewer,
    /// The same shape with C dated before B.
    NoFfFeatureOlder,
    /// The target merges C into B; the head is C's tree plus a file with
    /// parents C and B. C is dated after B.
    Plain,
}

/// Build `shape` on top of the fixture's 1.2.3 scaffold and return the
/// target and the head, whose real merge lowers the pin to 1.2.3.
fn criss_cross(fx: &Fixture, shape: CrissCross) -> (String, String) {
    let repo = fx.repo();
    let a = git(&repo, &["rev-parse", "main"]);
    let (raise, feature) = match shape {
        CrissCross::NoFfFeatureNewer => ("2030-01-02T10:00:00", "2030-01-04T10:00:00"),
        CrissCross::NoFfFeatureOlder => ("2030-01-03T10:00:00", "2030-01-02T10:00:00"),
        CrissCross::Plain => ("2030-01-02T10:00:00", "2030-01-03T10:00:00"),
    };
    git(&repo, &["checkout", "-q", "-B", "raise", &a]);
    set_pin(&repo, "1.2.4");
    git_at(
        &repo,
        raise,
        &["commit", "-qam", "chore: raise the codeflow pin"],
    );
    let b = git(&repo, &["rev-parse", "HEAD"]);
    git(&repo, &["checkout", "-q", "-B", "feat/x", &a]);
    std::fs::write(repo.join("c.txt"), "c\n").unwrap();
    git(&repo, &["add", "-A"]);
    git_at(&repo, feature, &["commit", "-qm", "feat: add c"]);
    let c = git(&repo, &["rev-parse", "HEAD"]);
    let (target, head) = if let CrissCross::Plain = shape {
        git(&repo, &["checkout", "-q", "--detach", &b]);
        git_at(
            &repo,
            "2030-01-04T12:00:00",
            &["merge", "-q", "--no-ff", "-m", "Merge feat", &c],
        );
        let target = git(&repo, &["rev-parse", "HEAD"]);
        git(&repo, &["checkout", "-q", "feat/x"]);
        std::fs::write(repo.join("e.txt"), "e\n").unwrap();
        git(&repo, &["add", "-A"]);
        let tree = git(&repo, &["write-tree"]);
        let head = git_at(
            &repo,
            "2030-01-06T10:00:00",
            &[
                "commit-tree",
                &tree,
                "-p",
                &c,
                "-p",
                &b,
                "-m",
                "chore: sync with main",
            ],
        );
        (target, head)
    } else {
        git(&repo, &["checkout", "-q", "main"]);
        git_at(
            &repo,
            "2030-01-03T12:00:00",
            &["merge", "-q", "--no-ff", "-m", "Merge raise", &b],
        );
        let d1 = git(&repo, &["rev-parse", "HEAD"]);
        git_at(
            &repo,
            "2030-01-05T10:00:00",
            &["merge", "-q", "--no-ff", "-m", "Merge feat", &c],
        );
        let target = git(&repo, &["rev-parse", "HEAD"]);
        git(&repo, &["checkout", "-q", "feat/x"]);
        git_at(
            &repo,
            "2030-01-06T10:00:00",
            &[
                "merge",
                "-q",
                "-s",
                "ours",
                "-m",
                "chore: sync with main",
                &d1,
            ],
        );
        std::fs::write(repo.join("e.txt"), "e\n").unwrap();
        git(&repo, &["add", "-A"]);
        git_at(
            &repo,
            "2030-01-06T11:00:00",
            &["commit", "-qm", "feat: add e"],
        );
        (target, git(&repo, &["rev-parse", "HEAD"]))
    };
    git(&repo, &["checkout", "-q", "--detach", &head]);
    (target, head)
}

/// A head that reaches an old-pin merge base through a criss-cross merge
/// cannot pass as having kept its pin: every merge base must carry the
/// head's pin, since the real merge lowers it (Fable's review, P1).
#[test]
fn a_criss_cross_head_cannot_keep_a_lowered_pin() {
    for shape in [
        CrissCross::NoFfFeatureNewer,
        CrissCross::NoFfFeatureOlder,
        CrissCross::Plain,
    ] {
        for platform in PLATFORMS {
            let fx = Fixture::new();
            fx.project("1.2.3");
            fx.publish("1.2.3", &Binary::Real);
            fx.publish("1.2.4", &Binary::Real);
            let (target, head) = criss_cross(&fx, shape);
            // The fixture is what the review found: git's real merge lowers it.
            let merged = git(&fx.repo(), &["merge-tree", "--write-tree", &target, &head]);
            let tree = merged.lines().next().unwrap();
            let state = git(
                &fx.repo(),
                &["show", &format!("{tree}:.codeflow/project.toml")],
            );
            assert!(
                state.contains("scaffold_version = \"1.2.3\""),
                "{shape:?}: {state}"
            );

            let out = fx.run(platform, Some(&target), &head);
            assert!(
                !out.status.success(),
                "{shape:?} {platform:?}: {}",
                text(&out)
            );
            assert!(
                text(&out).contains("lowers scaffold_version from 1.2.4 to 1.2.3"),
                "{shape:?} {platform:?}: {}",
                text(&out)
            );
            let calls = fx.calls();
            assert!(calls.iter().all(|c| c.starts_with("1.2.4 ")), "{calls:?}");
        }
    }
}
