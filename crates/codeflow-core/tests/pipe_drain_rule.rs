//! A test that waits for a child must not leave the child's piped output
//! unread (issue 100, TSK-282). A pipe holds a few hundred bytes on a host
//! under pipe memory pressure and 64 KiB on most others, so a child whose
//! output outgrows it blocks in `write` while the test sits in `wait()` or in
//! a `try_wait()` loop, and the stall reads as a hang or as a false failure in
//! a timing test.
//!
//! This test parses the Rust test code under `crates/` with `syn`: every file
//! under a `tests` directory, a file named `tests.rs` or `*_tests.rs`, and the
//! items marked `#[cfg(test)]` or `#[test]` elsewhere. In each function it
//! reads, every `let child = <command chain>.spawn()` whose chain sends
//! `.stdout(..)` or `.stderr(..)` to `Stdio::piped()` must keep this contract
//! for each piped stream:
//!
//! 1. The stream is read to the end or handed to a named drain helper in the
//!    same function, before the function's first `child.wait()` or
//!    `child.try_wait()` poll (a poll is a `try_wait` inside a loop or a
//!    closure). A drain is `drain_pipe(child.stdout.take())` or
//!    `drain_lines(child.stderr.take())` (the helpers in
//!    `crates/codeflow-cli/tests/support/drain_pipe.rs`; the list is
//!    [`DRAIN_HELPERS`]), or a call chain that starts at the field and ends in
//!    `.read_to_end(..)` or `.read_to_string(..)`
//!    (`child.stdout.take().unwrap().read_to_end(&mut bytes)`).
//! 2. A function with no wait or poll of that child must still drain the
//!    stream, or call `child.wait_with_output()`, so a helper that returns the
//!    child hands on a child whose output is already being read.
//!
//! Borrowing or taking a stream without handing it to a helper or reading it
//! earns no credit, and neither does `wait_with_output` after a wait or poll,
//! because it reads the pipes only after the function has already waited.
//! Children are told apart by the name they are bound to, so draining one
//! child does not excuse another, and a command run with `.output()` is not a
//! child.
//!
//! Limits, stated rather than hidden:
//!
//! - A function is judged by its own body. A helper that spawns and returns a
//!   child must drain the pipes itself (rule 2); a caller that spawns through
//!   a helper is not seen.
//! - Pipes set outside the spawn chain (`cmd.stdout(Stdio::piped());` as its
//!   own statement), through a variable, or through `Command::stdout(&mut c,
//!   ..)` are not seen as piped.
//! - A stream moved into a local before it is read (`let out =
//!   child.stdout.take(); ... out.read_to_end(..)`) is not a drain; hand the
//!   `take()` to a helper or chain the read.
//! - A child rebound under another name is not followed, and a child inside a
//!   struct field is not seen.
//! - Macro bodies are read only when they parse as comma-separated
//!   expressions (`assert!`, `assert_eq!`, `vec!`); others, and
//!   `macro_rules!` bodies, are not parsed.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use syn::ext::IdentExt;
use syn::spanned::Spanned;
use syn::visit::Visit;

/// The functions that read a stream handed to them until the child closes it.
const DRAIN_HELPERS: &[&str] = &["drain_pipe", "drain_lines"];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if name != "target" && !name.starts_with('.') {
                rust_files(&path, out);
            }
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// Whether the whole file is test code: it sits under a `tests` directory or
/// is named for its tests.
fn test_file(relative: &str) -> bool {
    let mut parts: Vec<&str> = relative.split('/').collect();
    let name = parts.pop().unwrap_or_default();
    name == "tests.rs" || name.ends_with("_tests.rs") || parts.contains(&"tests")
}

/// `#[cfg(test)]`, `#[cfg(all(test, ..))]` or `#[test]`.
fn is_test_attribute(attribute: &syn::Attribute) -> bool {
    if attribute.path().is_ident("test") {
        return true;
    }
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

fn marked_test(attributes: &[syn::Attribute]) -> bool {
    attributes.iter().any(is_test_attribute)
}

/// `Stdio::piped()`, however the path is qualified.
fn is_piped(argument: &syn::Expr) -> bool {
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
    names.len() >= 2 && names[names.len() - 2] == "Stdio" && names[names.len() - 1] == "piped"
}

/// The stream a field or setter names: `stdout` or `stderr`.
fn stream(name: &syn::Ident) -> Option<&'static str> {
    match name.unraw().to_string().as_str() {
        "stdout" => Some("stdout"),
        "stderr" => Some("stderr"),
        _ => None,
    }
}

