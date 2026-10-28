//! What `codeflow update --diff` writes for a file it changed (TSK-1029).
//!
//! The export never prints bytes that come from the project's files or from
//! the recorded baseline: either can hold a secret, and both are writable by
//! the project, so neither can say what is safe to show. The only reference
//! for what may be shown is the asset embedded in this binary.
//!
//! - **Fail closed by format.** The format comes from the path, and a path
//!   with no rule here is withheld whole. A file that does not parse in its
//!   format is withheld too.
//! - **JSON, TOML and YAML: changed key paths.** The section lists each key
//!   path whose value this update changed, with `CodeFlow`'s shipped value at
//!   that path and, for the file before and after, only a marker: whether
//!   it holds the shipped value, another value, a value at a path the
//!   shipped file lacks, or nothing. A key name is shown only when the
//!   shipped file has that key at that place; keys the project added are
//!   counted, never named, since a key can be a secret too. A list shows the
//!   shipped items this update writes and counts the rest.
//! - **Markdown, code and other line-based text: shipped lines.** The
//!   section shows each line this update writes that is a line of the
//!   embedded asset, counts every other line it writes or removes, and says
//!   whether the file before and after matches the shipped version.
//!
//! A shipped value is shown only when the embedded asset holds it as
//! written: a value filled in from the project's settings by templating is
//! named as such, not shown.

use std::collections::HashSet;

use serde_json::Value;

/// The embedded asset a section is judged against.
#[derive(Clone, Copy)]
pub struct Reference<'a> {
    /// The asset as update writes it: rendered with the project's template
    /// values, the merge-region block, or the composed default.
    pub rendered: &'a str,
    /// The asset's bytes as embedded in the binary, before templating.
    /// `None` withholds every shipped value and line.
    pub raw: Option<&'a str>,
}

/// The marker for a value equal to the shipped one at its path.
pub const SAME: &str = "<project value: same as shipped>";
/// The marker for a value that differs from the shipped one at its path.
pub const DIFFERS: &str = "<project value: differs>";
/// The marker for a value at a path the shipped file does not have.
pub const ADDED: &str = "<added by project>";
/// The marker for a path the file before this update did not have.
pub const ABSENT: &str = "<absent>";
/// The marker for a path this update removed.
pub const REMOVED: &str = "<removed>";

/// Extensions of line-based text that `CodeFlow` ships: instructions, code,
/// styles and pages.
const TEXT_EXTENSIONS: [&str; 13] = [
    "md", "markdown", "txt", "py", "js", "mjs", "cjs", "ts", "sh", "css", "html", "htm", "rules",
];

/// The `--diff` section for `dest`, from the file before and after this
/// update, judged against `reference`.
#[must_use]
pub fn section(dest: &str, old: &str, new: &str, reference: Reference<'_>) -> String {
    let body = match Kind::of(dest) {
        Kind::Structured(format) => structured(format, old, new, reference),
        Kind::Text => Some(text(old, new, reference)),
        Kind::Unknown => None,
    };
    match body {
        Some(body) => format!("=== {dest}\n{body}\n"),
        None => format!(
            "=== {dest}\n(content withheld: the export shows only values and lines from the \
             shipped version, and this file's format has no rule for finding them, or the file \
             did not parse)\n\n"
        ),
    }
}

/// How a path's contents are reported.
enum Kind {
    Structured(Format),
    Text,
    /// No rule: the default, so a new format is withheld until it has one.
    Unknown,
}

impl Kind {
    fn of(dest: &str) -> Kind {
        let extension = std::path::Path::new(dest)
            .extension()
            .and_then(|ext| ext.to_str())
            .map(str::to_ascii_lowercase);
        match extension.as_deref() {
            Some("json") => Kind::Structured(Format::Json),
            Some("toml") => Kind::Structured(Format::Toml),
            Some("yaml" | "yml") => Kind::Structured(Format::Yaml),
            Some(ext) if TEXT_EXTENSIONS.contains(&ext) => Kind::Text,
            // The managed files with no extension: the ignore list and the
            // git hook scripts `CodeFlow` ships.
            None if dest == ".gitignore" || dest.starts_with(".codeflow/git-hooks/") => Kind::Text,
            _ => Kind::Unknown,
        }
    }
}

#[derive(Clone, Copy)]
enum Format {
    Json,
    Toml,
    Yaml,
}

