//! The saved shell state a harness or terminal tool runs before its later
//! commands (TSK-1461): Claude Code's `session-env` and `shell-snapshots`
//! folders and the file `CLAUDE_ENV_FILE` names, Codex's `shell_snapshots`
//! folder, and herdr-core's template folder.
//!
//! Two parts. The inventory joins the startup class ([`extend_class`]), so
//! the class's own readers refuse a write to one of these locations as they
//! refuse a write to a startup file: in their default places, where
//! `CLAUDE_CONFIG_DIR`, `CODEX_HOME` or `APPDATA` move them, and by folder
//! name wherever such a folder is.
//!
//! The pass ([`evaluate`]) judges what those readers do not: a removal,
//! move, copy or extraction whose target holds, lies above or is saved
//! state without naming it. It reads the line once with the deletion
//! check's reader ([`crate::security::deletion::bound_commands_in`]), which
//! keeps the working directory per command (a subshell's `cd` stays in the
//! subshell), and refuses only a target that reader binds exactly:
//!
//! - every reading of the command (each directory it may run in, each
//!   program a variable may name) must refuse, or the pass adds nothing;
//! - a target it cannot resolve (an unknown directory, a variable it does
//!   not hold, a glob it cannot list, a line it did not read to the end)
//!   adds nothing, and the other checks' judgment stands.
//!
//! Its findings come after every other finding of the call, so they never
//! change what another rule reports.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use super::{key, lexical, resolve, shown, Class, Entry, StartupEnv};
use crate::hooks::git_guard::{has_glob, startup_glob_paths};
use crate::hooks::Violation;
use crate::security::dangerous::{program_name, recursive_rm_operands};
use crate::security::deletion::{bound_commands_in, bound_path, find_removals, Bound};
use crate::security::unresolved::{writer_effects, writer_name, DirOrder};

/// The folder names a harness keeps its saved shell state in, protected
/// wherever such a folder is: a configuration folder moved by a variable
/// the guard does not hold, or another profile's, is covered too.
pub(crate) const SAVED_STATE_FOLDERS: &[&str] = &[
    "session-env",
    "shell-snapshots",
    "shell_snapshots",
    "herdr-core/templates",
];

/// How many levels below a target the scan reads for a saved-state folder.
const SCAN_DEPTH: usize = 4;
/// A directory at the depth cutoff is not read, so the scan cannot show
/// what is below it: the target then refuses with the narrower-target
/// message.
const REFUSE_AT_DEPTH_CUTOFF: bool = true;
/// The directory budget of the scans of one call: directories queued, the
/// target's metadata, location checks and link resolutions. It is the
/// guard's one budget for recursive reads.
const SCAN_DIR_LIMIT: usize = crate::hooks::git_guard::SECRET_ENTRY_LIMIT;
/// The entry ceiling of the scans of one call.
const SCAN_ENTRY_LIMIT: usize = 1 << 20;

