//! The display boundary of `codeflow update` and `codeflow update --pin`
//! (TSK-1029): what this scan enforces, and what it does not.
//!
//! The type carries the guarantee. Every line those commands print or
//! export is a `scaffold::Shown`; its text is private, it has no conversion
//! from a string, and its constructors take compiled-in text
//! (`&'static str`), a typed value (a count, path, parse place, I/O
//! category, digest, version or date) or a slice of a `Shipped` text, which
//! only the asset reader or compiled-in text makes. `shown!` takes a
//! literal format and `Shown` arguments. The sinks (`Report`,
//! `ScaffoldError`, the `--diff` sections, the pin report) hold only
//! `Shown`, and their `Debug` writes the same text.
//!
//! The type cannot see three things, and this scan checks them in the
//! sources of `codeflow-core` and `codeflow-cli`:
//!
//! 1. A `&'static str` made at run time (`leak`) would pass as compiled-in
//!    text. No `leak` call is allowed outside [`LEAKS`].
//! 2. `Shown::path` and `Shown::iso_date` take runtime text, so each call is
//!    listed in [`RUNTIME_TEXT`] with what feeds it. Calls are found through
//!    `use ... as` and `type` aliases of `Shown`, inside macro arguments
//!    that parse as expressions, and in production-only items such as
//!    `#[cfg(not(test))]`; a macro whose arguments mention `Shown` but do
//!    not parse, and a `macro_rules!` whose body names `Shown` outside
//!    `shown.rs`, fail the scan.
//! 3. The CLI prints with `println!` and friends, which take any text. From
//!    the functions `update` and `update --pin` run ([`ROOTS`]), every CLI
//!    function they call by path is followed, and none may print except the
//!    writers that take a `Shown` ([`WRITERS`]).
//!
//! Limits, stated plainly: the scan reads source, not expanded code or
//! types. It follows calls by function name within the CLI crate, not
//! method calls, trait dispatch, closures stored and called later, or
//! calls into `codeflow-core` (whose update path returns `Shown` by type).
//! It does not scan `codeflow-present`: `update` calls only
//! `provision_state_root` there and prints a fixed line on its error. It
//! proves the listed properties of the sources; it is not a proof that
//! every route to a terminal crosses the boundary. The canary sweep
//! (`codeflow-cli/tests/update_canary_sweep.rs`) is the behavioural check.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use syn::visit::Visit;

