//! Uninstall command: remove `CodeFlow` from the system.

use anyhow::Result;

#[allow(clippy::unnecessary_wraps)]
pub fn run() -> Result<()> {
    println!("To uninstall CodeFlow:");
    println!("  1. Remove the codeflow binary from your PATH");
    println!("  2. Remove .state/ and .codeflow/ directories from your project");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uninstall_command_exists() {
        let _: fn() -> Result<()> = run;
    }

    #[test]
    fn test_uninstall_succeeds() {
        assert!(run().is_ok());
    }
}
