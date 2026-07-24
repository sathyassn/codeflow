//! `codeflow doctor` — run the health checks and render the results.

use std::io::Write;

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
    let opts = Options {
        project_dir: super::repo_root().to_string_lossy().into_owned(),
        ..Options::default()
    };
    run_with(args, &opts, &mut std::io::stdout())
}

/// The testable core of [`run`]: explicit options and output sink, no
/// process-global state.
fn run_with(args: &DoctorArgs, opts: &Options, out: &mut dyn Write) -> i32 {
    if args.list {
        for name in doctor::check_names() {
            let _ = writeln!(out, "{name}");
        }
        return 0;
    }
    let results = if let Some(name) = &args.check {
        match doctor::run_check(name, opts) {
            Ok(r) => vec![r],
            Err(e) => {
                eprintln!("codeflow doctor: {e}");
                return 1;
            }
        }
    } else {
        let rt = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                eprintln!("codeflow doctor: runtime: {e}");
                return 1;
            }
        };
        rt.block_on(doctor::run_all(opts))
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
        let _ = writeln!(
            out,
            "{badge}  {name}: {message}",
            name = r.name,
            message = r.message
        );
    }
    i32::from(failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(check: Option<&str>, list: bool) -> DoctorArgs {
        DoctorArgs {
            check: check.map(ToString::to_string),
            list,
        }
    }

    fn run_to_string(a: &DoctorArgs, opts: &Options) -> (i32, String) {
        let mut buf = Vec::new();
        let code = run_with(a, opts, &mut buf);
        (code, String::from_utf8(buf).unwrap())
    }

    /// A tempdir with a valid initialized `.codeflow/` directory.
    fn initialized_project() -> (tempfile::TempDir, Options) {
        let dir = tempfile::tempdir().unwrap();
        let cf = dir.path().join(".codeflow");
        std::fs::create_dir_all(&cf).unwrap();
        std::fs::write(cf.join("policy.json"), "{\"schema_version\": 1}\n").unwrap();
        std::fs::write(cf.join("manifest.json"), "{\"files\": {}}\n").unwrap();
        std::fs::write(cf.join("project.toml"), "tier = \"standard\"\n").unwrap();
        let opts = Options {
            project_dir: dir.path().to_string_lossy().into_owned(),
            look_path: Some(|name| Ok(format!("/stub/bin/{name}"))),
            exec_command: Some(|_, _| Ok(String::new())),
            exec_command_stdin: Some(|_, _, _| Ok(String::new())),
        };
        (dir, opts)
    }

    #[test]
    fn list_prints_every_check_name_and_exits_zero() {
        let (code, out) = run_to_string(&args(None, true), &Options::default());
        assert_eq!(code, 0);
        let listed: Vec<&str> = out.lines().collect();
        assert_eq!(listed, doctor::check_names());
    }

    #[test]
    fn check_config_passes_on_initialized_tempdir() {
        let (_dir, opts) = initialized_project();
        let (code, out) = run_to_string(&args(Some("config"), false), &opts);
        assert_eq!(code, 0, "valid config must be healthy: {out}");
        assert!(out.starts_with("ok  "), "got: {out}");
        assert!(out.contains("config:"), "got: {out}");
        assert_eq!(out.lines().count(), 1, "single check renders one line");
    }

    #[test]
    fn check_config_fails_on_invalid_json() {
        let (dir, opts) = initialized_project();
        std::fs::write(dir.path().join(".codeflow/manifest.json"), "{broken").unwrap();
        let (code, out) = run_to_string(&args(Some("config"), false), &opts);
        assert_eq!(code, 1);
        assert!(out.contains("FAIL"), "got: {out}");
        assert!(out.contains("manifest.json"), "names the offender: {out}");
    }

    #[test]
    fn check_unknown_name_exits_one() {
        let (code, out) = run_to_string(&args(Some("bogus"), false), &Options::default());
        assert_eq!(code, 1);
        assert!(out.is_empty(), "errors go to stderr, not the report");
    }

    #[test]
    fn full_run_healthy_renders_all_checks_and_exits_zero() {
        let (_dir, opts) = initialized_project();
        let (code, out) = run_to_string(&args(None, false), &opts);
        assert_eq!(code, 0, "all stubbed checks pass: {out}");
        for name in doctor::check_names() {
            assert!(
                out.contains(&format!("{name}:")),
                "{name} must render: {out}"
            );
        }
        assert!(
            out.contains("config: all .codeflow/ JSON files are valid"),
            "got: {out}"
        );
        assert!(
            out.contains("permissions: file permissions correct"),
            "got: {out}"
        );
    }

    #[test]
    fn full_run_with_failing_hooks_exits_one() {
        let (_dir, mut opts) = initialized_project();
        // Binary found but every hook subcommand probe fails.
        opts.exec_command = Some(|_, _| Err("boom".into()));
        let (code, out) = run_to_string(&args(None, false), &opts);
        assert_eq!(code, 1);
        assert!(out.contains("FAIL  hooks:"), "got: {out}");
        // Failure of one check never hides the others.
        assert!(out.contains("config:"), "got: {out}");
        assert!(out.contains("permissions:"), "got: {out}");
    }
}
