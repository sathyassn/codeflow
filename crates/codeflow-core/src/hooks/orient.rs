//! The session-orient digest (charter §3.4) — deliberately tiny.
//!
//! Budget: ≤30 lines / <500 tokens, **pointers not content**. Generated
//! live, never stored. Missing artifacts are gracefully omitted. The same
//! digest backs `codeflow orient`, the `SessionStart` hook, and (later)
//! `mcp serve`. Off-switch: `orient.enabled = false` in
//! `.codeflow/project.toml` (D21).

use std::fmt::Write as _;
use std::path::Path;

use crate::models::{EpicFilter, EpicStatus, TaskFilter, TaskStatus};
use crate::workgraph::{MarkdownStore, RecordStore};

use super::policy::{read_project_toml, Policy};
use super::repo::RepoInfo;

/// Hard cap from the charter (§3.4 / D21).
pub const MAX_LINES: usize = 30;

/// Generate the orient digest for the project at `root`.
///
/// Returns an empty string when orient is disabled in `project.toml`.
#[must_use]
pub fn generate(root: &Path) -> String {
    let project_toml = read_project_toml(root);
    if !enabled(project_toml.as_ref()) {
        return String::new();
    }

    let mut out = String::new();
    let name = project_name(root);
    let _ = writeln!(out, "# orient — {name}");

    if let Some(one_liner) = product_one_liner(root, project_toml.as_ref()) {
        let _ = writeln!(out, "{one_liner}");
    }

    if let Some(line) = branch_line(root) {
        let _ = writeln!(out, "{line}");
    }

    if let Some(line) = work_line(root) {
        let _ = writeln!(out, "{line}");
    }

    if let Some(line) = capabilities_line(root) {
        let _ = writeln!(out, "{line}");
    }

    let adrs = recent_adrs(root, 3);
    if !adrs.is_empty() {
        let _ = writeln!(out, "recent decisions:");
        for adr in adrs {
            let _ = writeln!(out, "  - {adr}");
        }
    }

    let _ = writeln!(out, "{}", gates_line(root));

    let pointers = pointer_paths(root);
    if !pointers.is_empty() {
        let _ = writeln!(out, "read: {}", pointers.join(" · "));
    }

    debug_assert!(out.lines().count() <= MAX_LINES);
    out
}

fn enabled(project_toml: Option<&toml::Value>) -> bool {
    project_toml
        .and_then(|v| v.get("orient"))
        .and_then(|o| o.get("enabled"))
        .and_then(toml::Value::as_bool)
        .unwrap_or(true)
}

fn project_name(root: &Path) -> String {
    root.file_name()
        .map_or_else(|| "project".to_string(), |n| n.to_string_lossy().to_string())
}

/// One line of purpose: `project.toml product_one_liner`, then product.md
/// frontmatter, then the first prose line under `## Purpose`, then the
/// first heading.
fn product_one_liner(root: &Path, project_toml: Option<&toml::Value>) -> Option<String> {
    if let Some(s) = project_toml
        .and_then(|v| v.get("product_one_liner"))
        .and_then(toml::Value::as_str)
    {
        if !s.trim().is_empty() {
            return Some(truncate(s.trim(), 120));
        }
    }

    let text = std::fs::read_to_string(root.join("docs").join("product.md")).ok()?;

    if let Some(rest) = text.strip_prefix("---\n") {
        if let Some((frontmatter, _)) = rest.split_once("\n---") {
            for line in frontmatter.lines() {
                for key in ["one_liner:", "description:", "summary:"] {
                    if let Some(v) = line.strip_prefix(key) {
                        let v = v.trim().trim_matches('"');
                        if !v.is_empty() {
                            return Some(truncate(v, 120));
                        }
                    }
                }
            }
        }
    }

    if let Some(idx) = text.find("## Purpose") {
        for line in text[idx..].lines().skip(1) {
            let line = line.trim();
            if line.is_empty()
                || line.starts_with("<!--")
                || line.starts_with('#')
                || line.contains("-->")
                || line.starts_with("{{")
            {
                if line.starts_with('#') {
                    break; // next section, no prose found
                }
                continue;
            }
            return Some(truncate(line, 120));
        }
    }

    let first = text.lines().find(|l| !l.trim().is_empty())?;
    Some(truncate(first.trim_start_matches('#').trim(), 120))
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max).collect();
        format!("{cut}…")
    }
}

