//! Which repository a git command in a shell line targets (TSK-112).
//!
//! The git-guard judges a git op against the repository it runs in. A `cd`,
//! a `-C`, a `--git-dir`, a `GIT_DIR=` setting, or a path held in a shell
//! variable moves that repository away from the session's. This module models
//! those moves only where every path the shell could take is known: a *flat*
//! line of simple commands joined by `;`, `&&` or newlines, with no subshell,
//! group, pipe, background job, top-level heredoc or control keyword. A `$(…)`
//! or backtick substitution inside a word is allowed; it runs in a subshell
//! and cannot move the line's own shell. The working directory is tracked as
//! the set of directories the shell could be in: a `cd` proves its move only
//! to the commands chained after it with `&&`. Anything the model cannot
//! follow leaves the target unresolved, and the guard then blocks a mutation
//! rather than assume the target is safe.

use std::collections::HashMap;

use super::git_guard::{capture_backtick, capture_balanced, starts_word};

/// Stands in a segment's text for a `$(…)` or backtick substitution the
/// splitter cut out from inside double quotes: the shell will put that
/// command's output there, as part of one word, and the guard cannot know it.
pub(super) const SUBSTITUTED: char = '\u{1}';

/// Stands for a substitution the splitter cut out from outside quotes: its
/// output is also split into words and globbed, so it can add, remove or
/// reorder arguments.
pub(super) const SUBSTITUTED_BARE: char = '\u{2}';

/// The placeholder for a substitution cut out inside (`quoted`) or outside
/// double quotes.
pub(super) fn substitution_placeholder(quoted: bool) -> char {
    if quoted {
        SUBSTITUTED
    } else {
        SUBSTITUTED_BARE
    }
}

/// `true` when `word` holds a substitution placeholder of either kind.
pub(super) fn has_substitution(word: &str) -> bool {
    word.contains([SUBSTITUTED, SUBSTITUTED_BARE])
}

/// How a top-level simple command is joined to the one before it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Join {
    /// The first command of the line.
    Start,
    /// After `;` or a newline: it runs whatever the previous command did.
    Seq,
    /// After `&&`: it runs only when the previous command succeeded.
    And,
}

/// Words that open shell control structures, whose bodies run conditionally
/// or repeatedly. A line using one is not flat.
const CONTROL_WORDS: &[&str] = &[
    "if", "then", "elif", "else", "fi", "while", "until", "do", "done", "for", "case", "esac",
    "select", "function", "coproc",
];