/// The name of a plain single-segment path (`child`), if the expression is one.
fn path_name(expr: &syn::Expr) -> Option<String> {
    let syn::Expr::Path(path) = expr else {
        return None;
    };
    if path.qself.is_some() || path.path.segments.len() != 1 {
        return None;
    }
    Some(path.path.segments[0].ident.unraw().to_string())
}

/// Whether an expression is `<chain>.spawn()` followed only by `unwrap`,
/// `expect` or `?`.
fn is_spawn_result(expr: &syn::Expr) -> bool {
    let mut expr = expr;
    loop {
        match expr {
            syn::Expr::MethodCall(call) => {
                let name = call.method.unraw().to_string();
                if name == "spawn" && call.args.is_empty() {
                    return true;
                }
                if name == "unwrap" || name == "expect" {
                    expr = &call.receiver;
                } else {
                    return false;
                }
            }
            syn::Expr::Try(inner) => expr = &inner.expr,
            syn::Expr::Paren(inner) => expr = &inner.expr,
            _ => return false,
        }
    }
}

type Position = (usize, usize);

fn position(span: proc_macro2::Span) -> Position {
    let start = span.start();
    (start.line, start.column)
}

/// Every `name.stdout` or `name.stderr` field read in an expression.
#[derive(Default)]
struct Fields {
    found: Vec<(String, &'static str)>,
}

impl<'ast> Visit<'ast> for Fields {
    fn visit_expr_field(&mut self, field: &'ast syn::ExprField) {
        if let (Some(name), syn::Member::Named(member)) = (path_name(&field.base), &field.member) {
            if let Some(stream) = stream(member) {
                self.found.push((name, stream));
            }
        }
        syn::visit::visit_expr_field(self, field);
    }
}

fn fields_in(expr: &syn::Expr) -> Vec<(String, &'static str)> {
    let mut fields = Fields::default();
    fields.visit_expr(expr);
    fields.found
}

/// Every `name.stdout.take()` or `name.stderr.take()` in an expression.
#[derive(Default)]
struct Takes {
    found: Vec<(String, &'static str)>,
}

impl<'ast> Visit<'ast> for Takes {
    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        if call.method.unraw() == "take" && call.args.is_empty() {
            if let syn::Expr::Field(field) = &*call.receiver {
                if let (Some(name), syn::Member::Named(member)) =
                    (path_name(&field.base), &field.member)
                {
                    if let Some(stream) = stream(member) {
                        self.found.push((name, stream));
                    }
                }
            }
        }
        syn::visit::visit_expr_method_call(self, call);
    }
}

fn takes_in(expr: &syn::Expr) -> Vec<(String, &'static str)> {
    let mut takes = Takes::default();
    takes.visit_expr(expr);
    takes.found
}

/// A child spawned with at least one piped output stream.
struct Child {
    name: Option<String>,
    piped: BTreeSet<&'static str>,
    line: usize,
    /// Where each stream is first drained.
    drained: BTreeMap<&'static str, Position>,
}

/// A `.wait()` or `try_wait()` poll.
struct Wait {
    name: Option<String>,
    at: Position,
    what: &'static str,
}

/// What one function does with its children.
#[derive(Default)]
struct Frame {
    children: Vec<Child>,
    waits: Vec<Wait>,
    /// The receiver name of each `wait_with_output` call.
    with_output: Vec<Option<String>>,
    in_test: bool,
}

#[derive(Default)]
struct Scan {
    frames: Vec<Frame>,
    /// Open test-item markers: how deep inside test items the visit is.
    test_depth: usize,
    /// Open loops and closures around the current expression.
    loop_depth: usize,
    /// The name a `let` binds, until its first spawn takes it.
    pending_name: Option<String>,
    found: Vec<(usize, String)>,
}

impl Scan {
    fn open(&mut self, attributes: &[syn::Attribute]) -> bool {
        let marked = marked_test(attributes);
        if marked {
            self.test_depth += 1;
        }
        marked
    }

