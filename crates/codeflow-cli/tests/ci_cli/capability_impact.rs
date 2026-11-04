//! Capability admission through the real CLI and immutable fixture commits.
use std::path::Path;
use std::process::{Command, Output};

const FILE: &str = "docs/capabilities/CAP-101-catalog.md";
const RECORD: &str =
    include_str!("../../../codeflow-core/tests/fixtures/capability-catalog/valid.md");
const NONE: &str =
    "## Capability impact\n\n- None: Internal refactoring preserves the catalog contract.\n";
const CHANGED: &str =
    "## Capability impact\n\n- Changed CAP-101 catalog: Readers see the revised contract.\n";
const ADDED: &str =
    "## Capability impact\n\n- Added CAP-101 catalog: Readers can inspect their capabilities.\n";
const IMPROVED: &str = "## Capability impact\n\n- Improved CAP-101 catalog: Readers get clearer guidance, entry unchanged.\n";

fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn commit(root: &Path) -> String {
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "feat: update fixture"]);
    git(root, &["rev-parse", "HEAD"])
}

fn policy(level: Option<&str>) -> String {
    let mut git = serde_json::Map::new();
    for key in codeflow_core::hooks::LEVEL_KEYS {
        let level = if matches!(*key, "work_records" | "work_planning") {
            "warn"
        } else {
            "off"
        };
        git.insert((*key).into(), serde_json::json!(level));
    }
    git.remove("pr_capability_impact");
    if let Some(level) = level {
        git.insert("pr_capability_impact".into(), serde_json::json!(level));
    }
    git.insert("product_paths".into(), serde_json::json!(["src/**"]));
    git.insert(
        "breaking_watch_paths".into(),
        serde_json::json!(["docs/api.md"]),
    );
    serde_json::json!({"git": git}).to_string()
}

fn project(tracked: bool) -> String {
    format!("schema_version = 1\ntier = '{}'\nscaffold_version = '4.0.0'\nstack = 'rust'\nareas = ['engine']\npolicy_armed = true\ngit_hooks = 'unwired'\npermission_preset = 'safe'\n", if tracked { "full" } else { "minimal" })
}

fn fixture(level: Option<&str>, catalog: bool, tracked: bool) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-qb", "main"]);
    git(root, &["config", "user.name", "Fixture"]);
    git(root, &["config", "user.email", "fixture@example.test"]);
    write(root, ".codeflow/policy.json", &policy(level));
    write(root, ".codeflow/project.toml", &project(tracked));
    write(root, "docs/capabilities/vocabulary.yaml", "inventory: incomplete\ngroups: [{name: records, about: Records}]\ntags: [{name: rust, about: Rust}]\n");
    if catalog {
        write(root, FILE, RECORD);
    }
    write(root, "src/lib.rs", "pub fn example() {}\n");
    commit(root);
    git(root, &["switch", "-qc", "feat/example"]);
    dir
}

fn product_change(root: &Path) {
    write(root, "src/lib.rs", "pub fn revised() {}\n");
    commit(root);
}

fn ci(root: &Path, body: Option<&str>, extra: &[&str]) -> Output {
    let mut args = vec![
        "ci",
        "--base",
        "main",
        "--head",
        "HEAD",
        "--branch",
        "feat/example",
    ];
    if let Some(body) = body {
        args.extend(["--pr-body", body]);
    }
    args.extend(extra);
    super::run_in(root, &args)
}