/// The top-level simple commands of a flat line, each as the segment text the
/// guard's splitter produces for it (substitutions removed) with its join, or
/// `None` when the line is not flat. A `$` whose quoting the guard's
/// quote-stripped words would lose (`\$`, `'…$…'`, `$'…'`, `$"…"`) also makes a
/// line not flat, so a literal `$R` is never expanded as a variable.
#[allow(clippy::too_many_lines)] // one character scanner
pub(super) fn flat_top_level(command: &str) -> Option<Vec<(String, Join)>> {
    let chars: Vec<char> = command.chars().collect();
    let mut out = Vec::new();
    let mut pending = Join::Start;
    let mut cur = String::new();
    let mut in_double = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        // Substitutions run in a subshell; the splitter replaces each with a
        // placeholder in the segment text, and so does this scanner.
        if c == '$' && next == Some('(') {
            i = capture_balanced(&chars, i + 2).1;
            cur.push(substitution_placeholder(in_double));
            continue;
        }
        if c == '`' {
            i = capture_backtick(&chars, i + 1).1;
            cur.push(substitution_placeholder(in_double));
            continue;
        }
        if c == '$' && matches!(next, Some('\'' | '"')) {
            return None;
        }
        if in_double {
            match c {
                '"' => in_double = false,
                '\\' => {
                    if next == Some('$') {
                        return None;
                    }
                    cur.push(c);
                    if let Some(n) = next {
                        cur.push(n);
                    }
                    i += 2;
                    continue;
                }
                _ => {}
            }
            cur.push(c);
            i += 1;
            continue;
        }
        match c {
            '\'' => {
                let close = chars[i + 1..].iter().position(|&ch| ch == '\'')? + i + 1;
                if chars[i + 1..close].contains(&'$') {
                    return None;
                }
                cur.extend(&chars[i..=close]);
                i = close + 1;
            }
            '"' => {
                in_double = true;
                cur.push(c);
                i += 1;
            }
            '\\' => {
                if next == Some('$') {
                    return None;
                }
                cur.push(c);
                if let Some(n) = next {
                    cur.push(n);
                }
                i += 2;
            }
            '#' if starts_word(&chars, i) => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '\n' | ';' => {
                close_segment(&mut cur, &mut out, &mut pending, Join::Seq)?;
                i += 1;
            }
            '&' if next == Some('&') => {
                close_segment(&mut cur, &mut out, &mut pending, Join::And)?;
                i += 2;
            }
            // `>&` / `&>` redirections stay with their command.
            '&' if next == Some('>') || cur.ends_with('>') => {
                cur.push(c);
                i += 1;
            }
            '|' if cur.ends_with('>') => {
                cur.push(c);
                i += 1;
            }
            '{' if next.is_none_or(super::git_guard::shell_blank) => return None,
            '}' if i == 0 || super::git_guard::shell_blank(chars[i - 1]) => return None,
            '<' if matches!(next, Some('<' | '(')) => return None,
            '>' if next == Some('(') => return None,
            // Background jobs, pipes, `||`, subshells, groups.
            '&' | '|' | '(' | ')' => return None,
            _ => {
                cur.push(c);
                i += 1;
            }
        }
    }
    if in_double {
        return None;
    }
    close_segment(&mut cur, &mut out, &mut pending, Join::Seq)?;
    Some(out)
}

/// End the current top-level command. An empty one records nothing: after a
/// `&&` still waiting for its command (a newline or a comment continues the
/// and-list) the join stays `&&`; otherwise (`;;`, a trailing `;`) the next
/// command gets the weaker join. `None` when the command opens a control
/// structure, negates a directory change, or runs substituted text.
fn close_segment(
    cur: &mut String,
    out: &mut Vec<(String, Join)>,
    pending: &mut Join,
    next: Join,
) -> Option<()> {
    let text = std::mem::take(cur);
    let text = text.trim_matches(super::git_guard::shell_blank);
    let mut words = text
        .split(super::git_guard::shell_blank)
        .filter(|word| !word.is_empty());
    let Some(first) = words.next() else {
        if *pending == Join::Seq || (*pending == Join::And && next == Join::And) {
            *pending = Join::Seq;
        }
        return Some(());
    };
    if CONTROL_WORDS.contains(&first) || has_substitution(first) {
        return None;
    }
    if first == "!" && words.next().is_some_and(moves_directory) {
        return None;
    }
    out.push((text.to_string(), *pending));
    *pending = next;
    Some(())
}

/// Map the guard's segments onto a flat line's top-level commands: for each
/// segment, `Some(join)` when it is a top-level command and `None` when it is
/// nested (inside a substitution or a `bash -c`/`eval` string). `None` when
/// the top-level commands cannot all be matched in order.
pub(super) fn map_top_level(
    segments: &[String],
    top: &[(String, Join)],
) -> Option<Vec<Option<Join>>> {
    let mut roles = Vec::with_capacity(segments.len());
    let mut next = 0;
    for segment in segments {
        if top.get(next).is_some_and(|(text, _)| text == segment) {
            roles.push(Some(top[next].1));
            next += 1;
        } else {
            roles.push(None);
        }
    }
    (next == top.len()).then_some(roles)
}

/// A shell value the guard tracks: known text, or unknown with the reason.
#[derive(Clone, Debug)]
pub(super) enum Val {
    Known(String),
    Unknown(String),
}

