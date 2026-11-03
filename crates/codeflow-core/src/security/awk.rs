//! Command-line awk sources, without loading files or evaluating expressions.

#[derive(Default)]
pub(crate) struct Sources {
    /// Argument position and byte offset of each visible source.
    pub code: Vec<(usize, usize)>,
    /// Non-code option values and input operands, with their original position.
    pub arguments: Vec<(usize, String)>,
    pub files: Vec<String>,
}

/// Read the shared awk option forms conservatively. Includes add a file but
/// do not supply the main program. Once operands begin they remain data.
#[allow(clippy::too_many_lines)] // One option grammar retains argument positions.
pub(crate) fn sources(args: &[String]) -> Sources {
    let mut result = Sources::default();
    let mut supplied = false;
    let mut options = true;
    let mut at = 0;
    while let Some(word) = args.get(at) {
        let position = at;
        at += 1;
        if options && word == "--" {
            options = false;
            continue;
        }
        let mut value = None;
        if let Some(long) = word.strip_prefix("--").filter(|_| options) {
            let (name, attached) = long
                .split_once('=')
                .map_or((long, None), |(name, text)| (name, Some(text)));
            if matches!(
                name,
                "source" | "file" | "include" | "exec" | "assign" | "field-separator"
            ) {
                value = Some((name, attached.map(|text| word.len() - text.len())));
            }
        } else if let Some(short) = word
            .strip_prefix('-')
            .filter(|short| options && !short.is_empty())
        {
            for (offset, letter) in short.char_indices() {
                let end = offset + 1 + letter.len_utf8();
                let name = match letter {
                    'e' => "source",
                    'f' => "file",
                    'i' => "include",
                    'E' => "exec",
                    'v' => "assign",
                    'F' => "field-separator",
                    // -W exec is mawk's last option. GNU also accepts
                    // long options after -W, including source and file.
                    'W' => "W",
                    // GNU's optional option values consume only the tail.
                    'd' | 'D' | 'o' | 'p' | 'L' => break,
                    _ => continue,
                };
                value = Some((name, (end < word.len()).then_some(end)));
                break;
            }
        } else {
            if !supplied {
                result.code.push((position, 0));
                supplied = true;
            } else {
                result.arguments.push((position, word.clone()));
            }
            options = false;
            continue;
        }
        let Some((mut name, offset)) = value else {
            continue;
        };
        let (mut index, mut offset) = if let Some(offset) = offset {
            (position, offset)
        } else {
            let index = at;
            at += 1;
            (index, 0)
        };
        let Some(mut text) = args.get(index).and_then(|word| word.get(offset..)) else {
            continue;
        };
        if name == "W" {
            let (option, attached) = text
                .split_once('=')
                .map_or((text, None), |(n, v)| (n, Some(v)));
            if !matches!(option, "source" | "file" | "include" | "exec") {
                continue;
            }
            name = option;
            if let Some(attached) = attached {
                offset += text.len() - attached.len();
                text = attached;
            } else {
                index = at;
                at += 1;
                offset = 0;
                let Some(next) = args.get(index) else {
                    continue;
                };
                text = next;
            }
        }
        if name == "source" {
            result.code.push((index, offset));
            supplied = true;
        } else {
            result.arguments.push((index, text.to_string()));
            if matches!(name, "file" | "include" | "exec") {
                result.files.push(text.to_string());
            }
            if matches!(name, "file" | "exec") {
                supplied = true;
            }
        }
        if name == "exec" {
            options = false;
        }
    }
    result
}

#[derive(Debug, PartialEq)]
enum Token {
    Word(String),
    Literal(String),
    Punct(char),
}

