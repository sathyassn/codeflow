//! `codeflow validate [<path>] [--docs]` — frontmatter/structure checks
//! plus the referential-integrity lint (charter §3.1, AC #9).

use std::path::{Path, PathBuf};

use clap::Args;
use codeflow_core::hooks::policy_schema;
use codeflow_core::remedy::{Finding, PORTAL_COPY};
use codeflow_core::validate::docs::lint_docs;
use codeflow_core::validate::portal::{
    validate_portal_with, ContentBlock, ContentText, LookupInputs,
};
use codeflow_core::validate::{
    validate_epic, validate_spec, validate_task, validate_workgraph, ValidateOptions,
};
use codeflow_present::copy_rules::{page_findings, CopyFinding, PageBlock, PageText};

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

    /// With --portal, list every copy warning by block instead of one
    /// summary line a page.
    #[arg(long, requires = "portal")]
    pub copy_detail: bool,

    /// Also judge every record changed since REF (a commit or branch)
    /// against the lifecycle transition rules, as CI judges a pull request.
    #[arg(long, value_name = "REF", conflicts_with_all = ["path", "portal"])]
    pub since: Option<String>,
}

pub fn run(args: &ValidateArgs) -> i32 {
    let root = match super::repo_root() {
        Ok(root) => root,
        Err(error) => {
            eprintln!("codeflow: {error}", error = crate::cmd::shown(&error));
            return 2;
        }
    };
    let mut failed = false;

    failed |= !validate_policy(&root);
    if let Some(portal) = &args.portal {
        failed |= !run_portal_validation(&root, portal, args.copy_detail);
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

fn run_portal_validation(root: &Path, portal: &Path, copy_detail: bool) -> bool {
    let stages = super::git_hook::stage_names();
    let inputs = LookupInputs {
        assets: &crate::embedded::EmbeddedAssets,
        hook_stages: &stages,
    };
    let report = validate_portal_with(root, portal, Some(&inputs));
    let pages = portal_copy_findings(&report.page_text);
    for warning in portal_copy_warnings(&pages, copy_detail) {
        eprintln!("{}", warning.line("validate --portal", "warning"));
    }
    // Integrity errors come after the copy warnings, so they are the last
    // thing a reader sees.
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

fn portal_copy_warnings(pages: &[PageCopy], copy_detail: bool) -> Vec<Finding> {
    let lines = if copy_detail {
        copy_detail_lines(pages)
    } else {
        copy_summary_lines(pages)
    };
    lines
        .into_iter()
        .map(|line| Finding::new(line, PORTAL_COPY.remedy()))
        .collect()
}

/// One built page's copy warnings (TSK-1255) and its longest text block in
/// characters.
struct PageCopy {
    path: String,
    findings: Vec<CopyFinding>,
    longest: usize,
}

/// The copy warnings for each built page: the same character budget, run,
/// ellipsis and time rules `present check` applies, read from the page's
/// paragraphs and list items. Pages with no warning are left out. They never
/// fail validation.
fn portal_copy_findings(page_text: &[(String, Vec<ContentBlock>)]) -> Vec<PageCopy> {
    let page = |text: &ContentText| PageText {
        text: text.text.clone(),
        prose: text.prose.clone(),
    };
    let mut pages = Vec::new();
    for (path, content) in page_text {
        let blocks: Vec<PageBlock> = content
            .iter()
            .map(|block| match block {
                ContentBlock::Paragraph(text) => PageBlock::Paragraph(page(text)),
                ContentBlock::List(items) => PageBlock::List(items.iter().map(page).collect()),
                ContentBlock::Visual => PageBlock::Visual,
            })
            .collect();
        let findings = page_findings(&blocks);
        if findings.is_empty() {
            continue;
        }
        let longest = content
            .iter()
            .flat_map(|block| match block {
                ContentBlock::Paragraph(text) => std::slice::from_ref(text),
                ContentBlock::List(items) => items.as_slice(),
                ContentBlock::Visual => &[],
            })
            .map(|text| text.text.chars().count())
            .max()
            .unwrap_or(0);
        pages.push(PageCopy {
            path: path.clone(),
            findings,
            longest,
        });
    }
    pages
}

/// One line a page, with its count by rule and its longest block, then a
/// total that names the flag for the per-block list.
fn copy_summary_lines(pages: &[PageCopy]) -> Vec<String> {
    let mut lines = Vec::new();
    let mut total = 0;
    for page in pages {
        let mut by_rule = std::collections::BTreeMap::<&str, usize>::new();
        for finding in &page.findings {
            *by_rule.entry(finding.rule).or_default() += 1;
        }
        let counts: Vec<String> = by_rule
            .iter()
            .map(|(rule, count)| format!("{rule} {count}"))
            .collect();
        total += page.findings.len();
        lines.push(format!(
            "{}: {} copy warning(s) ({}); longest block {} characters",
            page.path,
            page.findings.len(),
            counts.join(", "),
            page.longest
        ));
    }
    if total > 0 {
        lines.push(format!(
            "{total} copy warning(s) on {} page(s); `--copy-detail` lists each block",
            pages.len()
        ));
    }
    lines
}

/// Every copy warning by page and rule, with its message.
fn copy_detail_lines(pages: &[PageCopy]) -> Vec<String> {
    pages
        .iter()
        .flat_map(|page| {
            page.findings
                .iter()
                .map(|finding| format!("{}: {}: {}", page.path, finding.rule, finding.message))
        })
        .collect()
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
                eprintln!(
                    "validate --since: error: {error}",
                    error = crate::cmd::shown(&error)
                );
            }
            if verdict.is_clean() {
                println!("validate --since {since}: record transitions clean");
            }
            verdict.is_clean()
        }
        Err(error) => {
            eprintln!(
                "validate --since: error: {error}",
                error = crate::cmd::shown(&error)
            );
            false
        }
    }
}

