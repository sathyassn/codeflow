//! Conformance test suite: binary-level contract verification.
//!
//! Runs both the Go (`codeflow-cli/bin/codeflow`) and Rust
//! (`codeflow-rs/target/debug/codeflow`) binaries against identical stdin
//! inputs and diffs the results (stdout, stderr, exit code).
//!
//! Run all conformance tests:
//!   cargo test --test conformance
//!   cargo nextest run --test conformance
//!
//! Both binaries must be built before running:
//!   cd codeflow-cli && make build          # Go binary
//!   cd codeflow-rs && cargo build          # Rust binary

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
