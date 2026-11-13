//! The reader stops at a bound and must not read the stop as "nothing found"
//! (TSK-1374). These tests judge text. Nothing here runs it.

use std::path::Path;

use super::{composed_deletion_in, Found};

/// A deletion of a protected directory, the text every fixture wraps.
const DELETION: &str = "rm -rf /usr";

/// The guard's refusal for a line it could not finish reading.
const UNFINISHED: &str = "could not finish reading";

/// Nesting levels to judge, from none to well past what the reader follows.
const LEVELS: std::ops::RangeInclusive<usize> = 0..=9;

fn judged(line: &str) -> Option<Found> {
    composed_deletion_in(line, Some(Path::new("/fixture")))
}

fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// `body` wrapped `levels` times by `wrap`.
fn wrapped(body: &str, levels: usize, wrap: impl Fn(&str) -> String) -> String {
    (0..levels).fold(body.to_string(), |text, _| wrap(&text))
}

/// `body` at the end of a chain of `length` function calls.
fn function_chain(body: &str, length: usize) -> String {
    let mut parts = vec![format!("f0() {{ {body}; }}")];
    parts.extend((1..length).map(|at| format!("f{at}() {{ f{}; }}", at - 1)));
    parts.push(format!("f{}", length.saturating_sub(1)));
    parts.join("; ")
}

/// Every name in `names` first holds a safe value, then each takes the value
/// of the next, the last taking `end`: a loop body needs one pass per name
/// before the first name holds `end`.
fn value_chain(names: usize, end: &str) -> (String, String) {
    let seed = (0..=names)
        .map(|at| format!("V{at}=./safe"))
        .collect::<Vec<_>>()
        .join("; ");
    let mut steps: Vec<String> = (0..names).map(|at| format!("V{at}=$V{}", at + 1)).collect();
    steps.push(format!("V{names}={end}"));
    (seed, steps.join("; "))
}

/// A line that assigns `name` each of `values`, on separate branches.
fn branches(name: &str, values: &[&str]) -> String {
    let joined = values
        .iter()
        .map(|value| format!("if probe; then {name}={value}; fi"))
        .collect::<Vec<_>>()
        .join("; ");
    format!("{name}=./start; {joined}")
}

/// `count` commands that print safe text, then one that prints `last`.
fn printed_lines(count: usize, last: &str) -> String {
    let lines = (0..count)
        .map(|at| format!("echo ./item{at}"))
        .collect::<Vec<_>>()
        .join("; ");
    format!("{{ {lines}; echo {last}; }}")
}

#[test]
fn a_protected_deletion_is_refused_at_every_script_nesting_level() {
    for levels in LEVELS {
        for shell in ["sh -c", "bash -c", "bash -lc", "zsh -c"] {
            let line = wrapped(DELETION, levels, |text| format!("{shell} {}", quoted(text)));
            assert!(judged(&line).is_some(), "{shell} at level {levels}");
        }
        let line = wrapped(DELETION, levels, |text| format!("eval {}", quoted(text)));
        assert!(judged(&line).is_some(), "eval at level {levels}");
        let line = wrapped(DELETION, levels, |text| format!("echo $({text})"));
        assert!(judged(&line).is_some(), "substitution at level {levels}");
    }
}

#[test]
fn a_protected_deletion_is_refused_at_every_function_chain_length() {
    for length in (1..=24).chain([200]) {
        let line = function_chain(DELETION, length);
        assert!(judged(&line).is_some(), "chain of {length}");
    }
}

#[test]
fn a_protected_deletion_is_refused_behind_any_number_of_launch_wrappers() {
    for count in 0..=24 {
        for wrapper in ["env", "nohup", "nice", "exec", "command", "time"] {
            let line = format!("{}{DELETION}", format!("{wrapper} ").repeat(count));
            assert!(judged(&line).is_some(), "{wrapper} {count} times");
        }
    }
}

#[test]
fn a_find_that_runs_a_nested_remover_is_refused_at_every_level() {
    for levels in LEVELS {
        let body = wrapped("rm -rf {}", levels, |text| {
            format!("sh -c {}", quoted(text))
        });
        let line = format!("find /usr -exec sh -c {} \\;", quoted(&body));
        assert!(judged(&line).is_some(), "level {levels}");
    }
}

