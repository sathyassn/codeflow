//! Text a shell may read more than one way (TSK-1375). The deletion reader
//! builds every reading, judges each one, and refuses the line when any
//! reading is refused. These tests judge text. Nothing here runs it, and
//! which reading a given shell takes is expected behaviour, not a checked
//! fact.

use std::path::Path;

use super::super::dangerous::DangerousModule;
use super::{composed_deletion_in, Found};
use crate::security::{CheckContext, SecurityModule, SecurityPolicy, Verdict};

/// The reader's finding for `line`, read in a fixed project directory.
fn judged(line: &str) -> Option<Found> {
    composed_deletion_in(line, Some(Path::new("/fixture")))
}

/// The `security.dangerous_commands` verdict for `line` on default policy.
fn verdict(line: &str) -> Option<Verdict> {
    let policy = SecurityPolicy::defaults();
    DangerousModule.check(&CheckContext {
        command: line,
        sandbox_bypass: false,
        current_branch: "",
        policy: &policy,
    })
}

/// Each line refuses under `security.dangerous_commands`, and the reader
/// finds the deletion itself.
fn assert_refused(lines: &[&str]) {
    let missed: Vec<String> = lines
        .iter()
        .filter(|line| verdict(line).is_none() || judged(line).is_none())
        .map(|line| format!("{line} -> reader {:?}", judged(line)))
        .collect();
    assert!(missed.is_empty(), "not refused:\n{}", missed.join("\n"));
}

/// Each line is allowed: the reader finds nothing and the module passes it.
fn assert_allowed(lines: &[&str]) {
    let refused: Vec<String> = lines
        .iter()
        .filter(|line| verdict(line).is_some() || judged(line).is_some())
        .map(|line| format!("{line} -> {:?}", verdict(line).map(|v| v.reason)))
        .collect();
    assert!(refused.is_empty(), "refused:\n{}", refused.join("\n"));
}

/// The double-parenthesis command form may be read as arithmetic or as a
/// subshell inside a subshell. A deletion in the subshell reading refuses,
/// whether the closing parentheses touch or not.
#[test]
fn a_deletion_in_the_subshell_reading_of_a_double_parenthesis_command_refuses() {
    assert_refused(&[
        "((rm -rf ~) )",
        "((rm -rf ~))",
        "(( rm -rf ~ ))",
        "((cd / && rm -rf *))",
        "((D=/; rm -rf \"$D\"))",
        "true && ((rm -rf ~)) >/dev/null",
        "(((rm -rf ~)))",
        "while ((rm -rf ~)); do :; done",
        "if ((rm -rf ~) ); then :; fi",
    ]);
}

/// The arithmetic expansion in an assignment may be read as a command
/// substitution when its text is not arithmetic. A deletion in that
/// reading refuses, and so does a later deletion of the value it gives.
#[test]
fn a_deletion_in_the_substitution_reading_of_an_arithmetic_assignment_refuses() {
    assert_refused(&[
        "X=$((rm -rf ~) )",
        "X=$((rm -rf ~))",
        "X=\"$((rm -rf ~) )\"",
        "X=$((rm -rf ~) ) true",
        "export X=$((rm -rf ~) )",
        "X=${Y:-$((rm -rf ~) )}",
        "X=($((rm -rf ~) ))",
        "X=$((echo /) ); rm -rf \"$X\"",
    ]);
}

/// Text that is also well-formed arithmetic keeps one value, but what its
/// substitution reading deletes is still judged, in an assignment and in an
/// argument.
#[test]
fn a_deletion_in_the_substitution_reading_of_well_formed_arithmetic_refuses() {
    assert_refused(&["X=$((rm -r / x))", "echo $((rm -r / x))"]);
}

/// Inside an expansion the reader does not resolve, a `$((…))` is also read
/// as a command substitution, as a `$(…)` there already is.
#[test]
fn a_deletion_in_the_substitution_reading_inside_an_unresolved_expansion_refuses() {
    assert_refused(&["echo ${V/x/$((rm -rf ~) )}", "X=${V/x/$((rm -rf ~))}"]);
}

/// A deletion in the arithmetic reading of either form is judged as before.
#[test]
fn a_deletion_in_the_arithmetic_reading_still_refuses() {
    assert_refused(&[
        "(( $(rm -rf ~) ))",
        "X=$(( $(rm -rf ~) ))",
        "D=(/ build); i=1; (( i = 0 )); rm -rf \"${D[i]}\"",
        "D=(/ build); i=1; X=$((i = 0)); rm -rf \"${D[i]}\"",
    ]);
}