/// Each call of a `Shown` constructor that takes runtime text: file,
/// enclosing function, constructor, count and what feeds it.
const RUNTIME_TEXT: &[(&str, &str, &str, usize, &str)] = &[
    (
        "crates/codeflow-cli/src/cmd/mod.rs",
        "touch_registry_best_effort",
        "path",
        1,
        "the folder the registry could not inspect",
    ),
    (
        "crates/codeflow-core/src/remedy.rs",
        "shown_for",
        "path",
        1,
        "the path a catalogued remedy names: the registry file or the folder it could not inspect",
    ),
    (
        "crates/codeflow-cli/src/prompts.rs",
        "decide_pr_template",
        "path",
        1,
        "the kept PR template's path, from the scaffold manifest",
    ),
    (
        "crates/codeflow-core/src/hooks/policy.rs",
        "load_shown",
        "path",
        2,
        "the project's policy file path",
    ),
    (
        "crates/codeflow-core/src/root_checkout.rs",
        "workspace_hint_shown",
        "path",
        2,
        "the repository folder and the nested repositories' paths",
    ),
    (
        "crates/codeflow-core/src/registry.rs",
        "touch_registry",
        "path",
        4,
        "the project's .codeflow folder, the repository root and the registry file",
    ),
    (
        "crates/codeflow-core/src/scaffold/assets.rs",
        "read_text",
        "path",
        2,
        "an embedded asset path",
    ),
    (
        "crates/codeflow-core/src/scaffold/detect.rs",
        "report_note",
        "path",
        1,
        "the git hooks folder",
    ),
    (
        "crates/codeflow-core/src/scaffold/diff_redact.rs",
        "section",
        "path",
        2,
        "the destination of a managed file",
    ),
    (
        "crates/codeflow-core/src/scaffold/init.rs",
        "init_writes",
        "path",
        2,
        "the kept PR template file and the configured git hooks folder",
    ),
    (
        "crates/codeflow-core/src/scaffold/init.rs",
        "render_entry",
        "path",
        3,
        "an embedded asset path and the destination it renders to",
    ),
    (
        "crates/codeflow-core/src/scaffold/manifest.rs",
        "validate",
        "path",
        4,
        "a destination the embedded manifest names",
    ),
    (
        "crates/codeflow-core/src/scaffold/pr_template.rs",
        "find_kept",
        "path",
        1,
        "the kept PR template file",
    ),
    (
        "crates/codeflow-core/src/scaffold/pr_template.rs",
        "describe",
        "path",
        1,
        "the kept PR template file",
    ),
    (
        "crates/codeflow-core/src/scaffold/pr_template.rs",
        "edit_error",
        "path",
        1,
        "the policy file",
    ),
    (
        "crates/codeflow-core/src/scaffold/pr_template.rs",
        "read_policy",
        "path",
        3,
        "the policy file",
    ),
    (
        "crates/codeflow-core/src/scaffold/pr_template.rs",
        "record_decision",
        "path",
        3,
        "the policy file and the kept PR template file",
    ),
    (
        "crates/codeflow-core/src/scaffold/release_pin.rs",
        "pin_release",
        "path",
        1,
        "the project state file",
    ),
    (
        "crates/codeflow-core/src/scaffold/release_pin.rs",
        "shown",
        "path",
        1,
        "the project state file",
    ),
    (
        "crates/codeflow-core/src/scaffold/report.rs",
        "file",
        "path",
        1,
        "a report line's destination",
    ),
    (
        "crates/codeflow-core/src/scaffold/report.rs",
        "file_with_notes",
        "path",
        1,
        "a report line's destination",
    ),
    (
        "crates/codeflow-core/src/scaffold/update/pending.rs",
        "invalid",
        "path",
        1,
        "a managed destination",
    ),
    (
        "crates/codeflow-core/src/scaffold/update/pending.rs",
        "read",
        "path",
        3,
        "a managed destination and its pending record",
    ),
    (
        "crates/codeflow-core/src/scaffold/update.rs",
        "decide",
        "path",
        1,
        "a symlink under the project root",
    ),
    (
        "crates/codeflow-core/src/scaffold/update.rs",
        "skip_note",
        "path",
        1,
        "the kept PR template file",
    ),
    (
        "crates/codeflow-core/src/scaffold/update.rs",
        "update_writes",
        "path",
        1,
        "the --diff export file",
    ),
    (
        "crates/codeflow-core/src/scaffold/update.rs",
        "warn_steps_before_secret_scan",
        "path",
        1,
        "the shipped workflow or its proposal",
    ),
    (
        "crates/codeflow-core/src/scaffold/update.rs",
        "record_work_records_baseline",
        "path",
        3,
        "the project state file",
    ),
    (
        "crates/codeflow-core/src/scaffold/update.rs",
        "record_outcome_marker",
        "path",
        2,
        "the project state file",
    ),
    (
        "crates/codeflow-core/src/scaffold/state.rs",
        "load",
        "path",
        9,
        "the project state file",
    ),
    (
        "crates/codeflow-core/src/scaffold/state.rs",
        "store",
        "path",
        3,
        "the project state file",
    ),
    (
        "crates/codeflow-core/src/scaffold/state.rs",
        "write_defaults",
        "path",
        4,
        "the project state file",
    ),
    (
        "crates/codeflow-core/src/scaffold/update.rs",
        "write_dest",
        "path",
        1,
        "a manifest destination",
    ),
    (
        "crates/codeflow-core/src/scaffold/update.rs",
        "update_entry",
        "path",
        2,
        "a managed file's .new proposal",
    ),
    (
        "crates/codeflow-core/src/scaffold/version.rs",
        "check_write_version",
        "path",
        1,
        "the project state file",
    ),
    (
        "crates/codeflow-core/src/scaffold/pr_template.rs",
        "record_decision",
        "iso_date",
        1,
        "the date the binary's clock gives",
    ),
];

