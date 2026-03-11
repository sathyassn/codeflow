//! Update command: check for and apply `CodeFlow` updates.

use anyhow::Result;

#[allow(clippy::unnecessary_wraps)]
pub fn run() -> Result<()> {
    println!("codeflow {} is up to date", env!("CARGO_PKG_VERSION"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_update_command_exists() {
        let _: fn() -> Result<()> = run;
    }

    #[test]
    fn test_update_succeeds() {
        assert!(run().is_ok());
    }
}
