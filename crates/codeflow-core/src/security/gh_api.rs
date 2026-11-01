//! What a `gh api` or `gh auth status` call does, read the way gh's own
//! flag parser (pflag) reads its arguments (TSK-1070 security review round
//! 2). Claude Code runs these subcommands outside its sandbox by name, and
//! its permission rules match command text, so a list of spellings never
//! closes: `-X PoSt`, `-X=DeLeTe` and `-iXDELETE` each reached the API past
//! the text rules. Exec-guard reads the arguments here instead, with every
//! form pflag accepts: a separated value, an attached value, `=` after a
//! short or long name, short-option bundles, and the method in any case,
//! since gh upper-cases it before sending. A form this reader cannot be sure
//! of, such as a word the shell fills in or an option gh does not have, is
//! refused rather than guessed.

/// What one `gh api` call does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Intent {
    /// A GET or HEAD to the configured GitHub host.
    Read,
    /// A call that changes something or reaches another host; the reason
    /// names which. Claude asks the person before it runs.
    Write(String),
    /// A DELETE in any spelling: refused.
    Delete,
    /// A form the reader cannot be sure of: refused, with the reason.
    Unread(String),
}

/// `gh api` short options that take a value (gh 2.x `gh api --help`).
const API_VALUE_SHORT: &[char] = &['F', 'H', 'X', 'f', 'p', 'q', 't'];
/// `gh api` short options that take none.
const API_FLAG_SHORT: &[char] = &['i', 'h'];
/// `gh api` long options that take a value.
const API_VALUE_LONG: &[&str] = &[
    "cache",
    "field",
    "header",
    "hostname",
    "input",
    "jq",
    "method",
    "preview",
    "raw-field",
    "template",
];
/// `gh api` long options that take none.
const API_FLAG_LONG: &[&str] = &["include", "paginate", "silent", "slurp", "verbose", "help"];

/// One option as pflag reads it: its canonical long name and its value
/// (`None` for a flag given no `=` value).
struct ReadOption {
    name: &'static str,
    value: Option<String>,
}

fn api_long(short: char) -> &'static str {
    match short {
        'F' => "field",
        'H' => "header",
        'X' => "method",
        'f' => "raw-field",
        'p' => "preview",
        'q' => "jq",
        't' => "template",
        'i' => "include",
        _ => "help",
    }
}

/// Split `args` into options and positional words the way pflag does for a
/// command with the given options. `Err` names a form pflag would read
/// differently from this reader, or one gh rejects.
#[allow(clippy::type_complexity)]
fn parse(
    args: &[String],
    value_short: &[char],
    flag_short: &[char],
    value_long: &[&'static str],
    flag_long: &[&'static str],
    long_of: fn(char) -> &'static str,
) -> Result<(Vec<ReadOption>, Vec<String>), String> {
    let mut options = Vec::new();
    let mut positional = Vec::new();
    let mut words = args.iter();
    while let Some(word) = words.next() {
        if crate::hooks::git_guard::expands(word) {
            return Err(format!(
                "`{word}` is filled in by the shell, so the guard cannot see what gh receives"
            ));
        }
        if word == "--" {
            for rest in words.by_ref() {
                if crate::hooks::git_guard::expands(rest) {
                    return Err(format!(
                        "`{rest}` is filled in by the shell, so the guard cannot see what gh receives"
                    ));
                }
                positional.push(rest.clone());
            }
            break;
        }
        if let Some(long) = word.strip_prefix("--") {
            let (name, attached) = match long.split_once('=') {
                Some((name, value)) => (name, Some(value.to_string())),
                None => (long, None),
            };
            if let Some(&canonical) = value_long.iter().find(|n| **n == name) {
                let value = match attached {
                    Some(value) => value,
                    None => next_value(&mut words, word)?,
                };
                options.push(ReadOption {
                    name: canonical,
                    value: Some(value),
                });
            } else if let Some(&canonical) = flag_long.iter().find(|n| **n == name) {
                options.push(ReadOption {
                    name: canonical,
                    value: attached,
                });
            } else {
                return Err(format!("`--{name}` is not an option of this gh command"));
            }
            continue;
        }
        let Some(shorts) = word.strip_prefix('-').filter(|s| !s.is_empty()) else {
            positional.push(word.clone());
            continue;
        };
        let chars: Vec<char> = shorts.chars().collect();
        let mut at = 0;
        while let Some(&letter) = chars.get(at) {
            let rest: String = chars[at + 1..].iter().collect();
            if value_short.contains(&letter) {
                let value = if let Some(value) = rest.strip_prefix('=') {
                    value.to_string()
                } else if !rest.is_empty() {
                    rest
                } else {
                    next_value(&mut words, word)?
                };
                options.push(ReadOption {
                    name: long_of(letter),
                    value: Some(value),
                });
                break;
            }
            if flag_short.contains(&letter) {
                if let Some(value) = rest.strip_prefix('=') {
                    options.push(ReadOption {
                        name: long_of(letter),
                        value: Some(value.to_string()),
                    });
                    break;
                }
                options.push(ReadOption {
                    name: long_of(letter),
                    value: None,
                });
                at += 1;
                continue;
            }
            return Err(format!(
                "`-{letter}` in `{word}` is not an option of this gh command"
            ));
        }
    }
    Ok((options, positional))
}

fn next_value<'a>(
    words: &mut impl Iterator<Item = &'a String>,
    option: &str,
) -> Result<String, String> {
    let value = words
        .next()
        .ok_or_else(|| format!("`{option}` has no value"))?;
    if crate::hooks::git_guard::expands(value) {
        return Err(format!(
            "the value of `{option}` is filled in by the shell, so the guard cannot see it"
        ));
    }
    Ok(value.clone())
}

/// What the `gh api` call with these arguments (the words after `api`)
/// does.
pub(crate) fn api_intent(args: &[String]) -> Intent {
    let (options, positional) = match parse(
        args,
        API_VALUE_SHORT,
        API_FLAG_SHORT,
        API_VALUE_LONG,
        API_FLAG_LONG,
        api_long,
    ) {
        Ok(parsed) => parsed,
        Err(why) => return Intent::Unread(why),
    };
    if options
        .iter()
        .any(|o| o.name == "help" && o.value.as_deref().is_none_or(truthy))
    {
        return Intent::Read;
    }
    let mut methods = Vec::new();
    for option in &options {
        let value = option.value.as_deref().unwrap_or_default();
        match option.name {
            "method" => methods.push(value.to_string()),
            // A method override header changes the method the API applies.
            "header" => {
                if let Some((name, rest)) = value.split_once(':') {
                    if name.trim().eq_ignore_ascii_case("x-http-method-override") {
                        methods.push(rest.trim().to_string());
                    }
                }
            }
            _ => {}
        }
    }
    for method in &methods {
        if method.is_empty() || !method.chars().all(|c| c.is_ascii_alphabetic()) {
            return Intent::Unread(format!("the method `{method}` is not a plain HTTP method"));
        }
        if method.eq_ignore_ascii_case("DELETE") {
            return Intent::Delete;
        }
    }
    let endpoint = match positional.as_slice() {
        [] => return Intent::Read,
        [endpoint] => endpoint,
        [_, extra, ..] => {
            return Intent::Unread(format!(
                "`{extra}` is a second endpoint; gh takes one, so the guard cannot tell which runs"
            ))
        }
    };
    let body = options
        .iter()
        .any(|o| matches!(o.name, "field" | "raw-field" | "input"));
    let method = methods.last().map_or_else(
        || if body { "POST" } else { "GET" }.to_string(),
        |m| m.to_ascii_uppercase(),
    );
    if endpoint.contains("://") || endpoint.starts_with("//") {
        return Intent::Write(format!(
            "names its own host in `{endpoint}`, so the token goes where the URL points"
        ));
    }
    if options.iter().any(|o| o.name == "hostname") {
        return Intent::Write("sends the token to the host `--hostname` names".to_string());
    }
    if endpoint
        .trim_start_matches('/')
        .eq_ignore_ascii_case("graphql")
    {
        return Intent::Write(
            "is a GraphQL call, where a mutation and a query share one form".to_string(),
        );
    }
    if matches!(method.as_str(), "GET" | "HEAD") {
        Intent::Read
    } else {
        Intent::Write(format!("sends a {method} request"))
    }
}

/// pflag's reading of a boolean flag's `=` value.
fn truthy(value: &str) -> bool {
    matches!(value, "1" | "t" | "T" | "true" | "TRUE" | "True")
}

fn auth_long(short: char) -> &'static str {
    match short {
        'a' => "active",
        't' => "show-token",
        'h' => "hostname",
        _ => "help",
    }
}