/// Calls of `Shown::path` in the docs portal's transaction code, counted
/// for the file as a whole: every one names a portal file, folder or
/// journal path under the project root.
const PORTAL: (&str, usize) = ("crates/codeflow-core/src/scaffold/portal.rs", 73);

/// The constructors that take runtime text.
const RUNTIME: &[&str] = &["path", "iso_date"];

/// `leak` calls allowed in production code, with why none reaches a
/// `Shown`: file, enclosing function and reason.
const LEAKS: &[(&str, &str, &str)] = &[(
    "crates/codeflow-core/src/security/prose.rs",
    "",
    "a guard's word list for its own matching; the security module builds no Shown",
)];

/// The CLI functions `update` and `update --pin` start from.
const ROOTS: &[&str] = &[
    "update",
    "pin_release",
    "touch_registry_best_effort",
    "provision_state_root_or_warn",
];

/// The CLI functions that may print: each takes a `Shown` or a report built
/// from them.
const WRITERS: &[&str] = &["say", "warn", "prompt", "say_report"];

/// The macros that print.
const PRINTERS: &[&str] = &[
    "println",
    "print",
    "eprintln",
    "eprint",
    "write",
    "writeln",
    "print_block",
    "println_block",
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().collect();
    entries.sort_by_key(std::fs::DirEntry::path);
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// Whether a `cfg` predicate holds only in test builds: `test`, or an
/// `all(...)` with a bare `test` among its parts. `not(test)` is production.
fn test_only_cfg(tokens: &str) -> bool {
    let tokens: String = tokens.chars().filter(|c| !c.is_whitespace()).collect();
    tokens == "test"
        || tokens
            .strip_prefix("all(")
            .and_then(|rest| rest.strip_suffix(')'))
            .is_some_and(|parts| parts.split(',').any(|part| part == "test"))
}

fn is_test_item(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("cfg")
            && attr
                .meta
                .require_list()
                .is_ok_and(|list| test_only_cfg(&list.tokens.to_string()))
    })
}

/// What one source file holds.
#[derive(Default, Debug)]
struct Found {
    /// (function, constructor) -> count of runtime-text constructor calls.
    runtime: BTreeMap<(String, String), usize>,
    /// Functions with a `leak` call.
    leaks: Vec<String>,
    /// Macro invocations that mention `Shown` but whose arguments do not
    /// parse as expressions, and `macro_rules!` bodies that name `Shown`.
    opaque_macros: Vec<String>,
    /// Function name -> the paths it calls, as written (`helper`,
    /// `cmd::warn`, `crate::prompts::ask`). Paths through a type
    /// (`Report::new`, `Self::x`) are left out: they are methods.
    calls: BTreeMap<String, BTreeSet<Vec<String>>>,
    /// Functions that print.
    prints: BTreeSet<String>,
    /// `mod name;` declarations that are test-only.
    test_modules: Vec<String>,
}

struct Scan {
    item: Vec<String>,
    /// `Shown` and its aliases in this file.
    owners: BTreeSet<String>,
    found: Found,
}

impl Scan {
    fn current(&self) -> String {
        self.item.last().cloned().unwrap_or_default()
    }

    fn mentions_owner(&self, tokens: &proc_macro2::TokenStream) -> bool {
        let text = tokens.to_string();
        self.owners.iter().any(|owner| {
            text.split(|c: char| !c.is_alphanumeric() && c != '_')
                .any(|word| word == owner)
        })
    }

    fn path(&mut self, path: &syn::Path) {
        let segments: Vec<String> = path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect();
        if let [.., owner, name] = segments.as_slice() {
            if self.owners.contains(owner) && RUNTIME.contains(&name.as_str()) {
                *self
                    .found
                    .runtime
                    .entry((self.current(), name.clone()))
                    .or_default() += 1;
            }
        }
        if segments.last().is_some_and(|name| name == "leak") {
            let item = self.current();
            self.found.leaks.push(item);
        }
    }

