//! The push set, id reservations, the release preflight and the
//! per-user registry: each finding raised by a real push or command.

use super::*;

// ---------------------------------------------------------------------------
// Fixtures.
// ---------------------------------------------------------------------------

/// Point the reads of `dest` at a mirror that is down, while pushes still go
/// straight to it (`insteadOf` for reads, `pushInsteadOf` for pushes).
/// Returns the config key whose removal brings the reads back.
fn reads_down(root: &Path, dest: &Path) -> String {
    let dest = dest.to_str().unwrap();
    let mirror = root.parent().unwrap().join("mirror.git");
    let key = format!("url.{}.insteadOf", mirror.display());
    git(root, &["config", &key, dest]);
    git(
        root,
        &["config", &format!("url.{dest}.pushInsteadOf"), dest],
    );
    key
}

/// `git push` with `first` ahead of everything else on the `PATH` the hooks
/// see.
fn push_with_path(root: &Path, first: &Path, args: &[&str]) -> String {
    let mut cmd = command("git", root);
    let path = cmd
        .get_envs()
        .find(|(key, _)| *key == "PATH")
        .and_then(|(_, value)| value)
        .unwrap()
        .to_owned();
    let path = std::env::join_paths(
        std::iter::once(first.to_path_buf()).chain(std::env::split_paths(&path)),
    )
    .unwrap();
    text(
        &cmd.env("PATH", path)
            .arg("push")
            .arg("-q")
            .args(args)
            .output()
            .unwrap(),
    )
}

/// This repository's release calculator and the path-set table it reads.
const RELEASE_SCRIPT: &str = include_str!("../../../../scripts/release.py");
const PATH_SETS: &str = include_str!("../../../codeflow-core/src/workgraph/path_sets.toml");
const PATH_SETS_AT: &str = "crates/codeflow-core/src/workgraph/path_sets.toml";

/// The scaffold version `codeflow init` stamped.
fn stamped_version(root: &Path) -> String {
    read(root, ".codeflow/project.toml")
        .lines()
        .find_map(|line| line.strip_prefix("scaffold_version = "))
        .unwrap()
        .trim_matches('"')
        .to_string()
}

/// Set every coupled stamp the release calculator compares to `version`,
/// as `release.py sync` does: the Cargo workspace and lock, and the
/// scaffold's own stamps.
fn stamp(root: &Path, version: &str) {
    let current = stamped_version(root);
    write(
        root,
        "Cargo.toml",
        &format!("[workspace]\nmembers = []\n\n[workspace.package]\nversion = \"{version}\"\n"),
    );
    let packages = ["codeflow-cli", "codeflow-core", "codeflow-present"]
        .map(|name| format!("[[package]]\nname = \"{name}\"\nversion = \"{version}\"\n"));
    write(
        root,
        "Cargo.lock",
        &format!("version = 4\n\n{}", packages.join("\n")),
    );
    for (path, shape) in [
        (".codeflow/project.toml", "scaffold_version = \"{}\""),
        (".codeflow/manifest.json", "\"scaffold_version\": \"{}\""),
        ("AGENTS.md", "scaffold={} -->"),
        ("CLAUDE.md", "scaffold={} -->"),
    ] {
        let text = read(root, path);
        let from = shape.replace("{}", &current);
        assert!(text.contains(&from), "{path} lacks {from}");
        write(
            root,
            path,
            &text.replace(&from, &shape.replace("{}", version)),
        );
    }
}

/// The changelog with the published `version` and, when given, a pending
/// patch entry at `pending`.
fn changelog(version: &str, pending: Option<&str>) -> String {
    let published = format!("## [{version}] - 2026-01-01\n\n- public\n");
    match pending {
        Some(next) => format!(
            "# Changelog\n\n## [{next}]\n\n<!-- codeflow:release-impact patch -->\n\
             - **Fix the tool.** It works now.\n\n{published}"
        ),
        None => format!("# Changelog\n\n{published}"),
    }
}

/// The next patch version after `version`.
fn next_patch(version: &str) -> String {
    let (head, patch) = version.rsplit_once('.').unwrap();
    format!("{head}.{}", patch.parse::<u32>().unwrap() + 1)
}

/// Adopt `CodeFlow`'s release calculator in a project on `feat/x` with a
/// destination: this repository's `scripts/release.py`, a published
/// baseline at the scaffold's version with its local tag, and the release
/// configuration, landed as the destination's target. Returns the
/// published version.
fn released(root: &Path, dest: &Path) -> String {
    let version = stamped_version(root);
    write(root, "scripts/release.py", RELEASE_SCRIPT);
    write(root, PATH_SETS_AT, PATH_SETS);
    write(root, "CHANGELOG.md", &changelog(&version, None));
    stamp(root, &version);
    let state = read(root, ".codeflow/project.toml");
    write(
        root,
        ".codeflow/project.toml",
        &format!("{state}\n[release]\nbackend = \"codeflow\"\n"),
    );
    write(root, ".codeflow/test-config.json", QUICK_CONFIG);
    commit_all(root, "chore: record the published baseline");
    let tag = format!("v{version}");
    git(root, &["tag", &tag]);
    let rev = |spec: &str| {
        String::from_utf8(run("git", root, &["rev-parse", spec]).stdout)
            .unwrap()
            .trim()
            .to_string()
    };
    let (baseline, tree) = (rev("HEAD"), rev("HEAD^{tree}"));
    // The digest the bootstrap records for the published section.
    let section = format!("## [{version}] - 2026-01-01\n\n- public");
    let digest = run(
        "python3",
        root,
        &[
            "-c",
            "import hashlib, sys; print(hashlib.sha256(sys.argv[1].encode()).hexdigest())",
            &section,
        ],
    );
    let target = String::from_utf8(run("git", dest, &["symbolic-ref", "--short", "HEAD"]).stdout)
        .unwrap()
        .trim()
        .to_string();
    let config = serde_json::json!({
        "schema_version": 2,
        "release_unit": "codeflow",
        "main_branch": target,
        "bootstrap": {
            "comparison": {"tag": tag, "commit": baseline, "tree": tree},
            "published": {
                "changelog_sha256": String::from_utf8(digest.stdout).unwrap().trim(),
                "version": version,
                "source_commit": baseline,
                "release_target_commit": baseline,
                "source_archive_sha256": "a".repeat(64),
            },
        },
        "watched_contract_paths": [],
    });
    write(
        root,
        ".release/config.json",
        &serde_json::to_string_pretty(&config).unwrap(),
    );
    commit_all(root, "chore: add the release configuration");
    git(
        dest,
        &[
            "fetch",
            "-q",
            root.to_str().unwrap(),
            &format!("feat/x:{target}"),
        ],
    );
    git(root, &["fetch", "-q", "origin"]);
    version
}

