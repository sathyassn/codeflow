//! Names from git and the operating system are read as bytes (issue 79).
//!
//! The rule is in `docs/architecture.md` and the layer is
//! `crates/codeflow-core/src/git/name.rs`: a name is `GitName` bytes, shown
//! through `display()` and used by a rule through `rule_text()` (valid text
//! or a refusal) or by its bytes. A lossy decode makes two different names
//! equal, so it is never used where the value is compared, matched, keyed or
//! routed on.
//!
//! This test parses every Rust source under `crates/` with `syn` and finds
//! each production call that decodes lossily or reads text through a git2
//! text accessor:
//!
//! - `from_utf8_lossy` and `to_string_lossy`, as a call, a path used as a
//!   value (`map(String::from_utf8_lossy)`) or inside a macro such as
//!   `format!`;
//! - git2's `shorthand` and `symbolic_target`, and `name()` in a file that
//!   uses git2, which return text and fail or lose bytes on a name that is
//!   not valid UTF-8.
//!
//! A site is allowed only when listed in [`EXCEPTIONS`] with a closed tag
//! and its display, rejection-only or format-contract reason, keyed by
//! file, enclosing item and call, with a
//! count: a new call in an item, or a fixed one that leaves an entry, fails
//! the test. Items marked `#[cfg(test)]` are skipped one by one. The layer
//! itself (`git/name.rs`) is not scanned.
//!
//! The decision-input scan covers all crates: Unicode whitespace, line
//! splitting, predicate trimming and decode-to-absent chains. Only counted
//! exceptions with a closed reason tag may remain. Separator sets follow
//! the source grammar explicitly; decoding failure must not hide a name.
//!
//! Limits: a count keyed by item can net a fixed call against a new call in
//! the same item. This syntactic scan does not see `Path::display()`,
//! `format!("{:?}")`, regex `\s`, `matches!(c, ' ' | '\u{a0}')`, serde-side
//! trimming, `eq_ignore_ascii_case`, or proc-macro output. It does not prove
//! data flow or resolve types; site regressions remain the primary proof.
//! Obtaining errors include IO, environment, metadata and deserialization.
//! Wrapper return types and errors carried through a local variable before a
//! match require an explicit audit; a syntactic call inventory cannot prove
//! their consumer behavior. `unproven` rows name the refusing consumer.
//! Separator grammars use `grammar:`; delimiter removal uses `framing:` and
//! must occur in the reader registered in `FRAMING_OWNERS`, never downstream.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use syn::ext::IdentExt;
use syn::visit::Visit;

/// The layer that owns lossy decoding.
const LAYER: &str = "crates/codeflow-core/src/git/name.rs";

/// Calls found by name.
const LOSSY: &[&str] = &["from_utf8_lossy", "to_string_lossy"];
const UNICODE_WHITESPACE: &[&str] = &[
    "is_whitespace",
    "split_whitespace",
    "trim",
    "trim_start",
    "trim_end",
    "lines",
    "split_terminator",
];
const PREDICATE_TRIM: &[&str] = &["trim_matches", "trim_start_matches", "trim_end_matches"];
const OBTAIN: &[&str] = &[
    "read_to_string",
    "read",
    "read_line",
    "read_to_end",
    "metadata",
    "var",
    "var_os",
    "from_utf8",
    "to_str",
    "as_str",
    "utf8_to_str",
    "from_str",
    "from_slice",
    "from_reader",
    "find_tree",
    "find_blob",
    "find_commit",
    "find_object",
    "find_reference",
    "head",
    "graph_descendant_of",
    "merge_base",
    "get_path",
    "into_string",
];
/// Obtaining calls named with their type, since the bare name is too common
/// to read as one (`open`, `new`): (type, function).
const OBTAIN_QUALIFIED: &[(&str, &str)] = &[
    ("Repository", "discover"),
    ("Repository", "open"),
    ("Pattern", "new"),
];

/// Whether a method name is an obtaining call: [`OBTAIN`] or git2's
/// `peel*` family.
fn obtaining_method(name: &str) -> bool {
    OBTAIN.contains(&name) || name.starts_with("peel")
}
const ABSENT: &[&str] = &[
    "ok",
    "unwrap_or",
    "unwrap_or_default",
    "unwrap_or_else",
    "map_or",
    "map_or_else",
    "or",
    "or_else",
    "is_ok",
    "is_err",
    "flatten",
];
const GIT2_TEXT: &[&str] = &["shorthand", "symbolic_target"];

/// Production sites that decode lossily or read git2 text, where the value
/// only reaches a person or is not a git or OS name: (file, enclosing item,
/// call, how many, closed reason tag); the comment states the precise contract.
const EXCEPTIONS: &[(&str, &str, &str, usize, &str)] = &[
    // Failed Git stderr is displayed inside Err. Successful ignore-rule stdout remains exact bytes.
    (
        "codeflow-core/src/root_checkout.rs",
        "git_stdin",
        "from_utf8_lossy",
        1,
        "display",
    ),
    // process output shown to a person, never compared
    (
        "codeflow-cli/examples/release_integration.rs",
        "command",
        "from_utf8_lossy",
        2,
        "display",
    ),
    // a git or tool error message shown to a person, never compared
    (
        "codeflow-cli/src/cmd/ci.rs",
        "base_refusal",
        "from_utf8_lossy",
        1,
        "display",
    ),
    // Only failed git-log stderr is displayed lossily; successful commit messages must be UTF-8, as in commit-msg.
    (
        "codeflow-cli/src/cmd/ci.rs",
        "enumerate_commits",
        "from_utf8_lossy",
        1,
        "display",
    ),
    // an error message shown to a person
    (
        "codeflow-cli/src/cmd/ci.rs",
        "git_bytes",
        "from_utf8_lossy",
        1,
        "display",
    ),
    // the text of merge-base ids and a diff printed with core.quotepath=on: each path in it is ASCII escapes that unquote_git_path decodes to exact bytes (a GitName storage key), and an added line's text is file content scanned for policy characters and conflict markers, which a replaced invalid byte cannot form
    (
        "codeflow-cli/src/cmd/ci.rs",
        "git_stdout",
        "from_utf8_lossy",
        1,
        "grammar:git-quoted-diff",
    ),
    // a git or tool error message shown to a person, never compared
    (
        "codeflow-cli/src/cmd/ci.rs",
        "git_with_stdin",
        "from_utf8_lossy",
        1,
        "display",
    ),
    // an object id or fixed ASCII word git prints, compared with ASCII only
    (
        "codeflow-cli/src/cmd/ci.rs",
        "rev_parse",
        "from_utf8_lossy",
        1,
        "ascii-marker",
    ),
    // a git or tool error message shown to a person, never compared
    (
        "codeflow-cli/src/cmd/ci/classification.rs",
        "name_status",
        "from_utf8_lossy",
        1,
        "display",
    ),
    // output of the guard canary run in this process, shown by doctor
    (
        "codeflow-cli/src/cmd/hook.rs",
        "exec_guard_canary",
        "from_utf8_lossy",
        2,
        "display",
    ),
    // a git or tool error message shown to a person, never compared
    (
        "codeflow-cli/src/cmd/push_set.rs",
        "commits_from",
        "from_utf8_lossy",
        1,
        "display",
    ),
    // process output shown to a person, never compared
    (
        "codeflow-cli/src/cmd/push_set.rs",
        "run_check_with",
        "from_utf8_lossy",
        4,
        "display",
    ),
    // a default value of a command option, written into generated reference text
    (
        "codeflow-cli/src/command_reference.rs",
        "description",
        "to_string_lossy",
        1,
        "display",
    ),
    // Blob text only classifies changed line regions by ASCII frontmatter keys and fixed status/Closeout headings; paths remain bytes and no record identity or target value is read.
    (
        "codeflow-core/src/ceremony/history.rs",
        "status_only",
        "from_utf8_lossy",
        2,
        "grammar:ceremony-regions",
    ),
    // probe output that doctor shows
    (
        "codeflow-core/src/doctor/mod.rs",
        "Options::do_exec",
        "from_utf8_lossy",
        2,
        "display",
    ),
    // probe output that doctor shows
    (
        "codeflow-core/src/doctor/mod.rs",
        "Options::do_exec_stdin",
        "from_utf8_lossy",
        2,
        "display",
    ),
    // process output shown to a person, never compared
    (
        "codeflow-core/src/doctor/mod.rs",
        "run_captured",
        "from_utf8_lossy",
        2,
        "display",
    ),
    // a git or tool error message shown to a person, never compared
    (
        "codeflow-core/src/git/ci.rs",
        "parse_pr_checks_output",
        "from_utf8_lossy",
        1,
        "display",
    ),
    // a conflict marker kind's own name, not a git name (git2 is in the file)
    (
        "codeflow-core/src/hooks/conflict_markers.rs",
        "check",
        "name",
        1,
        "format-contract",
    ),
    // a git or tool error message shown to a person, never compared
    (
        "codeflow-core/src/hooks/conflict_markers.rs",
        "marker_sizes",
        "from_utf8_lossy",
        1,
        "display",
    ),
    // Added line content is tested for fixed ASCII conflict markers; independent path_bytes become exact GitName storage keys before content decoding.
    (
        "codeflow-core/src/hooks/conflict_markers.rs",
        "staged",
        "from_utf8_lossy",
        1,
        "grammar:conflict-marker-lines",
    ),
    // a git or tool error message shown to a person, never compared
    (
        "codeflow-core/src/hooks/delegate_turn.rs",
        "signal_tmux",
        "from_utf8_lossy",
        1,
        "display",
    ),
    // decided in the file: a dirty name read lossily can only add a refusal, never remove one, and a lookalike is judged as the work it is
    (
        "codeflow-core/src/hooks/git_discard.rs",
        "lossy",
        "from_utf8_lossy",
        1,
        "schema-reject-only",
    ),
    // a component of a command word, which is valid UTF-8 text; tests only for glob syntax
    (
        "codeflow-core/src/hooks/git_guard.rs",
        "WordGlob::prefix",
        "to_string_lossy",
        1,
        "grammar:shell-glob",
    ),
    // a component of a command word, which is valid UTF-8 text; tests only for glob syntax
    (
        "codeflow-core/src/hooks/git_guard.rs",
        "expand_components",
        "to_string_lossy",
        1,
        "grammar:shell-glob",
    ),
    // a find -name pattern matched against a candidate name: a candidate that is not UTF-8 stays when the pattern has a single-character wildcard, so the lossy spelling decides only literal and `*` patterns, which it answers as the bytes would, except that a literal U+FFFD in the pattern also matches such a name, which only adds a candidate or a refusal
    (
        "codeflow-core/src/hooks/git_guard.rs",
        "find_name_matches",
        "to_string_lossy",
        2,
        "schema-reject-only",
    ),
    // compared with ASCII enforcement folder names, which U+FFFD never equals, so an invalid name is none of them
    (
        "codeflow-core/src/hooks/git_guard.rs",
        "in_enforcement_dir",
        "to_string_lossy",
        1,
        "ascii-marker",
    ),
    // a git or tool error message shown to a person, never compared
    (
        "codeflow-core/src/hooks/git_guard.rs",
        "read_alias",
        "from_utf8_lossy",
        1,
        "display",
    ),
    // a git or tool error message shown to a person, never compared
    (
        "codeflow-core/src/hooks/git_guard.rs",
        "read_branch_name",
        "from_utf8_lossy",
        1,
        "display",
    ),
    // Added content is tested by secret token patterns; file identities come independently from path_bytes and this text is never a command, path or name.
    (
        "codeflow-core/src/hooks/git_hook.rs",
        "scan_staged",
        "from_utf8_lossy",
        1,
        "grammar:secret-scan-lines",
    ),
    // an object id or fixed ASCII word git prints, compared with ASCII only (`hooks 3`)
    (
        "codeflow-core/src/hooks/orient.rs",
        "generate",
        "from_utf8_lossy",
        1,
        "ascii-marker",
    ),
    // git stderr in an error message shown to a person, never compared
    (
        "codeflow-core/src/ids/git.rs",
        "checked",
        "from_utf8_lossy",
        1,
        "display",
    ),
    // git stderr in an error message shown to a person, never compared
    (
        "codeflow-core/src/ids/git.rs",
        "checked_bytes",
        "from_utf8_lossy",
        1,
        "display",
    ),
    // a git or tool error message shown to a person, never compared
    (
        "codeflow-core/src/ids/issue.rs",
        "fetch",
        "from_utf8_lossy",
        1,
        "display",
    ),
    // process output shown to a person, never compared
    (
        "codeflow-core/src/ids/issue.rs",
        "push",
        "from_utf8_lossy",
        2,
        "display",
    ),
    // a git or tool error message shown to a person, never compared
    (
        "codeflow-core/src/integrate.rs",
        "checkout",
        "from_utf8_lossy",
        1,
        "display",
    ),
    // an error message and the list of dirty files shown in a refusal, never compared
    (
        "codeflow-core/src/integrate.rs",
        "dirty_files",
        "from_utf8_lossy",
        2,
        "display",
    ),
    // a git or tool error message shown to a person, never compared
    (
        "codeflow-core/src/integrate.rs",
        "integrate",
        "from_utf8_lossy",
        1,
        "display",
    ),
    // a git or tool error message shown to a person, never compared
    (
        "codeflow-core/src/integrate.rs",
        "restore_checkout",
        "from_utf8_lossy",
        1,
        "display",
    ),
    // a path shown to a person, decoded from its reversible encoding (the encoding is the identity)
    (
        "codeflow-core/src/recall.rs",
        "display_encoded_path",
        "from_utf8_lossy",
        1,
        "display",
    ),
    // a path shown to a person; the reversible encoding is the identity
    (
        "codeflow-core/src/recall.rs",
        "display_rel_to",
        "to_string_lossy",
        1,
        "display",
    ),
    // Windows only: the readable part of the key of a path that is not text, followed by `%00` and the hex of its exact encoding, so the key is injective
    (
        "codeflow-core/src/recall.rs",
        "encode_path",
        "to_string_lossy",
        1,
        "format-contract",
    ),
    // process output shown to a person, never compared
    (
        "codeflow-core/src/release_local.rs",
        "failure",
        "from_utf8_lossy",
        2,
        "display",
    ),
    // Only unsuccessful gh stdout/stderr is displayed; successful output carrying repository identity is decoded strictly and errors refuse API dispatch.
    (
        "codeflow-core/src/remote.rs",
        "GithubProvider::run_gh",
        "from_utf8_lossy",
        2,
        "display",
    ),
    // BranchStep::name returns its existing &str unchanged for display; this is not a git2 name decoder.
    (
        "codeflow-core/src/root_checkout.rs",
        "WorkspaceReport::fmt",
        "name",
        1,
        "format-contract",
    ),
    // an object id or fixed ASCII word git prints, compared with ASCII only (`true`)
    (
        "codeflow-core/src/root_checkout.rs",
        "effective_ignore_case",
        "from_utf8_lossy",
        1,
        "ascii-marker",
    ),
    // BranchStep::name returns the already validated branch String by reference unchanged; no native bytes are decoded or replaced.
    (
        "codeflow-core/src/root_checkout.rs",
        "finish",
        "name",
        1,
        "format-contract",
    ),
    // a git or tool error message shown to a person, never compared
    (
        "codeflow-core/src/root_checkout.rs",
        "git_run",
        "from_utf8_lossy",
        1,
        "display",
    ),
    // Failed-process stderr is display-only; it never supplies a config value or an absence answer.
    (
        "codeflow-core/src/scaffold/gitutil.rs",
        "failed",
        "from_utf8_lossy",
        1,
        "display",
    ),
    // a git or tool error message shown to a person, never compared
    (
        "codeflow-core/src/scaffold/gitutil.rs",
        "add_and_commit",
        "from_utf8_lossy",
        1,
        "display",
    ),
    // a git or tool error message shown to a person, never compared
    (
        "codeflow-core/src/scaffold/gitutil.rs",
        "git_ok",
        "from_utf8_lossy",
        1,
        "display",
    ),
    // a name that is not UTF-8 makes the deletion unproven when the glob has a single-character wildcard or a bracket; the lossy spelling decides only literal and `*` patterns, which it answers as the bytes would, except that a literal U+FFFD in the pattern also matches such a name, which only adds a candidate or a refusal
    (
        "codeflow-core/src/security/deletion.rs",
        "Reader::glob_paths",
        "to_string_lossy",
        1,
        "schema-reject-only",
    ),
    // output of a sandbox probe, shown in a report
    (
        "codeflow-core/src/testing/delivery.rs",
        "observation",
        "from_utf8_lossy",
        2,
        "display",
    ),
    // a git or tool error message shown to a person, never compared
    (
        "codeflow-core/src/testing/delivery.rs",
        "preflight",
        "from_utf8_lossy",
        1,
        "display",
    ),
    // The parser takes &str after parse_junit read_to_string; quick_xml attributes/text are slices of validated UTF-8 source, including testcase names and classnames.
    (
        "codeflow-core/src/testing/report/junit.rs",
        "parse_junit_str",
        "from_utf8_lossy",
        15,
        "format-contract",
    ),
    // a test target's captured output, shown
    (
        "codeflow-core/src/testing/runner/mod.rs",
        "render_capture",
        "from_utf8_lossy",
        1,
        "display",
    ),
    // a test target's output scanned for progress words
    (
        "codeflow-core/src/testing/runner/mod.rs",
        "spawn_reader_with_progress",
        "from_utf8_lossy",
        1,
        "grammar:test-progress",
    ),
    // an object id or fixed ASCII word git prints, compared with ASCII only
    (
        "codeflow-core/src/testing/validation.rs",
        "default_base_ref",
        "from_utf8_lossy",
        1,
        "ascii-marker",
    ),
    // Lossy document text only locates the line number printed with an already determined finding; it does not select the finding, parsed record identity or target.
    (
        "codeflow-core/src/validate/docs.rs",
        "find_line",
        "from_utf8_lossy",
        1,
        "display",
    ),
    // commit subjects (message text) searched for a task id
    (
        "codeflow-core/src/validate/mod.rs",
        "landed_pull_requests",
        "from_utf8_lossy",
        1,
        "grammar:merge-subject",
    ),
    // Body bytes are returned by strict parse_frontmatter from a UTF-8 str; this conversion cannot replace undecodable bytes and only checks required Markdown headings.
    (
        "codeflow-core/src/validate/mod.rs",
        "validate_sections",
        "from_utf8_lossy",
        1,
        "format-contract",
    ),
    // Body bytes come from strict parse_frontmatter; acceptance evidence/checkbox parsing therefore consumes already validated UTF-8, never lossy record identities.
    (
        "codeflow-core/src/validate/mod.rs",
        "validate_task",
        "from_utf8_lossy",
        1,
        "format-contract",
    ),
    // a git or tool error message shown to a person, never compared
    (
        "codeflow-core/src/validate/portal.rs",
        "git_output_bounded",
        "from_utf8_lossy",
        1,
        "display",
    ),
    // RecordView::parse takes &str; strict parse_frontmatter returns body bytes sliced from that validated input, so this conversion cannot replace identity or target bytes.
    (
        "codeflow-core/src/workgraph/lifecycle.rs",
        "RecordView::parse",
        "from_utf8_lossy",
        1,
        "format-contract",
    ),
    // an error message and record content; the paths in the log are read as exact bytes
    (
        "codeflow-core/src/workgraph/lifecycle.rs",
        "shipped_in_history",
        "from_utf8_lossy",
        1,
        "display",
    ),
    // Git stderr diagnostic only; stdout now uses strict decoding.
    (
        "codeflow-core/src/workgraph/light_paths.rs",
        "git",
        "from_utf8_lossy",
        1,
        "display",
    ),
    // Git stderr diagnostic only; stdout now uses strict decoding.
    (
        "codeflow-core/src/workgraph/readiness.rs",
        "git",
        "from_utf8_lossy",
        1,
        "display",
    ),
    // a git or tool error message shown to a person, never compared
    (
        "codeflow-core/src/workgraph/readiness.rs",
        "git_bytes",
        "from_utf8_lossy",
        1,
        "display",
    ),
    // Windows only: PowerShell output arrives as U+FFFD for an invalid unit, so refuse_ambiguous_identity_text stops the one case where the lossy text could equal the expected profile path, and the instance argument is a random UUID, exact either way
    (
        "codeflow-present/src/browser.rs",
        "windows_output_text",
        "from_utf8_lossy",
        1,
        "schema-reject-only",
    ),
    // curl's stderr after a failed download, shown in the refusal only; the downloaded bytes are returned unchanged
    (
        "codeflow-core/src/scaffold/release_pin.rs",
        "fetch_with_curl",
        "from_utf8_lossy",
        1,
        "display",
    ),
    // git clone's stderr after a failed release clone, shown in the refusal only
    (
        "codeflow-cli/examples/release_integration.rs",
        "prepare",
        "from_utf8_lossy",
        1,
        "display",
    ),
    // a fixture clone's stderr in its panic message and its stdout returned to tests that show or discard it; neither is compared as a name
    (
        "codeflow-fixture/src/lib.rs",
        "Clone::run",
        "from_utf8_lossy",
        2,
        "display",
    ),
];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if name != "tests" && name != "target" {
                rust_files(&path, out);
            }
        } else if path.extension().is_some_and(|ext| ext == "rs")
            && name != "tests.rs"
            && !name.ends_with("_tests.rs")
        {
            out.push(path);
        }
    }
}

/// Whether an attribute is `#[cfg(test)]` or `#[cfg(all(test, ...))]`.
fn is_cfg_test(attribute: &syn::Attribute) -> bool {
    if !attribute.path().is_ident("cfg") {
        return false;
    }
    let Ok(meta) = attribute.parse_args::<syn::Meta>() else {
        return false;
    };
    match meta {
        syn::Meta::Path(path) => path.is_ident("test"),
        syn::Meta::List(list) if list.path.is_ident("all") => list
            .parse_args_with(
                syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
            )
            .is_ok_and(|parts| {
                parts
                    .iter()
                    .any(|part| matches!(part, syn::Meta::Path(p) if p.is_ident("test")))
            }),
        _ => false,
    }
}

fn skipped(attributes: &[syn::Attribute]) -> bool {
    attributes.iter().any(is_cfg_test)
}

fn item_attributes(item: &syn::Item) -> &[syn::Attribute] {
    use syn::Item::{
        Const, Enum, ExternCrate, Fn, ForeignMod, Impl, Macro, Mod, Static, Struct, Trait,
        TraitAlias, Type, Union, Use,
    };
    match item {
        Const(i) => &i.attrs,
        Enum(i) => &i.attrs,
        ExternCrate(i) => &i.attrs,
        Fn(i) => &i.attrs,
        ForeignMod(i) => &i.attrs,
        Impl(i) => &i.attrs,
        Macro(i) => &i.attrs,
        Mod(i) => &i.attrs,
        Static(i) => &i.attrs,
        Struct(i) => &i.attrs,
        Trait(i) => &i.attrs,
        TraitAlias(i) => &i.attrs,
        Type(i) => &i.attrs,
        Union(i) => &i.attrs,
        Use(i) => &i.attrs,
        _ => &[],
    }
}

fn impl_item_attributes(item: &syn::ImplItem) -> &[syn::Attribute] {
    match item {
        syn::ImplItem::Const(i) => &i.attrs,
        syn::ImplItem::Fn(i) => &i.attrs,
        syn::ImplItem::Type(i) => &i.attrs,
        syn::ImplItem::Macro(i) => &i.attrs,
        _ => &[],
    }
}

fn trait_item_attributes(item: &syn::TraitItem) -> &[syn::Attribute] {
    match item {
        syn::TraitItem::Const(i) => &i.attrs,
        syn::TraitItem::Fn(i) => &i.attrs,
        syn::TraitItem::Type(i) => &i.attrs,
        syn::TraitItem::Macro(i) => &i.attrs,
        _ => &[],
    }
}

/// One site: the enclosing item, the call, and the first line seen.
#[derive(Default)]
struct Sites {
    absence: bool,
    whitespace: bool,
    /// The file uses git2, so a zero-argument `name()` may be its text.
    uses_git2: bool,
    context: Vec<String>,
    option_return: Vec<bool>,
    seen: std::collections::BTreeSet<(usize, usize, String)>,
    /// (item, call) to the lines it was seen at.
    found: BTreeMap<(String, String), Vec<usize>>,
}

impl Sites {
    fn note(&mut self, span: proc_macro2::Span, call: &str) {
        let at = span.start();
        if !self.seen.insert((at.line, at.column, call.to_string())) {
            return;
        }
        let item = if self.context.is_empty() {
            "<file>".to_string()
        } else {
            self.context.join("::")
        };
        self.found
            .entry((item, call.to_string()))
            .or_default()
            .push(span.start().line);
    }

    fn flagged(&self, name: &str, arguments: usize) -> bool {
        if self.absence {
            return ABSENCE_METHODS.contains(&name);
        }
        if self.whitespace {
            return UNICODE_WHITESPACE.contains(&name);
        }
        LOSSY.contains(&name)
            || (GIT2_TEXT.contains(&name) && arguments == 0)
            || (name == "name" && arguments == 0 && self.uses_git2)
    }

    /// The idents of a macro's tokens, so `format!("{}", f(x))` is read too.
    fn scan_tokens(&mut self, tokens: proc_macro2::TokenStream) {
        if self.absence {
            let tokens: Vec<_> = tokens.into_iter().collect();
            for (at, token) in tokens.iter().enumerate() {
                if let proc_macro2::TokenTree::Group(group) = token {
                    self.scan_tokens(group.stream());
                }
                if let proc_macro2::TokenTree::Ident(ident) = token {
                    let punct = |index: usize, ch: char| matches!(tokens.get(index), Some(proc_macro2::TokenTree::Punct(p)) if p.as_char() == ch);
                    if ident == "NotFound"
                        && at >= 3
                        && punct(at - 1, ':')
                        && punct(at - 2, ':')
                        && matches!(&tokens[at - 3], proc_macro2::TokenTree::Ident(kind) if kind == "ErrorKind")
                    {
                        self.note(ident.span(), "ErrorKind::NotFound");
                    }
                    if ABSENCE_METHODS.contains(&ident.to_string().as_str())
                        && at > 0
                        && (punct(at - 1, '.') || punct(at - 1, ':'))
                    {
                        self.note(ident.span(), &ident.to_string());
                    }
                }
            }
            return;
        }
        let mut previous: Option<proc_macro2::TokenTree> = None;
        let mut iter = tokens.into_iter().peekable();
        while let Some(token) = iter.next() {
            match &token {
                proc_macro2::TokenTree::Group(group) => {
                    self.scan_tokens(group.stream());
                }
                proc_macro2::TokenTree::Ident(ident) => {
                    let name = ident.unraw().to_string();
                    let empty_call = matches!(
                        iter.peek(),
                        Some(proc_macro2::TokenTree::Group(group))
                            if group.delimiter() == proc_macro2::Delimiter::Parenthesis
                                && group.stream().is_empty()
                    );
                    let after_dot = matches!(
                        &previous,
                        Some(proc_macro2::TokenTree::Punct(punct)) if punct.as_char() == '.'
                    );
                    let arguments = usize::from(!empty_call);
                    // Zero-argument names count only as a method (`x.name()`).
                    if self.flagged(&name, arguments)
                        && (after_dot
                            || LOSSY.contains(&name.as_str())
                            || matches!(iter.peek(), Some(proc_macro2::TokenTree::Group(_))))
                    {
                        self.note(ident.span(), &name);
                    }
                }
                _ => {}
            }
            previous = Some(token);
        }
    }
}