#[cfg(test)]
thread_local! {
    static OFF: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether the saved-state inventory and pass run. Always outside tests;
/// a test turns both off to compare with the guard as it stood before.
#[cfg(not(test))]
pub(crate) fn enabled() -> bool {
    true
}

/// Whether the saved-state inventory and pass run.
#[cfg(test)]
pub(crate) fn enabled() -> bool {
    !OFF.with(std::cell::Cell::get)
}

/// Run `f` with the saved-state inventory and pass turned off.
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

/// Whether `reading` (a lower-case, slash-separated path) has the folder
/// names of `name` (`a` or `a/b`) as consecutive components.
pub(crate) fn dir_named(reading: &str, name: &str) -> bool {
    let want: Vec<&str> = name.split('/').collect();
    let parts: Vec<&str> = reading.split('/').collect();
    parts
        .windows(want.len())
        .any(|window| window == want.as_slice())
}

/// Whether `text`, a class label, needle or table entry, names saved state.
fn names_saved_state(text: &str) -> bool {
    let text = text.to_lowercase();
    SAVED_STATE_FOLDERS
        .iter()
        .any(|name| dir_named(&text, name))
}

/// The home folders a variable moves, with the variable and its value.
fn moved(env: &StartupEnv) -> [(&'static str, &'static str, Option<&PathBuf>); 4] {
    [
        (".config/", "XDG_CONFIG_HOME", env.xdg_config.as_ref()),
        (".claude/", "CLAUDE_CONFIG_DIR", env.claude_config.as_ref()),
        (".codex/", "CODEX_HOME", env.codex_home.as_ref()),
        ("AppData/Roaming/", "APPDATA", env.appdata.as_ref()),
    ]
}

/// The comparison readings of `path`: as written and through links.
fn readings_of(path: &Path) -> Vec<String> {
    let mut readings = vec![key(&lexical(path))];
    if let Ok(real) = resolve(path) {
        let real = key(&real);
        if !readings.contains(&real) {
            readings.push(real);
        }
    }
    readings
}

/// Add the saved shell state to the startup class: the locations the
/// variables the class does not already follow move, the file
/// `CLAUDE_ENV_FILE` names, the folder names protected wherever they are,
/// and their names for the text floor. With the pass off (a test), the
/// class is left as it stood before saved state joined it.
pub(super) fn extend_class(class: &mut Class, env: &StartupEnv) {
    if !enabled() {
        class.entries.retain(|entry| match entry {
            Entry::At { label, .. } => !names_saved_state(label),
            Entry::Name(_) | Entry::Dir(_) => true,
        });
        class.needles.retain(|needle| !names_saved_state(needle));
        return;
    }
    // `XDG_CONFIG_HOME` is followed by the class itself.
    let table = &crate::security::actions::table().startup_paths;
    for entry in &table.home {
        let dir = entry.ends_with('/');
        let rel = entry.trim_end_matches('/');
        if !names_saved_state(rel) {
            continue;
        }
        for (prefix, variable, root) in moved(env).into_iter().skip(1) {
            if let (Some(rest), Some(root)) = (rel.strip_prefix(prefix), root) {
                class.entries.push(Entry::At {
                    label: format!("${variable}/{rest}"),
                    readings: readings_of(&root.join(rest)),
                    dir,
                });
            }
        }
    }
    if let Some(file) = &env.claude_env_file {
        class.entries.push(Entry::At {
            label: "$CLAUDE_ENV_FILE".to_string(),
            readings: readings_of(file),
            dir: false,
        });
    }
    class.entries.extend(
        SAVED_STATE_FOLDERS
            .iter()
            .map(|name| Entry::Dir((*name).to_string())),
    );
    class.needles.extend(
        SAVED_STATE_FOLDERS
            .iter()
            .chain(&["claude_env_file"])
            .map(|name| (*name).to_string()),
    );
}

/// One saved-state location at a known path.
struct Location {
    label: String,
    path: PathBuf,
    readings: Vec<String>,
    dir: bool,
}

/// The saved-state locations at known paths: the defaults below each home,
/// the ones a variable the guard holds moves, and the file
/// `CLAUDE_ENV_FILE` names.
fn locations(env: &StartupEnv) -> Vec<Location> {
    let mut out = Vec::new();
    let mut add = |label: String, path: PathBuf, dir: bool| {
        let path = lexical(&path);
        out.push(Location {
            label,
            readings: readings_of(&path),
            path,
            dir,
        });
    };
    let table = &crate::security::actions::table().startup_paths;
    for entry in &table.home {
        let dir = entry.ends_with('/');
        let rel = entry.trim_end_matches('/');
        if !names_saved_state(rel) {
            continue;
        }
        for home in env.home.iter().chain(&env.other_homes) {
            add(format!("~/{rel}"), home.join(rel), dir);
        }
        for (prefix, variable, root) in moved(env) {
            if let (Some(rest), Some(root)) = (rel.strip_prefix(prefix), root) {
                add(format!("${variable}/{rest}"), root.join(rest), dir);
            }
        }
    }
    if let Some(file) = &env.claude_env_file {
        add("$CLAUDE_ENV_FILE".to_string(), file.clone(), false);
    }
    out
}

/// The variables the guard has values for, which the line may expand in a
/// target.
fn held(env: &StartupEnv) -> Vec<(String, String)> {
    [
        ("XDG_CONFIG_HOME", &env.xdg_config),
        ("ZDOTDIR", &env.zdotdir),
        ("CLAUDE_CONFIG_DIR", &env.claude_config),
        ("CODEX_HOME", &env.codex_home),
        ("APPDATA", &env.appdata),
        ("CLAUDE_ENV_FILE", &env.claude_env_file),
    ]
    .into_iter()
    .filter_map(|(name, value)| {
        let value = value.as_ref()?.to_str()?;
        Some((name.to_string(), value.to_string()))
    })
    .collect()
}

/// Judge `command`, run in `cwd`, for removals, moves, copies and
/// extractions over saved shell state that the startup class's readers do
/// not name. Every finding is `security.shell_startup`, always blocking.
#[must_use]
pub(crate) fn evaluate(command: &str, cwd: &Path, env: &StartupEnv) -> Vec<Violation> {
    if !enabled() {
        return Vec::new();
    }
    let line = bound_commands_in(command, Some(cwd), &held(env));
    if !line.whole || !line.commands.iter().any(|bound| operation(&bound.argv)) {
        return Vec::new();
    }
    let lower = command.to_lowercase();
    let pass = Pass {
        class: Class::new(env),
        locations: locations(env),
        env,
        base: cwd,
        dots: ["dotglob", "globdots", "globignore"]
            .iter()
            .any(|word| lower.contains(word)),
        scans: std::cell::RefCell::new(ScanState::default()),
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
                if member.found.is_empty() {
                    every = false;
                    break 'readings;
                }
                found.extend(member.found);
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
        .map(|message| Violation::always_blocking(super::RULE, message, super::SANCTIONED))
        .collect()
}

/// Whether `argv` runs a program the pass judges.
fn operation(argv: &[String]) -> bool {
    argv.first().is_some_and(|first| {
        matches!(
            program_name(first).as_str(),
            "rm" | "rmdir" | "unlink" | "find"
        ) || matches!(
            writer_name(first),
            "mv" | "cp" | "ditto" | "rsync" | "tar" | "unzip"
        )
    })
}

/// What one reading of a command gives: its findings. A target the pass
/// cannot resolve adds none, so the reading refuses only through another.
#[derive(Default)]
struct Member {
    found: Vec<String>,
}

/// What a scan below a directory found.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Scan {
    /// No saved-state folder within the bound.
    Clear,
    /// A saved-state folder.
    Holds,
    /// The tree cannot be shown clear: a budget ran out, the depth cutoff
    /// left a subtree unread, or an entry, a directory or a listing could
    /// not be read.
    OutOfBudget,
}

/// What the scans of one call have read and found.
#[derive(Default)]
struct ScanState {
    /// Resolved directory and whether traversal follows directory links.
    known: HashMap<(PathBuf, bool), Scan>,
    /// Directories queued or read and metadata calls made, against
    /// [`SCAN_DIR_LIMIT`].
    dirs: usize,
    /// Entries listed, against [`SCAN_ENTRY_LIMIT`].
    entries: usize,
}

struct Pass<'a> {
    class: Class,
    locations: Vec<Location>,
    env: &'a StartupEnv,
    base: &'a Path,
    /// Whether the line may make a glob match names that start with `.`.
    dots: bool,
    scans: std::cell::RefCell<ScanState>,
}

