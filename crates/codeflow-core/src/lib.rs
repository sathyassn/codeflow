//! `codeflow-core` — engine library for the `CodeFlow` v2 discipline layer.
//!
//! Modules arrive via the charter's import waves (docs/plan/v2/00-charter.md §3.2):
//! testing, workgraph/ledger, guards, scaffold, recall.

// The output sink (TSK-388), as in the CLI crate: these shadow `println!`,
// `print!`, `eprintln!` and `eprint!` for this crate, so what they print goes
// through [`git::display_output`], which escapes each control, format and
// separator character other than a line break or tab. The engine prints
// target names, ref names and errors (the test gate's progress lines, the
// edit guard's notes); none of them reaches the terminal raw. A field that
// can hold a line break also passes [`git::display_message`] where it is
// printed. Not covered: `write!` and `writeln!` to a terminal writer, and
// the standard library's panic message.
// One test-gate progress line on stderr: a single line by design, so the
// whole line passes [`git::display_line`], which also escapes a line break
// that a target name or reason holds (TSK-388).
macro_rules! progress {
    ($($arg:tt)*) => {
        ::std::eprintln!("{}", $crate::git::display_line(&::std::format!($($arg)*)))
    };
}
macro_rules! println {
    () => {
        ::std::println!()
    };
    ($($arg:tt)*) => {
        ::std::println!("{}", $crate::git::display_output(&::std::format!($($arg)*)))
    };
}
macro_rules! print {
    ($($arg:tt)*) => {
        ::std::print!("{}", $crate::git::display_output(&::std::format!($($arg)*)))
    };
}
macro_rules! eprint {
    ($($arg:tt)*) => {
        ::std::eprint!("{}", $crate::git::display_output(&::std::format!($($arg)*)))
    };
}
// Through `eprint!`, so both stay in use; escaping is idempotent.
macro_rules! eprintln {
    () => {
        eprint!("\n")
    };
    ($($arg:tt)*) => {
        eprint!("{}\n", ::std::format!($($arg)*))
    };
}

mod absence;
mod bounded_file;
pub mod capability;
pub mod ceremony;
mod contained;
pub mod delegate;
pub mod doctor;
pub mod error;
pub mod estimate;
pub mod feedback;
pub mod file_lock;
#[cfg(test)]
mod fixture_tests;
pub mod git;
pub mod hooks;
pub mod ids;
pub mod integrate;
pub mod ledger;
pub mod model_catalog;
pub mod model_qualification;
pub mod models;
pub mod portable_path;
pub mod reading;
pub mod recall;
pub mod registry;
pub mod release_local;
pub mod remedy;
pub mod remote;
pub mod root_checkout;
pub mod scaffold;
pub mod security;
pub mod settings;
pub mod status;
mod strict_json;
pub mod testing;
pub mod validate;
pub mod workgraph;
