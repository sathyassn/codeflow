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
//! A site is allowed only when listed in [`EXCEPTIONS`] with the reason it
//! never feeds a decision, keyed by file, enclosing item and call, with a
//! count: a new call in an item, or a fixed one that leaves an entry, fails
//! the test. Items marked `#[cfg(test)]` are skipped one by one. The layer
//! itself (`git/name.rs`) is not scanned.
//!
//! Guard modules also reject Unicode whitespace splitting/trimming unless
//! a counted exception explains the applicable grammar or non-command use.
//! Shell, Git and utility
//! separators are explicit so non-separator characters keep their identity.
//!
//! Limits, stated rather than hidden: a text conversion that is not one of
//! these calls (`Path::display().to_string()`, `to_str().unwrap_or("")`,
//! `format!("{path:?}")`) is not seen; the tests of each site (a
//! collision, a refusal or a whole-path fixture) are its primary
//! protection; a procedural macro that builds the call is not parsed.

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
];
const GIT2_TEXT: &[&str] = &["shorthand", "symbolic_target"];

/// Production sites that decode lossily or read git2 text, where the value
/// only reaches a person or is not a git or OS name: (file, enclosing item,
/// call, how many, why it never feeds a decision).
const EXCEPTIONS: &[(&str, &str, &str, usize, &str)] = &[
    ("codeflow-cli/examples/release_integration.rs", "command", "from_utf8_lossy", 2, "process output shown to a person, never compared"),
    ("codeflow-cli/src/cmd/ci.rs", "base_refusal", "from_utf8_lossy", 1, "a git or tool error message shown to a person, never compared"),
    ("codeflow-cli/src/cmd/ci.rs", "enumerate_commits", "from_utf8_lossy", 2, "the first is an error message; the second is `git log` text of commit ids and messages (content), whose files are read by commit_files as exact keys"),
    (
        "codeflow-cli/src/cmd/ci.rs",
        "git_bytes",
        "from_utf8_lossy",
        1,
        "an error message shown to a person",
    ),
    (
        "codeflow-cli/src/cmd/ci.rs",
        "git_stdout",
        "from_utf8_lossy",
        1,
        "the text of merge-base ids and a diff printed with core.quotepath=on, so each path in it is ASCII escapes that unquote_git_path decodes to exact bytes",
    ),
    ("codeflow-cli/src/cmd/ci.rs", "git_with_stdin", "from_utf8_lossy", 1, "a git or tool error message shown to a person, never compared"),
    ("codeflow-cli/src/cmd/ci.rs", "parse_batch", "from_utf8_lossy", 1, "the `git cat-file --batch` header line: an object id, a type and a size"),
    ("codeflow-cli/src/cmd/ci.rs", "rev_parse", "from_utf8_lossy", 1, "an object id or fixed ASCII word git prints, compared with ASCII only"),
    ("codeflow-cli/src/cmd/ci/adopter.rs", "check_head_config", "from_utf8_lossy", 1, "file or blob content, a format contract and not a name"),
    ("codeflow-cli/src/cmd/ci/classification.rs", "range_changes", "from_utf8_lossy", 1, "a git or tool error message shown to a person, never compared"),
    ("codeflow-cli/src/cmd/hook.rs", "exec_guard_canary", "from_utf8_lossy", 2, "output of the guard canary run in this process, shown by doctor"),
    ("codeflow-cli/src/cmd/push_set.rs", "commits_from", "from_utf8_lossy", 1, "a git or tool error message shown to a person, never compared"),
    ("codeflow-cli/src/cmd/push_set.rs", "run_check_with", "from_utf8_lossy", 4, "process output shown to a person, never compared"),
    ("codeflow-cli/src/command_reference.rs", "description", "to_string_lossy", 1, "a default value of a command option, written into generated reference text"),
    ("codeflow-core/src/ceremony/history.rs", "status_only", "from_utf8_lossy", 2, "file or blob content, a format contract and not a name"),
    ("codeflow-core/src/doctor/mod.rs", "Options::do_exec", "from_utf8_lossy", 2, "probe output that doctor shows"),
    ("codeflow-core/src/doctor/mod.rs", "Options::do_exec_stdin", "from_utf8_lossy", 2, "probe output that doctor shows"),
    ("codeflow-core/src/doctor/mod.rs", "run_captured", "from_utf8_lossy", 2, "process output shown to a person, never compared"),
    ("codeflow-core/src/git/ci.rs", "parse_pr_checks_output", "from_utf8_lossy", 1, "a git or tool error message shown to a person, never compared"),
    ("codeflow-core/src/hooks/conflict_markers.rs", "check", "name", 1, "a conflict marker kind's own name, not a git name (git2 is in the file)"),
    ("codeflow-core/src/hooks/conflict_markers.rs", "marker_sizes", "from_utf8_lossy", 1, "a git or tool error message shown to a person, never compared"),
    ("codeflow-core/src/hooks/conflict_markers.rs", "staged", "from_utf8_lossy", 1, "file or blob content, a format contract and not a name (an added line)"),
    ("codeflow-core/src/hooks/delegate_turn.rs", "signal_tmux", "from_utf8_lossy", 1, "a git or tool error message shown to a person, never compared"),
    ("codeflow-core/src/hooks/git_discard.rs", "lossy", "from_utf8_lossy", 1, "decided in the file: a dirty name read lossily can only add a refusal, never remove one, and a lookalike is judged as the work it is"),
    ("codeflow-core/src/hooks/git_guard.rs", "WordGlob::prefix", "to_string_lossy", 1, "a component of a command word, which is valid UTF-8 text; tests only for glob syntax"),
    ("codeflow-core/src/hooks/git_guard.rs", "expand_components", "to_string_lossy", 1, "a component of a command word, which is valid UTF-8 text; tests only for glob syntax"),
    ("codeflow-core/src/hooks/git_guard.rs", "find_action_violation", "from_utf8_lossy", 1, "a NUL list scanned for fixed ASCII enforcement/worktree substrings; replacement characters cannot erase those bytes and any added word boundary only adds a refusal"),
    ("codeflow-core/src/hooks/git_guard.rs", "find_name_matches", "to_string_lossy", 2, "a find -name pattern matched against a candidate name: a candidate that is not UTF-8 stays when the pattern has a single-character wildcard, so the lossy spelling decides only literal and `*` patterns, which it answers as the bytes would, except that a literal U+FFFD in the pattern also matches such a name, which only adds a candidate or a refusal"),
    ("codeflow-core/src/hooks/git_guard.rs", "in_enforcement_dir", "to_string_lossy", 1, "compared with ASCII enforcement folder names, which U+FFFD never equals, so an invalid name is none of them"),
    ("codeflow-core/src/hooks/git_guard.rs", "read_alias", "from_utf8_lossy", 1, "a git or tool error message shown to a person, never compared"),
    ("codeflow-core/src/hooks/git_guard.rs", "read_branch_name", "from_utf8_lossy", 1, "a git or tool error message shown to a person, never compared"),
    ("codeflow-core/src/hooks/git_guard.rs", "sed_text_violation", "from_utf8_lossy", 1, "script content scanned for fixed ASCII enforcement substrings; replacement characters are non-name boundaries, so the decode can only add matches, never remove an ASCII needle"),
    ("codeflow-core/src/hooks/git_hook.rs", "scan_staged", "from_utf8_lossy", 1, "file or blob content, a format contract and not a name (an added line scanned for secrets)"),
    ("codeflow-core/src/hooks/orient.rs", "generate", "from_utf8_lossy", 1, "an object id or fixed ASCII word git prints, compared with ASCII only (`hooks 3`)"),
    ("codeflow-core/src/ids/check.rs", "Texts::uid_at", "from_utf8_lossy", 1, "file or blob content, a format contract and not a name"),
    ("codeflow-core/src/ids/git.rs", "Git::blobs", "from_utf8_lossy", 1, "the `git cat-file --batch` header line: an object id, a type and a size"),
    ("codeflow-core/src/ids/git.rs", "Git::rev", "from_utf8_lossy", 1, "an object id or fixed ASCII word git prints, compared with ASCII only"),
    ("codeflow-core/src/ids/git.rs", "checked", "from_utf8_lossy", 1, "git stderr in an error message shown to a person, never compared"),
    ("codeflow-core/src/ids/git.rs", "checked_bytes", "from_utf8_lossy", 1, "git stderr in an error message shown to a person, never compared"),
    ("codeflow-core/src/ids/inventory.rs", "copies_at", "from_utf8_lossy", 1, "file or blob content, a format contract and not a name"),
    ("codeflow-core/src/ids/issue.rs", "fetch", "from_utf8_lossy", 1, "a git or tool error message shown to a person, never compared"),
    ("codeflow-core/src/ids/issue.rs", "push", "from_utf8_lossy", 2, "process output shown to a person, never compared"),
    ("codeflow-core/src/ids/ledger.rs", "Ledger::apply", "from_utf8_lossy", 1, "file or blob content, a format contract and not a name"),
    ("codeflow-core/src/ids/ledger.rs", "Ledger::check_addition", "from_utf8_lossy", 1, "file or blob content, a format contract and not a name"),
    ("codeflow-core/src/ids/ledger.rs", "Ledger::restore_problems", "from_utf8_lossy", 1, "file or blob content, a format contract and not a name"),
    ("codeflow-core/src/integrate.rs", "checkout", "from_utf8_lossy", 1, "a git or tool error message shown to a person, never compared"),
    ("codeflow-core/src/integrate.rs", "dirty_files", "from_utf8_lossy", 2, "an error message and the list of dirty files shown in a refusal, never compared"),
    ("codeflow-core/src/integrate.rs", "integrate", "from_utf8_lossy", 1, "a git or tool error message shown to a person, never compared"),
    ("codeflow-core/src/integrate.rs", "restore_checkout", "from_utf8_lossy", 1, "a git or tool error message shown to a person, never compared"),
    ("codeflow-core/src/recall.rs", "display_encoded_path", "from_utf8_lossy", 1, "a path shown to a person, decoded from its reversible encoding (the encoding is the identity)"),
    ("codeflow-core/src/recall.rs", "display_rel_to", "to_string_lossy", 1, "a path shown to a person; the reversible encoding is the identity"),
    ("codeflow-core/src/recall.rs", "encode_path", "to_string_lossy", 1, "Windows only: the readable part of the key of a path that is not text, followed by `%00` and the hex of its exact encoding, so the key is injective"),
    ("codeflow-core/src/release_local.rs", "failure", "from_utf8_lossy", 2, "process output shown to a person, never compared"),
    ("codeflow-core/src/release_local.rs", "preflight", "from_utf8_lossy", 1, "output of the release script, shown"),
    ("codeflow-core/src/remote.rs", "GithubProvider::run_gh", "from_utf8_lossy", 2, "gh output (JSON text and messages)"),
    ("codeflow-core/src/root_checkout.rs", "WorkspaceReport::fmt", "name", 1, "the branch step's own name() method, a GitName, not a git2 accessor"),
    ("codeflow-core/src/root_checkout.rs", "effective_ignore_case", "from_utf8_lossy", 1, "an object id or fixed ASCII word git prints, compared with ASCII only (`true`)"),
    ("codeflow-core/src/root_checkout.rs", "finish", "name", 1, "the branch step's own name() method, a GitName, not a git2 accessor"),
    ("codeflow-core/src/root_checkout.rs", "git_run", "from_utf8_lossy", 1, "a git or tool error message shown to a person, never compared"),
    ("codeflow-core/src/scaffold/gitutil.rs", "add_and_commit", "from_utf8_lossy", 1, "a git or tool error message shown to a person, never compared"),
    ("codeflow-core/src/scaffold/gitutil.rs", "git_ok", "from_utf8_lossy", 1, "a git or tool error message shown to a person, never compared"),
    ("codeflow-core/src/scaffold/gitutil.rs", "is_repo", "from_utf8_lossy", 1, "an object id or fixed ASCII word git prints, compared with ASCII only (`true`)"),
    ("codeflow-core/src/security/deletion.rs", "Reader::glob_paths", "to_string_lossy", 1, "a name that is not UTF-8 makes the deletion unproven when the glob has a single-character wildcard or a bracket; the lossy spelling decides only literal and `*` patterns, which it answers as the bytes would, except that a literal U+FFFD in the pattern also matches such a name, which only adds a candidate or a refusal"),
    ("codeflow-core/src/testing/coverage/cobertura.rs", "parse_cobertura_str", "from_utf8_lossy", 2, "attributes of a coverage XML report, a format contract"),
    ("codeflow-core/src/testing/delivery.rs", "observation", "from_utf8_lossy", 2, "output of a sandbox probe, shown in a report"),
    ("codeflow-core/src/testing/delivery.rs", "preflight", "from_utf8_lossy", 1, "a git or tool error message shown to a person, never compared"),
    ("codeflow-core/src/testing/report/junit.rs", "parse_junit_str", "from_utf8_lossy", 15, "attributes and text of a JUnit XML report, a format contract"),
    ("codeflow-core/src/testing/runner/mod.rs", "render_capture", "from_utf8_lossy", 1, "a test target's captured output, shown"),
    ("codeflow-core/src/testing/runner/mod.rs", "spawn_reader_with_progress", "from_utf8_lossy", 1, "a test target's output scanned for progress words"),
    ("codeflow-core/src/testing/validation.rs", "default_base_ref", "from_utf8_lossy", 1, "an object id or fixed ASCII word git prints, compared with ASCII only"),
    ("codeflow-core/src/validate/docs.rs", "find_line", "from_utf8_lossy", 1, "file or blob content, a format contract and not a name"),
    ("codeflow-core/src/validate/mod.rs", "landed_pull_requests", "from_utf8_lossy", 1, "commit subjects (message text) searched for a task id"),
    ("codeflow-core/src/validate/mod.rs", "parse_frontmatter", "from_utf8_lossy", 1, "file or blob content, a format contract and not a name"),
    ("codeflow-core/src/validate/mod.rs", "validate_sections", "from_utf8_lossy", 1, "file or blob content, a format contract and not a name"),
    ("codeflow-core/src/validate/mod.rs", "validate_task", "from_utf8_lossy", 1, "file or blob content, a format contract and not a name"),
    ("codeflow-core/src/validate/portal.rs", "git_output_bounded", "from_utf8_lossy", 1, "a git or tool error message shown to a person, never compared"),
    ("codeflow-core/src/workgraph/acceptance.rs", "RecordIndex::read", "from_utf8_lossy", 1, "file or blob content, a format contract and not a name"),
    ("codeflow-core/src/workgraph/acceptance.rs", "blob_at", "from_utf8_lossy", 1, "file or blob content, a format contract and not a name"),
    ("codeflow-core/src/workgraph/acceptance.rs", "presence_at", "from_utf8_lossy", 1, "file or blob content, a format contract and not a name"),
    ("codeflow-core/src/workgraph/lifecycle.rs", "Graph::from_revision", "from_utf8_lossy", 1, "file or blob content, a format contract and not a name"),
    ("codeflow-core/src/workgraph/lifecycle.rs", "RecordView::parse", "from_utf8_lossy", 1, "file or blob content, a format contract and not a name"),
    ("codeflow-core/src/workgraph/lifecycle.rs", "implemented_at_a_commit", "from_utf8_lossy", 1, "file or blob content, a format contract and not a name"),
    ("codeflow-core/src/workgraph/lifecycle.rs", "landing_paths", "from_utf8_lossy", 1, "file or blob content, a format contract and not a name"),
    ("codeflow-core/src/workgraph/lifecycle.rs", "reopened_in_range", "from_utf8_lossy", 1, "file or blob content, a format contract and not a name"),
    ("codeflow-core/src/workgraph/lifecycle.rs", "shipped_in_history", "from_utf8_lossy", 2, "an error message and record content; the paths in the log are read as exact bytes"),
    ("codeflow-core/src/workgraph/light_paths.rs", "git", "from_utf8_lossy", 2, "stdout compared with the ASCII remote name `origin` and an error message"),
    ("codeflow-core/src/workgraph/readiness.rs", "git", "from_utf8_lossy", 2, "object ids and fetch messages, and an error message"),
    ("codeflow-core/src/workgraph/readiness.rs", "git_bytes", "from_utf8_lossy", 1, "a git or tool error message shown to a person, never compared"),
    ("codeflow-core/src/workgraph/release_line.rs", "config_at", "from_utf8_lossy", 1, "file or blob content, a format contract and not a name"),
    ("codeflow-core/src/workgraph/release_line.rs", "record_of", "from_utf8_lossy", 1, "file or blob content, a format contract and not a name"),
    ("codeflow-present/src/browser.rs", "windows_output_text", "from_utf8_lossy", 1, "Windows only: PowerShell output arrives as U+FFFD for an invalid unit, so refuse_ambiguous_identity_text stops the one case where the lossy text could equal the expected profile path, and the instance argument is a random UUID, exact either way"),
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
    whitespace: bool,
    /// The file uses git2, so a zero-argument `name()` may be its text.
    uses_git2: bool,
    context: Vec<String>,
    /// (item, call) to the lines it was seen at.
    found: BTreeMap<(String, String), Vec<usize>>,
}

