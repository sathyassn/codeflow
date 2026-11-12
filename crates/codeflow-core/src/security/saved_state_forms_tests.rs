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
        both_hooks(&f, &places, &format!("echo x {op} {l}/h"), &writes, &mut wrong);
        both_hooks(&f, &places, &format!("echo x {op}{l}/h"), &writes, &mut wrong);
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

/// A zsh modifier or subscript on a variable the guard fills (`$NAME:a`,
/// `${NAME[1,-1]}`) changes the value in a way the guard does not compute,
/// so where the value is or holds saved state the write fails closed, at
/// the home and at a moved folder. The same forms on a variable aimed at an
/// ordinary folder still run.
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
        (moved.clone(), "CLAUDE_CONFIG_DIR", "/session-env/h", slashed(&conf)),
        (with_env, "CLAUDE_ENV_FILE", "", slashed(&env_file)),
    ] {
        for form in [
            format!("${name}:a"),
            format!("${name}:A"),
            format!("${name}:h"),
            format!("${{{name}:a}}"),
            format!("${name}[1,-1]"),
            format!("${{{name}[1,-1]}}"),
        ] {
            let word = format!("{form}{rest}");
            let message = format!(
                "a redirect writes `{word}`, which the guard cannot resolve, and it applies a modifier or subscript to `${name}`, whose value `{value}` is or holds saved shell state; {NEAR}"
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
    let out = slashed(&f.project.join("out"));
    for command in [
        format!("OUT={out}; echo x > $OUT:a/h"),
        format!("OUT={out}; echo x > ${{OUT[1,-1]}}/h"),
        "echo x > $PROJ:a/h".to_string(),
        "echo x > ${PROJ[1,-1]}/h".to_string(),
        "ls $HOME:a/.claude/session-env".to_string(),
    ] {
        both_hooks(&f, &moved, &command, &[], &mut wrong);
    }
    assert!(wrong.is_empty(), "wrong verdicts:\n{}", wrong.join("\n"));
}

/// The kernel reads a symbolic link's relative text from the folder the
/// link is in, so a link made in another folder than the command runs in
/// is judged from there: a link to a location refuses in both runs, and a
/// relative link to an ordinary folder still runs.
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
    assert_ne!(lexical(&f.project.join(text)), lexical(&conf.join("session-env")));
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
    for (places, cwd) in [(&in_project, &f.project), (&in_scratch, &scratch)] {
        for command in commands {
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
