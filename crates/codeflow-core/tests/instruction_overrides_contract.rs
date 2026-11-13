//! TSK-1302 AC-1 and AC-3: the shipped text classes every override unit, the
//! floor list equals the units classed `floor`, and no unit a project may
//! override holds floor text.
//!
//! The phrase probe is a regression check: it catches the listed phrases in
//! an `overridable` unit, not every paraphrase of a floor. The declared class
//! and its review carry the rest.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use codeflow_core::scaffold::overrides::{sections, Floors, FLOOR_PHRASES};
use codeflow_core::scaffold::rule_map::{Kernel, UnitClass};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root resolves")
}

fn asset(rel: &str) -> String {
    let path = repo_root().join("assets/base").join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn kernel() -> Kernel {
    Kernel::parse(&asset("rule-map.toml")).expect("the kernel parses")
}

fn floors() -> Floors {
    Floors::parse(&asset("instruction-floors.toml")).expect("the floor list parses")
}

/// Every declared unit of the kernel: name, class, fixed reason, text.
fn kernel_units() -> Vec<(String, Option<UnitClass>, Option<String>, String)> {
    let kernel = kernel();
    let rules = kernel.rules.iter().map(|rule| {
        (
            format!("rule:{}", rule.id),
            rule.class,
            rule.fixed_reason.clone(),
            format!("{} {}", rule.title, rule.text),
        )
    });
    let moments = kernel.moments.iter().map(|moment| {
        (
            format!("moment:{}", moment.key),
            moment.class,
            moment.fixed_reason.clone(),
            format!("{} {}", moment.when, moment.action),
        )
    });
    rules.chain(moments).collect()
}

/// Every shipped managed skill file: (skill, text).
fn skills() -> Vec<(String, String)> {
    let dir = repo_root().join("assets/base/agents/skills");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path().join("SKILL.md");
        if path.is_file() {
            let name = entry.file_name().to_string_lossy().into_owned();
            out.push((name, std::fs::read_to_string(&path).unwrap()));
        }
    }
    out.sort();
    out
}

#[test]
fn every_kernel_unit_declares_a_class_and_every_fixed_unit_a_reason() {
    for (name, class, reason, _) in kernel_units() {
        let class = class.unwrap_or_else(|| panic!("{name} declares no class"));
        assert_eq!(
            class == UnitClass::Fixed,
            reason.as_deref().is_some_and(|r| !r.trim().is_empty()),
            "{name}: a fixed unit, and only a fixed unit, carries fixed_reason"
        );
    }
}

#[test]
fn the_floor_list_is_exactly_the_units_classed_floor() {
    let classed: BTreeSet<String> = kernel_units()
        .into_iter()
        .filter(|(_, class, _, _)| *class == Some(UnitClass::Floor))
        .map(|(name, ..)| name)
        .collect();
    let mut listed = BTreeSet::new();
    for floor in &floors().floors {
        for unit in &floor.units {
            assert!(listed.insert(unit.clone()), "{unit} is listed twice");
        }
    }
    assert_eq!(listed, classed);
}

#[test]
fn the_floors_are_the_git_floor_and_the_non_relaxable_command_class() {
    let floors = floors();
    let names: Vec<&str> = floors.floors.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["the git floor", "the non-relaxable command class"]);
    let pinned = [
        "moment:refusal",
        "moment:secret",
        "moment:standing-rule",
        "rule:authority",
        "rule:forge-identity",
        "rule:git",
    ];
    for unit in pinned {
        assert!(floors.floor_of(unit).is_some(), "{unit} must stay a floor");
    }
}

#[test]
fn review_units_are_fixed() {
    for (name, class, _, _) in kernel_units() {
        if name == "rule:review" || name == "moment:review" || name == "moment:delegate" {
            assert_eq!(class, Some(UnitClass::Fixed), "{name}");
        }
    }
}

#[test]
fn no_overridable_unit_holds_floor_text() {
    for (name, class, _, text) in kernel_units() {
        if class != Some(UnitClass::Overridable) {
            continue;
        }
        let lower = text.to_lowercase();
        for phrase in FLOOR_PHRASES {
            assert!(
                !lower.contains(&phrase.to_lowercase()),
                "{name} is overridable but holds the floor phrase {phrase:?}"
            );
        }
    }
    for (skill, text) in skills() {
        let doc = sections::parse(&text).unwrap_or_else(|e| panic!("{skill}: {}", e.what()));
        for section in doc.sections() {
            let class = section
                .class
                .unwrap_or_else(|| panic!("{skill}#{} declares no class", section.id));
            if class != UnitClass::Overridable {
                continue;
            }
            let lower = section.body.to_lowercase();
            for phrase in FLOOR_PHRASES {
                assert!(
                    !lower.contains(&phrase.to_lowercase()),
                    "{skill}#{} is overridable but holds the floor phrase {phrase:?}",
                    section.id
                );
            }
        }
    }
}

#[test]
fn the_precedence_line_is_not_a_unit() {
    let template = asset("AGENTS.md.tmpl");
    assert!(
        template.contains("Precedence:"),
        "the precedence line moved"
    );
    for (name, _, _, text) in kernel_units() {
        assert!(
            !text.contains("Precedence:"),
            "{name} holds the precedence line"
        );
    }
}

#[test]
fn files_that_state_floor_text_carry_no_section_markers() {
    for rel in [
        "rules/git-rules.md",
        "rules/workflow-discipline.md",
        "agents/skills/cf-model-orchestrator/resources/quality/irreversible.md",
        "agents/skills/cf-model-orchestrator/SKILL.md",
    ] {
        assert!(
            !asset(rel).contains("codeflow:section"),
            "{rel} states floor text and must carry no section marker"
        );
    }
}

#[test]
fn every_section_marker_parses_and_the_tests_have_one_to_override() {
    let mut declared = Vec::new();
    for (skill, text) in skills() {
        let doc = sections::parse(&text).unwrap_or_else(|e| panic!("{skill}: {}", e.what()));
        declared.extend(doc.sections().map(|s| format!("{skill}#{}", s.id)));
    }
    assert!(
        declared.contains(&"cf-present#completion".to_string()),
        "{declared:?}"
    );
}
