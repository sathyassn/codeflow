//! Unit tests of the option-aware non-executing allowlist (TSK-397 Codex
//! review round 14, C14-1; security review round 9, SR9-1). Each line is
//! read as the directory check reads it: a kept run-time word is `derived`,
//! and an exempt command keeps no operands.

use crate::security::outward::read_line;
use crate::security::shell_lex::lex;

fn kept(line: &str) -> bool {
    read_line(&lex(line)).derived.is_some()
}

fn exempt(line: &str) -> bool {
    read_line(&lex(line)).operands.is_empty()
}

/// C14-1, SR9-1: an option that runs a program, writes a named file or
/// evaluates a word keeps the words, at the start of a command and behind
/// a launcher, in every spelling getopt accepts.
#[test]
fn an_executing_option_keeps_the_words() {
    for line in [
        r#"rg --pre "$P" x "$PWD""#,
        r#"rg --pre="$P" x "$PWD""#,
        r#"rg --pre=python3 . "$PWD/src/..""#,
        r#"rg --hostname-bin "$P" x "$PWD""#,
        r#"rg -z x "$PWD""#,
        r#"rg --search-zip x "$PWD""#,
        r#"env rg --pre "$P" x "$PWD""#,
        r#"command rg --pre "$P" x "$PWD""#,
        r#"nice -n 1 rg --pre "$P" x "$PWD""#,
        r#"ag --pager "$P" x "$PWD""#,
        r#"ag -z x "$PWD""#,
        r#"sort --compress-program="$C" -S 1k "$PWD""#,
        r#"env gsort --compress-program="$C" -S 1k "$PWD""#,
        r#"sort -o "$PWD/out" in"#,
        r#"sort --output="$PWD/out" in"#,
        r#"sort -T "$PWD" in"#,
        r#"file -z "$PWD/a.gz""#,
        r#"file -Z "$PWD/a.gz""#,
        r#"file -C -m "$PWD/magic""#,
        r#"command printf -v 'a[$(python3 r.py "$PWD/src/..")]' %s value"#,
        r#"command printf %n 'a[$(python3 r.py "$PWD/src/..")]'"#,
        r#"printf -v x %s "$PWD""#,
        r#"printf '%n' "$PWD""#,
        r#"printf '%5n' "$PWD""#,
        r#"printf '\045n' "$PWD""#,
        r#"printf "$F" "$PWD""#,
        r#"command test -v 'a[$(python3 r.py "$PWD/src/..")]'"#,
        r#"test -v "$N""#,
        r#"[ -v "$N" ]"#,
        r#"[[ -v "$N" ]]"#,
        r#"test "$N" -eq 1"#,
        r#"[ "$N" -gt 0 ]"#,
        r"[[ $N -eq 1 ]]",
        r#"uniq in "$PWD/out""#,
        r#"uniq in out "$PWD""#,
    ] {
        assert!(kept(line), "{line}: {:?}", read_line(&lex(line)));
    }
}

/// C14-1: getopt accepts any unambiguous prefix of a long option, so only
/// a listed name written in full is allowed.
#[test]
fn an_abbreviated_long_option_keeps_the_words() {
    for line in [
        r#"sort --comp="$C" "$PWD""#,
        r#"sort --compress "$C" "$PWD""#,
        r#"rg --pr "$P" x "$PWD""#,
        r#"rg --host "$P" x "$PWD""#,
        r#"ag --pag "$P" x "$PWD""#,
        r#"sort --out "$PWD/x" in"#,
        r#"grep --reg x "$PWD""#,
    ] {
        assert!(kept(line), "{line}");
    }
}

/// C14-1: an option whose name is filled in at run time, a word that may
/// expand to an option of a program with executing options, and arguments
/// `xargs` appends all keep the words.
#[test]
fn a_run_time_option_keeps_the_words() {
    for line in [
        r#"rg "$OPT" x "$PWD""#,
        r#"rg --"$O" x "$PWD""#,
        r#"rg -"$O" x "$PWD""#,
        r#"rg x * "$PWD""#,
        r#"rg x "$PWD""#,
        r#"sort "$F""#,
        r#"file -b "$PWD/a""#,
        r#"ls --colo* "$PWD""#,
        r#"cat -"$O" "$PWD""#,
        r#"printf x | xargs rg x "$PWD""#,
        r#"xargs sort "$PWD""#,
        r#"xargs printf '%s' "$PWD""#,
        r#"xargs test -f "$PWD""#,
    ] {
        assert!(kept(line), "{line}");
    }
}

