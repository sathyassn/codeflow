//! The display boundary of `codeflow update` and `codeflow update --pin`
//! (TSK-1029).
//!
//! Every report line, note, warning and error those commands print or
//! export is a `scaffold::Shown`, built only from a closed set of
//! fragments: text compiled into the binary, a count, a path, a parse
//! location, an I/O category, a version under the version rule, a hash the
//! binary computed, or text an embedded asset holds. The compiler enforces
//! most of it: `Shown` has no conversion from a string and its text is
//! private, and the sinks (`Report`, `ScaffoldError`, `PinReport`, the
//! `--diff` sections and the CLI's update printers) hold only `Shown`.
//!
//! This test checks what the type cannot:
//!
//! - a `shown!` format string never captures a variable (`{name}`), which
//!   would format a `String` past the type check;
//! - the constructors that take runtime text, `Shown::path`,
//!   `Shown::shipped`, `Shown::defined` and `Shown::iso_date`, are called
//!   only where [`RUNTIME_TEXT`] lists them, each with what feeds it, so a
//!   new call fails until it is reviewed;
//! - the macro's helpers are called only by the macro, and no source leaks
//!   a `String` into a `&'static str` for `Shown::fixed`;
//! - the sink types hold `Shown`, and the CLI's update and pin functions
//!   print only through the boundary;
//! - the settings presets, whose text `settings_merge` uses as the proof
//!   for names, are not templated.
//!
//! Limits: the scan parses sources with `syn` and reads the arguments of a
//! macro only when they parse as expressions; it does not follow data flow,
//! so each listed call's reason is the review.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use syn::visit::Visit;

/// Each call of a constructor that takes runtime text: file, enclosing
/// item, constructor, count and what feeds it.
const RUNTIME_TEXT: &[(&str, &str, &str, usize, &str)] = &[
    // Paths: a file or folder the command reads or writes.
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
    // Text an embedded asset holds.
    (
        "crates/codeflow-core/src/scaffold/diff_redact.rs",
        "key",
        "shipped",
        1,
        "a key, checked against the embedded asset's bytes",
    ),
    (
        "crates/codeflow-core/src/scaffold/diff_redact.rs",
        "embedded_json",
        "shipped",
        2,
        "a number or string, checked against the embedded asset's bytes",
    ),
    (
        "crates/codeflow-core/src/scaffold/diff_redact.rs",
        "text",
        "shipped",
        1,
        "a line, checked against the embedded asset's bytes",
    ),
    (
        "crates/codeflow-core/src/scaffold/settings_merge.rs",
        "shown_entry",
        "shipped",
        1,
        "an entry, checked against an earlier preset embedded in the binary",
    ),
    (
        "crates/codeflow-core/src/scaffold/settings_merge.rs",
        "prior_key",
        "shipped",
        1,
        "a key, checked against an earlier preset embedded in the binary",
    ),
    (
        "crates/codeflow-core/src/scaffold/settings_merge.rs",
        "name",
        "shipped",
        1,
        "a key or command, checked against the shipped preset (not templated)",
    ),
    (
        "crates/codeflow-core/src/scaffold/settings_merge.rs",
        "entry",
        "shipped",
        1,
        "an entry, checked against the shipped preset (not templated)",
    ),
    (
        "crates/codeflow-core/src/scaffold/update.rs",
        "policy_key",
        "shipped",
        1,
        "a key segment, checked against the embedded policy default",
    ),
    (
        "crates/codeflow-core/src/scaffold/update.rs",
        "shipped_value",
        "shipped",
        1,
        "a value, checked against the embedded policy default",
    ),
    // Names CodeFlow defines, and the binary's own date.
    (
        "crates/codeflow-core/src/scaffold/pr_template.rs",
        "shipped_section",
        "defined",
        1,
        "the default policy's required PR sections",
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
const RUNTIME: &[&str] = &["path", "shipped", "defined", "iso_date"];

/// The macro's helpers: called only through `shown!`.
const MACRO_HELPERS: &[&str] = &["from_shown_macro", "macro_argument"];

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

fn is_test_item(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("cfg")
            && attr
                .meta
                .require_list()
                .is_ok_and(|list| list.tokens.to_string().contains("test"))
    })
}

