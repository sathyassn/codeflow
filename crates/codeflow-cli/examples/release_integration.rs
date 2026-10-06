//! CodeFlow-only release integration. No adopter scaffold installs this runner.
//! A disposable clone holds prospective merges; the destination is updated once,
//! by a normal fast-forward push, only after every combined check succeeds.
use std::fmt::Write as _;
use std::path::Path;

use clap::Parser;
use codeflow_core::reading::{self, Inventory, SkillFiles};
use codeflow_core::workgraph::lifecycle::Graph;
use codeflow_core::workgraph::{check_epic_line, release_line, RecordKind};

#[derive(Parser)]
struct Args {
    /// Release branch configured by the repository's integration workflow.
    #[arg(long)]
    release: Option<String>,
    /// Narrow a local reproduction to one line; workflows omit this to catch up all lines.
    #[arg(long)]
    line: Option<String>,
    /// Publish the checked result. Without this flag, only reproduce checks.
    #[arg(long)]
    push: bool,
}

fn main() {
    let args = Args::parse();
    match integrate(
        Path::new("."),
        args.release.as_deref(),
        args.line.as_deref(),
        args.push,
        Path::new("codeflow"),
    ) {
        Ok(report) => print!("{report}"),
        Err(report) => {
            eprintln!("{report}");
            std::process::exit(1);
        }
    }
}

fn command(root: &Path, program: &Path, args: &[&str]) -> Result<String, String> {
    // Every git spawn goes through codeflow_core's constructor (TSK-141 AC-4).
    let output = codeflow_core::git::process(program)
        .args(args)
        .current_dir(root)
        // Do not inherit a caller's index, checkout, or pull-request context.
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("CODEFLOW_PR_BODY")
        .env_remove("GITHUB_HEAD_REF")
        .env_remove("GITHUB_BASE_REF")
        .env_remove("GITHUB_EVENT_NAME")
        .output()
        .map_err(|error| format!("{}: {error}", program.display()))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if output.status.success() {
        Ok(text)
    } else {
        Err(format!(
            "{} {} failed:\n{text}",
            program.display(),
            args.join(" ")
        ))
    }
}

fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    command(root, Path::new("git"), args)
        .map(|text| text.strip_suffix('\n').unwrap_or(&text).to_string())
}

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Run the same entry point locally and in the workflow. It never edits the
/// caller's checkout; a failed check leaves every remote branch unchanged.
///
/// # Errors
/// Returns the failed check, release owner and local reproduction command.
pub fn integrate(
    root: &Path,
    release: Option<&str>,
    line: Option<&str>,
    push: bool,
    codeflow: &Path,
) -> Result<String, String> {
    let configured =
        std::fs::read_to_string(root.join(".github/workflows/codeflow-release-integration.yml"))
            .is_ok_and(|workflow| workflow.contains("\n  release-integration:"));
    let Some(release) = release.filter(|_| configured) else {
        return Ok("no release integration configured\n".into());
    };
    let mut owner = release_line::NO_OWNER.to_string();
    let result = prepare(root, release, line, push, codeflow, &mut owner);
    result.map_err(|error| {
        let line_arg = line.map_or_else(String::new, |line| format!(" --line {}", quote(line)));
        format!(
            "{error}\nOwner: {owner}\nReproduce without pushing, from the repository checkout:\n\
             git fetch origin\n\
             cargo run -p codeflow-cli --example release_integration -- --release {}{line_arg}\n\
             Resolve the finding within the release pull request under R-120; automation never resolves conflicts.\n",
            quote(release)
        )
    })
}

