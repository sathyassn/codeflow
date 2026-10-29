//! The display boundary of `codeflow update` and `codeflow update --pin`
//! (TSK-1029).
//!
//! Every report line, note, warning and error these commands print or
//! export is a [`Shown`]. Its text is private and there is no conversion
//! from a string, so a `Shown` is built only from these fragments:
//!
//! - text compiled into the binary: [`Shown::fixed`] and the format string
//!   of [`shown!`](crate::shown) take `&'static str`;
//! - a count or number ([`Shown::count`], [`Shown::number`]);
//! - a hash or object id the binary computed ([`Shown::sha256_of`],
//!   [`Shown::object_id`]);
//! - a path ([`Shown::path`]), a parse location ([`Shown::place`]) or an
//!   I/O failure category ([`Shown::io`]);
//! - a version or a date, reduced to their digits and fixed suffixes
//!   ([`Shown::version`], [`Shown::iso_date`]);
//! - text the binary ships: [`Shown::shipped`] returns a slice of a
//!   [`Shipped`] text, which only the asset reader or a compiled-in
//!   `&'static str` makes, and [`Shown::defined`] returns one of a list of
//!   `&'static str` names.
//!
//! `shown!` joins `Shown` arguments into a `&'static str` format through
//! [`Shown::format`]; no `fmt::Arguments` and no runtime format string
//! enter. The guarantee rests on two things the compiler does not check: a
//! `&'static str` is made only from a literal or compiled-in text (no
//! `leak`), and the [`super::AssetSource`] the binary passes is its own
//! embedded tree. The scan test `update_display_boundary` checks the first
//! and lists the `Shown::path` sites.

use std::fmt;

use super::assets::Shipped;

/// Text `codeflow update` and `codeflow update --pin` may print or export.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Shown(String);

impl Shown {
    /// Text compiled into the binary.
    #[must_use]
    pub fn fixed(text: &'static str) -> Self {
        Self(text.to_string())
    }

    /// A number of things.
    #[must_use]
    pub fn count(count: usize) -> Self {
        Self(count.to_string())
    }

    /// A number, such as a byte limit.
    #[must_use]
    pub fn number(number: u64) -> Self {
        Self(number.to_string())
    }

    /// A git object id, as git computed it: a hash, never text a project
    /// chose.
    #[must_use]
    pub fn object_id(id: git2::Oid) -> Self {
        Self(id.to_string())
    }

    /// A path: a file or folder the command reads or writes. The contract
    /// allows a path; it is the place a person opens to see the problem.
    #[must_use]
    pub fn path(path: impl AsRef<std::path::Path>) -> Self {
        Self(path.as_ref().display().to_string())
    }

    /// Where a file failed to parse: format, a fixed kind, line and column.
    #[must_use]
    pub fn place(place: &super::ParsePlace) -> Self {
        Self(place.to_string())
    }

    /// The category of an I/O failure, never its message, which a custom
    /// error can fill with any text.
    #[must_use]
    pub fn io(error: &std::io::Error) -> Self {
        Self(error.kind().to_string())
    }

    /// A version under the version display rule (`version::shown`).
    #[must_use]
    pub fn version(version: &str) -> Self {
        Self(super::version::shown(version))
    }

    /// The part of `shipped` that reads as `text`, when the shipped text
    /// holds it as written; else `None`. The result is copied from the
    /// shipped text, so `text` only chooses which part.
    #[must_use]
    pub fn shipped(text: &str, shipped: Shipped<'_>) -> Option<Self> {
        let shipped = shipped.as_str();
        if text.is_empty() {
            return None;
        }
        let start = shipped.find(text)?;
        Some(Self(shipped[start..start + text.len()].to_string()))
    }

    /// The name in `defined`, names `CodeFlow` compiles in (such as the
    /// default policy's required PR sections), that equals `text` ignoring
    /// ASCII case; else `None`.
    #[must_use]
    pub fn defined(text: &str, defined: &[&'static str]) -> Option<Self> {
        defined
            .iter()
            .find(|name| name.eq_ignore_ascii_case(text))
            .map(|name| Self::fixed(name))
    }

    /// A calendar date in `YYYY-MM-DD` form, such as the binary's own
    /// clock gives; any other text is `None`.
    #[must_use]
    pub fn iso_date(date: &str) -> Option<Self> {
        let bytes = date.as_bytes();
        let shaped = bytes.len() == 10
            && bytes.iter().enumerate().all(|(index, byte)| match index {
                4 | 7 => *byte == b'-',
                _ => byte.is_ascii_digit(),
            });
        shaped.then(|| Self(date.to_string()))
    }

    /// The SHA-256 of `bytes`, as this binary computes it: a hash of what
    /// it read, never text a project wrote.
    #[must_use]
    pub fn sha256_of(bytes: &[u8]) -> Self {
        Self(super::hash::sha256_hex(bytes))
    }

    /// The parts joined by `separator`.
    #[must_use]
    pub fn join(parts: impl IntoIterator<Item = Shown>, separator: &'static str) -> Self {
        Self(
            parts
                .into_iter()
                .map(|part| part.0)
                .collect::<Vec<_>>()
                .join(separator),
        )
    }

    /// The text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The text, to hand to a printer or a file.
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }

