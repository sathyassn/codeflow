//! `{{PLACEHOLDER}}` substitution for templated assets.
//!
//! Substitution is map-based and conservative: only keys present in the
//! context are replaced. Unknown placeholders (e.g. `{{EPIC_ID}}` inside the
//! pm templates, `{{PRODUCT_PURPOSE}}` in docs seeds) are agent/human-fill
//! slots and must survive scaffolding untouched.

use std::collections::BTreeMap;

/// Substitution context built by init/update from project facts.
#[derive(Debug, Clone, Default)]
pub struct TemplateContext {
    values: BTreeMap<String, String>,
}

impl TemplateContext {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.values.insert(key.into(), value.into());
    }

    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    /// Replaces every `{{KEY}}` whose `KEY` exists in this context.
    #[must_use]
    pub fn substitute(&self, content: &str) -> String {
        let mut out = String::with_capacity(content.len());
        let mut rest = content;
        while let Some(start) = rest.find("{{") {
            out.push_str(&rest[..start]);
            let after = &rest[start + 2..];
            if let Some(end) = after.find("}}") {
                let key = &after[..end];
                if let Some(value) = self.values.get(key) {
                    out.push_str(value);
                } else {
                    out.push_str("{{");
                    out.push_str(key);
                    out.push_str("}}");
                }
                rest = &after[end + 2..];
            } else {
                out.push_str("{{");
                rest = after;
            }
        }
        out.push_str(rest);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> TemplateContext {
        let mut c = TemplateContext::new();
        c.set("PROJECT_NAME", "demo");
        c.set("STACK", "rust");
        c
    }

    #[test]
    fn substitutes_known_keys() {
        assert_eq!(
            ctx().substitute("# {{PROJECT_NAME}} ({{STACK}})"),
            "# demo (rust)"
        );
    }

    #[test]
    fn leaves_unknown_placeholders_intact() {
        assert_eq!(
            ctx().substitute("epic: {{EPIC_ID}} on {{PROJECT_NAME}}"),
            "epic: {{EPIC_ID}} on demo"
        );
    }

    #[test]
    fn handles_unterminated_braces() {
        assert_eq!(ctx().substitute("a {{PROJECT_NAME"), "a {{PROJECT_NAME");
        assert_eq!(ctx().substitute("tail {{"), "tail {{");
    }
}
