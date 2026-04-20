//! Mechanical guard: no hardcoded `pf-{N}` or `ws-{stage}` strings in
//! production Rust source outside the authorized pathflow/sentinel modules.
//!
//! INF-TSK-048-001 AC #12: All phase/stage/event references in new code must
//! resolve through `GateConfig` or `LedgerEvent`, never through string
//! literals like `"pathflow-pf-6"` or `"ws-dev"`. The autorun.rs:3922 fix
//! replaced the last known hardcoded fallback; this test exists to prevent
//! regressions from future PRs.
//!
//! Whitelist: the sentinel / phase / gate modules and their test fixtures
//! legitimately need to name phases/stages as strings, as do a small number
//! of test fixtures that simulate sentinel file contents. Every other file
//! containing these patterns must use GateConfig or LedgerEvent.
//!
//! Failure emits a file:line list for every violation.

use std::fs;
use std::path::{Path, PathBuf};

/// Directories to walk under the workspace root. Relative to the workspace
/// root (the directory containing `Cargo.toml` with `[workspace]`).
const ROOTS: &[&str] = &["core/src", "cli/src"];

/// Files whose contents are allowed to reference `pf-{N}` or `ws-{stage}`
/// verbatim. These are the modules that DEFINE phase/stage identifiers, so
/// string literals are load-bearing there.
///
/// Paths are relative to the workspace root. Exact-match only — a file not
/// on this list that contains a literal will fail the test.
const ALLOWED_FILES: &[&str] = &[
    // Phase / sentinel / gate definitions — these modules own the canonical
    // phase/stage identifiers as `FromStr` input or enum discriminants.
    "core/src/pathflow/gates.rs",
    "core/src/pathflow/mod.rs",
    "core/src/pathflow/phase.rs",
    "core/src/pathflow/stage.rs",
    "core/src/pathflow/checkpoint.rs",
    "core/src/pathflow/sentinel.rs",
    "core/src/types/events.rs",
    "core/src/types/sentinel.rs",
    "core/src/types/stage.rs",
    "core/src/types/phase.rs",
    // Sentinel writer hook + checkpoint hooks need phase/stage strings to
    // pattern-match stage-complete messages and emit sentinel filenames.
    "core/src/hooks/post_tool_use.rs",
    "core/src/hooks/session_end.rs",
    "core/src/hooks/session_start.rs",
    "core/src/hooks/task_completed.rs",
    // Checkpoint system needs phase-id strings to manage task registration.
    "core/src/hooks/checkpoint/mod.rs",
    "core/src/hooks/checkpoint/complete.rs",
    "core/src/hooks/checkpoint/register.rs",
    // Pre-tool-use gate evaluator reads sentinel filenames.
    "core/src/hooks/pre_tool_use.rs",
    "core/src/hooks/gate_check.rs",
    // CLI entrypoint references phase names in `/cf-stack`, `/cf-resume`.
    "cli/src/cmd/stack.rs",
    "cli/src/cmd/resume.rs",
    "cli/src/cmd/doctor.rs",
    // Sentinel file manipulation.
    "core/src/session/sentinels.rs",
    // Diagnostic "doctor" output legitimately names expected sentinels to
    // produce human-readable failure messages.
    "core/src/doctor/mod.rs",
    // Batch-list TUI has two legitimate hits: (1) TUI phase-badge widget
    // colours pf-1..pf-7 and (2) the fetch_running_stage_names() fallback
    // default. Both are UI presentation, not logic.
    "core/src/tui/data.rs",
    "core/src/tui/widgets/phase_badge.rs",
    // cli/src/cmd/autorun.rs was previously allow-listed for the
    // hardcoded `name == "pathflow-pf-7"` literal in `check_pathflow_progress`.
    // WS-REV REV-MINOR-1 replaced that literal with a GateConfig lookup
    // via `resolve_session_complete_sentinel_name`. Remaining `pathflow-pf-`
    // mentions are doc comments (skipped by `is_doc_comment_line`), the
    // sentinel filename prefix pattern `name.starts_with("pathflow-pf-")`
    // (no digit → doesn't match `\bpf-[0-9]\b`), or the format template
    // `format!("pathflow-pf-{}", phase.index())` where the digit is
    // generated from GateConfig at runtime. No allow-list entry needed.
];

/// Walks `dir` recursively and collects every `.rs` file path.
fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

