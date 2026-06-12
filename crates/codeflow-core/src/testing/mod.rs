//! Generic, stack-agnostic testing engine.
//!
//! Drives `codeflow test`: declarative targets from `.codeflow/test-config.json`,
//! per-target shell runners, JUnit/CTRF report parsing, coverage parsing
//! (lcov, cobertura, istanbul, go-cover), threshold evaluation with exceptions,
//! structural source-to-test mapping checks, PR-body rendering, and doctor checks.

pub mod config;
pub mod coverage;
pub mod doctor;
pub mod error;
pub mod pr_body;
pub mod report;
pub mod runner;
pub mod setup;
pub mod structural;
pub mod threshold;
pub mod validation;