    fn macro_args(&mut self, mac: &syn::Macro) {
        use syn::parse::Parser;
        let name = mac
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string())
            .unwrap_or_default();
        if PRINTERS.contains(&name.as_str()) {
            let item = self.current();
            self.found.prints.insert(item);
        }
        let parser = syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated;
        match parser.parse2(mac.tokens.clone()) {
            Ok(args) => {
                for arg in &args {
                    self.visit_expr(arg);
                }
            }
            Err(_) if self.mentions_owner(&mac.tokens) => {
                let item = self.current();
                self.found
                    .opaque_macros
                    .push(format!("{item}: {name}! with unparsed arguments"));
            }
            Err(_) => {}
        }
    }
}

impl<'ast> Visit<'ast> for Scan {
    fn visit_item_use(&mut self, node: &'ast syn::ItemUse) {
        // `use ...::Shown as Alias;` makes `Alias` an owner.
        fn walk(tree: &syn::UseTree, owners: &mut BTreeSet<String>) {
            match tree {
                syn::UseTree::Path(path) => walk(&path.tree, owners),
                syn::UseTree::Rename(rename) if rename.ident == "Shown" => {
                    owners.insert(rename.rename.to_string());
                }
                syn::UseTree::Group(group) => {
                    group.items.iter().for_each(|item| walk(item, owners));
                }
                _ => {}
            }
        }
        walk(&node.tree, &mut self.owners);
    }

    fn visit_item_type(&mut self, node: &'ast syn::ItemType) {
        // `type Alias = Shown;` makes `Alias` an owner.
        if let syn::Type::Path(path) = &*node.ty {
            if path
                .path
                .segments
                .last()
                .is_some_and(|s| s.ident == "Shown")
            {
                self.owners.insert(node.ident.to_string());
            }
        }
    }

    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        if is_test_item(&node.attrs) {
            return;
        }
        self.item.push(node.sig.ident.to_string());
        self.found.calls.entry(self.current()).or_default();
        syn::visit::visit_item_fn(self, node);
        self.item.pop();
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        if is_test_item(&node.attrs) {
            return;
        }
        self.item.push(node.sig.ident.to_string());
        self.found.calls.entry(self.current()).or_default();
        syn::visit::visit_impl_item_fn(self, node);
        self.item.pop();
    }

    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        if is_test_item(&node.attrs) {
            if node.content.is_none() {
                self.found.test_modules.push(node.ident.to_string());
            }
            return;
        }
        syn::visit::visit_item_mod(self, node);
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        if is_test_item(&node.attrs) {
            return;
        }
        syn::visit::visit_item_impl(self, node);
    }

    fn visit_item_macro(&mut self, node: &'ast syn::ItemMacro) {
        if node.ident.is_some() {
            // A `macro_rules!` definition: its body is a template, so a
            // `Shown` inside it is not seen where the macro is used.
            if self.mentions_owner(&node.mac.tokens) {
                let name = node
                    .ident
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_default();
                self.found
                    .opaque_macros
                    .push(format!("macro_rules! {name} names Shown in its body"));
            }
            return;
        }
        self.visit_macro(&node.mac);
    }

    fn visit_expr_path(&mut self, node: &'ast syn::ExprPath) {
        self.path(&node.path);
        syn::visit::visit_expr_path(self, node);
    }

    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path) = &*node.func {
            let segments: Vec<String> = path
                .path
                .segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect();
            let through_type = segments[..segments.len() - 1]
                .iter()
                .any(|segment| segment.starts_with(char::is_uppercase));
            if !through_type {
                let item = self.current();
                self.found.calls.entry(item).or_default().insert(segments);
            }
        }
        syn::visit::visit_expr_call(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        if node.method == "leak" {
            let item = self.current();
            self.found.leaks.push(item);
        }
        syn::visit::visit_expr_method_call(self, node);
    }

    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        self.macro_args(node);
        syn::visit::visit_macro(self, node);
    }
}

/// Scans one file's text.
fn scan_source(text: &str) -> Found {
    let file = syn::parse_file(text).expect("the source parses");
    let mut scan = Scan {
        item: Vec::new(),
        owners: BTreeSet::from(["Shown".to_string()]),
        found: Found::default(),
    };
    // Aliases are declared before use in practice, but collect them first
    // so the order does not matter.
    for item in &file.items {
        if let syn::Item::Use(node) = item {
            scan.visit_item_use(node);
        }
        if let syn::Item::Type(node) = item {
            scan.visit_item_type(node);
        }
    }
    scan.visit_file(&file);
    scan.found
}