/// What a file holds at the boundary.
#[derive(Default)]
struct Found {
    /// (item, constructor) -> count.
    runtime: BTreeMap<(String, String), usize>,
    /// Every `shown!` format string that captures a variable.
    captures: Vec<String>,
    /// Every direct call of a macro helper.
    helpers: Vec<String>,
}

struct Scan {
    item: Vec<String>,
    found: Found,
}

impl Scan {
    /// A path to a constructor or helper, called or passed as a value.
    fn path(&mut self, path: &syn::Path) {
        let segments: Vec<String> = path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect();
        let item = self.item.last().cloned().unwrap_or_default();
        if let [.., owner, name] = segments.as_slice() {
            if owner == "Shown" && RUNTIME.contains(&name.as_str()) {
                *self
                    .found
                    .runtime
                    .entry((item.clone(), name.clone()))
                    .or_default() += 1;
            }
        }
        if let Some(name) = segments.last() {
            if MACRO_HELPERS.contains(&name.as_str()) {
                self.found.helpers.push(format!("{item}: {name}"));
            }
        }
    }

    /// The arguments of a macro, when they parse as expressions.
    fn macro_args(&mut self, mac: &syn::Macro) {
        use syn::parse::Parser;
        let parser = syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated;
        let Ok(args) = parser.parse2(mac.tokens.clone()) else {
            return;
        };
        let is_shown = mac.path.segments.last().is_some_and(|s| s.ident == "shown");
        for (index, arg) in args.iter().enumerate() {
            if is_shown && index == 0 {
                if let syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(format),
                    ..
                }) = arg
                {
                    if captures(&format.value()) {
                        self.found.captures.push(format.value());
                    }
                }
                continue;
            }
            self.visit_expr(arg);
        }
    }
}

