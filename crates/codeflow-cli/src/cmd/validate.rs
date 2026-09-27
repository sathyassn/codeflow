//! `codeflow validate [<path>] [--docs]` — frontmatter/structure checks
//! plus the referential-integrity lint (charter §3.1, AC #9).

use std::path::{Path, PathBuf};

use clap::Args;
use codeflow_core::hooks::policy_schema;
use codeflow_core::validate::docs::lint_docs;
use codeflow_core::validate::portal::validate_portal;
use codeflow_core::validate::{
    validate_epic, validate_spec, validate_task, validate_workgraph, ValidateOptions,
};

#[derive(Args)]
pub struct ValidateArgs {
    /// File or directory to validate (default: project-management/)
    pub path: Option<PathBuf>,

    /// Also run the doc-graph referential-integrity lint
    #[arg(long)]
    pub docs: bool,

    /// Verify a portal evidence manifest without executing project code.
    #[arg(long, value_name = "DIR", conflicts_with = "path")]
    pub portal: Option<PathBuf>,

    /// Also judge every record changed since REF (a commit or branch)
    /// against the lifecycle transition rules, as CI judges a pull request.
    #[arg(long, value_name = "REF", conflicts_with_all = ["path", "portal"])]
    pub since: Option<String>,
}

pub fn run(args: &ValidateArgs) -> i32 {
    let root = super::repo_root();
    let mut failed = false;

    failed |= !validate_policy(&root);
    if let Some(portal) = &args.portal {
        failed |= !run_portal_validation(&root, portal);
        if args.docs {
            failed |= !run_docs_lint(&root);
        }
    } else if args.docs && args.path.is_none() {
        failed |= !run_workgraph_validation(&root);
        if let Some(since) = &args.since {
            failed |= !run_transition_validation(&root, since);
        }
    } else if let Some(since) = &args.since {
        failed |= !validate_records(&root, None);
        failed |= !run_transition_validation(&root, since);
    } else {
        failed |= !validate_records(&root, args.path.as_deref());
        if args.docs {
            failed |= !run_docs_lint(&root);
        }
    }

    i32::from(failed)
}

fn run_portal_validation(root: &Path, portal: &Path) -> bool {
    let report = validate_portal(root, portal);
    for issue in &report.issues {
        eprintln!("validate --portal: error: {issue}");
    }
    if report.is_clean() {
        println!("validate --portal: {} page(s) clean", report.checked_pages);
        true
    } else {
        eprintln!(
            "validate --portal: {} integrity error(s)",
            report.issues.len()
        );
        false
    }
}

fn run_workgraph_validation(root: &Path) -> bool {
    let report = validate_workgraph(root);
    for note in &report.notes {
        println!("validate --docs: note: {note}");
    }
    for warning in &report.warnings {
        eprintln!("validate --docs: warning: {warning}");
    }
    for issue in &report.issues {
        eprintln!("validate --docs: error: {issue}");
    }
    if report.is_clean() {
        println!("validate: {} record(s) clean", report.checked_records);
        println!("validate --docs: doc graph clean");
        true
    } else {
        eprintln!(
            "validate --docs: {} workgraph integrity error(s)",
            report.issues.len()
        );
        false
    }
}

/// Judge each record changed since `since` by the transition rules.
fn run_transition_validation(root: &Path, since: &str) -> bool {
    match codeflow_core::workgraph::lifecycle::judge_range(root, since, None) {
        Ok(verdict) => {
            for warning in &verdict.warnings {
                eprintln!("validate --since: warning: {warning}");
            }
            for error in &verdict.errors {
                eprintln!("validate --since: error: {error}");
            }
            if verdict.is_clean() {
                println!("validate --since {since}: record transitions clean");
            }
            verdict.is_clean()
        }
        Err(error) => {
            eprintln!("validate --since: error: {error}");
            false
        }
    }
}

/// Strictly validate `.codeflow/policy.json` — an invalid value would
/// otherwise silently default away a gate. Returns `true` when clean or absent.
fn validate_policy(root: &Path) -> bool {
    match policy_schema::validate_policy(root) {
        Ok(()) => {
            if root.join(".codeflow").join("policy.json").exists() {
                println!("validate: .codeflow/policy.json clean");
            } else {
                println!("validate: no .codeflow/policy.json — built-in policy defaults apply");
            }
            true
        }
        Err(errors) => {
            for e in &errors {
                eprintln!("validate: error: policy: {e}");
            }
            eprintln!(
                "validate: .codeflow/policy.json is invalid — see `codeflow policy explain` for every key's valid values"
            );
            false
        }
    }
}

/// Frontmatter validation over a file or tree. Returns `true` when clean.
fn validate_records(root: &Path, path: Option<&Path>) -> bool {
    let base = path.map_or_else(|| root.join("project-management"), Path::to_path_buf);

    if !base.exists() {
        println!(
            "validate: {} absent — no records to validate at this tier",
            base.display()
        );
        return true;
    }

    let files = if base.is_file() {
        // An explicitly named file must be validatable — silently skipping it
        // and reporting "0 record(s) clean" would be a false green.
        let name = base
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if !name.starts_with("TSK-") && !name.starts_with("EPC-") && !name.starts_with("SPC-") {
            eprintln!(
                "validate: {} is not a work record (expected an EPC-*/SPC-*/TSK-* filename) — nothing validated",
                base.display()
            );
            return false;
        }
        vec![base.clone()]
    } else {
        collect_record_files(&base)
    };

    if files.is_empty() {
        println!("validate: no work records under {}", base.display());
        return true;
    }

    let opts = ValidateOptions::default();
    let mut clean = true;
    let mut checked = 0usize;

    for file in files {
        let name = file
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        let result = if name.starts_with("TSK-") {
            validate_task(&file, &opts)
        } else if name.starts_with("EPC-") {
            validate_epic(&file, &opts)
        } else if name.starts_with("SPC-") {
            validate_spec(&file, &opts)
        } else {
            continue;
        };
        checked += 1;

        match result {
            Ok((errors, warnings)) => {
                for e in &errors {
                    eprintln!("{}: error: {e}", file.display());
                }
                for w in &warnings {
                    eprintln!("{}: warning: {w}", file.display());
                }
                if !errors.is_empty() {
                    clean = false;
                }
            }
            Err(e) => {
                eprintln!("{}: error: {e}", file.display());
                clean = false;
            }
        }
    }

    if clean {
        println!("validate: {checked} record(s) clean");
    }
    clean
}

/// Recursively collect `EPC-*`/`SPC-*`/`TSK-*` markdown files.
fn collect_record_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    let mut paths: Vec<PathBuf> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            out.extend(collect_record_files(&path));
        } else if path.extension().is_some_and(|ext| ext == "md") {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            if name.starts_with("TSK-") || name.starts_with("EPC-") || name.starts_with("SPC-") {
                out.push(path);
            }
        }
    }
    out
}

/// The docs-integrity lint. Returns `true` when clean.
fn run_docs_lint(root: &Path) -> bool {
    let report = lint_docs(root);

    for note in &report.notes {
        println!("validate --docs: note: {note}");
    }
    for issue in &report.issues {
        eprintln!("{issue}");
    }

    if report.is_clean() {
        println!("validate --docs: doc graph clean");
        true
    } else {
        eprintln!(
            "validate --docs: {} dangling reference(s) / integrity error(s)",
            report.issues.len()
        );
        false
    }
}
