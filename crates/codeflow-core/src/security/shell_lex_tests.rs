//! Unit tests of the TSK-397 shell lexer: each lexical rule is asserted on
//! the token or flag it sets, so reverting the rule fails its test even
//! where another guard check would still refuse the whole line (security
//! review SR6-4).

use super::{lex, Token, WRAPPERS};
use crate::security::outward::{read_line, LEXER_LAUNCHERS, MODELED_LAUNCHERS};

fn word(line: &str, command: usize, at: usize) -> Token {
    lex(line).commands[command][at].clone()
}

/// SR5-1: an unquoted tilde is seen wherever the shell expands it.
#[test]
fn a_tilde_is_seen_after_equals_and_colons() {
    for (line, expected) in [
        ("x=~+", "~+"),
        ("x=a:~+", "~+"),
        ("d=~+/src/..", "~+"),
        ("~-", "~-"),
        ("~/notes.txt", "~"),
    ] {
        assert_eq!(word(line, 0, 0).tilde.as_deref(), Some(expected), "{line}");
    }
    assert_eq!(word("export x=~+", 0, 1).tilde.as_deref(), Some("~+"));
    assert_eq!(word("a=(~+)", 0, 0).tilde.as_deref(), Some("~+"));
    assert_eq!(word("'~+'", 0, 0).tilde, None);
    assert_eq!(word("--dir=~/x", 0, 0).tilde.as_deref(), Some("~"));
    assert_eq!(word("{a,~/x}", 0, 0).tilde.as_deref(), Some("~"));
    // Mid-word, a tilde is text (`git rebase HEAD~3`).
    assert_eq!(word("HEAD~3", 0, 0).tilde, None);
    assert_eq!(word("a~/x", 0, 0).tilde, None);
}

/// SR5-3 to SR5-5: constructs the lexer does not model are opaque.
#[test]
fn unmodeled_constructs_are_opaque() {
    for line in [
        "x <<$'E'\nE",
        "x <<$(x)\n$(x)",
        "x <<`x`\n`x`",
        "x \"$(( '1' ))\"",
        "x \"$((echo 1) )\"",
        "x $((echo 1) )",
        "x \"$(( \\\\ ))\"",
        "x \"${x:-'}'}\"",
        "x 'unclosed",
        "x \"$(unclosed",
        "x <<E\nno end",
    ] {
        assert!(lex(line).opaque, "{line}");
    }
    for line in ["x <<E\nbody\nE", "x \"$(( 1 + 2 ))\"", "x \"${x:-y}\""] {
        assert!(!lex(line).opaque, "{line}");
    }
}

/// SR5-1, SR5-2: once the directory is unknown, every value filled in at
/// run time is derived, through any assignment form.
#[test]
fn every_run_time_operand_is_derived() {
    for line in [
        "x=~+; z; python3 r.py \"$x\"",
        "x=notes.txt; z; python3 r.py \"$x\"",
        "(x=notes.txt); z; python3 r.py \"$x\"",
        "x+=a; z; python3 r.py \"$x\"",
        "for x; do z; python3 r.py \"$x\"; done",
        "printf -v x %s a; z; python3 r.py \"$x\"",
        "z; python3 r.py d=~+/src/..",
        "z; python3 r.py \"$( pwd)\"",
        "z; python3 r.py ~/settings.json",
        "z; python3 r.py \"$HOME/settings.json\"",
    ] {
        let reading = read_line(&lex(line));
        assert!(reading.moves, "{line}");
        assert!(reading.derived.is_some(), "{line}: {reading:?}");
    }
    let plain = read_line(&lex("z; python3 r.py notes.txt"));
    assert!(plain.derived.is_none());
}

/// C10-1: launcher traversal never drops the operands of a command that
/// may still run an interpreter.
#[test]
fn operands_survive_wherever_traversal_stops() {
    for line in [
        "env \"python3\" r.py \"$PWD\"",
        "command \"python3\" r.py \"$PWD\"",
        "nice -n 5 \"python3\" r.py \"$PWD\"",
        "timeout 5 \"python3\" r.py \"$PWD\"",
        "builtin command python3 r.py \"$PWD\"",
        "env -u X \"python3\" r.py \"$PWD\"",
        "\"python3\" r.py \"$PWD\"",
        "nice --unknown python3 r.py \"$PWD\"",
    ] {
        let reading = read_line(&lex(line));
        assert!(
            reading.operands.iter().any(|w| w.text == "$PWD"),
            "{line}: {reading:?}"
        );
        assert!(reading.derived.is_some(), "{line}");
    }
}

/// SR6-1: a `$` before anything but a blank, the end or a closing quote
/// may expand (zsh `$=x`, `$^x`).
#[test]
fn an_unknown_dollar_form_is_complex() {
    for (line, at) in [("x $=y", 1), ("x $^y", 1), ("x \"$=y\"", 1), ("x $~y", 1)] {
        assert!(word(line, 0, at).complex, "{line}");
    }
    for line in ["x $", "x \"$\"", "x a$ b"] {
        let lexed = lex(line);
        assert!(lexed.commands[0].iter().all(|w| !w.complex), "{line}");
    }
}