    fn close(&mut self, marked: bool) {
        if marked {
            self.test_depth -= 1;
        }
    }

    fn function(&mut self, visit: impl FnOnce(&mut Self)) {
        let outer_loops = std::mem::take(&mut self.loop_depth);
        let outer_name = self.pending_name.take();
        self.frames.push(Frame {
            in_test: self.test_depth > 0,
            ..Frame::default()
        });
        visit(self);
        let frame = self.frames.pop().unwrap();
        self.loop_depth = outer_loops;
        self.pending_name = outer_name;
        if frame.in_test {
            self.judge(&frame);
        }
    }

    fn judge(&mut self, frame: &Frame) {
        let named: BTreeSet<&str> = frame
            .children
            .iter()
            .filter_map(|child| child.name.as_deref())
            .collect();
        // A wait or output read belongs to the named child it is called on;
        // one called on anything else belongs to the unnamed children.
        let is_theirs = |child: &Child, name: &Option<String>| match (&child.name, name) {
            (Some(own), Some(other)) => own == other,
            (None, None) => true,
            (None, Some(other)) => !named.contains(other.as_str()),
            (Some(_), None) => false,
        };
        for child in &frame.children {
            let first_wait = frame
                .waits
                .iter()
                .filter(|wait| is_theirs(child, &wait.name))
                .min_by_key(|wait| wait.at);
            let read_by_output = frame.with_output.iter().any(|name| is_theirs(child, name));
            // The streams this child leaves unread when it is first waited on.
            let unread: Vec<&str> = child
                .piped
                .iter()
                .copied()
                .filter(|stream| match (first_wait, child.drained.get(stream)) {
                    (Some(wait), Some(at)) => *at >= wait.at,
                    (Some(_), None) => true,
                    (None, drained) => drained.is_none() && !read_by_output,
                })
                .collect();
            if unread.is_empty() {
                continue;
            }
            let streams = unread.join(" and ");
            match first_wait {
                Some(wait) => self.found.push((
                    wait.at.0,
                    format!(
                        "{} before the child's piped {streams} is drained or read to the end",
                        wait.what
                    ),
                )),
                None => self.found.push((
                    child.line,
                    format!(
                        "a child is spawned with piped {streams} that this function neither \
                         drains nor reads with wait_with_output"
                    ),
                )),
            }
        }
    }

    fn drained(&mut self, name: &str, stream: &'static str, at: Position) {
        let Some(frame) = self.frames.last_mut() else {
            return;
        };
        if let Some(child) = frame
            .children
            .iter_mut()
            .rev()
            .find(|child| child.name.as_deref() == Some(name))
        {
            let earliest = child.drained.entry(stream).or_insert(at);
            if at < *earliest {
                *earliest = at;
            }
        }
    }
}

impl<'ast> Visit<'ast> for Scan {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        use syn::Item::{Const, Enum, Fn, Impl, Mod, Static, Struct, Trait, Type, Union, Use};
        let attributes: &[syn::Attribute] = match item {
            Const(i) => &i.attrs,
            Enum(i) => &i.attrs,
            Fn(i) => &i.attrs,
            Impl(i) => &i.attrs,
            Mod(i) => &i.attrs,
            Static(i) => &i.attrs,
            Struct(i) => &i.attrs,
            Trait(i) => &i.attrs,
            Type(i) => &i.attrs,
            Union(i) => &i.attrs,
            Use(i) => &i.attrs,
            _ => &[],
        };
        let marked = self.open(attributes);
        syn::visit::visit_item(self, item);
        self.close(marked);
    }

    fn visit_impl_item(&mut self, item: &'ast syn::ImplItem) {
        let attributes: &[syn::Attribute] = match item {
            syn::ImplItem::Fn(i) => &i.attrs,
            syn::ImplItem::Const(i) => &i.attrs,
            syn::ImplItem::Type(i) => &i.attrs,
            _ => &[],
        };
        let marked = self.open(attributes);
        syn::visit::visit_impl_item(self, item);
        self.close(marked);
    }

