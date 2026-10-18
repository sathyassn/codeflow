//! The differential for layer 1 of the composed-text check (TSK-302,
//! review round 11).
//!
//! Layer 1 (`security::composed`) claims the text a line composes. The
//! claim must be exact: for every line below, built only from harmless
//! values whose output is the marker `173302` or `notes.txt`, the host's
//! real shells run the line and layer 1's value for its last `printf`,
//! `echo` or `print` must equal what the shell printed, byte for byte, or
//! layer 1 must report the line unresolved. A value with a piece from a
//! variable the line never assigns matches when the shell's text fits it.
//!
//! Each line also states what layer 1 must do: resolve it in bash and zsh
//! (`R`), only in bash (`Rb`) or zsh (`Rz`), or leave it unresolved in both
//! (`U`), so a construct the module claims to model cannot drift into
//! being refused, and one it refuses cannot drift into a wrong value.
//! Every line prints its marker in at least one shell, which shows the
//! construct composes it there.
//!
//! The required shells: `bash` and `zsh` on `PATH`, `/bin/sh` (`sh` on
//! Windows) and `dash` read as POSIX `sh`. Unix also checks `/bin/bash`
//! when it is another version. A missing shell fails the prerequisite check.
//! When `CODEFLOW_DIFFERENTIAL_SHELL_DIR` is set, all four shells must come
//! from that directory, with no fallback. Only this test's shell children
//! receive that directory on `PATH`, so nested shell commands also use it.
//! Each runs in an empty directory with a fixed `HOME`,
//! `LC_ALL=C`, no startup files, and `CF_PART=1733` as the one variable the
//! lines read from outside.

use codeflow_core::security::composed::{final_output, Composed, Dialect};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

