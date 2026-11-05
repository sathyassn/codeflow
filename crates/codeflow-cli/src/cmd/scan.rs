//! `codeflow scan`: the hosted secret scan and dependency audit verdicts,
//! run locally over `base..head` (TSK-1176 AC-6 to AC-8, moved from
//! TSK-430).
//!
//! The base is the trusted commit: the levels (`git.secret_scan`,
//! `git.security_review`, `git.dep_audit`), the gitleaks exemptions and
//! configuration, `.gitattributes`, the `osv-scanner.toml` suppressions and
//! the project exception hook all come from it, never from the working tree
//! or the range, as the managed jobs read them. Each scan follows its level:
//! `block` fails the command on a finding, `warn` reports it, `off` skips the
//! scan. At `block` a pinned binary that is missing or mismatched fails
//! closed; at `warn` it is reported and the scan is skipped.
//!
//! Exit codes: 0 when nothing blocks, 1 when a finding or an unusable scan
//! blocks, 2 when the arguments or the policy cannot be read.

use std::path::Path;

use clap::Args;
use codeflow_core::git::display_text;
use codeflow_core::hooks::gitleaks::{self, range};
use codeflow_core::hooks::osv;

/// Arguments for `codeflow scan`.
#[derive(Debug, Args)]
pub struct ScanArgs {
    /// The trusted base: a pull request's target, such as `origin/main`.
    #[arg(long, value_name = "REF")]
    pub base: String,
    /// The head of the range.
    #[arg(long, value_name = "REF", default_value = "HEAD")]
    pub head: String,
    /// Run only one scan.
    #[arg(long, value_parser = ["secrets", "deps"])]
    pub only: Option<String>,
}

/// The base's level for a scan key, `block` when the key is absent.
fn secret_level(root: &Path, base: &str) -> Result<String, String> {
    let Some(policy) = range::blob(root, base, ".codeflow/policy.json")? else {
        return Ok("block".to_string());
    };
    let data: serde_json::Value = serde_json::from_slice(&policy)
        .map_err(|error| format!("cannot read .codeflow/policy.json at the base: {error}"))?;
    match data.get("git").and_then(|git| git.get("secret_scan")) {
        None => Ok("block".to_string()),
        Some(serde_json::Value::String(level)) => match level.as_str() {
            "block" | "warn" => Ok(level.clone()),
            // `allow` turns the commit-time scan off, as `off` does.
            "off" | "allow" => Ok("off".to_string()),
            _ => Err(format!(
                "git.secret_scan at the base is '{}', not off, warn, allow or block",
                display_text(level)
            )),
        },
        Some(_) => Err("git.secret_scan at the base is not a level".to_string()),
    }
}

/// One scan's result: whether it blocks.
fn verdict(scan: &str, level: &str, failed: bool, message: &str) -> bool {
    if !failed {
        println!("codeflow scan: {scan}: {message}");
        return false;
    }
    if level == "block" {
        eprintln!("codeflow scan: BLOCKED {scan} ({level}): {message}");
        true
    } else {
        eprintln!("codeflow scan: warning {scan} ({level}): {message}");
        false
    }
}

fn secret_scan(root: &Path, base: &str, head: &str) -> Result<bool, String> {
    let level = secret_level(root, base)?;
    if level == "off" {
        println!("codeflow scan: secret scan: skipped (git.secret_scan is off at the base)");
        return Ok(false);
    }
    let engine = gitleaks::Engine::from_env();
    let verified = match engine.verify() {
        Ok(verified) => verified,
        Err(unusable) => {
            let message = format!(
                "{}; the pin is {}; install it from {} and check with `codeflow doctor --check scan-tools`",
                unusable.describe(),
                engine.expected.describe(),
                gitleaks::RELEASES
            );
            return Ok(verdict("secret scan", &level, true, &message));
        }
    };
    let report = match engine.scan_range(&verified, root, base, head) {
        Ok(report) => report,
        Err(why) => return Ok(verdict("secret scan", &level, true, &why)),
    };
    let short = |id: &str| id.chars().take(12).collect::<String>();
    let scope = format!(
        "{} commit(s) {} holds and {} does not",
        report.commits,
        short(&report.head),
        short(&report.base)
    );
    if report.nothing_to_scan {
        return Ok(verdict(
            "secret scan",
            &level,
            false,
            &format!("no file changes in the {scope}; nothing to scan"),
        ));
    }
    if report.known_allowed > 0 {
        println!(
            "codeflow scan: secret scan: allowed {} finding(s) of the CodeFlow 3.0.0 pipeline prose",
            report.known_allowed
        );
    }
    if report.findings.is_empty() {
        return Ok(verdict(
            "secret scan",
            &level,
            false,
            &format!("no leaks found in the {scope}"),
        ));
    }
    for finding in &report.findings {
        eprintln!(
            "codeflow scan: gitleaks {} at {}:{} in commit {}",
            display_text(&finding.rule),
            display_text(&finding.file),
            finding.line,
            short(&finding.commit)
        );
    }
    let message = format!(
        "gitleaks found {} leak(s) in the {scope}; remove the secret from history, or land a reviewed fingerprint in the base's .gitleaksignore first",
        report.findings.len()
    );
    Ok(verdict("secret scan", &level, true, &message))
}

