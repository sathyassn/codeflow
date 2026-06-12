//! `codeflow doctor` — run the health checks and render the results.

use clap::Args;
use codeflow_core::doctor::{self, Options, Status};

#[derive(Args)]
pub struct DoctorArgs {
    /// Run a single named check (see `doctor --list`).
    #[arg(long)]
    pub check: Option<String>,
    /// List available check names.
    #[arg(long)]
    pub list: bool,
}

/// Run doctor checks; exit 0 when healthy (warnings allowed), 1 on failures.
pub fn run(args: &DoctorArgs) -> i32 {
    if args.list {
        for name in doctor::check_names() {
            println!("{name}");
        }
        return 0;
    }
    let opts = Options {
        project_dir: super::repo_root().to_string_lossy().into_owned(),
        ..Options::default()
    };
    let results = if let Some(name) = &args.check {
        match doctor::run_check(name, &opts) {
            Ok(r) => vec![r],
            Err(e) => {
                eprintln!("codeflow doctor: {e}");
                return 1;
            }
        }
    } else {
        let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
            Ok(rt) => rt,
            Err(e) => {
                eprintln!("codeflow doctor: runtime: {e}");
                return 1;
            }
        };
        rt.block_on(doctor::run_all(&opts))
    };
    let mut failed = false;
    for r in &results {
        let badge = match r.status {
            Status::Pass => "ok  ",
            Status::Warn => "warn",
            Status::Fail => {
                failed = true;
                "FAIL"
            }
        };
        println!("{badge}  {name}: {message}", name = r.name, message = r.message);
    }
    i32::from(failed)
}
