//! TSK-280 AC-4 (journey): a project at the last released version runs this
//! build's `codeflow update`, and the released binary still loads the policy
//! file it ends with. The released binary is the oracle, not a second
//! implementation of its schema.
//!
//! The test needs a binary built from the release tag, which this suite never
//! builds or downloads, so it is ignored unless that binary is named:
//!
//! ```sh
//! git worktree add --detach .worktrees/v311-oracle v3.1.1
//! (cd .worktrees/v311-oracle && CARGO_TARGET_DIR=$PWD/target cargo build -p codeflow-cli)
//! CODEFLOW_LAST_RELEASE_BIN=$PWD/.worktrees/v311-oracle/target/debug/codeflow \
//!   cargo test -p codeflow-cli --test policy_last_release_journey -- --ignored --nocapture
//! ```
//!
//! The released binary scaffolds the project (`init`), this build updates it,
//! and the released binary reads the result with `codeflow validate`, which
//! checks `.codeflow/policy.json` strictly, and `codeflow policy show`. A
//! planted unknown key, wrong-kind value and malformed structured value must
//! each be refused by the same commands, so the oracle is not vacuous.
//!
//! The fast placement checks in `codeflow-core/tests/policy_key_placement.rs`
//! stay as unit checks; this journey is the authority for AC-4.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn last_release_bin() -> PathBuf {
    let path = std::env::var_os("CODEFLOW_LAST_RELEASE_BIN")
        .expect("set CODEFLOW_LAST_RELEASE_BIN to a codeflow built from the last release tag");
    PathBuf::from(path)
}

fn run(exe: &Path, dir: &Path, home: &Path, args: &[&str]) -> Output {
    Command::new(exe)
        .args(args)
        .current_dir(dir)
        .env("CODEFLOW_HOME", home)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Journey")
        .env("GIT_AUTHOR_EMAIL", "journey@example.test")
        .env("GIT_COMMITTER_NAME", "Journey")
        .env("GIT_COMMITTER_EMAIL", "journey@example.test")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("CODEFLOW_HUMAN_OVERRIDE")
        .output()
        .unwrap_or_else(|error| panic!("{} runs: {error}", exe.display()))
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// Run `args` with `exe` and require success.
fn succeeds(exe: &Path, dir: &Path, home: &Path, args: &[&str]) -> String {
    let output = run(exe, dir, home, args);
    let out = text(&output);
    assert!(
        output.status.success(),
        "{} {args:?} failed ({}):\n{out}",
        exe.display(),
        output.status
    );
    out
}

/// The released binary reading the project's policy: `validate` strictly
/// checks `.codeflow/policy.json`; `policy show` reports invalid values.
fn released_reads(released: &Path, dir: &Path, home: &Path) -> Result<(), String> {
    for args in [&["validate"][..], &["policy", "show"][..]] {
        let output = run(released, dir, home, args);
        let out = text(&output);
        if !output.status.success() || out.contains("invalid") || out.contains("unknown key") {
            return Err(format!(
                "`codeflow {}` ({}):\n{out}",
                args.join(" "),
                output.status
            ));
        }
    }
    Ok(())
}

#[test]
#[ignore = "needs CODEFLOW_LAST_RELEASE_BIN, a codeflow built from the last release tag"]
fn update_leaves_a_last_release_project_loadable_by_the_released_binary() {
    let released = last_release_bin();
    let current = PathBuf::from(env!("CARGO_BIN_EXE_codeflow"));
    let home = tempfile::tempdir().expect("home tempdir");
    let project = tempfile::tempdir().expect("project tempdir");
    let (home, dir) = (home.path(), project.path());

    // The binaries' identities.
    let released_version = succeeds(&released, dir, home, &["--version"]);
    let current_version = succeeds(&current, dir, home, &["--version"]);
    eprintln!("released: {}", released_version.trim());
    eprintln!("current:  {}", current_version.trim());
    assert!(
        released_version.contains("3.1.1"),
        "the oracle is not the last release: {released_version}"
    );

    // 1. The released binary scaffolds the project from its own template.
    succeeds(&released, dir, home, &["init", "--minimal", "--yes"]);
    let policy_path = dir.join(".codeflow/policy.json");
    let shipped = std::fs::read_to_string(&policy_path).expect("policy.json exists");
    released_reads(&released, dir, home).expect("the released binary loads its own scaffold");

    // 2. This build updates it.
    let report = succeeds(&current, dir, home, &["update"]);
    eprintln!("update report:\n{report}");

    // 3. The released binary loads the result.
    let updated = std::fs::read_to_string(&policy_path).expect("policy.json exists");
    eprintln!(
        "policy.json {} by update",
        if updated == shipped {
            "unchanged"
        } else {
            "changed"
        }
    );
    released_reads(&released, dir, home)
        .unwrap_or_else(|why| panic!("the released binary refuses the updated policy: {why}"));

    // 4. The oracle is not vacuous: it refuses each planted case.
    let good: serde_json::Value = serde_json::from_str(&updated).expect("policy.json is JSON");
    let planted = [
        (
            "an unknown key",
            "planted_future_key",
            serde_json::json!("block"),
        ),
        (
            "a wrong-kind value",
            "commit_desc_max_len",
            serde_json::json!("fifty"),
        ),
        (
            "an unlisted level",
            "commit_format",
            serde_json::json!("strict"),
        ),
        (
            "a malformed profile entry",
            "automation_profiles",
            serde_json::json!([null]),
        ),
        (
            "a non-object section mapping",
            "pr_section_mapping",
            serde_json::json!([]),
        ),
    ];
    for (label, key, value) in planted {
        let mut bad = good.clone();
        bad["git"][key] = value;
        std::fs::write(&policy_path, serde_json::to_string_pretty(&bad).unwrap())
            .expect("write planted policy");
        let refused = released_reads(&released, dir, home);
        eprintln!(
            "planted {label}: {}",
            if refused.is_err() {
                "refused"
            } else {
                "ACCEPTED"
            }
        );
        assert!(
            refused.is_err(),
            "the released binary accepted {label}, so the journey proves nothing"
        );
    }
    std::fs::write(&policy_path, &updated).expect("restore policy");
    released_reads(&released, dir, home).expect("restored policy loads again");
}