fn said(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn pass(out: &Output) {
    assert_eq!(out.status.code(), Some(0), "{}", said(out));
}

fn refuse(out: &Output, message: &str) {
    let text = said(out);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(
        text.contains("git.pr_capability_impact") && text.contains(message),
        "{text}"
    );
}

#[test]
fn capability_activation_and_bootstrap() {
    for level in [None, Some("off"), Some("warn"), Some("block")] {
        let dir = fixture(level, true, false);
        product_change(dir.path());
        let out = ci(dir.path(), None, &[]);
        let text = said(&out);
        if level == Some("block") {
            refuse(&out, "PR body unavailable");
        } else {
            pass(&out);
        }
        if level == Some("warn") {
            assert!(
                text.contains("warning") && text.contains("not verified"),
                "{text}"
            );
        }
        if level.is_none() {
            assert!(text.contains("absent at the policy authority"), "{text}");
        }
    }
    let dir = fixture(Some("block"), false, false);
    product_change(dir.path());
    refuse(&ci(dir.path(), Some(NONE), &[]), "bootstrap");
    write(dir.path(), FILE, RECORD);
    commit(dir.path());
    pass(&ci(dir.path(), Some(ADDED), &[]));
}

#[test]
fn capability_missing_body_and_section_at_both_levels_and_tracking_states() {
    for tracked in [false, true] {
        for level in ["warn", "block"] {
            let dir = fixture(Some(level), true, tracked);
            product_change(dir.path());
            for body in [None, Some("## Summary\n\nA change.\n")] {
                let out = ci(dir.path(), body, &[]);
                let text = said(&out);
                assert!(text.contains("not verified"), "{text}");
                if level == "block" {
                    refuse(
                        &out,
                        if body.is_none() {
                            "PR body unavailable"
                        } else {
                            "Capability impact"
                        },
                    );
                } else {
                    pass(&out);
                }
            }
        }
    }
}

#[test]
fn capability_delta_verdicts_and_line_context() {
    for (replacement, body, succeeds, label) in [
        (
            RECORD.replace("status: shipped", "status: building"),
            CHANGED,
            true,
            "frontmatter",
        ),
        (
            RECORD.replace("status: shipped", "status: building"),
            IMPROVED,
            false,
            "frontmatter improved",
        ),
        (
            RECORD.replace("name: catalog", "name:   catalog"),
            NONE,
            true,
            "whitespace",
        ),
        (
            RECORD.replace("describes the product", "explains the product"),
            CHANGED,
            true,
            "prose changed",
        ),
        (
            RECORD.replace("describes the product", "explains the product"),
            IMPROVED,
            true,
            "prose improved",
        ),
        (RECORD.to_owned(), IMPROVED, true, "unchanged improved"),
        (RECORD.to_owned(), CHANGED, false, "unchanged changed"),
        (RECORD.to_owned(), ADDED, false, "unchanged added"),
    ] {
        let dir = fixture(Some("block"), true, false);
        write(dir.path(), FILE, &replacement);
        product_change(dir.path());
        let out = ci(dir.path(), Some(body), &[]);
        assert_eq!(
            out.status.code(),
            Some(i32::from(!succeeds)),
            "{label}: {}",
            said(&out)
        );
    }
}

#[test]
fn capability_declarations_require_ids_names_context_and_unique_verbs() {
    let dir = fixture(Some("block"), false, false);
    write(dir.path(), FILE, RECORD);
    commit(dir.path());
    for line in [
        "CAP-101",
        "Added 1 capability: Readers benefit.",
        "Added CAP-101: Readers benefit.",
        "Added CAP-101 catalog:",
        "Added CAP-999 catalog: Readers benefit.",
        "Added CAP-101 wrong name: Readers benefit.",
        "Added CAP-101 catalog: Readers benefit.\n- Changed CAP-101 catalog: Readers benefit.",
    ] {
        refuse(
            &ci(
                dir.path(),
                Some(&format!("## Capability impact\n\n- {line}\n")),
                &[],
            ),
            "capability",
        );
    }
    pass(&ci(dir.path(), Some(ADDED), &[]));
}

#[test]
fn capability_rename_removal_replacement_and_undeclared_edit() {
    let dir = fixture(Some("block"), true, false);
    git(
        dir.path(),
        &["mv", FILE, "docs/capabilities/CAP-101-renamed.md"],
    );
    commit(dir.path());
    refuse(
        &ci(dir.path(), Some(NONE), &[]),
        "must match the record filename",
    );
    write(
        dir.path(),
        "docs/capabilities/CAP-101-renamed.md",
        &RECORD.replace("name: catalog", "name: renamed"),
    );
    commit(dir.path());
    pass(&ci(dir.path(), Some("## Capability impact\n\n- Changed CAP-101 renamed: Readers see the new capability name.\n"), &[]));
    git(dir.path(), &["rm", "docs/capabilities/CAP-101-renamed.md"]);
    commit(dir.path());
    refuse(&ci(dir.path(), Some(NONE), &[]), "identity removed");
    let dir = fixture(Some("block"), true, false);
    write(dir.path(), FILE, &RECORD.replace("3b9e7c2a", "4b9e7c2a"));
    commit(dir.path());
    refuse(&ci(dir.path(), Some(CHANGED), &[]), "identity removed");
    let dir = fixture(Some("block"), true, false);
    write(
        dir.path(),
        FILE,
        &RECORD.replace("status: shipped", "status: building"),
    );
    commit(dir.path());
    refuse(&ci(dir.path(), Some(NONE), &[]), "None cannot");
}

#[test]
fn capability_integration_ranges_use_net_delta() {
    let dir = fixture(Some("block"), false, false);
    write(dir.path(), FILE, RECORD);
    commit(dir.path());
    write(
        dir.path(),
        FILE,
        &RECORD.replace("status: shipped", "status: building"),
    );
    commit(dir.path());
    pass(&ci(dir.path(), Some(ADDED), &[]));
    let dir = fixture(Some("block"), true, false);
    write(
        dir.path(),
        FILE,
        &RECORD.replace("status: shipped", "status: building"),
    );
    commit(dir.path());
    write(dir.path(), FILE, RECORD);
    write(
        dir.path(),
        ".release/entries/example.md",
        "Capability: CAP-101 improved\n",
    );
    commit(dir.path());
    refuse(&ci(dir.path(), Some(CHANGED), &[]), "disagrees");
    pass(&ci(dir.path(), Some(IMPROVED), &[]));
}

#[test]
fn capability_authority_and_catalogs_ignore_the_checkout() {
    let dir = fixture(Some("block"), true, false);
    let root = dir.path();
    write(root, ".codeflow/policy.json", &policy(Some("off")));
    product_change(root);
    let head = git(root, &["rev-parse", "HEAD"]);
    git(root, &["switch", "-q", "main"]);
    let out = super::run_in(
        root,
        &[
            "ci",
            "--base",
            "main",
            "--head",
            &head,
            "--branch",
            "feat/example",
        ],
    );
    refuse(&out, "PR body unavailable");
    // Uncommitted edits in the checkout are not inputs: a broken record, an
    // unparsable new record and a policy that turns the rule off leave both
    // verdicts unchanged.
    write(root, FILE, "---\nid: not-a-capability\n---\n");
    write(
        root,
        "docs/capabilities/CAP-102-broken.md",
        "not frontmatter\n",
    );
    write(root, ".codeflow/policy.json", &policy(Some("off")));
    let out = super::run_in(
        root,
        &[
            "ci",
            "--base",
            "main",
            "--head",
            &head,
            "--branch",
            "feat/example",
        ],
    );
    refuse(&out, "PR body unavailable");
    let out = super::run_in(
        root,
        &[
            "ci",
            "--base",
            "main",
            "--head",
            &head,
            "--branch",
            "feat/example",
            "--pr-body",
            NONE,
        ],
    );
    pass(&out);
}

#[test]
fn capability_head_cannot_narrow_authority_scope() {
    let dir = fixture(Some("block"), true, false);
    let root = dir.path();
    let mut narrowed: serde_json::Value = serde_json::from_str(&policy(Some("block"))).unwrap();
    narrowed["git"]["breaking_watch_paths"] = serde_json::json!([]);
    write(root, ".codeflow/policy.json", &narrowed.to_string());
    commit(root);
    // The authority remains the exact target, even when scope is changed in
    // the checkout and the assessed range itself contains only Markdown.
    let base = git(root, &["rev-parse", "HEAD"]);
    let authority = git(root, &["rev-parse", "main"]);
    write(root, "docs/api.md", "Changed API contract.\n");
    commit(root);
    let out = super::run_in(
        root,
        &[
            "ci",
            "--base",
            &base,
            "--policy-from",
            &authority,
            "--head",
            "HEAD",
            "--branch",
            "feat/example",
        ],
    );
    refuse(&out, "PR body unavailable");
}

#[test]
fn capability_unbound_legacy_conversion_fails_closed() {
    let dir = fixture(Some("block"), false, false);
    let root = dir.path();
    git(root, &["switch", "-q", "main"]);
    write(
        root,
        "docs/capabilities.md",
        "```yaml\nid: CAP-101\nname: catalog\narea: engine\nstatus: planned\n```\n",
    );
    commit(root);
    git(root, &["switch", "-q", "feat/example"]);
    git(root, &["merge", "--ff-only", "main"]);
    write(root, "docs/capabilities.md", "# Catalog\n");
    write(root, FILE, RECORD);
    commit(root);
    refuse(
        &ci(root, Some(CHANGED), &[]),
        "cannot pin capability registry",
    );
    std::fs::remove_file(root.join(FILE)).unwrap();
    commit(root);
    refuse(
        &ci(root, Some(NONE), &[]),
        "deprecate it instead of deleting it",
    );
}

#[test]
fn capability_executable_authority_config_is_only_read_as_data() {
    let dir = fixture(Some("block"), true, false);
    let root = dir.path();
    git(root, &["switch", "-q", "main"]);
    let blob = git(root, &["rev-parse", "HEAD:.codeflow/project.toml"]);
    git(
        root,
        &[
            "update-index",
            "--cacheinfo",
            &format!("100755,{blob},.codeflow/project.toml"),
        ],
    );
    git(
        root,
        &["commit", "-qm", "chore: preserve configuration mode"],
    );
    git(root, &["restore", "--worktree", ".codeflow/project.toml"]);
    git(root, &["switch", "-q", "feat/example"]);
    git(root, &["merge", "--ff-only", "main"]);
    product_change(root);
    pass(&ci(root, Some(NONE), &[]));
}

#[test]
fn capability_divergent_target_additions_are_not_removals() {
    let dir = fixture(Some("block"), true, false);
    let root = dir.path();
    product_change(root);
    git(root, &["switch", "-q", "main"]);
    write(
        root,
        "docs/capabilities/CAP-019-extra.md",
        &RECORD
            .replace("CAP-101", "CAP-019")
            .replace("name: catalog", "name: extra")
            .replace("3b9e7c2a", "4b9e7c2a"),
    );
    write(
        root,
        ".codeflow/project.toml",
        &project(false).replace("['engine']", "['engine', 'extra']"),
    );
    commit(root);
    git(root, &["switch", "-q", "feat/example"]);
    pass(&ci(root, Some(NONE), &[]));
    write(
        root,
        ".codeflow/project.toml",
        &project(false).replace("['engine']", "['engine', 'extra']"),
    );
    write(
        root,
        "docs/capabilities/CAP-019-another.md",
        &RECORD
            .replace("CAP-101", "CAP-019")
            .replace("name: catalog", "name: another")
            .replace("3b9e7c2a", "5b9e7c2a")
            .replace("area: engine", "area: extra"),
    );
    commit(root);
    pass(&ci(
        root,
        Some("## Capability impact\n\n- Added CAP-019 another: The new area is described.\n"),
        &[],
    ));
    git(root, &["merge", "--no-edit", "main"]);
    let out = super::run_in(root, &["validate", "--docs"]);
    assert!(
        !out.status.success() && said(&out).contains("duplicate CAP-019"),
        "{}",
        said(&out)
    );
}

#[test]
fn capability_only_ordinary_markdown_is_exempt() {
    for (path, exempt) in [
        ("docs/guide.md", true),
        ("docs/api.md", false),
        (".agents/guide.md", false),
    ] {
        let dir = fixture(Some("block"), true, false);
        write(dir.path(), path, "Updated guidance.\n");
        commit(dir.path());
        let out = ci(dir.path(), None, &[]);
        if exempt {
            pass(&out);
        } else {
            refuse(&out, "PR body unavailable");
        }
    }
}

#[test]
fn capability_legacy_catalog_counts_as_present() {
    let dir = fixture(Some("block"), false, false);
    let root = dir.path();
    git(root, &["switch", "-q", "main"]);
    write(
        root,
        "docs/capabilities.md",
        "```yaml\nid: CAP-101\nname: catalog\narea: engine\nstatus: planned\n```\n",
    );
    commit(root);
    git(root, &["switch", "-q", "feat/example"]);
    git(root, &["merge", "--ff-only", "main"]);
    product_change(root);
    pass(&ci(root, Some(NONE), &[]));
    write(
        root,
        "docs/capabilities.md",
        "```yaml\nid: CAP-101\nname: catalog\narea: engine\nstatus: planned\n```\nReaders now get detailed catalog guidance.\n",
    );
    commit(root);
    refuse(&ci(root, Some(NONE), &[]), "None cannot accompany");
    pass(&ci(root, Some(IMPROVED), &[]));
    let out = super::run_in(root, &["doctor", "--check", "adopter-fit"]);
    assert!(!said(&out).contains("catalog present, but"));
    write(root, ".codeflow/policy.json", &policy(Some("off")));
    let out = super::run_in(root, &["doctor", "--check", "adopter-fit"]);
    assert!(
        said(&out).contains("catalog present, but"),
        "{}",
        said(&out)
    );
}

#[test]
fn capability_examples_are_not_declarations() {
    let dir = fixture(Some("block"), true, false);
    product_change(dir.path());
    for body in [
        format!("```markdown\n{NONE}\n```"),
        format!("<!--\n{NONE}\n-->"),
        "## Capability impact\n\n> - None: A quoted example.\n".to_owned(),
        format!("{NONE}\n{NONE}"),
    ] {
        refuse(&ci(dir.path(), Some(&body), &[]), "Capability impact");
    }
}

#[test]
fn capability_body_file_journey_and_declaration_bounds() {
    let dir = fixture(Some("block"), false, false);
    let root = dir.path();
    write(root, FILE, RECORD);
    commit(root);
    let bodies = tempfile::tempdir().unwrap();
    let path = bodies.path().join("body.md");
    for (body, expected) in [
        ("## Summary\n\nAdds a catalog.\n".to_owned(), 1),
        (ADDED.to_owned(), 0),
        (
            format!("## Capability impact\n\n- None: {}\n", "x".repeat(4097)),
            1,
        ),
        (
            format!(
                "## Capability impact\n\n{}",
                "- None: No contract change.\n".repeat(10_001)
            ),
            1,
        ),
    ] {
        std::fs::write(&path, format!("Task: TSK-001\n{body}")).unwrap();
        let out = ci(root, None, &["--pr-body-file", path.to_str().unwrap()]);
        println!("capability body-file journey: exit {:?}", out.status.code());
        assert_eq!(out.status.code(), Some(expected), "{}", said(&out));
        if body.len() > 4096 {
            assert!(said(&out).contains("exceeds"), "{}", said(&out));
        }
    }
}

#[cfg(unix)]
#[test]
fn capability_moving_target_ref_keeps_first_commit_identity() {
    use std::io::{BufRead, BufReader, Read};
    use std::process::Stdio;
    let dir = fixture(Some("block"), true, false);
    let root = dir.path();
    let base = git(root, &["rev-parse", "main"]);
    product_change(root);
    let head = git(root, &["rev-parse", "HEAD"]);
    git(root, &["switch", "-qc", "moved-target", "main"]);
    write(root, FILE, "---\nid: [broken\n---\n");
    let moved = commit(root);
    git(root, &["switch", "-q", "feat/example"]);
    let bodies = tempfile::tempdir().unwrap();
    let fifo = bodies.path().join("body.fifo");
    assert!(Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap()
        .success());
    let mut child = super::codeflow()
        .args([
            "ci",
            "--base",
            "main",
            "--head",
            "HEAD",
            "--branch",
            "feat/example",
            "--pr-body-file",
        ])
        .arg(&fifo)
        .current_dir(root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut text = String::new();
    loop {
        let mut line = String::new();
        assert!(output.read_line(&mut line).unwrap() > 0, "{text}");
        let resolved = line.contains("acceptance target");
        text.push_str(&line);
        if resolved {
            break;
        }
    }
    // Context has pinned the commits; the FIFO holds body resolution until
    // the target ref moves. No production timing or test-only hook is needed.
    git(root, &["update-ref", "refs/heads/main", &moved, &base]);
    std::fs::write(&fifo, format!("Task: TSK-001\n{NONE}")).unwrap();
    output.read_to_string(&mut text).unwrap();
    let out = child.wait_with_output().unwrap();
    text.push_str(&said(&out));
    assert!(out.status.success(), "{text}");
    assert!(
        text.contains(&format!("authority {base}, merge-base {base}, head {head}")),
        "{text}"
    );
    assert_eq!(git(root, &["rev-parse", "main"]), moved);
}

#[test]
fn capability_retarget_uses_new_policy_and_catalog() {
    let dir = fixture(Some("off"), false, false);
    let root = dir.path();
    product_change(root);
    pass(&ci(root, None, &[]));
    git(root, &["switch", "-qc", "release", "main"]);
    write(root, ".codeflow/policy.json", &policy(Some("block")));
    write(root, FILE, RECORD);
    commit(root);
    git(root, &["switch", "-q", "feat/example"]);
    let run = |body: Option<&str>| {
        let mut args = vec![
            "ci",
            "--base",
            "release",
            "--head",
            "HEAD",
            "--branch",
            "feat/example",
        ];
        if let Some(body) = body {
            args.extend(["--pr-body", body]);
        }
        super::run_in(root, &args)
    };
    refuse(&run(None), "PR body unavailable");
    let out = run(Some(NONE));
    refuse(&out, "merge base predates the authority's catalog");
    assert!(said(&out).contains("merge the target into this branch"));
    git(root, &["merge", "--no-edit", "release"]);
    pass(&run(Some(NONE)));
    // Once baseline and head have catalogs, authority presence is not needed.
    // This malformed newer authority catalog must not trigger an eager read.
    git(root, &["switch", "-q", "release"]);
    write(root, FILE, "---\nid: [broken\n---\n");
    commit(root);
    git(root, &["switch", "-q", "feat/example"]);
    pass(&run(Some(NONE)));
}

#[test]
fn capability_duplicate_ids_and_uids_at_baseline_or_head_refuse() {
    for baseline in [false, true] {
        for same_uid in [false, true] {
            let dir = fixture(Some("block"), true, false);
            let root = dir.path();
            if baseline {
                git(root, &["switch", "-q", "main"]);
            }
            let extra = if same_uid {
                RECORD
                    .replace("CAP-101", "CAP-102")
                    .replace("name: catalog", "name: extra")
            } else {
                RECORD
                    .replace("name: catalog", "name: extra")
                    .replace("3b9e7c2a", "4b9e7c2a")
            };
            let path = if same_uid {
                "docs/capabilities/CAP-102-extra.md"
            } else {
                "docs/capabilities/CAP-101-extra.md"
            };
            write(root, path, &extra);
            commit(root);
            if baseline {
                git(root, &["switch", "-q", "feat/example"]);
                git(root, &["merge", "--ff-only", "main"]);
                product_change(root);
            }
            let bad_revision = git(root, &["rev-parse", if baseline { "main" } else { "HEAD" }]);
            let out = ci(root, Some(NONE), &[]);
            refuse(&out, "duplicate");
            assert!(
                said(&out).contains(&format!("catalog at {bad_revision}")),
                "{}",
                said(&out)
            );
            assert!(
                said(&out).contains(if same_uid {
                    "3b9e7c2a-51d4-4f0e-9a6b-0c2d8e417f55"
                } else {
                    "CAP-101"
                }),
                "{}",
                said(&out)
            );
        }
    }
}

#[test]
fn capability_symlink_and_submodule_catalog_directories_refuse() {
    for mode in ["120000", "160000"] {
        let dir = fixture(Some("block"), true, false);
        let root = dir.path();
        let object = git(
            root,
            &[
                "rev-parse",
                if mode == "160000" {
                    "HEAD"
                } else {
                    "HEAD:docs/capabilities/CAP-101-catalog.md"
                },
            ],
        );
        git(root, &["rm", "-rq", "docs/capabilities"]);
        git(
            root,
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("{mode},{object},docs/capabilities"),
            ],
        );
        git(root, &["commit", "-qm", "feat: replace catalog directory"]);
        refuse(&ci(root, Some(NONE), &[]), "docs/capabilities");
    }
}

#[cfg(unix)]
#[test]
fn capability_tree_reads_never_run_checkout_filters_or_head_loaders() {
    use std::os::unix::fs::PermissionsExt;
    let dir = fixture(Some("block"), true, false);
    let root = dir.path();
    write(
        root,
        ".gitattributes",
        "docs/capabilities/** filter=sentinel diff=sentinel\n",
    );
    write(
        root,
        "docs/capabilities/loader.sh",
        "touch .git/capability-executed\n",
    );
    product_change(root);
    for key in [
        "filter.sentinel.clean",
        "filter.sentinel.smudge",
        "diff.sentinel.textconv",
    ] {
        git(root, &["config", key, "touch .git/capability-executed"]);
    }
    write(
        root,
        ".git/hooks/post-checkout",
        "#!/bin/sh\ntouch .git/capability-executed\n",
    );
    std::fs::set_permissions(
        root.join(".git/hooks/post-checkout"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    pass(&ci(root, Some(NONE), &[]));
    assert!(!root.join(".git/capability-executed").exists());
}

#[test]
fn capability_catalog_of_two_thousand_records_fits_step_budget() {
    let dir = fixture(Some("block"), false, false);
    let root = dir.path();
    git(root, &["switch", "-q", "main"]);
    for n in 1..=2000 {
        let id = format!("CAP-{n:03}");
        let uid = format!("00000000-0000-4000-8000-{n:012}");
        write(
            root,
            &format!("docs/capabilities/{id}-catalog.md"),
            &RECORD
                .replace("CAP-101", &id)
                .replace("3b9e7c2a-51d4-4f0e-9a6b-0c2d8e417f55", &uid),
        );
    }
    commit(root);
    git(root, &["switch", "-q", "feat/example"]);
    git(root, &["merge", "--ff-only", "main"]);
    product_change(root);
    let started = std::time::Instant::now();
    let out = ci(root, Some(NONE), &[]);
    println!(
        "2000-record full CI invocation: {:.3}s\n{}",
        started.elapsed().as_secs_f64(),
        said(&out)
    );
    pass(&out);
    assert!(
        started.elapsed().as_secs() < 60,
        "catalog check exhausted the step budget"
    );
}

#[test]
fn capability_unsafe_modes_malformed_records_and_duplicates_refuse() {
    for mode in ["100755", "120000", "160000"] {
        let dir = fixture(Some("block"), true, false);
        let root = dir.path();
        let object = git(
            root,
            &[
                "rev-parse",
                if mode == "160000" {
                    "HEAD"
                } else {
                    "HEAD:docs/capabilities/CAP-101-catalog.md"
                },
            ],
        );
        git(
            root,
            &[
                "update-index",
                "--cacheinfo",
                &format!("{mode},{object},{FILE}"),
            ],
        );
        git(root, &["commit", "-qm", "feat: unsafe catalog mode"]);
        refuse(&ci(root, Some(NONE), &[]), "mode");
    }
    for text in [
        RECORD.replace("status: shipped", "status: ["),
        RECORD.replace("name: catalog", "unknown_field: catalog"),
    ] {
        let dir = fixture(Some("block"), true, false);
        write(dir.path(), FILE, &text);
        commit(dir.path());
        refuse(&ci(dir.path(), Some(CHANGED), &[]), "catalog at");
    }
    let dir = fixture(Some("block"), true, false);
    write(dir.path(), "docs/capabilities/CAP-101-other.md", RECORD);
    commit(dir.path());
    refuse(&ci(dir.path(), Some(NONE), &[]), "duplicate");
}

#[test]
fn capability_ambiguous_and_missing_history_fail_closed() {
    let dir = fixture(Some("block"), true, false);
    let root = dir.path();
    let base = git(root, &["rev-parse", "main"]);
    product_change(root);
    let a = git(root, &["rev-parse", "HEAD"]);
    let tree = git(root, &["rev-parse", "HEAD^{tree}"]);
    let b = git(
        root,
        &[
            "commit-tree",
            &tree,
            "-p",
            &base,
            "-m",
            "feat: second parent",
        ],
    );
    let one = git(
        root,
        &[
            "commit-tree",
            &tree,
            "-p",
            &a,
            "-p",
            &b,
            "-m",
            "feat: first merge",
        ],
    );
    let two = git(
        root,
        &[
            "commit-tree",
            &tree,
            "-p",
            &b,
            "-p",
            &a,
            "-m",
            "feat: second merge",
        ],
    );
    git(root, &["update-ref", "refs/heads/main", &one]);
    git(root, &["update-ref", "refs/heads/feat/example", &two]);
    refuse(&ci(root, Some(NONE), &[]), "exactly one merge base");
    let orphan = git(
        root,
        &["commit-tree", &tree, "-m", "feat: unrelated history"],
    );
    git(root, &["update-ref", "refs/heads/main", &orphan]);
    let out = ci(root, Some(NONE), &[]);
    assert!(!out.status.success(), "{}", said(&out));
}

#[test]
fn capability_shallow_history_is_not_verified() {
    let dir = fixture(Some("block"), true, false);
    let root = dir.path();
    product_change(root);
    let head = git(root, &["rev-parse", "HEAD"]);
    write(root, ".git/shallow", &format!("{head}\n"));
    let out = ci(root, Some(NONE), &[]);
    assert!(
        !out.status.success() && said(&out).contains("shallow"),
        "{}",
        said(&out)
    );
}

fn pre_push(root: &Path, remote: &Path, heads: &[(&str, &str)]) -> Output {
    use std::fmt::Write as _;
    use std::io::Write;
    use std::process::Stdio;
    let zero = "0".repeat(40);
    let mut input = String::new();
    for (branch, head) in heads {
        writeln!(
            input,
            "refs/heads/{branch} {head} refs/heads/{branch} {zero}"
        )
        .unwrap();
    }
    let mut child = super::codeflow()
        .args(["git-hook", "pre-push", "origin", remote.to_str().unwrap()])
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn capability_first_and_multiple_ref_pushes_need_no_body() {
    let dir = fixture(Some("block"), true, false);
    let root = dir.path();
    git(root, &["switch", "-q", "main"]);
    let mut configured: serde_json::Value = serde_json::from_str(&policy(Some("block"))).unwrap();
    configured["git"]["test_gate_on_push"] = serde_json::json!("block");
    write(root, ".codeflow/policy.json", &configured.to_string());
    commit(root);
    let remote = tempfile::tempdir().unwrap();
    git(remote.path(), &["init", "--bare", "-qb", "main"]);
    git(
        root,
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    git(root, &["push", "-q", "origin", "main"]);
    git(root, &["remote", "set-head", "origin", "main"]);
    git(root, &["switch", "-q", "feat/example"]);
    git(root, &["merge", "--ff-only", "main"]);
    product_change(root);
    let head = git(root, &["rev-parse", "HEAD"]);
    git(root, &["branch", "feat/second"]);
    for refs in [vec!["feat/example"], vec!["feat/example", "feat/second"]] {
        let heads: Vec<_> = refs.iter().map(|branch| (*branch, head.as_str())).collect();
        let out = pre_push(root, remote.path(), &heads);
        let text = said(&out);
        assert!(out.status.success(), "{text}");
        assert!(!text.contains("PR body unavailable"), "{text}");
        assert!(text.contains("candidate authority"), "{text}");
        assert_eq!(
            text.matches("candidate authority").count(),
            refs.len(),
            "{text}"
        );
    }
    git(root, &["switch", "-q", "feat/second"]);
    write(root, FILE, "---\nid: [malformed catalog\n---\n");
    let bad = commit(root);
    git(root, &["switch", "-q", "feat/example"]);
    let out = pre_push(
        root,
        remote.path(),
        &[("feat/example", &head), ("feat/second", &bad)],
    );
    let text = said(&out);
    assert!(out.status.success(), "{text}");
    assert!(!text.contains("catalog at"), "{text}");
    // The same malformed head is refused by the authoritative CLI path.
    git(root, &["switch", "-q", "feat/second"]);
    refuse(&ci(root, Some(NONE), &[]), "catalog at");
}

#[test]
fn capability_registry_join_preserves_renumbered_identity_and_refuses_bad_bindings() {
    for case in [
        "valid",
        "missing",
        "malformed",
        "mismatch",
        "symlink",
        "remote-missing",
        "ambiguous",
    ] {
        let dir = fixture(Some("block"), false, false);
        let root = dir.path();
        git(root, &["switch", "-q", "main"]);
        write(
            root,
            "docs/capabilities.md",
            "```yaml\nid: CAP-101\nname: catalog\narea: engine\nstatus: planned\n```\n",
        );
        commit(root);
        git(root, &["switch", "-q", "feat/example"]);
        git(root, &["merge", "--ff-only", "main"]);
        write(root, "docs/capabilities.md", "# Catalog\n");
        write(
            root,
            "docs/capabilities/CAP-202-catalog.md",
            &RECORD
                .replace("id: CAP-101", "id: CAP-202\nformer_ids: [CAP-101]")
                .replace(
                    "The catalog describes the product.",
                    "Readers see the converted and edited catalog.",
                ),
        );
        if case == "ambiguous" {
            let second = std::fs::read_to_string(root.join("docs/capabilities/CAP-202-catalog.md"))
                .unwrap()
                .replace("id: CAP-202", "id: CAP-203")
                .replace(
                    "3b9e7c2a-51d4-4f0e-9a6b-0c2d8e417f55",
                    "a7fb4faa-c84d-45ca-ad4d-24d039e0919b",
                );
            write(root, "docs/capabilities/CAP-203-catalog.md", &second);
        }
        commit(root);
        let (uid, sha) = registry_binding_fixture(root, case);
        let declaration = "## Capability impact\n\n- Changed CAP-202 catalog: Readers get the converted and edited catalog.\n";
        let out = ci(root, Some(declaration), &[]);
        match case {
            "valid" => {
                pass(&out);
                assert!(said(&out)
                    .contains(&format!("join CAP-101 -> CAP-202 uid {uid} registry {sha}")));
                refuse(
                    &ci(root, Some(&declaration.replace("Changed", "Added")), &[]),
                    "disagrees",
                );
            }
            "missing" => refuse(&out, "lacks ids/CAP/202.toml"),
            "malformed" => {
                refuse(&out, "ids/CAP/202.toml: the registry is damaged");
                assert!(!said(&out).contains("lacks ids/CAP/202.toml"));
            }
            "mismatch" => refuse(&out, "migration UID disagrees"),
            "symlink" => refuse(&out, "must be a regular blob"),
            "remote-missing" => refuse(&out, "cannot pin capability registry"),
            "ambiguous" => refuse(&out, "CAP-101 migration is ambiguous"),
            _ => unreachable!(),
        }
    }
}

fn registry_binding_fixture(root: &Path, case: &str) -> (&'static str, git2::Oid) {
    let repo = git2::Repository::open(root).unwrap();
    let uid = if case == "mismatch" {
        "a7fb4faa-c84d-45ca-ad4d-24d039e0919b"
    } else {
        "3b9e7c2a-51d4-4f0e-9a6b-0c2d8e417f55"
    };
    let entry = codeflow_core::ids::Entry::issued(
        codeflow_core::ids::RegId::canonical(codeflow_core::ids::Kind::Cap, 202),
        uid.to_owned(),
        "Catalog binding",
        "fixture@example.test",
        "none",
    );
    let bytes = if case == "malformed" {
        "invalid toml".to_owned()
    } else {
        entry.render()
    };
    let blob = repo.blob(bytes.as_bytes()).unwrap();
    let mut cap = repo.treebuilder(None).unwrap();
    if case != "missing" {
        cap.insert(
            "202.toml",
            blob,
            if case == "symlink" {
                0o120_000
            } else {
                0o100_644
            },
        )
        .unwrap();
    }
    let mut ids = repo.treebuilder(None).unwrap();
    ids.insert("CAP", cap.write().unwrap(), 0o040_000).unwrap();
    let mut registry = repo.treebuilder(None).unwrap();
    registry
        .insert("ids", ids.write().unwrap(), 0o040_000)
        .unwrap();
    let tree_id = registry.write().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    let signature = repo.signature().unwrap();
    let sha = repo
        .commit(
            Some(codeflow_core::ids::REGISTRY_REF),
            &signature,
            &signature,
            "chore: bind capability",
            &tree,
            &[],
        )
        .unwrap();
    if case == "remote-missing" {
        repo.remote("origin", "https://example.invalid/fixture.git")
            .unwrap();
    }
    (uid, sha)
}

/// TSK-1136 AC-3: every worked example in the shipped guide gives the outcome
/// the guide states when the real check judges it against the fixture
/// catalog.
#[test]
fn capability_guide_examples_match_the_check() {
    const GUIDE: &str = include_str!("../../../../assets/base/rules/capabilities.md");
    let body = |line: &str| {
        assert!(
            GUIDE.contains(&format!("`{line}`")),
            "the guide shows {line}"
        );
        format!("## Capability impact\n\n{line}\n")
    };
    let status_edit = RECORD.replace("status: shipped", "status: building");

    // Passes.
    let dir = fixture(Some("block"), false, false);
    write(dir.path(), FILE, RECORD);
    commit(dir.path());
    pass(&ci(
        dir.path(),
        Some(&body(
            "- Added CAP-101 catalog: readers can list their capabilities.",
        )),
        &[],
    ));

    let dir = fixture(Some("block"), true, false);
    write(dir.path(), FILE, &status_edit);
    commit(dir.path());
    pass(&ci(
        dir.path(),
        Some(&body(
            "- Changed CAP-101 catalog: readers see the new status.",
        )),
        &[],
    ));
    refuse(
        &ci(
            dir.path(),
            Some(&body("- None: no user-visible change.")),
            &[],
        ),
        "None cannot",
    );
    refuse(
        &ci(
            dir.path(),
            Some(&body(
                "- Improved CAP-101 catalog: clearer, entry unchanged.",
            )),
            &[],
        ),
        "disagrees with the net catalog delta",
    );

    let dir = fixture(Some("block"), true, false);
    product_change(dir.path());
    pass(&ci(
        dir.path(),
        Some(&body(
            "- Improved CAP-101 catalog: readers get faster listings, entry unchanged.",
        )),
        &[],
    ));
    pass(&ci(
        dir.path(),
        Some(&body(
            "- None: internal refactoring preserves the catalog contract.",
        )),
        &[],
    ));
    refuse(
        &ci(
            dir.path(),
            Some("## Summary\n\nNo capability section.\n"),
            &[],
        ),
        "exactly one ## Capability impact",
    );
    refuse(
        &ci(
            dir.path(),
            Some(&body("- Changed CAP-999 missing: ...")),
            &[],
        ),
        "unknown id CAP-999",
    );
    refuse(
        &ci(
            dir.path(),
            Some(&body("- Improved CAP-101 catalog: faster.")),
            &[],
        ),
        "disagrees with the net catalog delta",
    );

    let dir = fixture(Some("block"), true, false);
    git(dir.path(), &["rm", "-q", FILE]);
    write(dir.path(), "src/lib.rs", "pub fn after_removal() {}\n");
    commit(dir.path());
    refuse(&ci(dir.path(), Some(NONE), &[]), "identity removed");
}

/// TSK-1136 AC-3: the template's examples use the declaration grammar, and
/// its unfilled line is a placeholder the check refuses.
#[test]
fn capability_template_shows_the_grammar_and_refuses_its_placeholder() {
    const TEMPLATE: &str = include_str!("../../../../assets/base/ci/pull_request_template.md");
    for verb in [
        "- Added CAP-",
        "- Changed CAP-",
        "- Improved CAP-",
        "- None: ",
    ] {
        assert!(TEMPLATE.contains(verb), "the template shows {verb}");
    }
    assert!(TEMPLATE.contains("entry unchanged"));
    assert!(TEMPLATE.contains("\n- None: <reason>\n"));
    let dir = fixture(Some("block"), true, false);
    product_change(dir.path());
    refuse(
        &ci(
            dir.path(),
            Some("## Capability impact\n\n- None: <reason>\n"),
            &[],
        ),
        "None needs one substantive reason",
    );
}
