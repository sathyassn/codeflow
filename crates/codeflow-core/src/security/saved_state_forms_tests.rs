//! More shell forms the saved-state pass reads (TSK-1461): redirect
//! spellings, changed expansions of a variable the guard fills, link text,
//! the ways through and the sanctioned path the refusals name, the cost of
//! a location inside a project, and more copy and search modes. Each row
//! runs through exec-guard and git-guard.

use super::*;

/// The saved-state label of the moved Claude configuration folder's
/// `session-env`.
const LABEL: &str = "$CLAUDE_CONFIG_DIR/session-env";

/// The way through a refusal near saved state.
const NEAR: &str = "aim the program at a named sub-folder that holds no saved shell state, or name the file it writes";

/// The way through a program that places files in the folder it runs in,
/// which holds saved state.
const PLACE: &str = "keep the harness configuration folder outside the project, or run the program in a folder that holds no saved shell state";

/// A Claude configuration folder the session moved outside the home, with
/// a `session-env` location and a neighbour folder, and the locations for
/// it.
fn moved_conf(f: &Fixture) -> (PathBuf, Places) {
    let conf = f.home.parent().unwrap().join("conf");
    mkdir(&conf.join("session-env/abc"));
    mkdir(&conf.join("projects/p"));
    let places = Places {
        claude_config: Some(conf.clone()),
        ..f.places()
    };
    (conf, places)
}

/// zsh writes the file after each of its clobbering redirects, in every
/// spelling, wherever the line holds it: after a comment, a heredoc body or
/// an ANSI-C string with a quote in it too. Quoted data that holds these
/// characters, and plain redirects to ordinary files, still run.
#[test]
fn every_redirect_spelling_refuses_through_both_hooks() {
    let f = Fixture::new();
    let (conf, places) = moved_conf(&f);
    let l = format!("{}/session-env", slashed(&conf));
    let writes = [format!(
        "a redirect writes the saved shell state file `{LABEL}`"
    )];
    let mut wrong = Vec::new();
    for op in [
        ">!", ">>!", ">|", ">>|", ">&!", ">&|", ">>&!", ">>&|", "&>!", "&>|", "&>>!", "&>>|",
    ] {
        both_hooks(
            &f,
            &places,
            &format!("echo x {op} {l}/h"),
            &writes,
            &mut wrong,
        );
        both_hooks(
            &f,
            &places,
            &format!("echo x {op}{l}/h"),
            &writes,
            &mut wrong,
        );
    }
    for command in [
        format!("# it's a note\necho x >! {l}/h"),
        format!("# say \"hi\necho x >>| {l}/h"),
        format!("cat <<'E' > notes.md\nit's\nE\necho x >&! {l}/h"),
        format!("cat <<E > notes.md\n\"\nE\necho x >>&| {l}/h"),
        format!("echo $'it\\'s'; echo x >! {l}/h"),
    ] {
        both_hooks(&f, &places, &command, &writes, &mut wrong);
    }
    for command in [
        "echo 'a >! b >>| c >&! d &>| e'",
        "echo \"x >>&! y\"",
        "echo x > notes.md",
        "echo x >! notes.md",
        "echo x &>>| notes.md",
        "printf '%s\\n' '>|' >| out.txt",
        "echo x 2>&1 | cat",
    ] {
        both_hooks(&f, &places, command, &[], &mut wrong);
    }
    assert!(wrong.is_empty(), "wrong verdicts:\n{}", wrong.join("\n"));
}