    /// Appends `other`.
    pub fn push(&mut self, other: &Shown) {
        self.0.push_str(&other.0);
    }

    /// `format` with each `{}` replaced by the next of `parts`, `{:<N}` and
    /// `{:>N}` padding it to `N` characters, and `{{` and `}}` written as
    /// braces. The format is compiled-in text and every part is a `Shown`,
    /// so the result is one. This is what [`shown!`](crate::shown) calls.
    ///
    /// # Panics
    ///
    /// On a placeholder of another form, or a count of parts that does not
    /// match the placeholders: a mistake in the binary's own format string.
    #[must_use]
    pub fn format(format: &'static str, parts: &[&Shown]) -> Self {
        let mut out = String::with_capacity(format.len());
        let mut parts = parts.iter();
        let mut rest = format;
        while let Some(index) = rest.find(['{', '}']) {
            out.push_str(&rest[..index]);
            rest = &rest[index..];
            if let Some(after) = rest.strip_prefix("{{") {
                out.push('{');
                rest = after;
            } else if let Some(after) = rest.strip_prefix("}}") {
                out.push('}');
                rest = after;
            } else if rest.starts_with('{') {
                let end = rest.find('}').expect("shown!: unclosed placeholder");
                let spec = &rest[1..end];
                let part = parts.next().expect("shown!: more placeholders than parts");
                pad(&mut out, &part.0, spec);
                rest = &rest[end + 1..];
            } else {
                panic!("shown!: a lone closing brace in the format");
            }
        }
        out.push_str(rest);
        assert!(
            parts.next().is_none(),
            "shown!: more parts than placeholders"
        );
        Self(out)
    }
}

/// Writes `text` padded as the placeholder `spec` asks: empty, `:<N` or
/// `:>N`.
fn pad(out: &mut String, text: &str, spec: &str) {
    if spec.is_empty() {
        return out.push_str(text);
    }
    let width = |digits: &str| digits.parse::<usize>().ok();
    let (left, width) = match (spec.strip_prefix(":<"), spec.strip_prefix(":>")) {
        (Some(digits), _) => (true, width(digits)),
        (_, Some(digits)) => (false, width(digits)),
        _ => (true, None),
    };
    let width = width.expect("shown!: placeholders are {}, {:<N} or {:>N}");
    let fill = " ".repeat(width.saturating_sub(text.chars().count()));
    if left {
        out.push_str(text);
        out.push_str(&fill);
    } else {
        out.push_str(&fill);
        out.push_str(text);
    }
}

/// A [`Shown`] from a format string and `Shown` arguments ([`Shown::format`]).
/// The format string is a literal; it uses `{}`, `{:<N}` and `{:>N}`
/// placeholders only. An argument that is not a `Shown` does not compile.
#[macro_export]
macro_rules! shown {
    ($format:literal $(, $argument:expr)* $(,)?) => {
        $crate::scaffold::shown::Shown::format($format, &[$(&$argument),*])
    };
}

impl fmt::Display for Shown {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `pad` keeps width and alignment specifiers working.
        f.pad(&self.0)
    }
}

impl std::ops::Deref for Shown {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for Shown {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::borrow::Borrow<str> for Shown {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl PartialEq<str> for Shown {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<String> for Shown {
    fn eq(&self, other: &String) -> bool {
        &self.0 == other
    }
}

impl PartialEq<&str> for Shown {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

impl From<&super::ScaffoldError> for Shown {
    /// A scaffold error's text: every text-bearing variant holds a `Shown`,
    /// a path or a parse place, so its display is inside the boundary.
    fn from(error: &super::ScaffoldError) -> Self {
        Self(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_macro_takes_only_shown_arguments_and_keeps_padding() {
        let line = crate::shown!(
            "{:<6}|{} of {} {{{:>3}}}",
            Shown::fixed("ab"),
            Shown::count(3),
            Shown::path("a/b.md"),
            Shown::count(7)
        );
        assert_eq!(line.as_str(), "ab    |3 of a/b.md {  7}");
    }

    #[test]
    #[should_panic(expected = "placeholders are")]
    fn a_named_placeholder_is_refused() {
        let _ = Shown::format("{secret}", &[&Shown::fixed("x")]);
    }

    #[test]
    fn shipped_text_needs_the_asset_to_hold_it() {
        let asset = Shipped::compiled(r#"{"allow": ["Read"]}"#);
        assert_eq!(Shown::shipped("Read", asset).unwrap().as_str(), "Read");
        assert!(Shown::shipped("opaque-31337", asset).is_none());
        assert!(Shown::shipped("", asset).is_none());
        assert_eq!(
            Shown::defined("summary", &["Summary"]).unwrap().as_str(),
            "Summary"
        );
    }

    #[test]
    fn an_io_error_shows_its_kind_never_its_message() {
        let error = std::io::Error::other("opaque-31337");
        assert_eq!(Shown::io(&error).as_str(), "other error");
    }
}
