//! Constrain authored style rules to descendants of a runtime-owned HTML host.

use cssparser::{
    AtRuleParser, ParseError, Parser, ParserInput, ParserState, QualifiedRuleParser,
    StyleSheetParser, Token,
};

use crate::{PresentError, Result};

pub(crate) fn scope_stylesheet(css: &str, host: &str) -> Result<String> {
    let mut source = ParserInput::new(css);
    let mut parser = Parser::new(&mut source);
    let mut rules = ScopedRules { host };
    let mut output = String::new();
    for rule in StyleSheetParser::new(&mut parser, &mut rules) {
        output.push_str(&rule.map_err(|_| {
            PresentError::InvalidDocument(
                "html styles require balanced selectors and flat declaration blocks".into(),
            )
        })?);
    }
    Ok(output)
}

struct ScopedRules<'a> {
    host: &'a str,
}

impl AtRuleParser<'_> for ScopedRules<'_> {
    type Prelude = ();
    type AtRule = String;
    type Error = ();
}

impl<'i> QualifiedRuleParser<'i> for ScopedRules<'_> {
    type Prelude = Vec<String>;
    type QualifiedRule = String;
    type Error = ();

    fn parse_prelude<'t>(
        &mut self,
        input: &mut Parser<'i, 't>,
    ) -> std::result::Result<Vec<String>, ParseError<'i, ()>> {
        input.parse_comma_separated(|selector| {
            let state = selector.state();
            if matches!(selector.next()?, Token::Delim('+' | '>' | '~' | '|' | '&')) {
                return Err(selector.new_custom_error(()));
            }
            selector.reset(&state);
            consume_tokens(selector, 0)?;
            Ok(selector.slice_from(state.position()).trim().to_owned())
        })
    }

    fn parse_block<'t>(
        &mut self,
        selectors: Vec<String>,
        _: &ParserState,
        input: &mut Parser<'i, 't>,
    ) -> std::result::Result<String, ParseError<'i, ()>> {
        let start = input.position();
        consume_tokens(input, 0)?;
        // Every top-level selector gets a prefix. Parser-level splitting
        // preserves nested commas and pseudo-elements without letting later
        // comma-separated selectors escape the host.
        let scoped = selectors
            .iter()
            .map(|selector| format!("#{} {selector}", self.host))
            .collect::<Vec<_>>()
            .join(", ");
        Ok(format!("{scoped} {{{}}}", input.slice_from(start)))
    }
}

fn consume_tokens<'i>(
    input: &mut Parser<'i, '_>,
    depth: usize,
) -> std::result::Result<(), ParseError<'i, ()>> {
    if depth > 32 {
        return Err(input.new_custom_error(()));
    }
    while !input.is_exhausted() {
        let token = input.next()?.clone();
        if token.is_parse_error() {
            return Err(input.new_custom_error(()));
        }
        match token {
            Token::CurlyBracketBlock | Token::AtKeyword(_) | Token::Delim('&') => {
                return Err(input.new_custom_error(()))
            }
            Token::Function(_) | Token::ParenthesisBlock | Token::SquareBracketBlock => {
                input.parse_nested_block(|nested| consume_tokens(nested, depth + 1))?;
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scopes_entire_selector_list_and_preserves_strings() {
        let css = "p::before, body, :is(.x, .y) { color:red; content:'} )'; }";
        assert_eq!(
            scope_stylesheet(css, "cf-host").unwrap(),
            "#cf-host p::before, #cf-host body, #cf-host :is(.x, .y) { color:red; content:'} )';}"
        );
    }

    #[test]
    fn rejects_unmatched_tokens_and_nested_rules() {
        for css in [
            "p) {color:red}",
            "p] {color:red}",
            "} body {display:none}",
            "+ body {display:none}",
            "p, ~ body {display:none}",
            "p { .nested { color:red } }",
            "@media all {body{display:none}}",
            "p { content: 'broken\n; }",
        ] {
            assert!(scope_stylesheet(css, "cf-host").is_err(), "accepted {css}");
        }
    }
}