/// Expand the `$NAME` and `${NAME}` references in a quote-stripped word from
/// the tracked variables. `Err` names what could not be expanded: an unset or
/// unknown variable, a special parameter, `~`, a glob, or a value that word
/// splitting or globbing could change.
pub(super) fn expand_word(word: &str, vars: &HashMap<String, Val>) -> Result<String, String> {
    if word.starts_with('~') {
        return Err("`~`".to_string());
    }
    let chars: Vec<char> = word.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == SUBSTITUTED || c == SUBSTITUTED_BARE {
            return Err("a command substitution".to_string());
        }
        if matches!(c, '*' | '?' | '[') {
            return Err(format!("the pattern in `{word}`"));
        }
        if c != '$' {
            out.push(c);
            i += 1;
            continue;
        }
        let (name, end) = if chars.get(i + 1) == Some(&'{') {
            let close = chars[i + 2..]
                .iter()
                .position(|&ch| ch == '}')
                .map(|p| p + i + 2)
                .ok_or_else(|| format!("`{word}`"))?;
            (chars[i + 2..close].iter().collect::<String>(), close + 1)
        } else {
            let len = chars[i + 1..]
                .iter()
                .take_while(|ch| ch.is_ascii_alphanumeric() || **ch == '_')
                .count();
            (chars[i + 1..=i + len].iter().collect(), i + 1 + len)
        };
        if !is_name(&name) {
            return Err(format!("`{word}`"));
        }
        match vars.get(&name) {
            Some(Val::Known(v))
                if !v.contains(|ch: char| {
                    super::git_guard::shell_blank(ch) || matches!(ch, '*' | '?' | '[')
                }) =>
            {
                out.push_str(v);
            }
            Some(Val::Unknown(why)) => return Err(why.clone()),
            _ => return Err(format!("`${name}`")),
        }
        i = end;
    }
    Ok(out)
}

/// `true` for a shell variable name.
pub(super) fn is_name(s: &str) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// `NAME=value` split, when `word` is an assignment.
pub(super) fn assignment(word: &str) -> Option<(&str, &str)> {
    word.split_once('=').filter(|(name, _)| is_name(name))
}

/// Join `next` onto the directory expression `base` ("" is the session's
/// working directory). An absolute `next` replaces the base; an empty one
/// changes nothing, as `git -C ""` does.
pub(super) fn join_path(base: &str, next: &str) -> String {
    if next.is_empty() {
        base.to_string()
    } else if next.starts_with('/') || base.is_empty() {
        next.to_string()
    } else {
        format!("{}/{next}", base.trim_end_matches('/'))
    }
}

/// Environment variables that move where git reads and writes. The guard
/// models `GIT_DIR` given on the git command itself; any other way of setting
/// one of these leaves the target unresolved.
pub(super) const GIT_LOCATION_VARS: &[&str] = &["GIT_DIR", "GIT_COMMON_DIR", "GIT_WORK_TREE"];

/// The environment a simple command's launchers give the program they run:
/// leading `NAME=value` words and the assignments of `env`, through
/// `command`, `builtin` and `exec`. `Err` for an `env` option, whose effect
/// (`-C`, `-u`, `-S`, `-i`) the guard does not model.
pub(super) fn launcher_env(tokens: &[String]) -> Result<Vec<(String, String)>, String> {
    let mut env = Vec::new();
    let mut i = 0;
    loop {
        while let Some((name, value)) = tokens.get(i).and_then(|t| assignment(t)) {
            env.push((name.to_string(), value.to_string()));
            i += 1;
        }
        let Some(word) = tokens.get(i) else {
            return Ok(env);
        };
        if matches!(word.as_str(), "command" | "builtin" | "exec") {
            i += 1;
            continue;
        }
        if word.rsplit('/').next() == Some("env") {
            i += 1;
            while let Some(arg) = tokens.get(i) {
                if arg.starts_with('-') {
                    return Err(format!("the `env` option `{arg}`"));
                }
                let Some((name, value)) = assignment(arg) else {
                    break;
                };
                env.push((name.to_string(), value.to_string()));
                i += 1;
            }
            continue;
        }
        return Ok(env);
    }
}

/// The directories the shell could be in: path expressions ("" is the
/// session's working directory), or unknown with the reason.
#[derive(Clone, Debug)]
pub(super) enum Cwd {
    Paths(Vec<String>),
    Unknown(String),
}