/// Push, and check that the hook ran the release preflight.
fn preflighted(root: &Path) -> String {
    let out = pushed(root);
    assert!(out.contains("release preflight"), "{out}");
    out
}

// ---------------------------------------------------------------------------
// The destination and the time budget.
// ---------------------------------------------------------------------------

#[test]
fn clears_push_destination_silent() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    let dest = with_destination(&root);
    write(&root, ".codeflow/test-config.json", QUICK_CONFIG);
    commit_all(&root, "chore: add a quick target");
    pushed(&root);
    let rewrite = reads_down(&root, &dest);
    commit(&root, "y.txt", "feat: add y");
    let finding = "asking the destination for its branches failed";
    let before = pushed(&root);
    let printed = block(&before, finding).to_string();
    assert_prints_row(&printed, "PUSH_DESTINATION_SILENT");
    let step = printed_command(&printed, "PUSH_DESTINATION_SILENT", None);
    let asked = run("git", &root, &["ls-remote", "origin"]);
    assert!(!asked.status.success(), "{}", text(&asked));
    // The mirror's outage ends: reads reach the destination again.
    git(&root, &["config", "--unset", &rewrite]);
    run_printed(&root, &step, &[("<remote>", "origin")], &[]);
    commit(&root, "z.txt", "feat: add z");
    let after = pushed(&root);
    assert!(
        !after.contains(finding) && after.contains("push set finished"),
        "{after}"
    );
}

/// A test-config with a fast target and one whose quick mode sleeps past
/// the push set's 60 s budget.
fn slow_target_config() -> serde_json::Value {
    let mut config: serde_json::Value = serde_json::from_str(QUICK_CONFIG).unwrap();
    config["targets"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "name": "slow",
            "enabled": true,
            "runner": "custom",
            "modes": {"quick": {"command": "sleep 61"}, "full": {"command": "sleep 61"}},
        }));
    config
}

#[test]
fn clears_push_over_budget_target() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    with_destination(&root);
    let mut config = slow_target_config();
    write(
        &root,
        ".codeflow/test-config.json",
        &serde_json::to_string_pretty(&config).unwrap(),
    );
    commit_all(&root, "chore: add a slow quick target");
    let finding = "over its 60s budget";
    let before = pushed(&root);
    let printed = block(&before, finding).to_string();
    assert_prints_row(&printed, "PUSH_OVER_BUDGET_TARGET");
    assert!(printed.contains("target 'slow'"), "{printed}");
    config["targets"][1]["modes"]
        .as_object_mut()
        .unwrap()
        .remove("quick");
    write(
        &root,
        ".codeflow/test-config.json",
        &serde_json::to_string_pretty(&config).unwrap(),
    );
    commit_all(&root, "chore: move the slow target to the full gate");
    let after = pushed(&root);
    assert!(
        !after.contains(finding) && after.contains("quick targets passed"),
        "{after}"
    );
}

/// Write `script` as an executable `python3` in `bin`, a stand-in on the
/// `PATH` the hooks see that ends by running the real one.
#[cfg(unix)]
fn python_stand_in(bin: &Path, script: &str) {
    let real = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|dir| dir.join("python3"))
        .find(|path| path.is_file())
        .expect("python3 on PATH");
    write(
        bin,
        "python3",
        &format!("#!/bin/sh\n{script}exec '{}' \"$@\"\n", real.display()),
    );
    let made = run("chmod", bin, &["+x", "python3"]);
    assert!(made.status.success(), "{}", text(&made));
}

/// A `python3` that takes 61 s to start: a release preflight slower than
/// the push set's budget.
#[cfg(unix)]
fn slow_python(root: &Path) -> PathBuf {
    let bin = root.parent().unwrap().join("slow-bin");
    python_stand_in(&bin, "sleep 61\n");
    bin
}

/// The same stand-in on Windows, which starts only a `python3.exe` by that
/// name: a small program, compiled here, that waits and runs the real one.
#[cfg(windows)]
fn slow_python(root: &Path) -> PathBuf {
    let bin = root.parent().unwrap().join("slow-bin");
    std::fs::create_dir_all(&bin).unwrap();
    let real = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|dir| dir.join("python3.exe"))
        .find(|path| path.is_file())
        .expect("python3 on PATH");
    let source = bin.join("slow_python.rs");
    std::fs::write(
        &source,
        format!(
            "fn main() {{\n    std::thread::sleep(std::time::Duration::from_secs(61));\n    \
             let status = std::process::Command::new(r\"{}\")\n        \
             .args(std::env::args_os().skip(1))\n        .status()\n        \
             .expect(\"python3 runs\");\n    std::process::exit(status.code().unwrap_or(1));\n}}\n",
            real.display()
        ),
    )
    .unwrap();
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let built = Command::new(rustc)
        .arg(&source)
        .arg("-o")
        .arg(bin.join("python3.exe"))
        .output()
        .unwrap();
    assert!(built.status.success(), "{}", text(&built));
    bin
}

#[test]
fn clears_push_over_budget_builtin() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    let dest = with_destination(&root);
    released(&root, &dest);
    let slow = slow_python(&root);
    prove(
        "PUSH_OVER_BUDGET_BUILTIN",
        "over its 60s budget",
        || push_with_path(&root, &slow, &["origin", "HEAD"]),
        |printed| {
            assert!(printed.contains("built-in `release preflight"), "{printed}");
            let policy = read(&root, ".codeflow/policy.json");
            let mut value: serde_json::Value = serde_json::from_str(&policy).unwrap();
            value["git"]["test_gate_on_push"] = serde_json::json!("off");
            write(
                &root,
                ".codeflow/policy.json",
                &serde_json::to_string_pretty(&value).unwrap(),
            );
            commit_all(&root, "chore: drop the push set");
        },
    );
}

