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
//! reads, a stream the function sends to `Stdio::piped()` with `.stdout(..)`
//! or `.stderr(..)` must be taken from the child (`child.stdout.take()`,
//! `.as_mut()`, `.by_ref()`) when the same function also
//!
//! - calls `.wait()` with no arguments, or
//! - calls `.try_wait()` inside a loop or a closure (a poll).
//!
//! `wait_with_output` earns no credit: it reads the pipes only after the
//! function has already waited or polled. Output sent to `Stdio::null()`, to
//! a file or left inherited needs nothing, and `.output()` drains for itself.
//!
//! Limits, stated rather than hidden:
//!
//! - The check reads one function at a time. A helper that spawns with piped
//!   output and returns the child is judged by what it does itself; the caller
//!   that waits is judged by what it pipes. TSK-282's sweep table covers
//!   those splits by hand.
//! - Taking a stream counts as draining it. Reading it only after the wait is
//!   not seen.
//! - Output set through a variable (`.stdout(mode)`) or through
//!   `Command::stdout(&mut c, ..)` is not seen as piped.
//! - Macro bodies are read only when they parse as comma-separated
//!   expressions (`assert!`, `assert_eq!`, `vec!`); others, and
//!   `macro_rules!` bodies, are not parsed.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use syn::ext::IdentExt;
use syn::visit::Visit;

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

/// What one function does with a child and its output.
#[derive(Default)]
struct Frame {
    piped: BTreeSet<&'static str>,
    taken: BTreeSet<&'static str>,
    /// (line, what) of each wait or poll.
    waits: Vec<(usize, &'static str)>,
    in_test: bool,
}

#[derive(Default)]
struct Scan {
    frames: Vec<Frame>,
    /// Open test-item markers: how deep inside test items the visit is.
    test_depth: usize,
    /// Open loops and closures around the current expression.
    loop_depth: usize,
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

    fn enter_function(&mut self) {
        // A nested function starts outside its parent's loops.
        self.loop_depth = 0;
        self.frames.push(Frame {
            in_test: self.test_depth > 0,
            ..Frame::default()
        });
    }

    fn leave_function(&mut self, outer_loops: usize) {
        let frame = self.frames.pop().unwrap();
        self.loop_depth = outer_loops;
        if !frame.in_test {
            return;
        }
        let undrained: Vec<&str> = frame
            .piped
            .iter()
            .filter(|stream| !frame.taken.contains(*stream))
            .copied()
            .collect();
        if undrained.is_empty() {
            return;
        }
        if let Some((line, what)) = frame.waits.first() {
            self.found.push((
                *line,
                format!(
                    "{what} while the child's piped {} is not drained",
                    undrained.join(" and ")
                ),
            ));
        }
    }

    fn function(&mut self, visit: impl FnOnce(&mut Self)) {
        let outer_loops = self.loop_depth;
        self.enter_function();
        visit(self);
        self.leave_function(outer_loops);
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

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        let name = call.method.unraw().to_string();
        if let Some(frame) = self.frames.last_mut() {
            let line = call.method.span().start().line;
            match name.as_str() {
                "stdout" | "stderr" if call.args.len() == 1 && is_piped(&call.args[0]) => {
                    if let Some(stream) = stream(&call.method) {
                        frame.piped.insert(stream);
                    }
                }
                "take" | "as_mut" | "by_ref" | "as_deref_mut" => {
                    if let syn::Expr::Field(field) = &*call.receiver {
                        if let syn::Member::Named(member) = &field.member {
                            if let Some(stream) = stream(member) {
                                frame.taken.insert(stream);
                            }
                        }
                    }
                }
                "wait" if call.args.is_empty() => {
                    frame.waits.push((line, "`.wait()`"));
                }
                "try_wait" if self.loop_depth > 0 => {
                    frame.waits.push((line, "a `.try_wait()` poll"));
                }
                _ => {}
            }
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
        "drain a piped child's stdout and stderr (take each handle and read it on its own \
         thread) or send them to files before waiting or polling; see the doc comment of \
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
             let _ = c.stdout.take(); for _ in 0..9 { c.try_wait().unwrap(); } }",
            "stderr piped and stdout taken",
        ),
        (
            "fn f() { let mut c = cmd.stdout(Stdio::piped()).spawn().unwrap(); c.wait().unwrap(); }",
            "a blocking wait",
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
             let a = c.stdout.take(); let b = c.stderr.take();\n\
             while c.try_wait().unwrap().is_none() {} }",
            "both taken",
        ),
        (
            "fn f() { let mut c = cmd.stdout(Stdio::piped()).spawn().unwrap();\n\
             let a = c.stdout.as_mut().unwrap(); c.wait().unwrap(); }",
            "as_mut",
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
            "fn f() { let out = cmd.stdout(Stdio::piped()).output().unwrap(); }",
            "output()",
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
fn the_scan_judges_each_function_by_its_own_body() {
    let source = "fn spawn() -> Child { cmd.stdout(Stdio::piped()).spawn().unwrap() }\n\
        fn waits(c: &mut Child) { while c.try_wait().unwrap().is_none() {} }\n\
        fn both() { let mut c = cmd.stdout(Stdio::piped()).spawn().unwrap();\n\
        fn inner() { let mut d = cmd.stdout(Stdio::null()).spawn().unwrap(); d.wait().unwrap(); }\n\
        let _ = c.stdout.take(); c.wait().unwrap(); }";
    assert!(undrained_waits(source, true).is_empty());
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
         \x20   let out = child.stdout.take();\n\
         \x20   while child.try_wait().unwrap().is_none() {}\n\
         }\n",
    )
    .unwrap();
    let offenders = offenders_in(dir.path());
    assert_eq!(offenders.len(), 1, "{offenders:?}");
    assert!(
        offenders[0].starts_with("crates/planted/tests/undrained.rs:5:"),
        "{offenders:?}"
    );
}
