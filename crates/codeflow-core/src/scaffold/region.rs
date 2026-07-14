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
    let end = begin.and_then(|b| {
        lines[b..]
            .iter()
            .position(|l| is_end(l, format))
            .map(|e| e + b)
    });

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

    // No markers in the dest. If the dest already contains the block's
    // interior content verbatim — a pre-markers install of this same region,
    // e.g. an asset that ships without markers (`CLAUDE.md.tmpl`) whose text
    // was written straight to disk once — replace that span with the marked
    // block instead of appending. Appending would duplicate the whole region.
    // Idempotent: the next run finds the markers and takes the branch above.
    if let Some((start, len)) = interior_line_span(&lines, block, format) {
        let mut out: Vec<&str> = Vec::with_capacity(lines.len());
        out.extend_from_slice(&lines[..start]);
        out.extend(block.lines());
        out.extend_from_slice(&lines[start + len..]);
        let mut text = out.join("\n");
        if existing.ends_with('\n') || !text.ends_with('\n') {
            text.push('\n');
        }
        return (text, BlockOutcome::Replaced);
    }

    // No markers and no matching content: append, never touch existing content.
    let mut text = existing.trim_end_matches('\n').to_string();
    if !text.is_empty() {
        text.push_str("\n\n");
    }
    text.push_str(block);
    text.push('\n');
    (text, BlockOutcome::Appended)
}

/// The interior lines of a marked `block` (everything between the begin and end
/// markers), or an empty vec when the block has no interior.
fn block_interior(block: &str, format: RegionFormat) -> Vec<&str> {
    let lines: Vec<&str> = block.lines().collect();
    let Some(b) = lines.iter().position(|l| is_begin(l, format)) else {
        return Vec::new();
    };
    let Some(e) = lines[b..]
        .iter()
        .position(|l| is_end(l, format))
        .map(|e| e + b)
    else {
        return Vec::new();
    };
    if e > b + 1 {
        lines[b + 1..e].to_vec()
    } else {
        Vec::new()
    }
}

/// Finds the first run of lines in `haystack` matching the interior content of
/// `block` verbatim, returning `(start, len)`. Whole-line matching keeps the
/// match unambiguous — a region is many lines, so accidental collisions are
/// not a practical concern.
fn interior_line_span(
    haystack: &[&str],
    block: &str,
    format: RegionFormat,
) -> Option<(usize, usize)> {
    let needle = block_interior(block, format);
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|w| w == needle.as_slice())
        .map(|start| (start, needle.len()))
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
        assert_eq!(
            extract_block(&doc, RegionFormat::Markdown).unwrap(),
            BLOCK_V1
        );
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
    fn adopts_unmarked_region_in_place_without_duplicating() {
        // Regression: an asset that ships WITHOUT markers (e.g. CLAUDE.md.tmpl)
        // wraps its whole rendered body as the region. When the dest already
        // holds that identical body unmarked, update must REPLACE it in place,
        // not append — appending duplicated the entire region.
        let rendered = "@AGENTS.md\n\n## Notes\n- one\n- two\n";
        let block = wrap_block(rendered, RegionFormat::Markdown, "2.0.0");
        let dest = rendered; // pre-markers install: identical unmarked content

        let (out, outcome) = upsert_block(dest, &block, RegionFormat::Markdown);
        assert_eq!(
            outcome,
            BlockOutcome::Replaced,
            "unmarked-but-identical content must be adopted in place, not appended"
        );
        assert_eq!(
            out.matches("## Notes").count(),
            1,
            "region must appear exactly once, got:\n{out}"
        );
        assert!(out.contains("codeflow:managed:begin"));

        // Idempotent: a second run finds the markers and changes nothing.
        let (out2, outcome2) = upsert_block(&out, &block, RegionFormat::Markdown);
        assert_eq!(outcome2, BlockOutcome::Unchanged);
        assert_eq!(out2, out);
    }

    #[test]
    fn adopts_region_preserving_surrounding_project_content() {
        let rendered = "@AGENTS.md\n\n## Notes\n- one\n";
        let block = wrap_block(rendered, RegionFormat::Markdown, "2.0.0");
        let dest = format!("# Project header\n\n{rendered}\n## My own section\nkeep me\n");

        let (out, outcome) = upsert_block(&dest, &block, RegionFormat::Markdown);
        assert_eq!(outcome, BlockOutcome::Replaced);
        assert!(
            out.contains("# Project header"),
            "leading content preserved"
        );
        assert!(
            out.contains("## My own section") && out.contains("keep me"),
            "trailing project content preserved:\n{out}"
        );
        assert_eq!(out.matches("## Notes").count(), 1);
        assert!(out.contains("codeflow:managed:end"));
    }

    #[test]
    fn appends_when_no_marker_and_no_matching_content() {
        // Distinct content still appends (the pre-existing behavior).
        let block = wrap_block("region body\n", RegionFormat::Markdown, "2.0.0");
        let (out, outcome) = upsert_block("# Unrelated\nstuff\n", &block, RegionFormat::Markdown);
        assert_eq!(outcome, BlockOutcome::Appended);
        assert!(out.starts_with("# Unrelated\nstuff\n\n<!-- codeflow:managed:begin"));
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
