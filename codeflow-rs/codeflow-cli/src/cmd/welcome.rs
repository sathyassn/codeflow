//! Welcome command: display welcome message.

use anyhow::Result;

#[allow(clippy::unnecessary_wraps)]
pub fn run() -> Result<()> {
    println!("Welcome to CodeFlow {}", env!("CARGO_PKG_VERSION"));
    println!();
    println!("AI-native development framework for structured, traceable work.");
    println!();
    println!("Run `codeflow --help` for available commands.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_welcome_command_exists() {
        let _: fn() -> Result<()> = run;
    }

    #[test]
    fn test_welcome_succeeds() {
        assert!(run().is_ok());
    }
}
