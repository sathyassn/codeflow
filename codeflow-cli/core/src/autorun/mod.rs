//! Autorun batch parsing and orchestration.
//!
//! Provides:
//! - YAML batch file parsing with validation ([`batch`])
//! - Worker DI traits for isolated execution ([`worker`])
//! - Orchestrator for dependency-aware concurrent execution ([`orchestrator`])

pub mod batch;
pub mod config;
pub mod orchestrator;
pub mod worker;

pub use batch::{BatchFile, ParsedBatch, TaskSpec};
pub use config::{AutorunConfig, ParallelWorkConfig, load_config};
pub use orchestrator::{Orchestrator, WorkerConfig, WorkerResult};
pub use worker::{
    ClaudeInvoker, InvokeConfig, InvokeResult, RealWorktreeProvider, TaskMetadata, TmuxRunner,
    TmuxWorker, WorkerRunner, WorktreeInfo, WorktreeProvider, build_task_prompt,
    build_task_prompt_from_file, parse_task_markdown,
};
