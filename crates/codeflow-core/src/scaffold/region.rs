//! Managed-region handling for marker-delimited text files (charter §4.3.2).
//!
//! For `AGENTS.md`-style files codeflow owns exactly one marked block:
//!
//! ```text
//! <!-- codeflow:managed:begin scaffold=2.0.0 -->
//! ...codeflow-maintained content...
//! <!-- codeflow:managed:end -->
//! ```
//!
//! Everything outside the markers is project-owned and never touched. The
//! short forms `<!-- codeflow:begin -->` / `<!-- codeflow:end -->` are also
//! recognized, as are `#`-comment markers for gitignore-style files.

use super::manifest::RegionFormat;

/// Marker style for a region format. `Json` has no text markers.
fn is_marker(line: &str, format: RegionFormat, terminator: &str) -> bool {
    let trimmed = line.trim();
    let well_formed = match format {
        RegionFormat::Markdown => trimmed.starts_with("<!--") && trimmed.ends_with("-->"),
        RegionFormat::Hash => trimmed.starts_with('#'),
        RegionFormat::Json => return false,
    };
    well_formed && trimmed.contains("codeflow:") && trimmed.contains(terminator)
}

fn is_begin(line: &str, format: RegionFormat) -> bool {
    is_marker(line, format, "begin")
}

fn is_end(line: &str, format: RegionFormat) -> bool {
    is_marker(line, format, "end")
}

/// Extracts the marked block (inclusive of both marker lines) from `content`.
#[must_use]
pub fn extract_block(content: &str, format: RegionFormat) -> Option<String> {
    let lines: Vec<&str> = content.lines().collect();
    let begin = lines.iter().position(|l| is_begin(l, format))?;
    let end = lines[begin..].iter().position(|l| is_end(l, format))? + begin;
    Some(lines[begin..=end].join("\n"))
}

/// Wraps `content` in fresh markers of the given format (used when an asset
/// ships without markers of its own).
#[must_use]
pub fn wrap_block(content: &str, format: RegionFormat, version: &str) -> String {
    let (begin, end) = match format {
        RegionFormat::Markdown => (
            format!("<!-- codeflow:managed:begin scaffold={version} -->"),
            "<!-- codeflow:managed:end -->".to_string(),
        ),
        RegionFormat::Hash | RegionFormat::Json => (
            format!("# codeflow:managed:begin scaffold={version}"),
            "# codeflow:managed:end".to_string(),
        ),
    };
    format!("{begin}\n{}\n{end}", content.trim_end_matches('\n'))
}

/// Result of [`upsert_block`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockOutcome {
    /// Existing block content already matched.
    Unchanged,
    /// Existing markers found; block content replaced.
    Replaced,
    /// No markers found; block appended at end of file.
    Appended,
}

/// Inserts or replaces the marked block in `existing`, touching nothing
/// outside the markers. Returns the new content and what happened.
#[must_use]
pub fn upsert_block(existing: &str, block: &str, format: RegionFormat) -> (String, BlockOutcome) {
    let lines: Vec<&str> = existing.lines().collect();
    let begin = lines.iter().position(|l| is_begin(l, format));
    let end = begin.and_then(|b| lines[b..].iter().position(|l| is_end(l, format)).map(|e| e + b));

    if let (Some(b), Some(e)) = (begin, end) {
        let current = lines[b..=e].join("\n");
        if current == block {
            return (existing.to_string(), BlockOutcome::Unchanged);
        }
        let mut out: Vec<&str> = Vec::with_capacity(lines.len());
        out.extend_from_slice(&lines[..b]);
        out.extend(block.lines());
        out.extend_from_slice(&lines[e + 1..]);
        let mut text = out.join("\n");
        if existing.ends_with('\n') || !text.ends_with('\n') {
            text.push('\n');
        }
        return (text, BlockOutcome::Replaced);
    }

    // No markers: append, never touch existing content.
    let mut text = existing.trim_end_matches('\n').to_string();
    if !text.is_empty() {
        text.push_str("\n\n");
    }
    text.push_str(block);
    text.push('\n');
    (text, BlockOutcome::Appended)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLOCK_V1: &str =
        "<!-- codeflow:managed:begin scaffold=1.0.0 -->\nrules v1\n<!-- codeflow:managed:end -->";
    const BLOCK_V2: &str =
        "<!-- codeflow:managed:begin scaffold=2.0.0 -->\nrules v2\n<!-- codeflow:managed:end -->";

    #[test]
    fn extracts_block_inclusive_of_markers() {
        let doc = format!("# Title\n\n{BLOCK_V1}\n\ntail\n");
        assert_eq!(extract_block(&doc, RegionFormat::Markdown).unwrap(), BLOCK_V1);
    }

    #[test]
    fn recognizes_short_marker_form() {
        let doc = "<!-- codeflow:begin -->\nx\n<!-- codeflow:end -->\n";
        assert!(extract_block(doc, RegionFormat::Markdown).is_some());
    }

    #[test]
    fn replace_preserves_everything_outside() {
        let doc = format!("# Mine\nuser intro\n\n{BLOCK_V1}\n\nuser tail\n");
        let (out, outcome) = upsert_block(&doc, BLOCK_V2, RegionFormat::Markdown);
        assert_eq!(outcome, BlockOutcome::Replaced);
        assert!(out.contains("user intro") && out.contains("user tail"));
        assert!(out.contains("rules v2") && !out.contains("rules v1"));
    }

    #[test]
    fn append_when_no_markers() {
        let (out, outcome) = upsert_block("# Existing\nbody\n", BLOCK_V1, RegionFormat::Markdown);
        assert_eq!(outcome, BlockOutcome::Appended);
        assert!(out.starts_with("# Existing\nbody\n\n<!-- codeflow:managed:begin"));
    }

    #[test]
    fn unchanged_when_block_matches() {
        let doc = format!("intro\n{BLOCK_V1}\n");
        let (out, outcome) = upsert_block(&doc, BLOCK_V1, RegionFormat::Markdown);
        assert_eq!(outcome, BlockOutcome::Unchanged);
        assert_eq!(out, doc);
    }

    #[test]
    fn hash_markers_for_gitignore_style() {
        let block = wrap_block(".env\n*.pem", RegionFormat::Hash, "2.0.0");
        let (out, outcome) = upsert_block("target/\n", &block, RegionFormat::Hash);
        assert_eq!(outcome, BlockOutcome::Appended);
        assert!(out.contains("target/") && out.contains("# codeflow:managed:begin"));
        let (out2, outcome2) = upsert_block(&out, &block, RegionFormat::Hash);
        assert_eq!(outcome2, BlockOutcome::Unchanged);
        assert_eq!(out2, out);
    }
}