/// A zsh flag, modifier or subscript on a variable the guard fills, braced,
/// unbraced or nested, changes the value in a way the guard does not
/// compute, so where the value is or holds saved state the write fails
/// closed, at the home and at a moved folder, for a variable the line
/// assigns too. The same forms on a variable aimed at an ordinary folder
/// still run.
#[test]
fn a_changed_expansion_of_a_filled_variable_fails_closed() {
    let f = Fixture::new();
    mkdir(&f.home.join(".claude/session-env"));
    let (conf, moved) = moved_conf(&f);
    let env_file = f.home.parent().unwrap().join("env/hook-1.sh");
    let with_env = Places {
        claude_env_file: Some(env_file.clone()),
        ..f.places()
    };
    let mut wrong = Vec::new();
    for (places, name, rest, value) in [
        (
            f.places(),
            "HOME",
            "/.claude/session-env/h",
            slashed(&f.home),
        ),
        (
            moved.clone(),
            "CLAUDE_CONFIG_DIR",
            "/session-env/h",
            slashed(&conf),
        ),
        (with_env, "CLAUDE_ENV_FILE", "", slashed(&env_file)),
    ] {
        for form in [
            format!("${name}:a"),
            format!("${name}:A"),
            format!("${name}:h"),
            format!("${{{name}:a}}"),
            format!("${name}[1,-1]"),
            format!("${{{name}[1,-1]}}"),
            format!("$={name}"),
            format!("$~{name}"),
            format!("$^{name}"),
            format!("$=~{name}"),
            format!("${{${{{name}}}:a}}"),
            format!("${{${{{name}}}}}"),
        ] {
            let word = format!("{form}{rest}");
            let message = format!(
                "a redirect writes `{word}`, which the guard cannot resolve, and it changes `${name}` with a zsh flag, modifier or subscript, and its value `{value}` is or holds saved shell state; {NEAR}"
            );
            both_hooks(
                &f,
                &places,
                &format!("echo x >> {word}"),
                &[message],
                &mut wrong,
            );
        }
    }
    // The command reader hands the pass words with their quotes removed,
    // so a quote or backslash left in a word is a literal character and
    // never hides a later change of a variable.
    let p = slashed(&conf);
    for (command, word) in [
        (
            "echo x >> \"it's\"/../../../../../../../..$CLAUDE_CONFIG_DIR:a/session-env/h",
            "it's/../../../../../../../..$CLAUDE_CONFIG_DIR:a/session-env/h",
        ),
        (
            "echo x >> ../../../../../../../\"x\\\\\"$CLAUDE_CONFIG_DIR:a/session-env/h",
            "../../../../../../../x\\$CLAUDE_CONFIG_DIR:a/session-env/h",
        ),
    ] {
        let message = format!(
            "a redirect writes `{word}`, which the guard cannot resolve, and it changes `$CLAUDE_CONFIG_DIR` with a zsh flag, modifier or subscript, and its value `{p}` is or holds saved shell state; {NEAR}"
        );
        both_hooks(&f, &moved, command, &[message], &mut wrong);
    }
    // A variable the line assigns is filled the same way.
    both_hooks(
        &f,
        &moved,
        &format!("V={p}; echo x >> $V:a/session-env/h"),
        &[format!(
            "a redirect writes `$V:a/session-env/h`, which the guard cannot resolve, and it changes `$V` with a zsh flag, modifier or subscript, and its value `{p}` is or holds saved shell state; {NEAR}"
        )],
        &mut wrong,
    );
    let out = slashed(&f.project.join("out"));
    for command in [
        format!("OUT={out}; echo x > $OUT:a/h"),
        format!("OUT={out}; echo x > ${{OUT[1,-1]}}/h"),
        format!("OUT={out}; echo x > $=OUT/h"),
        format!("OUT={out}; echo x > ${{${{OUT}}:a}}/h"),
        "echo x > $PROJ:a/h".to_string(),
        "echo x > ${PROJ[1,-1]}/h".to_string(),
        "echo x > $~PROJ/h".to_string(),
        "ls $HOME:a/.claude/session-env".to_string(),
        "ls $=HOME/.claude/session-env".to_string(),
        "echo \"it's\" > notes.md".to_string(),
    ] {
        both_hooks(&f, &moved, &command, &[], &mut wrong);
    }
    assert!(wrong.is_empty(), "wrong verdicts:\n{}", wrong.join("\n"));
}