/// A copy of the binary under test, first on the hook's `PATH` so it is the
/// hook's own binary, and a one-shot `python3` beside it that deletes the
/// copy and itself: the binary vanishes during the release preflight, after
/// `codeflow ci` ran through it and before `codeflow validate --docs` does.
#[cfg(unix)]
fn vanishing_binary(root: &Path) -> PathBuf {
    let bin = root.parent().unwrap().join("vanishing-bin");
    std::fs::create_dir_all(&bin).unwrap();
    let copy = bin.join("codeflow");
    std::fs::copy(exe(), &copy).unwrap();
    python_stand_in(
        &bin,
        &format!(
            "rm -f '{}' '{}'\n",
            copy.display(),
            bin.join("python3").display()
        ),
    );
    bin
}

// The binary deletes itself while a hook runs it. Windows cannot delete a
// running executable, so the vanishing binary exists only on Unix.
#[cfg(unix)]
#[test]
fn clears_push_set_by_hand() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    let dest = with_destination(&root);
    released(&root, &dest);
    let bin = vanishing_binary(&root);
    let row = "PUSH_SET_BY_HAND";
    let finding = "push set check could not run";
    let before = push_with_path(&root, &bin, &["origin", "HEAD"]);
    let printed = block(&before, finding).to_string();
    assert_prints_row(&printed, row);
    assert!(
        printed.contains("could not run: `codeflow validate --docs`"),
        "{printed}"
    );
    // By hand, with the binary under test.
    let ci = printed_command(&printed, row, None);
    run_printed(&root, &ci, &[], &[]);
    let validate = printed_command(&printed, row, Some("codeflow validate"));
    assert_eq!(validate, "codeflow validate --docs", "{printed}");
    run_printed(&root, &validate, &[], &[]);
    // The binary is back, and nothing removes it this time.
    std::fs::copy(exe(), bin.join("codeflow")).unwrap();
    let after = push_with_path(&root, &bin, &["origin", "HEAD"]);
    assert!(
        !after.contains(finding) && after.contains("push set finished"),
        "{after}"
    );
}

// ---------------------------------------------------------------------------
// Id reservations: the authority is a local bare destination.
// ---------------------------------------------------------------------------

/// A full-tier project (the tier that keeps an id registry) on `plan/work`
/// whose destination is the id authority, holding a task reserved while
/// the authority's reads were down: a pending reservation. Returns the
/// destination and the read rewrite, still in place.
fn pending_reservation(root: &Path) -> (PathBuf, String) {
    let dest = with_destination(root);
    git(root, &["switch", "-q", "-c", "plan/work"]);
    write(root, ".codeflow/test-config.json", QUICK_CONFIG);
    commit_all(root, "chore: add a quick target");
    let seeded = codeflow(root, &["ids", "seed"]);
    assert!(!seeded.contains("error"), "{seeded}");
    let rewrite = reads_down(root, &dest);
    let out = codeflow(
        root,
        &[
            "task",
            "new",
            "--standalone-reason",
            "a fixture",
            "the work",
        ],
    );
    assert!(out.contains("TSK-001") && out.contains("pending"), "{out}");
    commit_all(root, "docs: plan the work");
    (dest, rewrite)
}

/// The authority's registry holds the reservation that was pending.
fn assert_published(dest: &Path) {
    let listed = run(
        "git",
        dest,
        &["ls-tree", "-r", "--name-only", "codeflow/registry"],
    );
    let listed = text(&listed);
    assert!(listed.contains("ids/TSK/001.toml"), "{listed}");
}

#[test]
fn clears_ids_pending_local() {
    let dir = scaffolded("--full");
    let root = project(&dir);
    let (dest, rewrite) = pending_reservation(&root);
    git(&root, &["config", "--unset", &rewrite]);
    git(&root, &["remote", "add", "backup", dest.to_str().unwrap()]);
    let finding = "pending id reservations stay local";
    let before = push(&root, &["backup", "HEAD"]);
    let printed = block(&before, finding).to_string();
    assert_prints_row(&printed, "IDS_PENDING_LOCAL");
    let step = printed_command(&printed, "IDS_PENDING_LOCAL", None);
    run_printed(&root, &step, &[], &[]);
    commit(&root, "y.txt", "feat: add y");
    let after = push(&root, &["backup", "HEAD"]);
    assert!(!after.contains(finding), "{after}");
    assert_published(&dest);
}

#[test]
fn clears_ids_sync_failed() {
    let dir = scaffolded("--full");
    let root = project(&dir);
    let (dest, rewrite) = pending_reservation(&root);
    let finding = "ids sync skipped, reservations stay pending";
    let before = pushed(&root);
    let printed = block(&before, finding).to_string();
    assert_prints_row(&printed, "IDS_SYNC_FAILED");
    // The authority answers again.
    git(&root, &["config", "--unset", &rewrite]);
    let step = printed_command(&printed, "IDS_SYNC_FAILED", None);
    run_printed(&root, &step, &[], &[]);
    commit(&root, "y.txt", "feat: add y");
    let after = pushed(&root);
    assert!(!after.contains(finding), "{after}");
    assert_published(&dest);
}