impl Format {
    fn parse(self, text: &str) -> Option<Value> {
        match self {
            Format::Json => serde_json::from_str(text).ok(),
            Format::Toml => toml::from_str::<toml::Value>(text)
                .ok()
                .and_then(|value| serde_json::to_value(value).ok()),
            Format::Yaml => serde_yaml::from_str(text).ok(),
        }
    }
}

fn structured(format: Format, old: &str, new: &str, reference: Reference<'_>) -> Option<String> {
    let old = format.parse(old)?;
    let new = format.parse(new)?;
    // A reference that does not parse owns nothing: every key is the
    // project's and no shipped value is shown.
    let shipped = format.parse(reference.rendered);
    let raw = reference.raw.and_then(|raw| format.parse(raw));
    let mut lines = vec![
        "(changed key paths; values and key names from the project are never shown)".to_string(),
    ];
    walk(
        &mut lines,
        "",
        Sides {
            old: Some(&old),
            new: Some(&new),
        },
        shipped.as_ref(),
        raw.as_ref(),
    );
    if lines.len() == 1 {
        lines.push("no key path changed".to_string());
    }
    Some(lines.join("\n") + "\n")
}

/// One place in the file before and after this update.
#[derive(Clone, Copy)]
struct Sides<'a> {
    old: Option<&'a Value>,
    new: Option<&'a Value>,
}

impl Sides<'_> {
    fn objects(&self) -> bool {
        [self.old, self.new]
            .iter()
            .flatten()
            .all(|value| value.is_object())
    }

    fn lists(&self) -> bool {
        [self.old, self.new]
            .iter()
            .flatten()
            .all(|value| value.is_array())
    }
}

fn walk(
    out: &mut Vec<String>,
    path: &str,
    sides: Sides<'_>,
    shipped: Option<&Value>,
    raw: Option<&Value>,
) {
    if sides.old == sides.new {
        return;
    }
    if sides.objects() {
        let mut keys: Vec<&String> = Vec::new();
        for map in [sides.new, sides.old].into_iter().flatten() {
            for key in map.as_object().into_iter().flat_map(|map| map.keys()) {
                if !keys.contains(&key) {
                    keys.push(key);
                }
            }
        }
        let mut project = Tally::default();
        for key in keys {
            let child = Sides {
                old: sides.old.and_then(|old| old.get(key)),
                new: sides.new.and_then(|new| new.get(key)),
            };
            if child.old == child.new {
                continue;
            }
            match shipped.and_then(|shipped| shipped.get(key)) {
                // The key is the shipped file's, so its name is shown.
                Some(owned) => walk(
                    out,
                    &join(path, key),
                    child,
                    Some(owned),
                    raw.and_then(|raw| raw.get(key)),
                ),
                None => project.count(child),
            }
        }
        if let Some(line) = project.line(path) {
            out.push(line);
        }
    } else if sides.lists() {
        list(out, path, sides, shipped, raw);
    } else {
        out.push(format!(
            "{}\n  shipped: {}\n  before:  {}\n  after:   {}",
            label(path),
            shown(shipped, raw),
            marker(sides.old, shipped, ABSENT),
            marker(sides.new, shipped, REMOVED),
        ));
    }
}

/// The changes under one object to keys the shipped file does not have.
#[derive(Default)]
struct Tally {
    added: usize,
    removed: usize,
    changed: usize,
}

impl Tally {
    fn count(&mut self, sides: Sides<'_>) {
        match (sides.old, sides.new) {
            (None, Some(_)) => self.added += 1,
            (Some(_), None) => self.removed += 1,
            _ => self.changed += 1,
        }
    }

    fn line(&self, path: &str) -> Option<String> {
        let parts: Vec<String> = [
            (self.added, "added"),
            (self.removed, "removed"),
            (self.changed, "changed"),
        ]
        .iter()
        .filter(|(count, _)| *count > 0)
        .map(|(count, what)| format!("{count} {what}"))
        .collect();
        (!parts.is_empty()).then(|| {
            format!(
                "{}\n  keys the shipped file does not have (names not shown): {}",
                label(path),
                parts.join(", ")
            )
        })
    }
}

