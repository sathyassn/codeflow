//! What a `gh api` or `gh auth` call does, judged against one narrow
//! allowlist (TSK-1070 security review round 3). Claude Code runs these
//! subcommands outside its sandbox by name, and its permission rules match
//! command text, so a list of spellings never closes. Round 2 read the
//! arguments by gh's own option grammar instead; round 3 showed that a
//! model of that grammar does not close either (a superseded `--help`, an
//! override header, an option before the subcommand). So exec-guard no
//! longer models it:
//!
//! - a `gh api` call is a read only in one plain form, a GET to one
//!   endpoint with output options; every other form asks the person;
//! - a word naming DELETE anywhere in a `gh api` call is refused;
//! - a `gh auth` call with the token flag anywhere, or with an option
//!   before its subcommand, is refused;
//! - a word the shell fills in is refused, since it could be any of these;
//! - an output expression is code that runs with gh's environment (round
//!   4: `--jq env` prints every variable gh inherited), so any `gh` call's
//!   `--jq` is admitted only as a plain field selector, and `--template` is
//!   refused ([`output_code`]).

use crate::hooks::git_guard::expands;

/// What one `gh api` call does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Intent {
    /// The plain read form: no prompt.
    Read,
    /// Any other form; the reason names what took it outside the plain
    /// read form. Claude asks the person before it runs.
    Write(String),
    /// A word naming DELETE, in any spelling or position: refused.
    Delete,
    /// A word the shell fills in: refused, with the reason.
    Unread(String),
}

/// What one `gh auth` call does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Auth {
    /// No token flag and no option before the subcommand.
    Plain,
    /// The token flag, or an option before the subcommand: refused as a
    /// `forge_exec` action.
    Refused,
    /// A word the shell fills in: refused, with the reason.
    Unread(String),
}

/// Options the plain read form takes with a value, as a separate word or
/// after `=` on the long name.
const READ_VALUE: &[&str] = &["-q", "--jq", "--cache"];
/// Options the plain read form takes without a value.
const READ_FLAG: &[&str] = &["-i", "--include", "--paginate", "--slurp", "--silent"];

/// Whether a word names DELETE: the method word itself, an option with the
/// method attached (`-XDELETE`, `--method=delete`), or a header value
/// (`X-HTTP-Method-Override: DELETE`), in any case. An endpoint path such
/// as `branches/delete-me` is not one.
fn names_delete(word: &str) -> bool {
    let lower = word.trim().to_ascii_lowercase();
    lower.contains("delete") && (lower == "delete" || lower.starts_with('-') || lower.contains(':'))
}

fn unread(word: &str) -> String {
    format!("`{word}` is filled in by the shell, so the guard cannot see what gh receives")
}

/// What a `gh api` call with these arguments (the words after `api`) does.
pub(crate) fn api_intent(args: &[String]) -> Intent {
    if let Some(word) = args.iter().find(|w| expands(w)) {
        return Intent::Unread(unread(word));
    }
    if args.iter().any(|w| names_delete(w)) {
        return Intent::Delete;
    }
    let outside = |what: String| Intent::Write(format!("{what}, outside the plain read form"));
    let mut endpoints = Vec::new();
    let mut words = args.iter();
    while let Some(word) = words.next() {
        if !word.starts_with('-') {
            endpoints.push(word);
            continue;
        }
        if matches!(word.as_str(), "-X" | "--method") {
            match words.next() {
                Some(method) if method.eq_ignore_ascii_case("GET") => continue,
                Some(method) => return outside(format!("uses the method `{method}`")),
                None => return outside(format!("gives `{word}` no value")),
            }
        }
        if let Some(method) = word.strip_prefix("--method=") {
            if method.eq_ignore_ascii_case("GET") {
                continue;
            }
            return outside(format!("uses the method `{method}`"));
        }
        if READ_VALUE.contains(&word.as_str()) {
            if words.next().is_none() {
                return outside(format!("gives `{word}` no value"));
            }
            continue;
        }
        if READ_FLAG.contains(&word.as_str())
            || ["--jq=", "--cache="].iter().any(|p| word.starts_with(p))
        {
            continue;
        }
        return outside(format!("uses `{word}`"));
    }
    let endpoint = match endpoints.as_slice() {
        [endpoint] => endpoint,
        [] => return outside("names no endpoint".to_string()),
        _ => return outside("names more than one endpoint".to_string()),
    };
    if endpoint.contains("://") || endpoint.starts_with("//") {
        return outside(format!("names its own host in `{endpoint}`"));
    }
    if endpoint
        .trim_start_matches('/')
        .eq_ignore_ascii_case("graphql")
    {
        return outside("is a GraphQL call".to_string());
    }
    Intent::Read
}

