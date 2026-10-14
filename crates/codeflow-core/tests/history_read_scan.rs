//! Keep judging parent and ancestry reads in the guarded reader (TSK-287).
//! This syntactic check does not prove error propagation or inspect Git
//! subprocess arguments. Planted-overlay tests exercise those boundaries.
use std::collections::BTreeMap;
use std::path::Path;
use syn::visit::Visit;

const READER: &str = "codeflow-core/src/workgraph/history.rs";
// These modules only report historical metrics or session context. None
// authorizes work, a landing, deletion or review coverage.
const DISPLAY: &[(&str, &str)] = &[
    (
        "codeflow-core/src/ceremony/history.rs",
        "ceremony metrics do not authorize a transition",
    ),
    (
        "codeflow-core/src/estimate/outcomes/history.rs",
        "historical estimates do not authorize a transition",
    ),
    (
        "codeflow-core/src/hooks/session_summary.rs",
        "session context is informational, never gate evidence",
    ),
    (
        "codeflow-core/src/root_checkout.rs",
        "ahead/behind counts are displayed in the checkout inventory",
    ),
];

#[derive(Default)]
struct Reads {
    found: Vec<String>,
}

fn test_only(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("cfg")
            && attr
                .parse_args::<syn::Path>()
                .is_ok_and(|path| path.is_ident("test"))
    })
}
impl<'ast> Visit<'ast> for Reads {
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
    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        let method = node.method.to_string();
        if matches!(
            method.as_str(),
            "parent_id"
                | "parent_ids"
                | "parent_count"
                | "parents"
                | "graph_descendant_of"
                | "merge_base"
                | "merge_commits"
                | "merge_base_many"
                | "merge_bases"
                | "graph_ahead_behind"
                | "revwalk"
        ) || (method == "parent" && !node.args.is_empty())
        {
            self.found.push(method);
        }
        syn::visit::visit_expr_method_call(self, node);
    }
}

fn scan(root: &Path, dir: &Path, found: &mut BTreeMap<String, Vec<String>>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            scan(root, &path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            let file = path
                .strip_prefix(root)
                .unwrap()
                .to_str()
                .unwrap()
                .replace('\\', "/");
            // These are externally declared fixture modules, not production.
            if file.ends_with("_tests.rs")
                || file.ends_with("/tests.rs")
                || file.contains("/tests/")
            {
                continue;
            }
            let syntax = syn::parse_file(&std::fs::read_to_string(&path).unwrap()).unwrap();
            let mut reads = Reads::default();
            reads.visit_file(&syntax);
            if !reads.found.is_empty() {
                found.insert(file, reads.found);
            }
        }
    }
}

#[test]
fn direct_history_reads_require_the_reader_or_a_display_reason() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut found = BTreeMap::new();
    for name in ["codeflow-core", "codeflow-cli", "codeflow-present"] {
        scan(root, &root.join(name).join("src"), &mut found);
    }
    assert!(
        found.remove(READER).is_some(),
        "reader moved: update the scan"
    );
    for (file, reason) in DISPLAY {
        assert!(!reason.is_empty());
        let calls = found
            .remove(*file)
            .unwrap_or_else(|| panic!("stale display exception: {file}"));
        let expected = match *file {
            "codeflow-core/src/ceremony/history.rs" => 6,
            "codeflow-core/src/estimate/outcomes/history.rs" => 12,
            "codeflow-core/src/hooks/session_summary.rs" => 2,
            "codeflow-core/src/root_checkout.rs" => 1,
            _ => unreachable!(),
        };
        assert_eq!(
            calls.len(),
            expected,
            "new direct history read in {file}: {reason}"
        );
        if file.ends_with("root_checkout.rs") {
            assert_eq!(
                calls,
                ["graph_ahead_behind"],
                "only display counts are excepted"
            );
        }
    }
    // `ContainedRoot::parent` walks a filesystem path, never Git history.
    assert_eq!(
        found.remove("codeflow-core/src/contained.rs").unwrap(),
        vec!["parent"; 8]
    );
    assert!(
        found.is_empty(),
        "unguarded judging history reads: {found:#?}"
    );
}

#[test]
fn scanner_detects_a_new_parent_read_and_skips_test_fixtures() {
    let source = syn::parse_file("fn judge() { commit.parent(0); commit.parent_ids(); repo.merge_base(a, b); } #[cfg(test)] mod tests { fn fixture() { commit.parent_count(); } }").unwrap();
    let mut reads = Reads::default();
    reads.visit_file(&source);
    assert_eq!(reads.found, ["parent", "parent_ids", "merge_base"]);
}