#[test]
fn clears_doctor_id_registry() {
    // The full tier keeps an id registry; with no remote it is local.
    let dir = scaffolded("--full");
    let root = project(&dir);
    git(&root, &["switch", "-q", "-c", "plan/work"]);
    let out = codeflow(
        &root,
        &[
            "task",
            "new",
            "--standalone-reason",
            "a fixture",
            "the work",
        ],
    );
    assert!(out.contains("reserved in the local registry"), "{out}");
    commit_all(&root, "docs: plan the work");
    // A record written by hand, under a number the registry never issued.
    git(&root, &["switch", "-q", "-c", "plan/fork"]);
    let hand = "project-management/tasks/TSK-040.md";
    write(
        &root,
        hand,
        &read(&root, TASK).replace("TSK-001", "TSK-040"),
    );
    set_field(&root, hand, "uid", "0b9c7e5a-3f1d-4c2e-9a8b-7d6e5f4a3b21");
    commit_all(&root, "docs: add a hand-written task");
    prove(
        "DOCTOR_ID_REGISTRY",
        "cannot place TSK-040",
        || doctor(&root, "id-registry"),
        |printed| {
            let admit = printed_command(printed, "DOCTOR_ID_REGISTRY", Some("codeflow ids admit"));
            assert_eq!(admit, "codeflow ids admit", "{printed}");
            run_printed(&root, &admit, &[], &[hand]);
            let confirm = printed_command(printed, "DOCTOR_ID_REGISTRY", None);
            run_printed(&root, &confirm, &[], &[]);
        },
    );
}

// ---------------------------------------------------------------------------
// The release preflight: this repository's calculator in the fixture.
// ---------------------------------------------------------------------------

#[test]
fn clears_release_preflight_note() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    let dest = with_destination(&root);
    let version = released(&root, &dest);
    commit(&root, "src/tool.txt", "feat: add the tool");
    let row = "RELEASE_PREFLIGHT_NOTE";
    let finding = "no pending changelog entry";
    prove(
        row,
        finding,
        || preflighted(&root),
        |printed| {
            // The pending entry, with the stamps at its version.
            let next = next_patch(&version);
            write(&root, "CHANGELOG.md", &changelog(&version, Some(&next)));
            stamp(&root, &next);
            commit_all(&root, "docs: add the pending entry");
            let step = printed_command(printed, row, Some("python3"));
            let said = run_printed(&root, &step, &[], &[]);
            assert!(!said.contains(finding), "{said}");
        },
    );
}

#[test]
fn clears_release_preflight() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    let dest = with_destination(&root);
    let version = released(&root, &dest);
    // A pending entry whose coupled stamps stay at the published version.
    let next = next_patch(&version);
    write(&root, "CHANGELOG.md", &changelog(&version, Some(&next)));
    commit_all(&root, "docs: add the pending entry");
    let row = "RELEASE_PREFLIGHT";
    let finding = "this push breaks the release tree";
    let before = pushed(&root);
    // The note that names the cause prints first; the violation follows.
    let violation = &before[before.find("policy rule git.test_gate_on_push").unwrap()..];
    let printed = block(violation, finding).to_string();
    assert_prints_row(&printed, row);
    assert!(before.contains("disagree with pending target"), "{before}");
    stamp(&root, &next);
    commit_all(&root, "chore: stamp the pending version");
    let step = printed_command(&printed, row, Some("python3"));
    run_printed(&root, &step, &[], &[]);
    let after = preflighted(&root);
    assert!(!after.contains(finding), "{after}");
}

#[test]
fn clears_release_preflight_unrun() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    let dest = with_destination(&root);
    released(&root, &dest);
    write(
        &root,
        "scripts/release.py",
        &format!("{RELEASE_SCRIPT}\nthis line is not python\n"),
    );
    commit_all(&root, "chore: damage the release script");
    let row = "RELEASE_PREFLIGHT_UNRUN";
    prove(
        row,
        "release preflight did not run for",
        || preflighted(&root),
        |printed| {
            let step = printed_command(printed, row, Some("python3"));
            let broken = run(
                "python3",
                &root,
                &words(&step)[1..]
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
            );
            assert!(!broken.status.success(), "{}", text(&broken));
            write(&root, "scripts/release.py", RELEASE_SCRIPT);
            commit_all(&root, "fix: restore the release script");
            run_printed(&root, &step, &[], &[]);
        },
    );
}

// ---------------------------------------------------------------------------
// The per-user registry and the staged-diff scan.
// ---------------------------------------------------------------------------

#[test]
fn clears_registry_unwritten() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    // Damaging the per-user registry needs a home of this test's own.
    let home = tempfile::tempdir().unwrap();
    let registry = home.path().join("registry.json");
    std::fs::write(&registry, "{ not json").unwrap();
    let status = || {
        text(
            &command(exe().to_str().unwrap(), &root)
                .env("CODEFLOW_HOME", home.path())
                .arg("status")
                .output()
                .unwrap(),
        )
    };
    prove(
        "REGISTRY_UNWRITTEN",
        "registry touch failed",
        status,
        |printed| {
            let named = printed
                .split("repair or delete ")
                .nth(1)
                .and_then(|rest| rest.split(", the per-user").next())
                .unwrap_or_else(|| panic!("no path printed:\n{printed}"));
            assert_eq!(Path::new(named), registry, "{printed}");
            std::fs::remove_file(named).unwrap();
        },
    );
    let written = std::fs::read_to_string(&registry).unwrap();
    let listed: serde_json::Value = serde_json::from_str(&written).unwrap();
    assert_eq!(
        listed["repos"].as_array().map(Vec::len),
        Some(1),
        "{written}"
    );
}

#[test]
fn clears_secret_scan_incomplete() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    git(&root, &["switch", "-q", "-c", "feat/x"]);
    write(&root, "app.txt", "app\n");
    git(&root, &["add", "app.txt"]);
    // The staged blob goes missing from the object store: the scan cannot
    // read the staged diff.
    let sha = String::from_utf8(run("git", &root, &["rev-parse", ":app.txt"]).stdout).unwrap();
    let sha = sha.trim();
    std::fs::remove_file(root.join(".git/objects").join(&sha[..2]).join(&sha[2..])).unwrap();
    let finding = "staged secret scan incomplete";
    let message = ["-q", "-m", "feat: add the app"];
    let before = text(&run("git", &root, &[&["commit"], &message[..]].concat()));
    let printed = block(&before, finding).to_string();
    assert_prints_row(&printed, "SECRET_SCAN_INCOMPLETE");
    // Writing the staged content back makes the diff readable again; the
    // retry is the scan run again.
    git(&root, &["hash-object", "-w", "app.txt"]);
    let step = printed_command(&printed, "SECRET_SCAN_INCOMPLETE", None);
    let after = run_printed(&root, &step, &[], &message);
    assert!(!after.contains(finding), "{after}");
    let tree =
        String::from_utf8(run("git", &root, &["ls-tree", "HEAD", "app.txt"]).stdout).unwrap();
    assert!(tree.contains(sha), "{tree}");
}