/// SR6-2: the words after a header's `do` are in command position.
#[test]
fn a_header_returns_to_command_position() {
    for line in [
        "for x do cd ..; done",
        "for x in a b; do cd ..; done",
        "select x in a; do z; done",
    ] {
        assert!(read_line(&lex(line)).moves, "{line}");
    }
    assert!(!read_line(&lex("for x in a b; do python3 r.py \"$x\"; done")).moves);
}

/// Every launcher the guard models, in either reader, and every GNU-prefixed
/// name it maps to one, is a wrapper: a launcher added later cannot be
/// missed (TSK-397 Codex review round 11).
#[test]
fn every_modeled_launcher_is_a_wrapper() {
    for name in LEXER_LAUNCHERS.iter().chain(MODELED_LAUNCHERS) {
        assert!(WRAPPERS.contains(name), "{name}");
        let prefixed = format!("g{name}");
        if crate::hooks::git_guard::launcher_name(&prefixed) == *name {
            assert!(WRAPPERS.contains(&prefixed.as_str()), "{prefixed}");
        }
    }
    for name in [
        "builtin",
        "sudo",
        "doas",
        "eval",
        "noglob",
        "nocorrect",
        "-",
    ]
    .iter()
    .chain(crate::hooks::git_guard::PREFIX_LAUNCHERS)
    .chain(crate::hooks::git_guard::COMMAND_CARRIERS)
    {
        assert!(WRAPPERS.contains(name), "{name}");
    }
}

/// C11-1, SR7-1: under each wrapper kind, an expanded program name or a
/// split command string keeps every remaining word as an operand, at the
/// top level and nested.
#[test]
fn wrappers_keep_expanded_names_and_split_strings() {
    for line in [
        "builtin command ${P:-python3} r.py \"$PWD\"",
        "builtin command \"$P\" r.py \"$PWD\"",
        "env -S \"${P:-python3}\" r.py \"$PWD\"",
        "env -S \"python3 r.py $PWD\"",
        "env env -S \"${P:-python3}\" r.py \"$PWD\"",
        "command env -S \"${P:-python3}\" r.py \"$PWD\"",
        "/usr/bin/env -S \"python3 r.py $PWD\"",
        "sudo \"$P\" r.py \"$PWD\"",
        "eval \"$P\" r.py \"$PWD\"",
        "genv -S \"python3 r.py $PWD\"",
    ] {
        let reading = read_line(&lex(line));
        assert!(reading.moves, "{line}");
        assert!(reading.derived.is_some(), "{line}: {reading:?}");
    }
    // An ordinary command keeps its words to itself.
    for line in ["echo \"$HOME\"", "ls ~", "cargo test -p \"$CRATE\""] {
        let reading = read_line(&lex(line));
        assert!(reading.operands.is_empty(), "{line}: {reading:?}");
    }
}

/// C12-1, C12-2: `arch`, `script` and `find -exec` wrappers, and the `time`
/// keyword with its options, keep the operands (TSK-397 Codex review round
/// 12).
#[test]
fn wrappers_and_the_time_keyword_keep_operands() {
    for line in [
        r#"arch -arm64 python3 r.py "$PWD""#,
        r#"arch -arm64 "${P:-python3}" r.py "$PWD""#,
        r#"/usr/bin/arch -arm64 python3 r.py "$PWD""#,
        r#"script -q t python3 r.py "$PWD""#,
        r#"script -q t "${P:-python3}" r.py "$PWD""#,
        r#"find . -prune -exec python3 r.py "$PWD" \;"#,
        r#"find . -prune -exec "${P:-python3}" r.py "$PWD" \;"#,
        r#"chrt 1 "$P" r.py "$PWD""#,
        r#"taskset 1 "$P" r.py "$PWD""#,
        r#"time -p python3 r.py "$PWD""#,
        r#"time -p "${P:-python3}" r.py "$PWD""#,
        r#"time -p -- python3 r.py "$PWD""#,
        r#"time -p env python3 r.py "$PWD""#,
        r#"time -p nice -n 5 env "$P" r.py "$PWD""#,
        r#"time python3 r.py "$PWD""#,
    ] {
        let reading = read_line(&lex(line));
        assert!(reading.derived.is_some(), "{line}: {reading:?}");
    }
    // `find` without an executing action runs no program.
    for line in [r#"find "$D" -delete"#, r#"find $D -name "$N" -print"#] {
        let reading = read_line(&lex(line));
        assert!(reading.operands.is_empty(), "{line}: {reading:?}");
    }
    let timed = read_line(&lex("time -p env python3 r.py notes.txt"));
    assert!(
        timed.operands.iter().any(|w| w.text == "notes.txt"),
        "{timed:?}"
    );
}
