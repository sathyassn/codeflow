use super::*;

fn entry(target: &str) -> Entry {
    Entry {
        target: target.to_string(),
        title: None,
        text: None,
        action: None,
        body: None,
    }
}

fn row(target: &str, replaced: &str, replacement: &str) -> AppliedUnit {
    AppliedUnit {
        entry: entry(target),
        replaced: replaced.to_string(),
        replacement: replacement.to_string(),
    }
}

const SKILL: &str = "# Skill\n\nintro\n\n<!-- codeflow:section id=loop class=overridable -->\n## The loop\n\nbody one\n\n## Next\n\nafter\n";

#[test]
fn targets_parse_only_in_the_three_declared_shapes() {
    assert_eq!(
        Target::parse("rule:outcome"),
        Some(Target::Rule("outcome".into()))
    );
    assert_eq!(
        Target::parse("moment:build"),
        Some(Target::Moment("build".into()))
    );
    assert_eq!(
        Target::parse("cf-present#completion"),
        Some(Target::Section {
            skill: "cf-present".into(),
            id: "completion".into()
        })
    );
    for bad in [
        "outcome",
        "rule:",
        "rule:Out come",
        "cf-present#",
        "#x",
        "a#b#c",
        "rule:../x",
    ] {
        assert_eq!(Target::parse(bad), None, "{bad}");
    }
}

#[test]
fn a_row_value_is_one_line_without_marker_text() {
    assert!(row_value_ok("Name the result.", false));
    for bad in [
        "",
        "  ",
        "two\nlines",
        "a <!-- b",
        "x --> y",
        "CodeFlow:begin",
    ] {
        assert!(!row_value_ok(bad, false), "{bad:?}");
    }
    assert!(row_value_ok("a | b", false));
    assert!(!row_value_ok("a | b", true), "a table cell refuses a pipe");
}

#[test]
fn a_row_replaces_exactly_one_matching_line() {
    let text = "head\n- one\n- two\n";
    let units = [row("rule:one", "- one", "- uno")];
    let out = apply_units(text, &units).unwrap();
    assert_eq!(out, "head\n- uno\n- two\n");
    assert!(only_units_changed(text, &out, &units));
    assert_eq!(
        apply_units("- one\n- one\n", &units),
        None,
        "two matches refuse"
    );
    assert_eq!(apply_units("- two\n", &units), None, "no match refuses");
}

#[test]
fn a_section_replaces_only_its_body() {
    let unit = AppliedUnit {
        entry: entry("skill#loop"),
        replaced: "\nbody one\n\n".into(),
        replacement: "our body\n".into(),
    };
    let out = apply_units(SKILL, std::slice::from_ref(&unit)).unwrap();
    assert!(out.contains(
        "<!-- codeflow:section id=loop class=overridable -->\n## The loop\nour body\n## Next"
    ));
    assert!(only_units_changed(SKILL, &out, std::slice::from_ref(&unit)));
    let stale = AppliedUnit {
        replaced: "not the shipped body\n".into(),
        ..unit
    };
    assert_eq!(apply_units(SKILL, &[stale]), None);
}

#[test]
fn a_change_outside_the_units_is_detected() {
    let units = [row("rule:one", "- one", "- uno")];
    assert!(!only_units_changed(
        "- one\n- two\n",
        "- uno\n- dos\n",
        &units
    ));
    assert!(!only_units_changed("- one\n", "- uno\n- extra\n", &units));
    let section = AppliedUnit {
        entry: entry("skill#loop"),
        replaced: "\nbody one\n\n".into(),
        replacement: "ours\n".into(),
    };
    let moved_heading = SKILL
        .replace("\nbody one\n\n", "ours\n")
        .replace("## Next", "## Renamed");
    assert!(!only_units_changed(SKILL, &moved_heading, &[section]));
}

#[test]
fn the_floor_list_names_its_floor_for_a_unit() {
    let floors = Floors::parse(
        "schema_version = 1\n[[floor]]\nname = \"the git floor\"\nrefusal = \"never\"\nunits = [\"rule:git\"]\n",
    )
    .unwrap();
    assert_eq!(floors.floor_of("rule:git").unwrap().name, "the git floor");
    assert!(floors.floor_of("rule:outcome").is_none());
    assert!(Floors::parse("schema_version = 2\nfloor = []\n").is_err());
    assert!(Floors::parse(
        "schema_version = 1\n[[floor]]\nname = \"x\"\nrefusal = \"y\"\nunits = []\nextra = 1\n"
    )
    .is_err());
}

const GUARDED: &str = "# Skill\n\n<!-- codeflow:section id=loop class=overridable -->\n## The loop\n\nbody one\n\n<!-- codeflow:section id=gate class=floor -->\n## The gate\n\nNever skip the gate.\n";

#[test]
fn a_floor_section_after_an_overridable_one_stays_byte_identical() {
    let unit = AppliedUnit {
        entry: entry("skill#loop"),
        replaced: "\nbody one\n\n".into(),
        replacement: "````\n```\nnested\n```\n````\n".into(),
    };
    assert_eq!(sections::check_body(&unit.replacement, 2), Ok(()));
    let out = apply_units(GUARDED, std::slice::from_ref(&unit)).unwrap();
    let floor =
        "<!-- codeflow:section id=gate class=floor -->\n## The gate\n\nNever skip the gate.\n";
    assert!(out.ends_with(floor), "{out}");
    assert!(only_units_changed(
        GUARDED,
        &out,
        std::slice::from_ref(&unit)
    ));
    let weakened = out.replace("Never skip the gate.", "Skip the gate.");
    assert!(!only_units_changed(GUARDED, &weakened, &[unit]));
}

#[test]
fn a_body_that_would_hide_later_text_is_refused() {
    use sections::BodyProblem;
    for open in ["````\n```\nx\n", "~~~\n```\n", "```\n~~~\n", "```\n``\n"] {
        assert_eq!(
            sections::check_body(open, 2),
            Err(BodyProblem::OpenFence),
            "{open:?}"
        );
    }
    for closed in ["```\nx\n````\n", "~~~~\n~~~\n~~~~\n", "``` rust\nx\n```\n"] {
        assert_eq!(sections::check_body(closed, 2), Ok(()), "{closed:?}");
    }
    for setext in ["Smuggled\n===\n", "Smuggled\n---\n"] {
        assert_eq!(
            sections::check_body(setext, 2),
            Err(BodyProblem::EndsSection),
            "{setext:?}"
        );
    }
    assert_eq!(
        sections::check_body("Lower\n---\n", 1),
        Ok(()),
        "a level-2 setext under a level-1 section"
    );
    assert_eq!(
        sections::check_body("text\n\n---\n", 2),
        Ok(()),
        "a thematic break"
    );
    for html in ["<!-- hidden\n", "<pre>\nx\n", "<script>\n", "<?php\n"] {
        assert_eq!(
            sections::check_body(html, 2),
            Err(BodyProblem::OpenHtmlBlock),
            "{html:?}"
        );
    }
    assert_eq!(sections::check_body("<!-- note -->\ntext\n", 2), Ok(()));
    assert_eq!(sections::check_body("<pre>\nx\n</pre>\n", 2), Ok(()));
}
