//! Declaration checks shared with the browser's figure grammar. Rust embeds
//! the same rule data so `present check` needs no browser or Node installation.
use std::sync::LazyLock;

use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Rules {
    composition: Composition,
    fact_checks: Vec<String>,
    json_source_check: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Composition {
    min_width: u32,
    wide_max_width: u32,
    narrow_max_width: u32,
    min_height: u32,
    max_height: u32,
}

static RULES: LazyLock<Rules> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../web/src/figure-declaration-rules.json"))
        .expect("embedded figure rules must parse")
});

pub(crate) fn declaration_faults(declaration: &Value) -> Vec<String> {
    let mut faults = Vec::new();
    let figure = &declaration["figure"];
    let rules = &RULES.composition;
    if figure.get("layout").is_none() {
        for (variant, max_width) in [
            ("wide", rules.wide_max_width),
            ("narrow", rules.narrow_max_width),
        ] {
            let composition = &figure[variant];
            for (dimension, min, max) in [
                ("width", rules.min_width, max_width),
                ("height", rules.min_height, rules.max_height),
            ] {
                if !composition[dimension]
                    .as_f64()
                    .is_some_and(|number| (f64::from(min)..=f64::from(max)).contains(&number))
                {
                    faults.push(format!(
                        "{variant}.{dimension} must be {min} to {max} units"
                    ));
                }
            }
        }
    }
    if let Some(facts) = figure["facts"].as_array() {
        for (index, fact) in facts.iter().enumerate() {
            let name = format!("fact {}", index + 1);
            let kind = fact["check"]["kind"].as_str().unwrap_or_default();
            if !RULES.fact_checks.iter().any(|allowed| allowed == kind) {
                faults.push(format!(
                    "{name} check.kind must be one of {}",
                    RULES.fact_checks.join(", ")
                ));
                continue;
            }
            if let Some(source) = fact["source"].as_str() {
                let path = source.split('#').next().unwrap_or_default();
                if is_json_source(path) && kind != RULES.json_source_check {
                    faults.push(format!("{name}: a JSON source takes the json check"));
                }
                if kind == RULES.json_source_check
                    && (!is_json_source(path) || source.contains('#'))
                {
                    faults.push(format!(
                        "{name}: a json check reads a JSON file with no anchor"
                    ));
                }
            }
            match kind {
                "contains" if fact["value"] != true => {
                    faults.push(format!("{name}: a contains check asserts value true"));
                }
                "count-items"
                    if !fact["value"].as_f64().is_some_and(|value| {
                        (0.0..=9_007_199_254_740_991.0).contains(&value) && value.fract() == 0.0
                    }) =>
                {
                    faults.push(format!(
                        "{name}: a count-items check asserts a whole number"
                    ));
                }
                "json" if fact.get("value").is_none() => {
                    faults.push(format!("{name}: a json check asserts a value"));
                }
                _ => {}
            }
        }
    }
    faults
}

#[expect(
    clippy::case_sensitive_file_extension_comparisons,
    reason = "Source references follow the browser grammar's literal .json suffix"
)]
fn is_json_source(path: &str) -> bool {
    path.ends_with(".json")
}

#[cfg(test)]
mod tests {
    #[test]
    fn width_boundaries_match_the_browser_grammar() {
        let document: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/contract-v2/documents/v2-framed.json"
        ))
        .unwrap();
        for (variant, width, valid) in [
            ("wide", 119, false),
            ("wide", 120, true),
            ("wide", 720, true),
            ("wide", 721, false),
            ("narrow", 368, true),
            ("narrow", 369, false),
        ] {
            let mut declaration = document["blocks"][1]["declaration"].clone();
            declaration["figure"][variant]["width"] = width.into();
            let faults = super::declaration_faults(&declaration);
            assert_eq!(faults.is_empty(), valid, "{variant} {width}: {faults:?}");
        }
    }

    #[test]
    fn rule_data_matches_every_grammar_distribution() {
        let source = include_str!("../../../docs-portal/scripts/figure-declaration-rules.json");
        assert_eq!(
            source,
            include_str!("../web/src/figure-declaration-rules.json")
        );
        assert_eq!(
            source,
            include_str!(
                "../../../assets/docs-portal/starter/scripts/figure-declaration-rules.json"
            )
        );
    }
}
