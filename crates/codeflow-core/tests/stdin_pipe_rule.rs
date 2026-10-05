//! A child's stdin is piped in one place: `crates/codeflow-core/src/git/stdin.rs`
//! (issue 71, TSK-237). A caller that wrote stdin itself, or piped it and
//! drained one output stream at a time, could deadlock against a child that
//! answers while it reads.
//!
//! This test parses every Rust source under `crates/` (not scripts or assets)
//! with `syn` and fails on production code outside the helper that
//!
//! - calls `.stdin(arg)` with anything but `Stdio::null()` or
//!   `Stdio::inherit()` (a variable or a conditional hides what it pipes), or
//!   calls a path function named `stdin` (`Command::stdin(&mut c, x)`) other
//!   than the process's own `io::stdin()`;
//! - reads a field named `stdin` (`child.stdin.take()`), or binds one in a
//!   struct pattern (`let Child { ref mut stdin, .. } = child`, under any
//!   alias or nesting);
//! - names the `ChildStdin` type, imported or not.
//!
//! Items marked `#[cfg(test)]` or `#[cfg(all(test, ...))]` are skipped one by
//! one, so production code after a test module is still read. Raw identifiers
//! (`r#stdin`) are read as the plain name. Limits, stated rather than hidden:
//! code inside macro invocations and `macro_rules!` bodies is not parsed, and
//! raw file descriptors or handles taken from the operating system are not
//! seen.

use std::path::{Path, PathBuf};

use syn::ext::IdentExt;
use syn::spanned::Spanned;
use syn::visit::Visit;

/// The only source file allowed to pipe a child's stdin.
const HELPER: &str = "crates/codeflow-core/src/git/stdin.rs";

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
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

fn is_stdin(ident: &syn::Ident) -> bool {
    ident.unraw() == "stdin"
}

