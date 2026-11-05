//! The portal validator confirms copy repairs; it does not edit or build pages.

use super::*;
use codeflow_core::scaffold::sha256_hex;

#[test]
fn confirms_portal_copy() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    portal_fixture(root, "Read at 13:41.");
    let before = codeflow(root, &["validate", "--portal", "portal"]);
    let warning = block(&before, "copy warning(s)");
    assert_prints_row(warning, "PORTAL_COPY");
    let step = printed_command(warning, "PORTAL_COPY", None);
    let source_before = read(root, "docs/Mixed Case + café.md");
    let built_before = read(root, "portal/dist/reference/Mixed Case + café/index.html");
    let confirmed = run_printed(root, &step, &[("<dir>", "portal")], &[]);
    assert!(confirmed.contains("copy-time-zone"), "{confirmed}");
    assert_prints_row(block(&confirmed, "copy-time-zone"), "PORTAL_COPY");
    assert_eq!(source_before, read(root, "docs/Mixed Case + café.md"));
    assert_eq!(
        built_before,
        read(root, "portal/dist/reference/Mixed Case + café/index.html")
    );

    // Model the author's source edit and rebuilt artifacts. No portal build
    // runs here: the real validator checks this commit-anchored fixture.
    portal_fixture(root, "Read at 13:41 UTC.");
    let repaired = run_printed(root, &step, &[("<dir>", "portal")], &[]);
    assert!(repaired.contains("1 page(s) clean"), "{repaired}");
    assert!(!repaired.contains("warning:"), "{repaired}");
}

// Write and commit the source inputs before creating derived artifacts.
// Returns the commit, source hash and configuration hash.
fn prepare_portal_source(root: &Path, copy: &str) -> (String, String, String) {
    for directory in [
        ".codeflow",
        "docs",
        "portal/.portal/generated",
        "portal/src/content/docs/reference",
        "portal/public/markdown/reference",
        "portal/dist/reference/Mixed Case + café",
    ] {
        std::fs::create_dir_all(root.join(directory)).unwrap();
    }
    let config = br#"{
          "schema_version": 1,
          "title": "Fixture guide",
          "description": "Complete public validator fixture",
          "theme": "signal",
          "repository_url": null,
          "repository_root": "..",
          "release_version": null,
          "primitive_tokens": null,
          "source_roots": ["docs"],
          "exclude": [],
          "layers": [
            {"id":"orient","label":"Orient","description":"Start","paths":[]},
            {"id":"system","label":"System","description":"Decisions","prefixes":["docs/decisions"]},
            {"id":"reference","label":"Reference","description":"Other","fallback":true}
          ],
          "base": "/"
        }"#;
    let source = format!("# Guide\n\n{copy}\n\n## Outcome\n\n[Jump](./Mixed%20Case%20%2B%20caf%C3%A9.md?view=1#outcome)\n");
    let source = source.as_bytes();
    std::fs::write(root.join("portal/portal.config.json"), config).unwrap();
    std::fs::create_dir_all(root.join("portal/scripts")).unwrap();
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs-portal/scripts/runtime-scripts.json"),
        root.join("portal/scripts/runtime-scripts.json"),
    )
    .unwrap();

    std::fs::write(root.join("docs/Mixed Case + café.md"), source).unwrap();
    for args in [
        &["init", "-q"][..],
        &["config", "user.email", "portal-tests@codeflow.invalid"][..],
        &["config", "user.name", "Portal tests"][..],
        &["config", "core.autocrlf", "false"][..],
        &[
            "add",
            "docs",
            "portal/portal.config.json",
            "portal/scripts/runtime-scripts.json",
        ][..],
        &["commit", "-q", "-m", "fixture"][..],
    ] {
        git(root, args);
    }
    let commit_output = run("git", root, &["rev-parse", "HEAD"]);
    assert!(commit_output.status.success(), "{}", text(&commit_output));
    let commit = String::from_utf8(commit_output.stdout)
        .unwrap()
        .trim()
        .to_owned();
    (commit, sha256_hex(source), sha256_hex(config))
}

