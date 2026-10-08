//! `codeflow update --pin <version>` and the project setup hook through the
//! built binary (sathyassn/codeflow#46, #47): a pin downloads the release
//! from a local `file://` mirror, checks every archive against its
//! `sha256.sum`, writes only `scaffold_version` and `[scaffold_sha256]`, and
//! refuses anything else without writing; `codeflow update` never touches
//! `.codeflow/ci-setup.sh`; doctor reports both.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use codeflow_core::scaffold::release_pin::{pinned_digests, PinnedDigests, TRIPLES};
use codeflow_core::scaffold::sha256_hex;

const STATE: &str = ".codeflow/project.toml";
const HOOK: &str = ".codeflow/ci-setup.sh";

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", text(&out));
    String::from_utf8_lossy(&out.stdout).to_string()
}

struct Fixture {
    dir: tempfile::TempDir,
}

impl Fixture {
    /// A freshly scaffolded minimal project, committed.
    fn new() -> Self {
        let fx = Self {
            dir: tempfile::tempdir().unwrap(),
        };
        std::fs::create_dir_all(fx.root()).unwrap();
        git(&fx.root(), &["init", "-q", "-b", "main"]);
        let init = fx.codeflow(&["init", "--minimal", "--yes"]);
        assert!(init.status.success(), "{}", text(&init));
        git(&fx.root(), &["config", "core.hooksPath", "/dev/null"]);
        git(&fx.root(), &["add", "-A"]);
        git(
            &fx.root(),
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@example.com",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "chore: scaffold",
            ],
        );
        fx
    }

    fn root(&self) -> PathBuf {
        self.dir.path().join("p")
    }

    fn codeflow(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_codeflow"))
            .args(args)
            .current_dir(self.root())
            .env("CODEFLOW_HOME", self.dir.path().join("home"))
            .env(
                "CODEFLOW_RELEASE_URL",
                format!("file://{}", self.dir.path().join("releases").display()),
            )
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env_remove("GIT_DIR")
            .output()
            .unwrap()
    }

    /// Publish `version` locally: one archive per triple and `sha256.sum`.
    fn publish(&self, version: &str) -> PathBuf {
        let release = self.dir.path().join(format!("releases/v{version}"));
        std::fs::create_dir_all(&release).unwrap();
        let mut sums = String::new();
        for triple in TRIPLES {
            let asset = format!("codeflow-cli-{triple}.tar.xz");
            let bytes = format!("{triple} {version}\n").into_bytes();
            sums.push_str(&sha256_hex(&bytes));
            sums.push_str("  ");
            sums.push_str(&asset);
            sums.push('\n');
            std::fs::write(release.join(asset), bytes).unwrap();
        }
        std::fs::write(release.join("sha256.sum"), sums).unwrap();
        release
    }

    fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.root().join(rel)).unwrap()
    }

    fn changed(&self) -> String {
        git(&self.root(), &["status", "--porcelain"])
    }
}

/// AC-5: the pin writes only the version and the digests it checked, and
/// refuses a malformed version, a lowered pin, a missing release or a
/// mismatched archive without writing anything.
#[test]
fn update_pin_writes_only_the_version_and_the_checked_digests() {
    let fx = Fixture::new();
    let release = fx.publish("99.0.0");
    let before = fx.read(STATE);

    let pinned = fx.codeflow(&["update", "--pin", "99.0.0"]);
    assert!(pinned.status.success(), "{}", text(&pinned));
    assert_eq!(fx.changed(), format!(" M {STATE}\n"));
    let state = fx.read(STATE);
    assert!(state.contains("scaffold_version = \"99.0.0\""), "{state}");
    assert!(state.contains("[scaffold_sha256]\n"), "{state}");
    assert!(state.contains("version = \"99.0.0\""), "{state}");
    for triple in TRIPLES {
        let bytes = format!("{triple} 99.0.0\n").into_bytes();
        let line = format!("{triple} = \"{}\"", sha256_hex(&bytes));
        assert!(state.contains(&line), "{line}: {state}");
        assert!(
            text(&pinned).contains(&sha256_hex(&bytes)),
            "{}",
            text(&pinned)
        );
    }
    // Every other key of the state is kept.
    for line in before
        .lines()
        .filter(|l| !l.starts_with("scaffold_version"))
    {
        assert!(state.contains(line), "{line} was dropped: {state}");
    }
    git(&fx.root(), &["checkout", "-q", "--", STATE]);

    // A replaced archive with its old sha256.sum entry: refused, unwritten.
    let asset = release.join("codeflow-cli-x86_64-unknown-linux-gnu.tar.xz");
    std::fs::write(&asset, "replaced\n").unwrap();
    let swapped = fx.codeflow(&["update", "--pin", "99.0.0"]);
    assert_eq!(swapped.status.code(), Some(1), "{}", text(&swapped));
    assert!(
        text(&swapped).contains("does not match its sha256.sum entry"),
        "{}",
        text(&swapped)
    );
    assert_eq!(fx.changed(), "");

    for (version, said) in [
        ("v99", "is not a codeflow version"),
        ("0.0.1", "the pin only rises"),
        ("98.0.0", "has no published checksum file"),
    ] {
        let refused = fx.codeflow(&["update", "--pin", version]);
        assert_eq!(
            refused.status.code(),
            Some(1),
            "{version}: {}",
            text(&refused)
        );
        assert!(
            text(&refused).contains(said),
            "{version}: {}",
            text(&refused)
        );
        assert_eq!(fx.changed(), "", "{version}");
    }

    // The pin is its own step: it never runs with --force or --diff.
    let mixed = fx.codeflow(&["update", "--pin", "99.0.0", "--force"]);
    assert_eq!(mixed.status.code(), Some(2), "{}", text(&mixed));
    assert_eq!(fx.changed(), "");
}

