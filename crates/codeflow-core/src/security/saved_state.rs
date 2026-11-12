//! The saved shell state a harness or terminal tool runs before its later
//! commands (TSK-1461): Claude Code's `session-env` and `shell-snapshots`
//! folders and the file `CLAUDE_ENV_FILE` names, Codex's `shell_snapshots`
//! folder, and herdr-core's template folder.
//!
//! The locations are the action table's `saved_state_paths` below each
//! home, the same entries where `CLAUDE_CONFIG_DIR`, `CODEX_HOME`,
//! `XDG_CONFIG_HOME` or `APPDATA` move them, and the file `CLAUDE_ENV_FILE`
//! names: the variables the hook inherits from the session that runs it.
//! They are not part of the startup class, whose readers stay as they were.
//!
//! Two runs judge a line. The first ([`readings`]) is the startup check's
//! own judge run a second time, over a class built from the locations: it
//! reads every form that check reads a write by, and where it cannot tell
//! what a command writes it refuses when the command names a location or
//! runs in a folder that holds one, so it fails closed. The startup check's
//! own class is judged as before.
//!
//! The second ([`evaluate`]) adds only what the first did not find. It
//! reads the line with the deletion check's reader
//! ([`crate::security::deletion::bound_commands_in`]), which keeps the
//! working directory per command (a subshell's `cd` stays in the subshell),
//! and judges what each command does to a location: a removal or move of
//! it, of a folder above it or of a link on the way to it, a copy, move,
//! link or extraction into it, a link made to it, a redirect or a program
//! that writes it, and an extraction into a folder above it, whose members
//! the guard cannot see.
//!
//! The second run compares a target with each location by file identity
//! (device and inode) along the walk the kernel makes to it, links and `..`
//! included, so a link before the last component or a folder reached
//! through `cd` to a link is the same node. A node that does not exist yet,
//! and every node on a system without these identities, is compared by its
//! readings: as written and as reached through links. A
//! location the guard cannot read is compared by its readings too, so it
//! fails closed.
//!
//! The findings of both runs come after every other finding of the call,
//! so they never change what another rule reports.

use std::cell::{OnceCell, RefCell};
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use super::{
    judge_class, key, lexical, reads_only, resolve, shown, Class, Entry, Kind, StartupEnv,
    LINK_LIMIT,
};
use crate::hooks::git_guard::{expand_commands_read, has_glob, is_shell, startup_glob_paths};
use crate::hooks::policy::PolicyLevel;
use crate::hooks::Violation;
use crate::security::dangerous::{program_name, recursive_rm_operands};
use crate::security::deletion::{bound_commands_in, bound_path, find_removals, Bound};
use crate::security::unresolved::{
    has_writer_entry, is_listed_writer, writer_effects, writer_name, DirOrder,
};

#[cfg(test)]
thread_local! {
    static OFF: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static READINGS_ONLY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether the pass runs. Always outside tests; a test turns it off to
/// compare with the guard as it stood before.
#[cfg(not(test))]
pub(crate) fn enabled() -> bool {
    true
}

/// Whether the pass runs.
#[cfg(test)]
pub(crate) fn enabled() -> bool {
    !OFF.with(std::cell::Cell::get)
}

/// Run `f` with the pass turned off.
#[cfg(test)]
pub(crate) fn without<R>(f: impl FnOnce() -> R) -> R {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            OFF.with(|off| off.set(self.0));
        }
    }
    let _restore = Restore(OFF.with(|off| off.replace(true)));
    f()
}

/// Run `f` with every comparison made by readings, as on a system without
/// file identities.
#[cfg(test)]
pub(crate) fn by_readings<R>(f: impl FnOnce() -> R) -> R {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            READINGS_ONLY.with(|off| off.set(self.0));
        }
    }
    let _restore = Restore(READINGS_ONLY.with(|off| off.replace(true)));
    f()
}

#[cfg(test)]
thread_local! {
    static FIXED: RefCell<Option<Places>> = const { RefCell::new(None) };
}

/// Run `f` with the hooks reading `places` in place of this process's
/// environment, so a test judges a call through a hook against fixture
/// locations.
#[cfg(test)]
pub(crate) fn fixed<R>(places: Places, f: impl FnOnce() -> R) -> R {
    struct Restore(Option<Places>);
    impl Drop for Restore {
        fn drop(&mut self) {
            FIXED.with(|fixed| *fixed.borrow_mut() = self.0.take());
        }
    }
    let _restore = Restore(FIXED.with(|fixed| fixed.borrow_mut().replace(places)));
    f()
}

#[cfg(test)]
fn identities() -> bool {
    !READINGS_ONLY.with(std::cell::Cell::get)
}

#[cfg(not(test))]
fn identities() -> bool {
    true
}

/// Where the saved shell state is for this guard: the startup class's
/// homes and `XDG_CONFIG_HOME`, and the variables that move a harness's own
/// folders.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Places {
    /// The homes and the startup variables, as the startup class reads them.
    pub(crate) startup: StartupEnv,
    /// `CLAUDE_CONFIG_DIR`, when set: Claude Code keeps the shell state it
    /// runs before each Bash command there instead of in `~/.claude`.
    pub(crate) claude_config: Option<PathBuf>,
    /// `CODEX_HOME`, when set: Codex keeps its shell snapshots there
    /// instead of in `~/.codex`.
    pub(crate) codex_home: Option<PathBuf>,
    /// `APPDATA`, when set on Windows: herdr-core reads its templates there
    /// instead of in `~/AppData/Roaming`.
    pub(crate) appdata: Option<PathBuf>,
    /// `CLAUDE_ENV_FILE`, when set: the file Claude Code runs before each
    /// later Bash command.
    pub(crate) claude_env_file: Option<PathBuf>,
}

impl Places {
    /// The locations from the variables `var` reads. A relative value is
    /// read from `base`, the directory the command runs in, as the harness
    /// or tool that reads it would from there.
    #[must_use]
    pub(crate) fn from_vars(var: &dyn Fn(&str) -> Option<OsString>, base: &Path) -> Self {
        let placed = |name: &str| {
            var(name)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
                .map(|x| if x.is_absolute() { x } else { base.join(&x) })
        };
        Self {
            startup: StartupEnv::from_vars(var, base),
            claude_config: placed("CLAUDE_CONFIG_DIR"),
            codex_home: placed("CODEX_HOME"),
            appdata: if cfg!(windows) {
                placed("APPDATA")
            } else {
                None
            },
            claude_env_file: placed("CLAUDE_ENV_FILE"),
        }
    }