/// A path that ends in `stdin` and names more than a local: `Command::stdin`,
/// `<Command>::stdin`, `io::stdin`. A single segment is a local or parameter.
fn is_stdin_path(path: &syn::ExprPath) -> bool {
    let segments = &path.path.segments;
    let own_stdin = path.qself.is_none()
        && segments.len() >= 2
        && segments[segments.len() - 2].ident.unraw() == "io";
    segments.last().is_some_and(|s| is_stdin(&s.ident))
        && (segments.len() > 1 || path.qself.is_some())
        && !own_stdin
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

/// `Stdio::null()` or `Stdio::inherit()`, however the path is qualified.
fn is_not_piped(argument: &syn::Expr) -> bool {
    let syn::Expr::Call(call) = argument else {
        return false;
    };
    if !call.args.is_empty() {
        return false;
    }
    let syn::Expr::Path(path) = &*call.func else {
        return false;
    };
    let names: Vec<String> = path
        .path
        .segments
        .iter()
        .map(|segment| segment.ident.unraw().to_string())
        .collect();
    names.len() >= 2
        && names[names.len() - 2] == "Stdio"
        && matches!(names[names.len() - 1].as_str(), "null" | "inherit")
}

#[derive(Default)]
struct Uses {
    found: Vec<(usize, String)>,
}

impl Uses {
    fn note(&mut self, span: proc_macro2::Span, what: &str) {
        self.found.push((span.start().line, what.to_string()));
    }
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

impl<'ast> Visit<'ast> for Uses {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        if !skipped(item_attributes(item)) {
            syn::visit::visit_item(self, item);
        }
    }

    fn visit_impl_item(&mut self, item: &'ast syn::ImplItem) {
        if !skipped(impl_item_attributes(item)) {
            syn::visit::visit_impl_item(self, item);
        }
    }

    fn visit_trait_item(&mut self, item: &'ast syn::TraitItem) {
        if !skipped(trait_item_attributes(item)) {
            syn::visit::visit_trait_item(self, item);
        }
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        if is_stdin(&call.method) {
            let piped = call.args.len() != 1 || !is_not_piped(&call.args[0]);
            if piped {
                self.note(
                    call.method.span(),
                    "`.stdin(...)` with an argument that may pipe",
                );
            }
        }
        syn::visit::visit_expr_method_call(self, call);
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        // The callee, through parentheses: `(Command::stdin)(&mut c, x)`.
        let mut callee = &*call.func;
        while let syn::Expr::Paren(inner) = callee {
            callee = &inner.expr;
        }
        if let syn::Expr::Path(path) = callee {
            if is_stdin_path(path) {
                // Only the process's own `io::stdin()` takes no argument.
                if !call.args.is_empty() {
                    self.note(path.span(), "a path call of `stdin` with arguments");
                }
                for argument in &call.args {
                    self.visit_expr(argument);
                }
                return;
            }
        }
        syn::visit::visit_expr_call(self, call);
    }

    fn visit_expr_path(&mut self, path: &'ast syn::ExprPath) {
        // `Command::stdin` used as a value (`map(Command::stdin)`, an alias).
        if is_stdin_path(path) {
            self.note(path.span(), "the `stdin` setter used as a value");
        }
        syn::visit::visit_expr_path(self, path);
    }

    fn visit_expr_field(&mut self, field: &'ast syn::ExprField) {
        if let syn::Member::Named(name) = &field.member {
            if is_stdin(name) {
                self.note(name.span(), "a read of a `stdin` field");
            }
        }
        syn::visit::visit_expr_field(self, field);
    }

    fn visit_field_pat(&mut self, field: &'ast syn::FieldPat) {
        if let syn::Member::Named(name) = &field.member {
            if is_stdin(name) {
                self.note(name.span(), "a `stdin` field bound in a struct pattern");
            }
        }
        syn::visit::visit_field_pat(self, field);
    }

    fn visit_use_name(&mut self, name: &'ast syn::UseName) {
        if name.ident.unraw() == "ChildStdin" {
            self.note(name.ident.span(), "the `ChildStdin` type, imported");
        }
    }

    fn visit_use_rename(&mut self, rename: &'ast syn::UseRename) {
        if rename.ident.unraw() == "ChildStdin" {
            self.note(
                rename.ident.span(),
                "the `ChildStdin` type, imported and renamed",
            );
        }
    }

    fn visit_path_segment(&mut self, segment: &'ast syn::PathSegment) {
        if segment.ident.unraw() == "ChildStdin" {
            self.note(segment.ident.span(), "the `ChildStdin` type");
        }
        syn::visit::visit_path_segment(self, segment);
    }
}

/// Every use of a child's stdin in `source`, as (line, what).
fn stdin_uses(source: &str) -> Vec<(usize, String)> {
    let file = syn::parse_file(source).expect("the source parses");
    let mut uses = Uses::default();
    // A file marked `#![cfg(test)]` is test code throughout.
    if !skipped(&file.attrs) {
        uses.visit_file(&file);
    }
    uses.found.sort();
    uses.found
}

#[test]
fn only_the_stdin_helper_pipes_a_childs_stdin() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);
    assert!(files.len() > 100, "the scan found the workspace sources");
    let mut offenders = Vec::new();
    for file in files {
        let relative = file
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/")
            .replace("/../", "/");
        let relative = relative.trim_start_matches("./").to_string();
        if relative == HELPER {
            continue;
        }
        let text = std::fs::read_to_string(&file).unwrap();
        let file_uses = std::panic::catch_unwind(|| stdin_uses(&text))
            .unwrap_or_else(|_| panic!("{relative} does not parse as Rust"));
        for (line, what) in file_uses {
            offenders.push(format!("{relative}:{line}: {what}"));
        }
    }
    assert!(
        offenders.is_empty(),
        "pipe a child's stdin through codeflow_core::git::output_with_input or \
         spawn_with_input (crates/codeflow-core/src/git/stdin.rs), which writes it from its \
         own thread; found:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn the_scan_flags_each_way_to_reach_a_childs_stdin() {
    for (source, what) in [
        ("fn f() { c.stdin(Stdio::piped()); }", "piped"),
        ("fn f() { c.stdin(std::process::Stdio::piped()); }", "qualified"),
        ("fn f() { c . stdin (\n  Stdio::piped(),\n); }", "spaced, split, trailing comma"),
        ("fn f() { c.stdin(mode); }", "a variable"),
        (
            "fn f() { c.stdin(if x { Stdio::piped() } else { Stdio::null() }); }",
            "a conditional",
        ),
        ("fn f() { Command::stdin(&mut c, mode); }", "a path call"),
        ("fn f() { let w = child.stdin.take(); }", "take"),
        ("fn f() { let w = child\n    .stdin\n    .take(); }", "take across lines"),
        ("fn f() { child.stdin.as_mut().unwrap(); }", "as_mut"),
        ("fn f() { c.r#stdin(Stdio::piped()); }", "raw identifier call"),
        ("fn f() { let w = child.r#stdin.take(); }", "raw identifier field"),
        ("fn f() { Command::r#stdin(&mut c, mode); }", "raw identifier path"),
        ("fn f() { let Child { stdin, .. } = child; }", "a destructured child"),
        ("fn f() { let Child { ref mut stdin, .. } = child; }", "ref mut"),
        ("fn f() { let Child { ref stdin, .. } = child; }", "ref"),
        ("fn f() { match c { Child { ref mut stdin, .. } => {} } }", "a match arm"),
        ("fn f() { let Child { stdout: Some(ChildStdout { .. }), stdin, .. } = c else { return; }; }", "nested"),
        ("use std::process::Child as P; fn f() { let P { stdin: pipe, .. } = c; }", "aliased"),
        ("fn f() { <Command>::stdin(&mut c, Stdio::piped()); }", "a qualified-self call"),
        ("fn f() { (Command::stdin)(&mut c, Stdio::piped()); }", "a parenthesized callee"),
        ("fn f() { let set = Command::stdin; }", "the setter as a value"),
        ("fn f() { v.into_iter().for_each(Command::stdin); }", "the setter passed on"),
        ("fn f(w: ChildStdin) {}", "the handle type"),
        ("use std::process::ChildStdin as Input; fn f(w: Input) {}", "an aliased handle type"),
        ("use std::process::{Child, ChildStdin};", "an imported handle type"),
        ("fn f() -> Option<std::process::ChildStdin> { None }", "the qualified handle type"),
    ] {
        assert_eq!(stdin_uses(source).len(), 1, "{what}: {source}");
    }
}