fn list(
    out: &mut Vec<String>,
    path: &str,
    sides: Sides<'_>,
    shipped: Option<&Value>,
    raw: Option<&Value>,
) {
    let items = |value: Option<&Value>| -> Vec<Value> {
        value.and_then(Value::as_array).cloned().unwrap_or_default()
    };
    let (mut old, new) = (items(sides.old), items(sides.new));
    let mut added = Vec::new();
    for item in new {
        match old.iter().position(|kept| *kept == item) {
            Some(index) => {
                old.remove(index);
            }
            None => added.push(item),
        }
    }
    let shipped_items = items(shipped);
    let raw_items = items(raw);
    let (written, other): (Vec<Value>, Vec<Value>) = added
        .into_iter()
        .partition(|item| shipped_items.contains(item) && raw_items.contains(item));
    let mut lines = vec![format!("{} (list)", label(path))];
    lines.extend(written.iter().map(|item| format!("  + {item}")));
    if !other.is_empty() {
        lines.push(format!("  {} other items added (not shown)", other.len()));
    }
    if !old.is_empty() {
        lines.push(format!("  {} items removed (not shown)", old.len()));
    }
    lines.push(format!("  before:  {}", marker(sides.old, shipped, ABSENT)));
    lines.push(format!(
        "  after:   {}",
        marker(sides.new, shipped, REMOVED)
    ));
    out.push(lines.join("\n"));
}

fn join(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.to_string()
    } else {
        format!("{path}.{key}")
    }
}

fn label(path: &str) -> &str {
    if path.is_empty() {
        "(top level)"
    } else {
        path
    }
}

/// The shipped value at a path, shown only when the embedded asset holds it
/// as written.
fn shown(shipped: Option<&Value>, raw: Option<&Value>) -> String {
    match shipped {
        None => "<not shipped>".to_string(),
        Some(value) if raw != Some(value) => "<filled from the project's settings>".to_string(),
        Some(Value::Object(_)) => "<an object>".to_string(),
        Some(Value::Array(items)) => format!("<a list of {} items>", items.len()),
        Some(value) => value.to_string(),
    }
}