    /// The locations from this process's environment, relative values read
    /// from `base`.
    #[must_use]
    pub(crate) fn from_process_at(base: &Path) -> Self {
        #[cfg(test)]
        if let Some(fixed) = FIXED.with(|fixed| fixed.borrow().clone()) {
            return fixed;
        }
        Self::from_vars(&|name| std::env::var_os(name), base)
    }

    /// The locations from this process's environment, relative values read
    /// from its working directory.
    #[must_use]
    pub(crate) fn from_process() -> Self {
        Self::from_process_at(&std::env::current_dir().unwrap_or_default())
    }

    /// The same locations with another home ([`StartupEnv::with_home_at`]).
    #[must_use]
    pub(crate) fn with_home_at(home: &Path, base: &Path) -> Self {
        Self {
            startup: StartupEnv::with_home_at(home, base),
            ..Self::from_process_at(base)
        }
    }

    /// The home folders a variable moves, with the variable and its value.
    fn moved(&self) -> [(&'static str, &'static str, Option<&PathBuf>); 4] {
        [
            (".claude/", "CLAUDE_CONFIG_DIR", self.claude_config.as_ref()),
            (".codex/", "CODEX_HOME", self.codex_home.as_ref()),
            (
                ".config/",
                "XDG_CONFIG_HOME",
                self.startup.xdg_config.as_ref(),
            ),
            ("AppData/Roaming/", "APPDATA", self.appdata.as_ref()),
        ]
    }

    /// The variables the guard has values for, which the line may expand
    /// in a target.
    fn held(&self) -> Vec<(String, String)> {
        [
            ("XDG_CONFIG_HOME", &self.startup.xdg_config),
            ("ZDOTDIR", &self.startup.zdotdir),
            ("CLAUDE_CONFIG_DIR", &self.claude_config),
            ("CODEX_HOME", &self.codex_home),
            ("APPDATA", &self.appdata),
            ("CLAUDE_ENV_FILE", &self.claude_env_file),
        ]
        .into_iter()
        .filter_map(|(name, value)| {
            let value = value.as_ref()?.to_str()?;
            Some((name.to_string(), value.to_string()))
        })
        .collect()
    }
}

/// One saved-state location at a known path.
struct Location {
    label: String,
    /// Whether the location is a folder and everything below it.
    dir: bool,
    seen: Seen,
}

/// The saved-state locations, each with its label, its path and whether it
/// is a folder: the table's entries below each home, the ones a variable
/// the guard holds moves, and the file `CLAUDE_ENV_FILE` names.
fn spots(env: &Places) -> Vec<(String, PathBuf, bool)> {
    let mut out = Vec::new();
    let table = &crate::security::actions::table().saved_state_paths;
    for entry in &table.home {
        let dir = entry.ends_with('/');
        let rel = entry.trim_end_matches('/');
        for home in env.startup.home.iter().chain(&env.startup.other_homes) {
            out.push((format!("~/{rel}"), home.join(rel), dir));
        }
        for (prefix, variable, root) in env.moved() {
            if let (Some(rest), Some(root)) = (rel.strip_prefix(prefix), root) {
                out.push((format!("${variable}/{rest}"), root.join(rest), dir));
            }
        }
    }
    for entry in &table.absolute {
        let dir = entry.ends_with('/');
        let rel = entry.trim_end_matches('/');
        out.push((rel.to_string(), PathBuf::from(rel), dir));
    }
    if let Some(file) = &env.claude_env_file {
        out.push(("$CLAUDE_ENV_FILE".to_string(), file.clone(), false));
    }
    out
}

/// The saved-state locations, each walked once.
fn locations(env: &Places) -> Vec<Location> {
    spots(env)
        .into_iter()
        .map(|(label, path, dir)| Location {
            label,
            dir,
            seen: walk(&path, true),
        })
        .collect()
}

/// The saved-state locations as a class of the startup check: each
/// location an entry, the table's anchored spellings and the full path of
/// each location a variable places as the text rule's needles, the
/// variables the guard holds, and each location's folder as a place a
/// placing program could fill.
fn readings_class(env: &Places) -> Class {
    let table = &crate::security::actions::table().saved_state_paths;
    let mut needles: Vec<String> = table
        .home
        .iter()
        .chain(&table.absolute)
        .map(|entry| entry.trim_end_matches('/').to_lowercase())
        .collect();
    let mut entries = Vec::new();
    for (label, path, dir) in spots(env) {
        let written = key(&lexical(&path));
        let mut readings = vec![written.clone()];
        if let Ok(real) = resolve(&path) {
            let real = key(&real);
            if !readings.contains(&real) {
                readings.push(real);
            }
        }
        // A location a variable places is named by its full path.
        if label.starts_with('$') && !needles.contains(&written) {
            needles.push(written);
        }
        entries.push(Entry::At {
            label,
            readings,
            dir,
        });
    }
    Class {
        entries,
        homes: env
            .startup
            .home
            .iter()
            .chain(&env.startup.other_homes)
            .map(|home| key(&lexical(home)))
            .collect(),
        needles,
        held: env.held(),
        folders_place: true,
    }
}

/// The sanctioned path printed with every saved-state refusal.
pub(crate) const STATE_SANCTIONED: &str = "a harness runs its saved shell state before its later commands, so only the harness writes it: read a file there with a reader the guard lists, such as `cat`, `head`, `grep` or `ls`, and ask the operator to make any other change by hand, outside the agent session; no policy key relaxes this rule";

/// The way through a refusal near saved state names: a program aimed at a
/// folder that holds it, or a write the guard cannot resolve there.
const NEAR_WAY: &str = "aim the program at a named sub-folder that holds no saved shell state, or name the file it writes";

/// The way through a program that changes the files of the folder it runs
/// in (`git checkout`, `tar`, `patch`), which a program cannot aim at a
/// sub-folder: the harness configuration folder stays outside the project.
const PLACE_WAY: &str = "keep the harness configuration folder outside the project, or run the program in a folder that holds no saved shell state";

/// The way through a copy of a folder's contents or a tree into a folder
/// that holds saved state.
const COPY_WAY: &str = "copy into a named sub-folder that holds no saved shell state";

/// The way through a directory change into a location, which the startup
/// check judges as a target unless the line is only directory changes to
/// plain, unquoted paths. A path with a space cannot be written unquoted,
/// so a listed reader given the full path is a way through too.
const CD_WAY: &str = "run `cd` alone with a plain unquoted path, or give the full path to a reader the guard lists, such as `cat`, `head`, `grep` or `ls`, instead of changing into the folder";

/// The way through a directory change whose quoted text names a location,
/// in place of the startup check's words that the program can write it.
const CD_TEXT_WAY: &str = ", where a harness keeps the shell state it runs before later commands; give the full path to a reader the guard lists, such as `cat`, `head`, `grep` or `ls`, instead of changing into the folder";