/// The production source files of a crate, by path relative to the
/// repository root: test-only modules (`#[cfg(test)] mod x;`) are left out.
fn production_sources(crate_dir: &str) -> Vec<(String, Found)> {
    let root = root();
    let mut files = Vec::new();
    rust_files(&root.join(crate_dir), &mut files);
    let mut scanned: BTreeMap<PathBuf, Found> = files
        .into_iter()
        .map(|path| {
            let found = scan_source(&std::fs::read_to_string(&path).unwrap());
            (path, found)
        })
        .collect();
    let mut test_only = BTreeSet::new();
    for (path, found) in &scanned {
        let stem = path.file_stem().unwrap().to_string_lossy();
        let dir = if ["mod", "lib", "main"].contains(&stem.as_ref()) {
            path.parent().unwrap().to_path_buf()
        } else {
            path.with_extension("")
        };
        for module in &found.test_modules {
            test_only.insert(dir.join(format!("{module}.rs")));
            test_only.insert(dir.join(module).join("mod.rs"));
        }
    }
    scanned.retain(|path, _| !test_only.contains(path));
    scanned
        .into_iter()
        .map(|(path, found)| {
            let relative = path
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            (relative, found)
        })
        .collect()
}

/// The CLI functions reachable from `roots` by calls through a path, with
/// those that print but are not writers. A call is followed when its path is
/// a bare name or starts at `crate`, `self`, `super` or one of the CLI's own
/// `modules`; a bare name imported from `codeflow-core` that shares a CLI
/// function's name is followed too, which errs toward a finding.
fn printers_reached(
    files: &[(String, Found)],
    modules: &BTreeSet<String>,
    roots: &[&str],
) -> Vec<String> {
    let local = |path: &Vec<String>| {
        path.len() == 1
            || ["crate", "self", "super"].contains(&path[0].as_str())
            || modules.contains(&path[0])
    };
    let mut calls: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    let mut prints: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (file, found) in files {
        for (function, callees) in &found.calls {
            calls.entry(function.as_str()).or_default().extend(
                callees
                    .iter()
                    .filter(|path| local(path))
                    .filter_map(|path| path.last().map(String::as_str)),
            );
        }
        for function in &found.prints {
            prints
                .entry(function.as_str())
                .or_default()
                .push(file.as_str());
        }
    }
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut queue: Vec<&str> = roots.to_vec();
    while let Some(function) = queue.pop() {
        if WRITERS.contains(&function) || !seen.insert(function) {
            continue;
        }
        if let Some(callees) = calls.get(function) {
            queue.extend(callees.iter().filter(|callee| calls.contains_key(*callee)));
        }
    }
    seen.into_iter()
        .filter_map(|function| {
            prints
                .get(function)
                .map(|files| format!("{function} (in {}) prints", files.join(", ")))
        })
        .collect()
}

