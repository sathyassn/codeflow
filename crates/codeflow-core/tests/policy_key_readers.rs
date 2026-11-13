//! Source-derived guard against documented controls with no consumer.
//!
//! Parse production Rust field reads, excluding tests, assignments and defaults.
//! This is a structural contract, not proof of runtime reachability or Rust type
//! resolution. A field's comment, declaration or claimed module is not a read.
use std::collections::BTreeSet;
use std::path::Path;

use codeflow_core::hooks::policy_schema::schema;
use syn::visit::Visit;

#[derive(Default)]
struct Reads(BTreeSet<String>);

fn test_only(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("cfg")
            && attr
                .parse_args::<syn::Path>()
                .is_ok_and(|p| p.is_ident("test"))
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

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        if !test_only(&node.attrs)
            && node
                .trait_
                .as_ref()
                .is_none_or(|(_, p, _)| p.segments.last().is_none_or(|s| s.ident != "Default"))
        {
            syn::visit::visit_item_impl(self, node);
        }
    }

    fn visit_expr_assign(&mut self, node: &'ast syn::ExprAssign) {
        self.visit_expr(&node.right);
    }

    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        if node.method == "get" || node.method == "pointer" {
            if let Some(syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(key),
                ..
            })) = node.args.first()
            {
                self.0
                    .insert(key.value().rsplit('/').next().unwrap().to_string());
            }
        }
        syn::visit::visit_expr_method_call(self, node);
    }

    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        // A fixed refusal consumes the key's meaning without reading its value.
        if let syn::Expr::Path(path) = &*node.func {
            if path
                .path
                .segments
                .last()
                .is_some_and(|s| s.ident == "always_blocking")
            {
                if let Some(syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(key),
                    ..
                })) = node.args.first()
                {
                    self.0
                        .insert(key.value().rsplit('.').next().unwrap().to_string());
                }
            }
        }
        syn::visit::visit_expr_call(self, node);
    }

    fn visit_expr_field(&mut self, node: &'ast syn::ExprField) {
        if let syn::Member::Named(name) = &node.member {
            self.0.insert(name.to_string());
        }
        syn::visit::visit_expr_field(self, node);
    }
}

fn collect(dir: &Path, reads: &mut Reads) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_str().unwrap();
        if name == "tests"
            || name == "tests.rs"
            || name.ends_with("_tests.rs")
            || name == "policy_schema.rs"
        {
            continue;
        }
        if path.is_dir() {
            collect(&path, reads);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            let source = std::fs::read_to_string(&path).unwrap();
            if schema()
                .iter()
                .any(|key| source.contains(key.path.rsplit('.').next().unwrap()))
            {
                reads.visit_file(&syn::parse_file(&source).unwrap());
            }
        }
    }
}

fn workflow_reads(source: &str, reads: &mut Reads) {
    // Only executable run blocks count; prose, comments and job names do not.
    let workflow: serde_yaml::Value = serde_yaml::from_str(source).unwrap();
    let jq = regex::Regex::new(r"jq -er '\.git\.([a-z_]+)'").unwrap();
    let python = regex::Regex::new(r#"= git\.get\("([a-z_]+)","#).unwrap();
    for job in workflow["jobs"].as_mapping().unwrap().values() {
        for step in job["steps"].as_sequence().into_iter().flatten() {
            for line in step["run"].as_str().unwrap_or("").lines() {
                if line.trim_start().starts_with('#') {
                    continue;
                }
                for pattern in [&jq, &python] {
                    for capture in pattern.captures_iter(line) {
                        reads.0.insert(capture[1].to_string());
                    }
                }
            }
        }
    }
}

fn missing(reads: &Reads, documented: &str) -> Vec<&'static str> {
    schema()
        .iter()
        .filter(|key| documented.contains(&format!("`{}`", key.path)))
        .filter(|key| !key.is_unsupported())
        .filter(|key| !reads.0.contains(key.path.rsplit('.').next().unwrap()))
        .map(|key| key.path)
        .collect()
}

#[test]
fn every_documented_policy_key_has_a_consumer_or_is_unsupported() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut reads = Reads::default();
    for crate_name in ["codeflow-core", "codeflow-cli", "codeflow-present"] {
        collect(
            &root.join("crates").join(crate_name).join("src"),
            &mut reads,
        );
    }
    workflow_reads(
        &std::fs::read_to_string(root.join("assets/base/ci/codeflow-ci.yml")).unwrap(),
        &mut reads,
    );
    let docs = std::fs::read_to_string(root.join("docs/policy-reference.md")).unwrap();
    assert_eq!(missing(&reads, &docs), Vec::<&str>::new());
}

#[test]
fn keeping_the_claimed_module_but_disconnecting_the_read_is_refused() {
    let docs = "`security.privilege_escalation`";
    let mut reads = Reads::default();
    reads.visit_file(&syn::parse_file(
        "mod privilege { fn enforce(policy: &SecuritySection) -> PolicyLevel { policy.privilege_escalation } }"
    ).unwrap());
    assert!(missing(&reads, docs).is_empty());
    let mut disconnected = Reads::default();
    disconnected.visit_file(&syn::parse_file(
        r"mod privilege {
            // Reader of security.privilege_escalation.
            fn enforce(policy: &SecuritySection) -> PolicyLevel { PolicyLevel::Block }
            fn configure(policy: &mut SecuritySection) { policy.privilege_escalation = PolicyLevel::Block; }
            #[cfg(test)] mod tests { fn check(p: &SecuritySection) { check(p.privilege_escalation); } }
        }"
    ).unwrap());
    assert_eq!(
        missing(&disconnected, docs),
        ["security.privilege_escalation"]
    );
}

#[test]
fn workflow_names_and_comments_do_not_replace_consumers() {
    let docs = "`git.security_review` `git.dep_audit` `git.secret_scan_inline_allow`";
    let mut reads = Reads::default();
    workflow_reads(
        r#"
jobs:
  security-review:
    steps:
      - run: |
          sr=$(printf '%s' "$policy" | jq -er '.git.security_review')
          da=$(printf '%s' "$policy" | jq -er '.git.dep_audit')
          inline_allow = git.get("secret_scan_inline_allow", "trusted")
"#,
        &mut reads,
    );
    assert!(missing(&reads, docs).is_empty());
    let mut disconnected = Reads::default();
    workflow_reads(
        r#"
jobs:
  security-review:
    steps:
      - name: git.security_review and git.dep_audit
        run: |
          # sr=$(printf '%s' "$policy" | jq -er '.git.security_review')
          # da=$(printf '%s' "$policy" | jq -er '.git.dep_audit')
          # inline_allow = git.get("secret_scan_inline_allow", "trusted")
          sr=warn
          da=warn
          inline_allow="trusted"
"#,
        &mut disconnected,
    );
    assert_eq!(missing(&disconnected, docs).len(), 3);
}

#[test]
fn unsupported_keys_are_accepted_and_described_without_a_control_claim() {
    for path in codeflow_core::hooks::policy_schema::UNSUPPORTED_KEYS {
        let spec = codeflow_core::hooks::policy_schema::spec_for(path).expect("accepted key");
        assert!(
            spec.purpose.starts_with("Not supported, has no effect."),
            "{path}"
        );
    }
}