/// The way through a program the guard does not list that names a
/// location: a reader it lists.
const READ_WAY: &str =
    "to read it, use a reader the guard lists, such as `cat`, `head`, `grep` or `ls`";

/// Phrases of the startup check that name a location rather than say what
/// is written or placed there; in them a folder is called a location.
const NAMING_PHRASES: &[&str] = &[
    "targets the saved shell state file `",
    "names the saved shell state file `",
    "could name the saved shell state file `",
    "searches the saved shell state file `",
];

/// `message` with a location that is a folder called a location where the
/// message names it, `files` being the labels and spellings of the
/// locations that are files. What is written or placed stays a file.
fn located(message: &str, files: &[String]) -> String {
    let mut out = message.to_string();
    for phrase in NAMING_PHRASES {
        let Some(at) = out.find(phrase) else {
            continue;
        };
        let named = out[at + phrase.len()..].split('`').next().unwrap_or("");
        if !files.iter().any(|file| file == named) {
            let fixed = phrase.replace(" file `", " location `");
            out.replace_range(at..at + phrase.len(), &fixed);
        }
    }
    out.replace(
        "names a saved shell state file",
        "names a saved shell state location",
    )
    .replace(
        "above saved shell state files",
        "above saved shell state locations",
    )
}

/// Words of a refusal of the startup check that mean the command acts
/// near saved state rather than on a location it names.
const NEAR_MARKS: &[&str] = &[
    "which the guard cannot resolve",
    "where it can place",
    "cannot tell the directory",
    "does not read its options",
    "an option the guard does not read",
];

/// Judge `command`, run in `cwd`, with the startup check over the
/// saved-state class: every form the check reads a write by, judged
/// against the locations. zsh's clobbering redirects are read as plain
/// ones first. A refusal near saved state, a directory change into it and
/// a program the guard does not list that names it each name a way
/// through.
#[must_use]
pub(crate) fn readings(command: &str, cwd: &Path, env: &Places) -> Vec<Violation> {
    if !enabled() {
        return Vec::new();
    }
    let line = plain_redirects(command);
    let expanded = expand_commands_read(&line);
    let class = readings_class(env);
    let files: Vec<String> = spots(env)
        .into_iter()
        .filter(|(_, _, dir)| !dir)
        .flat_map(|(label, path, _)| [label, key(&lexical(&path))])
        .collect();
    judge_class(
        &line,
        cwd,
        &env.startup,
        &expanded,
        &class,
        Kind::SavedState,
        PolicyLevel::Off,
    )
    .into_iter()
    .map(|mut finding| {
        finding.message = located(&finding.message, &files);
        let program = finding
            .message
            .strip_prefix('`')
            .and_then(|rest| rest.split_once('`'))
            .map(|(name, rest)| (name.to_string(), rest.to_string()));
        if let Some((name, rest)) = &program {
            if matches!(name.as_str(), "cd" | "pushd")
                && rest.starts_with(" is given text that names ")
            {
                if let Some(named) = finding.message.strip_suffix(", which it can write") {
                    finding.message = format!("{named}{CD_TEXT_WAY}");
                }
                return finding;
            }
            // A program the guard does not list as a writer may only read
            // what it names: `find` writes by its actions, which are read.
            if !matches!(name.as_str(), "cd" | "pushd" | "find")
                && rest.starts_with(" targets the saved shell state ")
                && !is_listed_writer(name)
            {
                finding.message = format!("{}; {READ_WAY}", finding.message);
                return finding;
            }
        }
        let message = &finding.message;
        let way = if message.contains(" copies the contents of ")
            || message.contains(" puts the tree ")
        {
            Some(COPY_WAY)
        } else if ["`cd` targets ", "`pushd` targets "]
            .iter()
            .any(|start| message.starts_with(start))
        {
            Some(CD_WAY)
        } else if message.contains("` runs in `")
            && message.contains(", which holds the saved shell state location `")
        {
            Some(PLACE_WAY)
        } else if NEAR_MARKS.iter().any(|mark| message.contains(mark)) {
            Some(NEAR_WAY)
        } else {
            None
        };
        if let Some(way) = way {
            finding.message = format!("{message}; {way}");
        }
        finding
    })
    .collect()
}

/// `command` with each of zsh's clobbering redirects written as the plain
/// redirect it is: a `!` or `|` after `>`, `>>`, `>&` or `>>&` is dropped,
/// so `&>|` reads as `&>` too. zsh writes the file each one names.
///
/// The rewrite reads no quotes, comments or heredocs: a quote in a comment
/// or a heredoc body cannot hide a redirect after it. Inside quoted text it
/// changes data only, which stays quoted. Only a character after a
/// backslash is kept as written, so an escaped `>` stays an argument.
fn plain_redirects(command: &str) -> String {
    let chars: Vec<char> = command.chars().collect();
    let mut out = String::with_capacity(command.len());
    let mut at = 0;
    while let Some(&c) = chars.get(at) {
        match c {
            '\\' => {
                out.push(c);
                if let Some(&next) = chars.get(at + 1) {
                    out.push(next);
                }
                at += 2;
            }
            '>' => {
                let mut width = 1;
                if chars.get(at + width) == Some(&'>') {
                    width += 1;
                }
                if chars.get(at + width) == Some(&'&') {
                    width += 1;
                }
                let operator: String = chars[at..at + width].iter().collect();
                out.push_str(&operator);
                at += width;
                if matches!(chars.get(at), Some('!' | '|')) {
                    at += 1;
                }
            }
            _ => {
                out.push(c);
                at += 1;
            }
        }
    }
    out
}

