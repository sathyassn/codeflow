//! Contract test suite: binary-level hook handler verification (Rust-only, post-cutover).
//!
//! Runs the Rust binary (`codeflow-cli/target/debug/codeflow`) against
//! fixture inputs and verifies exit codes, stdout, and graceful degradation.
//!
//! Run all contract tests:
//!   cargo test --test conformance
//!   cargo nextest run --test conformance
//!
//! The binary must be built before running:
//!   cd codeflow-cli && cargo build

/// Shared harness: binary path resolution, run helpers, assert utilities.
#[path = "conformance/harness.rs"]
mod harness;

/// Hook contract tests: all 21 handlers across 7 event groups.
#[path = "conformance/hooks.rs"]
mod hooks;

/// CLI command contract tests: version, validate, and key subcommands.
#[path = "conformance/commands.rs"]
mod commands;

/// Exit code contract tests: exit 0 (allow), exit 2 (block) paths.
#[path = "conformance/exit_codes.rs"]
mod exit_codes;

/// File I/O contract tests: logging handlers write to filesystem.
#[path = "conformance/file_io.rs"]
mod file_io;
