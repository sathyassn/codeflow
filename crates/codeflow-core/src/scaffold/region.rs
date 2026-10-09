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

/// The managed block of a Markdown file as its exact bytes, from the start
/// of the first begin marker line to the end of the first end marker line
/// after it, line breaks included as written. The markers are the ones
/// [`extract_block`] and `codeflow update` recognise, so two files have the
/// same managed block exactly when these slices are equal. `None` when the
/// text has no complete marker pair.
#[must_use]
pub fn managed_span(content: &str) -> Option<&str> {
    let spans = line_spans(content);
    let line = |span: &LineSpan| &content[span.start..span.text_end];
    let begin = spans
        .iter()
        .position(|span| is_begin(line(span), RegionFormat::Markdown))?;
    let end = spans[begin..]
        .iter()
        .position(|span| is_end(line(span), RegionFormat::Markdown))?
        + begin;
    Some(&content[spans[begin].start..spans[end].text_end])
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

/// One line of a document: the byte offset where it starts, where its text
/// ends (before `\n` or `\r\n`), and where the next line starts.
#[derive(Debug, Clone, Copy)]
struct LineSpan {
    start: usize,
    text_end: usize,
    end: usize,
}

/// Line spans with the same line breaks as [`str::lines`], keeping offsets
/// so a replacement can copy everything around it byte for byte.
fn line_spans(text: &str) -> Vec<LineSpan> {
    let mut spans = Vec::new();
    let mut start = 0;
    for piece in text.split_inclusive('\n') {
        let end = start + piece.len();
        let body = piece.strip_suffix('\n').unwrap_or(piece);
        let body = if piece.ends_with('\n') {
            body.strip_suffix('\r').unwrap_or(body)
        } else {
            body
        };
        spans.push(LineSpan {
            start,
            text_end: start + body.len(),
            end,
        });
        start = end;
    }
    spans
}

/// `block` with each line break written as `eol`, the document's own
/// convention.
fn with_eol(block: &str, eol: &str) -> String {
    block.lines().collect::<Vec<_>>().join(eol)
}

/// Replace lines `first..=last` of `existing` with `block`. Bytes before the
/// first line and after the last line's break are copied unchanged; the
/// block takes the line break the replaced span used, so a CRLF document
/// stays CRLF and a missing final newline stays missing.
fn splice(existing: &str, spans: &[LineSpan], first: usize, last: usize, block: &str) -> String {
    let from = spans[first].start;
    let to = spans[last].end;
    let eol = &existing[spans[first].text_end..spans[first].end];
    let eol = if eol.is_empty() { "\n" } else { eol };
    let trailing = &existing[spans[last].text_end..to];
    let mut text = String::with_capacity(existing.len() + block.len());
    text.push_str(&existing[..from]);
    text.push_str(&with_eol(block, eol));
    text.push_str(trailing);
    text.push_str(&existing[to..]);
    text
}

/// Inserts or replaces the marked block in `existing`, touching nothing
/// outside the markers: the bytes around the block, line breaks included,
/// are kept exactly. Returns the new content and what happened.
#[must_use]
pub fn upsert_block(existing: &str, block: &str, format: RegionFormat) -> (String, BlockOutcome) {
    let spans = line_spans(existing);
    let lines: Vec<&str> = spans
        .iter()
        .map(|span| &existing[span.start..span.text_end])
        .collect();
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
        return (
            splice(existing, &spans, b, e, block),
            BlockOutcome::Replaced,
        );
    }

    // No markers in the dest. If the dest already contains the block's
    // interior content verbatim — a pre-markers install of this same region,
    // e.g. an asset that ships without markers (`CLAUDE.md.tmpl`) whose text
    // was written straight to disk once — replace that span with the marked
    // block instead of appending. Appending would duplicate the whole region.
    // Idempotent: the next run finds the markers and takes the branch above.
    if let Some((start, len)) = interior_line_span(&lines, block, format) {
        let text = splice(existing, &spans, start, start + len - 1, block);
        return (text, BlockOutcome::Replaced);
    }

    // No markers and no matching content: append after one blank line in the
    // document's own line-break convention, never touching existing content.
    let eol = if existing.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut text = existing.trim_end_matches(['\n', '\r']).to_string();
    if !text.is_empty() {
        text.push_str(eol);
        text.push_str(eol);
    }
    text.push_str(&with_eol(block, eol));
    text.push_str(eol);
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
    fn the_managed_span_is_the_block_as_written() {
        let doc = format!(
            "# Title\r\n\r\n{}\r\n\r\ntail\r\n",
            BLOCK_V1.replace('\n', "\r\n")
        );
        assert_eq!(managed_span(&doc).unwrap(), BLOCK_V1.replace('\n', "\r\n"));
        // Text around the block is not part of it; one byte inside is.
        let edited = format!("# Other\n\n{BLOCK_V1}\n\nmore tail\n");
        assert_eq!(managed_span(&edited), Some(BLOCK_V1));
        let inside = BLOCK_V1.replace("rules v1", "rules v1 ");
        assert_ne!(managed_span(&inside), Some(BLOCK_V1));
        // The short markers count, as `codeflow update` reads them.
        let short = "a\n<!-- codeflow:begin -->\nx\n<!-- codeflow:end -->\nb\n";
        assert_eq!(
            managed_span(short),
            Some("<!-- codeflow:begin -->\nx\n<!-- codeflow:end -->")
        );
        assert_eq!(managed_span("no block\n"), None);
    }

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
    fn replace_keeps_crlf_bytes_outside_the_block() {
        let doc = format!(
            "# Mine\r\nintro\r\n\r\n{}\r\n\r\ntail\r\n",
            BLOCK_V1.replace('\n', "\r\n")
        );
        let (out, outcome) = upsert_block(&doc, BLOCK_V2, RegionFormat::Markdown);
        assert_eq!(outcome, BlockOutcome::Replaced);
        assert_eq!(
            out,
            format!(
                "# Mine\r\nintro\r\n\r\n{}\r\n\r\ntail\r\n",
                BLOCK_V2.replace('\n', "\r\n")
            )
        );
        let (again, outcome) = upsert_block(&out, BLOCK_V2, RegionFormat::Markdown);
        assert_eq!(
            (again.as_str(), outcome),
            (out.as_str(), BlockOutcome::Unchanged)
        );
    }

    #[test]
    fn replace_keeps_a_missing_final_newline() {
        let doc = format!("{BLOCK_V1}\n\ntail");
        let (out, _) = upsert_block(&doc, BLOCK_V2, RegionFormat::Markdown);
        assert_eq!(out, format!("{BLOCK_V2}\n\ntail"));
        let (out, _) = upsert_block(BLOCK_V1, BLOCK_V2, RegionFormat::Markdown);
        assert_eq!(out, BLOCK_V2);
    }

    #[test]
    fn append_follows_a_crlf_document() {
        let (out, outcome) = upsert_block("mine\r\n", BLOCK_V2, RegionFormat::Markdown);
        assert_eq!(outcome, BlockOutcome::Appended);
        assert_eq!(
            out,
            format!("mine\r\n\r\n{}\r\n", BLOCK_V2.replace('\n', "\r\n"))
        );
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