/// A word can change more than one variable the guard fills. Each changed
/// variable is judged, wherever it stands in the word: the write fails
/// closed when any one's value is or holds saved state, after a variable
/// aimed at an ordinary folder too, at the home and at a moved folder. A
/// word whose changed variables are all aimed at ordinary folders still
/// runs.
#[test]
fn every_changed_variable_in_a_word_is_judged() {
    let f = Fixture::new();
    mkdir(&f.home.join(".claude/session-env"));
    let (conf, moved) = moved_conf(&f);
    // A startup variable aimed at an ordinary folder, above no location.
    let ordinary = f.home.parent().unwrap().join("zd");
    mkdir(&ordinary);
    let with_ordinary = |places: Places| Places {
        startup: StartupEnv {
            zdotdir: Some(ordinary.clone()),
            ..places.startup
        },
        ..places
    };
    let default = with_ordinary(f.places());
    let moved = with_ordinary(moved);
    let home = slashed(&f.home);
    let p = slashed(&conf);
    let out = slashed(&f.project.join("out"));
    let mut wrong = Vec::new();
    for (places, assign, word, name, value) in [
        // An ordinary variable first, then one whose value holds saved
        // state, braced and unbraced.
        (
            &moved,
            "",
            "${ZDOTDIR:h}/..${CLAUDE_CONFIG_DIR:a}/session-env/h".to_string(),
            "CLAUDE_CONFIG_DIR",
            &p,
        ),
        (
            &moved,
            "",
            "$ZDOTDIR:h/..$CLAUDE_CONFIG_DIR:a/session-env/h".to_string(),
            "CLAUDE_CONFIG_DIR",
            &p,
        ),
        (
            &default,
            "",
            "${ZDOTDIR:h}/..${HOME:a}/.claude/session-env/h".to_string(),
            "HOME",
            &home,
        ),
        // The reverse order.
        (
            &moved,
            "",
            "${CLAUDE_CONFIG_DIR:a}/session-env/x${ZDOTDIR:t}".to_string(),
            "CLAUDE_CONFIG_DIR",
            &p,
        ),
        (
            &default,
            "",
            "${HOME:a}/.claude/session-env/x${ZDOTDIR:t}".to_string(),
            "HOME",
            &home,
        ),
        // Three changed variables, the last one near saved state.
        (
            &moved,
            "OUT=",
            "${ZDOTDIR:h}/..${OUT:a}/..${CLAUDE_CONFIG_DIR:a}/session-env/h".to_string(),
            "CLAUDE_CONFIG_DIR",
            &p,
        ),
        (
            &default,
            "OUT=",
            "${ZDOTDIR:h}/..${OUT:a}/..${HOME:a}/.claude/session-env/h".to_string(),
            "HOME",
            &home,
        ),
    ] {
        let command = if assign.is_empty() {
            format!("echo x > {word}")
        } else {
            format!("{assign}{out}; echo x > {word}")
        };
        let message = format!(
            "a redirect writes `{word}`, which the guard cannot resolve, and it changes `${name}` with a zsh flag, modifier or subscript, and its value `{value}` is or holds saved shell state; {NEAR}"
        );
        both_hooks(&f, places, &command, &[message], &mut wrong);
    }
    // Every changed variable aimed at an ordinary folder: the write runs.
    for places in [&default, &moved] {
        for command in [
            format!("OUT={out}; echo x > ${{ZDOTDIR:h}}/..${{OUT:a}}/h"),
            format!("OUT={out}; echo x > $OUT:a/..$ZDOTDIR:t/h"),
            format!("OUT={out}; echo x > ${{OUT:h}}/x${{ZDOTDIR:t}}${{OUT:t}}"),
        ] {
            both_hooks(&f, places, &command, &[], &mut wrong);
        }
    }
    assert!(wrong.is_empty(), "wrong verdicts:\n{}", wrong.join("\n"));
}