/// Match `\bpf-[0-9]\b` without regex deps — scan for "pf-" prefix, verify
/// the preceding char (if any) is a word boundary and the next char is a
/// digit followed by a word boundary.
fn find_pf_hits(text: &str) -> Vec<usize> {
    let bytes = text.as_bytes();
    let mut hits = Vec::new();
    let needle = b"pf-";
    let mut i = 0;
    while i + needle.len() < bytes.len() {
        if &bytes[i..i + needle.len()] == needle {
            let before_ok = i == 0 || !is_ident_byte(bytes[i - 1]);
            let digit_pos = i + needle.len();
            let after_digit = digit_pos + 1;
            if before_ok
                && bytes[digit_pos].is_ascii_digit()
                && (after_digit == bytes.len() || !is_ident_byte(bytes[after_digit]))
            {
                hits.push(i);
            }
        }
        i += 1;
    }
    hits
}

/// Match `\bws-(dev|plan|docs|rev|qa|test|sec)\b`.
fn find_ws_hits(text: &str) -> Vec<usize> {
    let stages: &[&[u8]] = &[b"dev", b"plan", b"docs", b"rev", b"qa", b"test", b"sec"];
    let bytes = text.as_bytes();
    let mut hits = Vec::new();
    let needle = b"ws-";
    let mut i = 0;
    while i + needle.len() <= bytes.len() {
        if &bytes[i..i + needle.len()] == needle {
            let before_ok = i == 0 || !is_ident_byte(bytes[i - 1]);
            if before_ok {
                let tail = &bytes[i + needle.len()..];
                for stage in stages {
                    if tail.len() >= stage.len() && &tail[..stage.len()] == *stage {
                        let boundary_idx = i + needle.len() + stage.len();
                        if boundary_idx == bytes.len() || !is_ident_byte(bytes[boundary_idx]) {
                            hits.push(i);
                            break;
                        }
                    }
                }
            }
        }
        i += 1;
    }
    hits
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn byte_to_line(text: &str, byte_offset: usize) -> usize {
    text[..byte_offset].bytes().filter(|&b| b == b'\n').count() + 1
}

/// Locate the workspace root by walking up from `CARGO_MANIFEST_DIR` until
/// we find a directory whose sibling `Cargo.toml` contains `[workspace]`.
///
/// `CARGO_MANIFEST_DIR` is set to the crate root by cargo at build time —
/// for codeflow-core tests that's `.../codeflow-cli/core`. The workspace
/// root is its parent (`.../codeflow-cli`).
fn workspace_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .expect("CARGO_MANIFEST_DIR has a parent")
        .to_path_buf()
}

