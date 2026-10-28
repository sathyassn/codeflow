//! The display boundary of `codeflow update` and `codeflow update --pin`
//! (TSK-1029).
//!
//! Every report line, note, warning and error these commands print or
//! export is a [`Shown`]. A `Shown` is built only from a closed set of
//! fragments, none of which can carry a value, key or line a project
//! wrote:
//!
//! - text compiled into the binary ([`Shown::fixed`], and the format string
//!   of [`shown!`](crate::shown), which takes only `Shown` arguments);
//! - a count ([`Shown::count`]);
//! - a path ([`Shown::path`]);
//! - a parse location ([`Shown::place`]);
//! - an I/O failure category ([`Shown::io`]);
//! - a version under the version display rule ([`Shown::version`]);
//! - text the binary's embedded assets hold ([`Shown::shipped`]).
//!
//! There is no conversion from `String` or `&str`, and the inner text is
//! private, so the compiler refuses any other construction. An error from a
//! lower layer is converted to these fragments where it enters a sink. The
//! scan test `update_display_boundary` checks what the type cannot: that a
//! `shown!` format string captures no variable, and that each call of the
//! two constructors that take runtime text (`path` and `shipped`) is
//! listed with what feeds it.

use std::fmt;

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

    /// `text` when `shipped`, the bytes of an asset embedded in the binary
    /// before any templating, holds it as written; else `None`.
    #[must_use]
    pub fn shipped(text: &str, shipped: &str) -> Option<Self> {
        (!text.is_empty() && shipped.contains(text)).then(|| Self(text.to_string()))
    }

    /// `text` when it equals one of `defined`, names `CodeFlow` defines (such
    /// as the default policy's required PR sections), else `None`.
    #[must_use]
    pub fn defined<'a>(text: &str, defined: impl IntoIterator<Item = &'a str>) -> Option<Self> {
        defined
            .into_iter()
            .find(|name| name.eq_ignore_ascii_case(text))
            .map(|name| Self(name.to_string()))
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

    /// Builds the text of [`shown!`](crate::shown). Called only by that
    /// macro, whose arguments must be `Shown` (the scan test checks the
    /// callers).
    #[doc(hidden)]
    #[must_use]
    pub fn from_shown_macro(args: fmt::Arguments<'_>) -> Self {
        Self(fmt::format(args))
    }

    /// Passes a macro argument through, so it must be a `Shown`.
    #[doc(hidden)]
    #[must_use]
    pub fn macro_argument(argument: &Shown) -> &Shown {
        argument
    }
}

/// A [`Shown`] from a format string and `Shown` arguments. The format string
/// is text compiled into the binary; it uses positional `{}` placeholders
/// only, never a captured variable (the scan test checks).
#[macro_export]
macro_rules! shown {
    ($format:literal $(, $argument:expr)* $(,)?) => {
        $crate::scaffold::shown::Shown::from_shown_macro(format_args!(
            $format
            $(, $crate::scaffold::shown::Shown::macro_argument(&$argument))*
        ))
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
            "{:<6}|{} of {}",
            Shown::fixed("ab"),
            Shown::count(3),
            Shown::path("a/b.md")
        );
        assert_eq!(line.as_str(), "ab    |3 of a/b.md");
    }

    #[test]
    fn shipped_text_needs_the_asset_to_hold_it() {
        assert!(Shown::shipped("Read", r#"{"allow": ["Read"]}"#).is_some());
        assert!(Shown::shipped("opaque-31337", r#"{"allow": ["Read"]}"#).is_none());
        assert!(Shown::shipped("", "anything").is_none());
    }

    #[test]
    fn an_io_error_shows_its_kind_never_its_message() {
        let error = std::io::Error::other("opaque-31337");
        assert_eq!(Shown::io(&error).as_str(), "other error");
    }
}
