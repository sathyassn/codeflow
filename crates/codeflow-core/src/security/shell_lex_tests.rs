//! Unit tests of the TSK-397 shell lexer: each lexical rule is asserted on
//! the token or flag it sets, so reverting the rule fails its test even
//! where another guard check would still refuse the whole line (security
//! review SR6-4).

use super::{lex, Token};
use crate::security::outward::read_line;

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