    fn visit_trait_item(&mut self, item: &'ast syn::TraitItem) {
        let attributes: &[syn::Attribute] = match item {
            syn::TraitItem::Fn(i) => &i.attrs,
            syn::TraitItem::Const(i) => &i.attrs,
            syn::TraitItem::Type(i) => &i.attrs,
            _ => &[],
        };
        let marked = self.open(attributes);
        syn::visit::visit_trait_item(self, item);
        self.close(marked);
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        self.function(|scan| syn::visit::visit_item_fn(scan, item));
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        self.function(|scan| syn::visit::visit_impl_item_fn(scan, item));
    }

    fn visit_trait_item_fn(&mut self, item: &'ast syn::TraitItemFn) {
        self.function(|scan| syn::visit::visit_trait_item_fn(scan, item));
    }

    fn visit_local(&mut self, local: &'ast syn::Local) {
        let name = match &local.pat {
            syn::Pat::Ident(ident) => Some(ident.ident.unraw().to_string()),
            syn::Pat::Type(typed) => match &*typed.pat {
                syn::Pat::Ident(ident) => Some(ident.ident.unraw().to_string()),
                _ => None,
            },
            _ => None,
        };
        // The name is the child's only when the binding holds the spawn
        // result itself, not something derived from it.
        let holds_child = local
            .init
            .as_ref()
            .is_some_and(|init| is_spawn_result(&init.expr));
        let name = if holds_child { name } else { None };
        let outer = std::mem::replace(&mut self.pending_name, name);
        syn::visit::visit_local(self, local);
        self.pending_name = outer;
    }

    fn visit_expr_while(&mut self, expr: &'ast syn::ExprWhile) {
        self.loop_depth += 1;
        syn::visit::visit_expr_while(self, expr);
        self.loop_depth -= 1;
    }

    fn visit_expr_loop(&mut self, expr: &'ast syn::ExprLoop) {
        self.loop_depth += 1;
        syn::visit::visit_expr_loop(self, expr);
        self.loop_depth -= 1;
    }

    fn visit_expr_for_loop(&mut self, expr: &'ast syn::ExprForLoop) {
        self.loop_depth += 1;
        syn::visit::visit_expr_for_loop(self, expr);
        self.loop_depth -= 1;
    }

    fn visit_expr_closure(&mut self, expr: &'ast syn::ExprClosure) {
        // A closure passed to `any`, `find_map` or a retry helper polls.
        self.loop_depth += 1;
        syn::visit::visit_expr_closure(self, expr);
        self.loop_depth -= 1;
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        if let Ok(arguments) = mac.parse_body_with(
            syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated,
        ) {
            for argument in &arguments {
                self.visit_expr(argument);
            }
        }
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path) = &*call.func {
            let helper = path.path.segments.last().is_some_and(|segment| {
                DRAIN_HELPERS.contains(&segment.ident.unraw().to_string().as_str())
            });
            if helper {
                let at = position(call.func.span());
                let taken: Vec<_> = call.args.iter().flat_map(takes_in).collect();
                for (name, stream) in taken {
                    self.drained(&name, stream, at);
                }
            }
        }
        syn::visit::visit_expr_call(self, call);
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        let name = call.method.unraw().to_string();
        let at = position(call.method.span());
        match name.as_str() {
            "spawn" if call.args.is_empty() => {
                let mut piped = BTreeSet::new();
                let mut chain = &*call.receiver;
                while let syn::Expr::MethodCall(link) = chain {
                    if let Some(stream) = stream(&link.method) {
                        if link.args.len() == 1 && is_piped(&link.args[0]) {
                            piped.insert(stream);
                        }
                    }
                    chain = &link.receiver;
                }
                let bound = self.pending_name.take();
                if let Some(frame) = self.frames.last_mut() {
                    if !piped.is_empty() {
                        frame.children.push(Child {
                            name: bound,
                            piped,
                            line: at.0,
                            drained: BTreeMap::new(),
                        });
                    }
                }
            }
            "read_to_end" | "read_to_string" => {
                for (child, stream) in fields_in(&call.receiver) {
                    self.drained(&child, stream, at);
                }
            }
            "wait" if call.args.is_empty() => {
                if let Some(frame) = self.frames.last_mut() {
                    frame.waits.push(Wait {
                        name: path_name(&call.receiver),
                        at,
                        what: "`.wait()`",
                    });
                }
            }
            "try_wait" if self.loop_depth > 0 => {
                if let Some(frame) = self.frames.last_mut() {
                    frame.waits.push(Wait {
                        name: path_name(&call.receiver),
                        at,
                        what: "a `.try_wait()` poll",
                    });
                }
            }
            "wait_with_output" => {
                if let Some(frame) = self.frames.last_mut() {
                    frame.with_output.push(path_name(&call.receiver));
                }
            }
            _ => {}
        }
        syn::visit::visit_expr_method_call(self, call);
    }
}