/// A variable the guard fills that a word reads anywhere inside a changed
/// expansion counts as changed, at every depth of nesting: inside an
/// unnamed or a named expansion, one, two or three levels down, braced or
/// not, alone or after variables aimed at ordinary folders, in the
/// default, replacement, flag and subscript forms, and through a tilde
/// word inside the expansion. Where its value is or
/// holds saved state the write fails closed, at the home and at a moved
/// folder. A default whose value is the variable itself, with nothing
/// changed around it, refuses as the path it names.
#[test]
fn a_variable_nested_in_a_changed_expansion_is_judged() {
    let f = Fixture::new();
    let (conf, default, moved) = ordinary_startup(&f);
    let home = slashed(&f.home);
    let p = slashed(&conf);
    let mut wrong = Vec::new();
    for (places, name, rest, value, label) in [
        (&moved, "CLAUDE_CONFIG_DIR", "/session-env/h", &p, LABEL),
        (
            &default,
            "HOME",
            "/.claude/session-env/h",
            &home,
            "~/.claude/session-env",
        ),
    ] {
        // An alternative that is the variable itself, with nothing changed
        // around it, is judged as the path it names.
        for form in [
            format!("${{:-${name}}}"),
            format!("${{NOPE:-${name}}}"),
            format!("${{NOPE:-~{name}}}"),
        ] {
            both_hooks(
                &f,
                places,
                &format!("echo x > {form}{rest}"),
                &[format!(
                    "a redirect writes the saved shell state file `{label}`"
                )],
                &mut wrong,
            );
        }
        for form in [
            // Unnamed, one, two and three levels down.
            format!("${{${{:-${name}}}:a}}"),
            format!("${{${{${{:-${name}}}}}:a}}"),
            format!("${{${{${{${{:-${name}}}}}}}:a}}"),
            format!("${{${{:-${{{name}}}}}:a}}"),
            // Named, one, two and three levels down.
            format!("${{${{NOPE:-${name}}}:a}}"),
            format!("${{${{${{NOPE:-${name}}}}}:a}}"),
            format!("${{${{${{${{NOPE:-${name}}}}}}}:a}}"),
            format!("${{${{ZDOTDIR:+${name}}}:a}}"),
            // Other changes around the nested one.
            format!("${{=${{:-${name}}}}}"),
            format!("$={{${{:-${name}}}}}"),
            format!("${{${{:-${name}}}[1,-1]}}"),
            format!("${{ZDOTDIR/#$ZDOTDIR/${name}}}"),
            format!("${{ZDOTDIR[1,${name}]}}"),
            format!("${{{name}}}[1,-1]"),
            // After variables aimed at ordinary folders.
            format!("${{${{:-$ZDOTDIR}}:h}}/..${{${{:-${name}}}:a}}"),
            format!("${{${{:-$ZDOTDIR/..${name}}}:a}}"),
            format!("${{${{${{NOPE:-$ZDOTDIR}}}}:h}}/..${{${{${{${{:-${name}}}}}}}:a}}"),
        ] {
            let word = format!("{form}{rest}");
            let message = format!(
                "a redirect writes `{word}`, which the guard cannot resolve, and it changes `${name}` with a zsh flag, modifier or subscript, and its value `{value}` is or holds saved shell state; {NEAR}"
            );
            both_hooks(
                &f,
                places,
                &format!("echo x > {word}"),
                &[message],
                &mut wrong,
            );
        }
    }
    // zsh flags around a nested expansion. At the home, the line also
    // names the location in words, and the command reader's own finding
    // for the flag group refuses it first.
    for flags in ["U", "j:/:"] {
        let word = format!("${{({flags})${{:-$CLAUDE_CONFIG_DIR}}}}/session-env/h");
        both_hooks(
            &f,
            &moved,
            &format!("echo x > {word}"),
            &[format!(
                "a redirect writes `{word}`, which the guard cannot resolve, and it changes `$CLAUDE_CONFIG_DIR` with a zsh flag, modifier or subscript, and its value `{p}` is or holds saved shell state; {NEAR}"
            )],
            &mut wrong,
        );
        both_hooks(
            &f,
            &default,
            &format!("echo x > ${{({flags})${{:-$HOME}}}}/.claude/session-env/h"),
            &[format!(
                "text this call produces names the saved shell state location `.claude/session-env`, and the line runs `{flags}`, which is not a data reader, so it could run or apply that text; read the file in its own call"
            )],
            &mut wrong,
        );
    }
    // A tilde alone as an alternative is the home.
    both_hooks(
        &f,
        &default,
        "echo x > ${NOPE:-~}/.claude/session-env/h",
        &["a redirect writes the saved shell state file `~/.claude/session-env`".to_string()],
        &mut wrong,
    );
    assert!(wrong.is_empty(), "wrong verdicts:\n{}", wrong.join("\n"));
}