// ---------------------------------------------------------------------------
// The target's policy judges the push (sathyassn/codeflow#22).
// ---------------------------------------------------------------------------

const POLICY: &str = ".codeflow/policy.json";
const FOUR_BULLETS: &str = "feat: add y\n\n- one\n- two\n- three\n- four\n";

/// Set `git.<key>` in the working copy's policy.
fn set_policy(root: &Path, entries: &[(&str, serde_json::Value)]) {
    let mut policy: serde_json::Value = serde_json::from_str(&read(root, POLICY)).unwrap();
    for (key, value) in entries {
        policy["git"][*key] = value.clone();
    }
    write(
        root,
        POLICY,
        &serde_json::to_string_pretty(&policy).unwrap(),
    );
}

fn loosen(root: &Path) {
    set_policy(
        root,
        &[
            ("commit_body_max_bullets", 10.into()),
            ("commit_body_bullet_max_len", 100.into()),
        ],
    );
}

/// Commit a four-bullet change, which the working copy's policy lets in.
fn four_bullets(root: &Path) {
    write(root, "y.txt", "y\n");
    let said = commit_all(root, FOUR_BULLETS);
    let subject = text(&run("git", root, &["log", "-1", "--format=%s"]));
    assert_eq!(subject.trim(), "feat: add y", "{said}");
}

/// The destination's default branch name.
fn default_branch(dest: &Path) -> String {
    String::from_utf8(run("git", dest, &["symbolic-ref", "--short", "HEAD"]).stdout)
        .unwrap()
        .trim()
        .to_string()
}

/// Land a policy change on the destination's default branch, as a merged
/// pull request would, and fetch it; the checkout stays where it was.
fn land_on_default(root: &Path, dest: &Path, change: impl FnOnce(&Path)) {
    let target = default_branch(dest);
    let here = text(&run("git", root, &["branch", "--show-current"]));
    git(
        root,
        &[
            "switch",
            "-q",
            "-c",
            "land/policy",
            &format!("origin/{target}"),
        ],
    );
    change(root);
    commit_all(root, "chore: change the policy");
    git(
        dest,
        &[
            "fetch",
            "-q",
            root.to_str().unwrap(),
            &format!("land/policy:{target}"),
        ],
    );
    git(root, &["switch", "-q", here.trim()]);
    git(root, &["branch", "-q", "-D", "land/policy"]);
    git(root, &["fetch", "-q", "origin"]);
}

/// The push was refused for the commit-body rule and left the destination
/// without `feat/x` at `local`.
fn refused(dest: &Path, out: &str, local: &str) {
    refused_at(dest, out, "feat/x", local);
}

/// The push was refused for the commit-body rule and left the destination
/// without `branch` at `local`.
fn refused_at(dest: &Path, out: &str, branch: &str, local: &str) {
    assert!(out.contains("git.commit_body"), "{out}");
    not_landed(dest, out, branch, local);
}

fn not_landed(dest: &Path, out: &str, branch: &str, local: &str) {
    let there = text(&run("git", dest, &["rev-parse", "--verify", "-q", branch]));
    assert_ne!(there.trim(), local, "the push went through:\n{out}");
}

/// Publish `branch` at the destination from the default branch with
/// `change` applied, as a landed pull request would, and fetch it.
fn land_branch(root: &Path, dest: &Path, branch: &str, change: impl FnOnce(&Path)) {
    let target = default_branch(dest);
    let here = text(&run("git", root, &["branch", "--show-current"]));
    git(
        root,
        &[
            "switch",
            "-q",
            "-c",
            "land/branch",
            &format!("origin/{target}"),
        ],
    );
    change(root);
    commit_all(root, "chore: change the policy");
    git(
        dest,
        &[
            "fetch",
            "-q",
            root.to_str().unwrap(),
            &format!("land/branch:{branch}"),
        ],
    );
    git(root, &["switch", "-q", here.trim()]);
    git(root, &["branch", "-q", "-D", "land/branch"]);
    git(root, &["fetch", "-q", "origin"]);
}

/// A task record declaring `target` as its integration target.
fn declare(root: &Path, id: &str, target: &str) {
    write(
        root,
        &format!("project-management/tasks/{id}.md"),
        &format!(
            "---\nid: {id}\nepic_id: null\nstandalone_reason: \"a fixture\"\ntitle: \"the work\"\nstatus: todo\nwork_type: feat\nspecs: []\ndepends_on: []\nintegration_target: \"{target}\"\nexternal_refs: []\ncreated: 2026-10-03\n---\n\n# {id}: the work\n\n## Description\n\nA fixture.\n\n## Acceptance Criteria\n\n- AC-1 the fixture holds; evidence: this test.\n"
        ),
    );
}

/// Add `pattern` to the working copy's protected branches.
fn protect(root: &Path, pattern: &str) {
    let mut protected: Vec<serde_json::Value> =
        serde_json::from_str::<serde_json::Value>(&read(root, POLICY)).unwrap()["git"]
            ["protected_branches"]
            .as_array()
            .unwrap()
            .clone();
    protected.push(pattern.into());
    set_policy(root, &[("protected_branches", protected.into())]);
}

/// Turn off the working copy's own protected-branch rules, so a branch it
/// protects can be committed to and pushed from here.
fn unguard(root: &Path) {
    set_policy(
        root,
        &[
            ("commit_to_protected", "off".into()),
            ("push_to_protected", "off".into()),
            ("local_ref_protection", "off".into()),
        ],
    );
}

fn head_sha(root: &Path) -> String {
    text(&run("git", root, &["rev-parse", "HEAD"]))
        .trim()
        .to_string()
}