fn marker(value: Option<&Value>, shipped: Option<&Value>, missing: &'static str) -> &'static str {
    match (value, shipped) {
        (None, _) => missing,
        (Some(_), None) => ADDED,
        (Some(value), Some(shipped)) if value == shipped => SAME,
        (Some(_), Some(_)) => DIFFERS,
    }
}

fn text(old: &str, new: &str, reference: Reference<'_>) -> String {
    let shipped_lines: HashSet<&str> = reference
        .raw
        .map(|raw| raw.split('\n').collect())
        .unwrap_or_default();
    let patch = diffy::create_patch(old, new);
    let mut written = Vec::new();
    let (mut other, mut removed) = (0usize, 0usize);
    for hunk in patch.hunks() {
        for line in hunk.lines() {
            match line {
                diffy::Line::Insert(line) => {
                    let body = line.strip_suffix('\n').unwrap_or(line);
                    if shipped_lines.contains(body) {
                        written.push(format!("+ {body}"));
                    } else {
                        other += 1;
                    }
                }
                diffy::Line::Delete(_) => removed += 1,
                diffy::Line::Context(_) => {}
            }
        }
    }
    let mut lines = vec![
        "(lines this update writes from the shipped version; lines from the project are never \
         shown)"
            .to_string(),
        format!("before: {}", likeness(reference.rendered, old)),
        format!("after:  {}", likeness(reference.rendered, new)),
    ];
    lines.extend(written);
    if other > 0 {
        lines.push(format!("{other} other lines written (not shown)"));
    }
    if removed > 0 {
        lines.push(format!("{removed} lines removed (not shown)"));
    }
    lines.join("\n") + "\n"
}

/// Whether `file` matches the shipped version, or by how many lines it
/// differs.
fn likeness(shipped: &str, file: &str) -> String {
    let changed = diffy::create_patch(shipped, file)
        .hunks()
        .iter()
        .flat_map(diffy::Hunk::lines)
        .filter(|line| !matches!(line, diffy::Line::Context(_)))
        .count();
    if changed == 0 {
        "matches the shipped version".to_string()
    } else {
        format!("differs from the shipped version in {changed} lines")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic value no output may show.
    const SECRET: &str = "opaque-neighbour-value-31337";

    fn reference(shipped: &str) -> Reference<'_> {
        Reference {
            rendered: shipped,
            raw: Some(shipped),
        }
    }

    fn assert_hidden(text: &str) {
        assert!(!text.contains(SECRET), "{text}");
        assert!(!text.contains("271828"), "{text}");
    }

    #[test]
    fn a_changed_shipped_value_shows_the_shipped_value_and_markers() {
        let shipped = r#"{"env": {"PCT": "35"}, "model": "y"}"#;
        let old = format!(r#"{{"env": {{"TOKEN": "{SECRET}", "PCT": "50"}}, "model": "y"}}"#);
        let new = format!(r#"{{"env": {{"TOKEN": "{SECRET}", "PCT": "35"}}, "model": "y"}}"#);
        let out = section(".claude/settings.json", &old, &new, reference(shipped));
        assert_hidden(&out);
        assert!(!out.contains("TOKEN"), "a project key is named: {out}");
        assert!(!out.contains("\"50\""), "{out}");
        assert!(out.contains("env.PCT\n  shipped: \"35\""), "{out}");
        assert!(out.contains(&format!("before:  {DIFFERS}")), "{out}");
        assert!(out.contains(&format!("after:   {SAME}")), "{out}");
        assert!(!out.contains("model"), "an unchanged path is listed: {out}");
    }

    #[test]
    fn a_replaced_shipped_scalar_never_shows_the_replacement() {
        let shipped = r#"{"$schema": "https://example.invalid/schema.json", "on": false, "n": 1}"#;
        let url = format!("https://user:{SECRET}@example.invalid/schema.json");
        // A string, a type change and a number replace shipped scalars; the
        // update then writes the shipped values back.
        let old = format!(r#"{{"$schema": "{url}", "on": "{SECRET}", "n": 271828}}"#);
        let out = section(".claude/settings.json", &old, shipped, reference(shipped));
        assert_hidden(&out);
        assert!(
            out.contains(&format!(
                "$schema\n  shipped: \"https://example.invalid/schema.json\"\n  before:  {DIFFERS}"
            )),
            "{out}"
        );
        // And the other way: the update keeps the project's replacement.
        let out = section(".claude/settings.json", shipped, &old, reference(shipped));
        assert_hidden(&out);
        assert!(out.contains(&format!("after:   {DIFFERS}")), "{out}");
    }

    #[test]
    fn project_keys_and_values_are_counted_never_shown() {
        let shipped = r#"{"permissions": {"allow": ["Read"]}}"#;
        let old = format!(
            r#"{{"permissions": {{"allow": ["Read", "Bash(curl {SECRET})"]}},
                "{SECRET}": 1, "api-key": "{SECRET}", "tools": {{"{SECRET}": 271828}}}}"#
        );
        let out = section(".claude/settings.json", &old, shipped, reference(shipped));
        assert_hidden(&out);
        assert!(!out.contains("api-key") && !out.contains("tools"), "{out}");
        assert!(
            out.contains("keys the shipped file does not have (names not shown): 3 removed"),
            "{out}"
        );
        assert!(
            out.contains("permissions.allow (list)\n  1 items removed (not shown)"),
            "{out}"
        );
    }

    #[test]
    fn a_list_shows_only_the_shipped_items_it_writes() {
        let shipped = r#"{"allow": ["Read", "Write"]}"#;
        let old = format!(r#"{{"allow": ["Read", "{SECRET}"]}}"#);
        let new = format!(r#"{{"allow": ["Read", "{SECRET}", "Write", "x-{SECRET}"]}}"#);
        let out = section("a.json", &old, &new, reference(shipped));
        assert_hidden(&out);
        assert!(out.contains("  + \"Write\""), "{out}");
        assert!(out.contains("1 other items added (not shown)"), "{out}");
    }

    #[test]
    fn a_value_filled_from_the_project_is_named_not_shown() {
        let raw = r#"{"name": "{{PROJECT_NAME}}", "v": 1}"#;
        let rendered = format!(r#"{{"name": "{SECRET}", "v": 1}}"#);
        let out = section(
            "x.json",
            r#"{"name": "old", "v": 1}"#,
            &rendered,
            Reference {
                rendered: &rendered,
                raw: Some(raw),
            },
        );
        assert_hidden(&out);
        assert!(
            out.contains("<filled from the project's settings>"),
            "{out}"
        );
    }

    #[test]
    fn toml_and_separator_variants_are_hidden() {
        let shipped = "model = \"m\"\n";
        let old = format!(
            "model = \"{SECRET}\"\n\"api-key\" = \"{SECRET}\"\n\"api key\" = 271828\n\
             [tools.httpHeaders]\nAuthorization = \"{SECRET}\"\n\
             [tools.\"http-headers\"]\nX = 271828\n"
        );
        let out = section(".codex/config.toml", &old, shipped, reference(shipped));
        assert_hidden(&out);
        assert!(!out.contains("api") && !out.contains("http"), "{out}");
        assert!(out.contains("model\n  shipped: \"m\""), "{out}");
    }

    #[test]
    fn yaml_aliases_merge_keys_and_documents_never_show_raw() {
        let shipped = "name: ci\njobs:\n  test:\n    runs-on: ubuntu\n";
        let aliases = format!(
            "name: ci\nx-base: &base\n  NOTE: {SECRET}\njobs:\n  test:\n    runs-on: ubuntu\n    \
             env:\n      <<: *base\n      COPY: *base\n"
        );
        let documents = format!("{shipped}---\nsecret: {SECRET}\n");
        let malformed = format!("name: ci\nenv: [\n  NOTE: {SECRET}\n");
        for old in [aliases, documents, malformed] {
            let out = section(
                ".github/workflows/codeflow-ci.yml",
                &old,
                shipped,
                reference(shipped),
            );
            assert_hidden(&out);
            assert!(
                out.contains("content withheld") || out.contains("names not shown"),
                "{out}"
            );
        }
    }

    #[test]
    fn a_workflow_step_name_and_run_are_never_shown() {
        let shipped = "jobs:\n  scan:\n    steps:\n    - uses: actions/checkout@v1\n";
        let old = format!(
            "jobs:\n  scan:\n    steps:\n    - uses: actions/checkout@v1\n    - name: {SECRET}\n      \
             run: echo {SECRET}\n"
        );
        let out = section(
            ".github/workflows/ci.yml",
            &old,
            shipped,
            reference(shipped),
        );
        assert_hidden(&out);
        assert!(
            out.contains("jobs.scan.steps (list)\n  1 items removed"),
            "{out}"
        );
    }

    #[test]
    fn text_shows_shipped_lines_and_counts_the_rest() {
        let shipped = "# Title\n\nshipped line\nnew shipped line\n";
        let old = format!("# Title\n\nshipped line\nexport KEY={SECRET}\n");
        for dest in ["AGENTS.md", ".codeflow/git-hooks/pre-push", ".gitignore"] {
            let out = section(dest, &old, shipped, reference(shipped));
            assert_hidden(&out);
            assert!(out.contains("+ new shipped line"), "{out}");
            assert!(out.contains("1 lines removed (not shown)"), "{out}");
            assert!(
                out.contains("before: differs from the shipped version in 2 lines"),
                "{out}"
            );
            assert!(out.contains("after:  matches the shipped version"), "{out}");
        }
    }

    #[test]
    fn a_line_only_the_rendering_has_is_never_shown() {
        // A rendered line filled from the project's settings, or a line a
        // tampered baseline or the project wrote, is not a line of the
        // embedded asset.
        let raw = "# {{PROJECT_NAME}}\nshipped\n";
        let rendered = format!("# {SECRET}\nshipped\n");
        let out = section(
            "AGENTS.md",
            "shipped\n",
            &rendered,
            Reference {
                rendered: &rendered,
                raw: Some(raw),
            },
        );
        assert_hidden(&out);
        assert!(out.contains("1 other lines written (not shown)"), "{out}");
    }

    #[test]
    fn an_unknown_format_or_unparsable_side_is_withheld() {
        for dest in ["notes.ini", ".env", "bin/run", "config.properties"] {
            let out = section(dest, &format!("K={SECRET}\n"), "K=1\n", reference("K=1\n"));
            assert_hidden(&out);
            assert!(out.contains("content withheld"), "{dest}: {out}");
        }
        let bad = format!("{{\"env\": {{\"K\": \"{SECRET}\"");
        let out = section(".claude/settings.json", &bad, "{}", reference("{}"));
        assert_hidden(&out);
        assert!(out.contains("content withheld"), "{out}");
        let out = section(
            ".codex/config.toml",
            &format!("x = [{SECRET}"),
            "x = 1\n",
            reference("x = 1\n"),
        );
        assert_hidden(&out);
        assert!(out.contains("content withheld"), "{out}");
    }

    #[test]
    fn a_reference_that_does_not_parse_owns_nothing() {
        let out = section(
            ".claude/settings.json",
            &format!(r#"{{"model": "{SECRET}"}}"#),
            r#"{"model": "m"}"#,
            reference("{"),
        );
        assert_hidden(&out);
        assert!(!out.contains("model"), "{out}");
        assert!(out.contains("1 changed"), "{out}");
    }
}
