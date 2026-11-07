//! TSK-1299 AC-5: a limit is declared once, in `crates/codeflow-limits`, and
//! a literal limit anywhere else in product code fails here with its file and
//! line.
//!
//! The scan parses each product source file with `syn`, skips test-only
//! items and files, and reports two shapes:
//!
//! - a `const` or `static` of an integer or `Duration` type whose name says
//!   it is a limit ([`LIMIT_WORDS`]) and whose value holds a number and does
//!   not read the registry;
//! - any `Duration::from_secs`, `from_millis`, `from_mins` or `from_micros`
//!   call with a number in it, in or out of a constant, because every such
//!   call in product code is a timeout, wait or poll.
//!
//! It is a structural check. A bare number in a condition (`if n > 1000`)
//! under no limit-named constant is outside what it can tell from an
//! ordinary number, so that shape still rests on review.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use proc_macro2::LineColumn;
use syn::spanned::Spanned;
use syn::visit::Visit;

/// Words that make a constant's name a limit.
const LIMIT_WORDS: [&str; 43] = [
    "MAX",
    "MIN",
    "MOST",
    "LIMIT",
    "CAP",
    "TIMEOUT",
    "WAIT",
    "BOUND",
    "BUDGET",
    "DEPTH",
    "ATTEMPTS",
    "RETRIES",
    "BYTES",
    "SIZE",
    "SECONDS",
    "SECS",
    "MILLIS",
    "DAYS",
    "INTERVAL",
    "POLL",
    "GRACE",
    "BATCH",
    "ENTRIES",
    "SHOWN",
    "TTL",
    "LINES",
    "CHARS",
    "WORDS",
    "JOBS",
    "SHARDS",
    "SETTLE",
    "FRESHNESS",
    "PASSES",
    "MULTIPLIER",
    "FOLDERS",
    "CANDIDATES",
    "VALUES",
    "VARIANTS",
    "PROMPT",
    "SHORT",
    "MEDIUM",
    "LONG",
    "WORD",
];

/// Constants a limit word matches that are not limits. Each is a fixed fact
/// of another system, so no project could mean to change it.
const NOT_LIMITS: [(&str, &str); 3] = [
    ("MAX_SAFE_INTEGER", "JavaScript's own largest exact integer"),
    (
        "MAX_DOCUMENT_SCHEMA_VERSION",
        "a format version, not a bound on work",
    ),
    ("MAX", "the bounds of the i32 type git reads a value into"),
];

/// A literal limit found in product code.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Literal {
    line: usize,
    what: String,
}

fn test_only(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("cfg")
            && attr.meta.require_list().is_ok_and(|list| {
                let text = list.tokens.to_string();
                text == "test" || text.starts_with("all (test") || text.contains("test ,")
            })
    })
}

fn is_limit_name(name: &str) -> bool {
    // An exit code is a fixed answer to a caller, whatever else its name says.
    !name.contains("EXIT_")
        && !name.ends_with("_EXIT_CODE")
        && !NOT_LIMITS.iter().any(|(listed, _)| *listed == name)
        && name.split('_').any(|word| LIMIT_WORDS.contains(&word))
}

fn is_limit_type(ty: &syn::Type) -> bool {
    let syn::Type::Path(path) = ty else {
        return false;
    };
    path.path.segments.last().is_some_and(|last| {
        matches!(
            last.ident.to_string().as_str(),
            "u8" | "u16"
                | "u32"
                | "u64"
                | "u128"
                | "usize"
                | "i8"
                | "i16"
                | "i32"
                | "i64"
                | "i128"
                | "isize"
                | "Duration"
        )
    })
}