/// Every undrained wait in `source`, as (line, what). `whole_file` marks all
/// of it as test code.
fn undrained_waits(source: &str, whole_file: bool) -> Vec<(usize, String)> {
    let file = syn::parse_file(source).expect("the source parses");
    let mut scan = Scan::default();
    if whole_file || marked_test(&file.attrs) {
        scan.test_depth = 1;
    }
    scan.visit_file(&file);
    scan.found.sort();
    scan.found
}

/// `file:line: what` for every offender under `root/crates`.
fn offenders_in(root: &Path) -> Vec<String> {
    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);
    files.sort();
    let mut offenders = Vec::new();
    for file in files {
        let relative = file
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let text = std::fs::read_to_string(&file).unwrap();
        let found = std::panic::catch_unwind(|| undrained_waits(&text, test_file(&relative)))
            .unwrap_or_else(|_| panic!("{relative} does not parse as Rust"));
        for (line, what) in found {
            offenders.push(format!("{relative}:{line}: {what}"));
        }
    }
    offenders
}

#[test]
fn no_test_waits_for_a_child_before_draining_its_piped_output() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root = root.canonicalize().unwrap();
    assert!(
        std::fs::read_dir(root.join("crates/codeflow-cli/tests")).is_ok(),
        "the scan found the workspace tests"
    );
    let offenders = offenders_in(&root);
    assert!(
        offenders.is_empty(),
        "drain a piped child's stdout and stderr with drain_pipe or drain_lines, or read each \
         to the end, before waiting or polling; see the doc comment of \
         crates/codeflow-core/tests/pipe_drain_rule.rs; found:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn the_scan_flags_each_wait_that_leaves_piped_output_unread() {
    for (source, what) in [
        (
            "fn f() { let mut c = Command::new(\"x\").stdout(Stdio::piped()).spawn().unwrap();\n\
             while c.try_wait().unwrap().is_none() {} }",
            "a polling loop",
        ),
        (
            "fn f() { let mut c = Command::new(\"x\").stderr(std::process::Stdio::piped()).spawn().unwrap();\n\
             loop { if c.try_wait().unwrap().is_some() { break; } } }",
            "a loop with a qualified path",
        ),
        (
            "fn f() { let mut c = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();\n\
             drain_pipe(c.stdout.take());\n\
             for _ in 0..9 { c.try_wait().unwrap(); } }",
            "stderr piped and only stdout drained",
        ),
        (
            "fn f() { let mut c = cmd.stdout(Stdio::piped()).spawn().unwrap(); c.wait().unwrap(); }",
            "a blocking wait",
        ),
        (
            "fn f() { let mut c = cmd.stdout(Stdio::piped()).spawn().unwrap();\n\
             let out = c.stdout.as_mut().unwrap(); c.wait().unwrap(); }",
            "a stream borrowed with as_mut and not read",
        ),
        (
            "fn f() { let mut c = cmd.stdout(Stdio::piped()).spawn().unwrap();\n\
             let out = c.stdout.take(); c.wait().unwrap(); }",
            "a stream taken into a variable and not read",
        ),
        (
            "fn f() { let mut c = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();\n\
             let a = c.stdout.take(); let b = c.stderr.take();\n\
             while c.try_wait().unwrap().is_none() {} }",
            "both streams taken and not read",
        ),
        (
            "fn f() { let mut a = x.stdout(Stdio::piped()).spawn().unwrap();\n\
             let mut b = y.stdout(Stdio::piped()).spawn().unwrap();\n\
             drain_pipe(b.stdout.take()); a.wait().unwrap(); }",
            "another child's drain does not excuse this one",
        ),
        (
            "fn f() { let mut c = cmd.stdout(Stdio::piped()).spawn().unwrap();\n\
             c.wait().unwrap(); drain_pipe(c.stdout.take()); }",
            "a drain after the wait",
        ),
        (
            "fn f() { let mut c = cmd.stdout(Stdio::piped()).spawn().unwrap();\n\
             while c.try_wait().unwrap().is_none() {}\n\
             let out = c.wait_with_output().unwrap(); }",
            "wait_with_output after a poll",
        ),
        (
            "fn f() { let mut c = cmd.stdout(Stdio::piped()).spawn().unwrap();\n\
             let done = (0..9).any(|_| c.try_wait().unwrap().is_some()); }",
            "a polling closure",
        ),
        (
            "fn f() { let mut c = cmd.stdout(Stdio::piped()).spawn().unwrap();\n\
             assert!(c.wait().unwrap().success(), \"failed\"); }",
            "a wait inside assert!",
        ),
        (
            "fn spawn() -> Child { let c = cmd.stdout(Stdio::piped()).spawn().unwrap(); c }",
            "a helper that returns a child whose output nobody reads",
        ),
        (
            "fn f() { cmd.stdout(Stdio::piped()).spawn().unwrap().wait().unwrap(); }",
            "a chained wait on an unnamed child",
        ),
        (
            "#[test] fn t() { let mut c = cmd.stdout(Stdio::piped()).spawn().unwrap(); c.wait().unwrap(); }",
            "a #[test] function outside a test module",
        ),
        (
            "#[cfg(test)] mod tests { fn t() { let mut c = cmd.stdout(Stdio::piped()).spawn().unwrap();\n\
             c.wait().unwrap(); } }",
            "a cfg(test) module",
        ),
        (
            "struct S; impl S { fn f(&self) { let mut c = cmd.r#stdout(Stdio::piped()).spawn().unwrap();\n\
             c.wait().unwrap(); } }",
            "a raw identifier in a method",
        ),
    ] {
        let whole_file = !source.starts_with("#[test]") && !source.starts_with("#[cfg(test)]");
        assert_eq!(
            undrained_waits(source, whole_file).len(),
            1,
            "{what}: {source}"
        );
    }
}