/// What a `gh` call whose first word is an option does, when `api` is
/// among its words. Which word gh takes as the subcommand depends on which
/// of those options take a value, so the call is never the plain read
/// form.
pub(crate) fn api_after_options(rest: &[String]) -> Intent {
    match api_intent(rest) {
        Intent::Read | Intent::Write(_) => {
            Intent::Write("has an option before `api`, outside the plain read form".to_string())
        }
        refused => refused,
    }
}

/// Whether a word is gh's token flag: `--show-token` with or without a
/// value, or a short option word holding `t`, in any case.
fn shows_token(word: &str) -> bool {
    let lower = word.to_ascii_lowercase();
    if let Some(long) = lower.strip_prefix("--") {
        long == "show-token" || long.starts_with("show-token=")
    } else {
        lower.strip_prefix('-').is_some_and(|s| s.contains('t'))
    }
}

/// What a `gh auth` call with these arguments (the words after `auth`)
/// does.
pub(crate) fn auth_intent(args: &[String]) -> Auth {
    if let Some(word) = args.iter().find(|w| expands(w)) {
        return Auth::Unread(unread(word));
    }
    // An option before the subcommand, or the token flag anywhere.
    if args.first().is_some_and(|w| w.starts_with('-')) || args.iter().any(|w| shows_token(w)) {
        return Auth::Refused;
    }
    Auth::Plain
}

/// Whether a jq program is a plain field selector: dot paths of
/// identifiers with `[]` and `[N]`, joined by `|` or `,`. Nothing else is
/// admitted: no `env`, `$ENV`, variables, functions, string interpolation,
/// formats, object construction or recursion, since gh evaluates the
/// program outside the sandbox with its own environment (security review
/// round 4, SEC-09).
pub(crate) fn is_field_selector(program: &str) -> bool {
    let program = program.trim();
    !program.is_empty()
        && program
            .split(['|', ','])
            .all(|path| is_dot_path(path.trim()))
}

fn is_dot_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    if bytes.first() != Some(&b'.') {
        return false;
    }
    if path == "." {
        return true;
    }
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'.' => {
                at += 1;
                // A dot is followed by a name or a bracket, never another
                // dot (`..` recurses).
                let start = at;
                while at < bytes.len() && (bytes[at].is_ascii_alphanumeric() || bytes[at] == b'_') {
                    at += 1;
                }
                let named = at > start;
                if named && bytes[start].is_ascii_digit() {
                    return false;
                }
                if !named && bytes.get(at) != Some(&b'[') {
                    return false;
                }
            }
            b'[' => {
                at += 1;
                while at < bytes.len() && bytes[at].is_ascii_digit() {
                    at += 1;
                }
                if bytes.get(at) != Some(&b']') {
                    return false;
                }
                at += 1;
            }
            _ => return false,
        }
    }
    true
}

