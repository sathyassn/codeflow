//! Terminal output escaping for this crate (TSK-388).
//!
//! The same rule as `codeflow_core::git::display_output` (this crate does
//! not depend on `codeflow-core`): each control, format and separator character other
//! than a line break or tab is written as `\xNN` (up to U+00FF) or
//! `\u{NNNN}`; backslashes are kept. The CLI's tests compare the two over
//! every character in the escaped ranges, so they cannot drift apart.

use std::fmt::Write as _;

/// `Default_Ignorable_Code_Point` and the other format characters (Cf), as
/// inclusive ranges, as in `codeflow_core::git::name`.
const IGNORABLE: &[(u32, u32)] = &[
    (0x00AD, 0x00AD),
    (0x034F, 0x034F),
    (0x0600, 0x0605),
    (0x061C, 0x061C),
    (0x06DD, 0x06DD),
    (0x070F, 0x070F),
    (0x0890, 0x0891),
    (0x08E2, 0x08E2),
    (0x115F, 0x1160),
    (0x17B4, 0x17B5),
    (0x180B, 0x180F),
    (0x200B, 0x200F),
    (0x202A, 0x202E),
    (0x2060, 0x206F),
    (0x3164, 0x3164),
    (0xFE00, 0xFE0F),
    (0xFEFF, 0xFEFF),
    (0xFFA0, 0xFFA0),
    (0xFFF0, 0xFFFB),
    (0x110BD, 0x110BD),
    (0x110CD, 0x110CD),
    (0x13430, 0x1343F),
    (0x1BCA0, 0x1BCA3),
    (0x1D173, 0x1D17A),
    (0xE0000, 0xE0FFF),
];

fn hidden(c: char) -> bool {
    c.is_control()
        || matches!(u32::from(c), 0x2028 | 0x2029)
        || IGNORABLE
            .iter()
            .any(|(low, high)| (*low..=*high).contains(&u32::from(c)))
}

/// `text` as a terminal may show it; see the module documentation.
#[must_use]
pub fn display_output(text: &str) -> String {
    let mut shown = String::with_capacity(text.len());
    for c in text.chars() {
        if hidden(c) && !matches!(c, '\n' | '\t') {
            let code = u32::from(c);
            if code <= 0xff {
                let _ = write!(shown, "\\x{code:02x}");
            } else {
                let _ = write!(shown, "\\u{{{code:04x}}}");
            }
        } else {
            shown.push(c);
        }
    }
    shown
}

/// A message field: [`display_output`], with each further line indented,
/// as in `codeflow_core::git::display_message`.
#[must_use]
pub fn display_message(text: &str) -> String {
    display_output(text).replace('\n', "\n  ")
}