impl<'ast> Visit<'ast> for Sites {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        if skipped(item_attributes(item)) {
            return;
        }
        match item {
            syn::Item::Fn(function) => {
                self.context.push(function.sig.ident.unraw().to_string());
                self.option_return
                    .push(returns_option(&function.sig.output));
                syn::visit::visit_item_fn(self, function);
                self.option_return.pop();
                self.context.pop();
            }
            syn::Item::Const(item) => {
                self.context.push(item.ident.unraw().to_string());
                syn::visit::visit_item_const(self, item);
                self.context.pop();
            }
            syn::Item::Static(item) => {
                self.context.push(item.ident.unraw().to_string());
                syn::visit::visit_item_static(self, item);
                self.context.pop();
            }
            syn::Item::Impl(item) => {
                let name = match &*item.self_ty {
                    syn::Type::Path(path) => path
                        .path
                        .segments
                        .last()
                        .map_or_else(|| "impl".to_string(), |s| s.ident.unraw().to_string()),
                    _ => "impl".to_string(),
                };
                self.context.push(name);
                syn::visit::visit_item_impl(self, item);
                self.context.pop();
            }
            syn::Item::Trait(item) => {
                self.context.push(item.ident.unraw().to_string());
                syn::visit::visit_item_trait(self, item);
                self.context.pop();
            }
            syn::Item::Mod(item) => {
                self.context.push(item.ident.unraw().to_string());
                syn::visit::visit_item_mod(self, item);
                self.context.pop();
            }
            _ => syn::visit::visit_item(self, item),
        }
    }

    fn visit_impl_item(&mut self, item: &'ast syn::ImplItem) {
        if skipped(impl_item_attributes(item)) {
            return;
        }
        if let syn::ImplItem::Fn(function) = item {
            self.context.push(function.sig.ident.unraw().to_string());
            self.option_return
                .push(returns_option(&function.sig.output));
            syn::visit::visit_impl_item_fn(self, function);
            self.option_return.pop();
            self.context.pop();
        } else {
            syn::visit::visit_impl_item(self, item);
        }
    }

    fn visit_trait_item(&mut self, item: &'ast syn::TraitItem) {
        if skipped(trait_item_attributes(item)) {
            return;
        }
        if let syn::TraitItem::Fn(function) = item {
            self.context.push(function.sig.ident.unraw().to_string());
            self.option_return
                .push(returns_option(&function.sig.output));
            syn::visit::visit_trait_item_fn(self, function);
            self.option_return.pop();
            self.context.pop();
        } else {
            syn::visit::visit_trait_item(self, item);
        }
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        let name = call.method.unraw().to_string();
        if self.flagged(&name, call.args.len())
            || (self.whitespace
                && PREDICATE_TRIM.contains(&name.as_str())
                && call
                    .args
                    .first()
                    .is_some_and(|arg| !literal_pattern(arg) || framing_pattern(arg)))
        {
            self.note(call.method.span(), &name);
        }
        if self.whitespace && ABSENT.contains(&name.as_str()) {
            if let Some((span, name)) = obtain_in_chain(&call.receiver) {
                self.note(span, &format!("obtain-absent:{name}"));
            }
        }
        if self.whitespace && matches!(name.as_str(), "filter_map" | "flat_map") {
            for argument in &call.args {
                if let syn::Expr::Path(path) = argument {
                    let segments: Vec<_> = path.path.segments.iter().collect();
                    if segments.len() >= 2
                        && segments[segments.len() - 2].ident == "Result"
                        && segments[segments.len() - 1].ident == "ok"
                    {
                        self.note(
                            segments[segments.len() - 1].ident.span(),
                            "obtain-absent:Result::ok",
                        );
                    }
                }
            }
        }
        syn::visit::visit_expr_method_call(self, call);
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if self.whitespace {
            if let syn::Expr::Path(path) = &*call.func {
                if path
                    .path
                    .segments
                    .last()
                    .is_some_and(|s| PREDICATE_TRIM.contains(&s.ident.to_string().as_str()))
                    && call
                        .args
                        .last()
                        .is_some_and(|arg| literal_pattern(arg) && !framing_pattern(arg))
                {
                    for argument in &call.args {
                        self.visit_expr(argument);
                    }
                    return;
                }
            }
        }
        syn::visit::visit_expr_call(self, call);
    }

    fn visit_expr_try(&mut self, expr: &'ast syn::ExprTry) {
        if self.whitespace && self.option_return.last() == Some(&true) {
            if let Some((span, name)) = obtain_in_chain(&expr.expr) {
                self.note(span, &format!("obtain-absent:{name}"));
            }
        }
        syn::visit::visit_expr_try(self, expr);
    }

    fn visit_expr_if(&mut self, expr: &'ast syn::ExprIf) {
        if self.whitespace {
            if let syn::Expr::Let(binding) = &*expr.cond {
                if variant_pattern(&binding.pat, "Ok", false) {
                    if let Some((span, name)) = obtain_in_chain(&binding.expr) {
                        self.note(span, &format!("obtain-absent:{name}"));
                    }
                }
            }
        }
        syn::visit::visit_expr_if(self, expr);
    }

    fn visit_expr_match(&mut self, expr: &'ast syn::ExprMatch) {
        if self.whitespace
            && expr
                .arms
                .iter()
                .any(|arm| variant_pattern(&arm.pat, "Err", true))
        {
            if let Some((span, name)) = obtain_in_chain(&expr.expr) {
                self.note(span, &format!("obtain-absent:{name}"));
            }
        }
        syn::visit::visit_expr_match(self, expr);
    }

    fn visit_local(&mut self, local: &'ast syn::Local) {
        if self.whitespace && matches!(local.pat, syn::Pat::Wild(_)) {
            if let Some(init) = &local.init {
                if let Some((span, name)) = obtain_in_chain(&init.expr) {
                    self.note(span, &format!("obtain-absent:{name}"));
                }
            }
        }
        syn::visit::visit_local(self, local);
    }

    fn visit_path(&mut self, path: &'ast syn::Path) {
        if self.absence {
            if let Some(last) = path.segments.last() {
                if ABSENCE_METHODS.contains(&last.ident.to_string().as_str())
                    && path.segments.len() > 1
                {
                    self.note(last.ident.span(), &last.ident.to_string());
                }
                if last.ident == "NotFound"
                    && path.segments.len() > 1
                    && path.segments[path.segments.len() - 2].ident == "ErrorKind"
                {
                    self.note(last.ident.span(), "ErrorKind::NotFound");
                }
            }
            syn::visit::visit_path(self, path);
            return;
        }
        // `String::from_utf8_lossy(x)` and `map(String::from_utf8_lossy)`.
        if let Some(last) = path.segments.last() {
            let name = last.ident.unraw().to_string();
            if (self.whitespace
                && path.segments.len() > 1
                && (UNICODE_WHITESPACE.contains(&name.as_str())
                    || PREDICATE_TRIM.contains(&name.as_str())))
                || (!self.whitespace && LOSSY.contains(&name.as_str()))
            {
                self.note(last.ident.span(), &name);
            }
        }
        syn::visit::visit_path(self, path);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        use syn::parse::Parser;
        if self.whitespace || self.absence {
            if let Ok(args) =
                syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated
                    .parse2(mac.tokens.clone())
            {
                for arg in &args {
                    self.visit_expr(arg);
                }
                return;
            }
        }
        self.scan_tokens(mac.tokens.clone());
        syn::visit::visit_macro(self, mac);
    }
}

fn returns_option(output: &syn::ReturnType) -> bool {
    matches!(output, syn::ReturnType::Type(_, ty) if matches!(&**ty, syn::Type::Path(path) if path.path.segments.last().is_some_and(|s| s.ident == "Option")))
}

fn literal_pattern(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::Lit(lit) => matches!(lit.lit, syn::Lit::Char(_) | syn::Lit::Str(_)),
        syn::Expr::Array(array) => array.elems.iter().all(literal_pattern),
        syn::Expr::Reference(expr) => literal_pattern(&expr.expr),
        syn::Expr::Paren(expr) => literal_pattern(&expr.expr),
        _ => false,
    }
}

fn framing_pattern(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::Lit(lit) => match &lit.lit {
            syn::Lit::Char(c) => matches!(c.value(), '\'' | '"' | '\\' | '\n'),
            syn::Lit::Str(s) => s.value().contains(['\'', '"', '\\', '\n']),
            _ => false,
        },
        syn::Expr::Array(array) => array.elems.iter().any(framing_pattern),
        syn::Expr::Reference(expr) => framing_pattern(&expr.expr),
        syn::Expr::Paren(expr) => framing_pattern(&expr.expr),
        _ => false,
    }
}

fn variant_pattern(pattern: &syn::Pat, variant: &str, wildcard: bool) -> bool {
    match pattern {
        syn::Pat::TupleStruct(pattern) => {
            pattern
                .path
                .segments
                .last()
                .is_some_and(|s| s.ident == variant)
                && (!wildcard
                    || pattern
                        .elems
                        .iter()
                        .any(|p| matches!(p, syn::Pat::Wild(_) | syn::Pat::Rest(_))))
        }
        syn::Pat::Or(pattern) => pattern
            .cases
            .iter()
            .any(|p| variant_pattern(p, variant, wildcard)),
        syn::Pat::Paren(pattern) => variant_pattern(&pattern.pat, variant, wildcard),
        _ => false,
    }
}

fn obtain_in_chain(expr: &syn::Expr) -> Option<(proc_macro2::Span, String)> {
    match expr {
        syn::Expr::MethodCall(call) => {
            let name = call.method.unraw().to_string();
            if obtaining_method(&name) {
                Some((call.method.span(), name))
            } else {
                obtain_in_chain(&call.receiver)
            }
        }
        syn::Expr::Call(call) => match &*call.func {
            syn::Expr::Path(path) => {
                let segments: Vec<_> = path.path.segments.iter().collect();
                let segment = segments.last()?;
                let name = segment.ident.unraw().to_string();
                let owner = segments
                    .len()
                    .checked_sub(2)
                    .map(|at| segments[at].ident.unraw().to_string());
                if obtaining_method(&name) {
                    return Some((segment.ident.span(), name));
                }
                owner
                    .filter(|owner| OBTAIN_QUALIFIED.contains(&(owner.as_str(), name.as_str())))
                    .map(|owner| (segment.ident.span(), format!("{owner}::{name}")))
            }
            _ => None,
        },
        syn::Expr::Paren(expr) => obtain_in_chain(&expr.expr),
        syn::Expr::Try(expr) => obtain_in_chain(&expr.expr),
        _ => None,
    }
}