fn branch_line(root: &Path) -> Option<String> {
    let info = RepoInfo::discover(root)?;
    let policy = Policy::load(root);
    let branch = if info.branch.is_empty() {
        "(detached)".to_string()
    } else {
        info.branch.clone()
    };
    let kind = if info.is_worktree { "worktree" } else { "root checkout" };
    let protected = if policy.git.branch_is_protected(&info.branch) {
        ", protected — develop in a worktree on a feature branch"
    } else {
        ""
    };
    Some(format!("branch: {branch} ({kind}{protected})"))
}

fn work_line(root: &Path) -> Option<String> {
    let pm = root.join("project-management");
    if !pm.join("epics").is_dir() && !pm.join("tasks").is_dir() {
        return None;
    }
    let store = MarkdownStore::new(&pm).ok()?;
    let epics = store.list_epics(EpicFilter::default()).ok()?;
    let tasks = store.list_tasks(TaskFilter::default()).ok()?;
    let active_epics = epics
        .iter()
        .filter(|e| e.status == EpicStatus::InProgress)
        .count();
    let open_tasks = tasks
        .iter()
        .filter(|t| matches!(t.status, TaskStatus::Todo | TaskStatus::InProgress | TaskStatus::Blocked))
        .count();
    Some(format!(
        "work: {} epics ({active_epics} in progress) · {} tasks ({open_tasks} open)",
        epics.len(),
        tasks.len(),
    ))
}

/// Count capability statuses from `docs/capabilities.md` yaml entries.
///
/// Delegates to the canonical registry parser
/// (`crate::capability::parse_capabilities`) rather than an ad-hoc text scan,
/// so HTML-commented example entries (the scaffold's `<!-- ... CAP-001 ...
/// -->`) are masked out here exactly as they are in `status`/`validate`
/// — a fresh repo must not count its own doc-comment example as a real
/// capability.
fn capabilities_line(root: &Path) -> Option<String> {
    const STATUSES: [&str; 4] = ["shipped", "building", "planned", "deprecated"];
    let text = std::fs::read_to_string(root.join("docs").join("capabilities.md")).ok()?;
    let (entries, _issues) = crate::capability::parse_capabilities(&text);
    let mut counts = [0usize; 4];
    for entry in &entries {
        if let Some(idx) = STATUSES.iter().position(|s| *s == entry.status) {
            counts[idx] += 1;
        }
    }
    if counts.iter().all(|c| *c == 0) {
        return None;
    }
    let parts: Vec<String> = STATUSES
        .iter()
        .zip(counts.iter())
        .filter(|(_, c)| **c > 0)
        .map(|(s, c)| format!("{c} {s}"))
        .collect();
    Some(format!("capabilities: {}", parts.join(" · ")))
}

/// Titles of the most recent `n` ADRs in `docs/decisions/`.
fn recent_adrs(root: &Path, n: usize) -> Vec<String> {
    let dir = root.join("docs").join("decisions");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(std::result::Result::ok)
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| {
            std::path::Path::new(n)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
                && !n.to_lowercase().contains("template")
                && !n.eq_ignore_ascii_case("readme.md")
        })
        .collect();
    names.sort();
    names
        .iter()
        .rev()
        .take(n)
        .map(|name| {
            let title = std::fs::read_to_string(dir.join(name))
                .ok()
                .and_then(|text| {
                    text.lines()
                        .find(|l| l.starts_with("# "))
                        .map(|l| l.trim_start_matches("# ").to_string())
                });
            title.unwrap_or_else(|| name.trim_end_matches(".md").to_string())
        })
        .collect()
}

/// One-line enforcement status: which hooks are wired (charter §3.4).
fn gates_line(root: &Path) -> String {
    let mark = |on: bool| if on { "✓" } else { "✗" };

    let hooks_dir = git_hooks_dir(root);
    let hook_wired = |name: &str| {
        hooks_dir
            .as_ref()
            .is_some_and(|d| d.join(name).exists())
    };

    let settings = std::fs::read_to_string(root.join(".claude").join("settings.json"))
        .unwrap_or_default();

    format!(
        "gates: git-hooks[pre-commit{} commit-msg{} pre-push{}] claude[git-guard{} orient{} summary{}]",
        mark(hook_wired("pre-commit")),
        mark(hook_wired("commit-msg")),
        mark(hook_wired("pre-push")),
        mark(settings.contains("git-guard")),
        mark(settings.contains("session-orient")),
        mark(settings.contains("session-summary")),
    )
}