/// A branch that loosens its own commit-body rules commits under them, but
/// its first push is judged by the policy at its target.
#[test]
fn a_push_is_judged_by_the_policy_the_destination_holds() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    let dest = with_destination(&root);
    write(&root, ".codeflow/test-config.json", QUICK_CONFIG);
    loosen(&root);
    commit_all(&root, "chore: loosen the commit body rules");
    four_bullets(&root);
    let out = pushed(&root);
    refused(&dest, &out, &head_sha(&root));
    assert!(out.contains("judges 'feat/x' with the policy at"), "{out}");
}

/// A branch already at the destination with its own looser policy: the
/// next push's range starts at that branch's own tip, which never judges
/// it.
#[test]
fn a_second_push_is_not_judged_by_the_branch_s_own_last_push() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    let dest = with_destination(&root);
    write(&root, ".codeflow/test-config.json", QUICK_CONFIG);
    loosen(&root);
    commit_all(&root, "chore: loosen the commit body rules");
    let first = pushed(&root);
    let there = text(&run("git", &dest, &["rev-parse", "feat/x"]));
    assert_eq!(there.trim(), head_sha(&root), "{first}");
    four_bullets(&root);
    let out = pushed(&root);
    refused(&dest, &out, &head_sha(&root));
}

/// The target tightens its policy after the branch forked: the merge base
/// would let the change in, the target's tip does not.
#[test]
fn a_push_is_judged_by_the_target_s_tip_not_the_fork_point() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    let dest = with_destination(&root);
    land_on_default(&root, &dest, loosen);
    let target = default_branch(&dest);
    git(
        &root,
        &["switch", "-q", "-C", "feat/x", &format!("origin/{target}")],
    );
    land_on_default(&root, &dest, |root| {
        set_policy(
            root,
            &[
                ("commit_body_max_bullets", 3.into()),
                ("commit_body_bullet_max_len", 72.into()),
            ],
        );
    });
    write(&root, ".codeflow/test-config.json", QUICK_CONFIG);
    commit_all(&root, "chore: add a quick target");
    four_bullets(&root);
    let out = pushed(&root);
    refused(&dest, &out, &head_sha(&root));
}

/// A head cannot lower or turn off the gate its target sets for its
/// `codeflow ci`: `git.test_gate_on_push` comes from the target too.
#[test]
fn a_head_cannot_lower_or_turn_off_the_target_s_push_gate() {
    for level in ["warn", "off"] {
        let dir = scaffolded("--standard");
        let root = project(&dir);
        let dest = with_destination(&root);
        write(&root, ".codeflow/test-config.json", QUICK_CONFIG);
        loosen(&root);
        set_policy(&root, &[("test_gate_on_push", level.into())]);
        commit_all(&root, "chore: loosen the push gate");
        four_bullets(&root);
        let out = pushed(&root);
        refused(&dest, &out, &head_sha(&root));
        assert!(out.contains("(block)"), "{level}: {out}");
    }
}

/// A declared target the default branch's policy does not protect is a
/// hint from the branch, so it never chooses the policy that judges it
/// (review round two, finding 1).
#[test]
fn a_declared_target_the_default_does_not_protect_is_not_trusted() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    let dest = with_destination(&root);
    land_branch(&root, &dest, "integration/lenient", loosen);
    git(
        &root,
        &[
            "switch",
            "-q",
            "-c",
            "task/TSK-001-work",
            "origin/integration/lenient",
        ],
    );
    write(&root, ".codeflow/test-config.json", QUICK_CONFIG);
    declare(&root, "TSK-001", "integration/lenient");
    commit_all(&root, "docs: plan the work");
    four_bullets(&root);
    let out = push(&root, &["origin", "HEAD"]);
    refused_at(&dest, &out, "task/TSK-001-work", &head_sha(&root));
}

/// A branch that names itself protected in its own working copy is still
/// judged by the default branch's policy, not by its own last push.
#[test]
fn a_branch_cannot_protect_itself_into_its_own_authority() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    let dest = with_destination(&root);
    write(&root, ".codeflow/test-config.json", QUICK_CONFIG);
    loosen(&root);
    commit_all(&root, "chore: loosen the commit body rules");
    let first = pushed(&root);
    let there = text(&run("git", &dest, &["rev-parse", "feat/x"]));
    assert_eq!(there.trim(), head_sha(&root), "{first}");
    protect(&root, "feat/x");
    unguard(&root);
    commit_all(&root, "chore: protect the branch");
    four_bullets(&root);
    let out = pushed(&root);
    refused(&dest, &out, &head_sha(&root));
}

/// An `integration/` branch the destination already has is not its own
/// authority unless the default branch's policy protects it.
#[test]
fn a_new_integration_branch_is_not_its_own_authority() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    let dest = with_destination(&root);
    let target = default_branch(&dest);
    git(
        &root,
        &[
            "switch",
            "-q",
            "-c",
            "integration/lenient",
            &format!("origin/{target}"),
        ],
    );
    write(&root, ".codeflow/test-config.json", QUICK_CONFIG);
    loosen(&root);
    commit_all(&root, "chore: loosen the commit body rules");
    let first = push(&root, &["origin", "HEAD"]);
    let there = text(&run("git", &dest, &["rev-parse", "integration/lenient"]));
    assert_eq!(there.trim(), head_sha(&root), "{first}");
    four_bullets(&root);
    let out = push(&root, &["origin", "HEAD"]);
    refused_at(&dest, &out, "integration/lenient", &head_sha(&root));
}

/// A declared target the destination does not have, with the working
/// copy's push gate off: the head cannot turn the check off (finding 2).
#[test]
fn an_unknown_declared_target_does_not_let_the_head_skip_the_check() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    let dest = with_destination(&root);
    git(&root, &["switch", "-q", "-c", "task/TSK-001-work"]);
    write(&root, ".codeflow/test-config.json", QUICK_CONFIG);
    declare(&root, "TSK-001", "integration/nonexistent");
    loosen(&root);
    set_policy(&root, &[("test_gate_on_push", "off".into())]);
    commit_all(&root, "docs: plan the work");
    four_bullets(&root);
    let out = push(&root, &["origin", "HEAD"]);
    refused_at(&dest, &out, "task/TSK-001-work", &head_sha(&root));
}

