//! `codeflow validate [<path>] [--docs]` — frontmatter/structure checks
//! plus the referential-integrity lint (charter §3.1, AC #9).

use std::path::{Path, PathBuf};

use clap::Args;
use codeflow_core::validate::docs::lint_docs;
use codeflow_core::validate::{ValidateOptions, validate_epic, validate_task};

#[derive(Args)]
pub struct ValidateArgs {
    /// File or directory to validate (default: project-management/)
    pub path: Option<PathBuf>,

    /// Also run the doc-graph referential-integrity lint
    #[arg(long)]
    pub docs: bool,
}

pub fn run(args: &ValidateArgs) -> i32 {
    let root = super::repo_root();
    let mut failed = false;

    failed |= !validate_records(&root, args.path.as_deref());

    if args.docs {
        failed |= !run_docs_lint(&root);
    }

    i32::from(failed)
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
        if !name.starts_with("TSK-") && !name.starts_with("EPC-") {
            eprintln!(
                "validate: {} is not an epic/task record (expected an EPC-*/TSK-* filename) — nothing validated",
                base.display()
            );
            return false;
        }
        vec![base.clone()]
    } else {
        collect_record_files(&base)
    };

    if files.is_empty() {
        println!("validate: no epic/task records under {}", base.display());
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

/// Recursively collect `EPC-*`/`TSK-*` markdown files.
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
            if name.starts_with("TSK-") || name.starts_with("EPC-") {
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