/// Whether the expression's tokens hold a number and read nothing else: no
/// registry entry, and no other name, so `5 * MINUTE` built from a declared
/// constant and `seconds.max(1)` guarding a read value both pass.
fn holds_literal(expr: &syn::Expr) -> bool {
    fn walk(tokens: proc_macro2::TokenStream, literal: &mut bool, registry: &mut bool) {
        for token in tokens {
            match token {
                proc_macro2::TokenTree::Literal(value) => {
                    *literal |= value.to_string().starts_with(|c: char| c.is_ascii_digit());
                }
                proc_macro2::TokenTree::Ident(ident) => {
                    let name = ident.to_string();
                    *registry |= !matches!(
                        name.as_str(),
                        "u8" | "u16" | "u32" | "u64" | "usize" | "MAX" | "KIB"
                    );
                }
                proc_macro2::TokenTree::Group(group) => walk(group.stream(), literal, registry),
                proc_macro2::TokenTree::Punct(_) => {}
            }
        }
    }
    let (mut literal, mut registry) = (false, false);
    walk(quote_expr(expr), &mut literal, &mut registry);
    literal && !registry
}

fn quote_expr(expr: &syn::Expr) -> proc_macro2::TokenStream {
    use std::str::FromStr;
    let text = expr_text(expr);
    proc_macro2::TokenStream::from_str(&text).unwrap_or_default()
}

fn expr_text(expr: &syn::Expr) -> String {
    expr.span().source_text().unwrap_or_default()
}

fn start(span: proc_macro2::Span) -> usize {
    let LineColumn { line, .. } = span.start();
    line
}

#[derive(Default)]
struct Scan {
    found: Vec<Literal>,
}

impl Scan {
    fn constant(&mut self, name: &syn::Ident, ty: &syn::Type, expr: &syn::Expr) {
        let name = name.to_string();
        if is_limit_type(ty) && is_limit_name(&name) && holds_literal(expr) {
            self.found.push(Literal {
                line: start(ty.span()),
                what: format!("constant {name} = {}", expr_text(expr)),
            });
        }
    }
}

impl<'ast> Visit<'ast> for Scan {
    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        if !test_only(&node.attrs) {
            syn::visit::visit_item_mod(self, node);
        }
    }

    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        if !test_only(&node.attrs) {
            syn::visit::visit_item_fn(self, node);
        }
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        if !test_only(&node.attrs) {
            syn::visit::visit_impl_item_fn(self, node);
        }
    }

    fn visit_expr_if(&mut self, node: &'ast syn::ExprIf) {
        if !test_only(&node.attrs) {
            syn::visit::visit_expr_if(self, node);
        }
    }

    fn visit_item_const(&mut self, node: &'ast syn::ItemConst) {
        if !test_only(&node.attrs) {
            self.constant(&node.ident, &node.ty, &node.expr);
            syn::visit::visit_item_const(self, node);
        }
    }

    fn visit_item_static(&mut self, node: &'ast syn::ItemStatic) {
        if !test_only(&node.attrs) {
            self.constant(&node.ident, &node.ty, &node.expr);
            syn::visit::visit_item_static(self, node);
        }
    }

    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if let syn::Expr::Path(function) = &*node.func {
            let mut names = function
                .path
                .segments
                .iter()
                .rev()
                .map(|s| s.ident.to_string());
            let call = names.next().unwrap_or_default();
            if names.next().as_deref() == Some("Duration")
                && matches!(
                    call.as_str(),
                    "from_secs" | "from_millis" | "from_mins" | "from_micros"
                )
                && node.args.first().is_some_and(holds_literal)
            {
                self.found.push(Literal {
                    line: start(node.func.span()),
                    what: format!("Duration::{call}({})", expr_text(&node.args[0])),
                });
            }
        }
        syn::visit::visit_expr_call(self, node);
    }
}

fn scan(text: &str) -> Vec<Literal> {
    let file = syn::parse_file(text).expect("product source parses");
    let mut scan = Scan::default();
    scan.visit_file(&file);
    scan.found.sort();
    scan.found
}

/// The out-of-line modules a `#[cfg(test)] mod name;` declares in `file`.
fn test_modules(file: &syn::File) -> Vec<String> {
    file.items
        .iter()
        .filter_map(|item| match item {
            syn::Item::Mod(module) if module.content.is_none() && test_only(&module.attrs) => {
                Some(module.ident.to_string())
            }
            _ => None,
        })
        .collect()
}

fn rust_files(dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
}