/// More directory candidates than this are treated as unknown.
const MAX_CANDIDATES: usize = 8;

impl Cwd {
    fn session() -> Self {
        Self::Paths(vec![String::new()])
    }

    fn union(&self, other: &Self) -> Self {
        match (self, other) {
            (Self::Unknown(why), _) | (_, Self::Unknown(why)) => Self::Unknown(why.clone()),
            (Self::Paths(a), Self::Paths(b)) => {
                let mut all = a.clone();
                for p in b {
                    if !all.contains(p) {
                        all.push(p.clone());
                    }
                }
                if all.len() > MAX_CANDIDATES {
                    Self::Unknown("too many possible directories".to_string())
                } else {
                    Self::Paths(all)
                }
            }
        }
    }
}

/// Builtins that change shell variables in ways the tracker does not model.
const VAR_WRITERS: &[&str] = &[
    "unset",
    "read",
    "declare",
    "typeset",
    "local",
    "readonly",
    "mapfile",
    "readarray",
    "getopts",
    "let",
];

/// `true` when a program can change the working directory of the shell that
/// runs it in ways the tracker does not follow.
pub(super) fn moves_directory(program: &str) -> bool {
    matches!(
        program,
        "cd" | "pushd" | "popd" | "chdir" | "source" | "." | "eval"
    )
}

/// The top-level shell state a flat line builds up, command by command.
pub(super) struct ShellState {
    flat: bool,
    /// Where the shell could be now.
    pub(super) cwd: Cwd,
    /// Everywhere the shell could be after the current and-list stops, at
    /// whichever member it stops.
    list_cwd: Cwd,
    pub(super) vars: HashMap<String, Val>,
    /// `true` while the current command runs only if an earlier `&&` member
    /// succeeded.
    conditional: bool,
    /// Variables set by a conditional member of the current and-list.
    cond_vars: Vec<String>,
}

impl ShellState {
    /// State for a flat line (`flat`), or an inert one otherwise.
    pub(super) fn new(flat: bool) -> Self {
        Self {
            flat,
            cwd: Cwd::session(),
            list_cwd: Cwd::session(),
            vars: HashMap::new(),
            conditional: false,
            cond_vars: Vec::new(),
        }
    }

    /// `true` when the line is flat and the state is tracked.
    pub(super) fn flat(&self) -> bool {
        self.flat
    }

    /// Enter a top-level command joined by `join`. After `;` the shell may be
    /// wherever the previous and-list stopped.
    pub(super) fn begin(&mut self, join: Join) {
        match join {
            Join::Start => {}
            Join::And => self.conditional = true,
            Join::Seq => {
                self.cwd = self.list_cwd.union(&self.cwd);
                for name in std::mem::take(&mut self.cond_vars) {
                    let why = format!("`${name}`, set only when an earlier command succeeds");
                    self.vars.insert(name, Val::Unknown(why));
                }
                self.conditional = false;
                self.list_cwd = self.cwd.clone();
            }
        }
    }

    /// A `cd <dir>`. It may fail (a missing directory, a failed redirection),
    /// so the directory before it stays possible once its and-list ends.
    pub(super) fn cd(&mut self, dir: &str) {
        if !self.flat {
            return;
        }
        let moved = match (expand_word(dir, &self.vars), &self.cwd) {
            (Ok(d), Cwd::Paths(bases)) => {
                Cwd::Paths(bases.iter().map(|b| join_path(b, &d)).collect())
            }
            (Ok(d), Cwd::Unknown(_)) if d.starts_with('/') => Cwd::Paths(vec![d]),
            (Ok(_), Cwd::Unknown(why)) => Cwd::Unknown(why.clone()),
            (Err(why), _) => Cwd::Unknown(why),
        };
        self.list_cwd = self.list_cwd.union(&moved);
        self.cwd = moved;
    }