impl Pass<'_> {
    /// One reading of a command: its program and arguments, run in `dir`.
    fn judge(&self, bound: &Bound, dir: &str) -> Member {
        let mut member = Member::default();
        let Some(first) = bound.argv.first() else {
            return member;
        };
        let args = &bound.argv[1..];
        match program_name(first).as_str() {
            "rm" => match recursive_rm_operands(args) {
                Some(operands) => {
                    for operand in operands {
                        self.removal(&mut member, "rm", "removes", operand, bound, dir);
                    }
                }
                None => {
                    for operand in plain_operands(args) {
                        self.named(&mut member, "rm", operand, bound, dir);
                    }
                }
            },
            name @ ("rmdir" | "unlink") => {
                for operand in plain_operands(args) {
                    self.named(&mut member, name, operand, bound, dir);
                }
            }
            "find" => {
                for start in find_removals(args).unwrap_or_default() {
                    self.removal(&mut member, "find", "removes", &start, bound, dir);
                }
            }
            _ => self.writer(&mut member, first, args, bound, dir),
        }
        member
    }

    /// A move, copy, sync or extraction, read with the writer table.
    fn writer(&self, member: &mut Member, first: &str, args: &[String], bound: &Bound, dir: &str) {
        let name = writer_name(first);
        if !matches!(name, "mv" | "cp" | "ditto" | "rsync" | "tar" | "unzip") {
            return;
        }
        let Some(effects) = writer_effects(first, args, &[]) else {
            return;
        };
        if effects.read.unknown.is_some() {
            // Unresolved: no finding.
            return;
        }
        let flagged = |names: &[&str]| {
            effects
                .read
                .flags
                .iter()
                .any(|flag| names.contains(&flag.as_str()))
        };
        if let Some(copy) = &effects.copy {
            let remote = |path: &str| {
                name == "rsync"
                    && (path.starts_with("rsync://")
                        || path.split('/').next().unwrap_or("").contains(':'))
            };
            if name == "mv" {
                for source in &copy.sources {
                    self.removal(member, "mv", "moves", source, bound, dir);
                }
            }
            let recursive = match name {
                "mv" | "ditto" => true,
                "cp" => flagged(&["-r", "-R", "-a", "--recursive", "--archive"]),
                "rsync" => flagged(&["-r", "-a", "--recursive", "--archive"]),
                _ => false,
            };
            if let Some(dest) = copy.dest.as_deref().filter(|dest| !remote(dest)) {
                let tree = recursive
                    && copy
                        .sources
                        .iter()
                        .filter(|source| !remote(source))
                        .any(|source| self.may_be_tree(source, bound, dir));
                self.placement(member, name, dest, tree, bound, dir);
            }
        }
        if matches!(name, "tar" | "unzip") {
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
                self.placement(member, name, &target, true, bound, dir);
            }
        }
    }

    /// The absolute paths `operand` names from `dir`, each match of a glob
    /// included; `None` when the pass cannot resolve it.
    fn paths(&self, operand: &str, bound: &Bound, dir: &str) -> Option<Vec<PathBuf>> {
        let text = bound_path(dir, operand)?;
        let path = self.absolute(&text, bound.home_assigned)?;
        if !has_glob(&text) {
            return Some(vec![path]);
        }
        let mut base = PathBuf::new();
        let mut rest = PathBuf::new();
        for component in path.components() {
            if rest.as_os_str().is_empty() && !has_glob(&component.as_os_str().to_string_lossy()) {
                base.push(component);
            } else {
                rest.push(component);
            }
        }
        let matches = startup_glob_paths(&base, &rest, self.dots, &self.class)?;
        // A word that matches nothing stays as written.
        Some(if matches.is_empty() {
            vec![path]
        } else {
            matches
        })
    }

    /// A path the reader resolved, made absolute: from the home the class
    /// is built from, from the root, or from where the line started.
    fn absolute(&self, text: &str, home_assigned: bool) -> Option<PathBuf> {
        if let Some(rest) = text.strip_prefix('~') {
            if home_assigned || !(rest.is_empty() || rest.starts_with('/')) {
                return None;
            }
            let home = self.env.home.as_ref()?;
            return Some(lexical(&home.join(rest.trim_start_matches('/'))));
        }
        if text.starts_with('/') {
            // A `/`-rooted path names no fixed place on Windows.
            return (!cfg!(windows)).then(|| lexical(Path::new(text)));
        }
        Some(lexical(&self.base.join(text)))
    }

    /// Whether a copy's source may be a directory tree: a directory that is
    /// not a link, or a source the pass cannot read.
    fn may_be_tree(&self, source: &str, bound: &Bound, dir: &str) -> bool {
        let Some(paths) = self.paths(source, bound, dir) else {
            return true;
        };
        paths.iter().any(|path| {
            match crate::absence::symlink_metadata_optional(path) {
                Ok(Some(metadata)) => metadata.is_dir(),
                Ok(None) => false,
                Err(_) => true,
            }
        })
    }

    /// A tree removed (`rm -r`, `find -delete`) or moved away (`mv`).
    fn removal(
        &self,
        member: &mut Member,
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
            let path = if follow {
                resolve(&path).unwrap_or(path)
            } else {
                path
            };
            match self.removal_of(&path) {
                Some(Scan::Holds) => member.found.push(format!(
                    "`{name}` {verb} `{operand}`, which is or holds saved shell state: `{}`",
                    shown(&path)
                )),
                Some(Scan::OutOfBudget) => member.found.push(format!(
                    "`{name}` {verb} `{operand}`, which the guard cannot show is clear of saved shell state: {}",
                    out_of_budget(&path)
                )),
                Some(Scan::Clear) => {}
                None => {}
            }
        }
    }

    /// Whether a removal of `path` takes saved state with it. The node
    /// itself is checked, then, for a directory that is not a link, the tree
    /// below it without following links. `None` for a spelling the guard
    /// cannot map.
    fn removal_of(&self, path: &Path) -> Option<Scan> {
        let mapped = crate::portable_path::guard_path(path).ok()?;
        if !self.spend_directory() {
            return Some(Scan::OutOfBudget);
        }
        let metadata = match crate::absence::symlink_metadata_optional(&mapped) {
            Ok(None) => return Some(Scan::Clear),
            Ok(Some(metadata)) => metadata,
            Err(_) => return Some(Scan::OutOfBudget),
        };
        match self.existing_state_related(&mapped) {
            Scan::Clear => {}
            other => return Some(other),
        }
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Some(Scan::Clear);
        }
        let Ok(real) = resolve(&mapped) else {
            return Some(Scan::OutOfBudget);
        };
        if real != mapped {
            match self.existing_state_related(&real) {
                Scan::Clear => {}
                other => return Some(other),
            }
        }
        Some(self.scan_below(&real, false))
    }

    /// `rmdir`, `unlink` or a plain `rm`: refused when the path is a
    /// saved-state location or lies in one and something is there.
    fn named(&self, member: &mut Member, name: &str, operand: &str, bound: &Bound, dir: &str) {
        let Some(paths) = self.paths(operand, bound, dir) else {
            // Unresolved: no finding.
            return;
        };
        for path in paths {
            // The last component is removed itself, so only the folders
            // above it are followed.
            let mut readings = vec![key(&lexical(&path))];
            if let (Some(parent), Some(file)) = (path.parent(), path.file_name()) {
                if let Ok(real) = resolve(parent) {
                    let real = key(&real.join(file));
                    if !readings.contains(&real) {
                        readings.push(real);
                    }
                }
            }
            let Some(label) = readings.iter().find_map(|r| self.location_of(r)) else {
                continue;
            };
            match crate::absence::symlink_metadata_optional(&path) {
                Ok(None) => {}
                Ok(Some(_)) | Err(_) => member.found.push(format!(
                    "`{name}` removes `{operand}`, which is saved shell state: `{label}`"
                )),
            }
        }
    }

    /// Files placed into `target`: a copy's or move's destination, or an
    /// extraction's directory. A location by path refuses for any
    /// placement; a tree placed also refuses into a directory above one, and
    /// into a directory whose tree holds one or cannot be read in full.
    fn placement(
        &self,
        member: &mut Member,
        name: &str,
        target: &str,
        tree: bool,
        bound: &Bound,
        dir: &str,
    ) {
        let Some(paths) = self.paths(target, bound, dir) else {
            // Unresolved: no finding.
            return;
        };
        for path in paths {
            let readings = readings_of(&path);
            let at = readings.iter().find_map(|r| self.location_of(r)).or_else(|| {
                tree.then(|| readings.iter().find_map(|r| self.location_below(r)))
                    .flatten()
            });
            if let Some(label) = at {
                member.found.push(format!(
                    "`{name}` places files into `{target}`, which is, holds or lies in saved shell state: `{label}`"
                ));
                continue;
            }
            if !tree {
                continue;
            }
            match self.scan_below(&path, true) {
                Scan::Clear => {}
                Scan::Holds => member.found.push(format!(
                    "`{name}` places files into `{target}`, which holds saved shell state below it: `{}`",
                    shown(&path)
                )),
                Scan::OutOfBudget => member.found.push(format!(
                    "`{name}` places files into `{target}`, which the guard cannot show is clear of saved shell state: {}",
                    out_of_budget(&path)
                )),
            }
        }
    }

    /// The location a reading is or lies in, or the folder name it has.
    fn location_of(&self, reading: &str) -> Option<String> {
        let reading = reading.trim_end_matches('/');
        self.locations
            .iter()
            .find(|location| {
                location.readings.iter().any(|at| {
                    reading == at
                        || (location.dir
                            && reading
                                .strip_prefix(at.as_str())
                                .is_some_and(|rest| rest.starts_with('/')))
                })
            })
            .map(|location| location.label.clone())
            .or_else(|| {
                SAVED_STATE_FOLDERS
                    .iter()
                    .find(|name| dir_named(reading, name))
                    .map(|name| (*name).to_string())
            })
    }

    /// The location below a reading: the reading is a directory above it.
    fn location_below(&self, reading: &str) -> Option<String> {
        let below = format!("{}/", reading.trim_end_matches('/'));
        self.locations
            .iter()
            .find(|location| location.readings.iter().any(|at| at.starts_with(&below)))
            .map(|location| location.label.clone())
    }

    fn spend_directory(&self) -> bool {
        let mut state = self.scans.borrow_mut();
        state.dirs += 1;
        state.dirs <= SCAN_DIR_LIMIT
    }

    /// Whether `path` is, holds or lies in a saved-state location that
    /// exists now, or has a saved-state folder name and exists.
    fn existing_state_related(&self, path: &Path) -> Scan {
        let reading = key(&lexical(path));
        let reading = reading.trim_end_matches('/');
        let below = format!("{reading}/");
        for location in &self.locations {
            let related = location.readings.iter().any(|at| {
                reading == at
                    || at.starts_with(&below)
                    || (location.dir
                        && reading
                            .strip_prefix(at.as_str())
                            .is_some_and(|rest| rest.starts_with('/')))
            });
            if !related {
                continue;
            }
            if !self.spend_directory() {
                return Scan::OutOfBudget;
            }
            match crate::absence::symlink_metadata_optional(&location.path) {
                Ok(Some(_)) => return Scan::Holds,
                Ok(None) => {}
                Err(_) => return Scan::OutOfBudget,
            }
        }
        if dir_run_at(path) {
            Scan::Holds
        } else {
            Scan::Clear
        }
    }

    /// Whether a resolved path is equal to, above or inside a saved-state
    /// location, or has a saved-state folder name.
    fn saved_state_path(&self, path: &Path) -> bool {
        let reading = key(&lexical(path));
        let reading = reading.trim_end_matches('/');
        self.location_of(reading).is_some() || self.location_below(reading).is_some()
    }

    /// Whether a saved-state folder exists in `dir` or a few levels below
    /// it. Fail closed: a budget spent, a subtree past the depth cutoff, or
    /// an entry, a directory or a listing that cannot be read is
    /// [`Scan::OutOfBudget`], never clear.
    fn scan_below(&self, dir: &Path, follow_links: bool) -> Scan {
        let Ok(real) = resolve(dir)
            .map_err(|_| ())
            .and_then(|path| crate::portable_path::guard_path(&path).map_err(|_| ()))
        else {
            return Scan::OutOfBudget;
        };
        let real = match crate::portable_path::canonicalize(&real) {
            Ok(canonical) => canonical,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => real,
            Err(_) => return Scan::OutOfBudget,
        };
        let mut state = self.scans.borrow_mut();
        if let Some(known) = state.known.get(&(real.clone(), follow_links)) {
            return *known;
        }
        let found = self.scan_for_saved_state(&real, &mut state, follow_links);
        state.known.insert((real, follow_links), found);
        found
    }

    fn scan_for_saved_state(&self, dir: &Path, state: &mut ScanState, follow_links: bool) -> Scan {
        state.dirs += 1;
        if state.dirs > SCAN_DIR_LIMIT {
            return Scan::OutOfBudget;
        }
        match crate::absence::metadata_optional(dir) {
            Ok(None) => return Scan::Clear,
            Ok(Some(metadata)) if !metadata.is_dir() => return Scan::Clear,
            Ok(Some(_)) => {}
            Err(_) => return Scan::OutOfBudget,
        }
        // Breadth first, so the shallow levels are read before the bound.
        // A repository's own administrative tree is read without a depth
        // cutoff; other trees keep it.
        let admin = own_git_directory(dir).then(|| dir.to_path_buf());
        let mut pending = VecDeque::from([(dir.to_path_buf(), 0_usize, admin)]);
        let mut visited = HashSet::from([dir.to_path_buf()]);
        state.dirs += 1;
        if state.dirs > SCAN_DIR_LIMIT {
            return Scan::OutOfBudget;
        }
        while let Some((current, depth, admin)) = pending.pop_front() {
            let Ok(entries) = std::fs::read_dir(&current) else {
                return Scan::OutOfBudget;
            };
            for entry in entries {
                state.entries += 1;
                if state.entries > SCAN_ENTRY_LIMIT {
                    return Scan::OutOfBudget;
                }
                let Ok(entry) = entry else {
                    return Scan::OutOfBudget;
                };
                let Ok(kind) = entry.file_type() else {
                    return Scan::OutOfBudget;
                };
                if (kind.is_dir() || (!follow_links && kind.is_symlink()))
                    && dir_run_at(&entry.path())
                {
                    return Scan::Holds;
                }
                let next = if kind.is_symlink() {
                    if !follow_links {
                        continue;
                    }
                    state.dirs += 1;
                    if state.dirs > SCAN_DIR_LIMIT {
                        return Scan::OutOfBudget;
                    }
                    let Ok(real) = resolve(&entry.path())
                        .map_err(|_| ())
                        .and_then(|path| crate::portable_path::guard_path(&path).map_err(|_| ()))
                    else {
                        return Scan::OutOfBudget;
                    };
                    if self.saved_state_path(&real) {
                        return Scan::Holds;
                    }
                    match crate::absence::metadata_optional(&real) {
                        Ok(Some(metadata)) if metadata.is_dir() => {
                            let Ok(canonical) = crate::portable_path::canonicalize(&real) else {
                                return Scan::OutOfBudget;
                            };
                            canonical
                        }
                        Ok(Some(_)) => continue,
                        Ok(None) | Err(_) => return Scan::OutOfBudget,
                    }
                } else if kind.is_dir() {
                    entry.path()
                } else {
                    continue;
                };
                let next_admin = admin
                    .as_ref()
                    .filter(|root| next.starts_with(root))
                    .cloned()
                    .or_else(|| own_git_directory(&entry.path()).then(|| next.clone()));
                if let Some(known) = state.known.get(&(next.clone(), follow_links)) {
                    match known {
                        Scan::Clear => continue,
                        other => return *other,
                    }
                }
                if !visited.insert(next.clone()) {
                    continue;
                }
                if next_admin.is_none() && depth + 1 >= SCAN_DEPTH {
                    if REFUSE_AT_DEPTH_CUTOFF {
                        return Scan::OutOfBudget;
                    }
                    continue;
                }
                state.dirs += 1;
                if state.dirs > SCAN_DIR_LIMIT {
                    return Scan::OutOfBudget;
                }
                pending.push_back((next, depth + 1, next_admin));
            }
        }
        for path in visited {
            state.known.insert((path, follow_links), Scan::Clear);
        }
        Scan::Clear
    }
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