/// The ordinary forms of the allowlisted programs keep their words to
/// themselves.
#[test]
fn ordinary_forms_stay_exempt() {
    for line in [
        r"rg -n --hidden -g '*.rs' pattern ~/src",
        r#"rg --type=rust -e "$P" src"#,
        r#"grep -rn "$PAT" "$PWD/src""#,
        r#"grep --include='*.rs' -e x "$PWD""#,
        r#"ls -la "$PWD""#,
        r"ls --color=auto ~",
        r#"cat "$PWD/notes.txt""#,
        r#"head -n 5 "$PWD/a""#,
        r#"head -5 "$PWD/a""#,
        r#"tail -n +3 "$PWD/a""#,
        r#"wc -l "$PWD/a""#,
        r"sort -u -k 2 src/list",
        r"sort -t, -k2n src/list",
        r"uniq -c in",
        r#"cut -d: -f1 "$PWD/a""#,
        r#"echo "$PWD""#,
        r#"echo -n "$HOME""#,
        r#"printf '%s\n' "$PWD""#,
        r#"printf -- '%d items\n' "$N""#,
        r#"stat -f %z "$PWD/a""#,
        r#"file -b ./"$F""#,
        r#"rg -n pattern ./"$D""#,
        r#"sort -u src/"$F""#,
        r#"du -sh "$PWD""#,
        r#"df -h "$PWD""#,
        r#"test -f "$PWD/a""#,
        r#"test "$A" = "$B""#,
        r#"test ! -d "$PWD/a""#,
        r#"[ -f "$PWD/a" ]"#,
        r#"[ "$A" != "$B" ]"#,
        r"[ 1 -eq 1 ]",
        r#"[[ -f "$PWD/a" ]]"#,
        r#"[[ "$A" == b* ]]"#,
        r#"basename "$PWD""#,
        r#"dirname "$PWD""#,
        r#"realpath "$PWD/..""#,
        r#"readlink -f "$PWD""#,
        r#"true "$PWD""#,
        r#"/usr/bin/grep x "$PWD""#,
        r#"xargs -n 1 cat "$PWD""#,
        r#"xargs head -n 1 "$PWD""#,
    ] {
        assert!(exempt(line), "{line}: {:?}", read_line(&lex(line)));
    }
}

/// A path outside the system directories is a program the line may have
/// put there, so its name does not exempt it.
#[test]
fn only_system_paths_take_the_name() {
    for line in [
        r#"./ls "$P" r.py "$PWD""#,
        r#"/tmp/x/cat "$P" r.py "$PWD""#,
        r#"/opt/homebrew/bin/grep "$P" r.py "$PWD""#,
        r#"bin/echo "$P" r.py "$PWD""#,
    ] {
        assert!(kept(line), "{line}");
    }
}

/// C14-1: a command substitution in a possible subscript is a hidden
/// command, quoted or not, whatever program reads it.
#[test]
fn a_subscript_command_is_a_run_time_value() {
    for line in [
        r"read 'a[$(python3 r.py ..)]' <<< x",
        r"unset 'a[$(python3 r.py ..)]'",
        r"declare -a 'a[`python3 r.py ..`]=1'",
        r"x='a[$(python3 r.py ..)]'; z",
        r"echo 'a[b[$(python3 r.py ..)]]'",
    ] {
        let reading = read_line(&lex(line));
        assert!(reading.moves, "{line}");
        assert!(reading.derived.is_some(), "{line}: {reading:?}");
    }
    for line in [
        "echo '[x] and `y`'",
        "awk '{a[$1]++}' f",
        "grep -E '^[a-z]+$' f",
    ] {
        assert!(read_line(&lex(line)).derived.is_none(), "{line}");
    }
}

/// A line that may change what a name runs (an alias, a builtin turned off
/// so a program of that name runs, a function, a sourced file, a new search
/// path) exempts no name (replay of the round 14 corpus:
/// `enable -n cd; cd "$p"`). The words only count in a program's place.
#[test]
fn a_shadowed_name_is_not_exempt() {
    for line in [
        r#"enable -n cd; cd "$p""#,
        r#"disable cd; cd "$p""#,
        "alias ls='python3 r.py'\nls \"$PWD/src/..\"",
        "eval 'alias ls=python3'\nls \"$PWD/src/..\"",
        r#"ls() { python3 r.py "$@"; }; ls "$PWD/src/..""#,
        r#"PATH=/tmp/x:$PATH; cat "$PWD/src/..""#,
        r#"export PATH="$D"; cat "$PWD/src/..""#,
        r#"hash -p /tmp/x/python3 cat; cat "$PWD/src/..""#,
        r#"read PATH; cat "$PWD/src/..""#,
        r#"path[1]=/tmp/x; cat "$PWD/src/..""#,
        r#"eval 'cat "$PWD/src/.."'; alias cat=python3"#,
        r#"source ./env.sh; cat "$PWD/src/..""#,
        r#". ./env.sh; cat "$PWD/src/..""#,
        r#"command alias cat=python3; cat "$PWD/src/..""#,
        r#""$A" cat=python3; cat "$PWD/src/..""#,
        r#"eval "$X"; cat "$PWD/src/..""#,
    ] {
        assert!(kept(line), "{line}: {:?}", read_line(&lex(line)));
    }
}

/// The entries the replay justified: the directory builtins and `git log`
/// with its reading options.
#[test]
fn replay_justified_entries() {
    for line in [
        r#"cd ~ && cd "$p""#,
        r#"cd / && pushd "$p""#,
        r"popd -q",
        r#"cd -P "$D""#,
        r#"git log --since="$D" --oneline"#,
        r#"git --no-pager log -n 5 --format='%h %s' -- src/"$F""#,
    ] {
        assert!(exempt(line), "{line}: {:?}", read_line(&lex(line)));
    }
    for line in [
        r#"git log --output="$PWD/x""#,
        r#"git log --ext-diff -p "$PWD""#,
        r#"git log --out="$PWD/x""#,
        r#"git -c core.pager=python3 log "$PWD""#,
        r#"git log "$REV""#,
        r#"git commit -m "$M""#,
        r#"chdir "$p""#,
    ] {
        assert!(kept(line), "{line}");
    }
}

/// A shadowing word as data, outside a program's place, changes nothing.
#[test]
fn a_shadowing_word_as_data_is_not_shadowing() {
    for line in [
        "grep alias ~/.bashrc",
        "rg --no-config alias ~/notes",
        "echo function hash enable",
        "grep -c PATH ~/notes",
        r#"[ -f "$F" ] && cat "$F""#,
    ] {
        assert!(exempt(line), "{line}: {:?}", read_line(&lex(line)));
    }
}