#[test]
fn no_hardcoded_pathflow_names_in_production_source() {
    let root = workspace_root();
    let mut violations: Vec<String> = Vec::new();

    for subdir in ROOTS {
        let full = root.join(subdir);
        let mut files = Vec::new();
        collect_rs_files(&full, &mut files);

        for file in &files {
            let relative = file.strip_prefix(&root).map_or_else(
                |_| file.to_string_lossy().to_string(),
                |p| p.to_string_lossy().to_string(),
            );
            // Normalize path separators for cross-platform allow-list match.
            let relative = relative.replace('\\', "/");

            if ALLOWED_FILES.iter().any(|allowed| relative == *allowed) {
                continue;
            }

            let Ok(body) = fs::read_to_string(file) else {
                continue;
            };

            // Find the first `#[cfg(test)]` or the first `mod tests {` —
            // anything AT or BEYOND that offset is considered test scope and
            // exempt. Simpler + correct vs brace counting, which breaks on
            // braces inside string literals (format! / assert! messages).
            let test_scope_start = first_test_scope_offset(&body);

            let mut for_file: Vec<(usize, &'static str)> = Vec::new();

            for offset in find_pf_hits(&body) {
                if let Some(start) = test_scope_start {
                    if offset >= start {
                        continue;
                    }
                }
                if is_doc_comment_line(&body, offset) {
                    continue;
                }
                for_file.push((offset, "pf-N"));
            }
            for offset in find_ws_hits(&body) {
                if let Some(start) = test_scope_start {
                    if offset >= start {
                        continue;
                    }
                }
                if is_doc_comment_line(&body, offset) {
                    continue;
                }
                for_file.push((offset, "ws-{stage}"));
            }

            for (offset, pattern) in for_file {
                let line = byte_to_line(&body, offset);
                violations.push(format!("{relative}:{line}: hardcoded `{pattern}`"));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "hardcoded pathflow/stage names found in production source \
         (use GateConfig::load or LedgerEvent variants instead):\n{}",
        violations.join("\n")
    );
}

/// Byte offset of the first `#[cfg(test)]` or `mod tests {` marker in the
/// file, if any. Anything at or after this offset is treated as test scope
/// and exempt from the guard.
///
/// Rationale: brace-balance counting breaks on braces inside string literals
/// (e.g., `format!("{expected} hits")`), which caused earlier false
/// positives. Rust convention places test modules at the bottom of files,
/// marked with `#[cfg(test)]`, so a simple "after first marker" rule is both
/// precise and robust.
fn first_test_scope_offset(body: &str) -> Option<usize> {
    let markers = [
        "#[cfg(test)]",
        "mod tests {",
        "mod tests{",
        "mod test {",
        "mod test{",
    ];
    markers.iter().filter_map(|m| body.find(m)).min()
}

/// Skip matches inside `///` / `//!` doc comments and `//` line comments —
/// those don't produce runtime string literals. Simple heuristic: check the
/// start of the line for `//`.
fn is_doc_comment_line(body: &str, offset: usize) -> bool {
    let line_start = body[..offset].rfind('\n').map_or(0, |i| i + 1);
    let line_prefix: String = body[line_start..offset]
        .chars()
        .take_while(|c| c.is_whitespace() || *c == '/' || *c == '!')
        .collect();
    line_prefix.contains("//")
}

// --- Self-tests of the matcher (so the guard is itself tested) ---

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pf_matcher_finds_literal() {
        assert_eq!(find_pf_hits("let x = \"pf-6\";"), vec![9]);
    }

    #[test]
    fn pf_matcher_respects_word_boundary_before() {
        // "sopf-6" should NOT match — preceded by an ident byte.
        assert!(find_pf_hits("sopf-6").is_empty());
    }

    #[test]
    fn pf_matcher_respects_word_boundary_after() {
        // "pf-6x" should NOT match — followed by an ident byte.
        assert!(find_pf_hits("pf-6x").is_empty());
    }

    #[test]
    fn pf_matcher_matches_pathflow_pf_prefix() {
        // The real regression this guard protects against.
        let hits = find_pf_hits("let s = \"pathflow-pf-6\";");
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn ws_matcher_finds_all_stages() {
        let cases = [
            ("ws-dev", 1),
            ("ws-plan", 1),
            ("ws-docs", 1),
            ("ws-rev", 1),
            ("ws-qa", 1),
            ("ws-test", 1),
            ("ws-sec", 1),
            ("ws-unknown", 0),
            ("aws-dev", 0),
            ("ws-devx", 0),
        ];
        for (input, expected) in cases {
            assert_eq!(
                find_ws_hits(input).len(),
                expected,
                "input={input} expected {expected} hits"
            );
        }
    }

    #[test]
    fn doc_comment_is_skipped() {
        let body = "// pf-6 is the pr-pushed sentinel\nfn x() {}";
        // Found by regex but filtered by is_doc_comment_line.
        let hits = find_pf_hits(body);
        assert_eq!(hits.len(), 1);
        assert!(is_doc_comment_line(body, hits[0]));
    }

    #[test]
    fn first_test_scope_offset_finds_cfg_test_attr() {
        let body = "fn prod() {}\n#[cfg(test)]\nmod tests { let s = \"pf-6\"; }";
        let start = first_test_scope_offset(body).unwrap();
        let hit = find_pf_hits(body)[0];
        assert!(
            hit >= start,
            "hit at {hit} should be in test scope at {start}"
        );
    }

    #[test]
    fn first_test_scope_offset_finds_bare_mod_tests() {
        // Some files use `mod tests {` without a `#[cfg(test)]` attribute
        // (rare but permitted). The scope anchor picks up on either.
        let body = "fn prod() {}\nmod tests { let s = \"pf-6\"; }";
        let start = first_test_scope_offset(body).unwrap();
        let hit = find_pf_hits(body)[0];
        assert!(hit >= start);
    }

    #[test]
    fn first_test_scope_offset_none_when_no_marker() {
        assert!(first_test_scope_offset("fn x() {}").is_none());
    }

    #[test]
    fn pre_test_hit_is_flagged() {
        // A hit BEFORE any test marker is a real violation.
        let body = "const X: &str = \"pf-6\";\n#[cfg(test)]\nmod tests {}";
        let start = first_test_scope_offset(body).unwrap();
        let hit = find_pf_hits(body)[0];
        assert!(
            hit < start,
            "pre-test hit at {hit} vs test start at {start}"
        );
    }
}