#[test]
fn the_scan_passes_what_does_not_pipe() {
    for (source, what) in [
        ("fn f() { c.stdin(Stdio::null()); }", "null"),
        (
            "fn f() { c . stdin ( std::process::Stdio::inherit() ); }",
            "inherit",
        ),
        ("fn f() { c.stdin(Stdio::null(),); }", "a trailing comma"),
        (
            "fn f() { let mut s = String::new(); std::io::stdin().read_line(&mut s); }",
            "own stdin",
        ),
        (
            "// c.stdin(Stdio::piped()) in a comment\nfn f() {}",
            "line comment",
        ),
        ("/* child.stdin.take() */ fn f() {}", "block comment"),
        ("fn f() { let s = \"child.stdin.take()\"; }", "string"),
        (
            "fn f() { let s = r#\"c.stdin(Stdio::piped())\"#; }",
            "raw string",
        ),
        (
            "fn f() { let p = \"C:\\\\piped\\\\stdin.rs\"; let c = '\\''; }",
            "windows path",
        ),
        (
            "fn f<'a>(x: &'a str) { c.stdin(Stdio::null()); }",
            "lifetime",
        ),
        (
            "fn stdin_text() {} fn g() { let stdin_bytes = 1; }",
            "a name that contains stdin",
        ),
        (
            "use std::io::stdin;\nuse std::io::{stdin as s, Read};",
            "importing own stdin",
        ),
        (
            "pub use std::io::stdin;\npub(crate) use std::io::stdin as t;",
            "re-exporting own stdin",
        ),
        ("use std::io::{stdin, Read};", "an import group"),
        (
            "fn f() { let read = std::io::stdin; read(); }",
            "own stdin as a value",
        ),
        (
            "#![cfg(test)]\nfn f() { c.stdin(Stdio::piped()); }",
            "a test-only file",
        ),
        (
            "fn f(stdin: &str, other: Option<&str>) {}",
            "a parameter named stdin",
        ),
        (
            "fn identity<T: Clone>(stdin: T) -> T { stdin }",
            "a generic parameter and local",
        ),
        (
            "fn f() { let g = |x: usize, stdin: &str| x; }",
            "a closure parameter",
        ),
        ("struct Request { stdin: String }", "a struct definition"),
        (
            "fn f() { let r = Request { stdin: text }; }",
            "a struct literal of another type",
        ),
        (
            "fn g(&mut self) -> &mut Child { self.0.as_mut() }",
            "a Child return type",
        ),
        (
            "fn f(child: &mut Child) -> &mut Child { let _ = std::io::stdin(); child }",
            "own stdin beside Child",
        ),
    ] {
        assert!(stdin_uses(source).is_empty(), "{what}: {source}");
    }
}

#[test]
fn the_scan_skips_test_items_but_reads_the_code_after_them() {
    let source = "fn a() { c.stdin(Stdio::null()); }\n\
        #[cfg(test)]\nmod tests { fn t() { y.stdin(Stdio::piped()); let w = z.stdin.take(); } }\n\
        #[cfg(all(test, unix))]\nfn only_in_tests() { y.stdin(Stdio::piped()); }\n\
        #[cfg(not(test))]\nfn later() { y.stdin(Stdio::piped()); }\n\
        fn after() { w.stdin.take(); }\n";
    let lines: Vec<usize> = stdin_uses(source)
        .into_iter()
        .map(|(line, _)| line)
        .collect();
    assert_eq!(lines, vec![7, 8]);
}