#[test]
fn the_scan_passes_output_that_is_drained_discarded_or_read_by_the_call() {
    for (source, what) in [
        (
            "fn f() { let mut c = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();\n\
             let a = drain_pipe(c.stdout.take()); let b = drain_lines(c.stderr.take());\n\
             while c.try_wait().unwrap().is_none() {} }",
            "both handed to the drain helpers",
        ),
        (
            "fn f() { let mut c = cmd.stdout(Stdio::piped()).spawn().unwrap();\n\
             let mut bytes = Vec::new();\n\
             c.stdout.take().unwrap().read_to_end(&mut bytes).unwrap(); c.wait().unwrap(); }",
            "a chained read to the end",
        ),
        (
            "fn f() { let mut c = cmd.stdout(Stdio::piped()).spawn().unwrap();\n\
             let mut text = String::new();\n\
             c.stdout.as_mut().unwrap().read_to_string(&mut text).unwrap(); c.wait().unwrap(); }",
            "a read to the end through as_mut",
        ),
        (
            "fn spawn(mut cmd: Command) -> (Child, Receiver<String>, JoinHandle<Vec<u8>>) {\n\
             let mut child = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();\n\
             let out = drain_pipe(child.stdout.take()); let rx = drain_lines(child.stderr.take());\n\
             (child, rx, out) }",
            "a helper that drains both streams and returns the child",
        ),
        (
            "fn f() { let mut c = cmd.stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();\n\
             while c.try_wait().unwrap().is_none() {} c.wait().unwrap(); }",
            "null output",
        ),
        (
            "fn f() { let mut c = cmd.stdout(file).spawn().unwrap(); c.wait().unwrap(); }",
            "output to a file",
        ),
        (
            "fn f() { let mut c = cmd.stdin(Stdio::piped()).spawn().unwrap(); c.wait().unwrap(); }",
            "stdin piped, output inherited",
        ),
        (
            "fn f() { let c = cmd.stdout(Stdio::piped()).spawn().unwrap(); c.wait_with_output().unwrap(); }",
            "wait_with_output alone",
        ),
        (
            "fn f() { let mut c = cmd.stdout(Stdio::piped()).spawn().unwrap();\n\
             assert!(c.try_wait().unwrap().is_none()); let _ = c.wait_with_output(); }",
            "one liveness check, then wait_with_output",
        ),
        (
            "fn f() { let blob = cmd.stdout(Stdio::piped()).spawn()\n\
             .and_then(|mut child| child.wait_with_output()).unwrap(); }",
            "an unnamed child read by wait_with_output in a closure",
        ),
        (
            "fn f() { let out = cmd.stdout(Stdio::piped()).output().unwrap(); }",
            "output()",
        ),
        (
            "fn f() { let out = a.stdout(Stdio::piped()).stderr(Stdio::piped()).output().unwrap();\n\
             let mut c = b.spawn().unwrap(); c.wait().unwrap(); }",
            "independent children: output() on one command, wait() on another",
        ),
        (
            "fn f() { let mut a = x.stdout(Stdio::piped()).spawn().unwrap();\n\
             let mut b = y.spawn().unwrap();\n\
             drain_pipe(a.stdout.take()); a.wait().unwrap(); b.wait().unwrap(); }",
            "a drained child beside an unpiped one",
        ),
        (
            "fn f() { let mut a = x.stdout(Stdio::piped()).spawn().unwrap();\n\
             let mut b = y.stdout(Stdio::piped()).spawn().unwrap();\n\
             drain_pipe(a.stdout.take()); drain_pipe(b.stdout.take());\n\
             a.wait().unwrap(); b.wait().unwrap(); }",
            "two children each drained",
        ),
        (
            "fn f() { let b = std::sync::Barrier::new(2); b.wait(); }",
            "a barrier wait",
        ),
        (
            "fn f() { let mut c = cmd.stdout(Stdio::piped()).spawn().unwrap(); c.wait().unwrap(); }",
            "production code outside any test item",
        ),
    ] {
        let whole_file = what != "production code outside any test item";
        assert!(
            undrained_waits(source, whole_file).is_empty(),
            "{what}: {source}"
        );
    }
}