#[test]
fn a_stop_in_nesting_gives_the_named_refusal_whatever_the_line_holds() {
    let harmless = function_chain("true", 40);
    let found = judged(&harmless).expect("a line the reader cannot finish refuses");
    assert!(found.unproven.is_some(), "{found:?}");
    let refusals = crate::hooks::exec_guard::evaluate_floor(&harmless);
    assert_eq!(refusals.len(), 1, "{refusals:?}");
    assert_eq!(refusals[0].rule, "security.dangerous_commands");
    assert!(refusals[0].message.contains(UNFINISHED), "{refusals:?}");
    assert!(refusals[0].message.contains("directly"), "{refusals:?}");
}

#[test]
fn brace_alternatives_past_the_value_bound_stay_unknown() {
    let list = (0..200)
        .map(|at| format!("./item{at}"))
        .collect::<Vec<_>>()
        .join(",");
    let line = format!("rm -rf {{{list},/usr}}");
    assert!(judged(&line).is_some());
    let line = format!("rm -rf {{{list},./last}}");
    let found = judged(&line).expect("the unread alternatives stay unknown");
    assert!(found.unproven.is_some(), "{found:?}");
    assert!(judged(&format!("echo {{{list},/usr}}")).is_none());
}

#[test]
fn brace_expressions_past_the_group_bound_stay_unknown() {
    let line = format!("rm -rf ./{}", "{a..a}".repeat(100));
    let found = judged(&line).expect("the unexpanded groups stay unknown");
    assert!(found.unproven.is_some(), "{found:?}");
    assert!(judged(&format!("rm -rf ./{}", "{a..a}".repeat(8))).is_none());
    // Each expression is one level of expansion: a long word stays cheap.
    for groups in [100, 20_000] {
        let word = "{a,b}".repeat(groups);
        assert!(judged(&format!("echo {word}")).is_none());
        let found = judged(&format!("rm -rf ./{word}")).expect("the long word stays unknown");
        assert!(found.unproven.is_some(), "{groups}: {found:?}");
    }
}

#[test]
fn a_program_word_past_the_value_bound_is_refused() {
    for count in [100, 200] {
        let list = vec!["env"; count].join(",");
        for line in [
            format!("{{{list},rm}} -rf /usr"),
            format!("env {{{list},rm}} -rf /usr"),
            format!("for P in {{{list},rm}}; do $P -rf /usr; done"),
        ] {
            let found = judged(&line);
            assert!(
                found.is_some_and(|f| f.unfinished),
                "{count} {}",
                &line[..20]
            );
        }
        assert!(judged(&format!("echo {{{list}}}")).is_none());
    }
}

#[test]
fn input_lines_past_the_value_bound_stay_unknown() {
    for count in [1, 10, 100, 200] {
        for consumer in ["xargs rm -rf", "parallel rm -rf"] {
            let line = format!("{} | {consumer}", printed_lines(count, "/usr"));
            assert!(judged(&line).is_some(), "{consumer} after {count}");
        }
    }
    assert!(judged(&format!("{} | xargs echo", printed_lines(200, "/usr"))).is_none());
}

#[test]
fn a_script_on_input_past_the_value_bound_is_refused() {
    for count in [1, 10, 100, 200] {
        let line = format!("{} | sh", printed_lines(count, &quoted(DELETION)));
        assert!(judged(&line).is_some(), "after {count}");
    }
}

#[test]
fn argument_combinations_past_the_variant_bound_are_refused_when_one_may_delete() {
    let flags = ["-a", "-b", "-c", "-d", "-e", "-f", "-g", "-h", "-i", "-rf"];
    let targets = [
        "./a", "/usr", "./b", "./c", "./d", "./e", "./f", "./g", "./h", "./i",
    ];
    let line = format!(
        "{}; {}; rm $F $D",
        branches("F", &flags),
        branches("D", &targets),
    );
    assert!(judged(&line).is_some(), "flag and target");
    let names = ["a", "b", "c", "d", "e", "f", "g", "h", "i", "rm"];
    let line = format!(
        "{}; {}; $C -rf $D",
        branches("C", &names),
        branches("D", &targets),
    );
    assert!(judged(&line).is_some(), "name and target");
    let line = format!(
        "{}; {}; printf %s $F $D",
        branches("F", &flags),
        branches("D", &targets),
    );
    assert!(judged(&line).is_none(), "a command that deletes nothing");
}