/// Each alternative of a default, assignment or alternate-value form is
/// judged as the path it gives, with the changes around it: the write
/// refuses when any alternative is or lies in saved state, two levels of
/// alternatives down too, and an outer change of a nested alternative
/// fails closed as before. A default that gives an ordinary path under a
/// home that holds saved state still runs.
#[test]
fn alternatives_are_judged_as_their_own_paths() {
    let f = Fixture::new();
    let (conf, default, moved) = ordinary_startup(&f);
    let p = slashed(&conf);
    let mut wrong = Vec::new();
    for (places, name, rest, label) in [
        (&moved, "CLAUDE_CONFIG_DIR", "/session-env/h", LABEL),
        (
            &default,
            "HOME",
            "/.claude/session-env/h",
            "~/.claude/session-env",
        ),
    ] {
        for op in ["-", ":-", "=", ":=", "::=", "+", ":+"] {
            both_hooks(
                &f,
                places,
                &format!("echo x > ${{OUT{op}${name}{rest}}}"),
                &[format!(
                    "a redirect writes the saved shell state file `{label}`"
                )],
                &mut wrong,
            );
        }
        for word in [
            format!("${{OUT:-${{NOPE:-${name}{rest}}}}}"),
            format!("${{OUT:+${{NOPE:-${name}{rest}}}}}"),
            format!("${{OUT:-${{NOPE:+${name}}}}}{rest}"),
        ] {
            both_hooks(
                &f,
                places,
                &format!("echo x > {word}"),
                &[format!(
                    "a redirect writes the saved shell state file `{label}`"
                )],
                &mut wrong,
            );
        }
    }
    // An outer change of nested alternatives still fails closed.
    let word = "${${OUT:-${NOPE:-$CLAUDE_CONFIG_DIR}}:a}/session-env/h";
    both_hooks(
        &f,
        &moved,
        &format!("echo x > {word}"),
        &[format!(
            "a redirect writes `{word}`, which the guard cannot resolve, and it changes `$CLAUDE_CONFIG_DIR` with a zsh flag, modifier or subscript, and its value `{p}` is or holds saved shell state; {NEAR}"
        )],
        &mut wrong,
    );
    // Alternatives that give ordinary paths run, under a home that holds
    // saved state too.
    for places in [&default, &moved] {
        for command in [
            "echo x > ${OUT:-$HOME/out.log}",
            "echo x > ${OUT:-~/out.log}",
            "echo x > ${OUT-$HOME/out.log}",
            "echo x > ${OUT:=$HOME/out.log}",
            "echo x > ${OUT:+$HOME/out.log}",
            "echo x > ${OUT:-${NOPE:-$HOME/out.log}}",
            "echo x > ${OUT:-$HOME}/out.log",
            "echo x > ${OUT:-$ZDOTDIR/h}",
        ] {
            both_hooks(&f, places, command, &[], &mut wrong);
        }
    }
    assert!(wrong.is_empty(), "wrong verdicts:\n{}", wrong.join("\n"));
}

/// The home with a saved-state folder, a startup variable aimed at an
/// ordinary folder, and the moved Claude configuration folder: the moved
/// folder, then the default and the moved locations with that variable.
fn ordinary_startup(f: &Fixture) -> (PathBuf, Places, Places) {
    mkdir(&f.home.join(".claude/session-env"));
    let (conf, moved) = moved_conf(f);
    let ordinary = f.home.parent().unwrap().join("zd");
    mkdir(&ordinary);
    let with_ordinary = |places: Places| Places {
        startup: StartupEnv {
            zdotdir: Some(ordinary.clone()),
            ..places.startup
        },
        ..places
    };
    (conf, with_ordinary(f.places()), with_ordinary(moved))
}

/// Nestings of ordinary variables and literals inside changed expansions
/// still run, at the home and at a moved folder.
#[test]
fn nested_expansions_of_ordinary_values_still_run() {
    let f = Fixture::new();
    let (_, default, moved) = ordinary_startup(&f);
    let out = slashed(&f.project.join("out"));
    let mut wrong = Vec::new();
    for places in [&default, &moved] {
        for command in [
            "echo x > ${${:-$ZDOTDIR}:a}/h".to_string(),
            "echo x > ${NOPE:-$ZDOTDIR}/h".to_string(),
            "echo x > ${${${:-$ZDOTDIR}}:a}/h".to_string(),
            "echo x > ${${${${NOPE:-$ZDOTDIR}}}:a}/h".to_string(),
            "echo x > ${(U)${:-$ZDOTDIR}}/h".to_string(),
            "echo x > ${${:-out}:a}/h".to_string(),
            "echo x > ${${${:-out/x}:h}:a}/h".to_string(),
            "echo x > ${#ZDOTDIR}.log".to_string(),
            "echo x > ${NOPE:-~ZDOTDIR}/h".to_string(),
            "echo x > ${ZDOTDIR}/h".to_string(),
            "echo x > out-$(date).log".to_string(),
            format!("OUT={out}; echo x > ${{${{OUT:-$ZDOTDIR}}:h}}/h"),
            format!("OUT={out}; echo x > ${{${{${{:-$OUT}}}}:a}}/h"),
        ] {
            both_hooks(&f, places, &command, &[], &mut wrong);
        }
    }
    assert!(wrong.is_empty(), "wrong verdicts:\n{}", wrong.join("\n"));
}