#[test]
fn a_planted_undrained_loop_fails_the_tree_scan() {
    let dir = tempfile::tempdir().unwrap();
    let tests = dir.path().join("crates/planted/tests");
    std::fs::create_dir_all(&tests).unwrap();
    std::fs::write(
        tests.join("undrained.rs"),
        "use std::process::{Command, Stdio};\n\
         #[test]\n\
         fn planted() {\n\
         \x20   let mut child = Command::new(\"true\").stdout(Stdio::piped()).spawn().unwrap();\n\
         \x20   let out = child.stdout.take();\n\
         \x20   while child.try_wait().unwrap().is_none() {}\n\
         }\n",
    )
    .unwrap();
    std::fs::write(
        tests.join("drained.rs"),
        "use std::process::{Command, Stdio};\n\
         #[test]\n\
         fn drained() {\n\
         \x20   let mut child = Command::new(\"true\").stdout(Stdio::piped()).spawn().unwrap();\n\
         \x20   let out = drain_pipe(child.stdout.take());\n\
         \x20   while child.try_wait().unwrap().is_none() {}\n\
         }\n",
    )
    .unwrap();
    let offenders = offenders_in(dir.path());
    assert_eq!(offenders.len(), 1, "{offenders:?}");
    assert!(
        offenders[0].starts_with("crates/planted/tests/undrained.rs:6:"),
        "{offenders:?}"
    );
}