/// Every variable of `vars` that `word` changes with a zsh flag, modifier
/// or subscript, each once with its value, in the order the word names
/// them: `$NAME:a`, `$NAME[1,-1]`, a braced form with anything after the
/// name (`${NAME:a}`, `${NAME[1]}`, `${NAME:-x}`), zsh's `(flags)`, `=`,
/// `~` and `^` before the name, braced or not, and a braced expansion that
/// holds another one. The guard does not compute the result, so the
/// startup check reads such a word as filled in at run time, and judges
/// each changed variable: one aimed at an ordinary folder never hides
/// another in the same word.
///
/// The word is read as the command reader hands it over, with its quotes
/// and backslashes removed, so a quote or backslash left in it is a
/// literal character and hides nothing: every `$` counts, as the
/// substitution that fills the word counts it.
pub(super) fn modified_variables(word: &str, vars: &[(String, String)]) -> Vec<(String, String)> {
    let mut changed_vars: Vec<(String, String)> = Vec::new();
    let chars: Vec<char> = word.chars().collect();
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let flags = |mut at: usize| {
        while chars.get(at).is_some_and(|c| matches!(c, '=' | '~' | '^')) {
            at += 1;
        }
        at
    };
    let mut at = 0;
    while let Some(&c) = chars.get(at) {
        at += 1;
        if c != '$' {
            continue;
        }
        let braced = chars.get(at) == Some(&'{');
        let mut start = at + usize::from(braced);
        // Whether a flag or an inner expansion changes the name's value.
        let mut changed = false;
        if braced && chars.get(start) == Some(&'(') {
            while chars.get(start).is_some_and(|c| *c != ')') {
                start += 1;
            }
            start += 1;
            changed = true;
        }
        let after = flags(start);
        changed |= after != start;
        start = after;
        if braced && chars.get(start) == Some(&'$') {
            // The outer braces hold an expansion of their own.
            start += 1;
            if chars.get(start) == Some(&'{') {
                start += 1;
            }
            start = flags(start);
            changed = true;
        }
        let start = start.min(chars.len());
        let mut end = start;
        while chars.get(end).copied().is_some_and(ident) {
            end += 1;
        }
        let name: String = chars[start..end].iter().collect();
        let Some((_, value)) = vars.iter().find(|(known, _)| *known == name) else {
            continue;
        };
        let next = chars.get(end).copied();
        let modified = changed
            || if braced {
                next != Some('}')
            } else {
                next == Some('[')
                    || (next == Some(':')
                        && chars
                            .get(end + 1)
                            .is_some_and(|c| c.is_ascii_alphabetic() || *c == '&'))
            };
        if modified && !changed_vars.iter().any(|(known, _)| *known == name) {
            changed_vars.push((name, value.clone()));
        }
    }
    changed_vars
}

/// A node's identity: its device and inode.
type Id = (u64, u64);

#[cfg(unix)]
fn id_of(meta: &std::fs::Metadata) -> Option<Id> {
    use std::os::unix::fs::MetadataExt;
    identities().then(|| (meta.dev(), meta.ino()))
}

/// No identity the guard can compare on this system: every comparison is
/// made by readings.
#[cfg(not(unix))]
fn id_of(_meta: &std::fs::Metadata) -> Option<Id> {
    None
}

/// What the guard read of a path on disk ([`walk`]).
#[derive(Debug, Default)]
struct Seen {
    /// The folders the path is reached through, root first, and the path's
    /// own node when it exists: each component of the physical path the
    /// walk reached, after every link and `..`. Empty without identities.
    chain: Vec<Id>,
    /// Every link node the walk passed through: removing one takes the way
    /// to the path with it.
    links: Vec<Id>,
    /// The path's own node, when it exists and has an identity.
    end: Option<Id>,
    /// Whether the path exists.
    exists: bool,
    /// Set when a component, a link or a type could not be read, or the
    /// walk passed more links than the guard follows: the path is then
    /// compared by readings, and a location fails closed.
    unread: bool,
    /// The readings, lower case: as written, with `.` and `..` folded, and
    /// as reached, the part that exists through its links and the rest as
    /// written.
    readings: Vec<String>,
}