/// A push to a fork cannot know its pull request's target: it is judged
/// by the fork's default branch, whatever the working copy's gate, and the
/// hook says the upstream's policy can differ.
#[test]
fn a_push_to_a_fork_is_judged_by_a_named_candidate_authority() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    let dest = with_destination(&root);
    let upstream = root.parent().unwrap().join("upstream.git");
    git(
        &root,
        &["remote", "add", "upstream", upstream.to_str().unwrap()],
    );
    write(&root, ".codeflow/test-config.json", QUICK_CONFIG);
    loosen(&root);
    set_policy(&root, &[("test_gate_on_push", "off".into())]);
    commit_all(&root, "chore: loosen the commit body rules");
    four_bullets(&root);
    let out = pushed(&root);
    refused(&dest, &out, &head_sha(&root));
    assert!(out.contains("may target the upstream"), "{out}");
    assert!(out.contains("a candidate authority"), "{out}");
}

/// A default branch whose policy fails strict validation cannot turn the
/// check off through a key the lenient reader accepts (finding 5).
#[test]
fn an_invalid_target_policy_does_not_turn_the_check_off() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    let dest = with_destination(&root);
    write(&root, ".codeflow/test-config.json", QUICK_CONFIG);
    write(&root, "x.txt", "x\n");
    commit_all(&root, "feat: add x");
    land_invalid_policy(&root, &dest);
    let out = pushed(&root);
    assert!(out.contains("is not valid for this codeflow"), "{out}");
    not_landed(&dest, &out, "feat/x", &head_sha(&root));
}

/// Land a default-branch policy that fails strict validation and turns the
/// push check off.
fn land_invalid_policy(root: &Path, dest: &Path) {
    land_unchecked_policy(root, dest, |root| {
        set_policy(
            root,
            &[
                ("test_gate_on_push", "off".into()),
                ("no_such_key", true.into()),
            ],
        );
    });
}

/// Land `change` to the default branch's policy. The local hooks refuse to
/// commit a policy they cannot use, so it lands with them off, as a change
/// made elsewhere would arrive; the checkout returns to `feat/x`.
fn land_unchecked_policy(root: &Path, dest: &Path, change: impl FnOnce(&Path)) {
    let target = default_branch(dest);
    git(
        root,
        &[
            "switch",
            "-q",
            "-c",
            "land/policy",
            &format!("origin/{target}"),
        ],
    );
    change(root);
    git(
        root,
        &[
            "-c",
            "core.hooksPath=/dev/null",
            "commit",
            "-q",
            "-am",
            "chore: an invalid policy",
        ],
    );
    git(
        dest,
        &[
            "fetch",
            "-q",
            root.to_str().unwrap(),
            &format!("land/policy:{target}"),
        ],
    );
    git(root, &["switch", "-q", "feat/x"]);
    git(root, &["fetch", "-q", "origin"]);
}

/// A malformed default policy refuses the push even when its range has no
/// base and `codeflow ci` cannot run (review round three, finding 2).
#[test]
fn an_invalid_default_policy_refuses_a_push_with_no_range() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    let dest = with_destination(&root);
    land_invalid_policy(&root, &dest);
    // A history the destination shares nothing with: a root commit of this
    // tree, so no advertised tip bounds its range.
    let made = run(
        "git",
        &root,
        &[
            "commit-tree",
            "HEAD^{tree}",
            "-m",
            "feat: an unrelated root",
        ],
    );
    let unrelated = text(&made).trim().to_string();
    assert!(made.status.success(), "{unrelated}");
    let out = push(
        &root,
        &["origin", &format!("{unrelated}:refs/heads/feat/unrelated")],
    );
    assert!(out.contains("is not valid for this codeflow"), "{out}");
    not_landed(&dest, &out, "feat/unrelated", &unrelated);
}

/// A protected wildcard does not make a branch the push creates its own
/// authority: after a first push of a line that relaxes its own policy,
/// the next push is still judged by the default branch (review round
/// three, finding 1).
#[test]
fn a_protected_wildcard_does_not_make_a_branch_its_own_authority() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    let dest = with_destination(&root);
    land_on_default(&root, &dest, |root| protect(root, "integration/*"));
    // The landed policy keeps a protected name from being made here, so
    // the line is pushed to it from a branch of another name.
    let target = default_branch(&dest);
    git(
        &root,
        &[
            "switch",
            "-q",
            "-c",
            "feat/self",
            &format!("origin/{target}"),
        ],
    );
    write(&root, ".codeflow/test-config.json", QUICK_CONFIG);
    loosen(&root);
    unguard(&root);
    set_policy(&root, &[("test_gate_on_push", "off".into())]);
    commit_all(&root, "chore: relax the line's own policy");
    let line = ["origin", "HEAD:refs/heads/integration/self"];
    let first = push(&root, &line);
    let there = text(&run("git", &dest, &["rev-parse", "integration/self"]));
    assert_eq!(there.trim(), head_sha(&root), "{first}");
    four_bullets(&root);
    let out = push(&root, &line);
    refused_at(&dest, &out, "integration/self", &head_sha(&root));
}

/// A protected symbolic branch at the destination does not lend the
/// policy of the branch it points at to a push that declares it as its
/// target (review round three, finding 1).
#[test]
fn a_protected_alias_does_not_lend_its_referent_s_policy() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    let dest = with_destination(&root);
    land_on_default(&root, &dest, |root| protect(root, "integration/*"));
    land_branch(&root, &dest, "feat/lenient", |root| {
        loosen(root);
        set_policy(root, &[("test_gate_on_push", "off".into())]);
    });
    git(
        &dest,
        &[
            "symbolic-ref",
            "refs/heads/integration/alias",
            "refs/heads/feat/lenient",
        ],
    );
    git(&root, &["fetch", "-q", "origin"]);
    git(
        &root,
        &[
            "switch",
            "-q",
            "-c",
            "task/TSK-001-work",
            "origin/feat/lenient",
        ],
    );
    write(&root, ".codeflow/test-config.json", QUICK_CONFIG);
    declare(&root, "TSK-001", "integration/alias");
    commit_all(&root, "docs: plan the work");
    four_bullets(&root);
    let out = push(&root, &["origin", "HEAD"]);
    refused_at(&dest, &out, "task/TSK-001-work", &head_sha(&root));
}

