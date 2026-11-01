//! `codeflow status [--capabilities] [--delivery]` — generated views, never
//! stored — and the progress views (`--task`, `--epic`, `--line`, `--json`)
//! that give percent complete, location, order, lanes, windows, issues and
//! pace for a scope (TSK-1095). Status writes nothing and never fetches.

use std::path::PathBuf;

use clap::Args;
use codeflow_core::progress::{self, Options, ScopeSpec};
use codeflow_core::status::{collect_status, render_delivery, render_status};

/// The environment variable that holds the snapshot time fixed
/// (`YYYY-MM-DDTHH:MM:SSZ`), for reproducible output.
const AS_OF_ENV: &str = "CODEFLOW_STATUS_AS_OF";

#[derive(Args)]
#[allow(clippy::struct_excessive_bools)]
pub struct StatusArgs {
    /// Show the full capability table (default: counts by status)
    #[arg(long)]
    pub capabilities: bool,

    /// Show the capability-delivery rollup: each capability's epics with their
    /// open/total task counts and next actionable tasks
    #[arg(long)]
    pub delivery: bool,

    /// Progress of one task
    #[arg(long, value_name = "TSK-NNN", conflicts_with_all = ["epic", "line"])]
    pub task: Option<String>,

    /// Progress of an epic; repeat for several
    #[arg(long, value_name = "EPC-NNN", conflicts_with = "line")]
    pub epic: Vec<String>,

    /// Progress of every task whose home is this integration line
    #[arg(long, value_name = "BRANCH")]
    pub line: Option<String>,

    /// Print the progress facts as versioned JSON (report-v1)
    #[arg(long)]
    pub json: bool,

    /// Open the progress report in cf-present (TSK-1098)
    #[arg(long)]
    pub present: bool,

    /// Print the progress report as Markdown (TSK-1098)
    #[arg(long)]
    pub md: bool,

    /// Merge a session facts file (session-v1) into the lanes view
    #[arg(long, value_name = "FILE")]
    pub session: Option<PathBuf>,

    /// Report what changed since a previous `--json` snapshot
    #[arg(long, value_name = "FILE")]
    pub since: Option<PathBuf>,
}

impl StatusArgs {
    fn progress(&self) -> bool {
        self.task.is_some()
            || !self.epic.is_empty()
            || self.line.is_some()
            || self.json
            || self.present
            || self.md
            || self.session.is_some()
            || self.since.is_some()
    }

    fn scope(&self) -> ScopeSpec {
        if let Some(task) = &self.task {
            ScopeSpec::Task(task.clone())
        } else if !self.epic.is_empty() {
            ScopeSpec::Epics(self.epic.clone())
        } else if let Some(line) = &self.line {
            ScopeSpec::Line(line.clone())
        } else {
            ScopeSpec::Default
        }
    }
}

pub fn run(args: &StatusArgs) -> i32 {
    let root = match super::repo_root() {
        Ok(root) => root,
        Err(error) => {
            eprintln!("codeflow: {error}", error = crate::cmd::shown(&error));
            return 2;
        }
    };
    if args.progress() {
        return run_progress(&root, args);
    }
    let view = collect_status(&root);
    print_block!(render_status(&view, args.capabilities));
    if args.delivery {
        print_block!(render_delivery(&view));
    }
    0
}

fn run_progress(root: &std::path::Path, args: &StatusArgs) -> i32 {
    let outputs: Vec<&str> = [
        ("--json", args.json),
        ("--present", args.present),
        ("--md", args.md),
    ]
    .iter()
    .filter(|(_, on)| *on)
    .map(|(name, _)| *name)
    .collect();
    if outputs.len() > 1 {
        eprintln!(
            "codeflow status: {} cannot be used together; choose one output",
            outputs.join(" and ")
        );
        return 2;
    }
    if args.present || args.md {
        eprintln!(
            "codeflow status: {} is not available in this build; the report views arrive with TSK-1098. Use --json or the text view.",
            outputs[0]
        );
        return 2;
    }
    let mut options = Options::new(args.scope());
    options.session.clone_from(&args.session);
    if let Ok(text) = std::env::var(AS_OF_ENV) {
        let Ok(Ok(at)) = codeflow_core::coord::parse_time(&text).map(i64::try_from) else {
            eprintln!("codeflow status: {AS_OF_ENV} `{text}` is not a UTC time of the form YYYY-MM-DDTHH:MM:SSZ");
            return 2;
        };
        options.now = Some(at);
    }
    let previous = match args.since.as_deref().map(progress::since::read).transpose() {
        Ok(previous) => previous,
        Err(error) => {
            eprintln!("codeflow status: {error}");
            return 2;
        }
    };
    let inputs = match progress::collect(root, &options) {
        Ok(inputs) => inputs,
        Err(error) => {
            eprintln!("codeflow status: {error}");
            return 2;
        }
    };
    let facts = progress::facts(&inputs);
    let value = match progress::to_json(&facts, previous.as_ref()) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("codeflow status: {error}");
            return 2;
        }
    };
    if args.json {
        match serde_json::to_string_pretty(&value) {
            Ok(text) => println!("{text}"),
            Err(error) => {
                eprintln!("codeflow status: {error}");
                return 2;
            }
        }
    } else {
        print!("{}", progress::text::render(&facts));
        if let Some(changes) = value
            .pointer("/since/changes")
            .and_then(serde_json::Value::as_array)
        {
            println!(
                "\nSince {}:",
                value
                    .pointer("/since/previous_at")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("the previous snapshot")
            );
            for change in changes {
                println!(
                    "  {} {}",
                    change
                        .get("change")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or(""),
                    change
                        .get("subject")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("")
                );
            }
        }
    }
    0
}