/// Walk `path` as the kernel resolves it: each component in turn, a link
/// followed where it stands and `..` taken from the folder actually
/// reached. With `follow_last` false, a link at the last component is the
/// node itself, as a removal sees it.
///
/// A `..` that leaves a folder that does not exist yet returns to the folder
/// above it, as it does once an earlier command on the line has made it.
// One loop over the components, as the kernel takes them, kept whole.
#[allow(clippy::too_many_lines)]
fn walk(path: &Path, follow_last: bool) -> Seen {
    let mut seen = Seen::default();
    let mut pending: Vec<OsString> = path
        .components()
        .rev()
        .map(|c| c.as_os_str().to_os_string())
        .collect();
    // The physical path reached so far, with one identity per component
    // (`None` for a component that does not exist).
    let mut current = PathBuf::new();
    let mut stack: Vec<Option<Id>> = Vec::new();
    let mut absent = false;
    // How many of the components walked since the first missing one are
    // missing too.
    let mut missing = 0usize;
    let mut is_dir = true;
    let mut links = 0;
    let mut last_kept = false;
    while let Some(part) = pending.pop() {
        let component = Path::new(&part);
        last_kept = false;
        match component.components().next() {
            Some(Component::CurDir) | None => continue,
            Some(Component::ParentDir) => {
                if stack.len() > 1 || !current.has_root() {
                    stack.pop();
                }
                current.pop();
                if absent && missing > 0 {
                    missing -= 1;
                    if missing == 0 {
                        absent = false;
                    }
                }
                continue;
            }
            Some(Component::Prefix(_)) => {
                current.push(component);
                continue;
            }
            Some(Component::RootDir) => {
                current.push(component);
                stack.clear();
                stack.push(
                    std::fs::metadata(&current)
                        .ok()
                        .and_then(|meta| id_of(&meta)),
                );
                continue;
            }
            Some(Component::Normal(_)) => {}
        }
        let next = current.join(component);
        if absent || !is_dir {
            if absent {
                missing += 1;
            }
            absent = true;
            current = next;
            stack.push(None);
            continue;
        }
        let last = pending.is_empty();
        match std::fs::symlink_metadata(&next) {
            Ok(meta) if meta.file_type().is_symlink() && (!last || follow_last) => {
                seen.links.extend(id_of(&meta));
                links += 1;
                let Ok(target) = std::fs::read_link(&next) else {
                    seen.unread = true;
                    break;
                };
                if links > LINK_LIMIT {
                    seen.unread = true;
                    break;
                }
                if target.has_root() {
                    current = PathBuf::new();
                    stack.clear();
                }
                pending.extend(
                    target
                        .components()
                        .rev()
                        .map(|c| c.as_os_str().to_os_string()),
                );
            }
            Ok(meta) => {
                stack.push(id_of(&meta));
                is_dir = meta.is_dir();
                current = next;
                last_kept = true;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                absent = true;
                missing += 1;
                current = next;
                stack.push(None);
            }
            Err(_) => {
                seen.unread = true;
                break;
            }
        }
    }
    if !seen.unread && !absent {
        if last_kept {
            seen.exists = true;
            seen.end = stack.last().copied().flatten();
        } else {
            // The path ends in `.` or `..`, or names a root: the folder
            // reached.
            match std::fs::metadata(&current) {
                Ok(meta) => {
                    seen.exists = true;
                    seen.end = id_of(&meta);
                    if let Some(top) = stack.last_mut() {
                        *top = seen.end;
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => seen.unread = true,
            }
        }
    }
    seen.readings.push(key(&lexical(path)));
    if seen.unread {
        seen.chain.clear();
        seen.links.clear();
        seen.end = None;
    } else {
        seen.chain = stack.into_iter().flatten().collect();
        let reached = key(&lexical(&current));
        if !seen.readings.contains(&reached) {
            seen.readings.push(reached);
        }
    }
    seen
}

/// Whether reading `inner` is `outer` or lies below it.
fn reading_within(inner: &str, outer: &str) -> bool {
    let inner = inner.trim_end_matches('/');
    let outer = outer.trim_end_matches('/');
    inner == outer
        || inner
            .strip_prefix(outer)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// Whether `target` is the location `at` or, for a folder, lies inside it:
/// the location's node is on the target's physical path. By readings when
/// either side has no identity to compare.
fn within(target: &Seen, at: &Location) -> bool {
    if let Some(end) = at.seen.end {
        if !target.unread {
            return if at.dir {
                target.chain.contains(&end)
            } else {
                target.end == Some(end)
            };
        }
    }
    target.readings.iter().any(|t| {
        at.seen.readings.iter().any(|a| {
            if at.dir {
                reading_within(t, a)
            } else {
                t.trim_end_matches('/') == a.trim_end_matches('/')
            }
        })
    })
}

/// Whether `target` is the location `at`, a folder above it or a link on
/// the way to it: the target's node is on the location's walk, which stops
/// where the location does not exist yet. By readings when either side has
/// no identity to compare.
fn holds(target: &Seen, at: &Location) -> bool {
    if let Some(end) = target.end {
        if !at.seen.unread {
            return at.seen.chain.contains(&end) || at.seen.links.contains(&end);
        }
    }
    at.seen
        .readings
        .iter()
        .any(|a| target.readings.iter().any(|t| reading_within(a, t)))
}

/// Judge `command`, run in `cwd`, for what it does to saved shell state.
/// Every finding is `security.shell_startup`, always blocking.
#[must_use]
pub(crate) fn evaluate(command: &str, cwd: &Path, env: &Places) -> Vec<Violation> {
    if !enabled() {
        return Vec::new();
    }
    let line = bound_commands_in(command, Some(cwd), &env.held());
    if !line.whole || line.commands.is_empty() {
        return Vec::new();
    }
    let lower = command.to_lowercase();
    let pass = Pass {
        env,
        base: cwd,
        class: OnceCell::new(),
        locations: OnceCell::new(),
        dots: ["dotglob", "globdots", "globignore"]
            .iter()
            .any(|word| lower.contains(word)),
    };
    let mut calls: Vec<usize> = Vec::new();
    for bound in &line.commands {
        if !calls.contains(&bound.call) {
            calls.push(bound.call);
        }
    }
    let mut messages: Vec<String> = Vec::new();
    for call in calls {
        let mut found: Vec<String> = Vec::new();
        let mut every = true;
        'readings: for bound in line.commands.iter().filter(|b| b.call == call) {
            if bound.dirs.is_empty() {
                every = false;
                break;
            }
            for dir in &bound.dirs {
                let member = pass.judge(bound, dir);
                if member.is_empty() {
                    every = false;
                    break 'readings;
                }
                found.extend(member);
            }
        }
        if every {
            for message in found {
                if !messages.contains(&message) {
                    messages.push(message);
                }
            }
        }
    }
    messages
        .into_iter()
        .map(|message| Violation::always_blocking(super::RULE, message, STATE_SANCTIONED))
        .collect()
}

/// The saved-state finding for a native edit tool's target, `path`
/// absolute: the file is a location or lies inside one.
#[must_use]
pub(crate) fn edit_target(path: &Path, env: &Places) -> Option<Violation> {
    if !enabled() {
        return None;
    }
    let seen = walk(path, true);
    let label = locations(env)
        .into_iter()
        .find(|at| within(&seen, at))?
        .label;
    Some(Violation::always_blocking(
        super::RULE,
        format!("file edit targets the saved shell state `{label}`"),
        STATE_SANCTIONED,
    ))
}

/// The writers whose copy, move, sync or extraction the pass reads with the
/// writer table.
const PLACERS: &[&str] = &[
    "mv", "cp", "ln", "install", "ditto", "rsync", "tar", "unzip",
];

struct Pass<'a> {
    env: &'a Places,
    base: &'a Path,
    /// The startup class, for listing a glob the way its reader does.
    class: OnceCell<Class>,
    /// The locations, walked once on first use.
    locations: OnceCell<Vec<Location>>,
    /// Whether the line may make a glob match names that start with `.`.
    dots: bool,
}

/// How a placement is judged: the files it lands are named, or it places
/// a tree whose names the guard does not know.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Lands {
    /// One file or link at the path.
    File,
    /// A tree at the path, its names known by the copy.
    Tree,
    /// An archive's members below the path, whose names the guard cannot
    /// see.
    Archive,
}

