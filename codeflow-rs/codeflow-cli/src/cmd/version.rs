//! Version command: display CLI version.

use anyhow::Result;

#[allow(clippy::unnecessary_wraps)]
pub fn run() -> Result<()> {
    println!("codeflow {}", env!("CARGO_PKG_VERSION"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_succeeds() {
        assert!(run().is_ok());
    }
}
