//! Build script: inject git commit hash and build timestamp into the binary.

use std::process::Command;

fn main() {
    // Git commit hash (short).
    let commit = git_short_commit().unwrap_or_else(|| "unknown".to_string());

    // Dirty working tree indicator.
    let dirty = if is_dirty() { "-dirty" } else { "" };

    // UTC build timestamp.
    let timestamp = build_timestamp();

    println!("cargo:rustc-env=GIT_COMMIT={commit}{dirty}");
    println!("cargo:rustc-env=BUILD_TIMESTAMP={timestamp}");

    // Re-run if git HEAD changes (new commit, branch switch).
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/index");
}

fn git_short_commit() -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let hash = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if hash.is_empty() { None } else { Some(hash) }
}

fn is_dirty() -> bool {
    Command::new("git")
        .args(["diff", "--quiet"])
        .status()
        .map(|s| !s.success())
        .unwrap_or(false)
}

fn build_timestamp() -> String {
    // Try git log timestamp first (reproducible), fall back to current time.
    let output = Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout).trim().to_string()
        }
        _ => "unknown".to_string(),
    }
}