/// Why the `gh` call with these words (every word after `gh`) is refused
/// for an output expression, or `None`. A `--jq`/`-q` program must be a
/// [`is_field_selector`]; `--template`, and `-t` wherever it can mean the
/// template, are refused, since a template is a program too. `-t` is the
/// title in `gh pr` and `gh issue` `create` and `edit`.
pub(crate) fn output_code(rest: &[String]) -> Option<String> {
    let title = matches!(
        (
            rest.first().map(String::as_str),
            rest.get(1).map(String::as_str)
        ),
        (Some("pr" | "issue"), Some("create" | "edit"))
    );
    let jq = |program: Option<&String>| match program {
        Some(program) if is_field_selector(program) => None,
        Some(program) => Some(format!(
            "`--jq {program}` is a program gh runs outside the sandbox with its environment"
        )),
        None => Some("`--jq` has no program".to_string()),
    };
    let mut words = rest.iter();
    while let Some(word) = words.next() {
        if word == "--" {
            break;
        }
        if let Some(long) = word.strip_prefix("--") {
            let (name, attached) = match long.split_once('=') {
                Some((name, value)) => (name, Some(value.to_string())),
                None => (long, None),
            };
            if name == "jq" {
                let program = attached.or_else(|| words.next().cloned());
                if let Some(why) = jq(program.as_ref()) {
                    return Some(why);
                }
            } else if name == "template" {
                return Some(format!(
                    "`{word}` is a template program gh runs outside the sandbox"
                ));
            }
            continue;
        }
        let Some(short) = word.strip_prefix('-').filter(|s| !s.is_empty()) else {
            continue;
        };
        // In a short option word, what follows `=` is always a value
        // (`-X=put`), so only the letters before it can be options.
        let letters = short.split('=').next().unwrap_or_default();
        if let Some(at) = letters.find('q') {
            let after = &short[at + 1..];
            let after = after.strip_prefix('=').unwrap_or(after);
            let program = if after.is_empty() {
                words.next().cloned()
            } else {
                Some(after.to_string())
            };
            if let Some(why) = jq(program.as_ref()) {
                return Some(why);
            }
            continue;
        }
        if !title && letters.contains('t') {
            return Some(format!(
                "`{word}` can be a template program gh runs outside the sandbox"
            ));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_string).collect()
    }

    fn intent(line: &str) -> Intent {
        api_intent(&words(line))
    }

    #[test]
    fn every_delete_word_is_refused_in_any_position() {
        for line in [
            "repos/o/r -X DELETE",
            "repos/o/r -X delete",
            "repos/o/r -X DeLeTe",
            "repos/o/r -XDeLeTe",
            "repos/o/r -X=DeLeTe",
            "repos/o/r -iXDELETE",
            "repos/o/r -iX=delete",
            "repos/o/r -iX dElEtE",
            "repos/o/r --method DeLeTe",
            "repos/o/r --method=dElete",
            "-X GET repos/o/r -X Delete",
            "repos/o/r -H X-HTTP-Method-Override:DELETE",
            "repos/o/r --header=x-http-method-override:delete",
            "repos/o/r -X=DeLeTe --help --help=false",
            "repos/o/r -X=DeLeTe --help=false --help",
            "repos/o/r -X GET -H X-HTTP-Method-Override:dElEtE",
            "repos/o/r -H X-HTTP-Method-Override:dElEtE -X GET",
            "repos/o/r -q -XDELETE",
        ] {
            assert_eq!(intent(line), Intent::Delete, "{line}");
        }
    }

    #[test]
    fn every_form_outside_the_plain_read_asks() {
        for line in [
            "repos/o/r/actions/runs/1/rerun -X PoSt",
            "repos/o/r -X post",
            "repos/o/r -XPuT",
            "repos/o/r -X=patch",
            "repos/o/r -iXPATCH",
            "repos/o/r --method=Post",
            "repos/o/r --method pAtCh",
            "repos/o/r -fbody=x",
            "repos/o/r -Fbody=x",
            "repos/o/r -f body=x",
            "repos/o/r --field=body=x",
            "repos/o/r --raw-field body=x",
            "repos/o/r --input body.json",
            "repos/o/r -if=x",
            "graphql -f query=x",
            "-i graphql",
            "/graphql",
            "repos/o/r --hostname evil.example",
            "https://example.invalid/review-canary",
            "repos/o/r -X OPTIONS",
            "repos/o/r -X HEAD",
            "repos/o/r -XGET",
            "repos/o/r -X GET -X POST",
            "search/issues -X GET -f q=repo:o/r",
            "repos/o/r -X PoSt --help --help=false",
            "repos/o/r -X PoSt --help=false --help",
            "repos/o/r --help",
            "repos/o/r -h",
            "--help",
            "repos/o/r -X PoSt -H X-HTTP-Method-Override:GET",
            "repos/o/r -H X-HTTP-Method-Override:GET -X PoSt",
            "repos/o/r -H Accept:application/json",
            "repos/o/r --verbose",
            "repos/o/r --paginate=false",
            "repos/o/r --metod POST",
            "repos/o/r -- other",
            "repos/o/r other/endpoint",
            "repos/o/r -X",
            "repos/o/r -q",
            "",
        ] {
            assert!(matches!(intent(line), Intent::Write(_)), "{line}");
        }
    }

    #[test]
    fn the_plain_read_form_is_a_read() {
        for line in [
            "repos/o/r",
            "repos/o/r/pulls/1 --jq .head.sha",
            "repos/o/r -X GET",
            "repos/o/r -X get",
            "repos/o/r --method=GET",
            "repos/o/r --paginate --slurp -q .name",
            "repos/o/r -i --silent --jq=.name --cache 1h",
            "repos/o/r/git/refs/heads/delete-me",
        ] {
            assert_eq!(intent(line), Intent::Read, "{line}");
        }
    }

    #[test]
    fn only_a_plain_field_selector_is_admitted() {
        for program in [
            ".",
            ".name",
            ".head.sha",
            ".[]",
            ".[].id",
            ".jobs[].databaseId",
            ".[0]",
            ".items[2].name_2",
            ".a, .b",
            ".[] | .name",
            ".env",
            " .x |.y ",
        ] {
            assert!(is_field_selector(program), "{program}");
        }
        for program in [
            "",
            "env",
            "env.GH_TOKEN",
            "$ENV",
            "$ENV.GH_TOKEN",
            "$__loc__",
            "getpath([\"a\"])",
            "env | to_entries",
            ".[env]",
            ".[\"GH_TOKEN\"]",
            ". as $x | $x",
            "\"\\(env)\"",
            "@base64",
            "{id, name}",
            "..",
            ".a..b",
            ".a.[0]x",
            "input",
            ".a?",
            "select(.a)",
            ".a // env",
            ".1a",
            "[.a]",
        ] {
            assert!(!is_field_selector(program), "{program}");
        }
    }

    #[test]
    fn output_programs_are_refused_on_every_gh_call() {
        for line in [
            "api rate_limit --jq env",
            "api rate_limit --jq=env",
            "api rate_limit -q env",
            "api rate_limit -qenv",
            "api rate_limit -iq=env",
            "api rate_limit -q $ENV",
            "pr view 458 --json number --jq env",
            "pr view 458 --json number -q .number|env",
            "run list --json name --jq getpath([\"a\"])",
            "api repos/o/r --template {{.name}}",
            "pr view 1 --json title --template={{.title}}",
            "pr view 1 --json title -t {{.title}}",
            "-R o/r pr create -t x",
            "api repos/o/r -iXpatch",
            "api repos/o/r --jq",
        ] {
            assert!(output_code(&words(line)).is_some(), "{line}");
        }
        for line in [
            "api repos/o/r --jq .name",
            "pr view 458 --json number --jq .number",
            "run view 1 --json jobs --jq .jobs[].databaseId",
            "pr create -t title -b body",
            "issue edit 1 -t title",
            "pr list --state open",
            "api repos/o/r -X=put",
            "api repos/o/r -X=quit",
        ] {
            assert_eq!(output_code(&words(line)), None, "{line}");
        }
    }

    #[test]
    fn a_word_the_shell_fills_in_is_refused() {
        for line in [
            "repos/o/r -X $M",
            "repos/o/r $FLAGS",
            "repos/o/r/pulls/$N",
            "repos/o/r -X PO$T",
            "repos/o/r -X `m`",
        ] {
            assert!(matches!(intent(line), Intent::Unread(_)), "{line}");
        }
    }

    #[test]
    fn an_option_before_api_never_reads() {
        assert_eq!(
            api_after_options(&words("-X=DeLeTe api repos/o/r")),
            Intent::Delete
        );
        assert_eq!(
            api_after_options(&words("-X DELETE api repos/o/r")),
            Intent::Delete
        );
        for line in ["-R o/r api repos/o/r", "--hostname=x api repos/o/r"] {
            assert!(
                matches!(api_after_options(&words(line)), Intent::Write(_)),
                "{line}"
            );
        }
    }

    #[test]
    fn the_token_flag_or_an_early_option_refuses_auth() {
        for line in [
            "status -t",
            "status -at",
            "status -ta",
            "status -aT",
            "status --show-token",
            "status --SHOW-TOKEN",
            "status --show-token=true",
            "status --show-token=false",
            "status -h github.com -t",
            "status -t=true",
            "--show-token=true status",
            "-t status",
            "-h github.com status",
            "token -t",
        ] {
            assert!(auth_intent(&words(line)) == Auth::Refused, "{line}");
        }
        for line in [
            "status",
            "status -a",
            "status -h github.com",
            "status --hostname=x",
        ] {
            assert_eq!(auth_intent(&words(line)), Auth::Plain, "{line}");
        }
        assert!(matches!(auth_intent(&words("status $T")), Auth::Unread(_)));
    }
}