/// Read quoted commands only when the entire command expression is a
/// literal (possibly parenthesized). Concatenation and variable expressions
/// remain runtime code, even when their value seems obvious to a reader.
pub(crate) fn commands(code: &str) -> Vec<String> {
    let tokens = tokens(code);
    let mut commands = Vec::new();
    for (at, token) in tokens.iter().enumerate() {
        if matches!(token, Token::Word(word) if word == "system")
            && tokens.get(at + 1) == Some(&Token::Punct('('))
        {
            if let Some((command, end)) = literal(&tokens, at + 2) {
                if tokens.get(end) == Some(&Token::Punct(')')) {
                    commands.push(command.to_string());
                }
            }
        }
        if token != &Token::Punct('|') {
            continue;
        }
        if tokens.get(at + 1) == Some(&Token::Punct('|'))
            || at.checked_sub(1).and_then(|at| tokens.get(at)) == Some(&Token::Punct('|'))
        {
            continue;
        }
        let after = at + 1 + usize::from(tokens.get(at + 1) == Some(&Token::Punct('&')));
        if matches!(tokens.get(after), Some(Token::Word(word)) if word == "getline") {
            if let Some(command) = literal_before(&tokens, at) {
                commands.push(command.to_string());
            }
        } else if let Some((command, end)) = literal(&tokens, after) {
            if tokens
                .get(end)
                .is_none_or(|token| matches!(token, Token::Punct(';' | '\n' | '}' | ')')))
            {
                commands.push(command.to_string());
            }
        }
    }
    commands
}

fn expression_start(tokens: &[Token], at: usize) -> bool {
    at == 0
        || matches!(
            tokens[at - 1],
            Token::Punct('{' | ';' | '\n' | '(' | '=' | ',')
        )
}

fn literal_before(tokens: &[Token], mut end: usize) -> Option<&str> {
    let mut parens = 0;
    while end > 0 && tokens[end - 1] == Token::Punct(')') {
        parens += 1;
        end -= 1;
    }
    let Token::Literal(text) = tokens.get(end.checked_sub(1)?)? else {
        return None;
    };
    end -= 1;
    for _ in 0..parens {
        if end == 0 || tokens[end - 1] != Token::Punct('(') {
            return None;
        }
        end -= 1;
    }
    expression_start(tokens, end).then_some(text)
}

fn literal(tokens: &[Token], at: usize) -> Option<(&str, usize)> {
    let mut end = at;
    while tokens.get(end) == Some(&Token::Punct('(')) {
        end += 1;
    }
    let parens = end - at;
    let Token::Literal(text) = tokens.get(end)? else {
        return None;
    };
    end += 1;
    for _ in 0..parens {
        if tokens.get(end) != Some(&Token::Punct(')')) {
            return None;
        }
        end += 1;
    }
    Some((text, end))
}

pub(crate) fn literals(code: &str) -> Vec<String> {
    tokens(code)
        .into_iter()
        .filter_map(|token| match token {
            Token::Literal(text) => Some(text),
            _ => None,
        })
        .collect()
}