/// Real arithmetic in both forms stays allowed, and the reader still reads
/// the rest of the line after it.
#[test]
fn real_arithmetic_stays_allowed() {
    assert_allowed(&[
        "((i++)); rm -rf ./build",
        "(( i < 3 )) && echo small; rm -rf ./build",
        "((count += 2)); rm -rf ./build",
        "i=0; while (( i < 3 )); do i=$((i + 1)); done; rm -rf ./build",
        "N=$((4 * 2)); echo \"$N\"; rm -rf ./build",
        "N=$(( $(wc -l < list.txt) + 1 )); echo \"$N\"",
        "X=$((Y + 1)) make test",
        "for ((i = 0; i < 3; i++)); do echo $i; done; rm -rf ./build",
        "[[ $((n % 2)) -eq 0 ]] && echo even; rm -rf ./build",
        "echo $((1 + 2)); rm -rf ./build",
        "OUT=$(( (a + b) * 2 )); rm -rf ./build",
    ]);
}

/// Text that does not surely parse as arithmetic also takes the value of
/// its substitution reading, in an assignment and in an argument: a
/// protected value refuses, and an unknown output refuses as unproven.
/// Real arithmetic keeps its one value, so a deletion through it is judged
/// as before.
#[test]
fn a_value_from_the_substitution_reading_is_judged() {
    assert_refused(&["rm -rf $((echo /) )", "rm -rf \"$((echo ~))\""]);
    for line in [
        "X=$((cat list) ); rm -rf \"out/$X\"",
        "rm -rf \"out/$((cat list))\"",
    ] {
        let found = judged(line).expect(line);
        assert!(found.unproven.is_some(), "{line}: {found:?}");
    }
    assert_allowed(&[
        "X=$((1 + 2)); rm -rf \"out/$X\"",
        "rm -rf \"out/$((i + 1))\"",
        "rm -rf \"out/$(( n * 2 ))\"",
    ]);
}

/// `inner` inside `levels` brace groups.
fn in_braces(inner: &str, levels: usize) -> String {
    format!("{}{inner}{}", "{ ".repeat(levels), "; }".repeat(levels))
}

/// A second reading, and a script parsed while reading the line, spend
/// what is left of the line's one nesting bound (TSK-1374) and never start
/// their own. Each line below nests within the bound counted on its own
/// parse, and past it counted across the reparse, so it refuses as a line
/// the reader could not finish; one level shallower it stays allowed.
#[test]
fn a_reparsed_reading_shares_the_nesting_bound() {
    let deep = super::MAX_NESTING - 3;
    // Three levels, as the subshell reading of the first form has.
    let inner = in_braces("true", 2);
    let forms: [(&str, String); 3] = [
        ("subshell reading", "((true))".to_string()),
        ("substitution", format!("echo \"$({inner})\"")),
        ("nested script", format!("bash -c '{inner}'")),
    ];
    for (form, text) in &forms {
        let shallower = in_braces(text, deep - 1);
        assert_allowed(&[shallower.as_str()]);
        let line = in_braces(text, deep);
        let found = judged(&line).unwrap_or_else(|| panic!("{form}: not refused"));
        assert!(found.unfinished, "{form}: {found:?}");
        assert!(verdict(&line).is_some(), "{form}: no verdict");
        let deletion = in_braces(&text.replace("true", "rm -rf /usr"), deep);
        assert!(verdict(&deletion).is_some(), "{form}: deletion not refused");
    }
}

/// Which arithmetic text is sure: numbers, names, subscripts, expansions
/// as operands and operators in a well-formed order. Anything else is not.
#[test]
fn only_well_formed_arithmetic_is_sure() {
    for sure in [
        "",
        "1",
        "i + 1",
        "i++",
        "--i",
        "count += 2",
        "a = b = 3",
        "(a + b) * 2",
        "n % 2 == 0",
        "i < 3 && j >= 4",
        "x ? y : z",
        "a[i + 1] << 2",
        "16#ff + 0x1F",
        "$n + ${m} * $(cat n) - $((k))",
        "-x, !y, ~z",
    ] {
        assert!(super::surely_arithmetic(sure), "{sure:?} is sure");
    }
    for unsure in [
        "echo /",
        "(echo /) ",
        "cat list",
        "rm -r / x y",
        "a b",
        "1 +",
        "x ? y",
        "'1'",
        "a; b",
        "1.5",
        "a[",
        "(1",
        "$ 1",
        "1 = 2",
        "a + b += c",
    ] {
        assert!(!super::surely_arithmetic(unsure), "{unsure:?} is not sure");
    }
    let deep = format!("{}1{}", "(".repeat(200), ")".repeat(200));
    assert!(
        !super::surely_arithmetic(&deep),
        "past the nesting it follows"
    );
}
