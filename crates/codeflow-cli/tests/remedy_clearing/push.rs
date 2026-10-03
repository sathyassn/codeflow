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