impl Pass<'_> {
    fn locations(&self) -> &[Location] {
        self.locations.get_or_init(|| locations(self.env))
    }

    fn class(&self) -> &Class {
        self.class.get_or_init(|| Class::new(&self.env.startup))
    }

    /// One reading of a command: its program and arguments, run in `dir`.
    /// The messages it gives; none when it touches no saved state or the
    /// pass cannot resolve what it touches.
    fn judge(&self, bound: &Bound, dir: &str) -> Vec<String> {
        let mut found = Vec::new();
        let Some(first) = bound.argv.first() else {
            return found;
        };
        if bound.redirect {
            for path in self.paths(first, bound, dir).unwrap_or_default() {
                if let Some(label) = self.written(&walk(&path, true)) {
                    found.push(format!(
                        "a redirect writes `{first}`, which is saved shell state: `{label}`"
                    ));
                }
            }
            return found;
        }
        let args = &bound.argv[1..];
        match program_name(first).as_str() {
            "rm" => match recursive_rm_operands(args) {
                Some(operands) => {
                    for operand in operands {
                        self.removal(&mut found, "rm", "removes", operand, bound, dir);
                    }
                }
                None => {
                    for operand in plain_operands(args) {
                        self.named(&mut found, "rm", operand, bound, dir);
                    }
                }
            },
            name @ ("rmdir" | "unlink") => {
                for operand in plain_operands(args) {
                    self.named(&mut found, name, operand, bound, dir);
                }
            }
            "find" => {
                for start in find_removals(args).unwrap_or_default() {
                    self.removal(&mut found, "find", "removes", &start, bound, dir);
                }
            }
            _ => self.writer(&mut found, first, args, bound, dir),
        }
        found
    }

    /// A program other than a remover: a copy, move, link, sync or
    /// extraction read with the writer table, a file its options write, and
    /// the operands of any other program that may write them.
    fn writer(
        &self,
        found: &mut Vec<String>,
        first: &str,
        args: &[String],
        bound: &Bound,
        dir: &str,
    ) {
        let name = writer_name(first);
        if reads_only(name, args) || is_shell(name) || name == "eval" {
            return;
        }
        if !has_writer_entry(name) {
            // A program the guard does not know may only read what it is
            // given, so the message says it targets the path.
            for word in args {
                for path_word in operand_paths(word) {
                    for path in self.paths(path_word, bound, dir).unwrap_or_default() {
                        if let Some(label) = self.written(&walk(&path, true)) {
                            found.push(format!(
                                "`{name}` targets `{path_word}`, which is saved shell state: `{label}`; {READ_WAY}"
                            ));
                        }
                    }
                }
            }
            return;
        }
        let Some(effects) = writer_effects(first, args, &[]) else {
            return;
        };
        if effects.read.unknown.is_some() {
            // An option the table does not list: its arity is unknown, so
            // the targets are unresolved.
            return;
        }
        for path in &effects.writes {
            self.write(found, name, path, bound, dir);
        }
        // An extraction's operands name archive members, not files on disk.
        let extracts = matches!(name, "tar" | "unzip");
        if let Some(copy) = &effects.copy {
            if PLACERS.contains(&name) && !copy.directories {
                self.copy(found, name, copy, &effects.read.flags, bound, dir);
            } else {
                for word in copy.sources.iter().chain(&copy.dest) {
                    self.write(found, name, word, bound, dir);
                }
            }
        } else if !extracts {
            for word in &effects.read.operands {
                self.write(found, name, word, bound, dir);
            }
        }
        if extracts {
            if let Some(places) = &effects.places {
                let target = match places.order {
                    DirOrder::Sequential => places.dirs.iter().fold(None::<String>, |at, next| {
                        Some(match at {
                            Some(at) if !next.starts_with(['/', '~']) => {
                                format!("{}/{next}", at.trim_end_matches('/'))
                            }
                            _ => next.clone(),
                        })
                    }),
                    DirOrder::Last | DirOrder::Each => places.dirs.last().cloned(),
                };
                let target = target.unwrap_or_else(|| ".".to_string());
                self.placement(found, name, &target, None, Lands::Archive, bound, dir);
            }
        }
    }

    /// A copy, move, link or sync: each source removed by a move, and the
    /// path each source lands at.
    fn copy(
        &self,
        found: &mut Vec<String>,
        name: &str,
        copy: &crate::security::unresolved::CopyRead,
        flags: &[String],
        bound: &Bound,
        dir: &str,
    ) {
        let remote = |path: &str| {
            matches!(name, "rsync" | "scp")
                && (path.starts_with("rsync://")
                    || path.split('/').next().unwrap_or("").contains(':'))
        };
        if name == "mv" {
            for source in &copy.sources {
                self.removal(found, "mv", "moves", source, bound, dir);
            }
        }
        // A link, hard or symbolic, to a location or a folder above one is
        // a second way to it: a later write through the link lands there.
        if name == "ln" {
            for source in &copy.sources {
                let mut linked = Vec::new();
                self.removal(&mut linked, "ln", "links", source, bound, dir);
                found.extend(linked.into_iter().map(|message| {
                    format!("{message}; link a named file that is not saved shell state, or a sub-folder that holds none")
                }));
            }
        }
        let Some(dest) = copy.dest.as_deref().filter(|dest| !remote(dest)) else {
            return;
        };
        let flagged = |names: &[&str]| flags.iter().any(|flag| names.contains(&flag.as_str()));
        let recursive = match name {
            "mv" | "ditto" => true,
            "cp" => flagged(&["-r", "-R", "-a", "--recursive", "--archive"]),
            "rsync" => flagged(&["-r", "-a", "--recursive", "--archive"]),
            _ => false,
        };
        let Some(dests) = self.paths(dest, bound, dir) else {
            // Unresolved: no finding.
            return;
        };
        // The destination is a folder each source lands in: a target
        // directory, more than one source, a trailing `/`, or a folder now.
        // `-T` makes it never a folder, and `ln -n` does not follow a link
        // there, so the link itself is replaced.
        let whole = matches!(name, "ln" | "mv" | "cp") && flagged(&["-T", "--no-target-directory"]);
        let unfollowed = name == "ln" && flagged(&["-n", "-h", "--no-dereference"]);
        let folder_now = |d: &PathBuf| {
            let metadata = if unfollowed {
                std::fs::symlink_metadata(d)
            } else {
                std::fs::metadata(d)
            };
            metadata.is_ok_and(|m| m.is_dir())
        };
        let into = !whole
            && (copy.target
                || copy.sources.len() > 1
                || dest.ends_with('/')
                || dests.iter().any(folder_now));
        // The kernel reads relative symbolic link text from the folder the
        // link is in, not from where the command runs.
        if name == "ln" && flagged(&["-s", "--symbolic"]) {
            // The destination may be a folder an earlier command on the
            // line makes, so it is tried as one whatever it is now, and as
            // the link's own name, unless `-T` or what the line shows
            // settles which.
            let folders: Vec<String> = dests
                .iter()
                .flat_map(|d| {
                    let folder = Some(d.as_path());
                    if into {
                        vec![folder]
                    } else if whole {
                        vec![d.parent()]
                    } else {
                        vec![folder, d.parent()]
                    }
                })
                .flatten()
                .filter_map(|folder| folder.to_str().map(str::to_string))
                .collect();
            for source in copy.sources.iter().filter(|s| !s.starts_with(['/', '~'])) {
                for folder in &folders {
                    let mut linked = Vec::new();
                    self.removal(&mut linked, "ln", "links", source, bound, folder);
                    for message in linked {
                        let message = format!("{message}; link a named file that is not saved shell state, or a sub-folder that holds none");
                        if !found.contains(&message) {
                            found.push(message);
                        }
                    }
                }
            }
        }
        for source in copy.sources.iter().filter(|source| !remote(source)) {
            let tree = recursive && self.may_be_tree(source, bound, dir);
            let lands = if tree { Lands::Tree } else { Lands::File };
            // `ditto`, a source ending in `/` (rsync, BSD cp) or `/.` places
            // what the source holds: the names below the destination are the
            // source's own, so the destination itself is compared.
            let contents = name == "ditto"
                || source.ends_with('/')
                || source.ends_with("/.")
                || matches!(source.as_str(), "." | "..");
            let placed: Option<String> = if into && !contents {
                last_name(source)
            } else {
                None
            };
            self.placement(found, name, dest, placed.as_deref(), lands, bound, dir);
        }
    }

    /// The absolute paths `operand` names from `dir`, each match of a glob
    /// included; `None` when the pass cannot resolve it. A path without a
    /// glob keeps its `..` components, for the walk to take from the folder
    /// actually reached.
    fn paths(&self, operand: &str, bound: &Bound, dir: &str) -> Option<Vec<PathBuf>> {
        let text = bound_path(dir, operand)?;
        if !has_glob(&text) {
            return Some(vec![self.raw(operand, bound, dir)?]);
        }
        let path = self.absolute(&text, bound.home_assigned)?;
        let mut base = PathBuf::new();
        let mut rest = PathBuf::new();
        for component in path.components() {
            if rest.as_os_str().is_empty() && !has_glob(&component.as_os_str().to_string_lossy()) {
                base.push(component);
            } else {
                rest.push(component);
            }
        }
        let matches = startup_glob_paths(&base, &rest, self.dots, self.class())?;
        // A word that matches nothing stays as written.
        Some(if matches.is_empty() {
            vec![path]
        } else {
            matches
        })
    }

    /// `operand` joined, as written, to the directory `dir` the command
    /// runs in: the walk then takes each link and `..` as the kernel does.
    fn raw(&self, operand: &str, bound: &Bound, dir: &str) -> Option<PathBuf> {
        if operand.starts_with(['/', '~']) {
            return self.absolute(operand, bound.home_assigned);
        }
        let dir = if dir.is_empty() {
            self.base.to_path_buf()
        } else {
            self.absolute(dir, bound.home_assigned)?
        };
        Some(dir.join(operand))
    }

    /// A path the reader resolved, made absolute: from the home the class
    /// is built from, from the root, or from where the line started.
    fn absolute(&self, text: &str, home_assigned: bool) -> Option<PathBuf> {
        if let Some(rest) = text.strip_prefix('~') {
            if home_assigned || !(rest.is_empty() || rest.starts_with('/')) {
                return None;
            }
            let home = self.env.startup.home.as_ref()?;
            return Some(home.join(rest.trim_start_matches('/')));
        }
        if text.starts_with('/') {
            // A `/`-rooted path names no fixed place on Windows.
            return (!cfg!(windows)).then(|| PathBuf::from(text));
        }
        Some(self.base.join(text))
    }

    /// Whether a copy's source may be a directory tree: a directory, a link
    /// to one, or a source the pass cannot read. A source not found may be
    /// a tree an earlier command on the line makes.
    fn may_be_tree(&self, source: &str, bound: &Bound, dir: &str) -> bool {
        let Some(paths) = self.paths(source, bound, dir) else {
            return true;
        };
        paths.iter().any(|path| match std::fs::metadata(path) {
            Ok(metadata) => metadata.is_dir(),
            Err(_) => true,
        })
    }

    /// The location whose way passes through `seen`'s own node as a link:
    /// replacing or removing that link changes where the location is.
    fn link_on_the_way(&self, seen: &Seen) -> Option<&Location> {
        let end = seen.end?;
        self.locations()
            .iter()
            .find(|at| !at.seen.unread && at.seen.links.contains(&end))
    }

    /// The location a written path is or lies in.
    fn written(&self, seen: &Seen) -> Option<String> {
        self.locations()
            .iter()
            .find(|at| within(seen, at))
            .map(|at| at.label.clone())
    }

    /// A file a program writes: refused when it is or lies in a location.
    fn write(&self, found: &mut Vec<String>, name: &str, word: &str, bound: &Bound, dir: &str) {
        if word.contains("{}") {
            return;
        }
        for path in self.paths(word, bound, dir).unwrap_or_default() {
            if let Some(label) = self.written(&walk(&path, true)) {
                found.push(format!(
                    "`{name}` writes `{word}`, which is saved shell state: `{label}`"
                ));
            }
        }
    }

    /// A tree removed (`rm -r`, `find -delete`) or moved away (`mv`):
    /// refused when it is or lies in a location, or is a folder above one
    /// that exists.
    fn removal(
        &self,
        found: &mut Vec<String>,
        name: &str,
        verb: &str,
        operand: &str,
        bound: &Bound,
        dir: &str,
    ) {
        let Some(paths) = self.paths(operand, bound, dir) else {
            // Unresolved: no finding.
            return;
        };
        // A trailing `/` makes the call follow a link to the tree it names.
        let follow = operand.ends_with('/');
        for path in paths {
            let seen = walk(&path, follow);
            if !seen.exists && !seen.unread {
                // Nothing there to remove.
                continue;
            }
            let hit = self.locations().iter().any(|at| {
                within(&seen, at) || ((at.seen.exists || at.seen.unread) && holds(&seen, at))
            });
            if hit {
                found.push(format!(
                    "`{name}` {verb} `{operand}`, which is or holds saved shell state: `{}`",
                    shown(&lexical(&path))
                ));
            }
        }
    }

    /// `rmdir`, `unlink` or a plain `rm`: refused when the path is a
    /// location or lies in one and something is there.
    fn named(&self, found: &mut Vec<String>, name: &str, operand: &str, bound: &Bound, dir: &str) {
        let Some(paths) = self.paths(operand, bound, dir) else {
            // Unresolved: no finding.
            return;
        };
        for path in paths {
            // The last component is removed itself, so only the folders
            // above it are followed.
            let seen = walk(&path, false);
            if !seen.exists && !seen.unread {
                continue;
            }
            if let Some(at) = self.locations().iter().find(|at| within(&seen, at)) {
                found.push(format!(
                    "`{name}` removes `{operand}`, which is saved shell state: `{}`",
                    at.label
                ));
            } else if let Some(at) = self.link_on_the_way(&seen) {
                found.push(format!(
                    "`{name}` removes `{operand}`, a link on the way to the saved shell state `{}`; remove a named file that is not on that way",
                    at.label
                ));
            }
        }
    }

    /// Files placed at `target`, or at `target/placed` when the call lands
    /// a source of that name in the folder: a copy's, move's or link's
    /// destination, or an extraction's folder. A path that is or lies in a
    /// location refuses for any placement; a tree or an archive also
    /// refuses at a folder above a location, which it can create.
    #[allow(clippy::too_many_arguments)]
    fn placement(
        &self,
        found: &mut Vec<String>,
        name: &str,
        target: &str,
        placed: Option<&str>,
        lands: Lands,
        bound: &Bound,
        dir: &str,
    ) {
        let Some(paths) = self.paths(target, bound, dir) else {
            // Unresolved: no finding.
            return;
        };
        for path in paths {
            let path = match placed {
                Some(placed) => path.join(placed),
                None => path,
            };
            if matches!(name, "ln" | "mv") {
                // A link or a move onto a link on the way to a location
                // replaces the link, which moves the location; a copy writes
                // through it and is judged where it lands.
                if let Some(at) = self.link_on_the_way(&walk(&path, false)) {
                    found.push(format!(
                        "`{name}` replaces `{target}`, a link on the way to the saved shell state `{}`; name a file that is not on that way",
                        at.label
                    ));
                    continue;
                }
            }
            let seen = walk(&path, true);
            if let Some(at) = self.locations().iter().find(|at| within(&seen, at)) {
                found.push(match lands {
                    Lands::File => format!(
                        "`{name}` writes `{target}`, which is saved shell state: `{}`",
                        at.label
                    ),
                    Lands::Tree | Lands::Archive => format!(
                        "`{name}` places files into `{target}`, which is, holds or lies in saved shell state: `{}`",
                        at.label
                    ),
                });
                continue;
            }
            if lands == Lands::File {
                continue;
            }
            let Some(at) = self.locations().iter().find(|at| holds(&seen, at)) else {
                continue;
            };
            found.push(match lands {
                Lands::Archive => format!(
                    "`{name}` extracts into `{}`, a folder above the saved shell state `{}`, and the guard cannot see what the archive holds: extract into a named sub-folder that is not above saved shell state (`{}`)",
                    shown(&lexical(&path)),
                    at.label,
                    if name == "unzip" { "-d <dir>" } else { "-C <dir>" },
                ),
                Lands::Tree | Lands::File => format!(
                    "`{name}` places files into `{target}`, which is, holds or lies in saved shell state: `{}`; copy into a named sub-folder that holds no saved shell state",
                    at.label
                ),
            });
        }
    }
}