/// A changed expansion the guard cannot read to the end fails closed: an
/// indirect name, evaluation or prompt expansion of a value, a flag that
/// takes an argument it does not read, a command substitution inside it, a
/// tilde word for a home or folder it cannot read, an expansion left open,
/// a closing brace left over after one, or more alternatives than the
/// guard reads. The guard cannot tell which variables such a word reads,
/// so it is read as near saved state, at the home and at a moved folder,
/// whatever the run folder holds.
#[test]
fn a_changed_expansion_the_guard_cannot_read_fails_closed() {
    let f = Fixture::new();
    mkdir(&f.home.join(".claude/session-env"));
    let (_, moved) = moved_conf(&f);
    let mut wrong = Vec::new();
    for places in [&f.places(), &moved] {
        // The line, and the word as the command reader hands it over: a
        // flag group is cut at its first `)`, and a command substitution
        // stands as a marker.
        for (line, word) in [
            (
                "V=CLAUDE_CONFIG_DIR; echo x > ${(P)V}/session-env/h",
                "${(P)V}/session-env/h",
            ),
            (
                "V=CLAUDE_CONFIG_DIR; echo x > ${!V}/session-env/h",
                "${!V}/session-env/h",
            ),
            (
                "echo x > ${(P)${:-CLAUDE_CONFIG_DIR}}/session-env/h",
                "${(P)${:-CLAUDE_CONFIG_DIR}}/session-env/h",
            ),
            (
                "V=X; echo x > ${(e)V}/session-env/h",
                "${(e)V}/session-env/h",
            ),
            (
                "echo x > ${(s:)}:)CLAUDE_CONFIG_DIR}/session-env/h",
                "${(s:)}:",
            ),
            (
                "echo x > ${(%):-%/}/session-env/h",
                "${(%):-%/}/session-env/h",
            ),
            (
                "echo x > ${${:-$(pwd)}:a}/session-env/h",
                "${${:-\u{2}}:a}/session-env/h",
            ),
            (
                "echo x > ${NOPE:-x/session-env/h",
                "${NOPE:-x/session-env/h",
            ),
            (
                "echo x > ${NOPE:-}x}/session-env/h",
                "${NOPE:-}x}/session-env/h",
            ),
            (
                "echo x > ${NOPE:-~nobody}/session-env/h",
                "${NOPE:-~nobody}/session-env/h",
            ),
            (
                "echo x > ${NOPE:-~+}/session-env/h",
                "${NOPE:-~+}/session-env/h",
            ),
            ("V=x; echo x > ${V@P}/session-env/h", "${V@P}/session-env/h"),
            (
                "echo x > ${A:-a}${B:-b}${C:-c}${D:-d}${E:-e}${F:-f}${G:-g}/session-env/h",
                "${A:-a}${B:-b}${C:-c}${D:-d}${E:-e}${F:-f}${G:-g}/session-env/h",
            ),
        ] {
            let message = format!(
                "a redirect writes `{word}`, which the guard cannot resolve, and a changed expansion in it is one the guard cannot read, so its value may be or hold saved shell state; {NEAR}"
            );
            both_hooks(&f, places, line, &[message], &mut wrong);
        }
    }
    assert!(wrong.is_empty(), "wrong verdicts:\n{}", wrong.join("\n"));
}

