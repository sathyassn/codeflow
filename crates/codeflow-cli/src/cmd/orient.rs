//! `codeflow orient` — print the session-start digest directly
//! (charter §3.1/§3.4; the same text the `session-orient` hook injects).

use codeflow_core::hooks::orient;

/// Print the digest; returns the process exit code.
#[must_use]
pub fn run() -> i32 {
    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    print!(
        "{}",
        orient::generate(&match super::project_root(&cwd) {
            Ok(root) => root,
            Err(error) => {
                eprintln!("codeflow: cannot read project root: {error}");
                return 2;
            }
        })
    );
    0
}
