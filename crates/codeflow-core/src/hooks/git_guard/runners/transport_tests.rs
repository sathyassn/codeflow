//! TSK-1489: the transport Git selects for each remote a command contacts.
//!
//! Each row writes a scratch repository configuration and reads it through
//! the same configuration reader the guards use, with the host's Git
//! configuration cut off. Nothing here contacts a remote or starts a helper.

use super::{contacts, select, Contact, Probe, Selected, HIDDEN_OPTION, UNPROVEN};
use crate::security::git_config::{
    current_branch, index_gitlinks, locate, read, Environment, Location,
};
use std::path::Path;

#[test]
fn a_transient_child_index_cannot_prove_absence() {
    let scratch = Scratch::new("");
    let args = vec!["filter-branch".to_string()];
    let child = super::super::Inputs::child(&args, &Environment::new()).unwrap();
    let result = index_gitlinks(&scratch.root, &[], &child.environment);
    assert!(
        result.unwrap_err() == "cannot establish an index or object input for this child; run the commands separately",
        "a transient child index was treated as the ordinary index"
    );
}

#[test]
fn uncertain_probe_views_have_a_fixed_bound() {
    let names = [
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_NO_REPLACE_OBJECTS",
    ];
    let mut literals = super::super::super::Literals::default();
    let mut inherited = Environment::new();
    for name in &names[..3] {
        literals.0.insert((*name).into(), Some("assigned".into()));
        inherited.insert((*name).into(), "inherited".into());
    }
    let launch = super::super::Launched::default();
    let views = super::super::launched_environments(&launch, &literals, &inherited).unwrap();
    assert_eq!(views.len(), 27);
    literals.0.insert(names[3].into(), Some("assigned".into()));
    let why = super::super::launched_environments(&launch, &literals, &inherited).unwrap_err();
    assert_eq!(why, "the line changes more of the variables that select the index or objects than the guard reads; run the commands separately");
}

#[test]
fn child_probe_views_include_mixed_inheritance() {
    let mut env = Environment::new();
    env.insert("GIT_NO_REPLACE_OBJECTS".into(), "1".into());
    env.insert("GIT_REPLACE_REF_BASE".into(), "refs/none/".into());
    let args = vec!["for-each-repo".into()];
    let views = super::super::Inputs::views(&args, &env, false).unwrap();
    for (first, second) in [
        ("1", crate::security::git_config::UNSET),
        (crate::security::git_config::UNSET, "refs/none/"),
    ] {
        assert!(views.iter().any(|view| view
            .environment
            .get("GIT_NO_REPLACE_OBJECTS")
            .map(String::as_str)
            == Some(first)
            && view
                .environment
                .get("GIT_REPLACE_REF_BASE")
                .map(String::as_str)
                == Some(second)));
    }
}

#[test]
fn opaque_probe_mentions_are_unresolved_after_segmentation() {
    for name in ["GIT_NO_REPLACE_OBJECTS", "GIT_REPLACE_REF_BASE"] {
        for segment in [
            format!("++{name}"),
            format!(": ${{{name}:=1}}"),
            format!(": $[{name}=1]"),
            format!("trap 'export {name}=1' DEBUG"),
        ] {
            let literals = super::super::probe_line_literals(std::slice::from_ref(&segment));
            assert_eq!(literals.0.get(name), Some(&None), "{segment}");
        }
    }
}

#[test]
fn export_options_make_probe_inputs_unresolved() {
    for segment in [
        "set -a",
        "set -ea",
        "set -o allexport",
        "set -eo allexport",
        "set \"$options\"",
        "builtin set -a",
        "bash -ac 'git fetch origin'",
        "sh -o allexport -c 'git fetch origin'",
        "env SHELLOPTS=allexport bash -c 'git fetch origin'",
        "SHELLOPTS=$options bash -c 'git fetch origin'",
    ] {
        let literals = super::super::probe_line_literals(&[segment.into()]);
        for name in super::super::probe_names() {
            assert_eq!(literals.0.get(name), Some(&None), "{segment}: {name}");
        }
    }
    for segment in ["set +a", "set -- -a", "echo 'set -a'", "bash -c 'echo -a'"] {
        assert!(
            super::super::probe_line_literals(&[segment.into()])
                .0
                .is_empty(),
            "{segment}"
        );
    }
}

#[test]
fn unresolved_probe_inputs_stay_unread_in_every_view() {
    let mut literals = super::super::super::Literals::default();
    let mut inherited = Environment::new();
    for name in super::super::probe_names() {
        literals.0.insert(name.into(), None);
        inherited.insert(name.into(), "inherited".into());
    }
    let launch = super::super::Launched::default();
    let views = super::super::launched_environments(&launch, &literals, &inherited).unwrap();
    assert_eq!(views.len(), 1);
    for name in super::super::probe_names() {
        assert_eq!(
            views[0].get(name).map(String::as_str),
            Some(crate::security::git_config::UNREAD_PROBE)
        );
    }
    literals
        .0
        .insert("GIT_NO_REPLACE_OBJECTS".into(), Some("assigned".into()));
    let views = super::super::launched_environments(&launch, &literals, &inherited).unwrap();
    assert_eq!(views.len(), 3);
    for view in views {
        for name in super::super::probe_names().filter(|name| *name != "GIT_NO_REPLACE_OBJECTS") {
            assert_eq!(
                view.get(name).map(String::as_str),
                Some(crate::security::git_config::UNREAD_PROBE)
            );
        }
    }
}