/// The kernel reads a symbolic link's relative text from the folder the
/// link is in, so a link made in another folder than the command runs in
/// is judged from there, and a destination is tried as a folder whatever
/// it is now, unless `-T` makes it the link itself: a link to a location
/// refuses in both runs, and a relative link to an ordinary folder still
/// runs.
#[cfg(unix)]
#[test]
fn link_text_is_read_from_the_link_s_folder() {
    let f = Fixture::new();
    let (conf, places) = moved_conf(&f);
    let sub = f.project.join("sub");
    mkdir(&sub);
    mkdir(&f.project.join("a/b"));
    let text = "../../../../conf/session-env";
    // From `sub` the text reaches the location; from the project it does not.
    assert_eq!(lexical(&sub.join(text)), lexical(&conf.join("session-env")));
    assert_ne!(
        lexical(&f.project.join(text)),
        lexical(&conf.join("session-env"))
    );
    let first = [format!(
        "`ln` targets the saved shell state location `{LABEL}`"
    )];
    let identity = |operand: &str| {
        format!(
            "`ln` links `{operand}`, which is or holds saved shell state: `{}/session-env`; link a named file that is not saved shell state, or a sub-folder that holds none",
            slashed(&conf)
        )
    };
    let mut wrong = Vec::new();
    for (command, operand) in [
        (format!("ln -s {text} sub/lnk && echo x > sub/lnk/h"), text),
        (format!("ln -s {text} sub/"), text),
        // Made from `sub` into `a/b`: the text reaches the location only
        // from `a/b`, neither from `sub` nor from the project.
        (
            "cd sub && ln -s ../../../../../conf/session-env ../a/b/lnk3 && echo x > ../a/b/lnk3/h"
                .to_string(),
            "../../../../../conf/session-env",
        ),
        // The folder is made earlier on the line, so it is absent when the
        // guard reads the line: the link may still land in it.
        (
            format!("mkdir d && ln -s {text} d && echo x > d/session-env/h"),
            text,
        ),
    ] {
        both_hooks(&f, &places, &command, &first, &mut wrong);
        let found = f.pass_with(&command, &places);
        if found != [identity(operand)] {
            wrong.push(format!("identity {command}: {found:?}"));
        }
    }
    for command in [
        "ln -s ../notes sub/lnk",
        "ln -s ../../../../conf/projects sub/lnk4",
        "cd sub && ln -s ../notes ../lnk5",
        // `-T` makes the link the destination itself, never a name in it.
        "ln -sT ../../../../conf/session-env e",
    ] {
        both_hooks(&f, &places, command, &[], &mut wrong);
    }
    assert!(wrong.is_empty(), "wrong verdicts:\n{}", wrong.join("\n"));
}

/// The refusals that named the home, `/etc` or a startup directory for a
/// folder above saved state name saved state instead, each with its way
/// through, and every saved-state refusal carries its own sanctioned path.
#[test]
fn near_refusals_name_saved_state_and_its_sanctioned_path() {
    let f = Fixture::new();
    let (conf, places) = moved_conf(&f);
    let p = slashed(&conf);
    let mut wrong = Vec::new();
    for (command, message) in [
        (
            format!("curl --frobnicate -o x {p}"),
            format!("`curl` is given `--frobnicate`, an option the guard does not read, with the home, `/etc` or a folder that holds saved shell state, where it can place a saved shell state file it does not name; {NEAR}"),
        ),
        (
            format!("7z x a.7z -o{p}"),
            format!("`7z` puts files where its options say, the guard does not read its options, and the call names the home, `/etc` or a folder that holds saved shell state; {NEAR}"),
        ),
        (
            format!("wget -P \"$X\" {p}"),
            format!("`wget` runs where the guard cannot tell the directory, on a line that names the home, `/etc` or a folder that holds saved shell state; {NEAR}"),
        ),
    ] {
        both_hooks(&f, &places, &command, &[message], &mut wrong);
    }
    let command = format!("echo x > {p}/session-env/h");
    for (hook, found) in [
        ("exec-guard", f.exec(&command, &places)),
        ("git-guard", f.git(&command, &places)),
    ] {
        let remedies: Vec<String> = found
            .iter()
            .filter(|v| v.message.contains("saved shell state"))
            .map(|v| v.remedy.to_string())
            .collect();
        if remedies.is_empty() || remedies.iter().any(|r| r != STATE_SANCTIONED) {
            wrong.push(format!("{hook}: {remedies:?}"));
        }
    }
    let edited = edit_target(&conf.join("session-env/h"), &places).expect("a finding");
    if edited.remedy.to_string() != STATE_SANCTIONED {
        wrong.push(format!("edit: {}", edited.remedy));
    }
    assert!(
        !STATE_SANCTIONED.contains("startup") && !STATE_SANCTIONED.contains("sandbox"),
        "{STATE_SANCTIONED}"
    );
    assert!(wrong.is_empty(), "wrong verdicts:\n{}", wrong.join("\n"));
}