/// A project line the installers refuse (here a key named outside printable
/// ASCII) stays as the project wrote it, so the pin refuses before writing
/// and names it, doctor names the same line, and once it is renamed the pin
/// writes a state the installers read.
#[test]
fn update_pin_refuses_a_state_the_installers_would_refuse() {
    let fx = Fixture::new();
    fx.publish("99.0.0");
    let state = fx.read(STATE);
    let commit = |message: &str| {
        git(&fx.root(), &["add", "-A"]);
        git(
            &fx.root(),
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@example.com",
                "commit",
                "-q",
                "-m",
                message,
            ],
        );
    };
    std::fs::write(
        fx.root().join(STATE),
        format!("{state}\n# a comment\n\n[notes]\n# about\n\"caf\u{e9}\" = \"kept\"\n"),
    )
    .unwrap();
    commit("chore: a note");
    // The line as it stands in the file, after comments and blank lines the
    // rewrite would drop.
    let line = format!("line {} (", state.lines().count() + 6);
    let refused = fx.codeflow(&["update", "--pin", "99.0.0"]);
    assert_eq!(refused.status.code(), Some(1), "{}", text(&refused));
    assert!(
        text(&refused).contains("a name with a character outside printable ASCII")
            && text(&refused).contains(&format!("{line}\"\\\"caf\u{e9}\\\" = \\\"kept\\\"\")"))
            && text(&refused).contains("nothing was written"),
        "{}",
        text(&refused)
    );
    assert_eq!(fx.changed(), "");
    let doctor = fx.codeflow(&["doctor", "--check", "ci-perimeter"]);
    assert!(
        text(&doctor).contains(&format!(
            "rewrite or remove line {} of",
            state.lines().count() + 6
        )),
        "{}",
        text(&doctor)
    );

    std::fs::write(
        fx.root().join(STATE),
        format!("{state}\n[notes]\ncafe = \"kept\"\n"),
    )
    .unwrap();
    commit("chore: an ASCII note");
    let pinned = fx.codeflow(&["update", "--pin", "99.0.0"]);
    assert!(pinned.status.success(), "{}", text(&pinned));
    assert!(matches!(
        pinned_digests(&fx.read(STATE)),
        PinnedDigests::Table { ref missing, .. } if missing.is_empty()
    ));
}

/// AC-2 and the AC-9 journey: `codeflow update` leaves the project setup
/// hook alone, and doctor reports the hook and the digest mode before and
/// after a pin of the version this build carries.
#[test]
fn update_keeps_the_setup_hook_and_doctor_reports_hook_and_digests() {
    let fx = Fixture::new();
    let hook = "#!/bin/sh\ncorepack enable\npnpm install --frozen-lockfile\n";
    std::fs::write(fx.root().join(HOOK), hook).unwrap();

    let updated = fx.codeflow(&["update"]);
    assert!(updated.status.success(), "{}", text(&updated));
    assert_eq!(fx.read(HOOK), hook);
    assert!(!text(&updated).contains("ci-setup"), "{}", text(&updated));

    let doctor = fx.codeflow(&["doctor", "--check", "ci-perimeter"]);
    let said = text(&doctor);
    assert!(
        said.contains(
            "project setup hook .codeflow/ci-setup.sh runs before `codeflow test` (first command `corepack enable`)"
        ),
        "{said}"
    );
    assert!(said.contains("sha256.sum only"), "{said}");

    let version = fx
        .read(STATE)
        .lines()
        .find_map(|l| l.strip_prefix("scaffold_version = \""))
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap()
        .to_string();
    fx.publish(&version);
    let pinned = fx.codeflow(&["update", "--pin", &version]);
    assert!(pinned.status.success(), "{}", text(&pinned));
    // Main still pins none, so doctor says the table applies once it lands.
    let doctor = fx.codeflow(&["doctor", "--check", "ci-perimeter"]);
    assert!(
        text(&doctor).contains(
            "this checkout changes the [scaffold_sha256] table, which CI checks once it lands on main (then: the release digests pinned in .codeflow/project.toml"
        ),
        "{}",
        text(&doctor)
    );

    // A later update keeps both the hook and the pinned digests.
    let again = fx.codeflow(&["update"]);
    assert!(again.status.success(), "{}", text(&again));
    assert_eq!(fx.read(HOOK), hook);
    assert!(
        fx.read(STATE).contains("[scaffold_sha256]"),
        "{}",
        fx.read(STATE)
    );
}