/// Whether `gh auth status` with these arguments (the words after
/// `status`) prints the token: `-t` or `--show-token` in any bundle or `=`
/// form. `Err` names a form the reader cannot be sure of.
pub(crate) fn auth_status_shows_token(args: &[String]) -> Result<bool, String> {
    let (options, _) = parse(
        args,
        &['h'],
        &['a', 't'],
        &["hostname"],
        &["active", "show-token", "help"],
        auth_long,
    )?;
    Ok(options
        .iter()
        .any(|o| o.name == "show-token" && o.value.as_deref().is_none_or(truthy)))
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
    fn every_delete_spelling_is_a_delete() {
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
        ] {
            assert_eq!(intent(line), Intent::Delete, "{line}");
        }
    }

    #[test]
    fn writes_are_read_in_every_spelling_and_case() {
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
        ] {
            assert!(matches!(intent(line), Intent::Write(_)), "{line}");
        }
    }

    #[test]
    fn reads_stay_reads() {
        for line in [
            "repos/o/r",
            "repos/o/r/pulls/1 --jq .head.sha",
            "repos/o/r -X GET",
            "repos/o/r -Xget",
            "search/issues -X GET -f q=repo:o/r",
            "repos/o/r --paginate --slurp -q .[]",
            "repos/o/r -i",
            "repos/o/r -X HEAD",
            "--help",
        ] {
            assert_eq!(intent(line), Intent::Read, "{line}");
        }
    }

    #[test]
    fn forms_the_reader_cannot_be_sure_of_are_refused() {
        for line in [
            "repos/o/r -X $M",
            "repos/o/r $FLAGS",
            "repos/o/r/pulls/$N",
            "repos/o/r -Z",
            "repos/o/r --metod POST",
            "repos/o/r --METHOD POST",
            "repos/o/r -X",
            "repos/o/r -X PO$T",
            "repos/o/r -X 'POST",
            "repos/o/r other/endpoint",
        ] {
            assert!(matches!(intent(line), Intent::Unread(_)), "{line}");
        }
    }

    #[test]
    fn the_token_flag_is_read_in_every_form() {
        for line in [
            "-t",
            "-at",
            "-ta",
            "--show-token",
            "--show-token=true",
            "-h github.com -t",
            "-t=true",
        ] {
            assert_eq!(auth_status_shows_token(&words(line)), Ok(true), "{line}");
        }
        for line in [
            "",
            "-a",
            "-h github.com",
            "-ht",
            "--show-token=false",
            "--hostname=x",
        ] {
            assert_eq!(auth_status_shows_token(&words(line)), Ok(false), "{line}");
        }
        assert!(auth_status_shows_token(&words("-x")).is_err());
        assert!(auth_status_shows_token(&words("$T")).is_err());
    }
}
