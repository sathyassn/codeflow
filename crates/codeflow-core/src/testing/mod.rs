//! Generic, stack-agnostic testing engine.
//!
//! Drives `codeflow test`: declarative targets from `.codeflow/test-config.json`,
//! per-target shell runners, JUnit/CTRF report parsing, coverage parsing
//! (lcov, cobertura, istanbul, go-cover), threshold evaluation with exceptions,
//! and doctor checks.
//!
//! The structural source-to-test mapping validator lives behind the
//! `structural-check` feature — its config surface (a target's `structural`
//! block) ships and is populated by stack detection, but the validator is not
//! yet wired into the gate.

pub mod config;
pub mod coverage;
pub mod doctor;
pub mod error;
pub mod gate;
pub mod gate_guard;
pub mod report;
pub mod runner;
pub mod setup;
#[cfg(feature = "structural-check")]
pub mod structural;
pub mod threshold;
pub mod validation;
