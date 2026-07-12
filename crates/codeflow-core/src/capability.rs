//! Capability registry parsing (`docs/capabilities.md`).
//!
//! The registry is the WHAT layer of the knowledge model (charter §5): one
//! entry per user-meaningful capability, each a heading plus a fenced yaml
//! block. This parser is shared by the `status` view (capability table) and
//! the `validate --docs` referential-integrity lint, which needs line
//! numbers for `file:line`-style messages.

use serde::Deserialize;

/// Legal capability status values (charter §5: planned → building →
/// shipped → deprecated; deprecate, never delete).
pub const CAPABILITY_STATUS_VALUES: &[&str] = &["planned", "building", "shipped", "deprecated"];

/// A parsed capability entry from the registry.
#[derive(Debug, Clone)]
pub struct CapabilityEntry {
    /// Capability id (`CAP-###`).
    pub id: String,
    pub name: String,
    pub area: String,
    /// Status string as written (validated by the docs lint, not here).
    pub status: String,
    /// Test tags proving the capability works.
    pub verified_by: Vec<String>,
    /// `EPC-###` ids that built or changed it.
    pub epics: Vec<String>,
    /// ADR ids that shaped it.
    pub adrs: Vec<String>,
    /// 1-based line number of the entry's yaml block opening fence.
    pub line: usize,
}

/// A problem found while parsing the registry (malformed yaml block).
#[derive(Debug, Clone)]
pub struct CapabilityParseIssue {
    /// 1-based line number of the offending yaml block's opening fence.
    pub line: usize,
    pub message: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RawCapability {
    id: String,
    name: String,
    area: String,
    status: String,
    verified_by: Vec<String>,
    epics: Vec<String>,
    adrs: Vec<String>,
}

/// Blank out every character inside an HTML comment (`<!-- ... -->`),
/// preserving newlines so 1-based line numbers stay accurate. An unterminated
/// comment runs to end of input. This keeps example capability entries that
/// live inside comments — such as the scaffold template's `<!-- ... ##
/// CAP-001 ... -->` — from being parsed as real entries (which would surface a
/// phantom capability named `<name>` in a fresh repo's `status`/`orient`).
fn mask_html_comments(content: &str) -> String {
    let mut out = String::with_capacity(content.len());
    let mut rest = content;
    while let Some(start) = rest.find("<!--") {
        out.push_str(&rest[..start]);
        let body = &rest[start..];
        let end = body[4..].find("-->").map_or(body.len(), |rel| 4 + rel + 3);
        for ch in body[..end].chars() {
            out.push(if ch == '\n' { '\n' } else { ' ' });
        }
        rest = &body[end..];
    }
    out.push_str(rest);
    out
}

/// Parse capability entries out of registry markdown content.
///
/// Scans for fenced ` ```yaml ` blocks whose yaml carries an `id` starting
/// with `CAP-`; other yaml blocks are ignored. Content inside HTML comment
/// blocks is skipped, so example entries embedded in comments are not parsed.
/// Malformed blocks that look like capability entries are reported as parse
/// issues, never silently dropped (charter principle 8: legible degradation).
#[must_use]
pub fn parse_capabilities(content: &str) -> (Vec<CapabilityEntry>, Vec<CapabilityParseIssue>) {
    let mut entries = Vec::new();
    let mut issues = Vec::new();

    let masked = mask_html_comments(content);
    let lines: Vec<&str> = masked.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        if trimmed != "```yaml" && trimmed != "```yml" {
            i += 1;
            continue;
        }
        let fence_line = i + 1; // 1-based
        let block_start = i + 1;
        let mut j = block_start;
        while j < lines.len() && lines[j].trim() != "```" {
            j += 1;
        }
        let block = lines[block_start..j].join("\n");
        i = j + 1;

        // Only yaml blocks that look like capability entries participate.
        if !block.contains("CAP-") {
            continue;
        }

        match serde_yaml::from_str::<RawCapability>(&block) {
            Ok(raw) if raw.id.starts_with("CAP-") => entries.push(CapabilityEntry {
                id: raw.id,
                name: raw.name,
                area: raw.area,
                status: raw.status,
                verified_by: raw.verified_by,
                epics: raw.epics,
                adrs: raw.adrs,
                line: fence_line,
            }),
            Ok(_) => {} // yaml mentioning CAP- without a CAP id — not an entry
            Err(e) => issues.push(CapabilityParseIssue {
                line: fence_line,
                message: format!("malformed capability yaml block: {e}"),
            }),
        }
    }