/// The last name of a source operand, as a copy names what it lands in a
/// folder; `None` when it has none the guard can read.
fn last_name(source: &str) -> Option<String> {
    let name = source.trim_end_matches('/').rsplit('/').next()?;
    (!name.is_empty() && !matches!(name, "." | "..") && !has_glob(name) && !name.contains('$'))
        .then(|| name.to_string())
}

/// The words in `word` that may name a path a program writes, as the
/// startup check reads a writing program's arguments: the value of a
/// `key=value` operand (`dd of=FILE`), every tail of a short option that
/// can start a path (`-o~/x`), and the word or a long option's value.
fn operand_paths(word: &str) -> Vec<&str> {
    let mut out = Vec::new();
    if !word.starts_with('-') {
        if let Some((key, value)) = word.split_once('=') {
            if !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                out.push(value);
            }
        }
    }
    if word.starts_with('-') && !word.starts_with("--") {
        for (at, c) in word.char_indices().skip(2) {
            if matches!(c, '~' | '/' | '.') {
                out.push(&word[at..]);
            }
        }
    }
    let value = if word.starts_with('-') {
        word.split_once('=').map(|(_, v)| v)
    } else {
        Some(word)
    };
    out.extend(value);
    out.retain(|w| {
        !w.is_empty()
            && !w.contains(|c: char| {
                crate::hooks::git_guard::shell_blank(c)
                    || matches!(c, '\'' | '"' | '(' | ')' | ';' | '<' | '>' | '|' | '&')
            })
    });
    out
}