/// The cost of a location inside a project, measured for the operator:
/// every folder above a location is one a placing program could fill, so
/// with `CLAUDE_CONFIG_DIR` inside the project, or `CODEX_HOME` inside a
/// temporary folder, these ordinary commands refuse there in both hooks.
#[test]
fn a_location_inside_a_project_makes_its_placements_refuse() {
    let f = Fixture::new();
    let inside = f.project.join(".claude-home");
    mkdir(&inside.join("session-env"));
    let in_project = Places {
        claude_config: Some(inside),
        ..f.places()
    };
    let scratch = f.home.parent().unwrap().join("scratch");
    mkdir(&scratch.join("codex/shell_snapshots"));
    let in_scratch = Places {
        codex_home: Some(scratch.join("codex")),
        ..f.places()
    };
    let commands = [
        "git checkout main",
        "git pull",
        "git merge topic",
        "tar xf a.tar",
        "unzip a.zip",
        "find . -name x -exec chmod 600 {} +",
        "echo x > \"$OUT\"",
    ];
    let mut refused = Vec::new();
    let mut wrong = Vec::new();
    for (places, cwd, label) in [
        (&in_project, &f.project, "$CLAUDE_CONFIG_DIR/session-env"),
        (&in_scratch, &scratch, "$CODEX_HOME/shell_snapshots"),
    ] {
        for command in commands {
            // A program that changes the folder it runs in names the
            // location there and the way through it can use.
            let program = command.split(' ').next().unwrap_or_default();
            let runs = (command.starts_with("git ") || ["tar", "unzip"].contains(&program))
                .then(|| {
                    format!(
                        "`{program}` runs in `{}`, which holds the saved shell state location `{label}`, where it can place a saved shell state file it does not name; {PLACE}",
                        slashed(cwd)
                    )
                });
            for (hook, found) in [
                (
                    "exec-guard",
                    fixed(places.clone(), || exec_judge(command, cwd)),
                ),
                (
                    "git-guard",
                    fixed(places.clone(), || git_judge(command, cwd)),
                ),
            ] {
                let messages = saved(&found);
                if messages.is_empty() {
                    wrong.push(format!("{hook} {command}: runs"));
                } else if runs.as_ref().is_some_and(|want| messages != [want.clone()]) {
                    wrong.push(format!("{hook} {command}: {messages:?}"));
                } else {
                    refused.push(format!("{hook} {command}: {}", messages[0]));
                }
            }
        }
    }
    // The same commands in a project with no location inside it run.
    for command in commands {
        both_hooks(&f, &f.places(), command, &[], &mut wrong);
    }
    assert!(
        wrong.is_empty(),
        "wrong verdicts:\n{}\nrefused:\n{}",
        wrong.join("\n"),
        refused.join("\n")
    );
}

/// `ditto` copying into a location or extracting above one, and `find`
/// following a link to a folder above a location (`-H`, `-L`) to remove
/// what it holds, refuse in both hooks.
#[cfg(unix)]
#[test]
fn more_copy_and_search_modes_refuse_through_both_hooks() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    let (conf, places) = moved_conf(&f);
    std::fs::write(conf.join("session-env/h"), "x").unwrap();
    mkdir(&f.project.join("s"));
    symlink(&conf, f.project.join("conf")).unwrap();
    let p = slashed(&conf);
    let mut wrong = Vec::new();
    for command in [
        format!("ditto s {p}/session-env"),
        format!("ditto -x -k a.zip {p}"),
        "find -H conf -delete".to_string(),
        "find -L conf -name h -delete".to_string(),
    ] {
        let found_exec = saved(&f.exec(&command, &places));
        let found_git = saved(&f.git(&command, &places));
        if found_exec.is_empty() || found_git.is_empty() {
            wrong.push(format!("{command}: {found_exec:?} {found_git:?}"));
        }
        if !saved(&without(|| f.exec(&command, &places))).is_empty() {
            wrong.push(format!("refused before: {command}"));
        }
    }
    for command in ["ditto s out", "find -H conf/projects -name x -delete"] {
        both_hooks(&f, &places, command, &[], &mut wrong);
    }
    assert!(wrong.is_empty(), "wrong verdicts:\n{}", wrong.join("\n"));
}