fn valid_reason(reason: &str) -> bool {
    matches!(
        reason,
        "display"
            | "prose"
            | "schema-reject-only"
            | "format-contract"
            | "ascii-marker"
            | "unproven"
    ) || reason
        .strip_prefix("grammar:")
        .or_else(|| reason.strip_prefix("framing:"))
        .is_some_and(|name| {
            !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

/// Every site in `source`, as ((item, call), lines). `file` only decides
/// whether git2 is in use.
fn sites_in(source: &str) -> BTreeMap<(String, String), Vec<usize>> {
    sites_in_mode(source, false)
}

fn sites_in_mode(source: &str, whitespace: bool) -> BTreeMap<(String, String), Vec<usize>> {
    let file = syn::parse_file(source).expect("the source parses");
    let mut sites = Sites {
        whitespace,
        uses_git2: source.contains("git2"),
        ..Sites::default()
    };
    // A file marked `#![cfg(test)]` is test code throughout.
    if !skipped(&file.attrs) {
        sites.visit_file(&file);
    }
    sites.found
}

fn relative(root: &Path, file: &Path) -> String {
    let path = file
        .strip_prefix(root)
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/")
        .replace("/../", "/");
    path.trim_start_matches("./").to_string()
}

#[test]
fn every_lossy_decode_of_a_name_is_listed_with_its_reason() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);
    assert!(files.len() > 100, "the scan found the workspace sources");
    let mut actual: BTreeMap<(String, String, String), usize> = BTreeMap::new();
    let mut lines: BTreeMap<(String, String, String), Vec<usize>> = BTreeMap::new();
    for file in files {
        let name = relative(&root, &file);
        // The layer itself, and a file the scan reads as part of the layer's
        // own claim, is not scanned.
        if name == LAYER {
            continue;
        }
        let text = std::fs::read_to_string(&file).unwrap();
        let found = std::panic::catch_unwind(|| sites_in(&text))
            .unwrap_or_else(|_| panic!("{name} does not parse as Rust"));
        for ((item, call), at) in found {
            let key = (name.trim_start_matches("crates/").to_string(), item, call);
            *actual.entry(key.clone()).or_default() += at.len();
            lines.entry(key).or_default().extend(at);
        }
    }
    if std::env::var_os("NAME_SCAN_LIST").is_some() {
        for ((file, item, call), count) in &actual {
            println!(
                "{file}\t{item}\t{call}\t{count}\t{:?}",
                lines[&(file.clone(), item.clone(), call.clone())]
            );
        }
    }
    let mut allowed: BTreeMap<(String, String, String), usize> = BTreeMap::new();
    for (file, item, call, count, reason) in EXCEPTIONS {
        assert!(
            valid_reason(reason),
            "{file} {item} {call}: invalid reason {reason}"
        );
        let key = (
            (*file).to_string(),
            (*item).to_string(),
            (*call).to_string(),
        );
        assert!(
            allowed.insert(key, *count).is_none(),
            "{file} {item} {call} is listed twice"
        );
    }
    let mut problems = Vec::new();
    for (key, count) in &actual {
        match allowed.get(key) {
            None => problems.push(format!(
                "{}: `{}` in `{}` ({count}x, lines {:?}) decodes a name lossily or as text: \
                 keep the bytes in a GitName and use display() to show it, or rule_text() \
                 where a rule needs text, or list it in EXCEPTIONS with the reason",
                key.0, key.2, key.1, lines[key]
            )),
            Some(listed) if listed != count => problems.push(format!(
                "{}: `{}` in `{}` is listed {listed}x but found {count}x (lines {:?})",
                key.0, key.2, key.1, lines[key]
            )),
            Some(_) => {}
        }
    }
    for (key, count) in &allowed {
        if !actual.contains_key(key) {
            problems.push(format!(
                "{}: the exception for `{}` in `{}` ({count}x) is stale: remove it",
                key.0, key.2, key.1
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

fn call_names(source: &str) -> Vec<(String, String)> {
    sites_in(source).into_keys().collect()
}

fn pair(item: &str, call: &str) -> (String, String) {
    (item.to_string(), call.to_string())
}

#[test]
fn the_scan_finds_each_way_to_decode_a_name_lossily() {
    for (source, call) in [
        (
            "fn f() { let s = String::from_utf8_lossy(b); }",
            "from_utf8_lossy",
        ),
        (
            "fn f() { let s = std::string::String::from_utf8_lossy(b); }",
            "from_utf8_lossy",
        ),
        (
            "fn f() { let s = b.iter().map(String::from_utf8_lossy); }",
            "from_utf8_lossy",
        ),
        (
            "fn f() { let s = path.to_string_lossy(); }",
            "to_string_lossy",
        ),
        (
            "fn f() { let s = path . to_string_lossy (\n); }",
            "to_string_lossy",
        ),
        (
            "fn f() { println!(\"{}\", String::from_utf8_lossy(b)); }",
            "from_utf8_lossy",
        ),
        (
            "fn f() { let s = format!(\"{}\", p.to_string_lossy()); }",
            "to_string_lossy",
        ),
        (
            "fn f() { vec![String::from_utf8_lossy(b)]; }",
            "from_utf8_lossy",
        ),
        (
            "fn f() { let s = r#try.to_string_lossy(); }",
            "to_string_lossy",
        ),
    ] {
        assert_eq!(call_names(source), [pair("f", call)], "{source}");
    }
}

#[test]
fn the_scan_finds_git2_text_but_not_every_name_method() {
    let git2 = "use git2::Repository;\n";
    assert_eq!(
        call_names(&format!("{git2}fn f() {{ r.shorthand(); }}")),
        [pair("f", "shorthand")]
    );
    assert_eq!(
        call_names(&format!("{git2}fn f() {{ r.symbolic_target(); }}")),
        [pair("f", "symbolic_target")]
    );
    assert_eq!(
        call_names(&format!("{git2}fn f() {{ r.name(); }}")),
        [pair("f", "name")]
    );
    // The bytes accessors and a `name` with arguments are not text reads.
    assert!(call_names(&format!("{git2}fn f() {{ r.name_bytes(); r.name(1); }}")).is_empty());
    // A file that does not use git2 may have its own `name()`.
    assert!(call_names("fn f() { r.name(); }").is_empty());
}

#[test]
fn the_scan_keys_a_site_by_its_enclosing_item() {
    let source = "
        struct S;
        impl S {
            fn method(&self) { String::from_utf8_lossy(b); }
        }
        mod inner {
            fn nested() { p.to_string_lossy(); }
        }
        const C: () = { String::from_utf8_lossy(b); };
        trait T { fn provided() { String::from_utf8_lossy(b); } }
    ";
    assert_eq!(
        call_names(source),
        [
            pair("C", "from_utf8_lossy"),
            pair("S::method", "from_utf8_lossy"),
            pair("T::provided", "from_utf8_lossy"),
            pair("inner::nested", "to_string_lossy"),
        ]
    );
}

#[test]
fn the_scan_counts_each_call_in_an_item() {
    let sites = sites_in("fn f() { a.to_string_lossy(); b.to_string_lossy(); }");
    assert_eq!(sites[&pair("f", "to_string_lossy")].len(), 2);
}

#[test]
fn the_scan_skips_test_code_one_item_at_a_time() {
    let source = "
        #[cfg(test)]
        fn only_in_tests() { String::from_utf8_lossy(b); }
        #[cfg(all(test, unix))]
        mod tests { fn t() { String::from_utf8_lossy(b); } }
        struct S;
        impl S {
            #[cfg(test)]
            fn helper(&self) { String::from_utf8_lossy(b); }
            fn production(&self) { String::from_utf8_lossy(b); }
        }
        fn after() { String::from_utf8_lossy(b); }
    ";
    assert_eq!(
        call_names(source),
        [
            pair("S::production", "from_utf8_lossy"),
            pair("after", "from_utf8_lossy"),
        ]
    );
    assert!(call_names("#![cfg(test)]\nfn f() { String::from_utf8_lossy(b); }").is_empty());
}

#[test]
fn the_scan_does_not_flag_the_strict_decodes() {
    assert!(call_names(
        "fn f() { String::from_utf8(b); std::str::from_utf8(b); p.to_str(); p.display(); }"
    )
    .is_empty());
}

// Exceptions concern prose, diagnostics, schema validation, or a language
// whose lexical grammar explicitly includes Unicode separators. Counts reject both new calls and stale exceptions.
const WHITESPACE_EXCEPTIONS: &[(&str, &str, &str, usize, &str)] = &[
    // Formats Git stderr after command failure, never as an operand or authority.
    (
        "crates/codeflow-cli/src/cmd/ci.rs",
        "base_refusal",
        "lines",
        1,
        "display",
    ),
    // Formats Git stderr after command failure, never as an operand or authority.
    (
        "crates/codeflow-cli/src/cmd/ci.rs",
        "base_refusal",
        "trim",
        1,
        "display",
    ),
    // Reads conventional commit subject/footer line boundaries.
    (
        "crates/codeflow-cli/src/cmd/ci.rs",
        "breaking_marker",
        "lines",
        2,
        "framing:commit-marker-lines",
    ),
    // Formats Git stderr after command failure, never as an operand or authority.
    (
        "crates/codeflow-cli/src/cmd/ci.rs",
        "enumerate_commits",
        "trim",
        1,
        "display",
    ),
    // Reads rendered PR template lines before rejecting unresolved placeholders.
    (
        "crates/codeflow-cli/src/cmd/ci.rs",
        "find_placeholders",
        "lines",
        1,
        "framing:template-placeholder-lines",
    ),
    // Finds unfilled Markdown template text and empty cells. Broad whitespace increases rejection without normalizing retained identifiers.
    (
        "crates/codeflow-cli/src/cmd/ci.rs",
        "find_placeholders",
        "split_whitespace",
        1,
        "schema-reject-only",
    ),
    // Finds unfilled Markdown template text and empty cells. Broad whitespace increases rejection without normalizing retained identifiers.
    (
        "crates/codeflow-cli/src/cmd/ci.rs",
        "find_placeholders",
        "trim",
        2,
        "schema-reject-only",
    ),
    // Formats Git stderr after command failure, never as an operand or authority.
    (
        "crates/codeflow-cli/src/cmd/ci.rs",
        "git_bytes",
        "trim",
        1,
        "display",
    ),
    // Formats Git stderr after command failure, never as an operand or authority.
    (
        "crates/codeflow-cli/src/cmd/ci.rs",
        "git_with_stdin",
        "trim",
        1,
        "display",
    ),
    // Finds unfilled Markdown template text and empty cells. Broad whitespace increases rejection without normalizing retained identifiers.
    (
        "crates/codeflow-cli/src/cmd/ci.rs",
        "is_empty_table_row",
        "trim",
        1,
        "schema-reject-only",
    ),
    // Octal escape digits that are not UTF-8 make unquote_git_path return unproven None; diff_path turns None into a cannot-read Err, so evaluate_commit_range marks added-lines skipped (ci exits 2) and conflict_markers::dispatch reports an incomplete finding at its level.
    (
        "crates/codeflow-cli/src/cmd/ci.rs",
        "unquote_git_path",
        "obtain-absent:from_utf8",
        1,
        "unproven",
    ),
    // Reads Markdown Task declaration line boundaries once before rejecting an existing declaration.
    (
        "crates/codeflow-cli/src/cmd/ci/adopter.rs",
        "supply_task",
        "lines",
        1,
        "framing:pr-task-lines",
    ),
    // Existing Task: prose prevents profile injection and leaves the original body for strict classification.
    (
        "crates/codeflow-cli/src/cmd/ci/adopter.rs",
        "supply_task",
        "trim_start",
        1,
        "schema-reject-only",
    ),
    // parse_raw returns unproven None for raw metadata that is not UTF-8 (git's own ASCII modes and status); range_inventory converts None to cannot-read Err and ci::run refuses. The path field is kept as GitName bytes, not decoded.
    (
        "crates/codeflow-cli/src/cmd/ci/change_class.rs",
        "parse_raw",
        "obtain-absent:from_utf8",
        1,
        "unproven",
    ),
    // Formats Git stderr only after command failure.
    (
        "crates/codeflow-cli/src/cmd/ci/classification.rs",
        "name_status",
        "trim",
        1,
        "display",
    ),
    // Reads Markdown Task declaration line boundaries; returned identities keep explicit token text.
    (
        "crates/codeflow-cli/src/cmd/ci/classification.rs",
        "task_lines",
        "lines",
        1,
        "framing:classification-task-lines",
    ),
    // HTML container parser removes the explicit HTML ASCII whitespace set before reading a trailing slash marker.
    (
        "crates/codeflow-cli/src/cmd/ci/pr_body.rs",
        "HtmlContainers::observe",
        "trim_end_matches",
        1,
        "grammar:html-whitespace",
    ),
    // Counts reader-visible natural-language words for the advisory body word limit, not identifiers or command tokens.
    (
        "crates/codeflow-cli/src/cmd/ci/pr_body.rs",
        "ReaderText::words",
        "split_whitespace",
        1,
        "prose",
    ),
    // Counts reader-visible natural-language words for the advisory body word limit, not identifiers or command tokens.
    (
        "crates/codeflow-cli/src/cmd/ci/pr_body.rs",
        "ReaderText::words_in",
        "split_whitespace",
        1,
        "prose",
    ),
    // Rendered Markdown heading and field values allow explicit ASCII space/tab/CR/LF separators, preserving non-ASCII text.
    (
        "crates/codeflow-cli/src/cmd/ci/pr_body.rs",
        "Section::matches",
        "trim_matches",
        1,
        "grammar:markdown-ascii-space",
    ),
    // Rejects whitespace-only rendered Markdown/HTML. The original body remains unchanged.
    (
        "crates/codeflow-cli/src/cmd/ci/pr_body.rs",
        "has_content",
        "trim",
        3,
        "schema-reject-only",
    ),
    // Emptiness check controls an advisory prose-length message, never an authority exemption.
    (
        "crates/codeflow-cli/src/cmd/ci/pr_body.rs",
        "length_message",
        "trim",
        1,
        "prose",
    ),
    // Numeric HTML references become visible whitespace for prose word counting. C1 replacement characters are excluded.
    (
        "crates/codeflow-cli/src/cmd/ci/pr_body.rs",
        "numeric_reference",
        "is_whitespace",
        1,
        "grammar:html-character-reference",
    ),
    // Reads rendered Markdown lines for the Not tested declaration.
    (
        "crates/codeflow-cli/src/cmd/ci/pr_body.rs",
        "presentation",
        "lines",
        1,
        "framing:presentation-marker-lines",
    ),
    // Rendered Markdown heading and field values allow explicit ASCII space/tab/CR/LF separators, preserving non-ASCII text.
    (
        "crates/codeflow-cli/src/cmd/ci/pr_body.rs",
        "presentation",
        "trim_matches",
        1,
        "grammar:markdown-ascii-space",
    ),
    // Reads rendered Markdown release field lines before ASCII field separator parsing.
    (
        "crates/codeflow-cli/src/cmd/ci/pr_body.rs",
        "release_fields_under",
        "lines",
        1,
        "framing:release-field-lines",
    ),
    // Rendered Markdown heading and field values allow explicit ASCII space/tab/CR/LF separators, preserving non-ASCII text.
    (
        "crates/codeflow-cli/src/cmd/ci/pr_body.rs",
        "release_fields_under",
        "trim_matches",
        3,
        "grammar:markdown-ascii-space",
    ),
    // Reads rendered Markdown table rows; exact revision comparison follows.
    (
        "crates/codeflow-cli/src/cmd/ci/pr_body.rs",
        "review_names_revision",
        "lines",
        1,
        "framing:review-row-lines",
    ),
    // Rendered Markdown heading and field values allow explicit ASCII space/tab/CR/LF separators, preserving non-ASCII text.
    (
        "crates/codeflow-cli/src/cmd/ci/pr_body.rs",
        "review_names_revision",
        "trim_matches",
        1,
        "grammar:markdown-ascii-space",
    ),
    // Rejects whitespace-only rendered Markdown/HTML. The original body remains unchanged.
    (
        "crates/codeflow-cli/src/cmd/ci/pr_body.rs",
        "visible_block",
        "trim",
        2,
        "schema-reject-only",
    ),
    // Reads literal NUL-delimited config entries once, passing retained entry bytes onward.
    (
        "crates/codeflow-cli/src/cmd/git_hook.rs",
        "git_config_values",
        "split_terminator",
        1,
        "framing:cli-git-config-z",
    ),
    // base_from_answer leaves an unreadable gh base unproven; git_guard::check_gh refuses an unresolved base for protected-merge checks.
    (
        "crates/codeflow-cli/src/cmd/hook.rs",
        "base_from_answer",
        "obtain-absent:from_utf8",
        1,
        "unproven",
    ),
    // var_os has no decoding error: HOME/USERPROFILE are raw OsString paths; or_else applies only to genuinely unset HOME.
    (
        "crates/codeflow-cli/src/cmd/hook.rs",
        "edit_guard",
        "obtain-absent:var_os",
        1,
        "format-contract",
    ),
    // This handler runs after the blocking guard verdict; malformed already-produced JSON chooses the textual denial message, retaining nonzero exit/refusal.
    (
        "crates/codeflow-cli/src/cmd/hook.rs",
        "grok_deny",
        "obtain-absent:from_str",
        1,
        "display",
    ),
    // Rejects whitespace-only model identities. Catalog comparisons retain original pinned and actual strings.
    (
        "crates/codeflow-cli/src/cmd/models.rs",
        "resolve",
        "trim",
        2,
        "schema-reject-only",
    ),
    // Lenient second read displays any readable invalid values; validate_policy and Policy::source independently preserve read/parse failure and nonzero exit.
    (
        "crates/codeflow-cli/src/cmd/policy.rs",
        "show",
        "obtain-absent:read_to_string",
        1,
        "display",
    ),
    // Wraps policy help prose into terminal columns only.
    (
        "crates/codeflow-cli/src/cmd/policy.rs",
        "wrap",
        "split_whitespace",
        1,
        "display",
    ),
    // RulesOut only adds rule identifiers to a child command failure already recorded as a violation; missing ledger never turns child failure into success.
    (
        "crates/codeflow-cli/src/cmd/push_set.rs",
        "RulesOut::read",
        "obtain-absent:read_to_string",
        1,
        "display",
    ),
    // RulesOut only adds rule identifiers to a child command failure already recorded as a violation; missing ledger never turns child failure into success.
    (
        "crates/codeflow-cli/src/cmd/push_set.rs",
        "RulesOut::read",
        "split_terminator",
        1,
        "display",
    ),
    // Reads ls-remote LF rows and cat-file LF responses once; tab separates fields and names remain exact.
    (
        "crates/codeflow-cli/src/cmd/push_set.rs",
        "advertised_commits",
        "split_terminator",
        3,
        "framing:push-advertisement-lines",
    ),
    // Reads rev-list boundary LF records once; callers receive bare object IDs.
    (
        "crates/codeflow-cli/src/cmd/push_set.rs",
        "boundary",
        "split_terminator",
        1,
        "framing:push-boundary-lines",
    ),
    // Reads rev-list boundary LF records once before ancestor queries.
    (
        "crates/codeflow-cli/src/cmd/push_set.rs",
        "bounded_by",
        "split_terminator",
        1,
        "framing:push-bounded-lines",
    ),
    // Trims stderr only to explain why the commit inventory is unknown.
    (
        "crates/codeflow-cli/src/cmd/push_set.rs",
        "commits_from",
        "trim",
        1,
        "display",
    ),
    // Trims the already-read rev-list numeric count only in the rewrite notice; range and authority selection use unchanged IDs.
    (
        "crates/codeflow-cli/src/cmd/push_set.rs",
        "existing_base",
        "trim",
        1,
        "display",
    ),
    // Reads submodule status LF records; the unmodified status byte decides incompleteness.
    (
        "crates/codeflow-cli/src/cmd/push_set.rs",
        "incomplete_checkout",
        "split_terminator",
        1,
        "framing:submodule-status-lines",
    ),
    // Literal LF separates status rows. Whitespace splitting extracts a displayed path only. The unchanged first status byte decides incompleteness.
    (
        "crates/codeflow-cli/src/cmd/push_set.rs",
        "incomplete_checkout",
        "split_whitespace",
        1,
        "grammar:git-submodule-status",
    ),
    // Formats already-emitted finding text only, without establishing guard verdicts or rewriting operands.
    (
        "crates/codeflow-cli/src/cmd/push_set.rs",
        "relayed_findings",
        "split_terminator",
        1,
        "display",
    ),
    // Formats already-emitted finding text only, without establishing guard verdicts or rewriting operands.
    (
        "crates/codeflow-cli/src/cmd/push_set.rs",
        "relayed_findings",
        "trim",
        1,
        "display",
    ),
    // Formats numeric counts in rewrite notices. The trimmed count never selects policy or a range.
    (
        "crates/codeflow-cli/src/cmd/push_set.rs",
        "target_base",
        "trim",
        1,
        "display",
    ),
    // target_policy maps unreadable/invalid policy to Malformed, judged_by maps that to Invalid, and run_ci_ranges always refuses Invalid.
    (
        "crates/codeflow-cli/src/cmd/push_set.rs",
        "target_policy",
        "obtain-absent:from_str",
        1,
        "unproven",
    ),
    // Trims captured test output only when printing failure reports.
    (
        "crates/codeflow-cli/src/cmd/test.rs",
        "run",
        "trim",
        2,
        "display",
    ),
    // Formats clap help prose into documentation cells, never runtime option values.
    (
        "crates/codeflow-cli/src/command_reference.rs",
        "cell",
        "split_whitespace",
        1,
        "display",
    ),
    // Formats clap help prose into documentation cells, never runtime option values.
    (
        "crates/codeflow-cli/src/command_reference.rs",
        "description",
        "trim",
        1,
        "display",
    ),
    // Interactive accept/refuse/custom tokens permit explicit ASCII space/tab/CR/LF separators; no filesystem identity is parsed.
    (
        "crates/codeflow-cli/src/prompts.rs",
        "decide_pr_template",
        "trim_matches",
        1,
        "grammar:prompt-answer",
    ),
    // CRLF/LF Markdown fence records contain YAML; ASCII indentation is explicit and YAML parses scalar identity unchanged.
    (
        "crates/codeflow-core/src/capability.rs",
        "parse_capabilities",
        "lines",
        1,
        "framing:capability-parse-capabilities-records",
    ),
    // Frontmatter and Markdown sections are physical CRLF/LF records; key and section indentation use explicit ASCII blanks.
    (
        "crates/codeflow-core/src/ceremony/history.rs",
        "regions",
        "lines",
        1,
        "framing:ceremony-history-regions-records",
    ),
    // started() only sets MergedPr.started_at for the printed ceremony retrospective, where the earliest one bounds the window of counted refusals (ceremony/mod.rs refusals); a walk it cannot read falls back to the merge time and feeds no allow/refuse, record or gate.
    (
        "crates/codeflow-core/src/ceremony/history.rs",
        "started",
        "obtain-absent:Result::ok",
        1,
        "display",
    ),
    // started() only sets MergedPr.started_at for the printed ceremony retrospective, where the earliest one bounds the window of counted refusals (ceremony/mod.rs refusals); a walk it cannot read falls back to the merge time and feeds no allow/refuse, record or gate.
    (
        "crates/codeflow-core/src/ceremony/history.rs",
        "started",
        "obtain-absent:find_commit",
        1,
        "display",
    ),
    // Only formats the error reason after gh has failed; status/JSON detection does not use the trimmed diagnostic.
    (
        "crates/codeflow-core/src/ceremony/host.rs",
        "gh",
        "lines",
        1,
        "display",
    ),
    // Only formats the error reason after gh has failed; status/JSON detection does not use the trimmed diagnostic.
    (
        "crates/codeflow-core/src/ceremony/host.rs",
        "gh",
        "trim",
        2,
        "display",
    ),
    // Removes a displayed gh error prefix after gh failed; command success and JSON use unmodified output.
    (
        "crates/codeflow-core/src/ceremony/host.rs",
        "gh",
        "trim_start_matches",
        1,
        "display",
    ),
    // Malformed hook JSON returns false only after cmd::hook::delegate_turn has received handle_hook Err; both branches return nonzero (1 or 2), never success.
    (
        "crates/codeflow-core/src/delegate.rs",
        "is_prompt_submission",
        "obtain-absent:from_str",
        1,
        "unproven",
    ),
    // Rejects whitespace-only model/effort provenance; accepted labels are now stored exactly so trailing Unicode whitespace cannot alias another identity.
    (
        "crates/codeflow-core/src/delegate.rs",
        "provenance_value",
        "trim",
        1,
        "schema-reject-only",
    ),
    // BufRead lines reads one JSONL transcript record, strips protocol CRLF/LF and surfaces I/O or UTF-8 errors as Err before identity checks.
    (
        "crates/codeflow-core/src/delegate/continuation.rs",
        "session_entries",
        "lines",
        1,
        "framing:delegate-continuation-session-entries-records",
    ),
    // The missing origin kind placeholder occurs only inside an error returned after native-origin verification has already failed.
    (
        "crates/codeflow-core/src/delegate/continuation.rs",
        "verify_notice_origin",
        "obtain-absent:as_str",
        1,
        "display",
    ),
    // Shipped CI template records permit CRLF/LF; indentation comparison now uses literal space/tab and exact remaining shell line bytes.
    (
        "crates/codeflow-core/src/doctor/ci_pin.rs",
        "recognized",
        "lines",
        1,
        "framing:doctor-ci-pin-recognized-records",
    ),
    // Shipped CI template records permit CRLF/LF; indentation comparison now uses literal space/tab and exact remaining shell line bytes.
    (
        "crates/codeflow-core/src/doctor/ci_pin.rs",
        "span",
        "lines",
        1,
        "framing:doctor-ci-pin-span-records",
    ),
    // A whitespace-only refusal reason rejects the deny response; JSON is parsed without trimming and the reason text is never rewritten.
    (
        "crates/codeflow-core/src/doctor/grok_hooks.rs",
        "is_deny_answer",
        "trim",
        1,
        "schema-reject-only",
    ),
    // Whitespace-only commands are rejected as Codex skips them; accepted command bytes remain unchanged in the trusted hash.
    (
        "crates/codeflow-core/src/doctor/mod.rs",
        "codex_hook_hash",
        "trim",
        1,
        "schema-reject-only",
    ),
    // Mirrors native Codex hook-state key trimming and merge order; this is the native state grammar rather than shell command tokenization.
    (
        "crates/codeflow-core/src/doctor/mod.rs",
        "codex_hook_states",
        "trim",
        1,
        "grammar:codex-hook-state",
    ),
    // Formats a failed doctor probe diagnostic for the operator; no ref, path, command, or config identity is selected from it.
    (
        "crates/codeflow-core/src/doctor/mod.rs",
        "exec_doctor_step",
        "trim",
        1,
        "display",
    ),
    // Formats a failed doctor probe diagnostic for the operator; no ref, path, command, or config identity is selected from it.
    (
        "crates/codeflow-core/src/doctor/mod.rs",
        "first_line",
        "lines",
        1,
        "display",
    ),
    // Formats a failed doctor probe diagnostic for the operator; no ref, path, command, or config identity is selected from it.
    (
        "crates/codeflow-core/src/doctor/mod.rs",
        "first_line",
        "trim",
        1,
        "display",
    ),
    // Mirrors xai-grok-env boolean parsing, whose native contract trims and lowercases these enumerated environment boolean values.
    (
        "crates/codeflow-core/src/doctor/mod.rs",
        "grok_bool",
        "trim",
        1,
        "grammar:grok-env-bool",
    ),
    // Requires a nonblank stderr refusal explanation in addition to exit 2 and parsed JSON deny; does not modify or parse command identity.
    (
        "crates/codeflow-core/src/doctor/mod.rs",
        "grok_canary",
        "lines",
        1,
        "schema-reject-only",
    ),
    // Requires a nonblank stderr refusal explanation in addition to exit 2 and parsed JSON deny; does not modify or parse command identity.
    (
        "crates/codeflow-core/src/doctor/mod.rs",
        "grok_canary",
        "trim",
        1,
        "schema-reject-only",
    ),
    // The key only selects the grok doctor wording: Trusted and Unverifiable give a Note, Untrusted a Warn (doctor/mod.rs check_grok); no branch yields Fail, and the guards run regardless of Grok trust.
    (
        "crates/codeflow-core/src/doctor/mod.rs",
        "grok_workspace_key",
        "obtain-absent:Repository::discover",
        1,
        "display",
    ),
    // A commondir that cannot be opened keeps the worktree key, which only selects the Note or Warn wording of the grok doctor check (doctor/mod.rs check_grok), never Fail.
    (
        "crates/codeflow-core/src/doctor/mod.rs",
        "grok_workspace_key",
        "obtain-absent:Repository::open",
        1,
        "display",
    ),
    // Only the diagnostic branch trims the displayed observed version; the comparison removes one terminal LF and preserves other characters.
    (
        "crates/codeflow-core/src/doctor/mod.rs",
        "observe_binding_drift",
        "trim",
        1,
        "display",
    ),
    // var_os returns native bytes and has no decoding error; None proves HOME/USERPROFILE unset and PathBuf retains present non-UTF8 bytes.
    (
        "crates/codeflow-core/src/doctor/mod.rs",
        "user_home",
        "obtain-absent:var_os",
        1,
        "format-contract",
    ),
    // Invalid JSON adds its path to invalid; check_config converts any such entry into Status::Fail.
    (
        "crates/codeflow-core/src/doctor/mod.rs",
        "walk_json_files_inner",
        "obtain-absent:from_slice",
        1,
        "unproven",
    ),
    // JSON parse failure adds invalid_json finding and returns ForecastReport; its valid predicate requires zero findings and estimate check exits nonzero.
    (
        "crates/codeflow-core/src/estimate/mod.rs",
        "check_forecast",
        "obtain-absent:from_slice",
        1,
        "unproven",
    ),
    // Rejects empty/whitespace-only bounded explanation text while retaining the original accepted string.
    (
        "crates/codeflow-core/src/estimate/shape.rs",
        "text",
        "trim",
        1,
        "schema-reject-only",
    ),
    // Reader::read returns None only after adding source_read or unsafe_source_path finding; check_forecast returns invalid ForecastReport and estimate check refuses it.
    (
        "crates/codeflow-core/src/estimate/sources.rs",
        "Reader::pin",
        "obtain-absent:read",
        1,
        "unproven",
    ),
    // Read/decode failure records source_read or source_record finding; check_forecast cannot return a valid ForecastReport when any finding is present.
    (
        "crates/codeflow-core/src/estimate/sources.rs",
        "Reader::record",
        "obtain-absent:from_utf8",
        1,
        "unproven",
    ),
    // Read/decode failure records source_read or source_record finding; check_forecast cannot return a valid ForecastReport when any finding is present.
    (
        "crates/codeflow-core/src/estimate/sources.rs",
        "Reader::record",
        "obtain-absent:read",
        1,
        "unproven",
    ),
    // A non-string epic_id makes the task record unproven and immediately adds source_record finding; it never substitutes a parent or default. check_forecast refuses a ForecastReport containing the source_record finding.
    (
        "crates/codeflow-core/src/estimate/sources.rs",
        "task",
        "obtain-absent:as_str",
        1,
        "unproven",
    ),
    // Trims gh stderr for an error message; no queried branch or repository identity is changed.
    (
        "crates/codeflow-core/src/git/ci.rs",
        "parse_pr_checks_output",
        "trim",
        1,
        "display",
    ),
    // Decodes two ASCII hex digits from the internal storage_key encoding, not arbitrary OS bytes; storage_key emits a complete hex suffix after NUL.
    (
        "crates/codeflow-core/src/git/name.rs",
        "GitName::from_storage_key",
        "obtain-absent:from_utf8",
        1,
        "format-contract",
    ),
    // UTF-8 selects the reversible storage-key representation; the other branch hex-encodes every byte, never loses or omits a name.
    (
        "crates/codeflow-core/src/git/name.rs",
        "GitName::storage_key",
        "obtain-absent:from_utf8",
        1,
        "format-contract",
    ),
    // Only failed git stderr is trimmed; error classification checks an ASCII substring and the request remains refused.
    (
        "crates/codeflow-core/src/hooks/conflict_markers.rs",
        "marker_sizes",
        "trim",
        1,
        "grammar:git-diagnostic",
    ),
    // A HEAD that cannot be read diffs the whole index as added, a superset of the staged change, so findings can only grow.
    (
        "crates/codeflow-core/src/hooks/conflict_markers.rs",
        "staged",
        "obtain-absent:head",
        1,
        "unproven",
    ),
    // A HEAD that cannot be read scans the whole index as added, a superset of the staged change, so findings can only grow.
    (
        "crates/codeflow-core/src/hooks/conflict_markers.rs",
        "staged",
        "obtain-absent:peel_to_tree",
        1,
        "unproven",
    ),
    // Trims tmux stderr in a failed completion-signal diagnostic.
    (
        "crates/codeflow-core/src/hooks/delegate_turn.rs",
        "signal_tmux",
        "trim",
        1,
        "display",
    ),
    // The sole caller (git_guard.rs, the root checkout exemption) passes the result as an exception; None removes the exemption, so an unread repository only refuses more, and hooks::repo::open already refuses an unreadable repository first.
    (
        "crates/codeflow-core/src/hooks/edit_guard.rs",
        "checkout_root_of",
        "obtain-absent:Repository::discover",
        1,
        "unproven",
    ),
    // var_os preserves native bytes; None is a genuinely unset optional environment key. HOME/USERPROFILE and XDG defaults follow their documented lookup precedence.
    (
        "crates/codeflow-core/src/hooks/edit_guard.rs",
        "global_git_config_target",
        "obtain-absent:var_os",
        2,
        "grammar:environment",
    ),
    // var_os preserves native HOME/USERPROFILE bytes for interpreter path checks; fallback occurs only for a genuinely unset variable, not a decoding failure.
    (
        "crates/codeflow-core/src/hooks/exec_guard.rs",
        "evaluate_in",
        "obtain-absent:var_os",
        1,
        "grammar:environment",
    ),
    // A ref that cannot be peeled is left out of the kept tips, which can only leave a deleted tip unbacked and return the discard refusal; it never adds backing.
    (
        "crates/codeflow-core/src/hooks/git_discard.rs",
        "delete_branches",
        "obtain-absent:peel_to_commit",
        1,
        "unproven",
    ),
    // Space, tab and newline are idempotent shell separators between command segments; no quote or path framing is removed.
    (
        "crates/codeflow-core/src/hooks/git_guard.rs",
        "Line::end_segment",
        "trim_matches",
        1,
        "grammar:posix-shell",
    ),
    // shell_blank explicitly accepts only space, tab and newline, matching the shell reader.
    (
        "crates/codeflow-core/src/hooks/git_guard.rs",
        "paren_in_word",
        "trim_end_matches",
        1,
        "grammar:posix-shell",
    ),
    // shell_blank explicitly accepts only space, tab and newline, matching the shell reader.
    (
        "crates/codeflow-core/src/hooks/git_guard.rs",
        "qualifier_code",
        "trim_matches",
        1,
        "grammar:posix-shell",
    ),
    // The invalid-byte branch returns AliasAnswer::Unreadable; expand_alias refuses execution of the unresolved alias.
    (
        "crates/codeflow-core/src/hooks/git_guard.rs",
        "read_alias",
        "obtain-absent:from_utf8",
        1,
        "unproven",
    ),
    // Only subprocess stderr is trimmed for a refusal message; identity stdout is strict and separately framed.
    (
        "crates/codeflow-core/src/hooks/git_guard.rs",
        "read_alias",
        "trim",
        1,
        "display",
    ),
    // An unreadable branch becomes NON_UTF8_BRANCH, which GitPolicy::branch_is_protected treats as protected; BranchTracker refuses protected mutations.
    (
        "crates/codeflow-core/src/hooks/git_guard.rs",
        "read_branch_name",
        "obtain-absent:from_utf8",
        1,
        "unproven",
    ),
    // Only subprocess stderr is trimmed for a refusal message; identity stdout is strict and separately framed.
    (
        "crates/codeflow-core/src/hooks/git_guard.rs",
        "read_branch_name",
        "trim",
        1,
        "display",
    ),
    // RootCheckout::read errors mark the target unresolved; judge_target always blocks unresolved mutations with git.policy_authority independently of session policy levels.
    (
        "crates/codeflow-core/src/hooks/git_guard.rs",
        "read_target",
        "obtain-absent:read",
        1,
        "unproven",
    ),
    // Predicate consumes ASCII digits only to identify an explicit file-descriptor prefix.
    (
        "crates/codeflow-core/src/hooks/git_guard.rs",
        "redirect_operator_len",
        "trim_start_matches",
        1,
        "grammar:shell-redirection",
    ),
    // One compiles the constant *, which cannot fail; the other falls back to every_name, which matches every name, so a pattern glob cannot compile over-approximates the reach and only refuses more.
    (
        "crates/codeflow-core/src/hooks/git_guard.rs",
        "shell_pattern",
        "obtain-absent:Pattern::new",
        2,
        "unproven",
    ),
    // shell_blank explicitly accepts only space, tab and newline, matching the shell reader.
    (
        "crates/codeflow-core/src/hooks/git_guard.rs",
        "split_into_segments",
        "trim_matches",
        2,
        "grammar:posix-shell",
    ),
    // Consumes explicit path_char characters to delimit an enforcement-name substring in script text; never rewrites a path used on disk.
    (
        "crates/codeflow-core/src/hooks/git_guard.rs",
        "worktree_text",
        "trim_start_matches",
        1,
        "grammar:enforcement-needle",
    ),
    // Owns physical text-line framing for this complete message/document reader. Commit message subject, comment or location parsing uses textual message lines; no ref/path identity is derived from this text.
    (
        "crates/codeflow-core/src/hooks/git_hook.rs",
        "commit_msg_from",
        "lines",
        1,
        "framing:git-commit-message-lines",
    ),
    // Commit message subject, comment or location parsing uses textual message lines; no ref/path identity is derived from this text.
    (
        "crates/codeflow-core/src/hooks/git_hook.rs",
        "commit_msg_from",
        "trim",
        1,
        "grammar:git-commit-message",
    ),
    // Owns physical text-line framing for this complete message/document reader. Commit message subject, comment or location parsing uses textual message lines; no ref/path identity is derived from this text.
    (
        "crates/codeflow-core/src/hooks/git_hook.rs",
        "commit_msg_with_files",
        "lines",
        1,
        "framing:git-commit-message-lines",
    ),
    // Commit message subject, comment or location parsing uses textual message lines; no ref/path identity is derived from this text.
    (
        "crates/codeflow-core/src/hooks/git_hook.rs",
        "commit_msg_with_files",
        "trim",
        1,
        "grammar:git-commit-message",
    ),
    // An unreadable binary digest is explicitly printed unavailable in the report; it is not used to authorize a command.
    (
        "crates/codeflow-core/src/hooks/git_hook.rs",
        "judging_identity",
        "obtain-absent:read",
        1,
        "display",
    ),
    // Unreadable loose or packed refs cannot prove a no-op. reference_transaction applies protected-ref mutation refusal instead of granting the keeps-value exception.
    (
        "crates/codeflow-core/src/hooks/git_hook.rs",
        "keeps_value",
        "obtain-absent:read_to_string",
        2,
        "unproven",
    ),
    // Unreadable upstream identity returns false; reference_transaction refuses the protected-branch update without the proven remote-sync exception.
    (
        "crates/codeflow-core/src/hooks/git_hook.rs",
        "new_matches_remote_head",
        "obtain-absent:as_str",
        1,
        "unproven",
    ),
    // A lookup error returns false, and the caller then pushes the git.local_ref_protection violation; an unread ref never counts as a sync.
    (
        "crates/codeflow-core/src/hooks/git_hook.rs",
        "new_matches_remote_head",
        "obtain-absent:find_reference",
        1,
        "unproven",
    ),
    // Owns physical text-line framing for this complete message/document reader. Commit message subject, comment or location parsing uses textual message lines; no ref/path identity is derived from this text.
    (
        "crates/codeflow-core/src/hooks/git_hook.rs",
        "policy_character_violation",
        "lines",
        1,
        "framing:git-commit-message-lines",
    ),
    // Commit message subject, comment or location parsing uses textual message lines; no ref/path identity is derived from this text.
    (
        "crates/codeflow-core/src/hooks/git_hook.rs",
        "policy_character_violation",
        "trim",
        1,
        "grammar:git-commit-message",
    ),
    // Owns physical text-line framing for this complete message/document reader. Commit message subject, comment or location parsing uses textual message lines; no ref/path identity is derived from this text.
    (
        "crates/codeflow-core/src/hooks/git_hook.rs",
        "strip_commit_comments",
        "lines",
        1,
        "framing:git-commit-message-lines",
    ),
    // shell_blank is exactly space, tab and newline; the predicate cannot erase Unicode operand characters.
    (
        "crates/codeflow-core/src/hooks/git_target.rs",
        "close_segment",
        "trim_matches",
        1,
        "grammar:posix-shell",
    ),
    // Selects advisory orientation/guidance prose only; malformed optional harness payload does not grant execution authority.
    (
        "crates/codeflow-core/src/hooks/guidance.rs",
        "payload_event",
        "obtain-absent:from_str",
        1,
        "display",
    ),
    // Extracts prompt prose only for advisory guidance; no command, path or policy authority is selected.
    (
        "crates/codeflow-core/src/hooks/guidance.rs",
        "payload_prompt",
        "obtain-absent:from_str",
        1,
        "display",
    ),
    // Selects the advisory resume message only; no command or policy authority is selected.
    (
        "crates/codeflow-core/src/hooks/guidance.rs",
        "payload_source",
        "obtain-absent:from_str",
        1,
        "display",
    ),
    // Normalizes natural-language request keywords using explicit punctuation; output chooses advisory guidance text, not command/ref/path authority.
    (
        "crates/codeflow-core/src/hooks/guidance.rs",
        "words",
        "trim_matches",
        1,
        "grammar:guidance-words",
    ),
    // read returns Result<Option<String>> and ? propagates every obtaining error before map_or_else; only a genuinely absent optional policy file selects the schema default.
    (
        "crates/codeflow-core/src/hooks/landed_policy.rs",
        "at",
        "obtain-absent:read",
        1,
        "format-contract",
    ),
    // Optional orientation summary only; unreadable input cannot authorize or route a command.
    (
        "crates/codeflow-core/src/hooks/orient.rs",
        "capabilities_line",
        "obtain-absent:read_to_string",
        1,
        "display",
    ),
    // Optional hook-installation display only; actual hook enforcement obtains its own inputs.
    (
        "crates/codeflow-core/src/hooks/orient.rs",
        "gates_line",
        "obtain-absent:read_to_string",
        1,
        "display",
    ),
    // Formats or bounds orientation prose (product summary or ADR titles); no command or filesystem routing uses the normalized text.
    (
        "crates/codeflow-core/src/hooks/orient.rs",
        "generate",
        "lines",
        1,
        "display",
    ),
    // Compares the fixed hooks-version marker; mismatch only produces an orientation warning.
    (
        "crates/codeflow-core/src/hooks/orient.rs",
        "generate",
        "trim",
        1,
        "ascii-marker",
    ),
    // Formats or bounds orientation prose (product summary or ADR titles); no command or filesystem routing uses the normalized text.
    (
        "crates/codeflow-core/src/hooks/orient.rs",
        "product_one_liner",
        "lines",
        3,
        "display",
    ),
    // Product title prose for orientation only; normalization never reaches an identity lookup.
    (
        "crates/codeflow-core/src/hooks/orient.rs",
        "product_one_liner",
        "obtain-absent:read_to_string",
        1,
        "display",
    ),
    // Formats or bounds orientation prose (product summary or ADR titles); no command or filesystem routing uses the normalized text.
    (
        "crates/codeflow-core/src/hooks/orient.rs",
        "product_one_liner",
        "trim",
        6,
        "display",
    ),
    // Product title prose for orientation only; normalization never reaches an identity lookup.
    (
        "crates/codeflow-core/src/hooks/orient.rs",
        "product_one_liner",
        "trim_matches",
        1,
        "display",
    ),
    // Formats or bounds orientation prose (product summary or ADR titles); no command or filesystem routing uses the normalized text.
    (
        "crates/codeflow-core/src/hooks/orient.rs",
        "recent_adrs",
        "lines",
        1,
        "display",
    ),
    // ADR title listing for orientation only; missing display entries do not change enforcement or configuration.
    (
        "crates/codeflow-core/src/hooks/orient.rs",
        "recent_adrs",
        "obtain-absent:Result::ok",
        1,
        "display",
    ),
    // recent_adrs feeds only the orient digest's printed list of recent ADR titles; an ADR file name that is not UTF-8 drops one title from that text.
    (
        "crates/codeflow-core/src/hooks/orient.rs",
        "recent_adrs",
        "obtain-absent:into_string",
        1,
        "display",
    ),
    // ADR title listing for orientation only; missing display entries do not change enforcement or configuration.
    (
        "crates/codeflow-core/src/hooks/orient.rs",
        "recent_adrs",
        "obtain-absent:read_to_string",
        1,
        "display",
    ),
    // Invalid JSON is PolicySource::MalformedFile; policy show reports invalid and effective policy loading refuses through Policy::load_file.
    (
        "crates/codeflow-core/src/hooks/policy.rs",
        "Policy::source",
        "obtain-absent:from_str",
        1,
        "unproven",
    ),
    // The policy-show reader returns None to display invalid JSON; enforcement uses strict Policy::load_file instead.
    (
        "crates/codeflow-core/src/hooks/policy_schema.rs",
        "parse_lenient",
        "obtain-absent:from_str",
        1,
        "display",
    ),
    // Selects a line of an existing schema error solely for its diagnostic message.
    (
        "crates/codeflow-core/src/hooks/policy_schema.rs",
        "validate_leaf",
        "lines",
        1,
        "display",
    ),
    // Rejects whitespace-only names/programs or wildcard-only branch profiles without rewriting accepted values; trimming can only add a schema refusal.
    (
        "crates/codeflow-core/src/hooks/policy_schema.rs",
        "validate_profiles",
        "trim",
        6,
        "schema-reject-only",
    ),
    // Rejects whitespace-only names/programs or wildcard-only branch profiles without rewriting accepted values; trimming can only add a schema refusal.
    (
        "crates/codeflow-core/src/hooks/policy_schema.rs",
        "validate_retry_entries",
        "trim",
        1,
        "schema-reject-only",
    ),
    // Unreadable relevant values retain the UNREADABLE_VALUE marker; update_remotes/fetch cannot resolve that group or boolean as an allowed remote. Unrelated config values are not decision inputs.
    (
        "crates/codeflow-core/src/hooks/ref_authority.rs",
        "config_entries",
        "obtain-absent:from_utf8",
        1,
        "unproven",
    ),
    // UTF-8 failure maps to true in the refusal condition; the remote URL mapping remains unproven and fetch is refused.
    (
        "crates/codeflow-core/src/hooks/ref_authority.rs",
        "fetch_from_repository",
        "obtain-absent:from_utf8",
        1,
        "unproven",
    ),
    // Consumes non-ASCII-alphanumeric prefix characters only when recognizing the existing literal placeholder-value grammar; no ref/path is selected.
    (
        "crates/codeflow-core/src/hooks/scan.rs",
        "is_placeholder_value",
        "trim_start_matches",
        1,
        "grammar:secret-placeholder",
    ),
    // Owns physical text-line framing for this complete message/document reader. Reads added textual lines for secret detection and line labels; no filename is resolved from a normalized hunk line.
    (
        "crates/codeflow-core/src/hooks/scan.rs",
        "scan_diff",
        "lines",
        1,
        "framing:unified-diff-lines",
    ),
    // Path text only labels a secret finding; detection examines the added content rather than looking up this label.
    (
        "crates/codeflow-core/src/hooks/scan.rs",
        "scan_diff",
        "trim",
        1,
        "display",
    ),
    // changed_files is written only into the session_end ledger event as a recall count; nothing reads it for a decision, and the session-end hook never blocks.
    (
        "crates/codeflow-core/src/hooks/session_summary.rs",
        "diff_stats",
        "obtain-absent:find_commit",
        1,
        "display",
    ),
    // The base fold only sets base_branch and the counts in the session_end ledger event, recall data that no decision reads.
    (
        "crates/codeflow-core/src/hooks/session_summary.rs",
        "diff_stats",
        "obtain-absent:peel_to_commit",
        1,
        "display",
    ),
    // Session telemetry payload only; fallback null retains a session event with unavailable optional labels, never an enforcement or path-selection input.
    (
        "crates/codeflow-core/src/hooks/session_summary.rs",
        "record",
        "obtain-absent:from_str",
        1,
        "display",
    ),
    // Parses session-summary telemetry payload for recording; this does not authorize a command or alter enforcement.
    (
        "crates/codeflow-core/src/hooks/session_summary.rs",
        "record",
        "trim",
        1,
        "display",
    ),
    // Build revision label only; unreadable or empty source metadata is explicitly displayed as unavailable, not used as a ref selector.
    (
        "crates/codeflow-core/src/hooks/source_identity.rs",
        "revision",
        "obtain-absent:from_utf8",
        1,
        "display",
    ),
    // Build revision label only; unreadable or empty source metadata is explicitly displayed as unavailable, not used as a ref selector.
    (
        "crates/codeflow-core/src/hooks/source_identity.rs",
        "revision",
        "trim",
        1,
        "display",
    ),
    // Owns physical text-line framing for this complete message/document reader. Commit message content and trailer syntax use the existing conventional-commit whitespace contract; these operations do not normalize a Git ref or filesystem operand.
    (
        "crates/codeflow-core/src/hooks/standards.rs",
        "breaking_marker_present",
        "lines",
        1,
        "framing:conventional-commits-lines",
    ),
    // Owns physical text-line framing for this complete message/document reader. Commit message content and trailer syntax use the existing conventional-commit whitespace contract; these operations do not normalize a Git ref or filesystem operand.
    (
        "crates/codeflow-core/src/hooks/standards.rs",
        "check_breaking_footer",
        "lines",
        1,
        "framing:conventional-commits-lines",
    ),
    // Commit message content and trailer syntax use the existing conventional-commit whitespace contract; these operations do not normalize a Git ref or filesystem operand.
    (
        "crates/codeflow-core/src/hooks/standards.rs",
        "check_breaking_footer",
        "trim_start",
        1,
        "grammar:conventional-commits",
    ),
    // Owns physical text-line framing for this complete message/document reader. Commit message content and trailer syntax use the existing conventional-commit whitespace contract; these operations do not normalize a Git ref or filesystem operand.
    (
        "crates/codeflow-core/src/hooks/standards.rs",
        "check_commit_body",
        "lines",
        1,
        "framing:conventional-commits-lines",
    ),
    // Commit message content and trailer syntax use the existing conventional-commit whitespace contract; these operations do not normalize a Git ref or filesystem operand.
    (
        "crates/codeflow-core/src/hooks/standards.rs",
        "check_commit_body",
        "trim",
        1,
        "grammar:conventional-commits",
    ),
    // Commit message content and trailer syntax use the existing conventional-commit whitespace contract; these operations do not normalize a Git ref or filesystem operand.
    (
        "crates/codeflow-core/src/hooks/standards.rs",
        "check_commit_body",
        "trim_end",
        1,
        "grammar:conventional-commits",
    ),
    // Commit message content and trailer syntax use the existing conventional-commit whitespace contract; these operations do not normalize a Git ref or filesystem operand.
    (
        "crates/codeflow-core/src/hooks/standards.rs",
        "check_commit_format",
        "trim_end",
        1,
        "grammar:conventional-commits",
    ),
    // Owns physical text-line framing for this complete message/document reader. Commit message content and trailer syntax use the existing conventional-commit whitespace contract; these operations do not normalize a Git ref or filesystem operand.
    (
        "crates/codeflow-core/src/hooks/standards.rs",
        "check_commit_ticket",
        "lines",
        1,
        "framing:conventional-commits-lines",
    ),
    // Commit message content and trailer syntax use the existing conventional-commit whitespace contract; these operations do not normalize a Git ref or filesystem operand.
    (
        "crates/codeflow-core/src/hooks/standards.rs",
        "check_commit_ticket",
        "trim_end",
        1,
        "grammar:conventional-commits",
    ),
    // Owns physical text-line framing for this complete message/document reader. Commit message content and trailer syntax use the existing conventional-commit whitespace contract; these operations do not normalize a Git ref or filesystem operand.
    (
        "crates/codeflow-core/src/hooks/standards.rs",
        "check_required_footers",
        "lines",
        1,
        "framing:conventional-commits-lines",
    ),
    // Commit message content and trailer syntax use the existing conventional-commit whitespace contract; these operations do not normalize a Git ref or filesystem operand.
    (
        "crates/codeflow-core/src/hooks/standards.rs",
        "check_required_footers",
        "trim_end",
        1,
        "grammar:conventional-commits",
    ),
    // Owns physical text-line framing for this complete message/document reader. Commit message content and trailer syntax use the existing conventional-commit whitespace contract; these operations do not normalize a Git ref or filesystem operand.
    (
        "crates/codeflow-core/src/hooks/standards.rs",
        "check_subject_separator",
        "lines",
        1,
        "framing:conventional-commits-lines",
    ),
    // Commit message content and trailer syntax use the existing conventional-commit whitespace contract; these operations do not normalize a Git ref or filesystem operand.
    (
        "crates/codeflow-core/src/hooks/standards.rs",
        "check_subject_separator",
        "trim",
        2,
        "grammar:conventional-commits",
    ),
    // Commit message content and trailer syntax use the existing conventional-commit whitespace contract; these operations do not normalize a Git ref or filesystem operand.
    (
        "crates/codeflow-core/src/hooks/standards.rs",
        "check_subject_separator",
        "trim_end",
        1,
        "grammar:conventional-commits",
    ),
    // Owns physical text-line framing for this complete message/document reader. Commit message content and trailer syntax use the existing conventional-commit whitespace contract; these operations do not normalize a Git ref or filesystem operand.
    (
        "crates/codeflow-core/src/hooks/standards.rs",
        "find_policy_character",
        "lines",
        1,
        "framing:conventional-commits-lines",
    ),
    // Commit message content and trailer syntax use the existing conventional-commit whitespace contract; these operations do not normalize a Git ref or filesystem operand.
    (
        "crates/codeflow-core/src/hooks/standards.rs",
        "is_breaking_footer_start",
        "trim_start",
        1,
        "grammar:conventional-commits",
    ),
    // Commit message content and trailer syntax use the existing conventional-commit whitespace contract; these operations do not normalize a Git ref or filesystem operand.
    (
        "crates/codeflow-core/src/hooks/standards.rs",
        "trailer_kv",
        "trim_end",
        1,
        "grammar:conventional-commits",
    ),
    // CRLF/LF YAML lines are framing; scalar padding uses explicit space/tab and Unicode value characters remain unchanged.
    (
        "crates/codeflow-core/src/ids/entry.rs",
        "frontmatter_value",
        "lines",
        1,
        "framing:ids-entry-frontmatter-value-records",
    ),
    // Explicit LF record separator preserves CR and Unicode in ref fields; Git for-each-ref format uses literal spaces between its fields.
    (
        "crates/codeflow-core/src/ids/git.rs",
        "Git::branch_refs",
        "split_terminator",
        1,
        "framing:ids-git-git-branch-refs-records",
    ),
    // Trims only Git stderr in an error value after nonzero exit; successful decision answers decode strictly.
    (
        "crates/codeflow-core/src/ids/git.rs",
        "checked",
        "trim",
        1,
        "display",
    ),
    // Trims only Git stderr in an error value after nonzero exit; successful decision answers decode strictly.
    (
        "crates/codeflow-core/src/ids/git.rs",
        "checked_bytes",
        "trim",
        1,
        "display",
    ),
    // Explicit LF records contain Git-generated ASCII commit and parent OIDs separated by literal spaces.
    (
        "crates/codeflow-core/src/ids/inventory.rs",
        "lifetime_start",
        "split_terminator",
        1,
        "framing:ids-inventory-lifetime-start-records",
    ),
    // Builds the human-readable reason separately from diagnostics() used to classify the failed push.
    (
        "crates/codeflow-core/src/ids/issue.rs",
        "classify_push",
        "lines",
        1,
        "display",
    ),
    // Builds the human-readable reason separately from diagnostics() used to classify the failed push.
    (
        "crates/codeflow-core/src/ids/issue.rs",
        "classify_push",
        "trim",
        1,
        "display",
    ),
    // Git push diagnostics are line records; tab-delimited status summaries are selected independently of ref and URL fields, with literal ASCII flag handling.
    (
        "crates/codeflow-core/src/ids/issue.rs",
        "diagnostics",
        "lines",
        1,
        "framing:ids-issue-diagnostics-records",
    ),
    // Stderr text is the failed fetch explanation; exit status already decides failure and trimming cannot turn it into success.
    (
        "crates/codeflow-core/src/ids/issue.rs",
        "fetch",
        "trim",
        1,
        "display",
    ),
    // Explicit LF record separator reads Git-generated ASCII OIDs used to inspect pending entries.
    (
        "crates/codeflow-core/src/ids/issue.rs",
        "pending_entries",
        "split_terminator",
        1,
        "framing:ids-issue-pending-entries-records",
    ),
    // Explicit LF records contain Git-generated commit OIDs for the range; no ref-name whitespace normalization occurs.
    (
        "crates/codeflow-core/src/ids/ledger.rs",
        "Ledger::range_violations",
        "split_terminator",
        1,
        "framing:ids-ledger-ledger-range-violations-records",
    ),
    // Edits YAML physical CRLF/LF lines when adding former_ids: each line's terminator is set apart and kept; identifiers are retained, and flow-list padding uses explicit ASCII blanks.
    (
        "crates/codeflow-core/src/ids/seed.rs",
        "add_former_id",
        "trim_end_matches",
        1,
        "framing:ids-seed-add-former-id-records",
    ),
    // A frontmatter fence is `---` with only ASCII space, tab, CR or LF after it, as the record parser reads it.
    (
        "crates/codeflow-core/src/ids/seed.rs",
        "is_fence",
        "trim_end_matches",
        1,
        "grammar:yaml-frontmatter-fence",
    ),
    // Formats Git error stderr after an unsuccessful command; the checkout or merge result is decided by exit status.
    (
        "crates/codeflow-core/src/integrate.rs",
        "checkout",
        "trim",
        1,
        "display",
    ),
    // Git porcelain quotes unsafe names; every nonempty status record blocks integration and names are only displayed, never opened or compared.
    (
        "crates/codeflow-core/src/integrate.rs",
        "dirty_files",
        "lines",
        1,
        "grammar:git-porcelain-status",
    ),
    // Git porcelain quotes unsafe names; every nonempty status record blocks integration and names are only displayed, never opened or compared.
    (
        "crates/codeflow-core/src/integrate.rs",
        "dirty_files",
        "trim",
        1,
        "grammar:git-porcelain-status",
    ),
    // An ancestry error gives is_fast_forward false, which restores the checkout and returns MergeFailed; an unreadable ancestry refuses the landing.
    (
        "crates/codeflow-core/src/integrate.rs",
        "integrate",
        "obtain-absent:graph_descendant_of",
        1,
        "unproven",
    ),
    // Formats Git error stderr after an unsuccessful command; the checkout or merge result is decided by exit status.
    (
        "crates/codeflow-core/src/integrate.rs",
        "integrate",
        "trim",
        1,
        "display",
    ),
    // Formats Git error stderr after an unsuccessful command; the checkout or merge result is decided by exit status.
    (
        "crates/codeflow-core/src/integrate.rs",
        "restore_checkout",
        "trim",
        1,
        "display",
    ),
    // Short OID is printed in the integration report; a failed abbreviation falls back to the full exact OID, never an alternate commit.
    (
        "crates/codeflow-core/src/integrate.rs",
        "short_id",
        "obtain-absent:as_str",
        1,
        "display",
    ),
    // short_id only formats an object id for messages and the report; on error it prints the full id.
    (
        "crates/codeflow-core/src/integrate.rs",
        "short_id",
        "obtain-absent:find_object",
        1,
        "display",
    ),
    // Explicit LF record framing and literal JSON space/tab/CR/LF padding preserve all JSON string identities.
    (
        "crates/codeflow-core/src/ledger/compact.rs",
        "read_events_from_file",
        "split_terminator",
        1,
        "framing:ledger-compact-read-events-from-file-records",
    ),
    // Space, tab, CR and LF are JSON whitespace between physical LF records; JSON string bytes are retained and malformed records return LedgerError.
    (
        "crates/codeflow-core/src/ledger/compact.rs",
        "read_events_from_file",
        "trim_matches",
        1,
        "grammar:json-whitespace",
    ),
    // Explicit LF record framing and literal JSON space/tab/CR/LF padding preserve all JSON string identities.
    (
        "crates/codeflow-core/src/ledger/rebuild.rs",
        "read_events_from_file",
        "split_terminator",
        1,
        "framing:ledger-rebuild-read-events-from-file-records",
    ),
    // Rejects whitespace-only observed identity fields; accepted model and harness IDs are retained exactly.
    (
        "crates/codeflow-core/src/model_catalog/inputs.rs",
        "CatalogInputs::load",
        "trim",
        2,
        "schema-reject-only",
    ),
    // Second JSON parse only chooses the explanatory text inside Catalog::parse map_err; it cannot convert the catalog error into success.
    (
        "crates/codeflow-core/src/model_catalog/inputs.rs",
        "load_catalog",
        "obtain-absent:from_slice",
        1,
        "display",
    ),
    // Markdown execution-contract records are CRLF/LF framed; literal ASCII padding is removed from keyed override fields, preserving Unicode identity characters.
    (
        "crates/codeflow-core/src/model_catalog/operator.rs",
        "anchored_override",
        "lines",
        1,
        "framing:model-catalog-operator-anchored-override-records",
    ),
    // Whitespace-only observed model IDs cannot become drift candidates; accepted IDs are compared exactly and never trimmed.
    (
        "crates/codeflow-core/src/model_catalog/resolve.rs",
        "Catalog::drift_candidate",
        "trim",
        1,
        "schema-reject-only",
    ),
    // Rejects whitespace-only schema strings; accepted catalog identity strings remain byte-for-byte unchanged.
    (
        "crates/codeflow-core/src/model_catalog/validate.rs",
        "nonempty",
        "trim",
        1,
        "schema-reject-only",
    ),
    // Rejects blank fields or paths with surrounding whitespace; accepted names and repository-relative references remain unchanged.
    (
        "crates/codeflow-core/src/model_qualification.rs",
        "validate_nonempty",
        "trim",
        1,
        "schema-reject-only",
    ),
    // Rejects blank fields or paths with surrounding whitespace; accepted names and repository-relative references remain unchanged.
    (
        "crates/codeflow-core/src/model_qualification.rs",
        "validate_repository_relative_reference",
        "trim",
        2,
        "schema-reject-only",
    ),
    // Regex captures are already valid text; a failed decimal component parse becomes RFC3339 validation Err, not a timestamp default. validate_qualification propagates the returned RFC3339 Err.
    (
        "crates/codeflow-core/src/model_qualification.rs",
        "validate_rfc3339",
        "obtain-absent:as_str",
        1,
        "unproven",
    ),
    // Operates on Markdown table visible text and spacing when recognizing the Read column and narrative trigger; link destinations remain separate exact values.
    (
        "crates/codeflow-core/src/reading.rs",
        "Scanner::content",
        "trim",
        1,
        "grammar:markdown-reading-table",
    ),
    // Operates on Markdown table visible text and spacing when recognizing the Read column and narrative trigger; link destinations remain separate exact values.
    (
        "crates/codeflow-core/src/reading.rs",
        "Scanner::end",
        "trim",
        1,
        "grammar:markdown-reading-table",
    ),
    // Operates on Markdown table visible text and spacing when recognizing the Read column and narrative trigger; link destinations remain separate exact values.
    (
        "crates/codeflow-core/src/reading.rs",
        "Scanner::start",
        "trim",
        2,
        "grammar:markdown-reading-table",
    ),
    // Requires a nonblank explanatory trigger/reason for a conditional read; does not rewrite link or artifact identities.
    (
        "crates/codeflow-core/src/reading.rs",
        "graph_faults",
        "trim",
        1,
        "schema-reject-only",
    ),
    // Requires a nonblank explanatory trigger/reason for a conditional read; does not rewrite link or artifact identities.
    (
        "crates/codeflow-core/src/reading.rs",
        "inventory_faults",
        "trim",
        2,
        "schema-reject-only",
    ),
    // Rejects code spans containing any whitespace before considering them paths; accepted path strings are unchanged.
    (
        "crates/codeflow-core/src/reading.rs",
        "is_code_path",
        "is_whitespace",
        1,
        "schema-reject-only",
    ),
    // Normalizes narrative sentences for the reading map; filesystem link destinations are extracted and stored separately.
    (
        "crates/codeflow-core/src/reading.rs",
        "normalized",
        "split_whitespace",
        1,
        "prose",
    ),
    // Builds search documents per JSONL physical line, skipping blank lines; indexed content and path keys are retained and this index grants no authority.
    (
        "crates/codeflow-core/src/recall.rs",
        "index_file",
        "lines",
        1,
        "framing:recall-index-file-records",
    ),
    // JSON parse is used solely for a search title; the full original event text and exact reversible source path are still indexed.
    (
        "crates/codeflow-core/src/recall.rs",
        "index_file",
        "obtain-absent:from_str",
        1,
        "display",
    ),
    // Builds search documents per JSONL physical line, skipping blank lines; indexed content and path keys are retained and this index grants no authority.
    (
        "crates/codeflow-core/src/recall.rs",
        "index_file",
        "trim",
        1,
        "grammar:jsonl-search",
    ),
    // Derives a display/search title from Markdown heading prose; the source path and indexed document identity are separate fields.
    (
        "crates/codeflow-core/src/recall.rs",
        "markdown_title",
        "lines",
        1,
        "prose",
    ),
    // Derives a display/search title from Markdown heading prose; the source path and indexed document identity are separate fields.
    (
        "crates/codeflow-core/src/recall.rs",
        "markdown_title",
        "trim",
        1,
        "prose",
    ),
    // var_os has no decode-error variant: only genuine HOME/USERPROFILE absence selects the next native path; present OS bytes are preserved.
    (
        "crates/codeflow-core/src/registry.rs",
        "codeflow_home",
        "obtain-absent:var_os",
        1,
        "format-contract",
    ),
    // Chooses and formats an error explanation only after local release preflight failed; JSON success is parsed directly from bytes.
    (
        "crates/codeflow-core/src/release_local.rs",
        "failure",
        "trim",
        2,
        "display",
    ),
    // Trims the end of a rendered finding message solely to indent its display; remedy and authority are already selected.
    (
        "crates/codeflow-core/src/remedy.rs",
        "Finding::line",
        "trim_end",
        1,
        "display",
    ),
    // Trims the combined stderr/stdout message only for a failed gh command; exit status determines the result.
    (
        "crates/codeflow-core/src/remote.rs",
        "GithubProvider::run_gh",
        "trim",
        1,
        "display",
    ),
    // Trims subprocess stderr for a workspace-operation error.
    (
        "crates/codeflow-core/src/root_checkout.rs",
        "git_run",
        "trim",
        1,
        "display",
    ),
    // Rejects whitespace-only configured locations without modifying any accepted path.
    (
        "crates/codeflow-core/src/root_checkout.rs",
        "is_valid_location",
        "trim",
        1,
        "schema-reject-only",
    ),
    // Any HEAD read error refuses with the no-commit stop; the wording assumes an unborn branch, but nothing proceeds.
    (
        "crates/codeflow-core/src/root_checkout.rs",
        "prepare_branch",
        "obtain-absent:head",
        1,
        "unproven",
    ),
    // The name is compared with GIT_HOOK_NAMES, all ASCII, so a name that is not UTF-8 is none of them; the list feeds only advisory doctor and update notes.
    (
        "crates/codeflow-core/src/scaffold/detect.rs",
        "git_dir_hooks",
        "obtain-absent:into_string",
        1,
        "ascii-marker",
    ),
    // Only failed-process stderr is trimmed for a refusal diagnostic.
    (
        "crates/codeflow-core/src/scaffold/gitutil.rs",
        "failed",
        "trim",
        1,
        "display",
    ),
    // Trims stderr solely when producing an error for a failed Git command.
    (
        "crates/codeflow-core/src/scaffold/gitutil.rs",
        "add_and_commit",
        "trim",
        1,
        "display",
    ),
    // Trims stderr solely when producing an error for a failed Git command.
    (
        "crates/codeflow-core/src/scaffold/gitutil.rs",
        "git_ok",
        "trim",
        1,
        "display",
    ),
    // Trims literal JSON whitespace around an object insertion point, not a JSON string or key; verified reparses and compares the complete expected value.
    (
        "crates/codeflow-core/src/scaffold/json_edit.rs",
        "insert_member",
        "trim_start_matches",
        1,
        "grammar:json-whitespace",
    ),
    // Only removes JSON whitespace around a member delimiter; verified reparses the result and rejects any semantic change beyond the expected object.
    (
        "crates/codeflow-core/src/scaffold/json_edit.rs",
        "remove_member_line",
        "trim_start_matches",
        1,
        "grammar:json-whitespace",
    ),
    // Parses newly generated JSON solely to prove it equals the already parsed expected Value. PR mapping callers refuse None; preserving_edit falls back to serializing that exact expected Value, so no obtained input error or identity is discarded.
    (
        "crates/codeflow-core/src/scaffold/json_edit.rs",
        "verified",
        "obtain-absent:from_str",
        1,
        "format-contract",
    ),
    // Rejects blank generator/version metadata; accepted state identities and file paths remain exact.
    (
        "crates/codeflow-core/src/scaffold/portal/state.rs",
        "Generator::is_valid",
        "trim",
        1,
        "schema-reject-only",
    ),
    // Rejects blank generator/version metadata; accepted state identities and file paths remain exact.
    (
        "crates/codeflow-core/src/scaffold/portal/state.rs",
        "validate",
        "trim",
        1,
        "schema-reject-only",
    ),
    // Markdown heading records accept CRLF/LF; heading indentation/padding now uses literal space/tab and retains Unicode heading text.
    (
        "crates/codeflow-core/src/scaffold/pr_template.rs",
        "headings",
        "lines",
        1,
        "framing:scaffold-pr-template-headings-records",
    ),
    // Managed Markdown/hash-comment regions explicitly normalize CRLF/LF for block comparison and restore destination EOL style; marker indentation uses ASCII blanks.
    (
        "crates/codeflow-core/src/scaffold/region.rs",
        "block_interior",
        "lines",
        1,
        "framing:scaffold-region-block-interior-records",
    ),
    // Managed Markdown/hash-comment regions explicitly normalize CRLF/LF for block comparison and restore destination EOL style; marker indentation uses ASCII blanks.
    (
        "crates/codeflow-core/src/scaffold/region.rs",
        "extract_block",
        "lines",
        1,
        "framing:scaffold-region-extract-block-records",
    ),
    // Normalizes trailing blank lines when composing a managed prose region; destination identity and the existing managed-span search are independent of the removed blank spacing.
    (
        "crates/codeflow-core/src/scaffold/region.rs",
        "upsert_block",
        "trim_end_matches",
        1,
        "prose",
    ),
    // Managed Markdown/hash-comment regions explicitly normalize CRLF/LF for block comparison and restore destination EOL style; marker indentation uses ASCII blanks.
    (
        "crates/codeflow-core/src/scaffold/region.rs",
        "with_eol",
        "lines",
        1,
        "framing:scaffold-region-with-eol-records",
    ),
    // Formats trailing blank lines around generated managed prose before markers; it does not parse or choose paths, commands or identities.
    (
        "crates/codeflow-core/src/scaffold/region.rs",
        "wrap_block",
        "trim_end_matches",
        1,
        "prose",
    ),
    // Composes authored instruction paragraphs with one blank separator; block selection already used tier fields, not trimmed paragraph content.
    (
        "crates/codeflow-core/src/scaffold/rule_map.rs",
        "Kernel::render_blocks",
        "trim_matches",
        1,
        "prose",
    ),
    // The first trimmed run line is only a label in the warning identifying an already-classified workflow step.
    (
        "crates/codeflow-core/src/scaffold/update.rs",
        "scan_order",
        "lines",
        1,
        "display",
    ),
    // The first trimmed run line is only a label in the warning identifying an already-classified workflow step.
    (
        "crates/codeflow-core/src/scaffold/update.rs",
        "scan_order",
        "trim",
        1,
        "display",
    ),
    // None means canonical containment is unproven, so temp_place grants no safe temporary-path exemption; raw catastrophic classification stays active.
    (
        "crates/codeflow-core/src/security/dangerous.rs",
        "canonical_operand",
        "obtain-absent:to_str",
        1,
        "unproven",
    ),
    // Fallback also parses raw PowerShell, whose separators include Unicode whitespace; POSIX composed deletion has its own exact reader.
    (
        "crates/codeflow-core/src/security/dangerous.rs",
        "command_tokens",
        "is_whitespace",
        1,
        "grammar:powershell",
    ),
    // Trailing backslash is an idempotent Windows path separator; quotes are already removed by command_tokens and are preserved here.
    (
        "crates/codeflow-core/src/security/dangerous.rs",
        "dangerous_windows_target",
        "trim_end_matches",
        1,
        "grammar:windows-path",
    ),
    // Arithmetic blank predicate is the explicit space/tab/newline set; Unicode identifier characters remain intact.
    (
        "crates/codeflow-core/src/security/deletion.rs",
        "Reader::arith_reference",
        "trim_matches",
        1,
        "grammar:posix-shell",
    ),
    // None denotes an unproven canonical/glob path and is propagated to Judged::Unproven, never an empty candidate set or a cleared deletion.
    (
        "crates/codeflow-core/src/security/deletion.rs",
        "Reader::glob_paths",
        "obtain-absent:to_str",
        1,
        "unproven",
    ),
    // Owns evaluated command-substitution output framing; the shell removes all trailing LF at this boundary.
    (
        "crates/codeflow-core/src/security/deletion.rs",
        "Reader::substitution",
        "trim_end_matches",
        1,
        "framing:shell-command-substitution",
    ),
    // Arithmetic blank predicate is the explicit space/tab/newline set; Unicode identifier characters remain intact.
    (
        "crates/codeflow-core/src/security/deletion.rs",
        "integer_literal",
        "trim_matches",
        1,
        "grammar:posix-shell",
    ),
    // Predicate combines configured IFS membership with shell_blank (space/tab/newline); other IFS separators retain empty fields.
    (
        "crates/codeflow-core/src/security/deletion.rs",
        "read_fields",
        "trim_matches",
        1,
        "grammar:posix-ifs",
    ),
    // Predicate combines configured IFS membership with shell_blank (space/tab/newline); other IFS separators retain empty fields.
    (
        "crates/codeflow-core/src/security/deletion.rs",
        "read_fields",
        "trim_start_matches",
        2,
        "grammar:posix-ifs",
    ),
    // Global filter rejects non-ASCII whitespace before prose certification; original command receives every normal guard check.
    (
        "crates/codeflow-core/src/security/prose.rs",
        "char_allowed",
        "is_whitespace",
        1,
        "schema-reject-only",
    ),
    // An ancestry error drops that proof; the item becomes Removable only through the separate patch-equivalence proof, otherwise RetainUnproven.
    (
        "crates/codeflow-core/src/status.rs",
        "classify_cleanup",
        "obtain-absent:graph_descendant_of",
        1,
        "unproven",
    ),
    // Cleanup advice only: an unread root HEAD leaves its branch out of the checked-out set, which can only add a removal suggestion that git refuses for a checked-out branch, and an unread linked worktree HEAD gives no tip, so classify_cleanup returns RetainUnproven.
    (
        "crates/codeflow-core/src/status.rs",
        "collect_cleanup",
        "obtain-absent:head",
        2,
        "display",
    ),
    // A failed peel leaves the tip unknown, and classify_cleanup returns RetainUnproven (landing target or revision unavailable).
    (
        "crates/codeflow-core/src/status.rs",
        "collect_cleanup",
        "obtain-absent:peel_to_commit",
        1,
        "unproven",
    ),
    // A repository that cannot be opened lists no cleanup items, so no removal is offered, and status adds a state-unavailable note.
    (
        "crates/codeflow-core/src/status.rs",
        "collect_status",
        "obtain-absent:Repository::open",
        1,
        "unproven",
    ),
    // The branch only labels the printed status view; codeflow status exits 0 and decides nothing from it.
    (
        "crates/codeflow-core/src/status.rs",
        "collect_status",
        "obtain-absent:head",
        1,
        "display",
    ),
    // A failed open returns None, and classify_cleanup maps an unknown working state to RetainUnproven.
    (
        "crates/codeflow-core/src/status.rs",
        "repository_dirty",
        "obtain-absent:Repository::open",
        1,
        "unproven",
    ),
    // A non-string schema_version becomes unsupported version Err; the empty placeholder is never an accepted default. run_gate propagates UnsupportedSchemaVersion as TestingError.
    (
        "crates/codeflow-core/src/testing/config/mod.rs",
        "load_test_config",
        "obtain-absent:as_str",
        1,
        "unproven",
    ),
    // Explicit LF records with one optional protocol CR retain the filename; numeric metadata splits on literal ASCII spaces.
    (
        "crates/codeflow-core/src/testing/coverage/go_cover.rs",
        "parse_go_cover_str",
        "split_terminator",
        1,
        "framing:testing-coverage-go-cover-parse-go-cover-str-records",
    ),
    // Explicit LF records with one optional protocol CR retain SF filenames; remaining trim only detects an entirely blank report for parse-error reporting.
    (
        "crates/codeflow-core/src/testing/coverage/lcov.rs",
        "parse_lcov_str",
        "split_terminator",
        1,
        "framing:testing-coverage-lcov-parse-lcov-str-records",
    ),
    // Explicit LF records with one optional protocol CR retain SF filenames; remaining trim only detects an entirely blank report for parse-error reporting.
    (
        "crates/codeflow-core/src/testing/coverage/lcov.rs",
        "parse_lcov_str",
        "trim",
        1,
        "grammar:lcov",
    ),
    // The fallback only keys gate evidence by the project root: green_base then finds no evidence and selection runs every target, and the gate writes durable evidence only when revision, from the same discovery, succeeded.
    (
        "crates/codeflow-core/src/testing/delivery.rs",
        "durable_root",
        "obtain-absent:Repository::discover",
        1,
        "unproven",
    ),
    // Formats probe stdout/stderr as an evidence observation; process exit status determines probe success.
    (
        "crates/codeflow-core/src/testing/delivery.rs",
        "observation",
        "trim",
        3,
        "display",
    ),
    // None becomes an empty revision, so the gate writes no durable evidence and an empty revision never matches a base in green_base; selection then runs every target.
    (
        "crates/codeflow-core/src/testing/delivery.rs",
        "revision",
        "obtain-absent:Repository::discover",
        1,
        "unproven",
    ),
    // None becomes an empty revision, so the gate writes no durable evidence and an empty revision never matches a base in green_base; selection then runs every target.
    (
        "crates/codeflow-core/src/testing/delivery.rs",
        "revision",
        "obtain-absent:head",
        1,
        "unproven",
    ),
    // var_os returns OS bytes or genuine unset; present CARGO_TARGET_DIR bytes become the native PathBuf without decoding or normalization.
    (
        "crates/codeflow-core/src/testing/delivery.rs",
        "target_dir",
        "obtain-absent:var_os",
        1,
        "format-contract",
    ),
    // The parse only validates the configured coverage glob; an error becomes a Fail check naming the invalid glob pattern.
    (
        "crates/codeflow-core/src/testing/doctor/mod.rs",
        "run_target_checks",
        "obtain-absent:Pattern::new",
        1,
        "schema-reject-only",
    ),
    // Produces a short human-readable gate failure message, not test selection or command operands.
    (
        "crates/codeflow-core/src/testing/gate.rs",
        "first_line",
        "lines",
        1,
        "display",
    ),
    // Produces a short human-readable gate failure message, not test selection or command operands.
    (
        "crates/codeflow-core/src/testing/gate.rs",
        "first_line",
        "trim",
        1,
        "display",
    ),
    // Formats owner PID/directory and stale-lock notes after lock authority has been decided using the lock itself.
    (
        "crates/codeflow-core/src/testing/gate_guard.rs",
        "LockHeld::fmt",
        "trim",
        1,
        "display",
    ),
    // Formats owner PID/directory and stale-lock notes after lock authority has been decided using the lock itself.
    (
        "crates/codeflow-core/src/testing/gate_guard.rs",
        "acquire_full_gate_lock",
        "trim",
        1,
        "display",
    ),
    // Formats owner PID/directory and stale-lock notes after lock authority has been decided using the lock itself.
    (
        "crates/codeflow-core/src/testing/gate_guard.rs",
        "describe_holder",
        "lines",
        1,
        "display",
    ),
    // Formats owner PID/directory and stale-lock notes after lock authority has been decided using the lock itself.
    (
        "crates/codeflow-core/src/testing/gate_guard.rs",
        "describe_holder",
        "trim",
        1,
        "display",
    ),
    // Explicit LF records and exact decimal group= values preserve the lock format; whitespace cannot change a process group identity.
    (
        "crates/codeflow-core/src/testing/gate_guard.rs",
        "live_groups",
        "split_terminator",
        1,
        "framing:testing-gate-guard-live-groups-records",
    ),
    // Splits captured output only to render progress notifications; it neither changes the subprocess output buffer nor gate results.
    (
        "crates/codeflow-core/src/testing/runner/mod.rs",
        "spawn_reader_with_progress",
        "lines",
        1,
        "display",
    ),
    // Strips quote characters only from the displayed serialized runner enum label; target configuration and execution retain the typed Runner value.
    (
        "crates/codeflow-core/src/testing/setup/wizard.rs",
        "run_wizard",
        "trim_matches",
        1,
        "display",
    ),
    // Finds a diagnostic line number after a finding has been determined; content decoding here does not establish document identity.
    (
        "crates/codeflow-core/src/validate/docs.rs",
        "find_line",
        "lines",
        1,
        "display",
    ),
    // Rejects blank verification evidence or standalone explanations; accepted record IDs and evidence strings remain unchanged.
    (
        "crates/codeflow-core/src/validate/docs.rs",
        "lint_capabilities",
        "trim",
        1,
        "schema-reject-only",
    ),
    // Rejects blank verification evidence or standalone explanations; accepted record IDs and evidence strings remain unchanged.
    (
        "crates/codeflow-core/src/validate/docs.rs",
        "lint_task_parent",
        "trim",
        2,
        "schema-reject-only",
    ),
    // Git log emits one merge subject per LF record; subjects are metadata with newline already collapsed by Git, and branch extraction uses literal markers.
    (
        "crates/codeflow-core/src/validate/mod.rs",
        "landed_pull_requests",
        "lines",
        1,
        "framing:validate-mod-landed-pull-requests-records",
    ),
    // Normalizes open-question prose for reports, without selecting refs, paths, commands, or config keys.
    (
        "crates/codeflow-core/src/validate/mod.rs",
        "open_questions",
        "trim",
        2,
        "prose",
    ),
    // Blank required integration_target or standalone_reason is rejected; nonblank integration targets are validated in their original exact form.
    (
        "crates/codeflow-core/src/validate/mod.rs",
        "validate_task",
        "trim",
        3,
        "schema-reject-only",
    ),
    // This duplicate-ID prepass skips unreadable bytes, but the same path is always passed to validate_epic/validate_task/validate_spec immediately afterwards; their read Err adds a ValidationReport issue, which rejects the check.
    (
        "crates/codeflow-core/src/validate/mod.rs",
        "validate_workgraph",
        "obtain-absent:read",
        1,
        "unproven",
    ),
    // The entire pointer object is validated as string-valued first; failure adds a records-pointer issue before these proven as_str conversions.
    (
        "crates/codeflow-core/src/validate/portal.rs",
        "configured_records",
        "obtain-absent:as_str",
        2,
        "schema-reject-only",
    ),
    // Undecodable percent-encoded URL returns None, and fragment verification records an invalid-link finding instead of resolving another target. verify_portal_fragments adds a finding for unreadable path or fragment encoding and validate_portal_with returns a non-clean report.
    (
        "crates/codeflow-core/src/validate/portal.rs",
        "decode_percent",
        "obtain-absent:from_utf8",
        1,
        "unproven",
    ),
    // Trims error details only after a failed Git status, returning Err regardless of diagnostic contents.
    (
        "crates/codeflow-core/src/validate/portal.rs",
        "git_output_bounded",
        "trim",
        2,
        "display",
    ),
    // Non-UTF8 path components fail the portable-path proof; the caller reports the artifact path as unsafe rather than substituting a filename. collect_reserved_public_files and source/artifact enumeration add unsafe-path findings to PortalValidationReport, which validate_portal_with rejects.
    (
        "crates/codeflow-core/src/validate/portal.rs",
        "portable_relative_path",
        "obtain-absent:to_str",
        1,
        "unproven",
    ),
    // YAML line endings are normalized explicitly before recovering IDs from malformed documents; key/value separation uses only literal space/tab.
    (
        "crates/codeflow-core/src/validate/portal.rs",
        "recover_unavailable_ids",
        "lines",
        1,
        "framing:validate-portal-recover-unavailable-ids-records",
    ),
    // Non-UTF8 path decoding selects a backslash sentinel that the immediately following condition rejects; no fallback path is joined. safe_join adds an unsafe path issue before returning None; validate_portal_with rejects the report.
    (
        "crates/codeflow-core/src/validate/portal.rs",
        "safe_join",
        "obtain-absent:to_str",
        1,
        "unproven",
    ),
    // Any Unicode whitespace rejects a repository URL; accepted URLs are preserved exactly.
    (
        "crates/codeflow-core/src/validate/portal.rs",
        "valid_repository_url",
        "is_whitespace",
        1,
        "schema-reject-only",
    ),
    // Authoritative config bytes were parsed by verify_config_contract earlier; malformed bytes already add a PortalValidationReport issue. This second parse cannot remove that issue or produce a clean report.
    (
        "crates/codeflow-core/src/validate/portal.rs",
        "validate_portal_with",
        "obtain-absent:from_slice",
        1,
        "unproven",
    ),
    // Remaining trims reject blank page titles/status metadata; commit comparison removes exactly one LF and retains other bytes.
    (
        "crates/codeflow-core/src/validate/portal.rs",
        "validate_portal_with",
        "trim",
        2,
        "schema-reject-only",
    ),
    // verify_config_metadata has already required bounded string title/description and reported errors before these proven conversions.
    (
        "crates/codeflow-core/src/validate/portal.rs",
        "verify_config_contract",
        "obtain-absent:as_str",
        2,
        "schema-reject-only",
    ),
    // Whitespace-only version metadata is invalid; accepted Pagefind artifact names are compared exactly.
    (
        "crates/codeflow-core/src/validate/portal.rs",
        "verify_pagefind_entry",
        "trim",
        1,
        "schema-reject-only",
    ),
    // Rendered Markdown provenance markers are whole CRLF/LF comment records; snippet-source decode failure emits an explicit issue before any range comparison.
    (
        "crates/codeflow-core/src/validate/portal.rs",
        "verify_rendered_claims",
        "lines",
        1,
        "framing:validate-portal-verify-rendered-claims-records",
    ),
    // Rendered Markdown provenance markers are whole CRLF/LF comment records; snippet-source decode failure emits an explicit issue before any range comparison.
    (
        "crates/codeflow-core/src/validate/portal.rs",
        "verify_snippets",
        "lines",
        1,
        "framing:validate-portal-verify-snippets-records",
    ),
    // Rendered Markdown provenance markers are whole CRLF/LF comment records; snippet-source decode failure emits an explicit issue before any range comparison. validate_portal_with rejects its non-clean PortalValidationReport.
    (
        "crates/codeflow-core/src/validate/portal.rs",
        "verify_snippets",
        "obtain-absent:from_utf8",
        1,
        "unproven",
    ),
    // Whitespace-only optional explanatory notes are invalid; accepted declaration paths and placement identifiers are unchanged.
    (
        "crates/codeflow-core/src/validate/portal/figures.rs",
        "PageClassEntry::allowed",
        "trim",
        1,
        "schema-reject-only",
    ),
    // Matches the Markdown adapter fence-tail blank grammar when counting narrative words and extracting sections; no filename or shell operand is normalized.
    (
        "crates/codeflow-core/src/validate/portal/figures.rs",
        "altitude_words",
        "trim",
        1,
        "grammar:markdown-fences",
    ),
    // Mirrors the portal adapter blank-line/BOM trimming for source-region framing; source bytes and their digest remain independently verified.
    (
        "crates/codeflow-core/src/validate/portal/figures.rs",
        "as_is_region_start",
        "is_whitespace",
        1,
        "grammar:ecmascript-markdown-adapter",
    ),
    // Mirrors the portal adapter blank-line/BOM trimming for source-region framing; source bytes and their digest remain independently verified.
    (
        "crates/codeflow-core/src/validate/portal/figures.rs",
        "as_is_region_start",
        "trim_matches",
        1,
        "grammar:ecmascript-markdown-adapter",
    ),
    // Matches the Markdown adapter fence-tail blank grammar when counting narrative words and extracting sections; no filename or shell operand is normalized.
    (
        "crates/codeflow-core/src/validate/portal/figures.rs",
        "markdown_sections",
        "trim",
        1,
        "grammar:markdown-fences",
    ),
    // Uses the portal adapter heading normalization for visible panel/title/anchor correspondence; this defined heading grammar is independent of filesystem names.
    (
        "crates/codeflow-core/src/validate/portal/figures.rs",
        "panel_of_anchor",
        "trim",
        1,
        "grammar:markdown-heading-identity",
    ),
    // Uses the portal adapter heading normalization for visible panel/title/anchor correspondence; this defined heading grammar is independent of filesystem names.
    (
        "crates/codeflow-core/src/validate/portal/figures.rs",
        "slug_heading",
        "trim",
        1,
        "grammar:markdown-heading-identity",
    ),
    // Uses the portal adapter heading normalization for visible panel/title/anchor correspondence; this defined heading grammar is independent of filesystem names.
    (
        "crates/codeflow-core/src/validate/portal/figures.rs",
        "title_matches",
        "trim",
        1,
        "grammar:markdown-heading-identity",
    ),
    // UTF8/JSON failure adds a derived-source-does-not-resolve issue; validate_portal_with returns a non-clean PortalValidationReport and portal validation refuses it.
    (
        "crates/codeflow-core/src/validate/portal/figures.rs",
        "verify_derived_binding",
        "obtain-absent:from_str",
        1,
        "unproven",
    ),
    // UTF8/JSON failure adds a derived-source-does-not-resolve issue; validate_portal_with returns a non-clean PortalValidationReport and portal validation refuses it.
    (
        "crates/codeflow-core/src/validate/portal/figures.rs",
        "verify_derived_binding",
        "obtain-absent:from_utf8",
        1,
        "unproven",
    ),
    // Unreadable source yields no derived row count and fails the valid lookup condition, which requires Some(rows). validate_portal_with rejects the PortalValidationReport lookup finding.
    (
        "crates/codeflow-core/src/validate/portal/figures.rs",
        "verify_lookup",
        "obtain-absent:from_utf8",
        1,
        "unproven",
    ),
    // Unreadable source adds a page-class-source issue before any class/binding checks; no missing-source exemption remains. validate_portal_with rejects the PortalValidationReport source finding.
    (
        "crates/codeflow-core/src/validate/portal/figures.rs",
        "verify_page_class",
        "obtain-absent:from_utf8",
        1,
        "unproven",
    ),
    // Unreadable source immediately returns a failed source-region finding. validate_portal_with rejects the returned source-region finding.
    (
        "crates/codeflow-core/src/validate/portal/figures.rs",
        "verify_source_region",
        "obtain-absent:from_utf8",
        1,
        "unproven",
    ),
    // Recognizes HTML comment marker text emitted by Markdown events; only marker scaffolding is inspected, not source or artifact path operands.
    (
        "crates/codeflow-core/src/validate/portal/figures/dom.rs",
        "render_markdown",
        "trim",
        1,
        "grammar:markdown-html-marker",
    ),
    // The explicit c<=space trim and tab/CR/LF removal follow URL scheme preprocessing to reject active-script destinations; accepted URLs are not rewritten.
    (
        "crates/codeflow-core/src/validate/portal/figures/dom.rs",
        "runs_script",
        "trim_matches",
        1,
        "grammar:url-scheme",
    ),
    // Rendered heading visible text is trimmed for panel/title correspondence, matching the portal adapter; artifact/link paths remain separate attributes.
    (
        "crates/codeflow-core/src/validate/portal/figures/dom.rs",
        "walk",
        "trim",
        3,
        "grammar:rendered-headings",
    ),
    // Every label reference is verified as a JSON string immediately before rendering; non-string references return Err before this otherwise-unreachable placeholder.
    (
        "crates/codeflow-core/src/validate/portal/figures/render.rs",
        "draw_item",
        "obtain-absent:as_str",
        1,
        "schema-reject-only",
    ),
    // Mirrors the portal renderer ECMAScript whitespace set, excluding NEL and including BOM, when comparing SVG path geometry text.
    (
        "crates/codeflow-core/src/validate/portal/figures/render.rs",
        "normalize_path",
        "is_whitespace",
        1,
        "grammar:ecmascript-svg",
    ),
    // String JSON values display as text and non-strings use js_stringify; this is deliberate polymorphic table rendering, not failed decoding or defaulted identity.
    (
        "crates/codeflow-core/src/validate/portal/figures/render.rs",
        "twin_table",
        "obtain-absent:as_str",
        1,
        "display",
    ),
    // Folds descriptive prose to one escaped Markdown table cell; source keys and destinations are separately retained.
    (
        "crates/codeflow-core/src/validate/portal/lookups.rs",
        "cell",
        "split_whitespace",
        1,
        "prose",
    ),
    // Extracts hook stage labels from an authored Markdown table; table-cell and code-span padding define these displayed lookup labels.
    (
        "crates/codeflow-core/src/validate/portal/lookups.rs",
        "hook_stage_names",
        "lines",
        1,
        "framing:validate-portal-lookups-hook-stage-names-records",
    ),
    // Extracts hook stage labels from an authored Markdown table; table-cell and code-span padding define these displayed lookup labels.
    (
        "crates/codeflow-core/src/validate/portal/lookups.rs",
        "hook_stage_names",
        "trim",
        1,
        "grammar:markdown-table",
    ),
    // Unreadable embedded skill text immediately becomes an Err naming that skill; no missing-asset fallback is accepted. verify_lookup_page propagates the skill source Err.
    (
        "crates/codeflow-core/src/validate/portal/lookups.rs",
        "skill_catalog",
        "obtain-absent:from_utf8",
        1,
        "unproven",
    ),
    // Frontmatter parse failure returns None to skill_catalog, which returns Err naming the missing readable description; lookup verification propagates refusal.
    (
        "crates/codeflow-core/src/validate/portal/lookups.rs",
        "skill_description",
        "obtain-absent:from_str",
        1,
        "unproven",
    ),
    // Only permits whitespace after the generated catalog region; any extra nonblank prose causes Err and generated identifiers are untouched.
    (
        "crates/codeflow-core/src/validate/portal/lookups.rs",
        "verify_lookup_page",
        "trim",
        1,
        "schema-reject-only",
    ),
    // Invalid UTF-8 in origin/HEAD returns Err; target_record converts the error to Presence::Unreadable, preventing target-based criteria relaxation.
    (
        "crates/codeflow-core/src/workgraph/acceptance.rs",
        "default_target_name",
        "obtain-absent:from_utf8",
        1,
        "unproven",
    ),
    // False is the strict side: the range is then judged as possibly reopening the task, which only adds reopen findings and withholds stacking on its review.
    (
        "crates/codeflow-core/src/workgraph/acceptance.rs",
        "head_is_declared_target",
        "obtain-absent:head",
        1,
        "unproven",
    ),
    // A tree that cannot be read returns Some(the trees cannot be read), which reviewed_span_problem and stacked turn into a refusal finding.
    (
        "crates/codeflow-core/src/workgraph/acceptance.rs",
        "later_change",
        "obtain-absent:find_commit",
        1,
        "unproven",
    ),
    // Whitespace-only narrative journey reasons are rejected; retained reason text never names a revision, branch, path or identity.
    (
        "crates/codeflow-core/src/workgraph/acceptance.rs",
        "leaf_journey",
        "trim",
        1,
        "schema-reject-only",
    ),
    // This reader consumes raw record physical LF/CRLF lines once before comparing reviewed content; downstream scan_record/frontmatter_len do not remove CR again.
    (
        "crates/codeflow-core/src/workgraph/acceptance.rs",
        "reviewed_part",
        "lines",
        1,
        "framing:reviewed-record-lines",
    ),
    // Only interpolated refusal messages trim the evidence; commit lookup and amendment matching use the original exact evidence.
    (
        "crates/codeflow-core/src/workgraph/acceptance.rs",
        "waiver_problem",
        "trim",
        7,
        "display",
    ),
    // Normalizes human-authored standalone explanation and rejects an empty narrative; task identity and integration target are separate exact fields.
    (
        "crates/codeflow-core/src/workgraph/allocate.rs",
        "create_task_with",
        "trim",
        2,
        "prose",
    ),
    // Normalizes a human-authored one-line title, not the allocated record id or target identity.
    (
        "crates/codeflow-core/src/workgraph/allocate.rs",
        "record_title",
        "trim",
        1,
        "prose",
    ),
    // Invalid UTF-8 returns the entry problem; range_problem treats that returned problem as a planning-only refusal.
    (
        "crates/codeflow-core/src/workgraph/amendment.rs",
        "entry_problem",
        "obtain-absent:from_utf8",
        1,
        "unproven",
    ),
    // A discovery error becomes Err inside the status read, and epic_problem then returns Some(cannot read the epic records), a refusal.
    (
        "crates/codeflow-core/src/workgraph/amendment.rs",
        "epic_problem",
        "obtain-absent:Repository::discover",
        1,
        "unproven",
    ),
    // A task file that cannot be read is None, which held_at turns into doubt ("cannot be read, so whether it is this task cannot be told"); the tree's answer becomes unproven, never absent.
    (
        "crates/codeflow-core/src/workgraph/landing.rs",
        "RecordStore::text",
        "obtain-absent:find_blob",
        1,
        "unproven",
    ),
    // A task file that is not UTF-8 is None, never a lossy rendering that could parse with other values; held_at turns None into doubt, so the answer is unproven.
    (
        "crates/codeflow-core/src/workgraph/landing.rs",
        "RecordStore::text",
        "obtain-absent:from_utf8",
        1,
        "unproven",
    ),
    // An entry already checked as 40 lowercase hex that Oid::from_str still rejects joins the refused reasons and yields Baseline::Refused; judge_records and the range judges surface baseline_refusal and deny the migration allowance. A lookup error other than NotFound is refused too.
    (
        "crates/codeflow-core/src/workgraph/lifecycle.rs",
        "Baseline::from_entries",
        "obtain-absent:from_str",
        1,
        "unproven",
    ),
    // Its only caller, stale_warnings, adds to Verdict.warnings, which validate keeps out of issues; is_clean and the ci and work checks read issues only.
    (
        "crates/codeflow-core/src/workgraph/lifecycle.rs",
        "has_active_branch",
        "obtain-absent:find_reference",
        1,
        "display",
    ),
    // The result only selects the stale in_progress warning text in stale_warnings; validate warnings never block (is_clean reads issues only).
    (
        "crates/codeflow-core/src/workgraph/lifecycle.rs",
        "has_active_branch",
        "obtain-absent:graph_descendant_of",
        1,
        "display",
    ),
    // The target tip only feeds the stale in_progress warning; report warnings are printed by validate and never fail ci or work.
    (
        "crates/codeflow-core/src/workgraph/lifecycle.rs",
        "has_active_branch",
        "obtain-absent:peel_to_commit",
        1,
        "display",
    ),
    // Only failed Git stderr is trimmed in the returned error; history stdout records use strict bytes and exact framing.
    (
        "crates/codeflow-core/src/workgraph/lifecycle.rs",
        "shipped_in_history",
        "trim",
        1,
        "display",
    ),
    // A failed discovery only changes the stale in_progress warnings in Verdict.warnings, which validate_workgraph keeps out of issues.
    (
        "crates/codeflow-core/src/workgraph/lifecycle.rs",
        "stale_warnings",
        "obtain-absent:Repository::discover",
        1,
        "display",
    ),
    // Normalizes the human ADR title before rendering heading/YAML; numeric ADR allocation uses separate directory identities.
    (
        "crates/codeflow-core/src/workgraph/light_paths.rs",
        "create_adr_with",
        "trim",
        1,
        "prose",
    ),
    // Trims stderr only after unsuccessful Git exit; successful stdout is decoded strictly and returned without trim.
    (
        "crates/codeflow-core/src/workgraph/light_paths.rs",
        "git",
        "trim",
        1,
        "display",
    ),
    // An open failure falls through to the spawned git cherry check, which is true only when git proves it; false becomes RetainUnproven in status cleanup.
    (
        "crates/codeflow-core/src/workgraph/readiness.rs",
        "cherry_landed",
        "obtain-absent:Repository::open",
        1,
        "unproven",
    ),
    // An unresolved name falls back to the spawned git cherry check, which errs to false; status cleanup then gives RetainUnproven, never proven landed.
    (
        "crates/codeflow-core/src/workgraph/readiness.rs",
        "cherry_landed",
        "obtain-absent:peel_to_commit",
        1,
        "unproven",
    ),
    // A read failure returns None, so cherry_landed_in lets the spawned git cherry decide alone, and any failure there gives false (not landed).
    (
        "crates/codeflow-core/src/workgraph/readiness.rs",
        "could_be_patch_equivalent",
        "obtain-absent:find_commit",
        2,
        "unproven",
    ),
    // FETCH_HEAD metadata supplies only the snapshot timestamp rendered by Backlog::snapshot_line; missing timestamp never changes target tips, readiness or claims.
    (
        "crates/codeflow-core/src/workgraph/readiness.rs",
        "fetched_at",
        "obtain-absent:metadata",
        1,
        "display",
    ),
    // Trims stderr only for a failed Git command, never the successful decision output.
    (
        "crates/codeflow-core/src/workgraph/readiness.rs",
        "git",
        "trim",
        1,
        "display",
    ),
    // Trims failed-command stderr for its error message; successful command stdout remains exact bytes.
    (
        "crates/codeflow-core/src/workgraph/readiness.rs",
        "git_bytes",
        "trim",
        1,
        "display",
    ),
    // Not landed is the strict side: an open claim refuses claim_on, keeps the branch in cleanup, and shows the work Active rather than Ready.
    (
        "crates/codeflow-core/src/workgraph/readiness.rs",
        "landed",
        "obtain-absent:graph_descendant_of",
        1,
        "unproven",
    ),
    // None fails the Some(false) test in landed, so the tip counts as not landed: an open claim that claim_on refuses and cleanup retains.
    (
        "crates/codeflow-core/src/workgraph/readiness.rs",
        "on_first_parent_line",
        "obtain-absent:find_commit",
        1,
        "unproven",
    ),
    // An unreadable tip is recorded as the zero id, which landed never proves, so it stays an open claim and claim_on refuses.
    (
        "crates/codeflow-core/src/workgraph/readiness.rs",
        "remote_claims",
        "obtain-absent:find_commit",
        1,
        "unproven",
    ),
    // An ancestry read failure gives Err(the standalone branch must contain its current target), so claim_on refuses.
    (
        "crates/codeflow-core/src/workgraph/readiness.rs",
        "standalone_claim_base",
        "obtain-absent:graph_descendant_of",
        1,
        "unproven",
    ),
    // ASCII spaces, tabs, CR and LF select the first nonblank block header; these are leading YAML/Markdown separators, not a delimiter removal. Exact header/schema parsing still refuses malformed content.
    (
        "crates/codeflow-core/src/workgraph/record_text.rs",
        "acceptance_blocks",
        "trim_start_matches",
        1,
        "grammar:acceptance-leading-separators",
    ),
    // Whitespace-only owner/window/follow-up values are rejected; an accepted follow-up is validated as the unchanged exact task id.
    (
        "crates/codeflow-core/src/workgraph/record_text.rs",
        "after_release_problems",
        "trim",
        1,
        "schema-reject-only",
    ),
    // Whitespace-only prose explanation for no follow-ups is rejected; referenced task identifiers use explicit ASCII grammar elsewhere.
    (
        "crates/codeflow-core/src/workgraph/record_text.rs",
        "check_block",
        "trim",
        1,
        "schema-reject-only",
    ),
    // Trimming only classifies an empty narrative/template placeholder as blank and thus rejects missing evidence.
    (
        "crates/codeflow-core/src/workgraph/record_text.rs",
        "is_blank_value",
        "trim",
        1,
        "schema-reject-only",
    ),
    // Words and punctuation are inspected only to distinguish narrative deliverable text from placeholder text; no word becomes a filesystem or task identity.
    (
        "crates/codeflow-core/src/workgraph/record_text.rs",
        "is_entry",
        "split_whitespace",
        1,
        "prose",
    ),
    // Words and punctuation are inspected only to distinguish narrative deliverable text from placeholder text; no word becomes a filesystem or task identity.
    (
        "crates/codeflow-core/src/workgraph/record_text.rs",
        "is_entry",
        "trim_matches",
        1,
        "prose",
    ),
    // Removes surrounding prose punctuation while recognizing whether narrative mentions a path-shaped deliverable; this does not resolve, access, compare or authorize the extracted path.
    (
        "crates/codeflow-core/src/workgraph/record_text.rs",
        "is_path",
        "trim_end_matches",
        1,
        "prose",
    ),
    // Removes surrounding prose punctuation while recognizing whether narrative mentions a path-shaped deliverable; this does not resolve, access, compare or authorize the extracted path.
    (
        "crates/codeflow-core/src/workgraph/record_text.rs",
        "is_path",
        "trim_start_matches",
        1,
        "prose",
    ),
    // Splits a narrative description into words to recognize a path-shaped deliverable mention; words are never used as actual filesystem paths.
    (
        "crates/codeflow-core/src/workgraph/record_text.rs",
        "names_path",
        "split_whitespace",
        1,
        "prose",
    ),
    // Public raw acceptance-text reader removes LF/CRLF once. acceptance_blocks passes already-framed lines directly to parse_acceptance_lines so content CR is never stripped again.
    (
        "crates/codeflow-core/src/workgraph/record_text.rs",
        "parse_acceptance",
        "lines",
        1,
        "framing:acceptance-input-lines",
    ),
    // Raw Markdown body physical-line reader owns LF/CRLF removal. scan receives framed lines and no longer strips CR; acceptance_blocks uses parse_acceptance_lines on its assembled framed lines.
    (
        "crates/codeflow-core/src/workgraph/record_text.rs",
        "section",
        "lines",
        1,
        "framing:record-section-lines",
    ),
    // Raw Markdown body reader owns LF/CRLF removal; both scan and returned selected lines preserve remaining CR content.
    (
        "crates/codeflow-core/src/workgraph/record_text.rs",
        "section_markdown",
        "lines",
        1,
        "framing:record-section-markdown-lines",
    ),
    // A failed read shortens the chain, and each consumer of contains-false is the strict side: off_line refuses the bridge, uncovered withholds legacy, and line_of None judges the merge as direct work.
    (
        "crates/codeflow-core/src/workgraph/release_line.rs",
        "Lines::chain",
        "obtain-absent:find_commit",
        1,
        "unproven",
    ),
    // A cut-short order only moves positions toward 0 and keeps their order, so ties stop a completion counting as newer and earlier findings stand.
    (
        "crates/codeflow-core/src/workgraph/release_line.rs",
        "Lines::position",
        "obtain-absent:find_commit",
        1,
        "unproven",
    ),
    // An error makes position return None, so a completion cannot supersede (newer is false) and earlier findings stand.
    (
        "crates/codeflow-core/src/workgraph/release_line.rs",
        "Lines::position",
        "obtain-absent:graph_descendant_of",
        1,
        "unproven",
    ),
    // A commit that cannot be read is treated as missing, fetched and checked again; if still unreadable, ensure_objects returns Err and the judge refuses.
    (
        "crates/codeflow-core/src/workgraph/release_line.rs",
        "ensure_objects",
        "obtain-absent:find_commit",
        1,
        "unproven",
    ),
    // The first-parent walk just parsed the oldest commit with its errors refused, and the loop below reads it again with a refusal, so a failed read never reaches a judgement.
    (
        "crates/codeflow-core/src/workgraph/release_line.rs",
        "judge",
        "obtain-absent:find_commit",
        1,
        "unproven",
    ),
    // OID parse failure becomes the table error through ok_or_else; table_on_chain/bridge propagate it and release_findings refuses the release transition.
    (
        "crates/codeflow-core/src/workgraph/release_line.rs",
        "parse_table",
        "obtain-absent:from_str",
        1,
        "unproven",
    ),
    // Only rejects an empty/whitespace-only declared pattern; nonempty glob/path text is returned unchanged for exact policy interpretation.
    (
        "crates/codeflow-core/src/workgraph/release_line.rs",
        "pattern_problem",
        "trim",
        1,
        "schema-reject-only",
    ),
    // first_parent_path's revwalk parsed the oldest commit with its errors refused (libgit2 reads every walked commit object, even with a commit-graph), so this read fails only if the object vanishes between two reads.
    (
        "crates/codeflow-core/src/workgraph/release_line.rs",
        "release_owner",
        "obtain-absent:find_commit",
        1,
        "unproven",
    ),
    // Markdown output construction removes trailing blank-line spacing and writes a fixed section separation; this is rendering, not consuming a Git or name delimiter.
    (
        "crates/codeflow-core/src/workgraph/status_verb.rs",
        "append_to_section",
        "trim_end_matches",
        3,
        "format-contract",
    ),
    // An unreadable HEAD returns the finding no HEAD commit to bind the acceptance block to, which gate_binding refuses at block level and carries as a warning otherwise.
    (
        "crates/codeflow-core/src/workgraph/status_verb.rs",
        "binding",
        "obtain-absent:head",
        1,
        "unproven",
    ),
    // An unreadable HEAD returns the finding no HEAD commit to bind the acceptance block to, which gate_binding refuses at block level and carries as a warning otherwise.
    (
        "crates/codeflow-core/src/workgraph/status_verb.rs",
        "binding",
        "obtain-absent:peel_to_commit",
        1,
        "unproven",
    ),
    // An unreadable HEAD becomes a binding finding that gate_binding refuses where git.work_records blocks.
    (
        "crates/codeflow-core/src/workgraph/status_verb.rs",
        "epic_binding",
        "obtain-absent:head",
        1,
        "unproven",
    ),
    // An unreadable HEAD becomes a binding finding that gate_binding refuses where git.work_records blocks.
    (
        "crates/codeflow-core/src/workgraph/status_verb.rs",
        "epic_binding",
        "obtain-absent:peel_to_commit",
        1,
        "unproven",
    ),
    // Markdown renderer joins complete section documents with fixed blank-line spacing; no parsed identity or authority is trimmed.
    (
        "crates/codeflow-core/src/workgraph/status_verb.rs",
        "insert_section_before",
        "trim_end_matches",
        3,
        "format-contract",
    ),
    // None withholds the own-range waiver route: waiver_problem then returns a problem, so a failure can only add a binding finding.
    (
        "crates/codeflow-core/src/workgraph/status_verb.rs",
        "own_range_base",
        "obtain-absent:head",
        1,
        "unproven",
    ),
    // None withholds the own-range waiver route: waiver_problem then returns a problem, so a failure can only add a binding finding.
    (
        "crates/codeflow-core/src/workgraph/status_verb.rs",
        "own_range_base",
        "obtain-absent:merge_base",
        1,
        "unproven",
    ),
    // An unresolved target gives None, and waiver_problem then refuses an own-range waiver as not on the target; nothing is allowed by the fold.
    (
        "crates/codeflow-core/src/workgraph/status_verb.rs",
        "own_range_base",
        "obtain-absent:peel_to_commit",
        1,
        "unproven",
    ),
    // Generated acceptance block text has its trailing render newline removed before insertion in a new Markdown fence; fields were validated separately.
    (
        "crates/codeflow-core/src/workgraph/status_verb.rs",
        "propose",
        "trim_end_matches",
        1,
        "format-contract",
    ),
    // Rejects missing/whitespace-only single-line required form fields and returns accepted text unchanged.
    (
        "crates/codeflow-core/src/workgraph/status_verb.rs",
        "required",
        "trim",
        1,
        "schema-reject-only",
    ),
    // An unread HEAD gives an empty branch, which carries no task, so standalone_at_head refuses a standalone record not yet on the target as not anchored; check_task_anchor reads the branch nowhere else.
    (
        "crates/codeflow-core/src/workgraph/work_start.rs",
        "check_work_start_anchored",
        "obtain-absent:head",
        1,
        "unproven",
    ),
    // unwrap_or(false) makes the pin not an ancestor, and check_work_start_on returns InvalidGraph (pin is not an ancestor of HEAD), so work start refuses.
    (
        "crates/codeflow-core/src/workgraph/work_start.rs",
        "check_work_start_on",
        "obtain-absent:graph_descendant_of",
        1,
        "unproven",
    ),
    // unwrap_or(false) leads to a DependencyPin refusal (not on line).
    (
        "crates/codeflow-core/src/workgraph/work_start.rs",
        "pinned_dependency",
        "obtain-absent:graph_descendant_of",
        1,
        "unproven",
    ),
    // A failed check rejects that candidate; only a proven container of all pins is chosen, so a failure gives the right base or the no reviewed base refusal.
    (
        "crates/codeflow-core/src/workgraph/work_start.rs",
        "stack_base_in",
        "obtain-absent:graph_descendant_of",
        1,
        "unproven",
    ),
    // The folded lookup becomes InvalidGraph (reviewed revision does not resolve), a refusal of the admission.
    (
        "crates/codeflow-core/src/workgraph/work_start.rs",
        "standalone_at_head",
        "obtain-absent:find_commit",
        1,
        "unproven",
    ),
    // Rejects a standalone task with no nonblank human reason; it does not normalize task identity, refs or paths.
    (
        "crates/codeflow-core/src/workgraph/work_start.rs",
        "standalone_at_head",
        "trim",
        1,
        "schema-reject-only",
    ),
    // Trimming formats the already-blocked task narrative reason in the refusal, and cannot convert refusal to allow.
    (
        "crates/codeflow-core/src/workgraph/work_start.rs",
        "start_gate",
        "trim",
        1,
        "display",
    ),
    // Rejects a standalone task with only whitespace in its human explanation; exact identifiers and configured targets are separate fields.
    (
        "crates/codeflow-core/src/workgraph/work_start.rs",
        "validate_task_structure",
        "trim",
        1,
        "schema-reject-only",
    ),
    // false makes the only caller, the task allocation, return StoreError::Invalid (target does not resolve), so the task is not created.
    (
        "crates/codeflow-core/src/workgraph/work_start.rs",
        "work_target_resolves",
        "obtain-absent:Repository::discover",
        1,
        "unproven",
    ),
    // Unreadable PID returns BrowserUnavailable in macos_inventory_candidates; owned_process_candidates propagates it so process ownership cannot be certified.
    (
        "crates/codeflow-present/src/browser.rs",
        "macos_inventory_candidates",
        "obtain-absent:from_utf8",
        1,
        "unproven",
    ),
    // Rejects whitespace-only alternative text. Accepted image identity configuration stays unchanged.
    (
        "crates/codeflow-present/src/config.rs",
        "UtilityTokens::validate",
        "trim",
        1,
        "schema-reject-only",
    ),
    // Rejects whitespace-only reply prose. Stored replies keep their original text.
    (
        "crates/codeflow-present/src/conversation.rs",
        "SessionStore::reply",
        "trim",
        1,
        "schema-reject-only",
    ),
    // Reads presentation code/diff text lines to construct the human selection text model.
    (
        "crates/codeflow-present/src/document.rs",
        "Block::canonical_review_text",
        "lines",
        1,
        "framing:canonical-review-lines",
    ),
    // Builds canonical reader-visible review text from prose/diff lines. This is the presentation selection model, not Git or shell parsing.
    (
        "crates/codeflow-present/src/document.rs",
        "Block::canonical_review_text",
        "trim",
        1,
        "prose",
    ),
    // Reads legacy presentation diff text lines for stored selection compatibility.
    (
        "crates/codeflow-present/src/document.rs",
        "Block::legacy_diff_review_text",
        "lines",
        1,
        "framing:legacy-review-lines",
    ),
    // Rejects whitespace-only summary, label or framing prose. Accepted fields are retained unchanged.
    (
        "crates/codeflow-present/src/document.rs",
        "PresentationDocument::validate",
        "trim",
        1,
        "schema-reject-only",
    ),
    // Removes trailing output LF while constructing canonical human Markdown text; original source/IDs are unchanged.
    (
        "crates/codeflow-present/src/document.rs",
        "markdown_text",
        "trim_end_matches",
        2,
        "prose",
    ),
    // Collapses and truncates navigation labels only. Document and block identifiers remain exact.
    (
        "crates/codeflow-present/src/document.rs",
        "prose_nav_label",
        "split_whitespace",
        1,
        "display",
    ),
    // Collapses and truncates navigation labels only. Document and block identifiers remain exact.
    (
        "crates/codeflow-present/src/document.rs",
        "prose_nav_label",
        "trim",
        1,
        "display",
    ),
    // Rejects whitespace-only summary, label or framing prose. Accepted fields are retained unchanged.
    (
        "crates/codeflow-present/src/document.rs",
        "require_nonempty_bounded",
        "trim",
        1,
        "schema-reject-only",
    ),
    // Collapses and truncates navigation labels only. Document and block identifiers remain exact.
    (
        "crates/codeflow-present/src/document.rs",
        "truncate_nav_label",
        "is_whitespace",
        1,
        "display",
    ),
    // Collapses and truncates navigation labels only. Document and block identifiers remain exact.
    (
        "crates/codeflow-present/src/document.rs",
        "truncate_nav_label",
        "split_whitespace",
        1,
        "display",
    ),
    // Rejects whitespace-only summary, label or framing prose. Accepted fields are retained unchanged.
    (
        "crates/codeflow-present/src/document.rs",
        "validate_stage_framing",
        "trim",
        1,
        "schema-reject-only",
    ),
    // CSS property tokens accept only CSS space/tab/CR/LF/form-feed; stripping these separators is idempotent and preserves property identity.
    (
        "crates/codeflow-present/src/entity.rs",
        "declares_geometry",
        "trim_matches",
        1,
        "grammar:css-whitespace",
    ),
    // Collapses human-readable labels only. Entity IDs are independently compared unchanged.
    (
        "crates/codeflow-present/src/entity.rs",
        "finish_label",
        "split_whitespace",
        1,
        "display",
    ),
    // Collapses human-readable labels only. Entity IDs are independently compared unchanged.
    (
        "crates/codeflow-present/src/entity.rs",
        "own_visible_text",
        "split_whitespace",
        1,
        "display",
    ),
    // SVG/CSS length reader allows explicit ASCII space/tab/CR/LF around its number and optional px suffix.
    (
        "crates/codeflow-present/src/entity.rs",
        "parse_length",
        "trim_matches",
        1,
        "grammar:svg-length",
    ),
    // Rejects whitespace-only form prose or rationale. Accepted labels and values remain unchanged.
    (
        "crates/codeflow-present/src/form.rs",
        "check_rationale",
        "trim",
        1,
        "schema-reject-only",
    ),
    // Rejects email whitespace without stripping or rewriting the address.
    (
        "crates/codeflow-present/src/form.rs",
        "is_email",
        "is_whitespace",
        1,
        "schema-reject-only",
    ),
    // Rejects whitespace-only form prose or rationale. Accepted labels and values remain unchanged.
    (
        "crates/codeflow-present/src/form.rs",
        "validate_field",
        "trim",
        1,
        "schema-reject-only",
    ),
    // Rejects whitespace-only form prose or rationale. Accepted labels and values remain unchanged.
    (
        "crates/codeflow-present/src/form.rs",
        "validate_label",
        "trim",
        1,
        "schema-reject-only",
    ),
    // Rejects whitespace-only form prose or rationale. Accepted labels and values remain unchanged.
    (
        "crates/codeflow-present/src/form.rs",
        "validate_title",
        "trim",
        1,
        "schema-reject-only",
    ),
    // Whitespace collapse belongs to the fuzzy human-text anchor grammar. Exact entity/document identifiers are not parsed here.
    (
        "crates/codeflow-present/src/fuzzy.rs",
        "is_space",
        "is_whitespace",
        1,
        "grammar:prose-anchor",
    ),
    // Reads presentation diff lines to produce accessible rendered rows.
    (
        "crates/codeflow-present/src/render.rs",
        "render_block",
        "lines",
        1,
        "framing:render-diff-lines",
    ),
    // Rejects whitespace-only ledger reply text. Stored text stays exact.
    (
        "crates/codeflow-present/src/responses.rs",
        "validate_ledger",
        "trim",
        1,
        "schema-reject-only",
    ),
    // Rejects empty required retired-diagram prose without rewriting accepted fields.
    (
        "crates/codeflow-present/src/retired.rs",
        "well_formed",
        "trim",
        1,
        "schema-reject-only",
    ),
    // HTML href fragment grammar permits only leading/trailing HTML ASCII whitespace before the exact fragment validation.
    (
        "crates/codeflow-present/src/safe_html.rs",
        "allowed_fragment_reference",
        "trim_matches",
        1,
        "grammar:html-url-space",
    ),
    // Broad whitespace splitting only adds refusals for runtime-reserved cf- HTML identities. All browser separators are covered and original attributes are unchanged.
    (
        "crates/codeflow-present/src/safe_html.rs",
        "validate_element",
        "split_whitespace",
        1,
        "schema-reject-only",
    ),
    // Counts human-readable data-cf-label characters after Unicode space collapse. data-cf-for IDs use explicit HTML separators.
    (
        "crates/codeflow-present/src/safe_html.rs",
        "validate_vocabulary",
        "split_whitespace",
        1,
        "grammar:prose-label",
    ),
    // Extracts reader-visible text for labels/review prose, never URLs or entity identities.
    (
        "crates/codeflow-present/src/safe_html.rs",
        "visible_text_from_html",
        "split_whitespace",
        1,
        "display",
    ),
    // CSS selector token slices are surrounded by the CSS ASCII whitespace set; selectors retain all non-separator characters.
    (
        "crates/codeflow-present/src/scoped_css.rs",
        "ScopedRules::parse_prelude",
        "trim_matches",
        1,
        "grammar:css-whitespace",
    ),
    // Unreadable Cookie/Origin/marker/Content-Type values produce an HTTP refusal from require_application_request; every endpoint caller returns that refusal before state access.
    (
        "crates/codeflow-present/src/service.rs",
        "require_application_request",
        "obtain-absent:to_str",
        4,
        "unproven",
    ),
    // Unreadable Host returns HTTP 421; require_application_request and bootstrap endpoint callers return that refusal.
    (
        "crates/codeflow-present/src/service.rs",
        "require_host",
        "obtain-absent:to_str",
        1,
        "unproven",
    ),
    // Invalid output HeaderValue returns empty HTTP 500; secure_html/plain propagate the refusal instead of content with missing headers.
    (
        "crates/codeflow-present/src/service.rs",
        "response_with_headers",
        "obtain-absent:from_str",
        1,
        "unproven",
    ),
    // service_encoding cannot decode any present Accept-Encoding value returns unproven None; asset returns HTTP 406 for None.
    (
        "crates/codeflow-present/src/service.rs",
        "service_encoding",
        "obtain-absent:to_str",
        1,
        "unproven",
    ),
    // The alternate UniqueKeys parse preserves the original typed parse error; SessionStore revision readers propagate Err and refuse loading the record.
    (
        "crates/codeflow-present/src/state.rs",
        "read_revision_record",
        "obtain-absent:from_slice",
        1,
        "unproven",
    ),
    // Rejects whitespace-only selectors, labels, actors, instructions or notes. Original strings remain exact for identity comparisons.
    (
        "crates/codeflow-present/src/state.rs",
        "validate_element_selector",
        "trim",
        3,
        "schema-reject-only",
    ),
    // Rejects whitespace-only selectors, labels, actors, instructions or notes. Original strings remain exact for identity comparisons.
    (
        "crates/codeflow-present/src/state.rs",
        "validate_feedback",
        "trim",
        2,
        "schema-reject-only",
    ),
    // Measures nonblank human excerpt text for feedback bounds. The original excerpt is retained and is not a path/ref/config identity.
    (
        "crates/codeflow-present/src/state.rs",
        "validate_feedback_excerpt",
        "trim",
        1,
        "grammar:prose-excerpt",
    ),
    // Rejects whitespace-only selectors, labels, actors, instructions or notes. Original strings remain exact for identity comparisons.
    (
        "crates/codeflow-present/src/state.rs",
        "validate_feedback_note",
        "trim",
        3,
        "schema-reject-only",
    ),
    // Rejects whitespace-only selectors, labels, actors, instructions or notes. Original strings remain exact for identity comparisons.
    (
        "crates/codeflow-present/src/state.rs",
        "validate_feedback_target",
        "trim",
        1,
        "schema-reject-only",
    ),
    // Rejects whitespace-only selectors, labels, actors, instructions or notes. Original strings remain exact for identity comparisons.
    (
        "crates/codeflow-present/src/state.rs",
        "validate_region_selector",
        "trim",
        1,
        "schema-reject-only",
    ),
    // Finds the setup hook's first command to show doctor's reader; nothing is decided from it, and control characters are escaped.
    (
        "crates/codeflow-core/src/doctor/ci_pin.rs",
        "setup_note",
        "lines",
        1,
        "display",
    ),
    // Trims the setup hook's lines only to show its first command; nothing is decided from it.
    (
        "crates/codeflow-core/src/doctor/ci_pin.rs",
        "setup_note",
        "trim",
        1,
        "display",
    ),
    // A CI file is read by line as the shell runs it; the source line is matched with only spaces and tabs around it.
    (
        "crates/codeflow-core/src/doctor/ci_pin.rs",
        "sources_setup_hook",
        "lines",
        1,
        "grammar:shell-script-lines",
    ),
    // The digest table is compared by line as the installers' awk reader splits it; each line is trimmed of POSIX blanks only.
    (
        "crates/codeflow-core/src/doctor/ci_pin.rs",
        "table_lines",
        "lines",
        1,
        "grammar:installer-awk-lines",
    ),
    // Mirrors the installers' awk reader in the C locale: leading POSIX [[:space:]] (space, tab, LF, VT, FF, CR) only.
    (
        "crates/codeflow-core/src/scaffold/release_pin.rs",
        "byte_line",
        "trim_start_matches",
        1,
        "grammar:installer-awk-lines",
    ),
    // Trims curl's stderr for the refusal message only.
    (
        "crates/codeflow-core/src/scaffold/release_pin.rs",
        "fetch_with_curl",
        "trim",
        1,
        "display",
    ),
    // sha256.sum is read as sha256sum writes it: one digest and one file name per line, separated by blanks; a file that is not UTF-8 refuses before this split.
    (
        "crates/codeflow-core/src/scaffold/release_pin.rs",
        "pin_release",
        "lines",
        1,
        "grammar:sha256sum-lines",
    ),
    // sha256.sum fields are the digest then the file name, split as sha256sum separates them; the name must equal the ASCII asset name and the digest must be lowercase hex, so no other spacing is accepted.
    (
        "crates/codeflow-core/src/scaffold/release_pin.rs",
        "pin_release",
        "split_whitespace",
        1,
        "grammar:sha256sum-lines",
    ),
    // Reads the state by line as the installers' awk reader does; byte_line refuses a carriage return inside a line and other control characters.
    (
        "crates/codeflow-core/src/scaffold/release_pin.rs",
        "pinned_digests",
        "lines",
        1,
        "grammar:installer-awk-lines",
    ),
    // Trims the value after `=` on a line the ENTRY pattern already matched with ASCII [[:space:]], so only those blanks precede its quote.
    (
        "crates/codeflow-core/src/scaffold/release_pin.rs",
        "pinned_digests",
        "trim_start",
        1,
        "grammar:installer-awk-lines",
    ),
    // Drops the line break from the line shown in doctor's label, which is printed escaped.
    (
        "crates/codeflow-core/src/scaffold/release_pin.rs",
        "unread_line",
        "trim_end_matches",
        1,
        "display",
    ),
    // A forecast whose bytes do not deserialize is reported as forecast_invalid with its path and joins nothing; the report says so instead of reading it as absent.
    (
        "crates/codeflow-core/src/estimate/outcomes/mod.rs",
        "forecasts",
        "obtain-absent:from_slice",
        1,
        "unproven",
    ),
    // Git's stderr after a failed release clone, trimmed for the refusal text only.
    (
        "crates/codeflow-cli/examples/release_integration.rs",
        "prepare",
        "trim",
        1,
        "display",
    ),
    // The first non-blank line of an item's title text for one index table cell; shown, never matched.
    (
        "crates/codeflow-core/src/feedback.rs",
        "cell",
        "lines",
        1,
        "display",
    ),
    // Trims an index table cell's text for display; nothing is decided from it.
    (
        "crates/codeflow-core/src/feedback.rs",
        "cell",
        "trim",
        2,
        "display",
    ),
    // Finds the line number of a frontmatter key for a lint message only.
    (
        "crates/codeflow-core/src/feedback.rs",
        "line_of",
        "lines",
        1,
        "display",
    ),
    // Re-reads an item only for the line numbers in its lint messages; load already read it and reports a read failure as an error.
    (
        "crates/codeflow-core/src/feedback.rs",
        "lint_item",
        "obtain-absent:read_to_string",
        1,
        "display",
    ),
    // YAML string items of a feedback item's frontmatter lists (file content), trimmed as values, not git or OS names.
    (
        "crates/codeflow-core/src/feedback.rs",
        "list",
        "trim",
        2,
        "format-contract",
    ),
    // Operator-typed one-line record text (summary, placement, evidence); the trimmed text is what is recorded and checked, so nothing differs between the check and its use.
    (
        "crates/codeflow-core/src/feedback.rs",
        "one_line",
        "trim",
        1,
        "prose",
    ),
    // A placement reference is the trimmed record text; it resolves exactly as it is recorded.
    (
        "crates/codeflow-core/src/feedback.rs",
        "placement_resolves",
        "trim",
        1,
        "prose",
    ),
    // Operator-typed one-line flag text (evidence, reason, successor); blank text refuses.
    (
        "crates/codeflow-core/src/feedback.rs",
        "required",
        "trim",
        1,
        "prose",
    ),
    // A YAML string value of a feedback item's frontmatter (file content), trimmed as a value.
    (
        "crates/codeflow-core/src/feedback.rs",
        "scalar",
        "trim",
        1,
        "format-contract",
    ),
    // The visible Markdown text of an item section (file content), trimmed for emptiness checks and display.
    (
        "crates/codeflow-core/src/feedback.rs",
        "section_text",
        "trim",
        1,
        "format-contract",
    ),
    // The visible Markdown text of an item section (file content), each line's trailing blanks dropped.
    (
        "crates/codeflow-core/src/feedback.rs",
        "section_text",
        "trim_end",
        1,
        "format-contract",
    ),
    // A topic with any whitespace inside it is refused; nothing is normalized.
    (
        "crates/codeflow-core/src/scaffold/state.rs",
        "FeedbackConfig::load",
        "is_whitespace",
        1,
        "schema-reject-only",
    ),
    // A TOML topic string (file content), trimmed as a value.
    (
        "crates/codeflow-core/src/scaffold/state.rs",
        "FeedbackConfig::load",
        "trim",
        1,
        "format-contract",
    ),
    // Finds the `[feedback]` TOML header line to insert under; the result is parsed again and refused unless feedback.topics reads back.
    (
        "crates/codeflow-core/src/scaffold/state.rs",
        "FeedbackConfig::write_defaults",
        "trim",
        1,
        "format-contract",
    ),
    // A blank adoption field is refused; the value is kept untrimmed.
    (
        "crates/codeflow-core/src/workgraph/line_adoption.rs",
        "parse",
        "trim",
        1,
        "schema-reject-only",
    ),
    // A fixture clone's stdout, trimmed for tests that show or discard it.
    (
        "crates/codeflow-fixture/src/lib.rs",
        "Clone::run",
        "trim",
        1,
        "display",
    ),
];

const FRAMING_OWNERS: &[(&str, &str, &str)] = &[
    (
        "acceptance-input-lines",
        "crates/codeflow-core/src/workgraph/record_text.rs",
        "parse_acceptance",
    ),
    (
        "canonical-review-lines",
        "crates/codeflow-present/src/document.rs",
        "Block::canonical_review_text",
    ),
    (
        "capability-parse-capabilities-records",
        "crates/codeflow-core/src/capability.rs",
        "parse_capabilities",
    ),
    (
        "ceremony-history-regions-records",
        "crates/codeflow-core/src/ceremony/history.rs",
        "regions",
    ),
    (
        "classification-task-lines",
        "crates/codeflow-cli/src/cmd/ci/classification.rs",
        "task_lines",
    ),
    (
        "cli-git-config-z",
        "crates/codeflow-cli/src/cmd/git_hook.rs",
        "git_config_values",
    ),
    (
        "commit-marker-lines",
        "crates/codeflow-cli/src/cmd/ci.rs",
        "breaking_marker",
    ),
    (
        "conventional-commits-lines",
        "crates/codeflow-core/src/hooks/standards.rs",
        "breaking_marker_present",
    ),
    (
        "conventional-commits-lines",
        "crates/codeflow-core/src/hooks/standards.rs",
        "check_breaking_footer",
    ),
    (
        "conventional-commits-lines",
        "crates/codeflow-core/src/hooks/standards.rs",
        "check_commit_body",
    ),
    (
        "conventional-commits-lines",
        "crates/codeflow-core/src/hooks/standards.rs",
        "check_commit_ticket",
    ),
    (
        "conventional-commits-lines",
        "crates/codeflow-core/src/hooks/standards.rs",
        "check_required_footers",
    ),
    (
        "conventional-commits-lines",
        "crates/codeflow-core/src/hooks/standards.rs",
        "check_subject_separator",
    ),
    (
        "conventional-commits-lines",
        "crates/codeflow-core/src/hooks/standards.rs",
        "find_policy_character",
    ),
    (
        "delegate-continuation-session-entries-records",
        "crates/codeflow-core/src/delegate/continuation.rs",
        "session_entries",
    ),
    (
        "doctor-ci-pin-recognized-records",
        "crates/codeflow-core/src/doctor/ci_pin.rs",
        "recognized",
    ),
    (
        "doctor-ci-pin-span-records",
        "crates/codeflow-core/src/doctor/ci_pin.rs",
        "span",
    ),
    (
        "git-commit-message-lines",
        "crates/codeflow-core/src/hooks/git_hook.rs",
        "commit_msg_from",
    ),
    (
        "git-commit-message-lines",
        "crates/codeflow-core/src/hooks/git_hook.rs",
        "commit_msg_with_files",
    ),
    (
        "git-commit-message-lines",
        "crates/codeflow-core/src/hooks/git_hook.rs",
        "policy_character_violation",
    ),
    (
        "git-commit-message-lines",
        "crates/codeflow-core/src/hooks/git_hook.rs",
        "strip_commit_comments",
    ),
    (
        "git-quoted-diff",
        "crates/codeflow-cli/src/cmd/ci.rs",
        "unquote_git_path",
    ),
    (
        "ids-entry-frontmatter-value-records",
        "crates/codeflow-core/src/ids/entry.rs",
        "frontmatter_value",
    ),
    (
        "ids-git-git-branch-refs-records",
        "crates/codeflow-core/src/ids/git.rs",
        "Git::branch_refs",
    ),
    (
        "ids-inventory-lifetime-start-records",
        "crates/codeflow-core/src/ids/inventory.rs",
        "lifetime_start",
    ),
    (
        "ids-issue-diagnostics-records",
        "crates/codeflow-core/src/ids/issue.rs",
        "diagnostics",
    ),
    (
        "ids-issue-pending-entries-records",
        "crates/codeflow-core/src/ids/issue.rs",
        "pending_entries",
    ),
    (
        "ids-ledger-ledger-range-violations-records",
        "crates/codeflow-core/src/ids/ledger.rs",
        "Ledger::range_violations",
    ),
    (
        "ids-seed-add-former-id-records",
        "crates/codeflow-core/src/ids/seed.rs",
        "add_former_id",
    ),
    (
        "ledger-compact-read-events-from-file-records",
        "crates/codeflow-core/src/ledger/compact.rs",
        "read_events_from_file",
    ),
    (
        "ledger-rebuild-read-events-from-file-records",
        "crates/codeflow-core/src/ledger/rebuild.rs",
        "read_events_from_file",
    ),
    (
        "legacy-review-lines",
        "crates/codeflow-present/src/document.rs",
        "Block::legacy_diff_review_text",
    ),
    (
        "model-catalog-operator-anchored-override-records",
        "crates/codeflow-core/src/model_catalog/operator.rs",
        "anchored_override",
    ),
    (
        "pr-task-lines",
        "crates/codeflow-cli/src/cmd/ci/adopter.rs",
        "supply_task",
    ),
    (
        "presentation-marker-lines",
        "crates/codeflow-cli/src/cmd/ci/pr_body.rs",
        "presentation",
    ),
    (
        "push-advertisement-lines",
        "crates/codeflow-cli/src/cmd/push_set.rs",
        "advertised_commits",
    ),
    (
        "push-boundary-lines",
        "crates/codeflow-cli/src/cmd/push_set.rs",
        "boundary",
    ),
    (
        "push-bounded-lines",
        "crates/codeflow-cli/src/cmd/push_set.rs",
        "bounded_by",
    ),
    (
        "recall-index-file-records",
        "crates/codeflow-core/src/recall.rs",
        "index_file",
    ),
    (
        "record-section-lines",
        "crates/codeflow-core/src/workgraph/record_text.rs",
        "section",
    ),
    (
        "record-section-markdown-lines",
        "crates/codeflow-core/src/workgraph/record_text.rs",
        "section_markdown",
    ),
    (
        "release-field-lines",
        "crates/codeflow-cli/src/cmd/ci/pr_body.rs",
        "release_fields_under",
    ),
    (
        "render-diff-lines",
        "crates/codeflow-present/src/render.rs",
        "render_block",
    ),
    (
        "review-row-lines",
        "crates/codeflow-cli/src/cmd/ci/pr_body.rs",
        "review_names_revision",
    ),
    (
        "reviewed-record-lines",
        "crates/codeflow-core/src/workgraph/acceptance.rs",
        "reviewed_part",
    ),
    (
        "scaffold-pr-template-headings-records",
        "crates/codeflow-core/src/scaffold/pr_template.rs",
        "headings",
    ),
    (
        "scaffold-region-block-interior-records",
        "crates/codeflow-core/src/scaffold/region.rs",
        "block_interior",
    ),
    (
        "scaffold-region-extract-block-records",
        "crates/codeflow-core/src/scaffold/region.rs",
        "extract_block",
    ),
    (
        "scaffold-region-with-eol-records",
        "crates/codeflow-core/src/scaffold/region.rs",
        "with_eol",
    ),
    (
        "shell-command-substitution",
        "crates/codeflow-core/src/security/deletion.rs",
        "Reader::substitution",
    ),
    (
        "xargs-delimiter",
        "crates/codeflow-core/src/security/deletion.rs",
        "items",
    ),
    (
        "shell-read-line",
        "crates/codeflow-core/src/security/deletion.rs",
        "Reader::read",
    ),
    (
        "shell-quotes",
        "crates/codeflow-core/src/hooks/git_guard.rs",
        "shell_words",
    ),
    (
        "submodule-status-lines",
        "crates/codeflow-cli/src/cmd/push_set.rs",
        "incomplete_checkout",
    ),
    (
        "template-placeholder-lines",
        "crates/codeflow-cli/src/cmd/ci.rs",
        "find_placeholders",
    ),
    (
        "testing-coverage-go-cover-parse-go-cover-str-records",
        "crates/codeflow-core/src/testing/coverage/go_cover.rs",
        "parse_go_cover_str",
    ),
    (
        "testing-coverage-lcov-parse-lcov-str-records",
        "crates/codeflow-core/src/testing/coverage/lcov.rs",
        "parse_lcov_str",
    ),
    (
        "testing-gate-guard-live-groups-records",
        "crates/codeflow-core/src/testing/gate_guard.rs",
        "live_groups",
    ),
    (
        "unified-diff-lines",
        "crates/codeflow-core/src/hooks/scan.rs",
        "scan_diff",
    ),
    (
        "validate-mod-landed-pull-requests-records",
        "crates/codeflow-core/src/validate/mod.rs",
        "landed_pull_requests",
    ),
    (
        "validate-portal-lookups-hook-stage-names-records",
        "crates/codeflow-core/src/validate/portal/lookups.rs",
        "hook_stage_names",
    ),
    (
        "validate-portal-recover-unavailable-ids-records",
        "crates/codeflow-core/src/validate/portal.rs",
        "recover_unavailable_ids",
    ),
    (
        "validate-portal-verify-rendered-claims-records",
        "crates/codeflow-core/src/validate/portal.rs",
        "verify_rendered_claims",
    ),
    (
        "validate-portal-verify-snippets-records",
        "crates/codeflow-core/src/validate/portal.rs",
        "verify_snippets",
    ),
];

/// Fail with each site found but not listed, each count that differs, and
/// each stale row, one per line, so a widened scan names what to triage.
fn assert_listed(
    actual: &BTreeMap<(String, String, String), usize>,
    allowed: &BTreeMap<(String, String, String), usize>,
    rule: &str,
) {
    let mut problems = Vec::new();
    for (key, count) in actual {
        match allowed.get(key) {
            None => problems.push(format!("unlisted: {key:?} {count}")),
            Some(listed) if listed != count => {
                problems.push(format!("count: {key:?} listed {listed}, found {count}"));
            }
            Some(_) => {}
        }
    }
    for (key, count) in allowed {
        if !actual.contains_key(key) {
            problems.push(format!("stale: {key:?} {count}"));
        }
    }
    assert!(problems.is_empty(), "{rule}:\n{}", problems.join("\n"));
}

fn valid_framing_owner(reason: &str, file: &str, item: &str) -> bool {
    reason
        .strip_prefix("framing:")
        .is_none_or(|grammar| FRAMING_OWNERS.contains(&(grammar, file, item)))
}

#[test]
fn guard_whitespace_uses_explicit_separator_rules() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);
    assert!(files.len() > 100);
    let mut actual = BTreeMap::new();
    for file in files {
        let name = relative(&root, &file);
        for ((item, call), lines) in sites_in_mode(&std::fs::read_to_string(&file).unwrap(), true) {
            println!("{name} {item} {call} {} {lines:?}", lines.len());
            actual.insert((name.clone(), item, call), lines.len());
        }
    }
    let mut allowed = BTreeMap::new();
    for (file, item, call, count, reason) in WHITESPACE_EXCEPTIONS {
        assert!(
            valid_framing_owner(reason, file, item),
            "{file} {item}: framing is outside its owner"
        );
        assert!(
            valid_reason(reason),
            "{file} {item} {call}: invalid reason {reason}"
        );
        assert!(allowed
            .insert(
                (file.to_string(), item.to_string(), call.to_string()),
                *count
            )
            .is_none());
    }
    assert_listed(
        &actual,
        &allowed,
        "Unicode whitespace in a guard needs a reviewed reason; shell and git names keep non-separator characters",
    );
}

#[test]
fn whitespace_scan_covers_methods_function_values_and_macros() {
    for expression in [
        "text.trim()",
        "text.split_whitespace()",
        "text.trim_start()",
        "text.trim_end()",
        "c.is_whitespace()",
        "iter.any(char::is_whitespace)",
        "format!(\"{}\", text.trim())",
    ] {
        let source = format!("fn f() {{ {expression}; }}");
        assert_eq!(
            sites_in_mode(&source, true)
                .values()
                .map(Vec::len)
                .sum::<usize>(),
            1,
            "{expression}"
        );
    }
    assert!(sites_in_mode("#[cfg(test)] mod tests { fn f() { text.trim(); } }", true).is_empty());
}

#[test]
fn decision_scan_detects_framing_predicates_and_decode_to_absent() {
    for expression in [
        "text.lines()",
        "text.split_terminator('\\n')",
        "text.trim_matches(char::is_whitespace)",
        "text.trim_start_matches(|c| c == ' ')",
        "text.trim_end_matches(shell_blank)",
        "buf.as_str().ok()",
        "path.to_str().unwrap_or(\"\")",
        "String::from_utf8(bytes).unwrap_or_default()",
        "std::str::from_utf8(bytes).map(str::to_owned).unwrap_or_else(|_| String::new())",
        "utf8_to_str(bytes).map_or(false, |s| s == name)",
        "format!(\"{:?}\", buf.as_str().ok())",
    ] {
        let source = format!("fn f() {{ {expression}; }}");
        assert_eq!(
            sites_in_mode(&source, true)
                .values()
                .map(Vec::len)
                .sum::<usize>(),
            if expression == "text.trim_matches(char::is_whitespace)" {
                2
            } else {
                1
            },
            "{expression}"
        );
    }
    for expression in [
        "buf.as_str()?",
        "path.to_str()?",
        "std::str::from_utf8(bytes)?",
    ] {
        let source = format!("fn f() -> Option<String> {{ {expression}; None }}");
        assert_eq!(
            sites_in_mode(&source, true)
                .values()
                .map(Vec::len)
                .sum::<usize>(),
            1,
            "{expression}"
        );
    }
    for source in [
        "fn f() { let lines = text; lines.iter(); consume(lines); }",
        "fn f() { str::trim_matches(text, [' ', '\\t']); }",
        "fn f() { text.trim_matches([' ', '\\t']); text.trim_start_matches(' '); }",
        "fn f() -> Result<(), E> { std::str::from_utf8(bytes)?; Ok(()) }",
        "fn f() { buf.as_str(); path.to_str(); }",
        "#[cfg(test)] fn f() { bytes.as_str().ok(); }",
    ] {
        assert!(sites_in_mode(source, true).is_empty(), "{source}");
    }
    assert_eq!(
        sites_in_mode("fn f() -> Option<()> { buf.as_str().ok()?; None }", true)
            .values()
            .map(Vec::len)
            .sum::<usize>(),
        1
    );
}

#[test]
fn decision_exception_reasons_are_closed() {
    for reason in [
        "display",
        "prose",
        "schema-reject-only",
        "format-contract",
        "ascii-marker",
        "grammar:posix-shell",
        "framing:git-quoted-diff",
        "unproven",
    ] {
        assert!(valid_reason(reason));
    }
    for reason in [
        "",
        "safe",
        "grammar:",
        "grammar:any arbitrary explanation",
        "display-only",
    ] {
        assert!(!valid_reason(reason));
    }
}

#[test]
fn obtaining_scan_covers_every_operation_consumer_and_discard_shape() {
    for operation in OBTAIN {
        let source = format!("fn f() {{ source.{operation}().ok(); }}");
        assert!(!sites_in_mode(&source, true).is_empty(), "{source}");
    }
    for operation in ["peel", "peel_to_commit", "peel_to_tree", "peel_to_blob"] {
        let source = format!("fn f() {{ reference.{operation}().ok(); }}");
        assert!(!sites_in_mode(&source, true).is_empty(), "{source}");
    }
    for (owner, function) in OBTAIN_QUALIFIED {
        let source = format!("fn f() {{ git2::{owner}::{function}(p).ok(); }}");
        assert!(!sites_in_mode(&source, true).is_empty(), "{source}");
    }
    // A bare `open` or `new` is no obtaining call by name alone.
    assert!(sites_in_mode("fn f() { Thing::new(p).ok(); open(p).ok(); }", true).is_empty());
    for consumer in ABSENT {
        let source = format!("fn f() {{ std::fs::read(path).{consumer}(); }}");
        assert!(!sites_in_mode(&source, true).is_empty(), "{source}");
    }
    for source in [
        "fn f() { if let Ok(text) = std::fs::read_to_string(p) {} }",
        "fn f() { if let Ok(text) = std::fs::read_to_string(p) {} else { refuse(); } }",
        "fn f() { match std::fs::metadata(p) { Ok(m) => m, Err(_) => fallback() } }",
        "fn f() { match std::fs::metadata(p) { Ok(m) => m, Err(..) => fallback() } }",
        "fn f() { let _ = std::fs::read(p); }",
        "fn f() { std::fs::read(p).ok(); }",
        "fn f() { iter.filter_map(Result::ok); }",
        "fn f() { iter.flat_map(Result::ok); }",
        "fn f() { text.trim_matches('\\\''); }",
        "fn f() { text.trim_matches('\\n'); }",
        "fn f() { text.trim_matches('\\\\'); }",
    ] {
        assert!(!sites_in_mode(source, true).is_empty(), "{source}");
    }
    assert!(sites_in_mode(
        "fn f() { match std::fs::read(p) { Ok(v) => v, Err(error) => return Err(error) } }",
        true
    )
    .is_empty());
    assert!(valid_framing_owner(
        "framing:shell-quotes",
        "crates/codeflow-core/src/hooks/git_guard.rs",
        "shell_words"
    ));
    assert!(!valid_framing_owner(
        "framing:shell-quotes",
        "crates/codeflow-core/src/hooks/git_guard.rs",
        "evaluate"
    ));
}

/// The absence scope: every engine and CLI source file. Filesystem absence
/// is proven centrally (`proven_absent`) or listed with its proof.
fn absence_scope(file: &str) -> bool {
    file.starts_with("crates/codeflow-core/src/") || file.starts_with("crates/codeflow-cli/src/")
}

fn absence_sites(source: &str) -> BTreeMap<(String, String), Vec<usize>> {
    let mut sites = Sites {
        absence: true,
        ..Sites::default()
    };
    sites.visit_file(&syn::parse_file(source).expect("Rust source"));
    sites.found
}

// (file, item, operation, count, proof). Obtained type checks and diagnostic probes.
const ABSENCE_METHODS: &[&str] = &["try_exists", "exists", "is_file", "is_dir"];

const ABSENCE_EXCEPTIONS: &[(&str, &str, &str, usize, &str)] = &[
    ("crates/codeflow-cli/src/cmd/ids.rs", "run_admit", "exists", 1, "False only sends a rev:path spec to git show, which refuses on error; a literal path whose metadata fails also fails the read that follows, so no unread input becomes a default."),
    ("crates/codeflow-cli/src/cmd/new.rs", "run_adr", "ErrorKind::NotFound", 1, "NotFound only picks the shipped template text for a new ADR, which check_adr then validates; any other read error refuses, and a dangling ancestor fails create_dir_all."),
    ("crates/codeflow-cli/src/cmd/present.rs", "read_bounded_regular", "is_file", 2, "Both are type checks on metadata obtained with errors propagated (symlink_metadata of the path and the opened file's metadata); a non-file refuses as UnsafePath or CorruptState."),
    ("crates/codeflow-cli/src/cmd/present.rs", "read_document", "is_file", 1, "Type check on symlink_metadata obtained with its error propagated; a non-regular file or a link refuses as UnsafePath."),
    ("crates/codeflow-cli/src/cmd/present.rs", "remove_regular_if_present", "ErrorKind::NotFound", 1, "The leaf is under the session's runtime control directory, which runtime_dir creates; NotFound means no stale ready or bootstrap file, and a later service failure ends in the readiness timeout refusal."),
    ("crates/codeflow-cli/src/cmd/present.rs", "remove_regular_if_present", "is_file", 1, "Type check on symlink_metadata whose errors other than NotFound propagate; anything but a regular file refuses as UnsafePath."),
    ("crates/codeflow-cli/src/cmd/present.rs", "start_service", "is_file", 1, "Readiness poll: false only keeps waiting, the loop ends in a ServiceUnavailable refusal on child exit or the deadline, and a true is re-read through read_bounded_regular."),
    ("crates/codeflow-cli/src/cmd/validate.rs", "collect_record_files", "is_dir", 1, "Type check on std::fs::metadata whose error propagates, so an unreadable entry or a dangling link makes validate_records report cannot read and fail."),
    ("crates/codeflow-cli/src/cmd/validate.rs", "validate_policy", "exists", 1, "Reached only after policy_schema::validate_policy returned Ok; it only picks the clean or defaults-apply message, and the verdict is already true."),
    ("crates/codeflow-cli/src/cmd/validate.rs", "validate_records", "ErrorKind::NotFound", 2, "The record home is absent only when both the followed and the unfollowed stat of the leaf under the discovered repository root find no name; a dangling link is reported as unreadable and fails validation."),
    ("crates/codeflow-cli/src/cmd/validate.rs", "validate_records", "is_file", 1, "Type check on metadata whose errors other than NotFound fail the check; a directory goes to collect_record_files, which propagates its errors."),
    ("crates/codeflow-cli/src/embedded.rs", "EmbeddedAssets::read", "ErrorKind::NotFound", 1, "Debug builds only (cfg(debug_assertions)) read the source tree's assets directory, mirroring the release Raw::get Option contract; release binaries never take this path."),
    ("crates/codeflow-core/src/absence.rs", "proven_absent", "ErrorKind::NotFound", 3, "The helper defines proven absence: a leaf or ancestor that is not found only continues the ancestor walk, and a dangling ancestor link becomes an error."),
    ("crates/codeflow-core/src/absence.rs", "proven_absent", "is_dir", 1, "Type check on ancestor metadata obtained with errors propagated; a non-directory ancestor returns an error, never absence."),
    ("crates/codeflow-core/src/absence.rs", "symlink_metadata_optional", "ErrorKind::NotFound", 1, "Returns None only when proven_absent confirms the leaf is missing; other errors propagate."),
    ("crates/codeflow-core/src/bounded_file.rs", "read_bounded_regular_with_hook", "is_file", 1, "Type check on symlink_metadata obtained with its error propagated; false returns an InvalidData error."),
    ("crates/codeflow-core/src/bounded_file.rs", "read_opened_regular", "is_file", 2, "Both check metadata of the opened file obtained with errors propagated; false refuses with InvalidData."),
    ("crates/codeflow-core/src/bounded_file/confined.rs", "open_locked", "is_dir", 1, "Type check on the opened handle's metadata, obtained with its error propagated; a mismatch returns invalid()."),
    ("crates/codeflow-core/src/bounded_file/confined.rs", "open_locked", "is_file", 1, "Type check on the opened handle's metadata, obtained with its error propagated; a mismatch returns invalid()."),
    ("crates/codeflow-core/src/bounded_file/confined/windows.rs", "check", "is_dir", 2, "Type checks on a held handle's metadata, obtained with its error propagated; any mismatch returns invalid()."),
    ("crates/codeflow-core/src/bounded_file/confined/windows.rs", "check", "is_file", 1, "Type check on a held handle's metadata, obtained with its error propagated; a non-file non-directory returns invalid()."),
    ("crates/codeflow-core/src/delegate.rs", "inspect_continuations", "ErrorKind::NotFound", 1, "The turn directory is validated as a real private directory before this call, so a leaf that is not found is proven absence."),
    ("crates/codeflow-core/src/delegate.rs", "path_exists_safely", "ErrorKind::NotFound", 1, "Callers pass leaves of validated private directories (validate_state_dir, the turn directory), so a leaf not found is proven absence; other errors refuse."),
    ("crates/codeflow-core/src/delegate.rs", "private_file_metadata", "is_file", 1, "Type check on symlink_metadata obtained with its error propagated; false refuses with unsafe_state."),
    ("crates/codeflow-core/src/delegate.rs", "validate_open_file", "is_file", 1, "Type check on the opened file's metadata, obtained with its error propagated; false refuses with unsafe_state."),
    ("crates/codeflow-core/src/delegate.rs", "validate_outside_git_worktree", "ErrorKind::NotFound", 1, "A .git that is not found under the walked ancestors means no worktree marker there; a missing or dangling ancestor makes the non-recursive mkdir that follows fail."),
    ("crates/codeflow-core/src/delegate.rs", "validate_private_dir", "is_dir", 1, "Type check on symlink_metadata obtained with its error propagated; false refuses with unsafe_state."),
    ("crates/codeflow-core/src/delegate/continuation.rs", "session_entries", "is_file", 1, "Type check on symlink_metadata obtained with its error propagated; false returns an error."),
    ("crates/codeflow-core/src/doctor/grok_hooks.rs", "hook_files", "try_exists", 2, "Feeds the templated and shell-guard readings, which check_grok maps only to Warn or Note; the hooks directory itself is proven readable by input_absent first."),
    ("crates/codeflow-core/src/doctor/grok_hooks.rs", "is_executable", "is_file", 2, "Type check on obtained metadata; the result only feeds grok_path_note, an unverified PATH sentence appended to the grok message."),
    ("crates/codeflow-core/src/doctor/mod.rs", "check_customization", "ErrorKind::NotFound", 1, "A doc is listed as missing, a Warn, only when proven_absent confirms it; a dangling link or any other read error returns the refused Fail."),
    ("crates/codeflow-core/src/doctor/mod.rs", "check_grok", "is_file", 1, "Only filters which .new proposals are named in the pending clause of a Warn message; it never changes the status."),
    ("crates/codeflow-core/src/doctor/mod.rs", "codex_hook_trust", "ErrorKind::NotFound", 1, "A Codex config.toml that is not found gives no trusted hooks, which the codex check maps to a Warn or Note only; other read errors become trust not read."),
    ("crates/codeflow-core/src/doctor/mod.rs", "grok_folder_trust", "ErrorKind::NotFound", 1, "A trusted_folders.toml that is not found gives Untrusted, which check_grok maps to a Warn; Grok trust never produces a Fail."),
    ("crates/codeflow-core/src/doctor/mod.rs", "grok_folder_trust", "exists", 1, "Picks the nearest existing ancestor to derive a Grok workspace key; the trust result only selects Note or Warn in check_grok."),
    ("crates/codeflow-core/src/doctor/mod.rs", "grok_gate_enabled", "try_exists", 1, "Only decides Ungated versus the trust lookup, which check_grok maps to a Note or Warn; errors become Unverifiable, a Note."),
    ("crates/codeflow-core/src/doctor/mod.rs", "hooks_wiring", "exists", 1, "Only chooses which Wiring::Broken finding text and remedy to print; both branches are the same Broken outcome."),
    ("crates/codeflow-core/src/doctor/mod.rs", "instruction_chains", "is_dir", 1, "Type check on entry.file_type(), whose error propagates and becomes the instructions Fail."),
    ("crates/codeflow-core/src/doctor/mod.rs", "instruction_chains", "try_exists", 1, "A nested .git only excludes a directory from the size walk; the instructions check maps sizes only to Pass or Warn, and other errors propagate to its Fail."),
    ("crates/codeflow-core/src/doctor/mod.rs", "shims_not_called", "ErrorKind::NotFound", 1, "A hook that is not found is recorded as uncalled, which yields Wiring::Broken, the stricter outcome; git runs no hook at a missing path either."),
    ("crates/codeflow-core/src/doctor/mod.rs", "shims_not_called", "is_file", 1, "Type check on entry.file_type(), whose error is propagated first."),
    ("crates/codeflow-core/src/doctor/mod.rs", "update_leaves", "exists", 1, "Selects a clause saying update does not manage the file inside a Warn message; the status is fixed by the caller."),
    ("crates/codeflow-core/src/doctor/mod.rs", "update_leaves", "is_file", 1, "Selects whether the message names the .codeflow/.baseline copy; wording only inside an existing Warn."),
    ("crates/codeflow-core/src/doctor/mod.rs", "walk_json_files_inner", "is_dir", 1, "Type check on std::fs::metadata, whose error propagates to the config check Fail."),
    ("crates/codeflow-core/src/estimate/sources.rs", "Reader::new", "is_dir", 1, "False yields None, and the estimate check records the project_root finding and returns, a refusal."),
    ("crates/codeflow-core/src/feedback.rs", "item_files", "is_file", 1, "Type check on entry.file_type() with its error propagated; a link is left out, never followed."),
    ("crates/codeflow-core/src/feedback.rs", "propose", "is_file", 1, "Type check on metadata from symlink_metadata_optional, whose read errors refuse; false refuses the supersede."),
    ("crates/codeflow-core/src/feedback.rs", "read_project_template", "is_file", 1, "Type check on metadata from symlink_metadata_optional, whose read errors propagate; a non-file returns Err, which feedback new shows as a warning before it uses the shipped template."),
    ("crates/codeflow-core/src/feedback.rs", "replace_whole", "is_file", 1, "Type check on metadata from symlink_metadata_optional, whose read errors propagate; only a proven-absent or non-file mode_from leaves the default permissions."),
    ("crates/codeflow-core/src/feedback.rs", "tracked", "is_dir", 1, "Type check on metadata from symlink_metadata_optional after contained_path refuses a link at or above it; a read error or a non-directory is returned as an error, never read as no feedback directory."),
    ("crates/codeflow-core/src/file_lock.rs", "locked_read_critical", "is_dir", 1, "Type check on the parent's metadata obtained with its error propagated; false returns an error."),
    ("crates/codeflow-core/src/file_lock.rs", "locked_rmw_typed_io", "ErrorKind::NotFound", 1, "The sidecar lock's create_dir_all and open prove the parent resolves; the only caller is the per-user registry, a rebuildable view."),
    ("crates/codeflow-core/src/hooks/adoption.rs", "detect_release_tools", "is_file", 1, "Optional advisory release-tool inventory for doctor, CI text and scaffold notes; it does not select release.backend or authorize a release or gate exemption."),
    ("crates/codeflow-core/src/hooks/delegate_turn.rs", "validate_result_path", "is_dir", 1, "Parent metadata acquisition already returned an explicit error on failure; this checks the obtained type."),
    ("crates/codeflow-core/src/hooks/delegate_turn.rs", "verify_exact_retry", "is_file", 1, "Symlink metadata was obtained fallibly; this rejects a non-regular result, not a missing input."),
    ("crates/codeflow-core/src/hooks/edit_guard.rs", "enforcement_patterns", "is_file", 1, "Git marker absence is proven; metadata errors propagate before classifying an existing linked-worktree gitfile."),
    ("crates/codeflow-core/src/hooks/edit_guard.rs", "find_candidates", "is_dir", 1, "Directory entry file_type errors propagate before testing the obtained type."),
    ("crates/codeflow-core/src/hooks/git_discard.rs", "restore_paths", "is_dir", 1, "Missing paths are proven absent; existing symlink metadata errors propagate before directory classification."),
    ("crates/codeflow-core/src/hooks/git_guard.rs", "every_path_below", "is_dir", 1, "Obtained path metadata; failures produce GlobStop::Unreadable and refuse the glob judgment."),
    ("crates/codeflow-core/src/hooks/git_guard.rs", "expand_components", "is_dir", 1, "Obtained entry FileType; failures produce GlobStop::Unreadable rather than omitting recursion."),
    ("crates/codeflow-core/src/hooks/git_guard.rs", "glob_directory", "is_dir", 1, "After proven absence handling, metadata errors are unproven; a known non-directory has no glob children."),
    ("crates/codeflow-core/src/hooks/git_guard.rs", "read_sed_script", "is_file", 2, "Both path and opened-file metadata are fallibly obtained; errors already yield SedRead::Unreadable."),
    ("crates/codeflow-core/src/hooks/git_hook.rs", "judging_identity", "is_dir", 1, "Only selects an additional source-drift diagnostic; it does not select policy or waive hook enforcement."),
    ("crates/codeflow-core/src/hooks/orient.rs", "gates_line", "exists", 3, "Selects presence marks in orientation output; the marks do not execute or waive a gate."),
    ("crates/codeflow-core/src/hooks/orient.rs", "pointer_paths", "exists", 1, "Filters optional documentation navigation pointers only."),
    ("crates/codeflow-core/src/hooks/orient.rs", "work_line", "is_dir", 2, "Selects optional printed work counts in orientation, not a task-operation gate or work-state mutation."),
    ("crates/codeflow-core/src/hooks/repo.rs", "open", "is_dir", 1, "Marker symlink metadata was obtained after proven absence handling; obtaining errors refuse discovery."),
    ("crates/codeflow-core/src/hooks/session_summary.rs", "unwritten", "exists", 2, "Selects repair wording after a ledger write has already failed; the original LedgerUnwritten error remains."),
    ("crates/codeflow-core/src/hooks/session_summary.rs", "unwritten", "is_dir", 2, "Selects a repair path description after a failed ledger write, never converts it to successful writing."),
    ("crates/codeflow-core/src/hooks/source_identity.rs", "input_files::visit", "is_dir", 1, "Symlink metadata and entry errors propagate before source input type classification."),
    ("crates/codeflow-core/src/hooks/source_identity.rs", "input_files::visit", "is_file", 1, "Symlink metadata errors propagate before regular source input classification."),
    ("crates/codeflow-core/src/hooks/source_identity.rs", "revision", "exists", 1, "Build provenance fallback records dirty=unavailable and supplied or unavailable revision; it grants no enforcement exemption."),
    ("crates/codeflow-core/src/ids/inventory.rs", "collect_files", "ErrorKind::NotFound", 1, "Returns empty only after proven_absent confirms the directory is missing."),
    ("crates/codeflow-core/src/ids/inventory.rs", "collect_files", "is_dir", 1, "Type check on entry.file_type() with its error propagated."),
    ("crates/codeflow-core/src/ids/inventory.rs", "collect_files", "is_file", 1, "Type check on entry.file_type() with its error propagated."),
    ("crates/codeflow-core/src/ids/seed.rs", "worktree_records", "ErrorKind::NotFound", 1, "Skips an optional root only after proven_absent confirms it is missing."),
    ("crates/codeflow-core/src/ids/seed.rs", "plan_link_rewrites", "is_file", 1, "Type check on symlink_metadata with its error propagated; a tracked link or a non-file is left alone, never followed."),
    ("crates/codeflow-core/src/ids/seed.rs", "worktree_records", "is_dir", 1, "Type check on entry.file_type() with its error propagated; a link is never followed."),
    ("crates/codeflow-core/src/ids/seed.rs", "worktree_records", "is_file", 1, "Type check on entry.file_type() with its error propagated; a link or other non-file is not a record."),
    ("crates/codeflow-core/src/ids/state.rs", "load", "ErrorKind::NotFound", 1, "Gives the empty state only after proven_absent confirms the file is missing."),
    ("crates/codeflow-core/src/ledger/rebuild.rs", "rebuild_ledger_type", "try_exists", 2, "try_exists propagates metadata errors; the only reader is the ceremony retrospective, where an empty ledger becomes Refusals::Unknown, never allow or refuse."),
    ("crates/codeflow-core/src/ledger/refusal.rs", "mark_recording", "exists", 1, "A false negative only appends a duplicate recording marker; readers take the earliest event and the caller discards the result."),
    ("crates/codeflow-core/src/model_catalog/scan.rs", "scan", "is_dir", 1, "Type check on entry.file_type() with its error propagated."),
    ("crates/codeflow-core/src/model_catalog/scan.rs", "scan", "is_file", 1, "Type check on entry.file_type() with its error propagated."),
    ("crates/codeflow-core/src/model_qualification.rs", "load_bindings", "is_dir", 1, "Absence is proven first; a directory check that fails returns Err, so an unreadable directory refuses and is never read as empty."),
    ("crates/codeflow-core/src/model_qualification.rs", "read_bounded_json", "is_file", 2, "Type checks on metadata obtained with errors propagated; false returns Err."),
    ("crates/codeflow-core/src/reading.rs", "load_under", "is_dir", 1, "Type check on entry.file_type(), whose error propagates out of load_skill_tree to its callers."),
    ("crates/codeflow-core/src/reading.rs", "load_under", "is_file", 1, "Type check on entry.file_type(), whose error propagates; a regular file is then read with its error propagated."),
    ("crates/codeflow-core/src/recall.rs", "collect_sources", "try_exists", 2, "Only decides whether capabilities.md and product.md enter the rebuildable search cache; recall output goes to a person and a missing kind is disclosed as a note."),
    ("crates/codeflow-core/src/recall.rs", "files_in", "is_file", 1, "Type check on entry.file_type(), whose error propagates as RecallError::Io."),
    ("crates/codeflow-core/src/recall.rs", "files_in", "try_exists", 1, "Only decides which ledger and doc files enter the search cache; results reach a person and an empty kind is disclosed as a coverage note."),
    ("crates/codeflow-core/src/recall.rs", "md_files_under", "is_dir", 1, "Type check on entry.file_type(), whose error propagates as RecallError::Io."),
    ("crates/codeflow-core/src/recall.rs", "md_files_under", "is_file", 1, "Type check on entry.file_type(), whose error propagates as RecallError::Io."),
    ("crates/codeflow-core/src/recall.rs", "md_files_under", "try_exists", 1, "Only decides whether docs/plan files enter the search cache; output reaches a person and a missing kind is disclosed as a note."),
    ("crates/codeflow-core/src/recall.rs", "recall", "is_dir", 1, "An unusable target root is skipped with a printed coverage note and no cache rows are deleted for it; the result is search output for a person."),
    ("crates/codeflow-core/src/recall.rs", "runtime_state_dir", "is_dir", 1, "Type check on fs::metadata of .git, whose error propagates."),
    ("crates/codeflow-core/src/recall.rs", "runtime_state_dir", "is_file", 1, "Type check on fs::metadata of .git, whose error propagates."),
    ("crates/codeflow-core/src/recall.rs", "runtime_state_dir", "try_exists", 1, "Its only caller outside tests is recall's collect_sources; None only drops ledger kinds from the search cache, disclosed as a coverage note."),
    ("crates/codeflow-core/src/registry.rs", "UserConfig::load", "ErrorKind::NotFound", 1, "The value only sets recall's default result limit, which shapes printed output and no decision."),
    ("crates/codeflow-core/src/registry.rs", "initialized_at", "is_file", 1, "Runs only after proven_absent is false, on std::fs::metadata whose error propagates as RootUnreadable."),
    ("crates/codeflow-core/src/registry.rs", "list_repos", "is_file", 1, "The only consumer is recall: with --all an empty list refuses, otherwise it only counts a printed note."),
    ("crates/codeflow-core/src/registry.rs", "read_project_info", "try_exists", 1, "Callers first pass initialized_at, which refuses a dangling project.toml; the results are only name and tier labels in the per-user registry."),
    ("crates/codeflow-core/src/release_local.rs", "adopted", "is_file", 1, "Absence is proven first; the type check runs on std::fs::metadata whose error is propagated."),
    ("crates/codeflow-core/src/root_checkout.rs", "git_marker", "is_dir", 1, "After proven absence, metadata errors propagate before repository marker type classification."),
    ("crates/codeflow-core/src/root_checkout.rs", "git_marker", "is_file", 2, "After proven absence, metadata errors propagate; these classify or reject the obtained marker."),
    ("crates/codeflow-core/src/root_checkout.rs", "nested_repositories", "is_dir", 2, "First receiver is fallibly obtained FileType; the .codeflow path predicate only selects a displayed kind, with both kinds retained in the same nested-repository/ignore inventory."),
    ("crates/codeflow-core/src/scaffold/assets.rs", "DirSource::read", "ErrorKind::NotFound", 1, "Returns None only when proven_absent confirms the asset is missing; a dangling link or an unreadable ancestor is an error."),
    ("crates/codeflow-core/src/scaffold/detect.rs", "detect_hook_manager", "is_dir", 1, "Type check on metadata obtained with its error propagated, after path_exists proved the .husky entry present."),
    ("crates/codeflow-core/src/scaffold/detect.rs", "git_dir_hooks", "is_file", 1, "The result only builds an advisory report note on init and update, or a doctor Warn instead of Pass; a Warn never fails doctor and nothing is written."),
    ("crates/codeflow-core/src/scaffold/detect.rs", "is_executable", "is_file", 1, "Windows variant; callers are the advisory git_dir_hooks note and doctor shims_not_called after the hook was read, choosing a Broken Warn or an Unverified Note, never Fail or a write."),
    ("crates/codeflow-core/src/scaffold/gitutil.rs", "add_and_commit", "exists", 1, "Its only caller passes paths this run just wrote through guard_beneath_root (no links), or the baseline directory when truly absent; exists can only misread them under a concurrent change."),
    ("crates/codeflow-core/src/scaffold/init.rs", "init_writes", "exists", 4, "A false state check reaches state.store, whose read_beneath_root refuses a link or IO error before any write; the policy check feeds diagnose, whose read_policy refuses a link; the third is a Codex note only."),
    ("crates/codeflow-core/src/scaffold/mod.rs", "path_exists", "try_exists", 1, "A false try_exists is accepted only when proven_absent confirms it; a dangling leaf or ancestor link returns an error."),
    ("crates/codeflow-core/src/scaffold/mod.rs", "should_skip_initial_stack_adr", "ErrorKind::NotFound", 1, "A listing not found is accepted only when proven_absent confirms docs/decisions is missing; a dangling link refuses."),
    ("crates/codeflow-core/src/scaffold/mod.rs", "should_skip_initial_stack_adr", "is_file", 1, "Type check on entry.file_type() with its error propagated; the listing itself was obtained or refused first."),
    ("crates/codeflow-core/src/scaffold/portal.rs", "acquire_portal_transaction_lease", "ErrorKind::NotFound", 1, "PortalIo::metadata stats without following links in a parent held through no-follow opens, so not found is a missing name; lock_file then creates it."),
    ("crates/codeflow-core/src/scaffold/portal.rs", "acquire_portal_transaction_lease", "is_file", 1, "Type check on metadata obtained from PortalIo::metadata; any other error returns lease_open_error."),
    ("crates/codeflow-core/src/scaffold/portal.rs", "authenticated_cleanup_outputs", "ErrorKind::NotFound", 1, "entries walks no-follow directory opens, so not found means no after directory inside the held, just listed transaction directory; nothing staged to remove."),
    ("crates/codeflow-core/src/scaffold/portal.rs", "missing_parent_directories", "ErrorKind::NotFound", 1, "PortalIo::metadata not found is a missing name under a held no-follow parent; the ancestor is recorded as missing so it is created and removed on rollback."),
    ("crates/codeflow-core/src/scaffold/portal.rs", "missing_parent_directories", "is_dir", 1, "Type check on metadata obtained from PortalIo::metadata; other errors return ScaffoldError::io."),
    ("crates/codeflow-core/src/scaffold/portal.rs", "plan_legacy_baseline_removal", "ErrorKind::NotFound", 1, "entries opens the baseline root through no-follow components, so not found proves no legacy baseline directory; a link or other error returns ScaffoldError::io."),
    ("crates/codeflow-core/src/scaffold/portal.rs", "read_project_file", "ErrorKind::NotFound", 1, "ConfinedRoot::read opens each component and the leaf without following links, so not found is a missing name; a link fails and is refused."),
    ("crates/codeflow-core/src/scaffold/portal.rs", "read_state_bytes", "ErrorKind::NotFound", 1, "guard_beneath_root refuses a link first and ConfinedRoot::read does not follow links, so not found proves no adoption state; other errors return ScaffoldError::io."),
    ("crates/codeflow-core/src/scaffold/portal.rs", "recover_portal_transaction", "ErrorKind::NotFound", 1, "PortalIo::metadata not found is a missing journal name under a held no-follow parent, so there is no transaction to recover; other errors propagate."),
    ("crates/codeflow-core/src/scaffold/portal.rs", "recover_portal_transaction", "is_dir", 1, "Type check on metadata obtained from PortalIo::metadata with errors propagated."),
    ("crates/codeflow-core/src/scaffold/portal.rs", "remove_portal_transaction", "ErrorKind::NotFound", 1, "Not found from PortalIo::metadata proves no journal under a held no-follow parent, so there is nothing to remove; other errors return ScaffoldError::io."),
    ("crates/codeflow-core/src/scaffold/portal.rs", "remove_portal_transaction", "is_dir", 1, "Type check on metadata obtained from PortalIo::metadata in the Ok arm; errors go to the not-found and io arms."),
    ("crates/codeflow-core/src/scaffold/portal.rs", "remove_staged_transaction", "is_dir", 1, "Type check on metadata obtained with its error propagated."),
    ("crates/codeflow-core/src/scaffold/portal.rs", "remove_transaction_tombstone", "is_dir", 1, "Type check on metadata obtained with its error propagated."),
    ("crates/codeflow-core/src/scaffold/portal.rs", "rollback_mutations", "ErrorKind::NotFound", 1, "Removing a directory this transaction created; not found under held no-follow traversal means it is already gone, the rollback goal; other errors propagate."),
    ("crates/codeflow-core/src/scaffold/portal.rs", "setup_portal", "exists", 1, "A false result returns Err(NotInitialized), so an unreadable or dangling project.toml refuses rather than proceeding."),
    ("crates/codeflow-core/src/scaffold/portal.rs", "setup_portal_locked", "is_dir", 1, "Type check on metadata from PortalIo::inspect, which propagates every error other than a proven missing name."),
    ("crates/codeflow-core/src/scaffold/portal.rs", "setup_portal_locked", "is_file", 1, "Type check on metadata from PortalIo::inspect, which propagates every error other than a proven missing name."),
    ("crates/codeflow-core/src/scaffold/portal.rs", "snapshot_project_file", "ErrorKind::NotFound", 1, "PortalIo::read goes through the no-follow ConfinedRoot::read, so not found proves the file absent; other errors refuse with InvalidState."),
    ("crates/codeflow-core/src/scaffold/portal.rs", "transfer_portal", "ErrorKind::NotFound", 1, "Portal root absence is proven under a held no-follow parent; transfer only rewrites state, and per-file reads keep deletions through read_project_file."),
    ("crates/codeflow-core/src/scaffold/portal.rs", "transfer_portal", "exists", 1, "A false result returns Err(NotInitialized), a refusal."),
    ("crates/codeflow-core/src/scaffold/portal.rs", "transfer_portal", "is_dir", 1, "Type check on metadata from PortalIo::metadata; other errors return ScaffoldError::io."),
    ("crates/codeflow-core/src/scaffold/portal.rs", "update_adopted_portal", "exists", 1, "Its only caller runs after scaffold::update loaded project.toml and rewrote it through guard_beneath_root, so the file is a regular file there."),
    ("crates/codeflow-core/src/scaffold/portal/transaction_io.rs", "PortalIo::inspect", "ErrorKind::NotFound", 1, "metadata walks no-follow directory opens and stats without following links, so not found proves a missing ancestor or leaf; other errors propagate."),
    ("crates/codeflow-core/src/scaffold/portal/transaction_io.rs", "PortalIo::parent", "ErrorKind::NotFound", 1, "enter opens each directory without following links, so a link or file component fails; not found is a missing directory, created only when asked."),
    ("crates/codeflow-core/src/scaffold/portal/transaction_io.rs", "PortalIo::remove_with_hook", "ErrorKind::NotFound", 2, "A missing ancestor or leaf under held no-follow traversal means nothing to remove; every other error propagates."),
    ("crates/codeflow-core/src/scaffold/portal/transaction_io.rs", "PortalIo::remove_with_hook", "is_dir", 1, "Type check on metadata from platform::metadata, whose errors go to the not-found and propagate arms."),
    ("crates/codeflow-core/src/scaffold/portal/transaction_io.rs", "PortalIo::remove_with_hook", "is_file", 1, "Type check on metadata from platform::metadata, whose errors go to the not-found and propagate arms."),
    ("crates/codeflow-core/src/scaffold/portal/transaction_io.rs", "PortalIo::rename_with_hook", "ErrorKind::NotFound", 1, "A destination not found by a no-follow stat in the held new parent is proven absence, the precondition for rename; other errors propagate."),
    ("crates/codeflow-core/src/scaffold/portal/transaction_io.rs", "PortalIo::write_with_hook", "ErrorKind::NotFound", 1, "A missing leaf in the held parent is proven by a no-follow stat; the write then goes through a temporary file and rename, and links fail as invalid."),
    ("crates/codeflow-core/src/scaffold/portal/transaction_io.rs", "PortalIo::write_with_hook", "is_file", 1, "Type check on metadata from platform::metadata with errors propagated."),
    ("crates/codeflow-core/src/scaffold/portal/transaction_io.rs", "platform::create_file", "ErrorKind::NotFound", 1, "A no-follow stat not finding the leaf in the held parent proves it missing; the create then opens without following links, and other errors propagate."),
    ("crates/codeflow-core/src/scaffold/portal/transaction_io.rs", "platform::create_file", "is_file", 3, "Type checks on metadata obtained with propagated errors (platform::metadata and the opened file's metadata on each platform)."),
    ("crates/codeflow-core/src/scaffold/portal/transaction_io.rs", "platform::remove", "is_dir", 1, "Windows type check on the metadata of the handle just opened; errors propagate."),
    ("crates/codeflow-core/src/scaffold/portal/transaction_io.rs", "platform::remove", "is_file", 1, "Windows type check on the metadata of the handle just opened; errors propagate."),
    ("crates/codeflow-core/src/scaffold/pr_template.rs", "automation_notes", "is_file", 1, "The result only adds Dependabot or Renovate report notes on init; the function is read-only and feeds no decision."),
    ("crates/codeflow-core/src/scaffold/pr_template.rs", "candidates", "is_dir", 1, "Type check on entry.file_type() with its error propagated."),
    ("crates/codeflow-core/src/scaffold/pr_template.rs", "candidates", "is_file", 1, "Type check on entry.file_type() with its error propagated."),
    ("crates/codeflow-core/src/scaffold/state.rs", "ProjectState::exists", "exists", 1, "Callers: init, where a false result reaches state.store, which refuses before writing, and doctor, where it only picks finding wording; both doctor branches are Wiring::Broken."),
    ("crates/codeflow-core/src/scaffold/state.rs", "ProjectState::load", "ErrorKind::NotFound", 1, "Not found becomes Err(ScaffoldError::NotInitialized), a refusal."),
    ("crates/codeflow-core/src/scaffold/state.rs", "ScaffoldConfig::load", "ErrorKind::NotFound", 1, "Its only callers run right after ProjectState::load read the same file and turned not found into NotInitialized, so not found here needs a concurrent delete."),
    ("crates/codeflow-core/src/scaffold/state.rs", "read_beneath_root", "ErrorKind::NotFound", 1, "guard_beneath_root refuses any linked component beneath the root first, so not found means a missing leaf or plain ancestor; other read errors propagate."),
    ("crates/codeflow-core/src/scaffold/state.rs", "remove_beneath_root", "ErrorKind::NotFound", 1, "guard_beneath_root refuses links first, so not found proves the file already absent, the goal of the remove; other errors propagate."),
    ("crates/codeflow-core/src/scaffold/state.rs", "sync::device_at", "exists", 1, "Only picks which device the sync batch flushes; the write that follows fails on a parent it cannot resolve, so no allow, refuse or presence decision depends on it."),
    ("crates/codeflow-core/src/scaffold/state.rs", "write_file_with_mode", "is_file", 1, "Type check on metadata from symlink_metadata_optional, whose read errors propagate; only a proven-absent or non-file mode_from leaves the default permissions."),
    ("crates/codeflow-core/src/scaffold/update.rs", "prune_orphans", "ErrorKind::NotFound", 1, "guard_beneath_root refuses links in the destination first, so not found is proven absence; an absent orphan only drops its record and baseline."),
    ("crates/codeflow-core/src/scaffold/update.rs", "prune_orphans", "exists", 1, "Repeats a read that either succeeded or proved the file missing beneath a link-free path; remove_beneath_root also tolerates only not found."),
    ("crates/codeflow-core/src/security/deletion.rs", "Reader::change_dir", "is_dir", 1, "Metadata classification failure sets may_fail and preserves both shell outcomes, never removes the target candidate."),
    ("crates/codeflow-core/src/security/deletion.rs", "Reader::glob_paths", "is_dir", 1, "A failed directory listing is ignored only for proven absence or successfully obtained non-directory metadata; other failures keep expansion unproven."),
    ("crates/codeflow-core/src/security/prose.rs", "plain_target", "is_file", 1, "FileType is obtained from successful metadata after proven absence handling; unreadable targets do not certify prose."),
    ("crates/codeflow-core/src/status.rs", "collect_delivery", "is_dir", 1, "Feeds only the status delivery rollup; codeflow status prints the view and always exits 0."),
    ("crates/codeflow-core/src/status.rs", "collect_work", "is_dir", 1, "Feeds only a status note and summary counts; codeflow status prints the view and always exits 0."),
    ("crates/codeflow-core/src/testing/delivery.rs", "green_base", "try_exists", 1, "try_exists propagates metadata errors; Ok(false) (a missing or dangling evidence directory) gives no green base, and select then runs every target."),
    ("crates/codeflow-core/src/testing/doctor/mod.rs", "run_all_checks", "exists", 1, "False returns a Fail config-exists check, so an unreadable path is never passed; a present config is then loaded by load_test_config, which refuses a dangling leaf."),
    ("crates/codeflow-core/src/testing/doctor/mod.rs", "run_target_checks", "exists", 1, "Only picks Pass or Warn text for a coverage exception path; exceptions are matched by path name in threshold evaluation and never read from disk."),
    ("crates/codeflow-core/src/testing/doctor/mod.rs", "run_target_checks", "is_dir", 1, "False yields a Fail cwd-exists check, so an unreadable or dangling target directory is reported failing, never passed."),
    ("crates/codeflow-core/src/testing/gate.rs", "copy_evidence", "is_dir", 1, "The entry file_type acquisition propagates errors before choosing recursive versus file copy."),
    ("crates/codeflow-core/src/testing/runner/mod.rs", "run_owed_target", "exists", 4, "One guards removing a stale report that feeds only the display summary; one false fails the target; two pick evidence copies, and the gate rereads the original paths."),
    ("crates/codeflow-core/src/testing/setup/detect.rs", "detect_go", "try_exists", 1, "try_exists propagates metadata errors; only a dangling go.mod link reads false, which only drops a proposed target from a starting config the tool could not build anyway."),
    ("crates/codeflow-core/src/testing/setup/detect.rs", "detect_node", "try_exists", 1, "try_exists propagates metadata errors; only a dangling package.json link reads false, which only drops a proposed target from a starting config."),
    ("crates/codeflow-core/src/testing/setup/detect.rs", "detect_python", "try_exists", 1, "try_exists and the read propagate their errors; only a dangling manifest link reads false, which only drops a proposed target from a starting config."),
    ("crates/codeflow-core/src/testing/setup/detect.rs", "detect_rust", "try_exists", 1, "try_exists propagates metadata errors; only a dangling Cargo.toml link reads false, which only drops a proposed target from a starting config."),
    ("crates/codeflow-core/src/testing/setup/mod.rs", "get_template_list", "exists", 1, "Feeds only list_templates, which prints template names; applying a template is checked separately in run_template."),
    ("crates/codeflow-core/src/testing/setup/mod.rs", "is_project_root", "is_dir", 1, "Type check on std::fs::metadata obtained after proven_absent, with both errors propagated."),
    ("crates/codeflow-core/src/testing/setup/mod.rs", "is_project_root", "is_file", 1, "Type check on std::fs::metadata obtained after proven_absent, with both errors propagated."),
    ("crates/codeflow-core/src/testing/setup/mod.rs", "run_add_target", "exists", 1, "False refuses with NoConfigForAddTarget, so an unreadable or dangling config is never treated as present or overwritten."),
    ("crates/codeflow-core/src/testing/setup/mod.rs", "run_auto", "exists", 1, "False leads to write_config, whose guard_beneath_root refuses a symlinked leaf, a dangling one included, and a path whose metadata fails cannot be written either."),
    ("crates/codeflow-core/src/testing/setup/mod.rs", "run_interactive", "exists", 1, "Only decides whether the wizard asks about an existing config; write_config then refuses a symlinked or dangling leaf through guard_beneath_root."),
    ("crates/codeflow-core/src/testing/setup/mod.rs", "run_template", "exists", 1, "False refuses with TemplateNotFound; an existing template is then read with its error propagated."),
    ("crates/codeflow-core/src/testing/setup/mod.rs", "run_template_content", "exists", 1, "False leads to write_config, whose guard_beneath_root refuses a symlinked or dangling leaf, and a path whose metadata fails cannot be written either."),
    ("crates/codeflow-core/src/testing/setup/mod.rs", "write_minimal_config", "exists", 1, "False leads to write_config, whose guard_beneath_root refuses a symlinked or dangling leaf, so an existing config is never replaced."),
    ("crates/codeflow-core/src/testing/structural/mod.rs", "gather_matches", "is_file", 1, "The module compiles only under the structural-check feature, which no shipped crate enables, and it is not wired into the gate."),
    ("crates/codeflow-core/src/testing/structural/mod.rs", "refuse_names_that_are_not_text", "ErrorKind::NotFound", 1, "Parked behind the structural-check feature and not built into codeflow; canonicalize just before resolved the directory, so NotFound only means a concurrent removal."),
    ("crates/codeflow-core/src/testing/structural/mod.rs", "refuse_names_that_are_not_text", "is_dir", 1, "The module compiles only under the structural-check feature, which no shipped crate enables, and it is not wired into the gate."),
    ("crates/codeflow-core/src/testing/structural/mod.rs", "validate_target", "is_file", 1, "False records a missing finding and fails the result, so an unreadable test is never counted present; the module is also parked behind the structural-check feature."),
    ("crates/codeflow-core/src/testing/validation.rs", "parse_target_coverage", "exists", 1, "False gives empty coverage, which collect_coverage_reports marks data_missing for an enforcing target; without coverage rules it is informational only."),
    ("crates/codeflow-core/src/testing/validation.rs", "parse_target_report", "exists", 1, "The report only builds the display FailureReport after a run; the gate verdict uses the exit code."),
    ("crates/codeflow-core/src/validate/docs.rs", "directory_present", "is_dir", 1, "Type check on fs::metadata after proven absence in input_present; a metadata error or a non-directory records cannot_read, a blocking issue."),
    ("crates/codeflow-core/src/validate/mod.rs", "awaiting_selection_errors", "exists", 1, "False only adds the validation error that the path must exist, so an unreadable path refuses the record and never passes it."),
    ("crates/codeflow-core/src/validate/portal.rs", "collect_dist_artifacts_with_entry_limit", "is_dir", 1, "Type check on symlink_metadata of dist, whose error arm pushes a built artifact root unreadable issue."),
    ("crates/codeflow-core/src/validate/portal.rs", "collect_dist_artifacts_with_entry_limit::visit", "is_dir", 1, "Type check on entry.file_type(), whose error arm pushes a metadata unreadable issue."),
    ("crates/codeflow-core/src/validate/portal.rs", "collect_dist_artifacts_with_entry_limit::visit", "is_file", 1, "Type check on entry.file_type(); other kinds are refused as non-regular, and entry metadata errors push an issue."),
    ("crates/codeflow-core/src/validate/portal.rs", "collect_reserved_public_files", "ErrorKind::NotFound", 1, "An absent namespace only empties the actual set; verify_reserved_public_inventory then reports claimed generated public output as absent for every expected path."),
    ("crates/codeflow-core/src/validate/portal.rs", "collect_reserved_public_files", "is_dir", 1, "Type check on entry.file_type(), whose error arm pushes a metadata unreadable issue."),
    ("crates/codeflow-core/src/validate/portal.rs", "collect_reserved_public_files", "is_file", 1, "Type check on entry.file_type(), whose error arm pushes an issue; size errors also push an issue and stop."),
    ("crates/codeflow-core/src/validate/portal.rs", "parse_git_batch", "ErrorKind::NotFound", 1, "Builds an error from git cat-file's documented missing header; every caller records any Err as a blocking issue."),
    ("crates/codeflow-core/src/workgraph/allocate.rs", "direct_record_stems", "ErrorKind::NotFound", 1, "Only epic and spec creation use it; under a dangling link create_dir_all fails, and write_new uses create_new, so nothing is overwritten."),
    ("crates/codeflow-core/src/workgraph/allocate.rs", "direct_record_stems", "is_dir", 1, "Type check on entry.file_type() with its error propagated."),
    ("crates/codeflow-core/src/workgraph/layout.rs", "bounded_entries", "ErrorKind::NotFound", 1, "The only caller is the estimate inventory; a referenced record that is missing raises a task, epic or spec resolution finding."),
    ("crates/codeflow-core/src/workgraph/layout.rs", "bounded_entries", "is_dir", 1, "Type check on symlink_metadata already obtained; other errors map to Unreadable."),
    ("crates/codeflow-core/src/workgraph/layout.rs", "bounded_markdown_files", "is_file", 1, "Type check on entry.file_type() with errors mapped to Unreadable."),
    ("crates/codeflow-core/src/workgraph/layout.rs", "bounded_record_files", "ErrorKind::NotFound", 1, "The nested leaf sits in an epic entry already shown to be a real directory by its file type, so not found is proven absence."),
    ("crates/codeflow-core/src/workgraph/layout.rs", "bounded_record_files", "is_dir", 1, "Type check on entry.file_type() with errors mapped to Unreadable."),
    ("crates/codeflow-core/src/workgraph/layout.rs", "bounded_record_files", "is_file", 2, "Type checks on entry.file_type() (errors mapped to Unreadable) and on symlink_metadata already obtained."),
    ("crates/codeflow-core/src/workgraph/layout.rs", "checked_directory_entries", "ErrorKind::NotFound", 1, "Callers probe project-management under the discovered repository root, then children of directories verified real, so a leaf not found is proven absence."),
    ("crates/codeflow-core/src/workgraph/layout.rs", "checked_directory_entries", "is_dir", 1, "Type check on symlink_metadata already obtained; other errors return Unreadable."),
    ("crates/codeflow-core/src/workgraph/layout.rs", "direct_markdown_files", "is_file", 1, "Type check on entry.file_type() with its error propagated."),
    ("crates/codeflow-core/src/workgraph/layout.rs", "epic_record_files", "ErrorKind::NotFound", 1, "The nested leaf is in an entry shown to be a real directory by its file type, so not found is proven absence."),
    ("crates/codeflow-core/src/workgraph/layout.rs", "epic_record_files", "is_dir", 1, "Type check on entry.file_type() with its error propagated."),
    ("crates/codeflow-core/src/workgraph/layout.rs", "epic_record_files", "is_file", 1, "Type check on symlink_metadata already obtained; other errors return."),
    ("crates/codeflow-core/src/workgraph/layout.rs", "has_task_record_path", "is_dir", 1, "Type check on entry.file_type() with errors mapped to Unreadable."),
    ("crates/codeflow-core/src/workgraph/layout.rs", "real_directory_entries", "ErrorKind::NotFound", 1, "Empty only when proven_absent confirms the record folder is missing; a record home below a dangling link returns the error."),
    ("crates/codeflow-core/src/workgraph/layout.rs", "real_directory_entries", "is_dir", 1, "Type check on symlink_metadata already obtained; other errors propagate."),
    ("crates/codeflow-core/src/workgraph/layout.rs", "task_dir_has_record", "is_file", 1, "Type check on entry.file_type() with errors mapped to Unreadable."),
    ("crates/codeflow-core/src/workgraph/layout.rs", "task_record_files", "is_dir", 1, "Type check on entry.file_type() with its error propagated."),
    ("crates/codeflow-core/src/workgraph/lifecycle.rs", "recorded_baseline", "ErrorKind::NotFound", 1, "Gives the empty baseline only after proven_absent confirms the file is missing."),
    ("crates/codeflow-core/src/workgraph/light_paths.rs", "next_adr_id", "ErrorKind::NotFound", 1, "Starts at ADR-0001 only after proven_absent confirms the directory is missing."),
    ("crates/codeflow-core/src/workgraph/record_template.rs", "load", "ErrorKind::NotFound", 1, "Uses the embedded template only when proven_absent confirms the project template is missing; otherwise it refuses."),
    ("crates/codeflow-core/src/workgraph/release_line.rs", "shallow_boundary", "ErrorKind::NotFound", 1, "Git keeps a linked worktree's shallow list in the common directory, so only a missing file in the worktree's own git directory falls back to it; that read's error, and any other error, refuses with the shallow-boundary reason."),
    ("crates/codeflow-core/src/workgraph/release_line.rs", "History::parent_of", "exists", 1, "An object database without the object returns Err with a fetch or unshallow remedy; it never reads as a root or as absent."),
    ("crates/codeflow-core/src/workgraph/release_line.rs", "history_overlay", "ErrorKind::NotFound", 1, "A graft file that cannot be found is unreadable to git and libgit2 as well, so no overlay is in effect for either; other errors refuse."),
    ("crates/codeflow-core/src/workgraph/work_start.rs", "durable_work_tracking_enabled", "ErrorKind::NotFound", 2, "Callers pass the discovered repository root: a .codeflow that is not found there is proven, and the state file is read under a real .codeflow or an absent one."),
    ("crates/codeflow-core/src/workgraph/work_start.rs", "durable_work_tracking_enabled", "is_dir", 1, "Type check on symlink_metadata already obtained; a non-directory, a symlink included, returns StatePath, and other errors return StateMetadata."),
    ("crates/codeflow-core/src/workgraph/work_start.rs", "durable_work_tracking_enabled", "is_file", 1, "Type check on symlink_metadata already obtained; a non-file returns StatePath, and other errors return StateMetadata."),
    ("crates/codeflow-core/src/estimate/adoption.rs", "home", "ErrorKind::NotFound", 1, "Only a root that names nothing is Home::Missing; any other metadata error is Home::Unusable with the reason."),
    ("crates/codeflow-core/src/estimate/adoption.rs", "home", "is_dir", 1, "The kind of a root whose metadata was read: a non-directory is Home::Unusable, never missing."),
    ("crates/codeflow-core/src/estimate/adoption.rs", "home", "is_file", 1, "The kind of a v<N>.json entry whose file type was read; a type that cannot be read leaves the home unusable."),
    ("crates/codeflow-core/src/estimate/adoption.rs", "real_dirs", "ErrorKind::NotFound", 1, "A forecasts directory that is proven missing holds no forecast; any other metadata error refuses through Home::Unusable."),
    ("crates/codeflow-core/src/estimate/adoption.rs", "real_dirs", "is_dir", 2, "The kinds of the forecasts directory and of each entry, read from metadata that was read; an unreadable entry refuses."),
];

#[test]
fn hook_and_guard_absence_is_proven() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);
    let mut actual = BTreeMap::new();
    for file in files {
        let name = relative(&root, &file);
        if !absence_scope(&name) {
            continue;
        }
        for ((item, call), lines) in absence_sites(&std::fs::read_to_string(&file).unwrap()) {
            println!("{name} {item} {call} {} {lines:?}", lines.len());
            actual.insert((name.clone(), item, call), lines.len());
        }
    }
    let mut allowed = BTreeMap::new();
    for (file, item, call, count, reason) in ABSENCE_EXCEPTIONS {
        assert!(absence_scope(file));
        assert!(*call == "ErrorKind::NotFound" || ABSENCE_METHODS.contains(call));
        assert!(*count > 0 && !reason.trim().is_empty());
        assert!(allowed
            .insert(
                (file.to_string(), item.to_string(), call.to_string()),
                *count
            )
            .is_none());
    }
    assert_listed(
        &actual,
        &allowed,
        "absence needs proven_absent or a counted proof with a reason",
    );
}

#[test]
fn absence_scan_is_scoped_and_counts_paths_methods_and_macros() {
    for expression in [
        "matches!(e.kind(), std::io::ErrorKind::NotFound)",
        "if e.kind() == ErrorKind::NotFound {}",
        "match e.kind() { io::ErrorKind::NotFound => (), _ => () }",
        "p.try_exists()",
        "Path::try_exists(p)",
        "iter.map(Path::try_exists)",
        "opaque!(p.try_exists(); ErrorKind::NotFound)",
    ] {
        let source = format!("fn f() {{ {expression}; }}");
        assert_eq!(
            absence_sites(&source).values().map(Vec::len).sum::<usize>(),
            if expression.starts_with("opaque") {
                2
            } else {
                1
            },
            "{expression}"
        );
    }
    let source = "impl R { fn f() { a.try_exists(); b.try_exists(); } } #[cfg(test)] mod tests { fn t() { a.try_exists(); } }";
    assert_eq!(
        absence_sites(source)
            .get(&("R::f".into(), "try_exists".into()))
            .unwrap()
            .len(),
        2
    );
    assert!(absence_sites(r#"fn f() { let try_exists = 1; let s = "ErrorKind::NotFound"; git2::ErrorCode::NotFound; s.split(' '); s.replace('x', "y"); }"#).is_empty());
    for file in [
        "hooks/policy.rs",
        "security/prose.rs",
        "root_checkout.rs",
        "remote.rs",
    ] {
        assert!(absence_scope(&format!("crates/codeflow-core/src/{file}")));
    }
    for file in [
        "crates/codeflow-cli/src/cmd/git_hook.rs",
        "crates/codeflow-core/src/scaffold/state.rs",
        "crates/codeflow-core/src/scaffold/pr_template.rs",
        "crates/codeflow-cli/src/cmd/remote.rs",
    ] {
        assert!(absence_scope(file));
    }
    for file in [
        "crates/codeflow-core/tests/name_decode_scan.rs",
        "crates/codeflow-present/src/state.rs",
        "unrelated/remote.rs",
    ] {
        assert!(!absence_scope(file));
    }
    let actual = absence_sites("fn f() { a.try_exists(); }");
    assert_ne!(actual, BTreeMap::new(), "new sites fail an empty allowance");
    assert_ne!(
        actual,
        absence_sites("fn f() { a.try_exists(); b.try_exists(); }"),
        "stale counts fail"
    );
}

#[test]
fn absence_scan_covers_boolean_path_probes_and_the_config_chain() {
    for method in ABSENCE_METHODS {
        for expression in [
            format!("p.{method}()"),
            format!("Path::{method}(p)"),
            format!("iter.map(std::path::Path::{method})"),
            format!("opaque!(nested!(p.{method}()); ErrorKind::NotFound)"),
        ] {
            let source = format!("fn f() {{ {expression}; }}");
            assert_eq!(
                absence_sites(&source).values().map(Vec::len).sum::<usize>(),
                if expression.starts_with("opaque") {
                    2
                } else {
                    1
                },
                "{expression}"
            );
        }
    }
    assert_eq!(
        absence_sites("fn f() { metadata.is_dir(); kind.is_file(); }").len(),
        2,
        "obtained metadata needs counted reasons too"
    );
    assert!(absence_sites(
        r#"fn f() { let exists=1; let is_file=2; let is_dir=3; let s="p.exists()"; }"#
    )
    .is_empty());
    assert!(absence_sites("#[cfg(test)] mod tests { fn t() { p.exists(); } }").is_empty());
    for path in [
        "testing/gate.rs",
        "testing/config/mod.rs",
        "testing/config/extra.rs",
        "hooks/git_hook.rs",
    ] {
        assert!(absence_scope(&format!("crates/codeflow-core/src/{path}")));
    }
    for path in [
        "testing/runner.rs",
        "testing/gate_extra.rs",
        "testing/configuration.rs",
    ] {
        assert!(absence_scope(&format!("crates/codeflow-core/src/{path}")));
        assert!(!absence_scope(&format!(
            "crates/codeflow-core/tests/{path}"
        )));
    }
}