    /// Record the variables a command sets: standalone `NAME=value` words or
    /// an `export`. `program` is the command's program after its leading
    /// assignments, `None` when there is none. A `redirected` command can fail
    /// before it assigns, so what it sets becomes unknown.
    pub(super) fn assign(&mut self, tokens: &[String], program: Option<&str>, redirected: bool) {
        if !self.flat {
            return;
        }
        let words: &[String] = match program {
            None if tokens.iter().all(|t| assignment(t).is_some()) => tokens,
            None => return,
            Some("export") if tokens.first().is_some_and(|t| t == "export") => {
                if tokens[1..].iter().any(|t| t.starts_with('-')) {
                    self.forget_vars("an `export` with options");
                    return;
                }
                &tokens[1..]
            }
            Some(_) => {
                // `NAME+=x` reads as a program word; it still changes NAME.
                if let Some(name) = tokens
                    .first()
                    .and_then(|t| t.split_once("+="))
                    .map(|(n, _)| n)
                    .filter(|n| is_name(n))
                {
                    self.set_var(name, Val::Unknown(format!("`${name}`, appended to")));
                }
                return;
            }
        };
        for word in words {
            let Some((name, value)) = assignment(word) else {
                continue;
            };
            let val = match expand_word(value, &self.vars) {
                Ok(_) if redirected => Val::Unknown(format!(
                    "`${name}`, set by a command whose redirection can fail"
                )),
                Ok(v) => Val::Known(v),
                Err(why) => Val::Unknown(why),
            };
            self.set_var(name, val);
        }
    }

    fn set_var(&mut self, name: &str, val: Val) {
        self.vars.insert(name.to_string(), val);
        if self.conditional {
            self.cond_vars.push(name.to_string());
        }
    }

    fn forget_vars(&mut self, why: &str) {
        for val in self.vars.values_mut() {
            *val = Val::Unknown(format!("a variable changed by {why}"));
        }
    }