/// (construct family, expectation, line).
const CASES: &[(&str, &str, &str)] = &[
    // Words and quoting.
    ("quoting", "R", r#"printf '%s\n' "17"'33'02"#),
    ("quoting", "R", r"printf '%s\n' 17\33\02"),
    ("quoting", "R", r"printf '%s\n' ''173302"),
    ("quoting", "R", r"printf '%s\n' $'\x31\x37'3302"),
    ("quoting", "R", r"printf '%s\n' $'\061'73302"),
    ("quoting", "R", r"printf '%s\n' $'\61'73302"),
    ("quoting", "R", r"printf '%s\n' notes$'\x2e'txt"),
    ("quoting", "R", "printf '%s\\n' 173302 # a comment"),
    ("quoting", "R", "a=1733\nprintf '%s02\\n' \"$a\""),
    ("quoting", "R", "printf '%s\\n' 1733\\\n02"),
    // printf.
    ("printf", "R", r"printf '%s\n' 173302"),
    ("printf", "R", r"printf '%s%s%s\n' 17 33 02"),
    ("printf", "R", r"printf '%s' 1733 02"),
    ("printf", "R", r"printf '%s%s%s\n' 1733 02"),
    ("printf", "R", r"printf '%%s%s\n' 173302"),
    ("printf", "R", r"printf '\x31\x37\x33\x33\x30\x32\n'"),
    ("printf", "R", r"printf '\061\067\063\063\060\062\n'"),
    ("printf", "R", r"printf '\61\67\63\63\60\62\n'"),
    ("printf", "R", r"printf '%b\n' '\x31\x37\x33\x33\x30\x32'"),
    ("printf", "R", r"printf '%b%s\n' '\0061' 73302"),
    ("printf", "Rb", r"printf '%b%s\n' '\61' 73302"),
    ("printf", "R", r"printf '%q%s\n' 1733 02"),
    ("printf", "R", r"printf '%q\n' notes.txt"),
    (
        "printf",
        "R",
        r#"printf -v v '%s%s' 1733 02; printf '%s\n' "$v""#,
    ),
    ("printf", "R", r"printf -- '%s\n' 173302"),
    ("printf", "R", r"builtin printf '%s\n' 173302"),
    ("printf", "R", r"command printf '%s\n' 173302"),
    ("printf", "R", r"/usr/bin/printf '%s%s' 1733 02"),
    ("printf", "U", r"printf '%d%s\n' 1733 02"),
    ("printf", "U", r"printf '%5s\n' 173302"),
    ("printf", "U", r"printf '%q\n' 'notes txt' 173302"),
    // echo.
    ("echo", "R", "echo 173302"),
    ("echo", "R", "echo 1733 02 173302"),
    ("echo", "R", "echo -n 173302"),
    ("echo", "R", r"echo -- 173302"),
    ("echo", "R", r"echo - 173302"),
    ("echo", "R", r"echo -e '\x31\x37\x33\x33\x30\x32'"),
    ("echo", "R", r"echo -e '\0061\0067\0063\0063\0060\0062'"),
    ("echo", "R", r"echo -E '\x31' 173302"),
    ("echo", "R", r"echo '\x31\x37\x33\x33\x30\x32' 173302"),
    ("echo", "R", r"command echo 173302"),
    ("echo", "R", r"/bin/echo 173302"),
    ("echo", "R", r"env echo 1733 02 173302"),
    // Option clusters (family 3).
    (
        "echo cluster",
        "R",
        r"echo -ne '\x31\x37\x33\x33\x30\x32\n'",
    ),
    (
        "echo cluster",
        "R",
        r"echo -en '\x31\x37\x33\x33\x30\x32\n'",
    ),
    (
        "echo cluster",
        "R",
        r"echo -n -e '\x31\x37\x33\x33\x30\x32\n'",
    ),
    ("echo cluster", "R", r"echo -nE '173302'"),
    ("echo cluster", "U", r"echo -eE '\x31\x37\x33\x33\x30\x32'"),
    ("echo cluster", "U", r"echo -Ee '\x31\x37\x33\x33\x30\x32'"),
    (
        "echo cluster",
        "U",
        r"echo -e -E '\x31\x37\x33\x33\x30\x32' 173302",
    ),
    // zsh print.
    ("print", "Rz", "print 173302"),
    ("print", "Rz", r"print '\x31\x37\x33\x33\x30\x32'"),
    ("print", "Rz", r"print '\61\67\63\63\60\62'"),
    ("print", "Rz", r"print -r '\x31' 173302"),
    ("print", "Rz", "print -n 173302"),
    ("print", "Rz", "print -l 1733 02 173302"),
    ("print", "Rz", "print -rn -- 173302"),
    ("print", "U", "print -P 173302"),
    // Assignments and expansion.
    ("assignment", "R", r#"a=1733; printf '%s%s\n' "$a" 02"#),
    ("assignment", "R", r#"a=notes; printf '%s\n' "$a".txt"#),
    ("assignment", "R", r#"a=notes; printf '%s\n' "${a}.txt""#),
    ("assignment", "R", r"a=notes.txt; printf '%s\n' $a"),
    (
        "assignment",
        "R",
        r#"a=17; a+=33; a+=02; printf '%s\n' "$a""#,
    ),
    (
        "assignment",
        "R",
        r#"a=notes b=.txt; printf '%s%s\n' "$a" "$b""#,
    ),
    ("assignment", "R", r#"a=; printf '%s|%s\n' "$a" 173302"#),
    ("assignment", "R", r"a=; printf '%s\n' $a 173302"),
    ("assignment", "Rz", r"a='1733 02'; printf '%s%s\n' $a"),
    ("assignment", "Rz", r"a='17*'; printf '%s173302\n' $a"),
    ("assignment", "R", r"a=x printf '%s\n' 173302"),
    ("assignment", "R", r#"a=173302 printf '%s\n' "$a" 173302"#),
    ("assignment", "R", r#"printf '%s\n' "$CF_PART"02"#),
    ("assignment", "R", r#"printf '%s\n' "$1"173302"#),
    ("assignment", "U", r#"printf '%s\n' "$_"173302"#),
    // Family 1: a value built from an expansion.
    (
        "assignment from expansion",
        "R",
        r#"a=1733; b=${a}02; printf '%s\n' "$b""#,
    ),
    (
        "assignment from expansion",
        "R",
        r#"a=1733; b="$a"02; printf '%s\n' $b"#,
    ),
    (
        "assignment from expansion",
        "R",
        r#"a=1733; b="${a}02"; c=$b; printf '%s\n' "$c""#,
    ),
    (
        "assignment from expansion",
        "R",
        r#"a=notes b=$a.txt; printf '%s\n' "$b""#,
    ),
    (
        "assignment from expansion",
        "R",
        r#"a=$CF_PART; b=${a}02; printf '%s\n' "$b""#,
    ),
    (
        "assignment from expansion",
        "R",
        r#"a=$(printf 1733); printf '%s02\n' "$a""#,
    ),
    (
        "assignment from expansion",
        "R",
        r#"a=`printf 1733`; printf '%s02\n' "$a""#,
    ),
    (
        "assignment from expansion",
        "R",
        r#"a=1733$((0)); printf '%s\n' "$a"2"#,
    ),
    (
        "assignment from expansion",
        "U",
        r#"a=${CF_PART:-x}02; printf '%s\n' "$a""#,
    ),
    (
        "assignment from expansion",
        "U",
        r#"a=1733; b=${a%x}02; printf '%s\n' "$b""#,
    ),
    (
        "assignment from expansion",
        "U",
        r#"a=173302; b=${#a}; printf '%s173302\n' "$b""#,
    ),
    // Family 2: declaration builtins and element assignments.
    ("declaration", "R", r#"export a=173302; printf '%s\n' "$a""#),
    (
        "declaration",
        "R",
        r#"export a=1733 b=02; printf '%s%s\n' "$a" "$b""#,
    ),
    (
        "declaration",
        "R",
        r#"declare a=173302; printf '%s\n' "$a""#,
    ),
    (
        "declaration",
        "R",
        r#"typeset a=173302; printf '%s\n' "$a""#,
    ),
    (
        "declaration",
        "R",
        r#"readonly a=173302; printf '%s\n' "$a""#,
    ),
    (
        "declaration",
        "R",
        r#"declare a; a=173302; printf '%s\n' "$a""#,
    ),
    (
        "declaration",
        "R",
        r#"a=1733; export b="${a}02"; printf '%s\n' "$b""#,
    ),
    ("declaration", "Rz", r#"local a=173302; printf '%s\n' "$a""#),
    (
        "declaration",
        "U",
        r#"declare -x a=173302; printf '%s\n' "$a""#,
    ),
    (
        "declaration",
        "U",
        r#"typeset -r a=173302; printf '%s\n' "$a""#,
    ),
    (
        "declaration",
        "U",
        r#"declare -a arr=(173302); printf '%s\n' "${arr[0]}""#,
    ),
    (
        "element assignment",
        "U",
        r#"arr=(x y); arr[1]=173302; printf '%s\n' "${arr[1]}""#,
    ),
    (
        "element assignment",
        "U",
        r#"a[0]=173302; printf '%s\n' "$a""#,
    ),
    (
        "element assignment",
        "U",
        r#"arr=(x 173302); arr[0]=y; printf '%s\n' "${arr[*]}""#,
    ),
    // Arrays and IFS.
    (
        "array",
        "R",
        r#"arr=(1733 02); IFS=; printf '%s\n' "${arr[*]}""#,
    ),
    (
        "array",
        "R",
        r#"arr=(1733 02); printf '%s%s\n' "${arr[@]}""#,
    ),
    ("array", "R", r"arr=(1733 02); printf '%s%s\n' ${arr[@]}"),
    (
        "array",
        "R",
        r#"arr=(17 33 02); IFS=; printf '%s\n' "${arr[*]}""#,
    ),
    (
        "array",
        "R",
        r#"arr=(1733 02); IFS=x; printf '%s173302\n' "${arr[*]}""#,
    ),
    (
        "array",
        "R",
        r#"arr=(17 33); arr+=(02); IFS=; printf '%s\n' "${arr[*]}""#,
    ),
    (
        "array",
        "R",
        r#"arr=(x 173302); printf '%s173302\n' "${arr[1]}""#,
    ),
    (
        "array",
        "Rb",
        r#"arr=(173302 x); printf '%s\n' "${arr[0]}""#,
    ),
    ("array", "Rb", r#"arr=(1733 02); printf '%s\n' "$arr"02"#),
    (
        "array",
        "Rz",
        r#"arr=(1733 02); printf '%s\n' "${(j::)arr}""#,
    ),
    ("array", "Rb", r#"a=173302; printf '%s\n' "$a[1]""#),
    // Family 4: a printf format from a variable.
    (
        "format from a variable",
        "R",
        r#"f='%s%s\n'; printf "$f" 1733 02"#,
    ),
    (
        "format from a variable",
        "R",
        r#"f='\x31%s\n'; printf "$f" 73302"#,
    ),
    (
        "format from a variable",
        "R",
        r"f=%s%s; printf $f'\n' 1733 02",
    ),
    (
        "format from a variable",
        "Rz",
        r"f='%s %s'; printf $f 1733 02 173302",
    ),
    ("format from a variable", "U", r#"printf "$CF_PART%s\n" 02"#),
    // Family 5: unicode and other escapes the guard does not decode.
    ("escape", "U", r"printf '\u0031%s\n' 73302"),
    ("escape", "U", r"printf '%b%s\n' '\U00000031' 73302"),
    ("escape", "U", r"echo -e '\u0031' 173302"),
    ("escape", "U", r"printf '%s\n' $'\u0031'73302"),
    ("escape", "U", r"print '\u0031' 173302"),
    ("escape", "U", r"printf '\xe2%s\n' 173302"),
    ("escape", "U", r"printf '%s%b\n' 173302 '\c'"),
    // Family 6: indirect expansion.
    ("indirect", "Rb", r#"n=v; v=173302; printf '%s\n' "${!n}""#),
    ("indirect", "U", r#"n=CF_PART; printf '%s02\n' "${!n}""#),
    ("indirect", "U", r#"n=v; printf '%s173302\n' "${!n}""#),
    // Where an assignment holds.
    ("scope", "R", r#"a=173302; (a=x); printf '%s\n' "$a""#),
    ("scope", "R", r#"{ a=173302; }; printf '%s\n' "$a""#),
    (
        "scope",
        "R",
        r#"a=1733 && b=02 && printf '%s%s\n' "$a" "$b""#,
    ),
    ("scope", "R", r#"eval 'a=173302'; printf '%s\n' "$a""#),
    (
        "scope",
        "R",
        r#"eval a=1733 b=02; printf '%s%s\n' "$a" "$b""#,
    ),
    ("scope", "R", r#"eval "printf '%s\n' 173302""#),
    ("scope", "R", r#"set -e; a=173302; printf '%s\n' "$a""#),
    (
        "scope",
        "R",
        r#"set -o pipefail; a=173302; printf '%s\n' "$a""#,
    ),
    ("scope", "R", r#"cd /; a=173302; printf '%s\n' "$a""#),
    (
        "scope",
        "U",
        r#"a=173302; false && a=x; printf '%s\n' "$a""#,
    ),
    ("scope", "U", r#"a=173302; true || a=x; printf '%s\n' "$a""#),
    ("scope", "U", r#"a=173302; a=x | true; printf '%s\n' "$a""#),
    (
        "scope",
        "U",
        r#"if true; then a=173302; fi; printf '%s\n' "$a""#,
    ),
    (
        "scope",
        "U",
        r#"for a in 173302; do printf '%s\n' "$a"; done"#,
    ),
    ("scope", "R", r#"set -- 1733 02; printf '%s%s\n' "$1" "$2""#),
    (
        "scope",
        "R",
        r#"set -- x 1733 02; shift; printf '%s%s\n' "$1" "$2""#,
    ),
    ("scope", "R", r#"set -- 1733 02; IFS=; printf '%s\n' "$*""#),
    ("scope", "R", r#"set -- 1733 02; printf '%s%s\n' "$@""#),
    // Child shells: what they inherit and the arguments they are given.
    ("child shell", "R", r#"bash -c "printf '%s\n' 173302""#),
    (
        "child shell",
        "R",
        r#"export a=1733; bash -c 'printf "%s02\n" "$a"'"#,
    ),
    (
        "child shell",
        "U",
        r#"a=1733; bash -c 'printf "%s173302\n" "$a"'"#,
    ),
    (
        "child shell",
        "R",
        r#"a=1733 bash -c 'printf "%s02\n" "$a"'"#,
    ),
    (
        "child shell",
        "R",
        r#"bash -c 'printf "%s%s\n" "$1" "$2"' x 1733 02"#,
    ),
    (
        "child shell",
        "R",
        r#"zsh -fc 'a=1733; printf "%s02\n" "$a"'"#,
    ),
    (
        "child shell",
        "R",
        r#"sh -c 'a=1733; printf "%s02\n" "$a"'"#,
    ),
    (
        "child shell",
        "R",
        r#"env a=1733 bash -c 'printf "%s02\n" "$a"'"#,
    ),
    (
        "child shell",
        "R",
        r#"c='a=1733; printf "%s02\n" "$a"'; bash -c "$c""#,
    ),
    (
        "child shell",
        "R",
        "bash <<'EOF'\na=1733; printf '%s02\\n' \"$a\"\nEOF",
    ),
    (
        "child shell",
        "R",
        r#"bash <<< 'a=1733; printf "%s02\n" "$a"'"#,
    ),
    ("scope", "R", r"source /dev/null; printf '%s\n' 173302"),
    ("scope", "R", r"alias x=y; printf '%s\n' 173302"),
    ("scope", "U", r"printf() { echo 173302; }; printf '%s\n' x"),
    (
        "scope",
        "R",
        r#"eval "$CF_PART" 2>/dev/null; printf '%s\n' 173302"#,
    ),
    (
        "scope",
        "Rz",
        r"shopt -s xpg_echo 2>/dev/null; echo '\x31' 173302",
    ),
    // Escape edges both shells agree on, and ones they do not.
    ("escape edge", "R", r"printf '%s\n' $'\e'173302"),
    ("escape edge", "R", r"printf '\e%s\n' 173302"),
    ("escape edge", "R", r"printf '%b%s\n' '\e' 173302"),
    ("escape edge", "Rz", r"print '\e' 173302"),
    ("escape edge", "U", r"echo -e '\e' 173302"),
    ("escape edge", "R", r#"printf '%s\n' $'\'\"'173302"#),
    ("escape edge", "R", r"printf '%s\n' $'\x3'173302"),
    ("escape edge", "R", r"printf '\x3%s\n' 173302"),
    ("escape edge", "R", r"printf '%b%s\n' '\x3g' 173302"),
    ("escape edge", "R", r"echo -e '\x3' 173302"),
    ("escape edge", "R", r"printf '\06173302\n'"),
    ("escape edge", "U", r"printf '%b%s\n' '\0617' 173302"),
    ("escape edge", "U", r"printf '%s\n' $'\q'173302"),
    ("escape edge", "U", r"printf '%s\n' $'\cA'173302"),
    ("escape edge", "U", r"printf '\0%s\n' 173302"),
    // Values from outside the line.
    ("outside value", "R", r#"printf '%s\n' "${CF_PART}"02"#),
    ("outside value", "Rz", r"printf '%s\n' ${CF_PART}02"),
    ("outside value", "R", r#"printf '%s\n' "${CF_PART%33}"3302"#),
    ("outside value", "R", r#"echo "$CF_PART"02"#),
    ("outside value", "U", r#"printf '%s\n' "${a:-173302}""#),
    ("outside value", "R", r#"printf '%s\n' "$(printf 1733)"02"#),
    // Arrays at their edges.
    (
        "array edge",
        "R",
        r#"arr=("17" '33' "0"2); IFS=; printf '%s\n' "${arr[*]}""#,
    ),
    (
        "array edge",
        "R",
        r#"arr=(); printf '%s173302\n' "${arr[@]}""#,
    ),
    (
        "array edge",
        "R",
        r#"arr=('' 173302); printf '%s|\n' "${arr[@]}""#,
    ),
    (
        "array edge",
        "R",
        r"arr=('' 173302); printf '%s|' ${arr[@]}",
    ),
    (
        "array edge",
        "R",
        r#"arr=(x); arr=(1733 02); IFS=; printf '%s\n' "${arr[*]}""#,
    ),
    // Scope through groups, loops and tests.
    (
        "scope edge",
        "U",
        r#"true && { a=173302; }; printf '%s\n' "$a""#,
    ),
    (
        "scope edge",
        "U",
        r#"a=173302; { a=x; } | true; printf '%s\n' "$a""#,
    ),
    (
        "scope edge",
        "U",
        r#"case x in x) a=173302;; esac; printf '%s\n' "$a""#,
    ),
    (
        "scope edge",
        "R",
        r#"while false; do :; done; a=173302; printf '%s\n' "$a""#,
    ),
    (
        "scope edge",
        "U",
        r#"[[ -n x ]] && a=173302; printf '%s\n' "$a""#,
    ),
    (
        "scope edge",
        "R",
        r#"a=173302; [[ $a == x ]]; printf '%s\n' "$a""#,
    ),
    ("scope edge", "R", r#"((1)); a=173302; printf '%s\n' "$a""#),
    (
        "scope edge",
        "R",
        r#"read a <<< x; a=173302; printf '%s\n' "$a""#,
    ),
    (
        "scope edge",
        "U",
        r#"read a <<< 173302; printf '%s\n' "$a""#,
    ),
    (
        "scope edge",
        "R",
        r#"f() { :; }; a=173302; printf '%s\n' "$a""#,
    ),
    // zsh-only readings.
    ("zsh reading", "Rb", r#"a=x/173302; printf '%s\n' "$a:t""#),
    ("zsh reading", "Rb", r"printf '%s\n' =x 173302"),
    ("zsh reading", "R", r"printf '%s\n' a=b 173302"),
    ("zsh reading", "U", r"a=173302; printf '%s\n' ${=a}"),
    ("zsh reading", "U", r"a=173302; printf '%s\n' $=a"),
    ("zsh reading", "U", r"a=173302; printf '%s\n' ${~a}"),
    ("zsh reading", "U", r"a=173302; printf '%s173302\n' $#a"),
    ("zsh reading", "U", r"a=173302; printf '%s\n' ${^a}"),
    ("assignment", "R", r#"a=*; printf '%s\n' "$a"173302"#),
    (
        "declaration",
        "R",
        r#"a=173302; readonly a; b=$a; printf '%s\n' "$b""#,
    ),
    // Command substitution over modelled builtins, and program output as
    // outside text.
    ("substitution", "R", r#"printf '%s\n' "$(printf 1733)02""#),
    ("substitution", "R", r#"printf '%s\n' "$(echo 1733)"02"#),
    (
        "substitution",
        "R",
        r#"printf '%s\n' "$(printf '1733\n\n')"02"#,
    ),
    (
        "substitution",
        "R",
        r#"a=$(echo -n 17; printf 33); printf '%s02\n' "$a""#,
    ),
    (
        "substitution",
        "R",
        r#"printf '%s\n' "$(printf 17; true)"3302"#,
    ),
    (
        "substitution",
        "R",
        r#"printf '%s\n' "$(a=1733; printf "$a")"02"#,
    ),
    (
        "substitution",
        "R",
        r#"a=1733; printf '%s\n' "$(printf "$a")"02"#,
    ),
    (
        "substitution",
        "R",
        r#"printf '%s\n' "$(printf 1733 >/dev/null)"173302"#,
    ),
    ("substitution", "R", r"printf '%s\n' $(printf 1733)02"),
    ("substitution", "R", r"echo $(printf 1733)02"),
    (
        "substitution",
        "R",
        r#"printf '%s\n' "$(printf 1733 | cat)"02"#,
    ),
    (
        "substitution",
        "R",
        r#"printf '%s\n' "$(cat /dev/null)"173302"#,
    ),
    (
        "substitution",
        "R",
        r#"printf '%s\n' "$(bash -c 'printf 1733')"02"#,
    ),
    ("substitution", "R", r#"echo "Done $(date)" 173302"#),
    ("substitution", "R", r"echo $(date) 173302"),
    ("substitution", "Rz", r"printf '%s\n' $(date) 173302"),
    (
        "substitution",
        "R",
        r#"x=$(date); printf '%s173302\n' "$x""#,
    ),
    (
        "substitution",
        "R",
        r#"ROOT=$(cd / && pwd); printf '%s\n' "$ROOT"173302"#,
    ),
    (
        "substitution",
        "U",
        r#"printf '%s\n' "$(printf '%q' 'a b')"173302"#,
    ),
    (
        "substitution",
        "U",
        r#"printf '%s\n' "$(printf '\u0031')"73302"#,
    ),
    // Arithmetic.
    ("arithmetic", "R", r#"printf '%s\n' "$((1733))"02"#),
    ("arithmetic", "R", r#"printf '%s\n' "$((1700+33))"02"#),
    ("arithmetic", "R", r#"printf '%s\n' "$((2*(3+4)))"173302"#),
    ("arithmetic", "R", r#"printf '%s\n' "$((-7/2))"173302"#),
    ("arithmetic", "R", r#"a=1733; printf '%s\n' "$((a))"02"#),
    ("arithmetic", "R", r#"printf '%s\n' "$((CF_PART))"02"#),
    ("arithmetic", "U", r#"printf '%s\n' "$((010))"173302"#),
    ("arithmetic", "U", r#"printf '%s\n' "$((2**3))"173302"#),
    // What read takes, and assignments in front of a command.
    (
        "read",
        "R",
        r#"read a < /dev/null; printf '%s173302\n' "$a""#,
    ),
    (
        "read",
        "R",
        r"while read f; do :; done < /dev/null; printf '%s\n' 173302",
    ),
    (
        "read",
        "U",
        r#"printf x | read a; printf '%s173302\n' "$a""#,
    ),
    (
        "assignment",
        "R",
        r#"a=1733; a=x true; printf '%s02\n' "$a""#,
    ),
    (
        "assignment",
        "R",
        r#"IFS=: read a < /dev/null; arr=(1733 02); printf '%s173302\n' "${arr[*]}""#,
    ),
    // A program named by a path runs as itself: outside text unless it is
    // `printf` or `echo`, read in its external form.
    (
        "path program",
        "R",
        r#"printf '%s\n' "$(/usr/bin/true)"173302"#,
    ),
    (
        "path program",
        "R",
        r#"printf '%s\n' "$(/bin/cat /dev/null)"173302"#,
    ),
    (
        "path program",
        "R",
        r#"printf '%s\n' "$(/bin/cat <<< 1733)"02"#,
    ),
    (
        "path program",
        "R",
        r#"printf '%s\n' "$(/usr/bin/basename /x/1733)"02"#,
    ),
    (
        "path program",
        "R",
        r#"printf '%s\n' "$(/usr/bin/env cat /dev/null)"173302"#,
    ),
    (
        "path program",
        "R",
        r#"printf '%s\n' "$(/bin/echo 1733)"02"#,
    ),
    (
        "path program",
        "R",
        r#"printf '%s\n' "$(/usr/bin/printf 1733)"02"#,
    ),
    (
        "path program",
        "R",
        r#"printf '%s\n' "$(/usr/bin/printf '%s%s' 17 33)"02"#,
    ),
    ("path program", "R", r"/usr/bin/printf '%s%s\n' 1733 02"),
    ("path program", "U", r"/usr/bin/printf '%b\n' 173302"),
    (
        "path program",
        "U",
        r#"arr=(* x); printf '%s173302\n' "${arr[1]}""#,
    ),
    ("path program", "U", r#"echo "$(/bin/pwd)"/* 173302"#),
    ("path program", "U", r"echo .cla* 173302"),
    (
        "path program",
        "U",
        r#"printf '%s\n' "$(echo .cla*)"173302"#,
    ),
    ("path program", "R", r"echo 173302 $(/usr/bin/true) x"),
    ("path program", "R", r#"echo 173302 "$(/usr/bin/true)" x"#),
    ("path program", "R", r"echo $(/usr/bin/true) 173302"),
    ("path program", "U", r"echo ok .cla* 173302"),
    // An expansion that is empty joins the spelled pieces around it.
    ("empty span", "R", r"echo notes$(/bin/cat /dev/null).txt"),
    ("empty span", "R", r"echo notes${CF_NOPE}.txt"),
    ("empty span", "R", r"echo 1733$(/usr/bin/true)02"),
    (
        "empty span",
        "R",
        r#"printf '%s\n' "1733$(/bin/cat /dev/null)02""#,
    ),
    // Globs, braces and tildes.
    ("expansion", "U", r"printf '%s\n' 1733{02,} 173302"),
    ("expansion", "U", r"printf '%s\n' ~ 173302"),
    ("expansion", "U", r"printf '%s\n' * 173302"),
];

struct Shell {
    label: String,
    program: PathBuf,
    args: &'static [&'static str],
    dialect: Dialect,
}

fn version(program: &Path) -> Option<String> {
    let out = Command::new(program)
        .arg("-c")
        .arg("echo \"$BASH_VERSION$ZSH_VERSION\"")
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8(out.stdout).unwrap_or_default())
}

fn shells(shell_dir: Option<&Path>) -> (Vec<Shell>, Vec<String>) {
    let executable = |name: &str| {
        shell_dir.map_or_else(
            || PathBuf::from(name),
            |dir| dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX)),
        )
    };
    let mut found = Vec::new();
    let mut skipped = Vec::new();
    let mut bash_versions = Vec::new();
    let bash_programs: &[&str] = if cfg!(windows) || shell_dir.is_some() {
        &["bash"]
    } else {
        &["bash", "/bin/bash"]
    };
    for &program in bash_programs {
        let program = executable(program);
        match version(&program) {
            Some(v) if !bash_versions.contains(&v) => {
                bash_versions.push(v.clone());
                found.push(Shell {
                    label: format!(
                        "{} {}",
                        program.display(),
                        v.split('(').next().unwrap_or_default()
                    ),
                    program,
                    args: &["--norc", "--noprofile"],
                    dialect: Dialect::Bash,
                });
            }
            Some(_) => {}
            None => skipped.push(program.display().to_string()),
        }
    }
    let zsh = executable("zsh");
    match version(&zsh) {
        Some(v) => found.push(Shell {
            label: format!(
                "{} {}",
                zsh.display(),
                v.split('\n').next().unwrap_or_default()
            ),
            program: zsh,
            args: &["-f"],
            dialect: Dialect::Zsh,
        }),
        None => skipped.push(zsh.display().to_string()),
    }
    let sh = if cfg!(windows) || shell_dir.is_some() {
        "sh"
    } else {
        "/bin/sh"
    };
    for program in [sh, "dash"] {
        let program = executable(program);
        if version(&program).is_some() {
            found.push(Shell {
                label: program.display().to_string(),
                program,
                args: &[],
                dialect: Dialect::Sh,
            });
        } else {
            skipped.push(program.display().to_string());
        }
    }
    (found, skipped)
}

fn run(shell: &Shell, line: &str, dir: &Path, child_path: Option<&OsStr>) -> Vec<u8> {
    let mut command = Command::new(&shell.program);
    if let Some(path) = child_path {
        command.env("PATH", path);
    }
    command
        .args(shell.args)
        .arg("-c")
        .arg(line)
        .current_dir(dir)
        .env("HOME", dir)
        .env("LC_ALL", "C")
        .env("CF_PART", "1733")
        .env_remove("BASH_ENV")
        .env_remove("ENV")
        .env_remove("ZDOTDIR")
        .env_remove("CDPATH")
        .env_remove("IFS")
        .output()
        .unwrap()
        .stdout
}

#[derive(Default)]
struct Tally {
    runs: usize,
    exact: usize,
    unresolved: usize,
}

#[test]
fn layer_one_values_match_the_shells_byte_for_byte() {
    let dir = tempfile::tempdir().unwrap();
    let shell_dir = std::env::var_os("CODEFLOW_DIFFERENTIAL_SHELL_DIR").map(|value| {
        assert!(
            !value.is_empty(),
            "CODEFLOW_DIFFERENTIAL_SHELL_DIR must not be empty"
        );
        // Resolve relative directories before the shell's working directory changes.
        std::env::current_dir().unwrap().join(value)
    });
    let child_path = shell_dir.as_ref().map(|dir| {
        let inherited = std::env::var_os("PATH").unwrap_or_default();
        std::env::join_paths(std::iter::once(dir.clone()).chain(std::env::split_paths(&inherited)))
            .expect("CODEFLOW_DIFFERENTIAL_SHELL_DIR must be usable in a child PATH")
    });
    let (shells, skipped) = shells(shell_dir.as_deref());
    assert!(
        skipped.is_empty(),
        "required differential shells unavailable: {skipped:?}; install bash, zsh, sh and dash in CODEFLOW_DIFFERENTIAL_SHELL_DIR when set, otherwise on PATH (with /bin/sh and /bin/bash on Unix); configured directories never fall back to PATH"
    );
    assert!(
        shells.iter().any(|s| s.dialect == Dialect::Bash),
        "no bash to compare against"
    );
    let mut tally = Tally::default();
    let mut wrong = Vec::new();
    for (family, expect, line) in CASES {
        let mut marked = false;
        for shell in &shells {
            let printed = run(shell, line, dir.path(), child_path.as_deref());
            marked |= printed.windows(6).any(|w| w == b"173302")
                || printed.windows(9).any(|w| w == b"notes.txt");
            tally.runs += 1;
            let claim = final_output(line, shell.dialect);
            let resolved = match &claim {
                Some(Composed::Resolved(value)) => {
                    tally.exact += 1;
                    let fits =
                        String::from_utf8(printed.clone()).is_ok_and(|text| value.matches(&text));
                    if !fits {
                        wrong.push(format!(
                            "{}: {family}: `{line}`: layer 1 {:?}, shell {:?}",
                            shell.label,
                            value.render(),
                            String::from_utf8_lossy(&printed)
                        ));
                    }
                    true
                }
                Some(Composed::Unresolved(_)) | None => {
                    tally.unresolved += 1;
                    false
                }
            };
            let must_resolve = match (*expect, shell.dialect) {
                ("R", Dialect::Bash | Dialect::Zsh)
                | ("Rb", Dialect::Bash)
                | ("Rz", Dialect::Zsh) => Some(true),
                ("U", Dialect::Bash | Dialect::Zsh)
                | ("Rb", Dialect::Zsh)
                | ("Rz", Dialect::Bash) => Some(false),
                _ => None,
            };
            if must_resolve.is_some_and(|must| must != resolved) {
                wrong.push(format!(
                    "{}: {family}: `{line}`: expected {expect}, layer 1 {claim:?}",
                    shell.label
                ));
            }
        }
        if !marked {
            wrong.push(format!("{family}: `{line}` prints its marker in no shell"));
        }
    }
    eprintln!(
        "{} lines x {} shells ({}): {} runs, {} resolved, {} unresolved; skipped: {skipped:?}",
        CASES.len(),
        shells.len(),
        shells
            .iter()
            .map(|s| s.label.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        tally.runs,
        tally.exact,
        tally.unresolved
    );
    assert!(
        wrong.is_empty(),
        "{} mismatches:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}
