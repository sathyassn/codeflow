//! A strict evaluator for the JSON Schema 2020-12 keywords the presentation
//! schemas use, so tests can check real CLI output against the published
//! contracts. An unknown assertion keyword fails the check rather than being
//! skipped.

use std::collections::HashMap;

use serde_json::Value;

/// The schemas a `$ref` may name by `$id`, each checked against its own root.
pub struct Registry {
    roots: HashMap<String, Value>,
}

const ANNOTATIONS: [&str; 9] = [
    "$schema",
    "$id",
    "$defs",
    "$comment",
    "title",
    "default",
    "format",
    "contentEncoding",
    "description",
];

impl Registry {
    pub fn new(schemas: impl IntoIterator<Item = Value>) -> Self {
        let roots = schemas
            .into_iter()
            .map(|schema| {
                let id = schema["$id"].as_str().expect("schema $id").to_string();
                (id, schema)
            })
            .collect();
        Self { roots }
    }

    /// Every violation of `value` against the schema registered as `id`.
    pub fn errors(&self, id: &str, value: &Value) -> Vec<String> {
        let root = &self.roots[id];
        let mut errors = Vec::new();
        self.check(root, root, value, "$", &mut errors);
        errors
    }

    #[allow(
        clippy::too_many_lines,
        clippy::cast_precision_loss,
        reason = "one match arm per keyword; lengths in these schemas are far below 2^52"
    )]
    fn check(
        &self,
        schema: &Value,
        root: &Value,
        value: &Value,
        at: &str,
        errors: &mut Vec<String>,
    ) {
        let Some(schema) = schema.as_object() else {
            errors.push(format!("{at}: schema is not an object"));
            return;
        };
        for (keyword, rule) in schema {
            if ANNOTATIONS.contains(&keyword.as_str()) || keyword.starts_with("x-") {
                continue;
            }
            match keyword.as_str() {
                "$ref" => {
                    let reference = rule.as_str().expect("$ref string");
                    let (target, target_root) =
                        if let Some(name) = reference.strip_prefix("#/$defs/") {
                            (&root["$defs"][name], root)
                        } else {
                            let other = self
                                .roots
                                .get(reference)
                                .unwrap_or_else(|| panic!("unregistered $ref {reference}"));
                            (other, other)
                        };
                    assert!(!target.is_null(), "unresolved $ref {reference}");
                    self.check(target, target_root, value, at, errors);
                }
                "type" => {
                    let allowed: Vec<&str> = match rule {
                        Value::String(name) => vec![name.as_str()],
                        Value::Array(names) => names.iter().filter_map(Value::as_str).collect(),
                        _ => panic!("type must be a string or an array"),
                    };
                    if !allowed.iter().any(|name| has_type(value, name)) {
                        errors.push(format!("{at}: expected type {allowed:?}, found {value}"));
                    }
                }
                "const" => {
                    if value != rule {
                        errors.push(format!("{at}: expected {rule}, found {value}"));
                    }
                }
                "enum" => {
                    if !rule.as_array().expect("enum array").contains(value) {
                        errors.push(format!("{at}: {value} is not one of {rule}"));
                    }
                }
                "required" => {
                    if let Some(object) = value.as_object() {
                        for name in rule.as_array().expect("required array") {
                            let name = name.as_str().expect("required name");
                            if !object.contains_key(name) {
                                errors.push(format!("{at}: missing required {name}"));
                            }
                        }
                    }
                }
                "properties" => {
                    if let Some(object) = value.as_object() {
                        for (name, property) in rule.as_object().expect("properties object") {
                            if let Some(child) = object.get(name) {
                                self.check(property, root, child, &format!("{at}.{name}"), errors);
                            }
                        }
                    }
                }
                "additionalProperties" => {
                    assert_eq!(rule, &Value::Bool(false), "only closed objects are used");
                    if let Some(object) = value.as_object() {
                        let declared = schema.get("properties").and_then(Value::as_object);
                        for name in object.keys() {
                            if !declared.is_some_and(|declared| declared.contains_key(name)) {
                                errors.push(format!("{at}: unexpected property {name}"));
                            }
                        }
                    }
                }
                "items" => {
                    if let Some(items) = value.as_array() {
                        for (index, item) in items.iter().enumerate() {
                            self.check(rule, root, item, &format!("{at}[{index}]"), errors);
                        }
                    }
                }
                "minItems" | "maxItems" => {
                    if let Some(items) = value.as_array() {
                        bound(keyword, rule, items.len() as f64, at, errors);
                    }
                }
                "minLength" | "maxLength" => {
                    if let Some(text) = value.as_str() {
                        bound(keyword, rule, text.chars().count() as f64, at, errors);
                    }
                }
                "minimum" | "maximum" => {
                    if let Some(number) = value.as_f64() {
                        bound(keyword, rule, number, at, errors);
                    }
                }
                "pattern" => {
                    if let Some(text) = value.as_str() {
                        let pattern = regex::Regex::new(rule.as_str().expect("pattern string"))
                            .expect("valid pattern");
                        if !pattern.is_match(text) {
                            errors.push(format!("{at}: {text:?} does not match {pattern}"));
                        }
                    }
                }
                "oneOf" | "anyOf" => {
                    let matches = rule
                        .as_array()
                        .expect("branches")
                        .iter()
                        .filter(|branch| self.passes(branch, root, value, at))
                        .count();
                    if (keyword == "oneOf" && matches != 1) || matches == 0 {
                        errors.push(format!("{at}: {matches} {keyword} branches matched"));
                    }
                }
                "allOf" => {
                    for branch in rule.as_array().expect("allOf array") {
                        self.check(branch, root, value, at, errors);
                    }
                }
                "not" => {
                    if self.passes(rule, root, value, at) {
                        errors.push(format!("{at}: matched a not schema"));
                    }
                }
                "if" => {
                    if self.passes(rule, root, value, at) {
                        if let Some(then) = schema.get("then") {
                            self.check(then, root, value, at, errors);
                        }
                    }
                }
                "then" => {}
                other => panic!("unsupported schema keyword {other} at {at}"),
            }
        }
    }

    fn passes(&self, schema: &Value, root: &Value, value: &Value, at: &str) -> bool {
        let mut errors = Vec::new();
        self.check(schema, root, value, at, &mut errors);
        errors.is_empty()
    }
}

fn has_type(value: &Value, name: &str) -> bool {
    match name {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        "number" => value.is_number(),
        "integer" => value.is_i64() || value.is_u64(),
        other => panic!("unknown type {other}"),
    }
}

fn bound(keyword: &str, rule: &Value, actual: f64, at: &str, errors: &mut Vec<String>) {
    let limit = rule.as_f64().expect("numeric bound");
    let ok = if keyword.starts_with("min") {
        actual >= limit
    } else {
        actual <= limit
    };
    if !ok {
        errors.push(format!("{at}: {keyword} {limit} violated by {actual}"));
    }
}