    (entries, issues)
}

#[cfg(test)]
mod tests {
    use super::*;

    const REGISTRY: &str = r"# myproject — capabilities

## CAP-001 — record validation

```yaml
id: CAP-001
name: record validation
area: engine
status: shipped
verified_by: [validate-core]
epics: [EPC-001]
adrs: [ADR-0001]
```

Validates epic and task frontmatter.

## CAP-002 — status view

```yaml
id: CAP-002
name: status view
area: engine
status: building
verified_by: []
epics: [EPC-002]
adrs: []
```

Generated status views.
";

    #[test]
    fn parses_all_entries() {
        let (entries, issues) = parse_capabilities(REGISTRY);
        assert!(issues.is_empty(), "unexpected issues: {issues:?}");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].id, "CAP-001");
        assert_eq!(entries[0].status, "shipped");
        assert_eq!(entries[0].verified_by, vec!["validate-core"]);
        assert_eq!(entries[0].epics, vec!["EPC-001"]);
        assert_eq!(entries[0].adrs, vec!["ADR-0001"]);
        assert_eq!(entries[1].id, "CAP-002");
        assert!(entries[1].verified_by.is_empty());
    }

    #[test]
    fn records_fence_line_numbers() {
        let (entries, _) = parse_capabilities(REGISTRY);
        // First fence is on line 5 of the registry constant.
        assert_eq!(entries[0].line, 5);
        assert!(entries[1].line > entries[0].line);
    }

    #[test]
    fn ignores_non_capability_yaml_blocks() {
        let content = "```yaml\nfoo: bar\n```\n";
        let (entries, issues) = parse_capabilities(content);
        assert!(entries.is_empty());
        assert!(issues.is_empty());
    }

    #[test]
    fn malformed_capability_block_is_an_issue() {
        let content = "intro\n\n```yaml\nid: CAP-003\nname: [unclosed\n```\n";
        let (entries, issues) = parse_capabilities(content);
        assert!(entries.is_empty());
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].line, 3);
        assert!(issues[0].message.contains("malformed"));
    }

    #[test]
    fn empty_content_yields_nothing() {
        let (entries, issues) = parse_capabilities("");
        assert!(entries.is_empty());
        assert!(issues.is_empty());
    }

    #[test]
    fn unclosed_fence_consumes_to_eof_without_panic() {
        let content = "```yaml\nid: CAP-009\nname: x\narea: a\nstatus: planned";
        let (entries, issues) = parse_capabilities(content);
        assert_eq!(entries.len(), 1, "issues: {issues:?}");
        assert_eq!(entries[0].id, "CAP-009");
    }

    #[test]
    fn skips_capability_entries_inside_html_comments() {
        // The scaffold template ships its example entry inside an HTML comment;
        // parsing it as real produced a phantom capability named `<name>` in a
        // fresh repo's status/orient. A comment-only registry yields nothing.
        const COMMENTED: &str = r"# myproject — capabilities

<!-- Entry format (example, not a real capability):

     ## CAP-001 — <name>

     ```yaml
     id: CAP-001
     name: <name>
     area: <area>
     status: planned
     verified_by: []
     epics: []
     adrs: []
     ```

     One paragraph describing the capability.
-->
";
        let (entries, issues) = parse_capabilities(COMMENTED);
        assert!(entries.is_empty(), "phantom entries: {entries:?}");
        assert!(issues.is_empty(), "unexpected issues: {issues:?}");
    }

    #[test]
    fn status_values_constant_matches_charter() {
        assert_eq!(
            CAPABILITY_STATUS_VALUES,
            &["planned", "building", "shipped", "deprecated"]
        );
    }
}