/// Advance the destination's default branch by a commit this clone does
/// not have, so judging a push needs a fetch.
#[cfg(unix)]
fn advance_unseen(dest: &Path) {
    let target = default_branch(dest);
    let made = run(
        "git",
        dest,
        &[
            "commit-tree",
            &format!("{target}^{{tree}}"),
            "-p",
            &target,
            "-m",
            "chore: advance",
        ],
    );
    let sha = text(&made).trim().to_string();
    assert!(made.status.success(), "{sha}");
    git(dest, &["update-ref", &format!("refs/heads/{target}"), &sha]);
}

/// Push with `args`, every object fetch from the destination refused by
/// its upload-pack and recorded. `uploadpack.packObjectsHook` is read only
/// from protected configuration, and git drops the command scope for a
/// local transport, so it is set in the global file the hook's own git
/// processes read. Returns the push's output and the number of fetches
/// refused.
#[cfg(unix)]
fn push_refusing_fetches(root: &Path, args: &[&str]) -> (String, usize) {
    let dir = root.parent().unwrap();
    let log = dir.join("fetches.log");
    write(
        dir,
        "refuse-pack",
        &format!("#!/bin/sh\necho fetch >> '{}'\nexit 1\n", log.display()),
    );
    let made = run("chmod", dir, &["+x", "refuse-pack"]);
    assert!(made.status.success(), "{}", text(&made));
    write(
        dir,
        "refusing.gitconfig",
        &format!(
            "[uploadpack]\n\tpackObjectsHook = {}\n",
            dir.join("refuse-pack").display()
        ),
    );
    let out = command("git", root)
        .env("GIT_CONFIG_GLOBAL", dir.join("refusing.gitconfig"))
        .arg("push")
        .arg("-q")
        .args(args)
        .output()
        .unwrap();
    let fetches = std::fs::read_to_string(&log).unwrap_or_default();
    (text(&out), fetches.lines().count())
}

/// A failed fetch of the default branch's tip is tried once per push, with
/// durable work tracking off and on: the release scope reuses the failure
/// instead of fetching again (review round three, finding 3).
#[cfg(unix)]
#[test]
fn a_failed_authority_fetch_is_tried_once_in_both_tracking_modes() {
    for tier in ["--standard", "--full"] {
        let dir = scaffolded(tier);
        let root = project(&dir);
        let dest = with_destination(&root);
        write(&root, "x.txt", "x\n");
        commit_all(&root, "feat: add x");
        advance_unseen(&dest);
        let (out, fetches) = push_refusing_fetches(&root, &["origin", "HEAD"]);
        not_landed(&dest, &out, "feat/x", &head_sha(&root));
        assert!(out.contains("SPC-013 R-120"), "{tier}: {out}");
        assert_eq!(fetches, 1, "{tier}: {out}");
    }
}

/// A default-branch policy whose bytes are not UTF-8 is content this
/// codeflow cannot read, not a missing authority: the push is refused
/// (review round four, finding 2).
#[test]
fn a_default_policy_that_is_not_utf8_refuses_the_push() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    let dest = with_destination(&root);
    write(&root, ".codeflow/test-config.json", QUICK_CONFIG);
    write(&root, "x.txt", "x\n");
    commit_all(&root, "feat: add x");
    land_unchecked_policy(&root, &dest, |root| {
        let mut bytes = std::fs::read(root.join(POLICY)).unwrap();
        bytes.extend_from_slice(b"\n\xff\n");
        std::fs::write(root.join(POLICY), bytes).unwrap();
    });
    let out = pushed(&root);
    assert!(out.contains("cannot be read by this codeflow"), "{out}");
    not_landed(&dest, &out, "feat/x", &head_sha(&root));
}

/// A commit the destination already holds under a tag is still checked
/// when a branch carries it: the advertised tag would leave the range
/// empty (review round four, finding 1).
#[test]
fn a_commit_published_as_a_tag_is_still_checked_on_its_branch() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    let dest = with_destination(&root);
    write(&root, ".codeflow/test-config.json", QUICK_CONFIG);
    loosen(&root);
    commit_all(&root, "chore: loosen the commit body rules");
    four_bullets(&root);
    let tagged = push(&root, &["origin", "HEAD:refs/tags/v0.0.1-bad"]);
    let there = text(&run("git", &dest, &["rev-parse", "v0.0.1-bad^{commit}"]));
    assert_eq!(there.trim(), head_sha(&root), "{tagged}");
    let out = pushed(&root);
    refused(&dest, &out, &head_sha(&root));
}

/// A commit an earlier push carried, which the default branch's policy
/// has since come to forbid, is checked again on the next push, as the
/// hosted job checks the whole branch from the target tip (review round
/// four, finding 1).
#[test]
fn an_earlier_push_is_checked_again_after_the_default_tightens() {
    let dir = scaffolded("--standard");
    let root = project(&dir);
    let dest = with_destination(&root);
    land_on_default(&root, &dest, loosen);
    let target = default_branch(&dest);
    git(
        &root,
        &["switch", "-q", "-C", "feat/x", &format!("origin/{target}")],
    );
    write(&root, ".codeflow/test-config.json", QUICK_CONFIG);
    commit_all(&root, "chore: add a quick target");
    four_bullets(&root);
    let first = pushed(&root);
    let there = text(&run("git", &dest, &["rev-parse", "feat/x"]));
    assert_eq!(there.trim(), head_sha(&root), "{first}");
    land_on_default(&root, &dest, |root| {
        set_policy(
            root,
            &[
                ("commit_body_max_bullets", 3.into()),
                ("commit_body_bullet_max_len", 72.into()),
            ],
        );
    });
    write(&root, "z.txt", "z\n");
    commit_all(&root, "feat: add z");
    let out = pushed(&root);
    refused(&dest, &out, &head_sha(&root));
}