// Based on the core validator's complete commit-anchored portal fixture.
// Recreate the provenance, Markdown twin and artifact hashes together.
fn portal_fixture(root: &Path, copy: &str) {
    let (commit, source_hash, config_hash) = prepare_portal_source(root, copy);
    let snippet_hash = sha256_hex(b"# Guide");
    let rendered = format!(
            "---\ntitle: \"Guide\"\n---\n\n<!-- codeflow-page-provenance source_sha256={source_hash} built_from_commit={commit} portal_version=1.0.0 release_version=none -->\n<!-- codeflow-source-snippet sha256={snippet_hash} lines=1-1 -->\n<div class=\"portal-provenance\">Source <code>docs/Mixed Case + café.md</code> at <code>{commit}</code></div>\n<div data-pagefind-body data-codeflow-search-root=\"reference/Mixed Case + café\">\n\n# Guide\n\n{copy}\n\n## Outcome\n\n[Jump](./Mixed%20Case%20%2B%20caf%C3%A9.md?view=1#outcome)\n\n</div>\n"
        );
    let rendered_hash = sha256_hex(rendered.as_bytes());
    for path in [
        "portal/src/content/docs/reference/Mixed Case + café.md",
        "portal/public/markdown/reference/Mixed Case + café.md",
    ] {
        std::fs::write(root.join(path), &rendered).unwrap();
    }
    let llms = format!(
            "# Fixture guide\n\nComplete public validator fixture\n\nRepository commit: {commit}\n\n- [reference/Mixed Case \\+ café](./markdown/reference/Mixed%20Case%20%2B%20caf%C3%A9.md) — docs/Mixed Case \\+ café\\.md\n"
        );
    std::fs::write(root.join("portal/public/llms.txt"), &llms).unwrap();
    // Copy checks read paragraphs inside the Starlight content region.
    // The core integrity fixture alone does not need this region.
    let built = format!("<main data-pagefind-body data-codeflow-search-root=\"reference/Mixed Case + café\"><h1>Guide</h1><div class=\"sl-markdown-content\"><p>{copy}</p></div><h2 id=\"outcome\">Outcome</h2><a href=\"/reference/Mixed%20Case%20%2B%20caf%C3%A9/?view=1#outcome\">Jump</a></main>\n");
    std::fs::write(
        root.join("portal/dist/reference/Mixed Case + café/index.html"),
        &built,
    )
    .unwrap();
    let adoption = serde_json::json!({
        "schema_version": 1,
        "root": "portal",
        "starter_version": "1.0.0",
        "files": {
            "portal.config.json": {
                "ownership": "user-owned",
                "pristine_sha256": config_hash
            }
        }
    });
    std::fs::write(
        root.join(".codeflow/docs-portal.json"),
        serde_json::to_vec(&adoption).unwrap(),
    )
    .unwrap();
    let evidence = serde_json::json!({
        "schema_version": 1,
        "generator": {"name": "@codeflow/docs-portal", "version": "1.0.0"},
        "repository": {"root": "..", "commit": commit, "release_version": null},
        "config_sha256": config_hash,
        "primitive_tokens": null,
        "media": [],
        "pages": [{
            "source_path": "docs/Mixed Case + café.md",
            "source_sha256": source_hash,
            "built_from_commit": commit,
            "route": "reference/Mixed Case + café",
            "title": "Guide",
            "status": null,
            "output_markdown": "src/content/docs/reference/Mixed Case + café.md",
            "output_markdown_sha256": rendered_hash,
            "markdown_twin": "public/markdown/reference/Mixed Case + café.md",
            "markdown_twin_sha256": rendered_hash,
            "stale": false,
            "searchable": true,
            "ids": [],
            "relationships": [],
            "backlinks": [],
            "snippets": [{"start_line": 1, "end_line": 1, "sha256": snippet_hash}],
            "stale_reason": null
        }],
        "llms": {"path": "public/llms.txt", "sha256": sha256_hex(llms.as_bytes())},
        "artifacts": [{
            "path": "dist/reference/Mixed Case + café/index.html",
            "sha256": sha256_hex(built.as_bytes())
        }]
    });
    std::fs::write(
        root.join("portal/.portal/generated/evidence.json"),
        serde_json::to_vec(&evidence).unwrap(),
    )
    .unwrap();
}
