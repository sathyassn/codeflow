//! Version command: display CLI version with build metadata.

use anyhow::Result;

#[allow(clippy::unnecessary_wraps)]
pub fn run() -> Result<()> {
    println!(
        "codeflow {} ({} {})",
        env!("CARGO_PKG_VERSION"),
        env!("GIT_COMMIT"),
        env!("BUILD_TIMESTAMP"),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_succeeds() {
        assert!(run().is_ok());
    }

    #[test]
    fn test_git_commit_env_set() {
        let commit = env!("GIT_COMMIT");
        assert!(!commit.is_empty(), "GIT_COMMIT should not be empty");
    }

    #[test]
    fn test_build_timestamp_env_set() {
        let ts = env!("BUILD_TIMESTAMP");
        assert!(!ts.is_empty(), "BUILD_TIMESTAMP should not be empty");
    }
}
