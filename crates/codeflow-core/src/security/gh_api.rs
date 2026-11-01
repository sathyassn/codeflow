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
//! - a word the shell fills in is refused, since it could be any of these.

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
const READ_VALUE: &[&str] = &["-q", "--jq", "-t", "--template", "--cache"];
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
            || ["--jq=", "--template=", "--cache="]
                .iter()
                .any(|p| word.starts_with(p))
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
            "repos/o/r -i --silent --jq=.name --template={{.name}} --cache 1h",
            "repos/o/r/git/refs/heads/delete-me",
        ] {
            assert_eq!(intent(line), Intent::Read, "{line}");
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