#[test]
fn runtime_text_reaches_the_boundary_only_where_listed() {
    let mut problems = Vec::new();
    let mut expected: BTreeMap<(String, String, String), usize> = BTreeMap::new();
    for (file, item, constructor, count, reason) in RUNTIME_TEXT {
        assert!(!reason.is_empty(), "{file} {item}: give a reason");
        *expected
            .entry(((*file).into(), (*item).into(), (*constructor).into()))
            .or_default() += count;
    }
    let files: Vec<(String, Found)> = ["crates/codeflow-core/src", "crates/codeflow-cli/src"]
        .into_iter()
        .flat_map(production_sources)
        .collect();
    for (relative, found) in &files {
        if relative.ends_with("scaffold/shown.rs") {
            continue;
        }
        for problem in &found.opaque_macros {
            problems.push(format!("{relative}: {problem}"));
        }
        for item in &found.leaks {
            if !LEAKS.iter().any(|(file, _, _)| file == relative) {
                problems.push(format!(
                    "{relative} {item}: a leak makes a &'static str at run time, which \
                     Shown::fixed would take as compiled-in text"
                ));
            }
        }
        if relative == PORTAL.0 {
            let paths: usize = found.runtime.values().sum();
            if paths != PORTAL.1 {
                problems.push(format!(
                    "{relative}: {paths} runtime-text calls, {} listed; review each new one",
                    PORTAL.1
                ));
            }
            continue;
        }
        for ((item, constructor), count) in &found.runtime {
            let key = (relative.clone(), item.clone(), constructor.clone());
            match expected.remove(&key) {
                Some(listed) if listed == *count => {}
                Some(listed) => problems.push(format!(
                    "{relative} {item}: {count} Shown::{constructor}, {listed} listed"
                )),
                None => problems.push(format!(
                    "{relative} {item}: {count} unlisted Shown::{constructor}; list it with \
                     what feeds it, or build the text from fixed fragments"
                )),
            }
        }
    }
    for ((file, item, constructor), count) in expected {
        problems.push(format!(
            "{file} {item}: {count} Shown::{constructor} listed but not found; remove the entry"
        ));
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn update_and_pin_print_only_through_the_shown_writers() {
    let files = production_sources("crates/codeflow-cli/src");
    let mut sources = Vec::new();
    rust_files(&root().join("crates/codeflow-cli/src"), &mut sources);
    let modules: BTreeSet<String> = sources
        .iter()
        .flat_map(|path| {
            let relative = path
                .strip_prefix(root().join("crates/codeflow-cli/src"))
                .unwrap();
            relative
                .with_extension("")
                .components()
                .map(|part| part.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        })
        .filter(|name| !["main", "mod", "lib"].contains(&name.as_str()))
        .collect();
    let reached = printers_reached(&files, &modules, ROOTS);
    assert!(
        reached.is_empty(),
        "a function update or update --pin calls prints outside the Shown writers:\n{}",
        reached.join("\n")
    );
    // The roots exist, so a rename cannot empty the check.
    for root in ROOTS.iter().chain(WRITERS) {
        assert!(
            files
                .iter()
                .any(|(_, found)| found.calls.contains_key(*root)),
            "no CLI function named {root}"
        );
    }
}

#[test]
fn shown_has_no_string_conversion_and_a_private_text() {
    let text =
        std::fs::read_to_string(root().join("crates/codeflow-core/src/scaffold/shown.rs")).unwrap();
    assert!(
        text.contains("pub struct Shown(String);"),
        "the text stays private"
    );
    for conversion in [
        "From<String> for Shown",
        "From<&str> for Shown",
        "From<&String> for Shown",
        "Arguments<'",
    ] {
        assert!(
            !text.contains(conversion),
            "shown.rs must not take {conversion}"
        );
    }
    let assets =
        std::fs::read_to_string(root().join("crates/codeflow-core/src/scaffold/assets.rs"))
            .unwrap();
    assert!(assets.contains("pub struct AssetText(String);"));
    assert!(assets.contains("pub struct Shipped<'a>(&'a str);"));
}

#[test]
fn the_sinks_hold_only_shown_text() {
    let read = |relative: &str| std::fs::read_to_string(root().join(relative)).unwrap();
    let report = read("crates/codeflow-core/src/scaffold/report.rs");
    for field in [
        "pub dest: Shown,",
        "pub notes: Vec<Shown>,",
        "pub title: Shown,",
        "pub warnings: Vec<Shown>,",
    ] {
        assert!(report.contains(field), "report.rs: {field}");
    }
    let errors = read("crates/codeflow-core/src/scaffold/mod.rs");
    for variant in [
        "ManifestMissing(Shown),",
        "ManifestInvalid(Shown),",
        "InvalidState { what: Shown, detail: Shown },",
        "Git(Shown),",
        "Json(ParsePlace),",
        "kind: Shown,",
    ] {
        assert!(errors.contains(variant), "ScaffoldError: {variant}");
    }
    let pin = read("crates/codeflow-core/src/scaffold/release_pin.rs");
    for text in [
        "pub digests: Vec<(&'static str, Shown)>,",
        "Unreadable(Shown),",
        "pub fn fetch_with_curl(url: &str) -> Result<Vec<u8>, Shown>",
        ") -> Result<PinReport, Shown> {",
        "pub fn release_url() -> Result<String, Shown>",
    ] {
        assert!(pin.contains(text), "release_pin.rs: {text}");
    }
}

#[test]
fn the_settings_presets_are_not_templated() {
    // `settings_merge` compiles the presets in as the proof for the names it
    // shows; that holds while the preset update writes is that text.
    let root = root();
    let manifest =
        std::fs::read_to_string(root.join("assets/base/scaffold-manifest.toml")).unwrap();
    let manifest: toml::Value = toml::from_str(&manifest).unwrap();
    let mut presets = 0;
    for entry in manifest["entry"].as_array().unwrap() {
        let src = entry["src"].as_str().unwrap();
        if src.starts_with("settings/") {
            presets += 1;
            assert!(
                !entry
                    .get("template")
                    .and_then(toml::Value::as_bool)
                    .unwrap_or(false),
                "{src} is templated"
            );
            let text = std::fs::read_to_string(root.join("assets/base").join(src)).unwrap();
            assert!(!text.contains("{{"), "{src} holds a template placeholder");
        }
    }
    assert!(presets > 0, "no settings presets found");
}

// Adversarial fixtures: each is a source the scan must flag.

#[test]
fn an_alias_of_shown_is_seen() {
    let found = scan_source(
        "use crate::scaffold::Shown as DisplayText;\n\
         type Line = crate::scaffold::Shown;\n\
         fn one(value: &str) -> DisplayText { DisplayText::path(value) }\n\
         fn two(value: &str) -> Line { Line::iso_date(value).unwrap() }\n",
    );
    assert_eq!(found.runtime.get(&("one".into(), "path".into())), Some(&1));
    assert_eq!(
        found.runtime.get(&("two".into(), "iso_date".into())),
        Some(&1)
    );
}

#[test]
fn production_only_code_is_scanned() {
    let found = scan_source(
        "#[cfg(not(test))]\nfn shipped_build(value: &str) { let _ = Shown::path(value); }\n\
         #[cfg(all(test, unix))]\nfn test_only(value: &str) { let _ = Shown::path(value); }\n\
         #[cfg(test)]\nmod tests;\n",
    );
    assert_eq!(
        found.runtime.get(&("shipped_build".into(), "path".into())),
        Some(&1)
    );
    assert!(!found
        .runtime
        .contains_key(&("test_only".into(), "path".into())));
    assert_eq!(found.test_modules, ["tests"]);
}

#[test]
fn a_macro_that_hides_shown_is_flagged() {
    let found = scan_source(
        "macro_rules! wrap { ($e:expr) => { Shown::path($e) }; }\n\
         fn one(value: &str) { let _ = vec![Shown::path(value)]; }\n\
         fn two(value: &str) { other!(Shown => value); }\n",
    );
    assert_eq!(found.runtime.get(&("one".into(), "path".into())), Some(&1));
    assert_eq!(found.opaque_macros.len(), 2, "{:?}", found.opaque_macros);
}

#[test]
fn a_leak_is_found() {
    let found = scan_source(
        "fn one(text: String) -> Shown { Shown::fixed(text.leak()) }\n\
         fn two(text: String) -> Shown { Shown::fixed(Box::leak(text.into_boxed_str())) }\n",
    );
    assert_eq!(found.leaks, ["one", "two"]);
}

#[test]
fn an_indirect_writer_is_found() {
    let files = vec![(
        "main.rs".to_string(),
        scan_source(
            "fn update() { helper(); say(&line); Report::new(); core::template(); }\n\
             fn helper() { deeper(); }\n\
             fn deeper() { eprintln!(\"{}\", value); }\n\
             fn say(line: &Shown) { println!(\"{line}\"); }\n\
             fn unrelated() { println!(\"x\"); }\n\
             fn new() { println!(\"a type's new is a method\"); }\n\
             fn template() { println!(\"a core function's namesake\"); }\n",
        ),
    )];
    assert_eq!(
        printers_reached(&files, &BTreeSet::new(), &["update"]),
        ["deeper (in main.rs) prints"]
    );
}