/// Whether `path` ends in a saved-state folder name and may be a directory.
fn dir_run_at(path: &Path) -> bool {
    let reading = key(&lexical(path));
    SAVED_STATE_FOLDERS
        .iter()
        .any(|name| dir_named(&reading, name))
        && crate::absence::is_kind(path, true) != Some(false)
}

/// Only the administrative directory of its parent repository is read
/// without the depth cutoff. A folder merely named `.git` keeps the
/// ordinary rules.
fn own_git_directory(path: &Path) -> bool {
    if path.file_name().is_none_or(|name| name != ".git") {
        return false;
    }
    let Some(parent) = path.parent() else {
        return false;
    };
    let Ok(repo) = git2::Repository::open(parent) else {
        return false;
    };
    match (
        crate::portable_path::canonicalize(path),
        crate::portable_path::canonicalize(repo.path()),
    ) {
        (Ok(candidate), Ok(actual)) => candidate == actual,
        _ => false,
    }
}

/// How a refusal names a target the scan cannot show clear, with the form
/// that passes.
fn out_of_budget(path: &Path) -> String {
    format!(
        "`{}` (the guard cannot read this target in full to check for saved shell state; name a narrower target directory that the guard can read)",
        shown(path)
    )
}

#[cfg(test)]
#[path = "saved_state_tests.rs"]
mod tests;