fn dependency_audit(root: &Path, base: &str, head: &str) -> Result<bool, String> {
    let base_id = range::resolve(root, base)?;
    let levels = match osv::levels(root, &base_id) {
        Ok(levels) => levels,
        Err(why) => return Ok(verdict("dependency audit", "block", true, &why)),
    };
    let level = levels.level();
    let keys = format!(
        "git.security_review={}, git.dep_audit={}",
        levels.security_review, levels.dep_audit
    );
    if level == "off" {
        println!("codeflow scan: dependency audit: skipped ({keys} at the base)");
        return Ok(false);
    }
    let scanner = osv::Scanner::with_path(std::env::var_os("PATH").as_deref());
    let verified = match scanner.verify() {
        Ok(verified) => verified,
        Err(why) => {
            let message = format!(
                "{why}; the pin is {}; install it from {} and check with `codeflow doctor --check scan-tools`",
                scanner.describe(),
                osv::RELEASES
            );
            return Ok(verdict("dependency audit", level, true, &message));
        }
    };
    let audit = match osv::audit(&verified, root, base, head) {
        Ok(audit) => audit,
        Err(why) => return Ok(verdict("dependency audit", level, true, &why)),
    };
    Ok(match audit {
        osv::Audit::Clean => verdict("dependency audit", level, false, "no known advisories"),
        osv::Audit::NoSources => verdict(
            "dependency audit",
            level,
            false,
            "no scannable manifests found; nothing to audit",
        ),
        osv::Audit::Accepted(lines) => verdict(
            "dependency audit",
            level,
            false,
            &format!(
                "every advisory ({}) is accepted by the base's project exception hook",
                lines.len()
            ),
        ),
        osv::Audit::Failed { code, output } => {
            eprint!("{}", codeflow_core::git::display_relay(&output));
            verdict(
                "dependency audit",
                level,
                true,
                &format!(
                    "dependency advisories found (or scan error, exit {code}) with {keys}; triage, or record a justified suppression in an osv-scanner.toml beside the lockfile on the base"
                ),
            )
        }
    })
}

/// Run `codeflow scan`; returns the exit code.
#[must_use]
pub fn run(args: &ScanArgs) -> i32 {
    let root = match super::repo_root() {
        Ok(root) => root,
        Err(error) => {
            eprintln!("codeflow scan: {error}");
            return 2;
        }
    };
    let base = match range::resolve(&root, &args.base) {
        Ok(base) => base,
        Err(error) => {
            eprintln!("codeflow scan: --base: {error}");
            return 2;
        }
    };
    if let Err(error) = range::resolve(&root, &args.head) {
        eprintln!("codeflow scan: --head: {error}");
        return 2;
    }
    let mut blocked = false;
    if args.only.as_deref() != Some("deps") {
        match secret_scan(&root, &base, &args.head) {
            Ok(stop) => blocked |= stop,
            Err(error) => {
                eprintln!("codeflow scan: secret scan: {error}");
                return 2;
            }
        }
    }
    if args.only.as_deref() != Some("secrets") {
        match dependency_audit(&root, &base, &args.head) {
            Ok(stop) => blocked |= stop,
            Err(error) => {
                eprintln!("codeflow scan: dependency audit: {error}");
                return 2;
            }
        }
    }
    i32::from(blocked)
}