impl Sites {
    fn note(&mut self, span: proc_macro2::Span, call: &str) {
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
        if self.whitespace {
            return UNICODE_WHITESPACE.contains(&name);
        }
        LOSSY.contains(&name)
            || (GIT2_TEXT.contains(&name) && arguments == 0)
            || (name == "name" && arguments == 0 && self.uses_git2)
    }

    /// The idents of a macro's tokens, so `format!("{}", f(x))` is read too.
    fn scan_tokens(&mut self, tokens: proc_macro2::TokenStream) {
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
                        && (after_dot || LOSSY.contains(&name.as_str()) || self.whitespace)
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
                syn::visit::visit_item_fn(self, function);
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
            syn::visit::visit_impl_item_fn(self, function);
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
            syn::visit::visit_trait_item_fn(self, function);
            self.context.pop();
        } else {
            syn::visit::visit_trait_item(self, item);
        }
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        let name = call.method.unraw().to_string();
        if self.flagged(&name, call.args.len()) {
            self.note(call.method.span(), &name);
        }
        syn::visit::visit_expr_method_call(self, call);
    }

    fn visit_path(&mut self, path: &'ast syn::Path) {
        // `String::from_utf8_lossy(x)` and `map(String::from_utf8_lossy)`.
        if let Some(last) = path.segments.last() {
            let name = last.ident.unraw().to_string();
            if (self.whitespace && UNICODE_WHITESPACE.contains(&name.as_str()))
                || (!self.whitespace && LOSSY.contains(&name.as_str()))
            {
                self.note(last.ident.span(), &name);
            }
        }
        syn::visit::visit_path(self, path);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        self.scan_tokens(mac.tokens.clone());
        syn::visit::visit_macro(self, mac);
    }
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
        assert!(!reason.is_empty(), "{file} {item} {call}: give the reason");
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
    ("crates/codeflow-core/src/security/dangerous.rs", "command_tokens", "is_whitespace", 1, "Multi-language catastrophe fallback also reads raw PowerShell, whose grammar includes Unicode separators. POSIX deletion has its own exact reader; this fallback does not compare protected ref names."),
    ("crates/codeflow-core/src/hooks/conflict_markers.rs", "marker_sizes", "trim", 1, "Diagnostic text only; never parsed as a command or identity."),
    ("crates/codeflow-core/src/hooks/delegate_turn.rs", "signal_tmux", "trim", 1, "Diagnostic text only; never parsed as a command or identity."),
    ("crates/codeflow-core/src/hooks/git_guard.rs", "read_alias", "trim", 1, "Diagnostic text only; never parsed as a command or identity."),
    ("crates/codeflow-core/src/hooks/git_guard.rs", "read_branch_name", "trim", 1, "Diagnostic text only; never parsed as a command or identity."),
    ("crates/codeflow-core/src/hooks/git_hook.rs", "commit_msg_from", "trim", 1, "Commit-message prose validation, whose whitespace rules are independent of shell token and ref identity."),
    ("crates/codeflow-core/src/hooks/git_hook.rs", "commit_msg_with_files", "trim", 1, "Commit-message prose validation, whose whitespace rules are independent of shell token and ref identity."),
    ("crates/codeflow-core/src/hooks/git_hook.rs", "policy_character_violation", "trim", 1, "Commit-message prose validation, whose whitespace rules are independent of shell token and ref identity."),
    ("crates/codeflow-core/src/hooks/orient.rs", "generate", "trim", 1, "Compares the fixed ASCII hooks-version marker for orientation; replacement or whitespace cannot conceal a command."),
    ("crates/codeflow-core/src/hooks/orient.rs", "product_one_liner", "trim", 6, "Human-readable product description for orientation only."),
    ("crates/codeflow-core/src/hooks/policy_schema.rs", "validate_profiles", "trim", 6, "Rejects empty schema fields and wildcard-only branch patterns; Unicode trimming only adds a rejection and does not rewrite the stored value."),
    ("crates/codeflow-core/src/hooks/policy_schema.rs", "validate_retry_entries", "trim", 1, "Rejects empty retry-provider labels; Unicode trimming only adds a rejection and does not rewrite the stored value."),
    ("crates/codeflow-core/src/hooks/scan.rs", "scan_diff", "trim", 1, "The path labels secret findings for display; secret detection inspects the unchanged added content."),
    ("crates/codeflow-core/src/hooks/session_summary.rs", "record", "trim", 1, "Session-summary JSON framing, not execution or policy input."),
    ("crates/codeflow-core/src/hooks/source_identity.rs", "revision", "trim", 1, "Rejects an empty archive revision label for build metadata; retains the original nonempty value."),
    ("crates/codeflow-core/src/hooks/standards.rs", "check_breaking_footer", "trim_start", 1, "Commit-message prose validation, whose whitespace rules are independent of shell token and ref identity."),
    ("crates/codeflow-core/src/hooks/standards.rs", "check_commit_body", "trim", 1, "Commit-message prose validation, whose whitespace rules are independent of shell token and ref identity."),
    ("crates/codeflow-core/src/hooks/standards.rs", "check_commit_body", "trim_end", 1, "Commit-message prose validation, whose whitespace rules are independent of shell token and ref identity."),
    ("crates/codeflow-core/src/hooks/standards.rs", "check_commit_format", "trim_end", 1, "Commit-message prose validation, whose whitespace rules are independent of shell token and ref identity."),
    ("crates/codeflow-core/src/hooks/standards.rs", "check_commit_ticket", "trim_end", 1, "Commit-message prose validation, whose whitespace rules are independent of shell token and ref identity."),
    ("crates/codeflow-core/src/hooks/standards.rs", "check_required_footers", "trim_end", 1, "Commit-message prose validation, whose whitespace rules are independent of shell token and ref identity."),
    ("crates/codeflow-core/src/hooks/standards.rs", "check_subject_separator", "trim", 2, "Commit-message prose validation, whose whitespace rules are independent of shell token and ref identity."),
    ("crates/codeflow-core/src/hooks/standards.rs", "check_subject_separator", "trim_end", 1, "Commit-message prose validation, whose whitespace rules are independent of shell token and ref identity."),
    ("crates/codeflow-core/src/hooks/standards.rs", "is_breaking_footer_start", "trim_start", 1, "Commit-message prose validation, whose whitespace rules are independent of shell token and ref identity."),
    ("crates/codeflow-core/src/hooks/standards.rs", "trailer_kv", "trim_end", 1, "Commit-message prose validation, whose whitespace rules are independent of shell token and ref identity."),
];

#[test]
fn guard_whitespace_uses_explicit_separator_rules() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    for dir in [
        "crates/codeflow-core/src/hooks",
        "crates/codeflow-core/src/security",
        "crates/codeflow-cli/src/cmd",
    ] {
        rust_files(&root.join(dir), &mut files);
    }
    let mut actual = BTreeMap::new();
    for file in files {
        let name = relative(&root, &file);
        if name.contains("codeflow-cli")
            && !file
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("hook")
        {
            continue;
        }
        for ((item, call), lines) in sites_in_mode(&std::fs::read_to_string(&file).unwrap(), true) {
            println!("{name} {item} {call} {} {lines:?}", lines.len());
            actual.insert((name.clone(), item, call), lines.len());
        }
    }
    let mut allowed = BTreeMap::new();
    for (file, item, call, count, reason) in WHITESPACE_EXCEPTIONS {
        assert!(!reason.is_empty());
        assert!(allowed
            .insert(
                (file.to_string(), item.to_string(), call.to_string()),
                *count
            )
            .is_none());
    }
    assert_eq!(actual, allowed, "Unicode whitespace in a guard needs a reviewed reason; shell and git names keep non-separator characters");
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