fn prepare(
    root: &Path,
    release: &str,
    line: Option<&str>,
    push: bool,
    codeflow: &Path,
    owner: &mut String,
) -> Result<String, String> {
    git(
        root,
        &["check-ref-format", &format!("refs/heads/{release}")],
    )?;
    let remote = git(root, &["remote", "get-url", "origin"])?;
    let temporary = tempfile::tempdir().map_err(|error| error.to_string())?;
    let clone = temporary.path().join("release");
    git(
        root,
        &[
            "clone",
            "--quiet",
            "--no-checkout",
            "--",
            &remote,
            clone.to_str().ok_or("non-UTF-8 clone path")?,
        ],
    )?;
    let destination = release_line::ask_destination(&clone, Some(&remote))?;
    if !release_line::scope(&clone, &destination, release, None)?.head {
        return Err(format!(
            "{release} does not match the default target's release pattern"
        ));
    }
    let (default, default_tip) = destination
        .default
        .as_ref()
        .ok_or("destination has no default target")?;
    let before = destination
        .heads
        .iter()
        .find(|(name, _)| name == release)
        .map(|(_, oid)| oid.to_string())
        .ok_or("configured release branch does not exist")?;
    // The release may predate its owner assignment. Read the destination's
    // default target before any attempted merge can conflict.
    git(
        &clone,
        &["checkout", "--quiet", "--detach", &default_tip.to_string()],
    )?;
    let holders = release_owners(&clone)?;
    if !holders.is_empty() {
        *owner = holders.join(", ");
    }
    if holders.len() > 1 {
        return Err("more than one open release-integration task".into());
    }
    git(&clone, &["checkout", "--quiet", "--detach", &before])?;
    let mut report = format!("Release: {release} at {before}\nOwner: {owner}\n");
    let mut changed = false;
    if let Some(line) = line {
        if !destination.heads.iter().any(|(name, _)| name == line) {
            return Err(format!(
                "landed line {line} does not exist at the destination"
            ));
        }
    }
    for (name, tip) in &destination.heads {
        if line.is_some_and(|line| line != name) || !name.starts_with("integration/EPC-") {
            continue;
        }
        let tip = tip.to_string();
        if let Err(error) = check_epic_line(&clone, name, default, &default_tip.to_string(), &tip) {
            if line.is_some() {
                return Err(error);
            }
            let _ = writeln!(report, "Skip unverified line {name}: {error}");
            continue;
        }
        if git(&clone, &["merge-base", "--is-ancestor", &tip, "HEAD"]).is_ok() {
            let _ = writeln!(report, "Already integrated: {name} at {tip}");
            continue;
        }
        report.push_str(&merge_and_check(
            &clone,
            release,
            &default_tip.to_string(),
            &remote,
            name,
            &tip,
            codeflow,
        )?);
        changed = true;
    }
    if line.is_some_and(|line| !line.starts_with("integration/EPC-")) {
        return Err("the landing is not an epic line".into());
    }
    if changed && push {
        // A normal push cannot overwrite a release branch advanced by another
        // actor. Workflow concurrency serializes our runs; a race fails here.
        report.push_str(&git(
            &clone,
            &["push", "origin", &format!("HEAD:refs/heads/{release}")],
        )?);
        report.push_str("\nPushed checked release integration.\n");
    } else if changed {
        report.push_str("Checks passed; no push requested.\n");
    }
    Ok(report)
}

fn release_owners(root: &Path) -> Result<Vec<String>, String> {
    Ok(Graph::from_worktree(root)?
        .records
        .values()
        .filter(|record| {
            record.kind == RecordKind::Task
                && record.role.as_deref() == Some(release_line::RELEASE_ROLE)
                && !matches!(record.status.as_str(), "complete" | "cancelled")
        })
        .map(|record| record.id.clone())
        .collect())
}

fn merge_and_check(
    root: &Path,
    release: &str,
    base: &str,
    remote: &str,
    line: &str,
    tip: &str,
    codeflow: &Path,
) -> Result<String, String> {
    git(
        root,
        &[
            "-c",
            "user.name=CodeFlow release integration",
            "-c",
            "user.email=codeflow-release@users.noreply.github.com",
            "merge",
            "--no-ff",
            "--no-edit",
            "-m",
            &format!("merge: integrate {line}"),
            tip,
        ],
    )?;
    let mut report = format!("Combined {line} at {tip}\n");
    report.push_str(&command(
        root,
        codeflow,
        &[
            "ci",
            "--base",
            base,
            "--head",
            "HEAD",
            "--branch",
            release,
            "--destination",
            remote,
        ],
    )?);
    check_reading(root)?;
    Ok(report)
}

fn check_reading(root: &Path) -> Result<(), String> {
    for trees in [
        vec!["assets/base/agents/skills", "assets/base/claude/skills"],
        vec![".agents/skills"],
        vec![".claude/skills"],
    ] {
        let mut files = SkillFiles::new();
        for tree in trees {
            reading::load_skill_tree(&root.join(tree), &mut files)
                .map_err(|error| error.to_string())?;
        }
        if files.is_empty() {
            continue;
        }
        let faults = reading::structure_faults(&files, &Inventory::SHIPPED);
        if !faults.is_empty() {
            return Err(format!("reading-check finding:\n{}", faults.join("\n")));
        }
    }
    Ok(())
}