/// Every product source file outside the registry crate, without the files
/// only tests compile.
fn product_files(repository: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for crate_name in ["codeflow-core", "codeflow-cli", "codeflow-present"] {
        rust_files(
            &repository.join("crates").join(crate_name).join("src"),
            &mut files,
        );
    }
    let mut test_only_paths = BTreeSet::new();
    for path in &files {
        let text = std::fs::read_to_string(path).unwrap();
        let parsed = syn::parse_file(&text).expect("product source parses");
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        let parent = path.parent().unwrap();
        for module in test_modules(&parsed) {
            for base in [parent.to_path_buf(), parent.join(&stem)] {
                test_only_paths.insert(base.join(format!("{module}.rs")));
                test_only_paths.insert(base.join(&module));
            }
        }
    }
    files.retain(|path| {
        !path.to_string_lossy().ends_with("_tests.rs")
            && !test_only_paths
                .iter()
                .any(|skipped| path == skipped || path.starts_with(skipped))
    });
    files.sort();
    files
}

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn no_literal_limit_lives_outside_the_registry() {
    let root = repository();
    let mut findings = Vec::new();
    for path in product_files(&root) {
        let text = std::fs::read_to_string(&path).unwrap();
        for literal in scan(&text) {
            findings.push(format!(
                "{}:{}: {}",
                path.strip_prefix(&root).unwrap().display(),
                literal.line,
                literal.what
            ));
        }
    }
    assert!(
        findings.is_empty(),
        "{} literal limits outside crates/codeflow-limits; declare each once in the registry \
         and read it from there:\n{}",
        findings.len(),
        findings.join("\n")
    );
}

#[test]
fn a_planted_literal_fails_naming_its_line() {
    let planted = "\n\nconst PUSH_TIMEOUT: u64 = 30;\n";
    assert_eq!(
        scan(planted),
        [Literal {
            line: 3,
            what: "constant PUSH_TIMEOUT = 30".into()
        }]
    );
    let inline = "fn wait() {\n    sleep(std::time::Duration::from_millis(250));\n}\n";
    assert_eq!(
        scan(inline),
        [Literal {
            line: 2,
            what: "Duration::from_millis(250)".into()
        }]
    );
}

/// TSK-1325's literal cap in the test configuration is one shape this check
/// must catch, in the parser and in its deadline arithmetic alike.
#[test]
fn a_literal_cap_in_the_test_config_fails() {
    let planted = "const MAX_CLEANUP_GRACE_SECONDS: u64 = 3600;\n\
                   pub const MAX_TIMEOUT_SECONDS: u64 = u64::MAX / (4 * 1_000);\n";
    let found = scan(planted);
    assert_eq!(found.len(), 2, "{found:?}");
    assert_eq!(found[0].line, 1);
}

#[test]
fn a_registry_read_and_test_code_pass() {
    let clean = "pub const DEADLINE: u64 = codeflow_limits::GUARD_DEADLINE.default;\n\
                 const SCHEMA_VERSION: u32 = 2;\n\
                 #[cfg(test)]\nmod tests {\n    const WAIT_SECONDS: u64 = 3;\n}\n";
    assert_eq!(scan(clean), []);
}

/// The product sources outside the registry, comment lines removed.
fn product_text(folder: &Path, text: &mut String) {
    for entry in std::fs::read_dir(folder).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            product_text(&path, text);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            for line in std::fs::read_to_string(&path).unwrap().lines() {
                if !line.trim_start().starts_with("//") {
                    text.push_str(line);
                    text.push('\n');
                }
            }
        }
    }
}

/// An entry nothing reads is a page row that changes nothing, so every
/// declared limit has a reader outside the registry.
#[test]
fn every_registry_entry_is_read_outside_the_registry() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let registry = std::fs::read_to_string(crates.join("codeflow-limits/src/registry.rs")).unwrap();
    let mut text = String::new();
    for name in ["codeflow-core", "codeflow-cli", "codeflow-present"] {
        product_text(&crates.join(name).join("src"), &mut text);
    }
    let unread: Vec<&str> = registry
        .lines()
        .filter_map(|line| line.strip_prefix("    ")?.split_once(" = "))
        .map(|(name, _)| name)
        .filter(|name| {
            name.chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        })
        .filter(|name| !text.contains(name))
        .collect();
    assert!(unread.is_empty(), "no reader for: {unread:?}");
}