/// Resolve the active hooks directory (`core.hooksPath` or `<git>/hooks`).
fn git_hooks_dir(root: &Path) -> Option<std::path::PathBuf> {
    let repo = super::repo::open(root)?;
    if let Ok(config) = repo.config() {
        if let Ok(path) = config.get_string("core.hookspath") {
            let p = std::path::PathBuf::from(&path);
            return Some(if p.is_absolute() { p } else { root.join(p) });
        }
    }
    Some(repo.commondir().join("hooks"))
}

fn pointer_paths(root: &Path) -> Vec<String> {
    [
        "docs/product.md",
        "docs/architecture.md",
        "docs/capabilities.md",
        "docs/decisions/",
        "AGENTS.md",
    ]
    .iter()
    .filter(|p| root.join(p.trim_end_matches('/')).exists())
    .map(ToString::to_string)
    .collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .output()
            .expect("git runs");
        assert!(out.status.success());
    }

    fn full_fixture(root: &Path) {
        git(root, &["init", "-b", "feat/x"]);
        std::fs::create_dir_all(root.join("docs/decisions")).unwrap();
        std::fs::create_dir_all(root.join(".codeflow")).unwrap();
        std::fs::create_dir_all(root.join(".claude")).unwrap();
        std::fs::write(
            root.join("docs/product.md"),
            "# demo — product\n\n## Purpose\n\nA tiny demo project for orient tests.\n",
        )
        .unwrap();
        std::fs::write(root.join("docs/architecture.md"), "# arch\n").unwrap();
        std::fs::write(
            root.join("docs/capabilities.md"),
            "# caps\n\n```yaml\nid: CAP-001\nstatus: shipped\n```\n\n```yaml\nid: CAP-002\nstatus: building\n```\n\n```yaml\nid: CAP-003\nstatus: shipped\n```\n",
        )
        .unwrap();
        for (n, title) in [(1, "use markdown"), (2, "bundle sqlite"), (3, "embed assets"), (4, "ship one binary")] {
            std::fs::write(
                root.join(format!("docs/decisions/ADR-{n:04}.md")),
                format!("# ADR-{n:04} — {title}\n\nbody\n"),
            )
            .unwrap();
        }
        std::fs::write(
            root.join(".claude/settings.json"),
            r#"{"hooks":{"PreToolUse":[{"hooks":[{"command":"codeflow hook git-guard"}]}],"SessionStart":[{"hooks":[{"command":"codeflow hook session-orient"}]}],"SessionEnd":[{"hooks":[{"command":"codeflow hook session-summary"}]}]}}"#,
        )
        .unwrap();
        std::fs::write(root.join("AGENTS.md"), "# contract\n").unwrap();
    }

    #[test]
    fn test_digest_full_fixture_within_budget() {
        let dir = tempfile::tempdir().unwrap();
        full_fixture(dir.path());
        let digest = generate(dir.path());

        assert!(digest.lines().count() <= MAX_LINES, "{digest}");
        // <500 token budget: chars/4 is a generous proxy.
        assert!(digest.len() < 2000, "digest too large: {} bytes", digest.len());

        assert!(digest.contains("# orient —"));
        assert!(digest.contains("A tiny demo project"));
        assert!(digest.contains("branch: feat/x"));
        assert!(digest.contains("capabilities: 2 shipped · 1 building"));
        // Last 3 ADRs only, newest first.
        assert!(digest.contains("ADR-0004 — ship one binary"));
        assert!(digest.contains("ADR-0002 — bundle sqlite"));
        assert!(!digest.contains("ADR-0001"));
        assert!(digest.contains("claude[git-guard✓ orient✓ summary✓]"));
        assert!(digest.contains("read: docs/product.md"));
        assert!(digest.contains("AGENTS.md"));
    }

    #[test]
    fn test_digest_empty_dir_graceful() {
        let dir = tempfile::tempdir().unwrap();
        let digest = generate(dir.path());
        assert!(digest.starts_with("# orient —"));
        assert!(!digest.contains("capabilities:"));
        assert!(!digest.contains("recent decisions:"));
        assert!(digest.lines().count() <= MAX_LINES);
    }

    #[test]
    fn test_digest_disabled_by_project_toml() {
        let dir = tempfile::tempdir().unwrap();
        let cf = dir.path().join(".codeflow");
        std::fs::create_dir_all(&cf).unwrap();
        std::fs::write(cf.join("project.toml"), "[orient]\nenabled = false\n").unwrap();
        assert_eq!(generate(dir.path()), "");
    }

    #[test]
    fn test_one_liner_from_project_toml_wins() {
        let dir = tempfile::tempdir().unwrap();
        let cf = dir.path().join(".codeflow");
        std::fs::create_dir_all(&cf).unwrap();
        std::fs::write(
            cf.join("project.toml"),
            "product_one_liner = \"the discipline layer\"\n",
        )
        .unwrap();
        let digest = generate(dir.path());
        assert!(digest.contains("the discipline layer"));
    }

    #[test]
    fn test_work_counts_from_markdown_store() {
        use crate::models::{Epic, Task};

        let dir = tempfile::tempdir().unwrap();
        let store = MarkdownStore::new(dir.path().join("project-management")).unwrap();
        let now = "2026-06-12T00:00:00Z".to_string();
        let epic = Epic {
            id: "epic-01a".into(),
            format_id: "EPC-001".into(),
            title: "Hook plane".into(),
            summary: None,
            status: EpicStatus::InProgress,
            work_type: "feat".into(),
            priority: "high".into(),
            pr_number: None,
            created_at: now.clone(),
            updated_at: now.clone(),
        };
        store.create_epic(&epic).unwrap();
        for (i, status) in [TaskStatus::Todo, TaskStatus::Complete].iter().enumerate() {
            store
                .create_task(&Task {
                    id: format!("task-0{i}"),
                    format_id: format!("TSK-001-00{i}"),
                    epic_id: "epic-01a".into(),
                    title: format!("t{i}"),
                    description: None,
                    status: *status,
                    work_type: "feat".into(),
                    priority: "medium".into(),
                    estimate: None,
                    acceptance: Vec::new(),
                    tests: Vec::new(),
                    branch: None,
                    pr_number: None,
                    created_at: now.clone(),
                    updated_at: now.clone(),
                    started_at: None,
                    completed_at: None,
                })
                .unwrap();
        }

        let digest = generate(dir.path());
        assert!(
            digest.contains("work: 1 epics (1 in progress) · 2 tasks (1 open)"),
            "{digest}"
        );
    }

    #[test]
    fn test_capabilities_line_ignores_unknown_status() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("docs")).unwrap();
        std::fs::write(
            dir.path().join("docs/capabilities.md"),
            "# caps\n\n\
             ```yaml\nid: CAP-001\nstatus: shipped\n```\n\n\
             ```yaml\nid: CAP-002\nstatus: bogus\n```\n\n\
             ```yaml\nid: CAP-003\nstatus: planned\n```\n",
        )
        .unwrap();
        let line = capabilities_line(dir.path()).unwrap();
        assert_eq!(line, "capabilities: 1 shipped · 1 planned");
    }

    /// A capability entry that lives inside an HTML comment (the scaffold
    /// template's worked example) must not be counted — a fresh repo's
    /// orient digest should show zero capabilities, not the doc-comment
    /// example, matching `capability::parse_capabilities`'s masking.
    #[test]
    fn test_capabilities_line_ignores_html_commented_entry() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("docs")).unwrap();
        std::fs::write(
            dir.path().join("docs/capabilities.md"),
            "# caps\n\n\
             <!-- worked example:\n\
             ## CAP-001 — example\n\n\
             ```yaml\nid: CAP-001\nname: example\nstatus: shipped\n```\n\
             -->\n",
        )
        .unwrap();
        assert_eq!(
            capabilities_line(dir.path()),
            None,
            "an HTML-commented example entry must not be counted"
        );
    }

    #[test]
    fn test_gates_line_reports_missing_hooks() {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-b", "main"]);
        let line = gates_line(dir.path());
        assert!(line.contains("pre-commit✗"));
        assert!(line.contains("git-guard✗"));
    }

    #[test]
    fn test_gates_line_detects_wired_hooks() {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-b", "main"]);
        let hooks = dir.path().join(".git/hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        std::fs::write(hooks.join("pre-commit"), "#!/bin/sh\n").unwrap();
        let line = gates_line(dir.path());
        assert!(line.contains("pre-commit✓"), "{line}");
        assert!(line.contains("commit-msg✗"));
    }
}