#[test]
fn indirect_writes_keep_inherited_probe_inputs_unread() {
    let mut wrong = Vec::new();
    for segment in [
        "((x))",
        ": $((x))",
        ": $[x]",
        "let x",
        "declare -i n=x",
        "typeset -i n=x",
        "integer n=x",
        "[[ x -eq 1 ]]",
        "test x -eq 1",
        "printf -v \"$destination\" %s 1",
        "declare \"$destination=1\"",
        "typeset \"$destination=1\"",
        "local \"$destination=1\"",
        "readonly \"$destination=1\"",
        "export \"$destination=1\"",
        "read \"$destination\"",
        "mapfile \"$destination\"",
        "getopts x \"$destination\"",
        "wait -p \"$destination\"",
        "array[x]=1",
        "${destination}=1",
    ] {
        let literals = super::super::probe_line_literals(&[segment.into()]);
        let inherited = super::super::probe_names()
            .map(|name| (name.into(), "inherited".into()))
            .collect();
        match super::super::launched_environments(
            &super::super::Launched::default(),
            &literals,
            &inherited,
        ) {
            Ok(views)
                if views.len() == 1
                    && super::super::probe_names().all(|name| {
                        views[0].get(name).map(String::as_str)
                            == Some(crate::security::git_config::UNREAD_PROBE)
                    }) => {}
            result => wrong.push(format!("{segment}: {result:?}")),
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn arithmetic_contexts_keep_probe_inputs_unread() {
    let mut wrong = Vec::new();
    for context in [
        r#": "${s:x}""#,
        r#": "${s:0:x}""#,
        r#": "${BASH_VERSINFO[x]}""#,
        "[[ -v a[x] ]]",
        "eval '((x))'",
        "eval ': $((x))'",
        r"eval \(\(x\)\)",
        ": \"$( ((x)); git fetch origin )\"",
        "# don't\n((x))",
        r"echo $'\''; ((x))",
        "repeat x do :; done",
        "float n=x",
        "typeset -F n=x",
        "typeset -E n=x",
        r#": "$s[x]""#,
    ] {
        let words = super::super::super::command_argv(context);
        if !super::super::probe_arithmetic(context, &words) {
            wrong.push(context);
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn parameter_evaluation_keeps_probe_inputs_unread() {
    let mut wrong = Vec::new();
    for context in [
        ": \"${?:x}\"",
        ": \"${$:x}\"",
        ": \"${-:x}\"",
        ": \"${#:x}\"",
        ": \"${!:x}\"",
        ": \"${!x}\"",
        ": \"${x@P}\"",
        ": \"${(P)x}\"",
        ": \"${(e)x}\"",
        "[[ -v $x ]]",
        "[ -v \"$x\" ]",
        "test -v \"$x\"",
        ": \"$(\\\n(x))\"",
        "(\\\n(x))",
        ": \"$\\\n{s:x}\"",
        "eval ': \"$(\\\n(x))\"'",
        "sh -c ': \"$\\\n{s:x}\"'",
    ] {
        let words = super::super::super::command_argv(context);
        if !super::super::probe_arithmetic(context, &words) {
            wrong.push(context);
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn continuation_provenance_keeps_probe_inputs_unread() {
    let mut wrong = Vec::new();
    for context in [
        "# don't\n: \"$(\\\n(x))\"",
        "# it's\n(\\\n(x))",
        "cat <<EOF\nit's\nEOF\n: \"$(\\\n(x))\"",
        ": $'\\'' \"$\\\n{s:x}\"",
        "cat <<EOF\nit's $\\\n{s:x}\nEOF",
        ": \"$(echo data; # don't\n: \"$(\\\n(x))\")\"",
    ] {
        let words = super::super::super::command_argv(context);
        if !super::super::probe_arithmetic(context, &words) {
            wrong.push(context);
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn shell_operand_evaluation_keeps_probe_inputs_unread() {
    let mut wrong = Vec::new();
    for context in [
        "printf %d \"$x\"",
        "printf -v result %d \"$x\"",
        "printf %n \"$n\"",
        "print -f %d \"$x\"",
        "shift \"$x\"",
        "return \"$x\"",
        "break \"$x\"",
        "continue \"$x\"",
        "exit \"$x\"",
        "zsh -c 'printf %d \"$x\"'",
    ] {
        let words = super::super::super::command_argv(context);
        if !super::super::probe_arithmetic(context, &words) {
            wrong.push(context);
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn shell_operand_data_keeps_probe_inputs_known() {
    for context in [
        "printf '%s\\n' \"$x\"",
        "print -f %s -- \"$x\"",
        "shift 2",
        "printf %d 2",
        "bash -c 'printf %d \"$x\"'",
        "cat <<'EOF'\nit's $\\\n{s:x}\nEOF",
        "echo '$\\\n{s:x}'",
        "echo data # don't $\\\nword",
        ": \"$\\\n(echo data)\"",
    ] {
        let words = super::super::super::command_argv(context);
        assert!(
            !super::super::probe_arithmetic(context, &words),
            "{context}"
        );
    }
}

#[test]
fn continued_substitution_keeps_nested_evaluation_evidence() {
    let context = ": \"$\\\n(echo data; ((x)))\"";
    let words = super::super::super::command_argv(context);
    assert!(super::super::probe_arithmetic(context, &words));
}

#[test]
fn continued_parameter_inputs_remain_unread() {
    let mut wrong = Vec::new();
    for context in [": \"$\\\ns[x]\"", ": \"$s\\\n[x]\""] {
        let words = super::super::super::command_argv(context);
        if !super::super::probe_arithmetic(context, &words) {
            wrong.push(context);
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn filled_operator_inputs_remain_unread() {
    let expanded = super::super::super::expand_shell_commands(
        "printf -v y %s _BASE; x=PIPESTATUS[GIT_REPLACE_REF$y=1]; f=-v; [ $f \"$x\" ]",
    );
    let segments = super::super::literal_segments(&expanded.segments);
    let literals = super::super::probe_line_literals_in_context(&segments);
    assert_eq!(literals.0.get("GIT_INDEX_FILE"), Some(&None));
}

#[test]
fn output_boundaries_preserve_uncertainty() {
    let mut wrong = Vec::new();
    for context in [
        "print -f %s -$1 'a[$x]' 1",
        "print -f %s \"$1\" 'a[$x]' 1",
        "print {-f,%d} \"$x\"",
        // Existing numeric-format control, already unread on the parent.
        "printf {%d,%s} \"$x\"",
        "print -f {1..2} \"$x\"",
        "printf {1..2} \"$x\"",
    ] {
        let expanded = super::super::super::expand_shell_commands(context);
        if !expanded.arithmetic {
            wrong.push(context);
        }
    }
    // Existing shell controls: configuration uncertainty is tested separately.
    for context in [
        "bash -c 'print -f %s \"$1\" value'",
        "bash -c 'printf {%d,%s} \"$x\"'",
    ] {
        if super::super::super::expand_shell_commands(context).arithmetic {
            wrong.push(context);
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn output_data_preserves_configuration_provenance() {
    let mut wrong = Vec::new();
    for context in [
        "printf 'Files (%d)\\n' 3",
        "printf '# %s\\n' title",
        "print 'Ready?'",
        "printf \"Processing $f\\n\"",
        "print \"Processing $f\\n\"",
        "print -f 'Files (%d)\\n' -- 3",
        "print -f \"Processing $f\\n\" -- title",
    ] {
        if super::super::super::configuration_shell_change(context).is_some() {
            wrong.push(context);
        }
    }
    // Existing controls: an expanding option can still hide a destination.
    for context in ["print \"$@\"", "printf \"$@\""] {
        if super::super::super::configuration_shell_change(context).is_none() {
            wrong.push(context);
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn output_data_without_operands_stays_known() {
    let mut wrong = Vec::new();
    for context in [
        "echo \"$msg\"",
        "echo \"${s:-literal}\"",
        "echo $(value)",
        "echo \"${a[@]}\"",
        "echo \"${a[*]}\"",
        r"echo \$s[x]",
    ] {
        let expanded = super::super::super::expand_shell_commands(context);
        if expanded.arithmetic {
            wrong.push(context);
        }
    }
    // A data word cannot conceal another command's operand reader.
    for context in [
        "echo message; print -f %d \"$x\"",
        "echo message; printf %d \"$x\"",
    ] {
        if !super::super::super::expand_shell_commands(context).arithmetic {
            wrong.push(context);
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn indirect_writers_clear_tracked_values() {
    let mut wrong = Vec::new();
    for context in [
        "o=origin; for o in x::y; do :; done",
        "o=origin; select o in x::y; do :; done",
        // Existing supported-writer control, already invalidated on the parent.
        "o=origin; while read o; do :; done",
        "o=origin; source file",
        "o=origin; . file",
        "o=origin; vared o",
        "o=origin; coproc change",
        "o=origin; zparseopts -A o a",
        "o=origin; zstyle -s pattern style o",
        "o=origin; zformat -f o format",
        "o=origin; strftime -s o format",
        "o=origin; sysread -o o",
        "o=origin; zstat -A o file",
        "o=origin; private o",
        "o=origin; foreach o (x::y)",
        "o=origin; printf \"prefix$format\" o",
        // Existing supported-writer controls.
        "o=origin; read o",
        "o=origin; getopts a o",
        "o=origin; mapfile o",
        "o=origin; readarray o",
    ] {
        let mut segments = super::super::super::expand_shell_commands(context).segments;
        segments.push("capture \"$o\"".into());
        let filled = super::super::literal_segments(&segments);
        if filled.last().unwrap() != "capture \"$o\"" {
            wrong.push(context);
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn unsettled_output_words_keep_inputs_unread() {
    let mut wrong = Vec::new();
    for context in [
        "false && f=%s; printf $f %d \"$x\"",
        "false && o=--; print $o -f %d \"$x\"",
        "(f=%s); printf $f %d \"$x\"",
        "true || f=%s; printf $f %d \"$x\"",
        "set -- -f %d \"$x\"; print \"$@\"",
        // Existing control, already unread before the preceding repair.
        "set -- -f %d \"$x\"; print \"$@\" tail",
        "set -- %d \"$x\"; printf \"$@\"",
        // Existing control, already unread before the preceding repair.
        "set -- %d \"$x\"; printf \"$@\" tail",
        "o=message; printf -v o %s -f; print \"$o\" %d \"$x\"",
        "n=0; printf -v n GIT_REPLACE_REF%s=1 _BASE; shift \"$n\"",
        "print \"$options\"",
        "printf \"$format\"",
        "print -r \"$options\"",
        // Existing destination control, already unread before the preceding repair.
        "printf -v out \"$format\"",
        "print -f \"$format\"",
        "print *",
        "printf *",
        "print $(value)",
        "printf $(value)",
    ] {
        let expanded = super::super::super::expand_shell_commands(context);
        let segments = super::super::literal_segments(&expanded.segments);
        let literals = super::super::expanded_probe_literals(&segments, expanded.arithmetic);
        if literals.0.get("GIT_INDEX_FILE") != Some(&None) {
            wrong.push(context);
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn literal_values_are_invalidated_by_builtin_writes() {
    let mut wrong = Vec::new();
    for context in [
        "o=message; printf -v o %s -f",
        "o=message; read o",
        "o=message; typeset o",
        "o=message; eval change",
        "o=message; read \"$target\"",
        "o=message; set -- data",
        "o=message; shift",
    ] {
        let expanded = super::super::super::expand_shell_commands(context);
        let mut segments = expanded.segments;
        segments.push("capture \"$o\"".into());
        let filled = super::super::literal_segments(&segments);
        if filled.last().unwrap() != "capture \"$o\"" {
            wrong.push(context);
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn expanded_output_option_inputs_remain_unread() {
    let mut wrong = Vec::new();
    for context in [
        "o=-f; print $o %d \"$x\"",
        "f=-u; print \"$f\" 1 -f %d \"$x\"",
        "print \"$options\" \"$x\"",
    ] {
        let expanded = super::super::super::expand_shell_commands(context);
        let segments = super::super::literal_segments(&expanded.segments);
        let literals = super::super::expanded_probe_literals(&segments, expanded.arithmetic);
        if literals.0.get("GIT_INDEX_FILE") != Some(&None) {
            wrong.push(context);
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn continued_special_parameter_inputs_remain_unread() {
    let context = ": \"$?\\\n[x]\"";
    let words = super::super::super::command_argv(context);
    assert!(super::super::probe_arithmetic(context, &words), "{context}");
}

#[test]
fn expanded_output_and_special_parameter_controls_stay_known() {
    for context in [
        "print -r -- \"$a\" \"$b\"",
        "printf %s \"$x\"",
        "echo \"$x\"",
        "echo \"$options\"",
        "echo -n \"$options\"",
        "echo *",
        "echo $(value)",
        "echo '*'",
        "printf '%s' '*'",
        "print -r -- '<->'",
        "bash -c 'o=-f; print $o %d \"$x\"'",
        "bash -c 'f=-u; print \"$f\" 1 -f %d \"$x\"'",
        ": \"$?\\\n[0]\"",
    ] {
        let expanded = super::super::super::expand_shell_commands(context);
        let segments = super::super::literal_segments(&expanded.segments);
        let literals = super::super::expanded_probe_literals(&segments, expanded.arithmetic);
        assert_ne!(literals.0.get("GIT_INDEX_FILE"), Some(&None), "{context}");
    }
}

#[test]
fn output_word_conservatism_is_explicit() {
    for context in [
        "print \"$msg\"",
        "o=-f; print $o %s \"$x\"",
        "f=-u; print \"$f\" 1 -f %s \"$x\"",
        "o=message; print \"$o\" \"$x\"",
        "print -f %s \"$x\"",
        "print -u 2 -f %s \"$x\"",
        "print -vout -f %s \"$x\"",
        "print -f %d \"$?\"",
    ] {
        let expanded = super::super::super::expand_shell_commands(context);
        assert!(expanded.arithmetic, "{context}");
    }
}

#[test]
fn valued_output_options_keep_inputs_unread() {
    let mut wrong = Vec::new();
    for context in [
        "print -v out -f %d \"$x\"",
        "print -u 2 -f %d \"$x\"",
        "print -u 1 -f %d \"$x\"",
        "print -C 2 -f %d \"$x\"",
        "print -x 2 -f %d \"$x\"",
        "print -X 2 -f %d \"$x\"",
        "print -u2 -vout -f %d \"$x\"",
        "print -ru 2 -f %d \"$x\"",
        "print -u 2 -v \"$destination\" data",
        "print -u \"$options\" data",
        "logout \"$x\"",
    ] {
        let words = super::super::super::command_argv(context);
        if !super::super::probe_arithmetic(context, &words) {
            wrong.push(context);
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn numeric_parameter_operands_keep_inputs_known() {
    let mut wrong = Vec::new();
    for operand in ["$?", "$#", "$$", "$!", "${?}", "${#}", "${$}", "${!}"] {
        for program in [
            "exit",
            "shift",
            "return",
            "break",
            "continue",
            "bye",
            "logout",
            "printf %d",
            "print -f %d --",
        ] {
            let context = format!("{program} \"{operand}\"");
            let words = super::super::super::command_argv(&context);
            if super::super::probe_arithmetic(&context, &words) {
                wrong.push(context);
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn continued_parameter_and_output_controls_keep_inputs_known() {
    for context in [
        ": \"$s\\\n[0]\"",
        "print -u 2 -f %s -- \"$x\"",
        "print -vout -f %s -- \"$x\"",
        "bash -c 'print -u 2 -f %d \"$x\"'",
        "printf %d 123",
    ] {
        let words = super::super::super::command_argv(context);
        assert!(
            !super::super::probe_arithmetic(context, &words),
            "{context}"
        );
    }
}

#[test]
fn literal_parameter_forms_keep_probe_inputs_known() {
    let mut wrong = Vec::new();
    for context in [
        ": \"${sha:0:7}\"",
        ": \"${sha:0}\"",
        ": \"${BASH_SOURCE[0]}\"",
        ": \"${PIPESTATUS[0]}\"",
        ": \"$a[0]\"",
        ": \"${#a[0]}\"",
        ": \"${p}\"",
        ": \"${12}\"",
        ": \"${?}${$}${-}${#}${!}${@}${*}\"",
        ": \"${#p}${#?}\"",
        ": \"${!prefix*}${!prefix@}${!name[@]}${!name[*]}\"",
        ": \"${p-word}${p=word}${p+word}${p?word}\"",
        ": \"${p:-word}${p:=word}${p:+word}${p:?word}\"",
        ": \"${p#word}${p##word}${p%word}${p%%word}\"",
        ": \"${p/word/data}${p//word/data}\"",
        ": \"${p^}${p^^}${p,}${p,,}\"",
        ": \"${a[@]}${a[*]}\"",
        "echo '${!x}${x@P}${(e)x}'",
        "cat <<'EOF'\n$\\\n((example))\nEOF\ngit fetch origin",
    ] {
        let words = super::super::super::command_argv(context);
        if super::super::probe_arithmetic(context, &words) {
            wrong.push(context);
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn arithmetic_data_keeps_probe_inputs_known() {
    for context in [
        "git fetch origin # ((example))",
        "cat <<'EOF'\n((example))\nEOF\ngit fetch origin",
        "echo '((example))'",
        "echo '${s:x}'",
        "echo '${a[x]}'",
        "printf -v result %s 'literal[x]'",
        "read -p 'literal[x]' result",
        "getopts 'literal[x]' result",
    ] {
        let words = super::super::super::command_argv(context);
        assert!(
            !super::super::probe_arithmetic(context, &words),
            "{context}"
        );
    }
}

#[test]
fn additional_export_options_keep_probe_inputs_unresolved() {
    for segment in [
        "shopt -o -s allexport",
        "setopt allexport",
        "setopt ALL_EXPORT",
        "set -o all_export",
        "unsetopt noallexport",
        "setopt No_No_ALL_EXPORT",
    ] {
        let literals = super::super::probe_line_literals(&[segment.into()]);
        for name in super::super::probe_names() {
            assert_eq!(literals.0.get(name), Some(&None), "{segment}: {name}");
        }
    }
}

#[test]
fn noncontacting_probe_mentions_need_no_probe() {
    let scratch = Scratch::new(HTTPS);
    for command in [
        "git commit -m 'GIT_INDEX_FILE GIT_OBJECT_DIRECTORY GIT_ALTERNATE_OBJECT_DIRECTORIES GIT_NO_REPLACE_OBJECTS GIT_REPLACE_REF_BASE'",
        "echo 'GIT_INDEX_FILE GIT_OBJECT_DIRECTORY GIT_ALTERNATE_OBJECT_DIRECTORIES GIT_NO_REPLACE_OBJECTS GIT_REPLACE_REF_BASE'; git status",
        "echo 'GIT_INDEX_FILE GIT_OBJECT_DIRECTORY GIT_ALTERNATE_OBJECT_DIRECTORIES GIT_NO_REPLACE_OBJECTS GIT_REPLACE_REF_BASE'; git fetch --no-recurse-submodules origin",
    ] {
        let mut bodies = Vec::new();
        let result = super::super::collect(
            command,
            scratch.root(),
            0,
            &super::super::Inputs::default(),
            &mut bodies,
        );
        assert!(result.is_ok(), "{command}: {result:?}");
    }
}

/// A scratch repository on branch `main` whose local configuration ends
/// with `config`.
struct Scratch {
    _dir: tempfile::TempDir,
    root: std::path::PathBuf,
}

/// The probe the runner reader uses, from the scratch repository's root.
struct RootProbe<'a>(&'a Path);

impl Probe for RootProbe<'_> {
    fn location(&mut self) -> Result<Location, String> {
        locate(self.0, &[], &Environment::new())
    }

    fn branch(&mut self) -> Result<String, String> {
        current_branch(self.0, &[], &Environment::new())
    }

    fn gitlinks(&mut self) -> Result<bool, String> {
        index_gitlinks(self.0, &[], &Environment::new())
    }
}

/// A probe that reads the repository as a call does from `cwd`, with the
/// call's global options and environment.
struct ScopedProbe<'a> {
    cwd: &'a Path,
    globals: Vec<String>,
    env: &'a Environment,
}

impl Probe for ScopedProbe<'_> {
    fn location(&mut self) -> Result<Location, String> {
        locate(self.cwd, &self.globals, self.env)
    }

    fn branch(&mut self) -> Result<String, String> {
        current_branch(self.cwd, &self.globals, self.env)
    }

    fn gitlinks(&mut self) -> Result<bool, String> {
        index_gitlinks(self.cwd, &self.globals, self.env)
    }
}

impl Scratch {
    fn new(config: &str) -> Self {
        Self::bytes(config.as_bytes())
    }

    /// The contacts of a command run from `cwd`, with `globals` as its
    /// global options and `extra` set in its environment.
    fn contacts_from(
        &self,
        cwd: &Path,
        globals: &[&str],
        extra: &[(&str, &str)],
        words: &[&str],
    ) -> Result<Vec<Contact>, String> {
        let mut env = Environment::new();
        env.insert("GIT_CONFIG_GLOBAL".into(), "/dev/null".into());
        env.insert("GIT_CONFIG_SYSTEM".into(), "/dev/null".into());
        env.insert("GIT_CONFIG_NOSYSTEM".into(), "1".into());
        env.insert("HOME".into(), self.root.display().to_string());
        // The configuration read never sees the index-file variable; only
        // the probe does.
        let config = read(cwd, &[], &env)?;
        for (name, value) in extra {
            env.insert((*name).into(), (*value).into());
        }
        let words: Vec<String> = words.iter().map(|word| (*word).to_string()).collect();
        let (verb, rest) = words.split_first().unwrap();
        let mut probe = ScopedProbe {
            cwd,
            globals: globals.iter().map(|word| (*word).to_string()).collect(),
            env: &env,
        };
        contacts(verb, rest, &config, &mut probe)
    }

    fn bytes(config: &[u8]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = crate::portable_path::canonicalize(dir.path())
            .unwrap()
            .join("repo");
        git2::Repository::init(&root).unwrap();
        std::fs::write(root.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        let file = root.join(".git/config");
        let mut bytes = std::fs::read(&file).unwrap();
        bytes.extend_from_slice(b"\n");
        bytes.extend_from_slice(config);
        std::fs::write(&file, bytes).unwrap();
        Self { _dir: dir, root }
    }

    /// A scratch bare repository whose configuration ends with `config`.
    fn bare(config: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = crate::portable_path::canonicalize(dir.path())
            .unwrap()
            .join("repo.git");
        git2::Repository::init_bare(&root).unwrap();
        let file = root.join("config");
        let mut bytes = std::fs::read(&file).unwrap();
        bytes.extend_from_slice(b"\n");
        bytes.extend_from_slice(config.as_bytes());
        std::fs::write(&file, bytes).unwrap();
        Self { _dir: dir, root }
    }

    /// The contacts of a command given word by word, so a word may be empty.
    fn contacts_of(&self, words: &[&str]) -> Result<Vec<Contact>, String> {
        self.contacts_reading(words, "/dev/null")
    }

    /// [`Scratch::contacts_of`] with `global` as the global configuration
    /// file the call reads.
    fn contacts_reading(&self, words: &[&str], global: &str) -> Result<Vec<Contact>, String> {
        self.contacts_in(words, global, &[])
    }

    /// [`Scratch::contacts_of`] with `settings` as the call's global
    /// options, which the configuration read sees and the probe does not.
    fn contacts_with(&self, words: &[&str], settings: &[&str]) -> Result<Vec<Contact>, String> {
        self.contacts_in(words, "/dev/null", settings)
    }

    fn contacts_in(
        &self,
        words: &[&str],
        global: &str,
        settings: &[&str],
    ) -> Result<Vec<Contact>, String> {
        let mut env = Environment::new();
        env.insert("GIT_CONFIG_GLOBAL".into(), global.into());
        env.insert("GIT_CONFIG_SYSTEM".into(), "/dev/null".into());
        env.insert("GIT_CONFIG_NOSYSTEM".into(), "1".into());
        env.insert("HOME".into(), self.root.display().to_string());
        let settings: Vec<String> = settings.iter().map(|word| (*word).to_string()).collect();
        let config = read(&self.root, &settings, &env)?;
        let words: Vec<String> = words.iter().map(|word| (*word).to_string()).collect();
        let (verb, rest) = words.split_first().unwrap();
        contacts(verb, rest, &config, &mut RootProbe(&self.root))
    }

    /// The helper programs `command` starts, or the refusal.
    fn helpers(&self, command: &str) -> Result<Vec<String>, String> {
        let words: Vec<&str> = command.split_whitespace().collect();
        self.helpers_of(&words)
    }

    /// [`Scratch::helpers`] for a command given word by word.
    fn helpers_of(&self, words: &[&str]) -> Result<Vec<String>, String> {
        Ok(helper_names(self.contacts_of(words)?))
    }

    /// [`Scratch::helpers`] with `global` written as the global
    /// configuration file the call reads.
    fn helpers_with_global(&self, command: &str, global: &str) -> Result<Vec<String>, String> {
        let file = self.root.with_file_name("global-config");
        std::fs::write(&file, global).unwrap();
        let words: Vec<&str> = command.split_whitespace().collect();
        Ok(helper_names(
            self.contacts_reading(&words, &file.display().to_string())?,
        ))
    }

    /// Assert that `command` refuses as a form the guard cannot prove, with
    /// `part` in the reason.
    fn unproven(&self, command: &str, part: &str) {
        let error = self
            .helpers(command)
            .expect_err(&format!("`{command}` must refuse"));
        assert!(error.starts_with(UNPROVEN), "{command}: {error}");
        assert!(error.contains(part), "{command}: {error}");
    }

    fn root(&self) -> &Path {
        &self.root
    }
}

/// The helper names among `contacts`.
fn helper_names(contacts: Vec<Contact>) -> Vec<String> {
    contacts
        .into_iter()
        .filter_map(|contact| match contact.selected {
            Selected::Helper(name) => Some(name),
            Selected::Ext => Some("ext".into()),
            Selected::Stock => None,
        })
        .collect()
}

const HTTPS: &str = "[remote \"origin\"]\n\turl = https://example.test/project.git\n";

#[test]
fn addresses_select_the_transport_git_selects() {
    for (address, expected) in [
        ("https://example.test/r.git", Selected::Stock),
        ("http://example.test/r.git", Selected::Stock),
        ("ftps://example.test/r.git", Selected::Stock),
        ("ssh://example.test/r.git", Selected::Stock),
        ("git+ssh://example.test/r.git", Selected::Stock),
        ("ssh+git://example.test/r.git", Selected::Stock),
        ("git@example.test:org/r.git", Selected::Stock),
        ("example.test:r.git", Selected::Stock),
        ("git://example.test/r.git", Selected::Stock),
        ("file:///srv/r.git", Selected::Stock),
        ("/srv/r.git", Selected::Stock),
        ("../r.git", Selected::Stock),
        ("C:\\work\\r.git", Selected::Stock),
        ("https::https://example.test/r.git", Selected::Stock),
        (
            "fixture::https://example.test/r.git",
            Selected::Helper("fixture".into()),
        ),
        (
            "fix-ture.2+x::anything",
            Selected::Helper("fix-ture.2+x".into()),
        ),
        ("::anything", Selected::Helper(String::new())),
        (
            "fixture://example.test/r",
            Selected::Helper("fixture".into()),
        ),
        ("HTTPS://example.test/r", Selected::Helper("HTTPS".into())),
        ("ext::sh -c true", Selected::Ext),
    ] {
        assert_eq!(select(address), expected, "{address}");
    }
}

#[test]
fn ordinary_transports_start_no_program() {
    for address in [
        "https://example.test/project.git",
        "ssh://example.test/project.git",
        "git@example.test:project.git",
        "/srv/project.git",
        "file:///srv/project.git",
    ] {
        let scratch = Scratch::new(&format!("[remote \"origin\"]\n\turl = {address}\n"));
        for command in [
            "fetch",
            "pull",
            "fetch --prune",
            "fetch origin",
            "fetch -q origin main",
            "pull --rebase origin main",
            "push origin HEAD:refs/heads/task/x",
            "push -u origin HEAD",
            "ls-remote origin",
            "remote update origin",
            "maintenance run --task=gc",
            "status",
        ] {
            assert_eq!(scratch.helpers(command), Ok(vec![]), "{address} {command}");
        }
    }
}

#[test]
fn a_remote_resolves_to_the_program_git_starts() {
    let scratch = Scratch::new("[remote \"fixture\"]\n\turl = fixture::https://example.test/r\n");
    assert_eq!(scratch.helpers("fetch fixture"), Ok(vec!["fixture".into()]));
    assert_eq!(
        scratch.helpers("ls-remote fixture"),
        Ok(vec!["fixture".into()])
    );
    let scheme = Scratch::new("[remote \"fixture\"]\n\turl = fixture://example.test/r\n");
    assert_eq!(
        scheme.helpers("push fixture HEAD"),
        Ok(vec!["fixture".into()])
    );
    let plain = Scratch::new("");
    assert_eq!(
        plain.helpers("clone fixture::https://example.test/r copy"),
        Ok(vec!["fixture".into()])
    );
    assert_eq!(
        plain.helpers("fetch fixture://example.test/r main"),
        Ok(vec!["fixture".into()])
    );
    assert_eq!(
        plain.helpers("archive --remote=fixture::x HEAD"),
        Ok(vec!["fixture".into()])
    );
    assert_eq!(
        plain.helpers("fetch -- fixture::x"),
        Ok(vec!["fixture".into()])
    );
}

#[test]
fn a_program_chosen_by_remote_settings_refuses() {
    for setting in ["vcs = fixture", "vcs = https", "vcs ="] {
        let with_url = Scratch::new(&format!(
            "[remote \"fixture\"]\n\turl = https://example.test/r\n\t{setting}\n"
        ));
        with_url.unproven("fetch fixture", "helper through its configuration");
        with_url.unproven("push fixture HEAD", "helper through its configuration");
    }
    let without_url = Scratch::new("[remote \"fixture\"]\n\tvcs = fixture\n");
    without_url.unproven("fetch fixture", "remote `fixture`");
    let clone = Scratch::new("[remote \"other\"]\n\tvcs = fixture\n");
    clone.unproven("clone https://example.test/r copy", "a new copy would use");
}

#[test]
fn an_implicit_contact_reads_the_remote_git_picks() {
    let default = Scratch::new("[remote \"origin\"]\n\turl = fixture::x\n");
    assert_eq!(default.helpers("fetch"), Ok(vec!["fixture".into()]));
    assert_eq!(default.helpers("pull"), Ok(vec!["fixture".into()]));
    let tracked = Scratch::new(&format!(
        "{HTTPS}[remote \"other\"]\n\turl = fixture::x\n[branch \"main\"]\n\tremote = other\n"
    ));
    assert_eq!(tracked.helpers("fetch"), Ok(vec!["fixture".into()]));
    assert_eq!(tracked.helpers("fetch origin"), Ok(vec![]));
    let off = Scratch::new(&format!("{HTTPS}[fetch]\n\tall = false\n"));
    assert_eq!(off.helpers("fetch"), Ok(vec![]));
}

#[test]
fn an_implicit_contact_with_no_remote_contacts_nothing() {
    let none = Scratch::new("");
    for command in ["fetch", "pull -r", "push", "ls-remote -h"] {
        assert_eq!(none.helpers(command), Ok(vec![]), "{command}");
    }
    std::fs::create_dir(none.root().join(".git/remotes")).unwrap();
    std::fs::write(none.root().join(".git/remotes/origin"), "URL: fixture::x\n").unwrap();
    none.unproven("fetch", "legacy remote file");
    none.unproven("push", "legacy remote file");

    let named = Scratch::new("[branch \"main\"]\n\tremote = elsewhere\n");
    named.unproven("fetch", "remote `elsewhere` is not configured");
    named.unproven("push", "names no remote");
    let pushed = Scratch::new("[remote]\n\tpushDefault = elsewhere\n");
    pushed.unproven("push", "names no remote");
}

#[test]
fn an_implicit_contact_refuses_each_state_it_cannot_prove() {
    let ordinary = Scratch::new(HTTPS);
    ordinary.unproven("push", "names no remote");
    ordinary.unproven("ls-remote", "names no remote");
    ordinary.unproven("push --force-with-lease=main", "names no remote");

    let detached = Scratch::new(HTTPS);
    std::fs::write(
        detached.root().join(".git/HEAD"),
        "0123456789012345678901234567890123456789\n",
    )
    .unwrap();
    detached.unproven("fetch", "current branch");

    let missing = Scratch::new(&format!("{HTTPS}[branch \"main\"]\n\tremote = gone\n"));
    missing.unproven("fetch", "remote `gone` is not configured");

    let no_origin = Scratch::new("[remote \"other\"]\n\turl = https://example.test/r\n");
    no_origin.unproven("fetch", "remote `origin` is not configured");
    no_origin.unproven("pull", "remote `origin` is not configured");

    for value in ["true", "yes", "1"] {
        let all = Scratch::new(&format!("{HTTPS}[fetch]\n\tall = {value}\n"));
        all.unproven("fetch", "every configured remote");
    }
    let bare_key = Scratch::new(&format!("{HTTPS}[fetch]\n\tall\n"));
    bare_key.unproven("fetch", "every configured remote");

    let group = Scratch::new(&format!("{HTTPS}[remotes]\n\torigin = origin\n"));
    group.unproven("fetch", "remote group");
    group.unproven("fetch origin", "remote group");
    let folded = Scratch::new(&format!("{HTTPS}[remotes]\n\tOrigin = origin\n"));
    folded.unproven("fetch origin", "remote group");

    let twice = Scratch::new(&format!(
        "{HTTPS}[remote \"origin\"]\n\turl = https://example.test/r\n"
    ));
    twice.unproven("fetch", "more than one address");

    let unreadable = Scratch::bytes(
        b"[remote \"origin\"]\n\turl = https://example.test/r\n[branch \"main\"]\n\tremote = or\xffigin\n",
    );
    let error = unreadable.helpers("fetch").unwrap_err();
    assert!(error.contains("not UTF-8"), "{error}");
    let unclear = Scratch::new(&format!("{HTTPS}[fetch]\n\tall = maybe\n"));
    unclear.unproven("fetch", "cannot be read");
}

#[test]
fn selections_of_several_remotes_refuse() {
    let scratch = Scratch::new(&format!("{HTTPS}[remote \"backup\"]\n\turl = fixture::x\n"));
    assert_eq!(scratch.helpers("fetch origin"), Ok(vec![]));
    assert_eq!(scratch.helpers("push origin HEAD"), Ok(vec![]));
    assert_eq!(scratch.helpers("fetch backup"), Ok(vec!["fixture".into()]));
    assert_eq!(
        scratch.helpers("fetch --multiple origin backup"),
        Ok(vec!["fixture".into()])
    );
    scratch.unproven("fetch --all", "every remote");
    scratch.unproven("fetch --al", "every remote");
    scratch.unproven("pull --all", "every remote");
    assert_eq!(scratch.helpers("remote update"), Ok(vec!["fixture".into()]));
    assert_eq!(
        scratch.helpers("remote update --prune"),
        Ok(vec!["fixture".into()])
    );
    scratch.unproven("maintenance run", "fetch ahead");
    scratch.unproven("maintenance run --task=prefetch", "fetch ahead");
    scratch.unproven("maintenance run --task gc --task prefetch", "fetch ahead");
    let group = Scratch::new(&format!(
        "{HTTPS}[remote \"backup\"]\n\turl = https://example.test/b\n[remotes]\n\tmirrors = origin backup\n"
    ));
    group.unproven("fetch mirrors", "remote group");
    group.unproven("remote update mirrors", "remote group");
}

#[test]
fn a_default_selection_reads_the_remotes_git_selects() {
    let backup = format!("{HTTPS}[remote \"backup\"]\n\turl = fixture::x\n");
    // Every configured remote, unless it skips the default update.
    let all = Scratch::new(&backup);
    assert_eq!(all.helpers("remote update"), Ok(vec!["fixture".into()]));
    let skipped = Scratch::new(&format!("{backup}\tskipDefaultUpdate = true\n"));
    assert_eq!(skipped.helpers("remote update"), Ok(vec![]));
    assert_eq!(
        skipped.helpers("remote update backup"),
        Ok(vec!["fixture".into()])
    );
    let fetch_all = Scratch::new(&format!("{backup}\tskipFetchAll = yes\n"));
    assert_eq!(fetch_all.helpers("remote update"), Ok(vec![]));
    let disagree = Scratch::new(&format!(
        "{backup}\tskipDefaultUpdate = true\n\tskipFetchAll = false\n"
    ));
    assert_eq!(
        disagree.helpers("remote update"),
        Ok(vec!["fixture".into()])
    );
    let unclear = Scratch::new(&format!("{backup}\tskipDefaultUpdate = maybe\n"));
    unclear.unproven("remote update", "cannot be read");
    // A set group is read as Git reads it.
    let default = Scratch::new(&format!("{backup}[remotes]\n\tdefault = origin\n"));
    assert_eq!(default.helpers("remote update"), Ok(vec![]));
    let both = Scratch::new(&format!("{backup}[remotes]\n\tdefault = origin backup\n"));
    assert_eq!(both.helpers("remote update"), Ok(vec!["fixture".into()]));
    let tabbed = Scratch::new(&format!(
        "{backup}[remotes]\n\tdefault = \"origin\\tbackup\"\n"
    ));
    assert_eq!(tabbed.helpers("remote update"), Ok(vec!["fixture".into()]));
    let nested = Scratch::new(&format!(
        "{backup}[remotes]\n\tdefault = origin mirrors\n\tmirrors = backup\n"
    ));
    nested.unproven("remote update", "remote group");
    // A definition the guard cannot rule out refuses.
    let legacy = Scratch::new(HTTPS);
    assert_eq!(legacy.helpers("remote update"), Ok(vec![]));
    std::fs::create_dir_all(legacy.root().join(".git/remotes")).unwrap();
    assert_eq!(legacy.helpers("remote update"), Ok(vec![]));
    std::fs::write(legacy.root().join(".git/remotes/old"), "URL: fixture::x\n").unwrap();
    legacy.unproven("remote update", "legacy remote file");
}

#[test]
fn an_empty_default_selection_refuses() {
    const FALLBACK: &str = "[remote \"default\"]\n\turl = fixture::x\n";
    for group in ["\"\"", "\" \\t \""] {
        let configured = Scratch::new(&format!(
            "{HTTPS}{FALLBACK}[remotes]\n\tdefault = {group}\n"
        ));
        configured.unproven("remote update", "empty default group");
        let legacy = Scratch::new(&format!("{HTTPS}[remotes]\n\tdefault = {group}\n"));
        std::fs::create_dir_all(legacy.root().join(".git/remotes")).unwrap();
        std::fs::write(
            legacy.root().join(".git/remotes/default"),
            "URL: fixture::x\n",
        )
        .unwrap();
        legacy.unproven("remote update", "empty default group");
    }
    // A group that lists a remote still reads its members.
    let listed = Scratch::new(&format!("{HTTPS}{FALLBACK}[remotes]\n\tdefault = origin\n"));
    assert_eq!(listed.helpers("remote update"), Ok(vec![]));
}

#[test]
fn an_unnamed_remote_definition_refuses() {
    let empty = Scratch::new(&format!("{HTTPS}[remote \"\"]\n\turl = fixture::x\n"));
    empty.unproven("remote update", "empty name");
    // A dotted name and names that differ only in case are each read.
    let dotted = Scratch::new(&format!("{HTTPS}[remote \"a.b\"]\n\turl = fixture::x\n"));
    assert_eq!(dotted.helpers("remote update"), Ok(vec!["fixture".into()]));
    let cased = Scratch::new(&format!(
        "{HTTPS}[remote \"Up\"]\n\turl = fixture::x\n[remote \"up\"]\n\turl = https://example.test/r\n\tskipDefaultUpdate = true\n"
    ));
    assert_eq!(cased.helpers("remote update"), Ok(vec!["fixture".into()]));
    // An unusable name refuses for a promisor remote too.
    let promisor = Scratch::new(&format!(
        "{HTTPS}[remote \"\"]\n\turl = https://example.test/r\n\tpromisor = true\n"
    ));
    promisor.unproven("status", "promisor remote with an empty name");
    let partial = Scratch::new(&format!("{HTTPS}[extensions]\n\tpartialClone = \"\"\n"));
    partial.unproven("status", "promisor remote with an empty name");
}

/// Replace the legacy remote folder `folder` of `scratch` with `form`.
#[cfg(unix)]
fn legacy_folder(scratch: &Scratch, folder: &str, form: &str) {
    let path = scratch.root().join(".git").join(folder);
    // A link or file goes with `remove_file`; a folder needs the other call.
    if std::fs::remove_file(&path).is_err() {
        let _ = std::fs::remove_dir_all(&path);
    }
    let target = scratch.root().join("empty-target");
    std::fs::create_dir_all(&target).unwrap();
    match form {
        "empty" => std::fs::create_dir(&path).unwrap(),
        "link to empty" => std::os::unix::fs::symlink(&target, &path).unwrap(),
        "dangling link" => std::os::unix::fs::symlink(target.join("gone"), &path).unwrap(),
        "file" => std::fs::write(&path, "").unwrap(),
        "populated" => {
            std::fs::create_dir(&path).unwrap();
            std::fs::write(path.join("old"), "URL: fixture::x\n").unwrap();
        }
        _ => unreachable!("{form}"),
    }
}

#[cfg(unix)]
#[test]
fn an_older_definition_store_passes_only_when_proven_empty() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new(HTTPS);
    for folder in ["remotes", "branches"] {
        legacy_folder(&scratch, folder, "empty");
        assert_eq!(scratch.helpers("remote update"), Ok(vec![]), "{folder}");
        for form in ["link to empty", "dangling link", "file", "populated"] {
            legacy_folder(&scratch, folder, form);
            scratch.unproven("remote update", "legacy remote file");
        }
        // A real folder that cannot be read; skipped where the reader can
        // read it anyway, as a superuser can.
        legacy_folder(&scratch, folder, "empty");
        let path = scratch.root().join(".git").join(folder);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::read_dir(&path).is_err() {
            scratch.unproven("remote update", "legacy remote file");
        }
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        legacy_folder(&scratch, folder, "empty");
    }
}

#[test]
fn a_remote_with_ambiguous_addresses_refuses() {
    for remote in [
        "\turl = https://example.test/r\n\turl = fixture::x\n",
        "\turl = fixture::x\n\turl =\n\turl = https://example.test/r\n",
        "\turl =\n",
        "\tfetch = +refs/heads/*:refs/remotes/origin/*\n",
    ] {
        let scratch = Scratch::new(&format!("[remote \"origin\"]\n{remote}"));
        assert!(
            scratch
                .helpers("fetch origin")
                .unwrap_err()
                .starts_with(UNPROVEN),
            "{remote}"
        );
    }
    for pushurls in [
        "\tpushurl = https://example.test/r\n\tpushurl = fixture::x\n",
        "\tpushurl = fixture::x\n\tpushurl =\n",
        "\tpushurl =\n",
    ] {
        let scratch = Scratch::new(&format!(
            "[remote \"origin\"]\n\turl = https://example.test/r\n{pushurls}"
        ));
        scratch.unproven("push origin HEAD", "push address");
        scratch.unproven("fetch origin", "push address");
    }
}

#[test]
fn an_address_change_for_both_directions_is_judged() {
    let scratch = Scratch::new(&format!(
        "{HTTPS}[url \"fixture::https://example.test/\"]\n\tinsteadOf = https://example.test/\n"
    ));
    assert_eq!(scratch.helpers("fetch origin"), Ok(vec!["fixture".into()]));
    assert_eq!(
        scratch.helpers("push origin HEAD"),
        Ok(vec!["fixture".into()])
    );
    let short = Scratch::new("[url \"fixture::https://example.test/\"]\n\tinsteadOf = fx:\n");
    assert_eq!(short.helpers("clone fx:r copy"), Ok(vec!["fixture".into()]));
}

#[test]
fn an_address_change_for_one_direction_is_judged_there() {
    let scratch = Scratch::new(&format!(
        "{HTTPS}[url \"fixture::https://example.test/\"]\n\tpushInsteadOf = https://example.test/\n"
    ));
    assert_eq!(
        scratch.helpers("push origin HEAD"),
        Ok(vec!["fixture".into()])
    );
    assert_eq!(scratch.helpers("fetch origin"), Ok(vec![]));
}

#[test]
fn address_changes_apply_in_git_order() {
    // Each direction reads its own rewrite.
    let scratch = Scratch::new(&format!(
        "{HTTPS}[url \"fixture::https://example.test/\"]\n\tpushInsteadOf = https://example.test/\n[url \"https://mirror.test/\"]\n\tinsteadOf = https://example.test/\n"
    ));
    assert_eq!(
        scratch.helpers("push origin HEAD"),
        Ok(vec!["fixture".into()])
    );
    assert_eq!(scratch.helpers("fetch origin"), Ok(vec![]));
    let reversed = Scratch::new(&format!(
        "{HTTPS}[url \"https://mirror.test/\"]\n\tpushInsteadOf = https://example.test/\n[url \"fixture::https://example.test/\"]\n\tinsteadOf = https://example.test/\n"
    ));
    assert_eq!(reversed.helpers("push origin HEAD"), Ok(vec![]));
    assert_eq!(reversed.helpers("fetch origin"), Ok(vec!["fixture".into()]));
    // A push address reads the ordinary rewrite.
    let pushurl = Scratch::new(
        "[remote \"origin\"]\n\turl = https://example.test/r\n\tpushurl = https://push.test/r\n[url \"fixture::https://push.test/\"]\n\tinsteadOf = https://push.test/\n",
    );
    assert_eq!(
        pushurl.helpers("push origin HEAD"),
        Ok(vec!["fixture".into()])
    );
    assert_eq!(pushurl.helpers("fetch origin"), Ok(vec![]));
}

#[test]
fn an_unresolved_selection_refuses_naming_the_remote() {
    let bytes = Scratch::bytes(b"[remote \"fixture\"]\n\turl = https://example.test/\xff\n");
    let error = bytes.helpers("fetch fixture").unwrap_err();
    assert!(error.contains("remote `fixture`"), "{error}");
    for folder in ["remotes", "branches"] {
        let legacy = Scratch::new(HTTPS);
        std::fs::create_dir_all(legacy.root().join(".git").join(folder)).unwrap();
        std::fs::write(
            legacy.root().join(".git").join(folder).join("old"),
            "fixture::x\n",
        )
        .unwrap();
        legacy.unproven("fetch old", "legacy remote file");
        legacy.unproven("ls-remote old", "legacy remote file");
        assert_eq!(legacy.helpers("fetch origin"), Ok(vec![]));
    }
}

#[test]
fn nested_contacts_refuse_unless_the_line_turns_them_off() {
    let configured = Scratch::new(&format!("{HTTPS}[submodule \"lib\"]\n\turl = fixture::x\n"));
    let active = Scratch::new(&format!("{HTTPS}[submodule]\n\tactive = .\n"));
    let listed = Scratch::new(HTTPS);
    std::fs::write(listed.root().join(".gitmodules"), "").unwrap();
    let cloned = Scratch::new(HTTPS);
    std::fs::create_dir_all(cloned.root().join(".git/modules/lib")).unwrap();
    for scratch in [&configured, &active, &listed, &cloned] {
        scratch.unproven("fetch origin", "nested repositories");
        scratch.unproven("pull origin main", "nested repositories");
        scratch.unproven(
            "fetch origin --recurse-submodules=no --recurse-submodules",
            "nested repositories",
        );
        scratch.unproven("remote update origin", "nested repositories");
        scratch.unproven("submodule update", "nested repositories");
        scratch.unproven(
            "submodule --quiet add https://example.test/lib lib",
            "nested repositories",
        );
        assert_eq!(
            scratch.helpers("fetch --no-recurse-submodules origin"),
            Ok(vec![])
        );
        assert_eq!(
            scratch.helpers("pull --recurse-submodules=no origin main"),
            Ok(vec![])
        );
        assert_eq!(scratch.helpers("push origin HEAD"), Ok(vec![]));
        assert_eq!(scratch.helpers("log"), Ok(vec![]));
    }
    for setting in [
        "[push]\n\trecurseSubmodules = on-demand\n",
        "[push]\n\trecurseSubmodules = check\n",
        "[submodule]\n\trecurse = true\n",
    ] {
        let scratch = Scratch::new(&format!("{HTTPS}{setting}"));
        std::fs::write(scratch.root().join(".gitmodules"), "").unwrap();
        scratch.unproven("push origin HEAD", "nested repositories");
        assert_eq!(
            scratch.helpers("push --no-recurse-submodules origin HEAD"),
            Ok(vec![])
        );
    }
    let plain = Scratch::new("");
    plain.unproven(
        "clone --recurse-submodules https://example.test/r copy",
        "new copy",
    );
    plain.unproven("clone --recursive https://example.test/r copy", "new copy");
    plain.unproven("clone --recurse https://example.test/r copy", "new copy");
    assert_eq!(
        plain.helpers("clone https://example.test/r copy"),
        Ok(vec![])
    );
    let recursing = Scratch::new("[submodule]\n\trecurse = true\n");
    recursing.unproven("clone https://example.test/r copy", "new copy");
}

#[test]
fn remotes_any_command_may_contact_pass_the_same_rule() {
    let promisor = Scratch::new(&format!(
        "{HTTPS}[remote \"lazy\"]\n\turl = fixture::x\n\tpromisor = true\n"
    ));
    assert_eq!(promisor.helpers("status"), Ok(vec!["fixture".into()]));
    assert_eq!(promisor.helpers("log"), Ok(vec!["fixture".into()]));
    let partial = Scratch::new(&format!(
        "{HTTPS}[remote \"lazy\"]\n\turl = fixture::x\n[extensions]\n\tpartialClone = lazy\n"
    ));
    assert_eq!(partial.helpers("show HEAD"), Ok(vec!["fixture".into()]));
    let set = Scratch::new(&format!(
        "{HTTPS}[remote \"lazy\"]\n\turl = https://example.test/r\n\tvcs = fixture\n\tpromisor = true\n"
    ));
    set.unproven("status", "helper through its configuration");
    let missing = Scratch::new(&format!("{HTTPS}[extensions]\n\tpartialClone = gone\n"));
    missing.unproven("status", "remote `gone` is not configured");
}

#[test]
fn every_registration_of_such_remotes_is_judged() {
    const SHADOW: &str = "[remote \"shadow\"]\n\turl = fixture::x\n";
    // Each way of registering an implicit remote counts.
    for extra in [
        "\tpartialCloneFilter = blob:none\n",
        "\tpromisor = false\n\tpartialCloneFilter = blob:none\n",
        "\tpromisor = true\n\tpromisor = false\n",
    ] {
        let scratch = Scratch::new(&format!("{HTTPS}{SHADOW}{extra}"));
        assert_eq!(
            scratch.helpers("status"),
            Ok(vec!["fixture".into()]),
            "{extra}"
        );
    }
    // A remote with none of them is no promisor.
    let plain = Scratch::new(&format!("{HTTPS}{SHADOW}"));
    assert_eq!(plain.helpers("status"), Ok(vec![]));
    let unset = Scratch::new(&format!("{HTTPS}{SHADOW}\tpromisor = false\n"));
    assert_eq!(unset.helpers("status"), Ok(vec![]));
    // A name Git reads as empty, in either representation of it, refuses.
    for config in [
        "[remote \"\"]\n\tpartialCloneFilter = blob:none\n",
        "[remote]\n\tpartialCloneFilter = blob:none\n",
        "[remote]\n\tpromisor = true\n",
    ] {
        let scratch = Scratch::new(&format!("{HTTPS}{config}"));
        scratch.unproven("status", "promisor remote with an empty name");
    }
}

#[test]
fn an_empty_word_reads_as_git_reads_it() {
    // An empty operand is judged as Git reads it.
    const REWRITES: &str = "[url \"https://ok.test/\"]\n\tinsteadOf = https://\n[url \"fixture::x\"]\n\tinsteadOf = \"\"\n";
    let scratch = Scratch::new(&format!("{HTTPS}{REWRITES}"));
    assert_eq!(
        scratch.helpers_of(&["fetch", ""]),
        Ok(vec!["fixture".into()])
    );
    // Beside a remote definition with an empty name, it refuses.
    let section = Scratch::new(&format!(
        "{HTTPS}[remote \"\"]\n\turl = https://example.test/r\n{REWRITES}"
    ));
    let error = section.helpers_of(&["fetch", ""]).unwrap_err();
    assert!(
        error.starts_with(UNPROVEN) && error.contains("`remote..url`"),
        "{error}"
    );
}

#[test]
fn an_unnamed_remote_definition_refuses_every_contact() {
    // A definition the guard cannot resolve refuses every contact that reads
    // the configuration's remotes, naming the key.
    const CONTACTS: &[&str] = &[
        "fetch origin",
        "ls-remote origin",
        "fetch https://example.test/r",
        "ls-remote https://example.test/r",
        "remote update origin",
        "clone https://example.test/r copy",
        "remote add -f fresh https://example.test/r",
    ];
    const SECTION: &str = "[remote \"\"]\n\turl = fixture::x\n";
    for (key, line, selecting) in [
        (
            "remote..url",
            "\turl = fixture::x\n",
            &["ls-remote .url", "fetch .url", "archive --remote=.url HEAD"][..],
        ),
        (
            "remote..pushurl",
            "\tpushurl = fixture::x\n",
            &["push .pushurl HEAD:refs/heads/task/x"][..],
        ),
        ("remote..vcs", "\tvcs = fixture\n", &["ls-remote .vcs"][..]),
    ] {
        let scratch = Scratch::new(&format!("{HTTPS}[remote \"\"]\n{line}"));
        for command in selecting.iter().chain(CONTACTS) {
            scratch.unproven(command, &format!("`{key}`"));
        }
        // A command that contacts no remote is not judged by it.
        assert_eq!(scratch.helpers("status"), Ok(vec![]), "{key}");
    }
    // A remote the configuration selects by the same name refuses too.
    for (config, command) in [
        ("[branch \"main\"]\n\tremote = .url\n", "fetch"),
        ("[extensions]\n\tpartialClone = .url\n", "status"),
        ("[remotes]\n\tdefault = .url\n", "remote update"),
        ("", "remote update"),
    ] {
        let scratch = Scratch::new(&format!("{HTTPS}{SECTION}{config}"));
        scratch.unproven(command, "`remote..url`");
    }
    // A section whose name starts with a dot is an ordinary remote.
    let dotted = Scratch::new(&format!("{HTTPS}[remote \".a\"]\n\turl = fixture::x\n"));
    assert_eq!(dotted.helpers("ls-remote .a"), Ok(vec!["fixture".into()]));
    assert_eq!(dotted.helpers("fetch origin"), Ok(vec![]));
}

#[test]
fn every_spelling_that_turns_nested_contacts_on_reads_as_on() {
    let plain = Scratch::new("");
    for command in [
        "clone --recurse-submodules=no https://example.test/r copy",
        "clone --recurse-submodules=false https://example.test/r copy",
        "clone --recurs https://example.test/r copy",
        "clone --recursive=lib https://example.test/r copy",
        "clone --no-recurse-submodules --recurse-submodules=no https://example.test/r copy",
    ] {
        plain.unproven(command, "copy without recursion");
    }
    for command in [
        "clone -c submodule.recurse=true https://example.test/r copy",
        "clone -qc submodule.active=. https://example.test/r copy",
        "clone --config=Submodule.Recurse=1 https://example.test/r copy",
        "clone --con submodule.recurse=1 https://example.test/r copy",
        "clone -c",
    ] {
        plain.unproven(command, "setting for nested repositories");
    }
    assert_eq!(
        plain.helpers("clone --no-recurse-submodules https://example.test/r copy"),
        Ok(vec![])
    );
    assert_eq!(
        plain.helpers("clone -c core.autocrlf=false https://example.test/r copy"),
        Ok(vec![])
    );

    let listed = Scratch::new(HTTPS);
    std::fs::write(listed.root().join(".gitmodules"), "").unwrap();
    for command in [
        "push --no-recurse-submodules --recu=on-demand origin HEAD:refs/heads/task/x",
        "push --no-recurse-submodules --r=check origin HEAD",
        "push --recurse-submodules=only origin HEAD",
        "push --no-recurse-submodules --no-recurse origin HEAD",
        "fetch --no-recurse-submodules --recurse=no origin",
    ] {
        listed.unproven(command, "nested repositories");
    }
    for order in [
        "[push]\n\trecurseSubmodules = no\n[submodule]\n\trecurse = true\n",
        "[submodule]\n\trecurse = true\n[push]\n\trecurseSubmodules = no\n",
    ] {
        let scratch = Scratch::new(&format!("{HTTPS}{order}"));
        std::fs::write(scratch.root().join(".gitmodules"), "").unwrap();
        scratch.unproven("push origin HEAD", "nested repositories");
    }
    let off = Scratch::new(&format!("{HTTPS}[push]\n\trecurseSubmodules = no\n"));
    std::fs::write(off.root().join(".gitmodules"), "").unwrap();
    assert_eq!(off.helpers("push origin HEAD"), Ok(vec![]));
}

#[test]
fn indirect_contact_paths_refuse_unless_proven() {
    let plain = Scratch::new(HTTPS);
    for command in [
        "submodule--helper update --init",
        "submodule--helper add https://example.test/lib lib",
        "submodule--helper clone --path lib --url https://example.test/lib",
        "submodule--helper --quiet update",
        "submodule--helper",
    ] {
        plain.unproven(command, "internal builtin for nested repositories");
    }
    for command in [
        "submodule--helper foreach --quiet true",
        "submodule--helper status",
    ] {
        assert_eq!(plain.helpers(command), Ok(vec![]), "{command}");
    }

    let fresh = "remote add -f fresh https://example.test/r";
    assert_eq!(plain.helpers(fresh), Ok(vec![]));
    assert_eq!(
        plain.helpers("remote add --tags --fetch fresh https://example.test/r"),
        Ok(vec![])
    );
    plain.unproven(
        "remote add -f -t main fresh https://example.test/r",
        "fetches with settings",
    );
    plain.unproven(
        "remote add -vf fresh https://example.test/r",
        "fetches with settings",
    );
    plain.unproven("remote add -f fresh", "exactly one name and one address");

    let helper = Scratch::new(&format!("{HTTPS}[remote \"fresh\"]\n\tvcs = fixture\n"));
    for command in [
        fresh,
        "remote add --fe fresh https://example.test/r",
        "remote add --no-fetch fresh https://example.test/r",
    ] {
        helper.unproven(command, "already holds settings for remote `fresh`");
    }
    assert_eq!(
        helper.helpers("remote add fresh https://example.test/r"),
        Ok(vec![])
    );
    let group = Scratch::new(&format!("{HTTPS}[remotes]\n\tfresh = origin\n"));
    group.unproven(fresh, "already holds settings");
    let legacy = Scratch::new(HTTPS);
    std::fs::create_dir_all(legacy.root().join(".git/remotes")).unwrap();
    std::fs::write(
        legacy.root().join(".git/remotes/fresh"),
        "URL: fixture::x\n",
    )
    .unwrap();
    legacy.unproven(fresh, "legacy remote file");
}

/// A word an option reading consumes as a value proves nothing, and the
/// reading goes on past it.
#[test]
fn a_word_an_option_consumes_proves_nothing() {
    let listed = Scratch::new(HTTPS);
    std::fs::write(listed.root().join(".gitmodules"), "").unwrap();
    for command in [
        "fetch --recurse-submodules=on-demand --server-option --no-recurse-submodules origin",
        "fetch -o --no-recurse-submodules origin",
        "fetch --server-option --recurse-submodules=no origin",
        "pull origin main --strategy --no-recurse-submodules",
        "push --recurse-submodules --no-recurse-submodules origin HEAD",
    ] {
        listed.unproven(command, "nested repositories");
    }
    for command in [
        "fetch -q --no-recurse-submodules origin",
        "fetch --depth=1 --no-recurse-submodules origin",
        "fetch --recurse-submodules --no-recurse-submodules origin",
        "pull origin main --no-recurse-submodules",
    ] {
        assert_eq!(listed.helpers(command), Ok(vec![]), "{command}");
    }

    let plain = Scratch::new("");
    for command in [
        "clone --recurse-submodules --server-option --no-recurse-submodules https://example.test/r copy",
        "clone --server-option -- --recurse-submodules https://example.test/r copy",
        "clone --recursive -u --no-recurse-submodules https://example.test/r copy",
    ] {
        plain.unproven(command, "copy without recursion");
    }
    plain.unproven(
        "clone --server-option -- -c submodule.recurse=true https://example.test/r copy",
        "setting for nested repositories",
    );
    assert_eq!(
        plain.helpers("clone -q --no-recurse-submodules https://example.test/r copy"),
        Ok(vec![])
    );

    let origin = Scratch::new(HTTPS);
    for command in [
        "push -o -- --dry-run",
        "push --recurse-submodules no",
        "ls-remote -u x",
    ] {
        origin.unproven(command, "names no remote");
    }
    assert_eq!(origin.helpers("push origin -- HEAD"), Ok(vec![]));

    let helper = Scratch::new(&format!("{HTTPS}[remote \"fresh\"]\n\tvcs = fixture\n"));
    for scratch in [&origin, &helper] {
        scratch.unproven(
            "remote add -t -- -f fresh https://example.test/r",
            "fetches with settings",
        );
    }
}

/// Every verb the contacting-verb inventory lists either resolves its
/// contact here or refuses by name.
#[test]
fn a_listed_contacting_verb_without_a_resolution_refuses() {
    let plain = Scratch::new(HTTPS);
    plain.unproven("request-pull HEAD fixture::x HEAD", "`request-pull`");
    plain.unproven(
        "request-pull HEAD https://example.test/r HEAD",
        "`request-pull`",
    );
    assert_eq!(plain.helpers("credential fill"), Ok(vec![]));
    assert_eq!(plain.helpers("ls-remote origin"), Ok(vec![]));
}

#[test]
fn a_batch_task_passes_only_when_proven_local() {
    let scratch = Scratch::new(&format!("{HTTPS}[remote \"backup\"]\n\turl = fixture::x\n"));
    for command in [
        "maintenance run --task=PREFETCH",
        "maintenance run --task=Prefetch",
        "maintenance run --task=gc --ta=prefetch",
        "maintenance run --task=gc --t prefetch",
        "maintenance run --task=gc --auto",
        "maintenance run --task=gc --schedule=hourly",
        "maintenance run --task=gc -x",
        "maintenance run --task=gc extra",
        "maintenance run --task",
        "maintenance run --quiet",
        "maintenance --quiet run --task=gc",
    ] {
        scratch.unproven(command, "fetch ahead");
    }
    for command in [
        "maintenance run --task=GC",
        "maintenance run --ta=gc --task commit-graph",
        "maintenance run --task=gc --quiet --no-detach",
        "maintenance run -q --task=pack-refs",
    ] {
        assert_eq!(scratch.helpers(command), Ok(vec![]), "{command}");
    }
}

#[cfg(unix)]
#[test]
fn an_older_definition_name_is_checked_as_git_reads_it() {
    let scratch = Scratch::new(HTTPS);
    std::fs::create_dir_all(scratch.root().join(".git/remotes")).unwrap();
    std::fs::write(
        scratch.root().join(".git/remotes/old\\name"),
        "URL: fixture::x\n",
    )
    .unwrap();
    scratch.unproven("ls-remote old\\name", "legacy remote file");
    scratch.unproven("fetch old\\name", "legacy remote file");
}

/// A last word that stands for a wider selection is read as Git reads it.
#[test]
fn a_selection_word_reads_as_git_reads_it() {
    let backup = format!("{HTTPS}[remote \"backup\"]\n\turl = fixture::x\n");
    let scratch = Scratch::new(&backup);
    for command in [
        "remote update default",
        "remote update -p default",
        "remote update origin default",
    ] {
        assert_eq!(
            scratch.helpers(command),
            Ok(vec!["fixture".into()]),
            "{command}"
        );
    }
    // The wider selection skips what the default update skips.
    let skipped = Scratch::new(&format!("{backup}\tskipDefaultUpdate = true\n"));
    assert_eq!(skipped.helpers("remote update default"), Ok(vec![]));
    // A group of that name keeps the group reading.
    let grouped = Scratch::new(&format!("{backup}[remotes]\n\tdefault = origin\n"));
    grouped.unproven("remote update default", "remote group");
    let ordinary = Scratch::new(HTTPS);
    assert_eq!(ordinary.helpers("remote update default"), Ok(vec![]));
}

/// A word Git's option parser keeps as an operand names the remote,
/// wherever the remote's definition comes from.
#[test]
fn every_word_git_keeps_as_an_operand_names_the_remote() {
    let scratch = Scratch::new(&format!("{HTTPS}[remote \"-\"]\n\turl = fixture::x\n"));
    for command in [
        "fetch -",
        "fetch -q -",
        "pull - main",
        "ls-remote -",
        "push - HEAD",
        "remote show -",
        "remote update -",
    ] {
        assert_eq!(
            scratch.helpers(command),
            Ok(vec!["fixture".into()]),
            "{command}"
        );
    }
    assert_eq!(scratch.helpers("fetch origin"), Ok(vec![]));
    let legacy = Scratch::new(HTTPS);
    std::fs::create_dir_all(legacy.root().join(".git/remotes")).unwrap();
    std::fs::write(legacy.root().join(".git/remotes/-"), "URL: fixture::x\n").unwrap();
    legacy.unproven("fetch -", "legacy remote file");
    legacy.unproven("ls-remote -", "legacy remote file");
    let rewritten = Scratch::new(&format!("{HTTPS}[url \"fixture::x\"]\n\tinsteadOf = -\n"));
    assert_eq!(rewritten.helpers("fetch -"), Ok(vec!["fixture".into()]));
}

/// A new copy is judged by every address its remote may have, from any
/// configuration scope.
#[test]
fn a_copy_judges_every_address_it_may_use() {
    for name in ["origin", "upstream"] {
        let scratch = Scratch::new("");
        let global = format!("[remote \"{name}\"]\n\turl = fixture::x\n");
        for command in [
            "clone https://example.test/r copy",
            "clone -o upstream https://example.test/r copy",
            "clone --no-recurse-submodules https://example.test/r copy",
        ] {
            assert_eq!(
                scratch.helpers_with_global(command, &global),
                Ok(vec!["fixture".into()]),
                "{name} {command}"
            );
        }
    }
    let rewritten = Scratch::new("");
    assert_eq!(
        rewritten.helpers_with_global(
            "clone https://example.test/r copy",
            "[remote \"origin\"]\n\turl = https://example.test/o\n[url \"fixture::x\"]\n\tinsteadOf = https://example.test/o\n",
        ),
        Ok(vec!["fixture".into()])
    );
    let ordinary = Scratch::new("");
    assert_eq!(
        ordinary.helpers_with_global(
            "clone https://example.test/r copy",
            "[remote \"origin\"]\n\turl = https://example.test/o\n",
        ),
        Ok(vec![])
    );
    assert_eq!(
        ordinary.helpers("clone https://example.test/r copy"),
        Ok(vec![])
    );
}

/// Build a nested-repository fixture at `path` under `scratch`.
fn nested_in_index_only(scratch: &Scratch, path: &str) {
    let nested = scratch.root().join(path);
    git2::Repository::init(&nested).unwrap();
    let config = nested.join(".git/config");
    let mut bytes = std::fs::read(&config).unwrap();
    bytes.extend_from_slice(b"\n[remote \"origin\"]\n\turl = fixture::x\n");
    std::fs::write(&config, bytes).unwrap();
    let repo = git2::Repository::open(scratch.root()).unwrap();
    let mut index = repo.index().unwrap();
    index
        .add(&git2::IndexEntry {
            ctime: git2::IndexTime::new(0, 0),
            mtime: git2::IndexTime::new(0, 0),
            dev: 0,
            ino: 0,
            mode: 0o160_000,
            uid: 0,
            gid: 0,
            file_size: 0,
            id: git2::Oid::from_str("0123456789abcdef0123456789abcdef01234567").unwrap(),
            flags: u16::try_from(path.len()).unwrap(),
            flags_extended: 0,
            path: path.as_bytes().to_vec(),
        })
        .unwrap();
    index.write().unwrap();
}

/// A nested repository the guard cannot rule out refuses unless the line
/// turns recursion off.
#[test]
fn nested_contacts_with_one_marker_refuse() {
    let scratch = Scratch::new(HTTPS);
    nested_in_index_only(&scratch, "lib");
    for command in [
        "fetch origin",
        "fetch --recurse-submodules origin",
        "fetch",
        "pull origin main",
        "remote update origin",
        "remote update",
    ] {
        scratch.unproven(command, "nested repositories");
    }
    assert_eq!(
        scratch.helpers("fetch --no-recurse-submodules origin"),
        Ok(vec![])
    );
    assert_eq!(scratch.helpers("push origin HEAD"), Ok(vec![]));
    // A record the guard cannot confirm refuses.
    let unpopulated = Scratch::new(HTTPS);
    nested_in_index_only(&unpopulated, "lib");
    std::fs::remove_dir_all(unpopulated.root().join("lib")).unwrap();
    unpopulated.unproven("fetch origin", "nested repositories");
}

/// Hold a definition file in the index only, or (with `in_commit`) in the
/// last commit only, and leave the work tree without it.
fn definition_outside_work_tree(scratch: &Scratch, in_commit: bool) {
    let repo = git2::Repository::open(scratch.root()).unwrap();
    std::fs::write(scratch.root().join(".gitmodules"), "").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new(".gitmodules")).unwrap();
    index.write().unwrap();
    if in_commit {
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let who = git2::Signature::now("fixture", "fixture@example.test").unwrap();
        repo.commit(Some("HEAD"), &who, &who, "fixture", &tree, &[])
            .unwrap();
        index.remove_path(Path::new(".gitmodules")).unwrap();
        index.write().unwrap();
    }
    std::fs::remove_file(scratch.root().join(".gitmodules")).unwrap();
}

/// A definition the work tree lacks but the index or the last commit holds
/// is not proof of no nested repositories, so recursion refuses unless the
/// line turns it off; with no definition anywhere, it passes.
#[test]
fn a_definition_held_outside_the_work_tree_refuses() {
    for in_commit in [false, true] {
        let scratch = Scratch::new(HTTPS);
        definition_outside_work_tree(&scratch, in_commit);
        for command in ["fetch origin", "fetch", "pull origin main", "remote update"] {
            scratch.unproven(command, "nested repositories");
        }
        assert_eq!(
            scratch.helpers("fetch --no-recurse-submodules origin"),
            Ok(vec![])
        );
        assert_eq!(scratch.helpers("push origin HEAD"), Ok(vec![]));
    }
    // Controls: a commit without a definition, and no commit at all.
    let committed = Scratch::new(HTTPS);
    let repo = git2::Repository::open(committed.root()).unwrap();
    let tree = repo
        .find_tree(repo.index().unwrap().write_tree().unwrap())
        .unwrap();
    let who = git2::Signature::now("fixture", "fixture@example.test").unwrap();
    repo.commit(Some("HEAD"), &who, &who, "fixture", &tree, &[])
        .unwrap();
    assert_eq!(committed.helpers("fetch origin"), Ok(vec![]));
    assert_eq!(Scratch::new(HTTPS).helpers("fetch origin"), Ok(vec![]));
}

/// The refusal for nested repositories the guard cannot rule out, from a
/// command judged in `cwd` with `globals`.
fn nested_refusal(scratch: &Scratch, cwd: &Path, globals: &[&str], line: &[&str]) -> String {
    match scratch.contacts_from(cwd, globals, &[], line) {
        Err(error) => {
            assert!(error.starts_with(UNPROVEN), "{error}");
            error
        }
        Ok(_) => panic!("{globals:?} {line:?} in {} must refuse", cwd.display()),
    }
}

/// The definition check reads the top of the repository, whichever directory
/// or directory option the command is judged with.
#[test]
fn a_definition_outside_the_work_tree_refuses_from_any_directory() {
    for in_commit in [false, true] {
        let scratch = Scratch::new(HTTPS);
        definition_outside_work_tree(&scratch, in_commit);
        let child = scratch.root().join("sub");
        std::fs::create_dir(&child).unwrap();
        for (cwd, globals) in [
            (child.as_path(), &[][..]),
            (scratch.root(), &["-C", "sub"][..]),
        ] {
            let error = nested_refusal(&scratch, cwd, globals, &["fetch", "origin"]);
            assert!(error.contains("nested repositories"), "{error}");
            let off = ["fetch", "--no-recurse-submodules", "origin"];
            assert!(
                scratch.contacts_from(cwd, globals, &[], &off).is_ok(),
                "{globals:?}"
            );
        }
    }
    // Control: a definition below the top only is no definition at the top.
    let scratch = Scratch::new(HTTPS);
    let repo = git2::Repository::open(scratch.root()).unwrap();
    std::fs::create_dir(scratch.root().join("sub")).unwrap();
    std::fs::write(scratch.root().join("sub/.gitmodules"), "").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new("sub/.gitmodules")).unwrap();
    index.write().unwrap();
    for cwd in [scratch.root().to_path_buf(), scratch.root().join("sub")] {
        assert!(
            scratch
                .contacts_from(&cwd, &[], &[], &["fetch", "origin"])
                .is_ok(),
            "{}",
            cwd.display()
        );
    }
}

/// Commit an empty tree on the branch `HEAD` names and return the commit.
fn commit_empty(scratch: &Scratch) -> git2::Oid {
    let repo = git2::Repository::open(scratch.root()).unwrap();
    let tree = repo
        .find_tree(repo.treebuilder(None).unwrap().write().unwrap())
        .unwrap();
    let who = git2::Signature::now("fixture", "fixture@example.test").unwrap();
    repo.commit(Some("HEAD"), &who, &who, "fixture", &tree, &[])
        .unwrap()
}

/// A last commit the probe cannot read is no proof of absence, whatever the
/// reason; only a branch with no commit at all holds no definition.
#[test]
fn an_unreadable_last_commit_refuses() {
    let missing = Scratch::new(HTTPS);
    let commit = commit_empty(&missing).to_string();
    let object = missing
        .root()
        .join(".git/objects")
        .join(&commit[..2])
        .join(&commit[2..]);
    std::fs::remove_file(object).unwrap();
    let error = nested_refusal(&missing, missing.root(), &[], &["fetch", "origin"]);
    assert!(error.contains("last commit"), "{error}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let hidden = Scratch::new(HTTPS);
        commit_empty(&hidden);
        let file = hidden.root().join(".git/refs/heads/main");
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o000)).unwrap();
        let outcome = hidden.contacts_from(hidden.root(), &[], &[], &["fetch", "origin"]);
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(
            outcome
                .as_ref()
                .is_err_and(|error| error.starts_with(UNPROVEN) && error.contains("last commit")),
            "{:?}",
            outcome.err()
        );
    }
    // Controls: a readable commit and a branch with no commit pass.
    let readable = Scratch::new(HTTPS);
    commit_empty(&readable);
    assert!(readable
        .contacts_from(readable.root(), &[], &[], &["fetch", "origin"])
        .is_ok());
    let unborn = Scratch::new(HTTPS);
    assert!(unborn
        .contacts_from(unborn.root(), &[], &[], &["fetch", "origin"])
        .is_ok());
}

/// Commit a tree holding a nested-repository record named `lib`. With
/// `branch` the commit goes on that new branch and the checked-out one stays
/// as it is; without, it becomes the last commit and the index does not hold
/// the record.
fn commit_nested_record(scratch: &Scratch, branch: Option<&str>) {
    let repo = git2::Repository::open(scratch.root()).unwrap();
    let mut tree = repo.treebuilder(None).unwrap();
    tree.insert(
        "lib",
        git2::Oid::from_str("0123456789012345678901234567890123456789").unwrap(),
        0o160_000,
    )
    .unwrap();
    let tree = repo.find_tree(tree.write().unwrap()).unwrap();
    let who = git2::Signature::now("fixture", "fixture@example.test").unwrap();
    let update = branch.map_or("HEAD".to_string(), |name| format!("refs/heads/{name}"));
    repo.commit(Some(&update), &who, &who, "fixture", &tree, &[])
        .unwrap();
}

/// A push whose recursion may be on refuses whatever the guard reads in the
/// index, the last commit and the work tree, since it never reads the
/// revisions the push sends. A push that leaves recursion off, or turns it
/// off on the line, keeps its verdict.
#[test]
fn a_push_whose_recursion_may_be_on_refuses() {
    let on = [
        "push --recurse-submodules=on-demand origin other",
        "push --recurse-submodules=check origin other",
        "push --recurse-submodules=only origin other",
        "push --recurse-submodules origin other",
    ];
    // The record is in a revision other than the last commit and the index.
    let elsewhere = Scratch::new(HTTPS);
    commit_nested_record(&elsewhere, Some("other"));
    // The record is in the last commit only.
    let last = Scratch::new(HTTPS);
    commit_nested_record(&last, None);
    // Nothing the guard reads holds a record.
    let plain = Scratch::new(HTTPS);
    for scratch in [&elsewhere, &last, &plain] {
        for command in on {
            scratch.unproven(command, "nested repositories");
        }
        for command in [
            "push origin other",
            "push --no-recurse-submodules origin other",
            "push --recurse-submodules=no origin other",
        ] {
            assert_eq!(scratch.helpers(command), Ok(vec![]), "{command}");
        }
    }
    // The effective mode may come from the settings too.
    for setting in [
        "[push]\n\trecurseSubmodules = on-demand\n",
        "[push]\n\trecurseSubmodules = check\n",
        "[submodule]\n\trecurse = true\n",
    ] {
        let scratch = Scratch::new(&format!("{HTTPS}{setting}"));
        commit_nested_record(&scratch, Some("other"));
        scratch.unproven("push origin other", "nested repositories");
        assert_eq!(
            scratch.helpers("push --no-recurse-submodules origin other"),
            Ok(vec![])
        );
    }
    let off = Scratch::new(&format!("{HTTPS}[push]\n\trecurseSubmodules = no\n"));
    commit_nested_record(&off, Some("other"));
    assert_eq!(off.helpers("push origin other"), Ok(vec![]));
}

/// The probe reads the last commit as the call does: a replacement the call
/// turns off by option, or by setting, is not followed.
#[test]
fn the_last_commit_is_read_as_the_call_reads_it() {
    let scratch = Scratch::new(HTTPS);
    definition_outside_work_tree(&scratch, true);
    let repo = git2::Repository::open(scratch.root()).unwrap();
    let original = repo.head().unwrap().target().unwrap();
    let tree = repo
        .find_tree(repo.treebuilder(None).unwrap().write().unwrap())
        .unwrap();
    let who = git2::Signature::now("fixture", "fixture@example.test").unwrap();
    let swap = repo
        .commit(None, &who, &who, "fixture", &tree, &[])
        .unwrap();
    repo.reference(&format!("refs/replace/{original}"), swap, true, "fixture")
        .unwrap();
    let line = ["fetch", "origin"];
    let error = nested_refusal(&scratch, scratch.root(), &["--no-replace-objects"], &line);
    assert!(error.contains("nested repositories"), "{error}");
    assert!(scratch
        .contacts_from(scratch.root(), &[], &[], &line)
        .is_ok());
    let setting = Scratch::new(&format!("{HTTPS}[core]\n\tuseReplaceRefs = false\n"));
    let error = nested_refusal(&setting, setting.root(), &[], &line);
    assert!(error.contains("nested repositories"), "{error}");
}

/// A command not proven local refuses; one that only removes setup passes.
#[test]
fn a_command_that_sets_up_later_contacts_refuses() {
    let scratch = Scratch::new(&format!("{HTTPS}[remote \"backup\"]\n\turl = fixture::x\n"));
    for command in [
        "maintenance start",
        "maintenance start --scheduler=crontab",
        "maintenance register",
        "maintenance register --config-file=other",
        "maintenance --quiet start",
        "maintenance",
        "maintenance unknown",
    ] {
        scratch.unproven(command, "background");
    }
    for command in [
        "maintenance stop",
        "maintenance unregister",
        "maintenance unregister --force",
        "maintenance is-needed",
    ] {
        assert_eq!(scratch.helpers(command), Ok(vec![]), "{command}");
    }
}

/// Where the probe cannot look, recursion refuses unless the line turns it
/// off.
#[test]
fn nested_contacts_refuse_where_the_probe_cannot_look() {
    let plain = Scratch::new(HTTPS);
    let elsewhere = plain.root().with_file_name("elsewhere");
    std::fs::create_dir(&elsewhere).unwrap();
    let worktree = format!("core.worktree={}", elsewhere.display());
    assert_eq!(
        plain
            .contacts_with(&["fetch", "origin"], &[])
            .map(helper_names),
        Ok(vec![])
    );
    let error = plain
        .contacts_with(&["fetch", "origin"], &["-c", &worktree])
        .expect_err("a work tree the probe does not read must refuse");
    assert!(error.starts_with(UNPROVEN), "{error}");
    assert_eq!(
        plain
            .contacts_with(
                &["fetch", "--no-recurse-submodules", "origin"],
                &["-c", &worktree]
            )
            .map(helper_names),
        Ok(vec![])
    );
    let bare = Scratch::bare(HTTPS);
    assert_eq!(
        bare.contacts_with(&["fetch", "origin"], &[])
            .map(helper_names),
        Ok(vec![])
    );
    let error = bare
        .contacts_with(&["fetch", "origin"], &["-c", "core.bare=false"])
        .expect_err("a bare repository read as one with a work tree must refuse");
    assert!(error.starts_with(UNPROVEN), "{error}");
}

/// A command whose remote option Git's option parser reads in any accepted
/// spelling is judged in every spelling, attached or separate.
#[test]
fn a_remote_option_is_read_in_every_spelling_git_accepts() {
    let scratch = Scratch::new("");
    for command in [
        "archive --remote=fixture::x HEAD",
        "archive --remot=fixture::x HEAD",
        "archive --rem=fixture::x HEAD",
        "archive --r=fixture::x HEAD",
        "archive --remote fixture::x HEAD",
        "archive --rem fixture::x HEAD",
        "archive --r fixture::x HEAD",
        "archive HEAD --rem fixture::x",
        "archive -o out.tar --rem=fixture::x HEAD",
    ] {
        assert_eq!(
            scratch.helpers(command),
            Ok(vec!["fixture".into()]),
            "{command}"
        );
    }
    // Stock controls: an ordinary address in each spelling, and no remote.
    for command in [
        "archive --remote=https://example.test/r HEAD",
        "archive --rem=https://example.test/r HEAD",
        "archive --rem https://example.test/r HEAD",
        "archive HEAD",
        "archive --format=tar -o out.tar HEAD",
    ] {
        assert_eq!(scratch.helpers(command), Ok(vec![]), "{command}");
    }
    // A form the option table does not prove refuses.
    for command in [
        "archive --a fixture::x HEAD",
        "archive --rem",
        "archive --unlisted=1 HEAD",
        "archive -z HEAD",
    ] {
        let error = scratch
            .helpers(command)
            .expect_err(&format!("`{command}` must refuse"));
        assert!(error.starts_with(UNPROVEN), "{command}: {error}");
    }
}

/// An option reading the guard cannot prove refuses.
#[test]
fn a_remote_option_word_another_option_may_consume_refuses() {
    let scratch = Scratch::new("");
    for command in [
        "archive --format --remote=fixture::x HEAD",
        "archive --prefix --remote fixture::x HEAD",
        "archive -9o --remote=fixture::x HEAD",
        "archive --out --remote=fixture::x HEAD",
        "archive --add-file --remote=fixture::x HEAD",
        "archive --add-virtual-file --remote=fixture::x HEAD",
        "archive --mtime --remote=fixture::x HEAD",
        "archive -o --remote=fixture::x HEAD",
        "archive -vo --rem fixture::x HEAD",
        "archive --format --rem=https://example.test/r HEAD",
    ] {
        let error = scratch
            .helpers(command)
            .expect_err(&format!("`{command}` must refuse"));
        assert!(error.starts_with(UNPROVEN), "{command}: {error}");
        assert!(error.contains(HIDDEN_OPTION), "{command}: {error}");
    }
    // Controls: an option with its own value, then the remote option; the
    // remote is judged as in every other spelling.
    for command in [
        "archive --format zip --remote=https://example.test/r HEAD",
        "archive --prefix p/ --rem https://example.test/r HEAD",
        "archive -9 -o out.tar --remote=https://example.test/r HEAD",
        "archive --format zip HEAD",
    ] {
        assert_eq!(scratch.helpers(command), Ok(vec![]), "{command}");
    }
    assert_eq!(
        scratch.helpers("archive --format zip --remote=fixture::x HEAD"),
        Ok(vec!["fixture".into()])
    );
}

/// Every option the resolution reads is read in each abbreviation Git
/// accepts as it is read in full, attached or separate.
#[test]
fn an_abbreviated_option_reads_as_the_full_option() {
    let scratch = Scratch::new(&format!("{HTTPS}[remote \"backup\"]\n\turl = fixture::x\n"));
    for refused in [
        "fetch --all",
        "fetch --al",
        "maintenance run --task=prefetch",
        "maintenance run --ta=prefetch",
        "maintenance run --t=prefetch",
        "maintenance run --ta prefetch",
        "maintenance run --t prefetch",
        "remote add --fetch backup https://example.test/r",
        "remote add --fetc backup https://example.test/r",
        "remote add --f backup https://example.test/r",
        "clone --config=submodule.recurse=1 https://example.test/r copy",
        "clone --confi=submodule.recurse=1 https://example.test/r copy",
        "clone --con=submodule.recurse=1 https://example.test/r copy",
        "clone --confi submodule.recurse=1 https://example.test/r copy",
        "clone --con submodule.recurse=1 https://example.test/r copy",
    ] {
        let error = scratch
            .helpers(refused)
            .expect_err(&format!("`{refused}` must refuse"));
        assert!(error.starts_with(UNPROVEN), "{refused}: {error}");
    }
    // Stock controls: the same options with an ordinary meaning pass.
    let scratch = Scratch::new(HTTPS);
    for allowed in [
        "maintenance run --task=gc",
        "maintenance run --ta=gc",
        "maintenance run --ta gc",
        "remote add --no-fetch fresh https://example.test/r",
        "remote add --no-fet fresh https://example.test/r",
        "clone --config=core.autocrlf=false --no-recurse-submodules https://example.test/r copy",
        "clone --conf=core.autocrlf=false --no-recurse-submodules https://example.test/r copy",
    ] {
        assert_eq!(scratch.helpers(allowed), Ok(vec![]), "{allowed}");
    }
}

/// Add a repository record at `path` to the index file `index`, which holds
/// no other entry.
fn gitlink_only_index(index: &Path, path: &str) {
    let mut other = git2::Index::open(index).unwrap();
    other
        .add(&git2::IndexEntry {
            ctime: git2::IndexTime::new(0, 0),
            mtime: git2::IndexTime::new(0, 0),
            dev: 0,
            ino: 0,
            mode: 0o160_000,
            uid: 0,
            gid: 0,
            file_size: 0,
            id: git2::Oid::from_str("0123456789abcdef0123456789abcdef01234567").unwrap(),
            flags: u16::try_from(path.len()).unwrap(),
            flags_extended: 0,
            path: path.as_bytes().to_vec(),
        })
        .unwrap();
    other.write().unwrap();
}

/// The index proof covers the whole index, whatever directory the call runs
/// in and whichever way it selects one.
#[test]
fn the_index_proof_covers_the_whole_index() {
    let scratch = Scratch::new(HTTPS);
    nested_in_index_only(&scratch, "lib");
    let nested = scratch.root().join("src/deep");
    std::fs::create_dir_all(&nested).unwrap();
    let root = scratch.root();
    let src = root.join("src");
    let fetch = ["fetch", "origin"];
    let off = ["fetch", "--no-recurse-submodules", "origin"];
    // The root and each nested directory read the same index.
    for cwd in [root, src.as_path(), nested.as_path()] {
        let error = scratch
            .contacts_from(cwd, &[], &[], &fetch)
            .expect_err("a sibling record must refuse");
        assert!(error.starts_with(UNPROVEN), "{}: {error}", cwd.display());
        assert_eq!(
            scratch.contacts_from(cwd, &[], &[], &off).map(helper_names),
            Ok(vec![]),
            "{}",
            cwd.display()
        );
    }
    // A call that selects a nested directory with its own option reads the
    // same index too.
    for globals in [&["-C", "src"][..], &["-C", "src", "-C", "deep"][..]] {
        let error = scratch
            .contacts_from(root, globals, &[], &fetch)
            .expect_err("a sibling record must refuse");
        assert!(error.starts_with(UNPROVEN), "{globals:?}: {error}");
        assert_eq!(
            scratch
                .contacts_from(root, globals, &[], &off)
                .map(helper_names),
            Ok(vec![]),
            "{globals:?}"
        );
    }
    // Controls: with no record in the index, every directory passes.
    let plain = Scratch::new(HTTPS);
    let plain_src = plain.root().join("src");
    std::fs::create_dir_all(&plain_src).unwrap();
    for cwd in [plain.root(), plain_src.as_path()] {
        assert_eq!(
            plain.contacts_from(cwd, &[], &[], &fetch).map(helper_names),
            Ok(vec![]),
            "{}",
            cwd.display()
        );
    }
    // An index file the call selects is the one read, from every directory.
    let selected = Scratch::new(HTTPS);
    let selected_src = selected.root().join("src");
    std::fs::create_dir_all(&selected_src).unwrap();
    let alternate = selected.root().with_file_name("alternate-index");
    gitlink_only_index(&alternate, "lib");
    let alternate = alternate.display().to_string();
    for cwd in [selected.root(), selected_src.as_path()] {
        let error = selected
            .contacts_from(cwd, &[], &[("GIT_INDEX_FILE", &alternate)], &fetch)
            .expect_err("a record in the selected index must refuse");
        assert!(error.starts_with(UNPROVEN), "{}: {error}", cwd.display());
        assert_eq!(
            selected
                .contacts_from(cwd, &[], &[], &fetch)
                .map(helper_names),
            Ok(vec![]),
            "{}",
            cwd.display()
        );
    }
}
