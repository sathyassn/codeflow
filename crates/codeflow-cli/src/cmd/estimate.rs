//! Read-only forecast checking; no adoption or registry mutation.

use clap::{Args, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Args)]
pub struct EstimateArgs {
    #[command(subcommand)]
    pub command: EstimateCommand,
}

#[derive(Debug, Subcommand)]
pub enum EstimateCommand {
    /// Check explicit allocations and pinned evidence without scheduling or writes.
    Check {
        /// Forecast JSON file (relative to the current directory or absolute).
        forecast_path: PathBuf,
        /// Emit the versioned JSON report.
        #[arg(long)]
        json: bool,
    },
}

#[must_use]
pub fn run(args: &EstimateArgs) -> i32 {
    let EstimateCommand::Check {
        forecast_path,
        json,
    } = &args.command;
    let root = super::repo_root();
    let input = if forecast_path.is_absolute() {
        forecast_path.clone()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| root.clone())
            .join(forecast_path)
    };
    let report = codeflow_core::estimate::check_forecast(&root, &input);
    if *json {
        let Ok(text) = serde_json::to_string_pretty(&report) else {
            eprintln!("estimate check: report serialization failed");
            return 1;
        };
        println!("{text}");
    } else {
        for finding in &report.findings {
            println!(
                "{} at {}: {}",
                finding.code, finding.location, finding.message
            );
        }
        println!(
            "estimate check: {} ({} packages)",
            if report.is_valid() {
                "valid supplied allocation"
            } else {
                "findings"
            },
            report.checked_package_count
        );
        for scenario in &report.scenarios {
            if let Some(elapsed) = scenario.elapsed_seconds {
                println!("  {:?}: {elapsed} elapsed seconds", scenario.name);
                for resource in &scenario.resources {
                    println!(
                        "    {}: {} resource-seconds",
                        resource.resource_id, resource.consumption_seconds
                    );
                }
            }
        }
        println!("{}", report.limitation);
    }
    i32::from(!report.is_valid())
}