#[allow(clippy::too_many_lines)] // One lexer keeps string, comment and regexp states together.
fn tokens(code: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut chars = code.chars().peekable();
    let mut operand = true;
    while let Some(ch) = chars.next() {
        match ch {
            '#' => {
                while chars.peek().is_some_and(|ch| *ch != '\n') {
                    chars.next();
                }
            }
            '"' => {
                let mut text = String::new();
                let mut closed = false;
                while let Some(ch) = chars.next() {
                    if ch == '"' {
                        closed = true;
                        break;
                    }
                    if ch != '\\' {
                        text.push(ch);
                        continue;
                    }
                    let Some(escape) = chars.next() else { break };
                    match escape {
                        '\n' => {}
                        'n' => text.push('\n'),
                        'r' => text.push('\r'),
                        't' => text.push('\t'),
                        'b' => text.push('\u{8}'),
                        'f' => text.push('\u{c}'),
                        'v' => text.push('\u{b}'),
                        '0'..='7' | 'x' => {
                            let radix = if escape == 'x' { 16 } else { 8 };
                            let mut digits = if escape == 'x' {
                                String::new()
                            } else {
                                escape.to_string()
                            };
                            let limit = if escape == 'x' { 2 } else { 3 };
                            while digits.len() < limit
                                && chars.peek().is_some_and(|ch| ch.is_digit(radix))
                            {
                                digits.push(chars.next().unwrap());
                            }
                            if let Ok(value) = u32::from_str_radix(&digits, radix) {
                                if let Some(ch) = char::from_u32(value) {
                                    text.push(ch);
                                }
                            }
                        }
                        other => text.push(other),
                    }
                }
                if closed {
                    tokens.push(Token::Literal(text));
                }
                operand = false;
            }
            '/' if operand => {
                // A regexp is data, including any apparent calls inside it.
                let mut bracket = false;
                while let Some(ch) = chars.next() {
                    match ch {
                        '\\' => {
                            chars.next();
                        }
                        '[' => bracket = true,
                        ']' => bracket = false,
                        '/' if !bracket => break,
                        _ => {}
                    }
                }
                tokens.push(Token::Word(String::new()));
                operand = false;
            }
            ch if ch.is_alphanumeric() || ch == '_' => {
                let mut word = ch.to_string();
                while chars
                    .peek()
                    .is_some_and(|ch| ch.is_alphanumeric() || *ch == '_')
                {
                    word.push(chars.next().unwrap());
                }
                operand = matches!(word.as_str(), "print" | "printf" | "return");
                tokens.push(Token::Word(word));
            }
            '\\' if chars.peek() == Some(&'\n') => {
                chars.next();
            }
            ch if ch.is_whitespace() && ch != '\n' => {}
            ch => {
                tokens.push(Token::Punct(ch));
                operand = !matches!(ch, ')' | ']');
            }
        }
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_sources_keep_options_files_and_operands_distinct() {
        for (args, code, files, operands) in [
            (
                vec!["-F,", "{print $2}", "notes.csv"],
                vec!["{print $2}"],
                vec![],
                vec![",", "notes.csv"],
            ),
            (
                vec!["-i", "lib.awk", "{print}", "data.txt"],
                vec!["{print}"],
                vec!["lib.awk"],
                vec!["lib.awk", "data.txt"],
            ),
            (
                vec!["-f", "prog.awk", "data.txt"],
                vec![],
                vec!["prog.awk"],
                vec!["prog.awk", "data.txt"],
            ),
            (
                vec!["-eBEGIN{}", "--source=END{}", "-fother.awk", "data.txt"],
                vec!["BEGIN{}", "END{}"],
                vec!["other.awk"],
                vec!["other.awk", "data.txt"],
            ),
            (
                vec!["-W", "exec", "prog.awk", "-e", "operand"],
                vec![],
                vec!["prog.awk"],
                vec!["prog.awk", "-e", "operand"],
            ),
            (
                vec!["--", "{print}", "-e", "operand"],
                vec!["{print}"],
                vec![],
                vec!["-e", "operand"],
            ),
        ] {
            let args: Vec<String> = args.into_iter().map(str::to_string).collect();
            let source = sources(&args);
            let actual: Vec<_> = source
                .code
                .iter()
                .map(|(at, offset)| &args[*at][*offset..])
                .collect();
            assert_eq!(actual, code, "{args:?}");
            assert_eq!(source.files, files, "{args:?}");
            assert_eq!(
                source
                    .arguments
                    .iter()
                    .map(|(_, text)| text.as_str())
                    .collect::<Vec<_>>(),
                operands,
                "{args:?}"
            );
        }
    }

    #[test]
    fn command_literals_exclude_comments_regexps_and_runtime_expressions() {
        for (code, expected) in [
            (
                r#"BEGIN { system("tee out"); print "x" | "cat"; "wc -l" | getline n }"#,
                vec!["tee out", "cat", "wc -l"],
            ),
            (
                r#"BEGIN { system((("cat"))); ("wc") | getline n; print 1 | ("cat") }"#,
                vec!["cat", "wc", "cat"],
            ),
            (
                r#"BEGIN { system("te\145 out"); print 1 | "\x63at" }"#,
                vec!["tee out", "cat"],
            ),
            ("# system(\"cat\")\n/system(\"cat\")/ { print 1 }", vec![]),
            (
                r#"BEGIN { cmd="cat"; system(cmd); print 1 | cmd; cmd | getline n }"#,
                vec![],
            ),
            (
                r#"BEGIN { system("ca" "t"); print 1 | "ca" "t"; "ca" "t" | getline n }"#,
                vec![],
            ),
            (
                r#"BEGIN { if (x || "cat") print 1; x / system("cat") }"#,
                vec!["cat"],
            ),
        ] {
            assert_eq!(commands(code), expected, "{code}");
        }
    }
}
