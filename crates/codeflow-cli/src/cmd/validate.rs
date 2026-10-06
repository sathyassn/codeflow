//! `codeflow validate [<path>] [--docs]` — frontmatter/structure checks
//! plus the referential-integrity lint (charter §3.1, AC #9).

use std::path::{Path, PathBuf};

use clap::Args;
use codeflow_core::hooks::policy_schema;
use codeflow_core::validate::docs::lint_docs;
use codeflow_core::validate::portal::{validate_portal_with, LookupInputs};
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
    let root = match super::repo_root() {
        Ok(root) => root,
        Err(error) => {
            eprintln!("codeflow: {error}");
            return 2;
        }
    };
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
    let stages = super::git_hook::stage_names();
    let inputs = LookupInputs {
        assets: &crate::embedded::EmbeddedAssets,
        hook_stages: &stages,
    };
    let report = validate_portal_with(root, portal, Some(&inputs));
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
        println!("{}", note.line("validate --docs", "note"));
    }
    for warning in &report.warnings {
        eprintln!("{}", warning.line("validate --docs", "warning"));
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
            for notice in &verdict.notices {
                eprintln!("{}", notice.line("validate --since", "notice"));
            }
            for warning in &verdict.warnings {
                eprintln!("{}", warning.line("validate --since", "warning"));
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
            for warning in policy_schema::deprecation_warnings(root) {
                eprintln!("{}", warning.line("validate", "warning"));
            }
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
            if let Some(hint) =
                policy_schema::upgrade_order_hint(&errors, env!("CARGO_PKG_VERSION"))
            {
                eprintln!("validate: {hint}");
            }
            false
        }
    }
}

/// Frontmatter validation over a file or tree. Returns `true` when clean.
fn validate_records(root: &Path, path: Option<&Path>) -> bool {
    let base = path.map_or_else(|| root.join("project-management"), Path::to_path_buf);

    let metadata = match std::fs::metadata(&base) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && path.is_none() => {
            println!(
                "validate: {} absent — no records to validate at this tier",
                base.display()
            );
            return true;
        }
        Err(error) => {
            eprintln!("validate: cannot read {}: {error}", base.display());
            return false;
        }
    };

    let files = if metadata.is_file() {
        // An explicitly named file must be validatable — silently skipping it
        // and reporting "0 record(s) clean" would be a false green.
        let name = base.file_name().unwrap_or_default().as_encoded_bytes();
        if !name.starts_with(b"TSK-") && !name.starts_with(b"EPC-") && !name.starts_with(b"SPC-") {
            eprintln!(
                "validate: {} is not a work record (expected an EPC-*/SPC-*/TSK-* filename) — nothing validated",
                base.display()
            );
            return false;
        }
        vec![base.clone()]
    } else {
        match collect_record_files(&base) {
            Ok(files) => files,
            Err(error) => {
                eprintln!(
                    "validate: cannot read records under {}: {error}",
                    base.display()
                );
                return false;
            }
        }
    };

    if files.is_empty() {
        println!("validate: no work records under {}", base.display());
        return true;
    }

    let opts = ValidateOptions::default();
    let mut clean = true;
    let mut checked = 0usize;

    for file in files {
        let name = file.file_name().unwrap_or_default().as_encoded_bytes();
        let result = if name.starts_with(b"TSK-") {
            validate_task(&file, &opts)
        } else if name.starts_with(b"EPC-") {
            validate_epic(&file, &opts)
        } else if name.starts_with(b"SPC-") {
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
                    eprintln!(
                        "{}",
                        w.finding(&file)
                            .line(&file.display().to_string(), "warning")
                    );
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
fn collect_record_files(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut paths = std::fs::read_dir(dir)?
        .map(|entry| entry.map(|e| e.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    paths.sort();
    for path in paths {
        if std::fs::metadata(&path)?.is_dir() {
            out.extend(collect_record_files(&path)?);
        } else if path.extension().is_some_and(|ext| ext == "md") {
            let name = path.file_name().unwrap_or_default().as_encoded_bytes();
            if name.starts_with(b"TSK-") || name.starts_with(b"EPC-") || name.starts_with(b"SPC-") {
                out.push(path);
            }
        }
    }
    Ok(out)
}

/// The docs-integrity lint. Returns `true` when clean.
fn run_docs_lint(root: &Path) -> bool {
    let report = lint_docs(root);

    for note in &report.notes {
        println!("{}", note.line("validate --docs", "note"));
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

#[cfg(test)]
mod tests {

    #[test]
    fn r16_unreadable_record_directory_refuses() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("records");
        std::fs::write(&file, "not a directory").unwrap();
        assert!(super::collect_record_files(&file).is_err());
    }

    use std::path::{Path, PathBuf};

    use codeflow_core::validate::portal::lookups::{
        self, page_with_title, verify_lookup_page, LookupInputs, POLICY_REFERENCE, SKILL_CATALOG,
    };

    use crate::embedded::EmbeddedAssets;

    fn repository() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    /// The lookup pages this repository publishes equal what the binary
    /// generates, and the policy reference names exactly the dispatcher's
    /// hook stages. With `CODEFLOW_REGENERATE_LOOKUPS=1` the generated part of
    /// each page is rewritten first; the authored stage table is kept.
    #[test]
    fn derived_lookup_pages_match_the_binary() {
        let stages = crate::cmd::git_hook::stage_names();
        let inputs = LookupInputs {
            assets: &EmbeddedAssets,
            hook_stages: &stages,
        };
        for (path, derive) in [
            ("docs/skills.md", SKILL_CATALOG),
            ("docs/policy-reference.md", POLICY_REFERENCE),
        ] {
            let file = repository().join(path);
            if std::env::var_os("CODEFLOW_REGENERATE_LOOKUPS").is_some() {
                let current = std::fs::read_to_string(&file).unwrap_or_default();
                let end = format!("<!-- codeflow-derived {derive} end -->\n");
                let authored = current
                    .split_once(&end)
                    .map_or(String::new(), |(_, rest)| rest.to_string());
                let generated = if derive == SKILL_CATALOG {
                    lookups::skill_catalog(&EmbeddedAssets).unwrap()
                } else {
                    lookups::policy_reference()
                };
                let title = lookups::title_of(derive).unwrap();
                std::fs::write(
                    &file,
                    format!("{}{authored}", page_with_title(title, &generated)),
                )
                .unwrap();
            }
            let text = std::fs::read_to_string(&file)
                .unwrap()
                .replace("\r\n", "\n");
            if let Err(why) = verify_lookup_page(derive, &text, &inputs) {
                panic!("{path}: {why}; run CODEFLOW_REGENERATE_LOOKUPS=1 cargo test -p codeflow-cli derived_lookup");
            }
        }
    }

    #[test]
    fn the_dispatcher_names_the_five_git_hook_stages() {
        let mut stages = crate::cmd::git_hook::stage_names();
        stages.sort();
        assert_eq!(
            stages,
            [
                "commit-msg",
                "pre-commit",
                "pre-merge-commit",
                "pre-push",
                "reference-transaction"
            ]
        );
    }
}