/// Whether a format string has a placeholder that names a variable.
fn captures(format: &str) -> bool {
    let mut chars = format.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
            }
            '{' => {
                let name: String = chars
                    .by_ref()
                    .take_while(|c| *c != '}' && *c != ':')
                    .collect();
                if !name.is_empty() && !name.chars().all(|c| c.is_ascii_digit()) {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

impl<'ast> Visit<'ast> for Scan {
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        if is_test_item(&node.attrs) {
            return;
        }
        self.item.push(node.sig.ident.to_string());
        syn::visit::visit_item_fn(self, node);
        self.item.pop();
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        if is_test_item(&node.attrs) {
            return;
        }
        self.item.push(node.sig.ident.to_string());
        syn::visit::visit_impl_item_fn(self, node);
        self.item.pop();
    }

    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        if is_test_item(&node.attrs) {
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

    fn visit_expr_path(&mut self, node: &'ast syn::ExprPath) {
        self.path(&node.path);
        syn::visit::visit_expr_path(self, node);
    }

    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        self.macro_args(node);
        syn::visit::visit_macro(self, node);
    }
}

fn scan(path: &Path) -> Found {
    let text = std::fs::read_to_string(path).unwrap();
    let file = syn::parse_file(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut scan = Scan {
        item: Vec::new(),
        found: Found::default(),
    };
    scan.visit_file(&file);
    scan.found
}

fn sources() -> Vec<(String, PathBuf)> {
    let root = root();
    let mut files = Vec::new();
    for dir in ["crates/codeflow-core/src", "crates/codeflow-cli/src"] {
        rust_files(&root.join(dir), &mut files);
    }
    files
        .into_iter()
        .map(|path| {
            let relative = path
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            (relative, path)
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
    for (relative, path) in sources() {
        if relative.ends_with("scaffold/shown.rs") {
            continue;
        }
        let found = scan(&path);
        for format in &found.captures {
            problems.push(format!(
                "{relative}: a shown! format string captures a variable: {format:?}"
            ));
        }
        for helper in &found.helpers {
            problems.push(format!(
                "{relative}: {helper} is called directly; only shown! may call it"
            ));
        }
        if relative == PORTAL.0 {
            let paths: usize = found
                .runtime
                .iter()
                .filter(|((_, constructor), _)| constructor == "path")
                .map(|(_, count)| count)
                .sum();
            if paths != PORTAL.1 {
                problems.push(format!(
                    "{relative}: {paths} Shown::path calls, {} listed; review each new one",
                    PORTAL.1
                ));
            }
            for ((item, constructor), count) in &found.runtime {
                if constructor != "path" {
                    problems.push(format!(
                        "{relative} {item}: {count} unlisted Shown::{constructor}"
                    ));
                }
            }
            continue;
        }
        for ((item, constructor), count) in found.runtime {
            let key = (relative.clone(), item.clone(), constructor.clone());
            match expected.remove(&key) {
                Some(listed) if listed == count => {}
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
    ] {
        assert!(
            !text.contains(conversion),
            "shown.rs must not implement {conversion}"
        );
    }
    // A `&'static str` made from a runtime string would pass `Shown::fixed`.
    for (relative, path) in sources() {
        let source = std::fs::read_to_string(&path).unwrap();
        if source.contains("Shown") {
            assert!(
                !source.contains("Box::leak") && !source.contains(".leak()"),
                "{relative}: leaks a String into a &'static str"
            );
        }
    }
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
    ] {
        assert!(errors.contains(variant), "ScaffoldError: {variant}");
    }
    assert!(
        !errors.contains("#[source]") && !errors.contains("#[from]"),
        "ScaffoldError keeps no source"
    );
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
    let redact = read("crates/codeflow-core/src/scaffold/diff_redact.rs");
    assert!(
        redact.contains("reference: Reference<'_>) -> Shown {"),
        "a --diff section is Shown"
    );
}

/// The body of `fn name` in `source`.
fn body<'a>(source: &'a str, name: &str) -> &'a str {
    let start = source
        .find(&format!("\nfn {name}("))
        .unwrap_or_else(|| panic!("fn {name}"));
    let rest = &source[start..];
    let end = rest.find("\n}\n").expect("end of fn");
    &rest[..end]
}

#[test]
fn the_cli_prints_update_and_pin_only_through_the_boundary() {
    let main = std::fs::read_to_string(root().join("crates/codeflow-cli/src/main.rs")).unwrap();
    let update = body(&main, "update");
    for needed in [
        "scaffold::update(assets, cwd, &options).map_err(boundary_error)?",
        "scaffold::portal::update_adopted_portal(assets, cwd).map_err(boundary_error)?",
        "print_block!(report.block());",
    ] {
        assert!(update.contains(needed), "fn update: {needed}");
    }
    assert!(
        !update.contains("println!") && !update.contains("eprintln!"),
        "fn update prints only report blocks"
    );
    let pin = body(&main, "pin_release");
    assert!(
        pin.contains("println!(\"{}\", report.shown());"),
        "fn pin_release: report"
    );
    assert_eq!(
        pin.matches("eprintln!").count(),
        pin.matches("error = crate::cmd::shown(&error)").count(),
        "fn pin_release prints only its Shown errors"
    );
    let boundary = body(&main, "boundary_error");
    assert!(boundary.contains("anyhow::Error::msg(scaffold::Shown::from(&error).into_string())"));
}

#[test]
fn the_settings_presets_are_not_templated() {
    // `settings_merge` names a key or entry only when the preset it merges
    // holds it, which proves it is CodeFlow's only while the preset update
    // passes is the embedded asset as written.
    let root = root();
    let manifest =
        std::fs::read_to_string(root.join("assets/base/scaffold-manifest.toml")).unwrap();
    let manifest: toml::Value = toml::from_str(&manifest).unwrap();
    let entries = manifest["entry"].as_array().unwrap();
    let mut presets = 0;
    for entry in entries {
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

#[test]
fn a_format_string_capture_is_found() {
    assert!(captures("value {secret}"));
    assert!(captures("value {secret:?}"));
    assert!(!captures("value {} and {0} and {:<22} and {{literal}}"));
}