    /// Account for a program that may change the shell's state untracked.
    pub(super) fn observe(&mut self, program: &str, args: &[String]) {
        if !self.flat {
            return;
        }
        if moves_directory(program) {
            let lost = Cwd::Unknown(format!("the directory after `{program}`"));
            self.list_cwd = lost.clone();
            self.cwd = lost;
        }
        match program {
            "source" | "." | "eval" => self.forget_vars(&format!("`{program}`")),
            "printf" if args.iter().any(|a| a == "-v") => self.forget_vars("`printf -v`"),
            p if VAR_WRITERS.contains(&p) => self.forget_vars(&format!("`{p}`")),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn unicode_blanks_stay_in_target_commands() {
        for command in [
            "cd repo\u{a0}",
            "if\u{a0} echo",
            "{\u{a0} echo",
            "echo x\u{a0}}",
        ] {
            let commands = flat_top_level(command).unwrap();
            assert_eq!(commands[0].0, command);
        }
        let vars = HashMap::from([("DIR".into(), Val::Known("repo\u{a0}".into()))]);
        assert_eq!(expand_word("$DIR", &vars).unwrap(), "repo\u{a0}");
    }
    use super::*;

    fn joins(cmd: &str) -> Option<Vec<Join>> {
        flat_top_level(cmd).map(|t| t.into_iter().map(|(_, j)| j).collect())
    }

    #[test]
    fn test_flat_top_level_reads_sequences_and_and_lists() {
        assert_eq!(
            joins("R=/x; cd a && git commit -m 'm'\ngit status"),
            Some(vec![Join::Start, Join::Seq, Join::And, Join::Seq])
        );
        assert_eq!(joins("git log 2>&1 >/dev/null"), Some(vec![Join::Start]));
        assert_eq!(
            joins("cd \"${R}\" && git commit # done"),
            Some(vec![Join::Start, Join::And])
        );
        assert_eq!(joins(";; git status;"), Some(vec![Join::Start]));
        // R2-2: a newline or a comment after `&&` continues the and-list.
        assert_eq!(
            joins("cd a &&\ngit commit"),
            Some(vec![Join::Start, Join::And])
        );
        assert_eq!(
            joins("cd a && # into a\n\ngit commit"),
            Some(vec![Join::Start, Join::And])
        );
        assert_eq!(
            joins("cd a\ngit commit"),
            Some(vec![Join::Start, Join::Seq])
        );
        // A substitution in a word runs in a subshell; the line stays flat.
        assert_eq!(
            flat_top_level("cd /w && git commit -m \"$(cat <<'EOF'\nmsg (x)\nEOF\n)\""),
            Some(vec![
                ("cd /w".to_string(), Join::Start),
                ("git commit -m \"\u{1}\"".to_string(), Join::And)
            ])
        );
    }

    #[test]
    fn test_flat_top_level_rejects_what_scopes_or_hides_state() {
        for cmd in [
            "(cd a); git commit",
            "{ cd a; }; git commit",
            "cd a | git commit",
            "false || cd a; git commit",
            "cd a & git commit",
            "git commit -F - <<EOF\nx\nEOF",
            "if true; then cd a; fi; git commit",
            "for d in a; do cd $d; done",
            "R=/x; git -C '$R' commit",
            "R=/x; git -C \"\\$R\" commit",
            "R=/x; git -C \\$R commit",
            "git -C $'/x' commit",
            "git -C \"unterminated",
            "diff <(git show a) b",
            "! cd a && git commit",
            "$(printf cd) a; git commit",
        ] {
            assert_eq!(flat_top_level(cmd), None, "{cmd}");
        }
    }

    #[test]
    fn test_map_top_level_marks_nested_segments() {
        let segments: Vec<String> = ["git log -1", "git commit -m \"\""]
            .iter()
            .map(ToString::to_string)
            .collect();
        let top = vec![("git commit -m \"\"".to_string(), Join::Start)];
        assert_eq!(
            map_top_level(&segments, &top),
            Some(vec![None, Some(Join::Start)])
        );
        assert_eq!(map_top_level(&segments[..1], &top), None);
    }

    #[test]
    fn test_launcher_env_reads_env_and_refuses_its_options() {
        let t = |s: &str| s.split(' ').map(ToString::to_string).collect::<Vec<_>>();
        assert_eq!(
            launcher_env(&t("A=1 command env GIT_DIR=/g git -C x commit")).unwrap(),
            vec![
                ("A".to_string(), "1".to_string()),
                ("GIT_DIR".to_string(), "/g".to_string())
            ]
        );
        assert!(launcher_env(&t("env -u GIT_DIR git commit")).is_err());
        assert!(launcher_env(&t("/usr/bin/env -C /x git commit")).is_err());
    }

    #[test]
    fn test_expand_word_resolves_only_known_plain_values() {
        let mut vars = HashMap::new();
        vars.insert("R".to_string(), Val::Known("/repo".to_string()));
        vars.insert("S".to_string(), Val::Known("/a b".to_string()));
        vars.insert("U".to_string(), Val::Unknown("u".to_string()));
        assert_eq!(expand_word("$R/sub", &vars).unwrap(), "/repo/sub");
        assert_eq!(expand_word("${R}x", &vars).unwrap(), "/repox");
        for word in ["$S", "$U", "$MISSING", "$1", "~/x", "/a/*", "${R:-/y}"] {
            assert!(expand_word(word, &vars).is_err(), "{word}");
        }
    }

    #[test]
    fn test_join_path_composes_like_chdir() {
        assert_eq!(join_path("", "a"), "a");
        assert_eq!(join_path("/w", "a"), "/w/a");
        assert_eq!(join_path("/w/", "a"), "/w/a");
        assert_eq!(join_path("/w", "/abs"), "/abs");
        assert_eq!(join_path("/w", ""), "/w");
    }

    #[test]
    fn test_cd_before_a_semicolon_keeps_the_old_directory() {
        let mut shell = ShellState::new(true);
        shell.cd("/feature");
        shell.begin(Join::And);
        assert!(matches!(&shell.cwd, Cwd::Paths(p) if p == &vec!["/feature".to_string()]));
        shell.begin(Join::Seq);
        assert!(
            matches!(&shell.cwd, Cwd::Paths(p) if p.contains(&String::new()) && p.contains(&"/feature".to_string()))
        );
    }
}