#[test]
fn a_value_that_arrives_after_the_loop_passes_stays_unknown() {
    for names in 1..=20 {
        let (seed, steps) = value_chain(names, "/usr");
        for line in [
            format!("{seed}; for i in 1 2; do rm -rf $V0; {steps}; done"),
            format!("{seed}; while probe; do rm -rf $V0; {steps}; done"),
            format!("{seed}; for i in 1 2; do {steps}; done; rm -rf $V0"),
            format!("{seed}; while probe; do {steps}; done; rm -rf $V0"),
            format!("{seed}; trap {} DEBUG; true; rm -rf $V0", quoted(&steps)),
        ] {
            assert!(judged(&line).is_some(), "chain of {names}: {line}");
        }
    }
}

#[test]
fn a_directory_that_arrives_after_the_loop_passes_stays_unknown() {
    for names in 1..=20 {
        let (seed, steps) = value_chain(names, "/");
        let line = format!("{seed}; for i in 1 2; do cd $V0; {steps}; rm -rf usr; done");
        assert!(judged(&line).is_some(), "chain of {names}");
    }
}

#[test]
fn deep_parameter_defaults_are_refused_without_exhausting_the_stack() {
    for levels in [100, 1000, 5000, 20000] {
        let (open, close) = ("${x:-".repeat(levels), "}".repeat(levels));
        for line in [
            format!("echo {open}$({DELETION}){close}"),
            format!("x={open}y{close}"),
            format!("echo {open}y{close}"),
        ] {
            let found = judged(&line);
            assert!(
                found.is_some_and(|f| f.unfinished),
                "{levels} {}",
                &line[..4]
            );
        }
    }
    assert!(judged("echo ${a:-${b:-${c:-./d}}}").is_none());
}

#[test]
fn deep_structure_is_refused_without_exhausting_the_stack() {
    for levels in [100, 1000, 5000] {
        for (open, close) in [
            ("( ", " )"),
            ("{ ", "; }"),
            ("if probe; then ", "; fi"),
            ("while probe; do ", "; done"),
            ("for i in 1; do ", "; done"),
            ("case x in x) ", ";; esac"),
            ("f() ", ""),
        ] {
            let line = format!("{}{DELETION}{}", open.repeat(levels), close.repeat(levels));
            assert!(judged(&line).is_some(), "{open} {levels} times");
            let line = format!("{}true{}", open.repeat(levels), close.repeat(levels));
            assert!(judged(&line).is_some(), "{open} {levels} times, harmless");
        }
    }
}

#[test]
fn ordinary_nested_commands_stay_allowed() {
    for line in [
        "sh -c 'make test'",
        "sh -c 'cd crates && cargo test -p codeflow-core'",
        "bash -lc 'cargo fmt --all -- --check && cargo clippy --workspace'",
        "bash -c 'for f in a b c; do echo \"$f\"; done'",
        "bash -lc \"sh -c 'echo one; echo two'\"",
        "sh -c 'bash -c \"sh -c \\\"echo deep\\\"\"'",
        "timeout 60 sudo env A=1 nice -n 10 make -C sub all",
        "find . -name '*.tmp' -exec echo {} \\;",
        "find . -type f -exec sh -c 'echo \"$1\"' _ {} \\;",
        "find . -name '*.rs' | xargs grep -n main",
        "printf '%s\\n' a b c | xargs echo",
        "printf '%s\\n' a b | xargs -I{} echo {}",
        "make -j2 test && make install DESTDIR=./out",
        "rm -rf ./build && make",
        "sh -c 'rm -rf ./build'",
        "bash -lc 'rm -rf ./target/tmp && cargo build'",
        "x=$(git rev-parse HEAD); echo \"$x\"",
        "echo \"$(date +%s) $(git rev-parse --short HEAD)\"",
        "f() { echo hi; }; g() { f; }; g",
        "i=0; while [ $i -lt 3 ]; do i=$((i+1)); done; echo $i",
        "n=0; for f in a b c; do n=$((n+1)); rm -rf ./out/x; done",
        "n=0; until make test; do n=$((n+1)); [ $n -gt 3 ] && break; done",
        "s=a; while probe; do s=\"${s}b\"; done; rm -rf ./build",
        "echo {1..200}",
        "for i in {1..200}; do echo $i; done",
    ] {
        let found = judged(line);
        assert!(found.is_none(), "{line}: {found:?}");
    }
}