/// The operands of a call that reads `-` words as options until `--`.
fn plain_operands(args: &[String]) -> Vec<&str> {
    let mut operands = Vec::new();
    let mut only = false;
    for arg in args {
        if only || !arg.starts_with('-') || arg == "-" {
            operands.push(arg.as_str());
        } else if arg == "--" {
            only = true;
        }
    }
    operands
}

/// Holds the saved-state findings of the outermost evaluation of one hook
/// until it ends, so they follow every other finding of the call, the
/// findings of its nested commands included.
pub(crate) struct Held(usize);

/// A held finding, and whether the startup check's run over the saved-state
/// class gave it ([`readings`]), so it is dropped when the call already
/// refuses under the same rule.
type Kept = (bool, Violation);

thread_local! {
    static HELD: RefCell<[Option<Vec<Kept>>; 2]> = const { RefCell::new([None, None]) };
}

/// The hooks that hold findings, each in a slot of its own.
#[derive(Clone, Copy)]
pub(crate) enum Hook {
    Exec = 0,
    Git = 1,
}

impl Held {
    /// Start holding findings for `hook`; `None` inside an evaluation that
    /// already holds them, whose findings join the outer ones.
    pub(crate) fn enter(hook: Hook) -> Option<Self> {
        HELD.with(|held| {
            let mut held = held.borrow_mut();
            let slot = &mut held[hook as usize];
            if slot.is_some() {
                return None;
            }
            *slot = Some(Vec::new());
            Some(Self(hook as usize))
        })
    }

    /// Add `findings` to the ones `hook` holds; `read` marks the startup
    /// check's findings over the saved-state class.
    fn hold(hook: Hook, findings: Vec<Violation>, read: bool) {
        HELD.with(|held| {
            if let Some(slot) = held.borrow_mut()[hook as usize].as_mut() {
                slot.extend(findings.into_iter().map(|finding| (read, finding)));
            }
        });
    }

    /// Judge `command`, run in `cwd`, with the locations from this
    /// process's environment, and hold the findings for `hook`: the
    /// startup check over the saved-state class ([`readings`]), and the
    /// identity pass ([`evaluate`]) only when that check finds nothing.
    pub(crate) fn run(hook: Hook, command: &str, cwd: &Path) {
        let places = Places::from_process_at(cwd);
        let read = readings(command, cwd, &places);
        if read.is_empty() {
            Self::hold(hook, evaluate(command, cwd, &places), false);
        } else {
            Self::hold(hook, read, true);
        }
    }

    /// Append the findings `held` holds after `violations`; nothing for an
    /// inner evaluation, whose findings the outer one appends.
    pub(crate) fn finish(held: Option<Self>, violations: &mut Vec<Violation>) {
        if let Some(held) = held {
            held.append(violations);
        }
    }

    /// Append the held findings after `violations`, each once. A finding
    /// of the startup check over the saved-state class is dropped when the
    /// call already refuses under that rule: the startup check over its own
    /// class met the same rule first.
    pub(crate) fn append(self, violations: &mut Vec<Violation>) {
        let held = HELD.with(|held| held.borrow_mut()[self.0].take().unwrap_or_default());
        let refused = violations.iter().any(|v| v.rule == super::RULE);
        for (read, finding) in held {
            if read && refused {
                continue;
            }
            let duplicate = violations
                .iter()
                .any(|v| v.rule == finding.rule && v.message == finding.message);
            if !duplicate {
                violations.push(finding);
            }
        }
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        HELD.with(|held| held.borrow_mut()[self.0].take());
    }
}

#[cfg(test)]
#[path = "saved_state_tests.rs"]
mod tests;
