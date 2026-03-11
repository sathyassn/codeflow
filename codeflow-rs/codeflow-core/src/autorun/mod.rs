//! Autorun batch parsing and orchestration.
//!
//! Provides:
//! - YAML batch file parsing with validation ([`batch`])
//! - Worker DI traits for isolated execution ([`worker`])
//! - Orchestrator for dependency-aware concurrent execution ([`orchestrator`])

pub mod batch;
pub mod orchestrator;
pub mod worker;

pub use batch::{BatchFile, ParsedBatch, TaskSpec};
pub use orchestrator::{Orchestrator, WorkerConfig, WorkerResult};
pub use worker::{ClaudeInvoker, InvokeConfig, InvokeResult, TmuxRunner, WorkerRunner};
