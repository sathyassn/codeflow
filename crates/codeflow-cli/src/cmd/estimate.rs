//! Read-only forecast checking and outcome reports; no adoption, record or
//! registry mutation.

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
    /// Derive completed tasks' timings from git and compare them with frozen forecasts.
    Outcomes {
        /// Only tasks completed or cancelled on this UTC date or later (`YYYY-MM-DD`).
        #[arg(long, value_name = "DATE")]
        since: Option<String>,
        /// Only tasks of this epic.
        #[arg(long, value_name = "EPC-NNN")]
        epic: Option<String>,
        /// Join this forecast instead of the adopted home's frozen forecasts.
        #[arg(long, value_name = "PATH")]
        forecast: Option<PathBuf>,
        /// Ratios a group needs before its median is a verdict.
        #[arg(long, default_value_t = 3, value_name = "N")]
        minimum: usize,
        /// A median ratio below this contradicts the forecast.
        #[arg(long, default_value_t = 0.5, value_name = "RATIO")]
        low: f64,
        /// A median ratio above this contradicts the forecast.
        #[arg(long, default_value_t = 2.0, value_name = "RATIO")]
        high: f64,
        /// Emit the versioned JSON report.
        #[arg(long)]
        json: bool,
    },
}

#[must_use]
pub fn run(args: &EstimateArgs) -> i32 {
    match &args.command {
        EstimateCommand::Check {
            forecast_path,
            json,
        } => check(forecast_path, *json),
        EstimateCommand::Outcomes {
            since,
            epic,
            forecast,
            minimum,
            low,
            high,
            json,
        } => {
            if let Some(since) = since {
                if !is_date(since) {
                    eprintln!("estimate outcomes: `{since}` is not a date written YYYY-MM-DD");
                    return 2;
                }
            }
            if *minimum == 0 {
                eprintln!("estimate outcomes: --minimum must be at least 1");
                return 2;
            }
            if !(low.is_finite() && high.is_finite() && *low >= 0.0 && low < high) {
                eprintln!(
                    "estimate outcomes: --low and --high must be finite, --low at least 0 and below --high"
                );
                return 2;
            }
            let options = codeflow_core::estimate::outcomes::Options {
                since: since.clone(),
                epic: epic.clone(),
                forecast: forecast.clone(),
                minimum: *minimum,
                low: *low,
                high: *high,
            };
            outcomes(&options, *json)
        }
    }
}

fn is_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 10
        && bytes.iter().enumerate().all(|(i, b)| {
            if i == 4 || i == 7 {
                *b == b'-'
            } else {
                b.is_ascii_digit()
            }
        })
        && matches!(text[5..7].parse::<u8>(), Ok(1..=12))
        && matches!(text[8..10].parse::<u8>(), Ok(1..=31))
}

fn outcomes(options: &codeflow_core::estimate::outcomes::Options, json: bool) -> i32 {
    let root = super::repo_root();
    let cwd = std::env::current_dir().unwrap_or_else(|_| root.clone());
    let report = codeflow_core::estimate::outcomes::outcomes(&root, &cwd, options);
    if json {
        let Ok(text) = serde_json::to_string_pretty(&report) else {
            eprintln!("estimate outcomes: report serialization failed");
            return 1;
        };
        println!("{text}");
    } else {
        print!("{}", report.render());
    }
    i32::from(!report.is_clean())
}

fn check(forecast_path: &std::path::Path, json: bool) -> i32 {
    let root = super::repo_root();
    let input = if forecast_path.is_absolute() {
        forecast_path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| root.clone())
            .join(forecast_path)
    };
    let report = codeflow_core::estimate::check_forecast(&root, &input);
    if json {
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