/// Strictly validate `.codeflow/policy.json` — an invalid value would
/// otherwise silently default away a gate. Returns `true` when clean or absent.
fn validate_policy(root: &Path) -> bool {
    match policy_schema::validate_policy(root) {
        Ok(()) => {
            for warning in policy_schema::deprecation_warnings(root)
                .into_iter()
                .filter(|warning| policy_schema::first_unsupported_warning(&warning.text))
            {
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
                eprintln!("validate: error: policy: {e}", e = crate::cmd::shown(&e));
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
    let shown = codeflow_core::git::display_path(&base);

    let metadata = match std::fs::metadata(&base) {
        Ok(metadata) => metadata,
        // Absent only when the name itself is missing: the root is the
        // discovered repository root, and a dangling link cannot be read.
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound
                && path.is_none()
                && std::fs::symlink_metadata(&base)
                    .is_err_and(|leaf| leaf.kind() == std::io::ErrorKind::NotFound) =>
        {
            println!("validate: {shown} absent — no records to validate at this tier");
            return true;
        }
        Err(error) => {
            let error = crate::cmd::shown(&error);
            eprintln!("validate: cannot read {shown}: {error}");
            return false;
        }
    };

    let files = if metadata.is_file() {
        // An explicitly named file must be validatable — silently skipping it
        // and reporting "0 record(s) clean" would be a false green.
        let name = base.file_name().unwrap_or_default().as_encoded_bytes();
        if !name.starts_with(b"TSK-") && !name.starts_with(b"EPC-") && !name.starts_with(b"SPC-") {
            eprintln!(
                "validate: {shown} is not a work record (expected an EPC-*/SPC-*/TSK-* filename) — nothing validated"
            );
            return false;
        }
        vec![base.clone()]
    } else {
        match collect_record_files(&base) {
            Ok(files) => files,
            Err(error) => {
                let error = crate::cmd::shown(&error);
                eprintln!("validate: cannot read records under {shown}: {error}");
                return false;
            }
        }
    };

    if files.is_empty() {
        println!("validate: no work records under {shown}");
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
                    eprintln!(
                        "{}: error: {e}",
                        codeflow_core::git::display_path(&file),
                        e = crate::cmd::shown(&e)
                    );
                }
                for w in &warnings {
                    eprintln!(
                        "{}",
                        w.finding(&file)
                            .line(&codeflow_core::git::display_path(&file), "warning")
                    );
                }
                if !errors.is_empty() {
                    clean = false;
                }
            }
            Err(e) => {
                eprintln!(
                    "{}: error: {e}",
                    codeflow_core::git::display_path(&file),
                    e = crate::cmd::shown(&e)
                );
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
    paths.sort_by(|a, b| codeflow_core::ids::compare_id_paths(a, b));
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
    for warning in &report.warnings {
        eprintln!("{}", warning.line("validate --docs", "warning"));
    }
    // An issue starts with a checkout file path: a fixed label owns the
    // line's first bytes (TSK-388).
    for issue in &report.issues {
        eprintln!("validate --docs: error: {issue}");
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
    use super::{
        copy_detail_lines, copy_summary_lines, portal_copy_findings, ContentBlock, ContentText,
    };

    fn text(value: &str) -> ContentText {
        ContentText {
            text: value.to_string(),
            prose: value.to_string(),
        }
    }

    /// TSK-1255: a built page with a long paragraph, a cut list item and a
    /// run of text blocks draws the copy warnings, named by its path; a
    /// short page with a figure between its blocks draws none.
    #[test]
    fn portal_copy_warnings_name_the_page_and_rule() {
        let long = "word ".repeat(30);
        let pages = vec![
            (
                "dist/status/index.html".to_string(),
                vec![
                    ContentBlock::Paragraph(text(long.trim())),
                    ContentBlock::List(vec![text("items cut to that...")]),
                    ContentBlock::Paragraph(text("Read at 13:41.")),
                    ContentBlock::Paragraph(text("Four.")),
                ],
            ),
            (
                "dist/clean/index.html".to_string(),
                vec![
                    ContentBlock::Paragraph(text("Lead at 13:41 EST.")),
                    ContentBlock::Visual,
                    ContentBlock::Paragraph(text("After the figure.")),
                ],
            ),
        ];
        let rules: Vec<String> = copy_detail_lines(&portal_copy_findings(&pages))
            .iter()
            .map(|warning| {
                warning
                    .splitn(3, ": ")
                    .take(2)
                    .collect::<Vec<_>>()
                    .join(": ")
            })
            .collect();
        assert_eq!(
            rules,
            [
                "dist/status/index.html: copy-density",
                "dist/status/index.html: copy-ellipsis",
                "dist/status/index.html: copy-time-zone",
                "dist/status/index.html: copy-run",
            ]
        );
    }

    /// TSK-1255: by default each page with warnings gets one line, with its
    /// count by rule and its longest block, and a total follows; a clean
    /// page gets no line.
    #[test]
    fn portal_copy_summary_is_one_line_a_page_and_a_total() {
        let long = "word ".repeat(30);
        let pages = vec![
            (
                "dist/status/index.html".to_string(),
                vec![
                    ContentBlock::Paragraph(text(long.trim())),
                    ContentBlock::List(vec![text("items cut to that...")]),
                    ContentBlock::Paragraph(text("Read at 13:41.")),
                    ContentBlock::Paragraph(text("Four.")),
                ],
            ),
            (
                "dist/clean/index.html".to_string(),
                vec![ContentBlock::Paragraph(text("Lead at 13:41 EST."))],
            ),
            (
                "dist/guide/index.html".to_string(),
                vec![ContentBlock::Paragraph(text(&"word ".repeat(40)))],
            ),
        ];
        assert_eq!(
            copy_summary_lines(&portal_copy_findings(&pages)),
            [
                "dist/status/index.html: 4 copy warning(s) (copy-density 1, copy-ellipsis 1, \
                 copy-run 1, copy-time-zone 1); longest block 149 characters",
                "dist/guide/index.html: 1 copy warning(s) (copy-density 1); \
                 longest block 200 characters",
                "5 copy warning(s) on 2 page(s); `--copy-detail` lists each block",
            ]
        );
        assert!(copy_summary_lines(&portal_copy_findings(&pages[1..2])).is_empty());
    }

    #[test]
    fn portal_copy_warnings_carry_a_remedy_in_summary_and_detail_order() {
        let pages = portal_copy_findings(&[(
            "dist/status/index.html".to_string(),
            vec![ContentBlock::Paragraph(text("Read at 13:41."))],
        )]);
        for detail in [false, true] {
            let expected = if detail {
                copy_detail_lines(&pages)
            } else {
                copy_summary_lines(&pages)
            };
            let warnings = super::portal_copy_warnings(&pages, detail);
            assert_eq!(warnings.len(), if detail { 1 } else { 2 });
            for (warning, line) in warnings.iter().zip(expected) {
                assert_eq!(
                    warning.line("validate --portal", "warning"),
                    format!(
                        "validate --portal: warning: {line}\n  clear it: revise the source page \
                         to address the copy rules, rebuild the portal, then rerun \
                         `codeflow validate --portal <dir> --copy-detail`"
                    )
                );
            }
        }
        assert!(super::portal_copy_warnings(&[], false).is_empty());
        assert!(super::portal_copy_warnings(&[], true).is_empty());
    }

    /// A record home behind a dangling link cannot be read; it is never a
    /// tier without records. A missing home still validates clean.
    #[cfg(unix)]
    #[test]
    fn r23_a_dangling_record_home_refuses() {
        let dir = tempfile::tempdir().unwrap();
        assert!(super::validate_records(dir.path(), None));
        std::os::unix::fs::symlink("gone", dir.path().join("project-management")).unwrap();
        assert!(!super::validate_records(dir.path(), None));
    }

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
