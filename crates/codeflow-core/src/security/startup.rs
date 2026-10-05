//! The shell startup class (issue 86, TSK-242): the files a shell or
//! terminal runs when it starts. A function or alias planted in one is
//! loaded by every later shell, and every command-text guard then judges
//! the name it sees, not what runs.
//!
//! Containment comes from the harness sandboxes, generated from the same
//! list (`actions.json`, `startup_paths`). This module is the backstop the
//! guards run on every harness: [`class_target`] for the native edit tools
//! and [`evaluate`] for shell calls, both under the always-blocking rule
//! `security.shell_startup`. No policy key relaxes it.
//!
//! The shell check reads the command with git-guard's reader and refuses
//! the writes it can see: a redirect or a writing program whose target is
//! in the class (spelled in any form the guard can expand, through a glob,
//! a brace expansion or a symbolic link), a placing program that writes
//! into the home, `/etc` or a startup directory, interpreter code or a
//! heredoc that names a startup file, a startup variable set for a shell
//! that reads startup files, and `direnv allow`. What it cannot see is
//! stated, not guessed: a path built at run time, a script written in one
//! call and run in another, and a program that writes the file on its own.
//! A sandbox that denies the writes is the containment for those.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use super::actions;
use crate::hooks::git_guard::{
    basename, command_argv, expand_commands, has_glob, is_shell, launcher_effects, redirect_writes,
    run_dirs, shell_tokens, strip_launchers, strip_reserved_words, unresolved_word, word_readings,
};
use crate::hooks::Violation;

/// The rule id of every finding here.
pub const RULE: &str = "security.shell_startup";

/// The sanctioned path printed with every refusal.
pub const SANCTIONED: &str = "shell startup files belong to the operator: ask the operator to make this change by hand, outside the agent session; no policy key relaxes this rule, and the harness sandbox denies these writes where one runs (issue 86)";

/// The most entries a glob word is expanded over before the guard stops
/// and judges the word as unresolved.
const GLOB_LIMIT: usize = 4096;

/// The most symbolic links one path is followed through.
const LINK_LIMIT: usize = 40;

/// The zsh startup files, which `ZDOTDIR` moves.
const ZSH_FILES: &[&str] = &[".zshenv", ".zprofile", ".zshrc", ".zlogin", ".zlogout"];

/// Where the class lives for this guard: the home and the directories the
/// environment moves startup files to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StartupEnv {
    /// The home directory (`HOME`, else `USERPROFILE`).
    pub home: Option<PathBuf>,
    /// `ZDOTDIR`, when set: zsh reads its startup files there.
    pub zdotdir: Option<PathBuf>,
    /// `XDG_CONFIG_HOME`, when set: fish, `PowerShell`, tmux and direnv read
    /// their configuration there.
    pub xdg_config: Option<PathBuf>,
}

impl StartupEnv {
    /// The locations from this process's environment. An empty value, or a
    /// relative home, is ignored. A relative `ZDOTDIR` is kept as it is:
    /// zsh reads it from whatever directory it starts in, so the class then
    /// protects the zsh file names in every directory. A relative
    /// `XDG_CONFIG_HOME` is read from this process's directory (review
    /// round three).
    #[must_use]
    pub fn from_process() -> Self {
        Self::from_process_at(&std::env::current_dir().unwrap_or_default())
    }

    /// The same, with a relative `XDG_CONFIG_HOME` read from `base`, the
    /// directory the command runs in (security review F-5).
    #[must_use]
    pub fn from_process_at(base: &Path) -> Self {
        let var = |name: &str| {
            std::env::var_os(name)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        };
        let absolute = |name: &str| var(name).filter(|p| p.is_absolute());
        Self {
            home: absolute("HOME").or_else(|| absolute("USERPROFILE")),
            zdotdir: var("ZDOTDIR"),
            xdg_config: var("XDG_CONFIG_HOME").map(
                |x| {
                    if x.is_absolute() {
                        x
                    } else {
                        base.join(&x)
                    }
                },
            ),
        }
    }

    /// The same locations with another home, a relative `XDG_CONFIG_HOME`
    /// read from `base`.
    #[must_use]
    pub fn with_home_at(home: &Path, base: &Path) -> Self {
        Self {
            home: Some(home.to_path_buf()),
            ..Self::from_process_at(base)
        }
    }
}

/// One protected location: a file, a directory and everything below it,
/// or a file name in every directory.
#[derive(Debug, Clone)]
enum Entry {
    /// A file or directory at a known path, with its two readings
    /// (lexical and through symbolic links), lower case.
    At {
        label: String,
        readings: Vec<String>,
        dir: bool,
    },
    /// A file name protected in every directory.
    Name(String),
}

/// The class, resolved once for one evaluation.
pub(crate) struct Class {
    entries: Vec<Entry>,
    home: Option<String>,
    /// The text floor's needles: each entry's spelling below its anchor.
    needles: Vec<String>,
}

impl Class {
    /// The class for these locations.
    #[must_use]
    pub(crate) fn new(env: &StartupEnv) -> Self {
        let table = &actions::table().startup_paths;
        let mut entries = Vec::new();
        let mut anywhere_zsh = Vec::new();
        let mut needles = Vec::new();
        let mut add = |label: String, path: PathBuf, dir: bool| {
            let mut readings = vec![key(&lexical(&path))];
            if let Some(real) = resolve(&path) {
                let real = key(&real);
                if !readings.contains(&real) {
                    readings.push(real);
                }
            }
            entries.push(Entry::At {
                label,
                readings,
                dir,
            });
        };
        for entry in &table.home {
            let dir = entry.ends_with('/');
            let rel = entry.trim_end_matches('/');
            needles.push(rel.to_lowercase());
            if let Some(home) = &env.home {
                add(format!("~/{rel}"), home.join(rel), dir);
            }
            let zsh = ZSH_FILES
                .iter()
                .any(|f| rel == *f || rel.strip_prefix(f) == Some(".zwc"));
            match (zsh, &env.zdotdir) {
                (true, Some(zdot)) if zdot.is_absolute() => {
                    add(format!("$ZDOTDIR/{rel}"), zdot.join(rel), dir);
                }
                // A relative `ZDOTDIR` names a different directory for each
                // place zsh starts, so the file name is protected anywhere.
                (true, Some(_)) => anywhere_zsh.push(rel.to_lowercase()),
                _ => {}
            }
            if let (Some(rest), Some(xdg)) = (rel.strip_prefix(".config/"), &env.xdg_config) {
                add(format!("$XDG_CONFIG_HOME/{rest}"), xdg.join(rest), dir);
            }
        }
        for entry in &table.absolute {
            let dir = entry.ends_with('/');
            let path = entry.trim_end_matches('/');
            needles.push(path.to_lowercase());
            add(path.to_string(), PathBuf::from(path), dir);
        }
        entries.extend(anywhere_zsh.into_iter().map(Entry::Name));
        for name in &table.anywhere {
            needles.push(name.to_lowercase());
            entries.push(Entry::Name(name.to_lowercase()));
        }
        // Names a reader of the code may meet without the directory part.
        needles.extend(
            ["config.fish", "profile.ps1"]
                .iter()
                .map(|s| (*s).to_string()),
        );
        Self {
            entries,
            home: env.home.as_deref().map(|h| key(&lexical(h))),
            needles,
        }
    }

    /// The class entry `path` is or lies in, read lexically and through
    /// symbolic links. A path whose links cannot be followed is judged by
    /// its lexical reading and by the link text it holds.
    #[must_use]
    pub(crate) fn target(&self, path: &Path) -> Option<String> {
        let mut readings = vec![key(&lexical(path))];
        if let Some(real) = resolve(path) {
            readings.push(key(&real));
        }
        readings.iter().find_map(|reading| self.entry_of(reading))
    }

    fn entry_of(&self, reading: &str) -> Option<String> {
        self.entries.iter().find_map(|entry| match entry {
            Entry::At {
                label,
                readings,
                dir,
            } => readings
                .iter()
                .any(|p| {
                    reading == p
                        || (*dir
                            && reading
                                .strip_prefix(p.as_str())
                                .is_some_and(|rest| rest.starts_with('/')))
                })
                .then(|| label.clone()),
            Entry::Name(name) => {
                (reading.rsplit('/').next() == Some(name.as_str())).then(|| name.clone())
            }
        })
    }

    /// Whether `path` is a directory a placing program could fill with a
    /// startup file it does not name: the home, `/etc`, the root, or any
    /// directory above a class entry.
    #[must_use]
    pub(crate) fn placement(&self, path: &Path) -> Option<String> {
        let mut readings = vec![key(&lexical(path))];
        if let Some(real) = resolve(path) {
            readings.push(key(&real));
        }
        readings.iter().find_map(|reading| {
            let reading = reading.trim_end_matches('/');
            if reading.is_empty() || self.home.as_deref() == Some(reading) || reading == "/etc" {
                return Some(shown(path));
            }
            let below = format!("{reading}/");
            self.entries
                .iter()
                .any(|entry| match entry {
                    Entry::At { readings, .. } => readings.iter().any(|p| p.starts_with(&below)),
                    Entry::Name(_) => false,
                })
                .then(|| shown(path))
        })
    }

    /// The startup file `text` names, as a path or a file name, in any
    /// letter case and with either slash. A name inside a longer name
    /// (`user.profile`, `my.zshrc.txt` before the dot) does not count.
    #[must_use]
    pub(crate) fn named_in(&self, text: &str) -> Option<String> {
        let text = text.replace('\\', "/").to_lowercase();
        self.needles
            .iter()
            .find(|needle| mentions(&text, needle))
            .cloned()
    }
}

/// Whether `needle` occurs in `text` as a path or a name: not preceded by
/// a name character, and not followed by one.
fn mentions(text: &str, needle: &str) -> bool {
    let name_char = |c: char| c.is_alphanumeric() || matches!(c, '_' | '-');
    text.match_indices(needle).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + needle.len()..].chars().next();
        before.is_none_or(|c| !(name_char(c) || c == '.')) && after.is_none_or(|c| !name_char(c))
    })
}

/// A path as the class compares it: slashes, lower case.
fn key(path: &Path) -> String {
    crate::portable_path::slashed(path).to_lowercase()
}

fn shown(path: &Path) -> String {
    crate::portable_path::slashed(path)
}

/// `path` with `.` and `..` folded and repeated separators removed,
/// without reading the file system.
fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// `path` with each symbolic link followed, one component at a time, a
/// broken link included (its target is where a write lands). Missing
/// components are kept. `None` past [`LINK_LIMIT`] links or when a
/// component cannot be inspected.
fn resolve(path: &Path) -> Option<PathBuf> {
    let mut pending: Vec<std::ffi::OsString> = path
        .components()
        .rev()
        .map(|c| c.as_os_str().to_os_string())
        .collect();
    let mut out = PathBuf::new();
    let mut links = 0;
    while let Some(part) = pending.pop() {
        let component = Path::new(&part);
        match component.components().next() {
            Some(Component::CurDir) | None => continue,
            Some(Component::ParentDir) => {
                out.pop();
                continue;
            }
            Some(Component::RootDir | Component::Prefix(_)) => {
                out.push(component);
                continue;
            }
            Some(Component::Normal(_)) => {}
        }
        let next = out.join(component);
        match std::fs::symlink_metadata(&next) {
            Ok(meta) if meta.file_type().is_symlink() => {
                links += 1;
                if links > LINK_LIMIT {
                    return None;
                }
                let target = std::fs::read_link(&next).ok()?;
                if target.is_absolute() {
                    out = PathBuf::new();
                }
                pending.extend(
                    target
                        .components()
                        .rev()
                        .map(|c| c.as_os_str().to_os_string()),
                );
            }
            Ok(_) => out = next,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => out = next,
            Err(_) => return None,
        }
    }
    Some(out)
}

/// What a word names as a write target.
enum Judged {
    /// A class entry.
    Class(String),
    /// A directory a placing program could fill with a startup file.
    Placement(String),
    /// A part the guard cannot read: a variable it does not know, a
    /// substitution, `~user`, a brace or glob expansion too large to read.
    Unresolved(String),
    /// None of these.
    Ordinary,
}

/// The facts of one command line that every word is judged against.
#[derive(Clone)]
struct Line<'a> {
    class: &'a Class,
    env: &'a StartupEnv,
    text: &'a str,
    /// Literal values the line assigns (`p=~/.zshrc`), so `"$p"` reads
    /// as its value.
    assigned: BTreeMap<String, String>,
    /// Every directory a command on the line can run in.
    dirs: Vec<PathBuf>,
    /// Why that list may miss one.
    unknown: Option<String>,
}

impl Line<'_> {
    /// The path `word` names from `dir`, or `None` when part of it is
    /// filled in at run time.
    fn expand(&self, word: &str, dir: &Path) -> Option<PathBuf> {
        let text = self.substitute(word);
        if unresolved_word(&text) {
            return None;
        }
        let home = || self.env.home.clone();
        let path = if text == "~" {
            home()?
        } else if let Some(rest) = text.strip_prefix("~/") {
            home()?.join(rest)
        } else if text == "~+" {
            dir.to_path_buf()
        } else if let Some(rest) = text.strip_prefix("~+/") {
            dir.join(rest)
        } else if text.starts_with('~') {
            return None;
        } else {
            dir.join(&text)
        };
        Some(path)
    }

    /// `word` with each variable the guard can read replaced by its value.
    fn substitute(&self, word: &str) -> String {
        let mut text = word.to_string();
        // Known variables, the line's own literal assignments first.
        for (name, value) in self.vars() {
            for form in [format!("${{{name}}}"), format!("${name}")] {
                while let Some(at) = text.find(&form) {
                    let end = at + form.len();
                    let continues = form.starts_with("${")
                        || !text[end..]
                            .chars()
                            .next()
                            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
                    if !continues {
                        break;
                    }
                    text.replace_range(at..end, &value);
                }
            }
        }
        text
    }

    /// The variables the guard can read: the line's literal assignments,
    /// then the locations of the class.
    fn vars(&self) -> Vec<(String, String)> {
        let mut vars: Vec<(String, String)> = self
            .assigned
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let shown_env = |p: &Option<PathBuf>| p.as_deref().map(shown);
        for (name, value) in [
            ("HOME", shown_env(&self.env.home)),
            ("ZDOTDIR", shown_env(&self.env.zdotdir)),
            ("XDG_CONFIG_HOME", shown_env(&self.env.xdg_config)),
        ] {
            if let Some(value) = value {
                if !self.assigned.contains_key(name) {
                    vars.push((name.to_string(), value));
                }
            }
        }
        // Longer names first, so `$HOMEDIR` is never read as `$HOME`.
        vars.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
        vars
    }

    /// Judge `word` as a write target. A word that uses a variable the
    /// line assigns is judged with that value, and also as unresolved: the
    /// assignment can come after the use, so the inherited value may be
    /// the one the shell expands (review round two).
    fn judge(&self, word: &str, dirs: &[PathBuf]) -> Judged {
        let judged = self.judge_literal(word, dirs);
        if matches!(judged, Judged::Ordinary) && self.uses_assigned(word) {
            return Judged::Unresolved(word.to_string());
        }
        judged
    }

    /// Whether `word` expands a variable the line assigns.
    fn uses_assigned(&self, word: &str) -> bool {
        self.assigned.keys().any(|name| {
            [
                format!("${{{name}}}"),
                format!("${{{name}:"),
                format!("${{{name}-"),
            ]
            .iter()
            .any(|form| word.contains(form.as_str()))
                || word.match_indices(&format!("${name}")).any(|(at, m)| {
                    !word[at + m.len()..]
                        .starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_')
                })
        })
    }

    /// Judge `word` as a write target from every directory the command can
    /// run in, each brace reading of it, and each path a glob in it reaches.
    fn judge_literal(&self, word: &str, dirs: &[PathBuf]) -> Judged {
        if word.is_empty() {
            return Judged::Ordinary;
        }
        let Some(readings) = word_readings(word) else {
            return Judged::Unresolved(word.to_string());
        };
        let mut placement = None;
        for reading in &readings {
            for dir in dirs {
                let Some(path) = self.expand(reading, dir) else {
                    return Judged::Unresolved(reading.clone());
                };
                let text = shown(&path);
                if has_glob(&text) {
                    match self.glob(&text) {
                        Judged::Ordinary => {}
                        Judged::Placement(p) => placement = Some(p),
                        other => return other,
                    }
                    continue;
                }
                if let Some(label) = self.class.target(&path) {
                    return Judged::Class(label);
                }
                if placement.is_none() {
                    placement = self.class.placement(&path);
                }
            }
        }
        placement.map_or(Judged::Ordinary, Judged::Placement)
    }

    /// A glob pattern: matched against each class path as text, then
    /// expanded over the file system with every match resolved through
    /// symbolic links (a link to a startup file counts). A leading `.` must
    /// be spelled, as in Bash and zsh by default, unless the line turns on
    /// an option that lets a pattern match it (`dotglob`, `GLOBIGNORE`,
    /// `globdots`).
    fn glob(&self, pattern: &str) -> Judged {
        let folded = self.text.to_lowercase().replace('_', "");
        let dots = ["dotglob", "globignore", "globdots"]
            .iter()
            .any(|option| folded.contains(option));
        let options = glob::MatchOptions {
            case_sensitive: false,
            require_literal_separator: true,
            require_literal_leading_dot: !dots,
        };
        let Ok(compiled) = glob::Pattern::new(&pattern.to_lowercase()) else {
            return Judged::Unresolved(pattern.to_string());
        };
        for entry in &self.class.entries {
            if let Entry::At {
                label, readings, ..
            } = entry
            {
                if readings.iter().any(|p| compiled.matches_with(p, options)) {
                    return Judged::Class(label.clone());
                }
            }
        }
        // The walk is bounded by the entries it lists, which the match limit
        // below does not count: a tree over the budget, or one a directory
        // link could extend, is not walked and refuses near the class
        // (review F-1 and round two).
        if !glob_within_budget(pattern) {
            return Judged::Unresolved(pattern.to_string());
        }
        // A shell without `globstar` reads `**` as `*`, which also matches
        // files; the glob crate reads it as directories only. Both readings
        // are walked (review round two).
        let mut readings = vec![pattern.to_string()];
        if pattern.contains("**") {
            readings.push(pattern.replace("**", "*"));
        }
        let mut placement = None;
        for reading in &readings {
            let Ok(paths) = glob::glob_with(reading, options) else {
                return Judged::Unresolved(pattern.to_string());
            };
            for (seen, path) in paths.enumerate() {
                if seen >= GLOB_LIMIT {
                    return Judged::Unresolved(pattern.to_string());
                }
                let Ok(path) = path else {
                    return Judged::Unresolved(pattern.to_string());
                };
                if let Some(label) = self.class.target(&path) {
                    return Judged::Class(label);
                }
                if placement.is_none() {
                    placement = self.class.placement(&path);
                }
            }
        }
        placement.map_or(Judged::Ordinary, Judged::Placement)
    }

    /// The paths a source word names: each brace reading, from each run
    /// directory, with globs expanded within the walk budget. `None` when a
    /// reading cannot be expanded or read whole (review round six:
    /// `cp -P links/* out/`).
    fn source_paths(&self, word: &str, dirs: &[PathBuf]) -> Option<Vec<PathBuf>> {
        // Wider than the shell's matching (any case, dot files too), so a
        // match it would make is never missed.
        let options = glob::MatchOptions {
            case_sensitive: false,
            require_literal_separator: true,
            require_literal_leading_dot: false,
        };
        let mut out = Vec::new();
        for reading in word_readings(word)? {
            for dir in dirs {
                let path = self.expand(&reading, dir)?;
                let text = shown(&path);
                if !has_glob(&text) {
                    out.push(path);
                    continue;
                }
                if !glob_within_budget(&text) {
                    return None;
                }
                let mut patterns = vec![text.clone()];
                if text.contains("**") {
                    patterns.push(text.replace("**", "*"));
                }
                for pattern in patterns {
                    for (seen, path) in glob::glob_with(&pattern, options).ok()?.enumerate() {
                        if seen >= GLOB_LIMIT {
                            return None;
                        }
                        out.push(path.ok()?);
                    }
                }
            }
        }
        Some(out)
    }

    /// Whether an unresolved word may name a startup file: the line names
    /// one, or the literal directory before the part the shell fills in is
    /// the home, `/etc` or a startup directory.
    fn unresolved_near_class(&self, word: &str, dirs: &[PathBuf]) -> Option<String> {
        if let Some(name) = self.class.named_in(self.text) {
            return Some(format!("the line names `{name}`"));
        }
        for dir in dirs {
            if let Some(path) = self.expand(word, dir) {
                let text = shown(&path);
                if has_glob(&text) && !glob_within_budget(&text) {
                    return Some(format!(
                        "its glob reaches more than the {GLOB_LIMIT} entries the guard reads, or a directory link"
                    ));
                }
            }
        }
        if self.uses_assigned(word) {
            // Judged with the inherited value as well as the assigned one.
            let bare = Line {
                assigned: BTreeMap::new(),
                ..self.clone()
            };
            if let Some(why) = bare.unresolved_near_class(word, dirs) {
                return Some(why);
            }
        }
        if let Some(rest) = word.strip_prefix('~') {
            let user = rest.split('/').next().unwrap_or_default();
            if !user.is_empty() && user != "+" {
                return Some(format!("`{word}` names a home the guard cannot read"));
            }
        }
        let known = self.substitute(word);
        let cut = known
            .find(['$', '`', '\u{1}', '\u{2}', '*', '?', '[', '{'])
            .unwrap_or(known.len());
        let literal = &known[..cut];
        let dir_part = literal.rfind('/').map_or("", |at| &literal[..=at]);
        if dir_part.is_empty() {
            // A relative name filled in at run time (`"$DEST"`, `.$NAME`)
            // lands in the directory the command runs in.
            if literal.starts_with('/') {
                return None;
            }
            return dirs.iter().find_map(|dir| {
                self.class
                    .placement(dir)
                    .map(|p| format!("it lands in `{p}` when it is a relative name"))
            });
        }
        for dir in dirs {
            if let Some(path) = self.expand(dir_part.trim_end_matches('/'), dir) {
                let path = if dir_part == "/" {
                    PathBuf::from("/")
                } else {
                    path
                };
                if let Some(p) = self
                    .class
                    .target(&path)
                    .or_else(|| self.class.placement(&path))
                {
                    return Some(format!("its directory is `{p}`"));
                }
            }
        }
        None
    }
}

/// Programs that only read the files they are given and write nothing but
/// their standard output, judged by operation: `printf -v`, `file -C` and
/// the git subcommands with `--output` are writers. `less`, `more` and
/// `bat` are not here: a log file, a shell escape or a preprocessor from a
/// configuration file can write; `rg` and `sed` are judged by operation.
const READERS: &[&str] = &[
    "cat",
    "head",
    "tail",
    "grep",
    "egrep",
    "fgrep",
    "wc",
    "ls",
    "stat",
    "file",
    "diff",
    "cmp",
    "test",
    "[",
    "echo",
    "printf",
    "realpath",
    "readlink",
    "basename",
    "dirname",
    "od",
    "hexdump",
    "strings",
    "jq",
    "md5",
    "md5sum",
    "shasum",
    "sha1sum",
    "sha256sum",
    "sha512sum",
    "cksum",
    "b2sum",
    "true",
    "false",
    "type",
    "which",
];

/// Git subcommands that only read.
const GIT_READS: &[&str] = &[
    "status",
    "log",
    "diff",
    "show",
    "blame",
    "ls-files",
    "ls-tree",
    "grep",
    "cat-file",
    "rev-parse",
    "describe",
    "shortlog",
    "rev-list",
    "for-each-ref",
    "name-rev",
];

/// Git subcommands that write no file in the directory git runs in other
/// than one they name, so that directory does not matter to the class. A
/// path they name is still judged word by word (`git config -f ~/.zshrc`,
/// `git worktree add ~/.zsh`).
const GIT_KEEPS_WORKTREE: &[&str] = &[
    "push",
    "fetch",
    "remote",
    "ls-remote",
    "tag",
    "branch",
    "commit",
    "add",
    "rm",
    "notes",
    "config",
    "init",
    "gc",
    "prune",
    "repack",
    "fsck",
    "reflog",
    "count-objects",
    "verify-commit",
    "verify-tag",
    "maintenance",
    "clean",
    "worktree",
    "update-ref",
    "symbolic-ref",
    "update-index",
    "write-tree",
    "commit-tree",
    "mktag",
    "mktree",
    "hash-object",
    "pack-refs",
    "show-ref",
    "show-branch",
    "merge-base",
    "check-ignore",
    "check-attr",
    "check-ref-format",
    "cherry",
    "range-diff",
    "var",
];

/// Programs that can write files whose names are not on the command line
/// (archive members, a download's remote name, a checkout's tree), so a
/// destination or working directory in the home or a startup directory
/// refuses.
const PLACING: &[&str] = &[
    "tar", "bsdtar", "gtar", "unzip", "cpio", "pax", "7z", "7za", "7zz", "ditto", "rsync", "git",
    "patch", "stow", "chezmoi", "yadm", "wget", "curl", "scp", "svn", "hg",
];

/// Programs that copy or link named sources to a destination.
const COPIERS: &[&str] = &["cp", "ln", "install", "mv", "rsync", "scp", "ditto"];

/// Programs that run code they are given or a file they are pointed at.
const RUNNERS: &[&str] = &[
    "sh",
    "bash",
    "zsh",
    "dash",
    "ksh",
    "ash",
    "mksh",
    "fish",
    "pwsh",
    "powershell",
    "source",
    ".",
    "eval",
    "exec",
    "python",
    "python3",
    "node",
    "nodejs",
    "deno",
    "bun",
    "perl",
    "ruby",
    "php",
    "lua",
    "osascript",
    "xargs",
];

/// The startup variables, and the shells each one changes.
const STARTUP_VARS: &[&str] = &[
    "ZDOTDIR",
    "HOME",
    "BASH_ENV",
    "ENV",
    "PROMPT_COMMAND",
    "XDG_CONFIG_HOME",
];

fn finding(message: String) -> Violation {
    Violation::always_blocking(RULE, message, SANCTIONED)
}

/// Judge a shell command line for writes to the startup class.
#[must_use]
pub fn evaluate(command: &str, cwd: &Path, env: &StartupEnv) -> Vec<Violation> {
    let class = Class::new(env);
    let segments = expand_commands(command);
    let run = run_dirs(&segments, cwd);
    let mut line = Line {
        class: &class,
        env,
        text: command,
        assigned: BTreeMap::new(),
        dirs: run.dirs.clone(),
        unknown: run.unknown.clone(),
    };
    line.assigned = literal_assignments(&segments);
    if let Some(v) = direnv_trust(&segments) {
        return vec![v];
    }
    if let Some(v) = startup_environment(&segments) {
        return vec![v];
    }
    for code in super::outward::interpreter_bodies(command) {
        if let Some(name) = class.named_in(&code) {
            return vec![finding(format!(
                "interpreter code names the shell startup file `{name}`, which it can write"
            ))];
        }
    }
    if let Some(v) = staged_run(&segments, &line) {
        return vec![v];
    }
    for segment in &segments {
        if let Some(v) = segment_violation(segment, &line) {
            return vec![v];
        }
    }
    Vec::new()
}

/// The literal values the line assigns, read only where the shell reads an
/// assignment: the words before a command, and the operands of `export`,
/// `declare`, `typeset`, `local` and `readonly`. A name assigned anywhere
/// with more than one value, or with a value the guard cannot read, is
/// left out, so a word that uses it stays unresolved and is judged as such.
fn literal_assignments(segments: &[String]) -> BTreeMap<String, String> {
    let is_name = |name: &str| {
        !name.is_empty()
            && !name.starts_with(|c: char| c.is_ascii_digit())
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    };
    let mut values: BTreeMap<String, Option<String>> = BTreeMap::new();
    let note = |values: &mut BTreeMap<String, Option<String>>, name: &str, value: Option<&str>| {
        let value = value.filter(|v| !unresolved_word(v)).map(str::to_string);
        values
            .entry(name.to_string())
            .and_modify(|seen| {
                if *seen != value {
                    *seen = None;
                }
            })
            .or_insert(value);
    };
    for segment in segments {
        let tokens = shell_tokens(segment);
        let mut words = tokens.iter().map(String::as_str);
        let mut declaring = false;
        for word in words.by_ref() {
            if let Some((name, value)) = word.split_once('=') {
                if is_name(name) {
                    note(&mut values, name, Some(value));
                    continue;
                }
            }
            if declaring && word.starts_with(['-', '+']) {
                continue;
            }
            if matches!(
                word,
                "export" | "declare" | "typeset" | "local" | "readonly"
            ) {
                declaring = true;
                continue;
            }
            if declaring && is_name(word) {
                continue;
            }
            // The command word: the rest are its arguments.
            break;
        }
        // An argument that looks like an assignment (`echo p=x`) assigns
        // nothing, but a name it shares with a real one makes that name
        // unclear, so it is dropped.
        for word in words {
            if let Some((name, _)) = word.split_once('=') {
                if is_name(name) && values.contains_key(name) {
                    note(&mut values, name, None);
                }
            }
        }
    }
    values
        .into_iter()
        .filter_map(|(name, value)| value.map(|v| (name, v)))
        .collect()
}

/// `direnv allow`, `permit` or `grant` trusts an `.envrc`, which then runs
/// in every shell that enters its directory.
fn direnv_trust(segments: &[String]) -> Option<Violation> {
    segments.iter().find_map(|segment| {
        let mut words = command_argv(segment);
        strip_reserved_words(&mut words);
        let (program, args) = strip_launchers(&words)?;
        (basename(program) == "direnv"
            && args
                .first()
                .is_some_and(|a| matches!(a.as_str(), "allow" | "permit" | "grant")))
        .then(|| {
            finding(
                "`direnv allow` trusts an `.envrc`, which then runs in every shell that enters its directory"
                    .to_string(),
            )
        })
    })
}

/// A startup variable set on a line that launches a shell which reads
/// startup files through it.
fn startup_environment(segments: &[String]) -> Option<Violation> {
    let mut set = Vec::new();
    for segment in segments {
        for token in shell_tokens(segment) {
            for var in STARTUP_VARS {
                if token
                    .strip_prefix(var)
                    .is_some_and(|rest| rest.starts_with('='))
                    && !set.contains(var)
                {
                    set.push(*var);
                }
            }
        }
    }
    if set.is_empty() {
        return None;
    }
    for segment in segments {
        let mut words = command_argv(segment);
        strip_reserved_words(&mut words);
        let Some((program, args)) = strip_launchers(&words) else {
            continue;
        };
        let name = basename(program).trim_end_matches(".exe").to_lowercase();
        let reads: &[&str] = match startup_reader(&name, args) {
            Some(vars) => vars,
            None => continue,
        };
        if let Some(var) = set.iter().find(|v| reads.contains(v)) {
            return Some(finding(format!(
                "`{var}` is set on a line that starts `{name}`, which reads its startup files through it"
            )));
        }
    }
    None
}

/// Whether a zsh launch has its startup files off: a `-f`, `--no-rcs` or
/// `--norcs` among the options, and nothing that could turn them back on.
/// Any `+` option, any `-o` (attached or not) and any long option other
/// than those two leave them on, since the guard does not read zsh's whole
/// option grammar; `-c` takes the next word as its code, and options are
/// read up to the first other word (review round two).
fn zsh_skips_rcs(args: &[String]) -> bool {
    let mut off = false;
    let mut at = 0;
    while let Some(arg) = args.get(at) {
        let a = arg.as_str();
        if a == "--" || a == "-" || !(a.starts_with('-') || a.starts_with('+')) {
            break;
        }
        if let Some(long) = a.strip_prefix("--") {
            if matches!(long, "no-rcs" | "norcs") {
                off = true;
            } else {
                return false;
            }
        } else {
            let body = &a[1..];
            if a.starts_with('+') || body.contains('o') {
                return false;
            }
            if body.contains('f') {
                off = true;
            }
            if body.contains('c') {
                // The code string is not an option.
                at += 1;
            }
        }
        at += 1;
    }
    off
}

/// The startup variables a shell launch reads, or `None` for a program
/// that reads none.
fn startup_reader(name: &str, args: &[String]) -> Option<&'static [&'static str]> {
    // Only `-` options: a `+` option turns the same setting off.
    let has = |flags: &[&str]| {
        args.iter()
            .take_while(|a| a.starts_with('-') || a.starts_with('+'))
            .filter(|a| a.starts_with('-'))
            .any(|a| {
                flags.contains(&a.as_str())
                    || (!a.starts_with("--")
                        && a.len() > 1
                        && flags
                            .iter()
                            .filter(|f| f.len() == 2 && !f.starts_with("--"))
                            .any(|f| a[1..].contains(&f[1..])))
            })
    };
    let interactive_or_login = has(&["-i", "-l", "--login"]);
    match name {
        "zsh" if zsh_skips_rcs(args) => None,
        "zsh" => Some(&["ZDOTDIR", "HOME"]),
        "bash" if interactive_or_login => Some(&["BASH_ENV", "HOME", "ENV", "PROMPT_COMMAND"]),
        "bash" => Some(&["BASH_ENV"]),
        "sh" | "dash" | "ksh" | "ash" | "mksh" if interactive_or_login => Some(&["ENV", "HOME"]),
        "fish" if has(&["-N", "--no-config"]) => None,
        "pwsh" | "powershell"
            if args.iter().any(|a| {
                a.eq_ignore_ascii_case("-noprofile") || a.eq_ignore_ascii_case("-nop")
            }) =>
        {
            None
        }
        "fish" | "pwsh" | "powershell" => Some(&["HOME", "XDG_CONFIG_HOME"]),
        "tmux" | "screen" | "script" | "login" => Some(&[
            "HOME",
            "ZDOTDIR",
            "ENV",
            "BASH_ENV",
            "XDG_CONFIG_HOME",
            "PROMPT_COMMAND",
        ]),
        "exec" if args.first().is_some_and(|a| is_shell(basename(a))) => {
            Some(&["HOME", "ZDOTDIR", "ENV", "BASH_ENV"])
        }
        _ => None,
    }
}

/// A heredoc, here-string or piped text that names a startup file, on a
/// line that also runs code or a file: "write the script, then run it"
/// in one call.
fn staged_run(segments: &[String], line: &Line<'_>) -> Option<Violation> {
    if !(line.text.contains("<<") || line.text.contains('|')) {
        return None;
    }
    let name = line.class.named_in(line.text)?;
    segments.iter().find_map(|segment| {
        let mut words = command_argv(segment);
        strip_reserved_words(&mut words);
        let (program, _) = strip_launchers(&words)?;
        let runner = RUNNERS.contains(&basename(program))
            || program.starts_with("./")
            || (program.starts_with('/') && !READERS.contains(&basename(program)));
        runner.then(|| {
            finding(format!(
                "a heredoc or piped text names the shell startup file `{name}`, and the line runs `{program}`, which can write it"
            ))
        })
    })
}

/// The writes one simple command makes.
fn segment_violation(segment: &str, line: &Line<'_>) -> Option<Violation> {
    let mut tokens = command_argv(segment);
    strip_reserved_words(&mut tokens);
    let raw = shell_tokens(segment);
    let dirs: Vec<PathBuf> = {
        let moves = launcher_effects(&raw).0;
        line.dirs
            .iter()
            .map(|dir| {
                moves.iter().fold(dir.clone(), |d, m| {
                    line.expand(m, &d).unwrap_or_else(|| d.join(m))
                })
            })
            .collect()
    };
    // Redirects write their targets whatever the program.
    let redirects = redirect_writes(segment);
    for target in redirects.targets.iter().chain(&redirects.unread) {
        match line.judge(target, &dirs) {
            Judged::Class(label) => {
                return Some(finding(format!(
                    "a redirect writes the shell startup file `{label}`"
                )))
            }
            Judged::Unresolved(word) => {
                if let Some(why) = line.unresolved_near_class(&word, &dirs) {
                    return Some(finding(format!(
                        "a redirect writes `{word}`, which the guard cannot resolve, and {why}"
                    )));
                }
            }
            Judged::Placement(_) | Judged::Ordinary => {
                if line.unknown.is_some() && !target.starts_with(['/', '~', '$']) {
                    if let Some(file) = line.class.named_in(target) {
                        return Some(finding(format!(
                            "a redirect runs where the guard cannot tell the directory, and `{target}` could name the shell startup file `{file}`"
                        )));
                    }
                }
            }
        }
    }
    let (program, args) = strip_launchers(&tokens)?;
    let name = basename(program);
    if reads_only(name, args) {
        return None;
    }
    if is_shell(name) || name == "eval" {
        // The script a shell is given is judged as its own commands; the
        // reader expands `-c` strings and `eval` arguments into segments.
        return None;
    }
    if let Some(call) = copy_call(name, args) {
        return copy_judgment(name, &call, line, &dirs).or_else(|| {
            (name == "rsync")
                .then(|| placing_violation(name, args, line, &dirs))
                .flatten()
        });
    }
    for word in args {
        if let Some(v) = word_violation(name, word, line, &dirs) {
            return Some(v);
        }
    }
    placing_violation(name, args, line, &dirs)
}

/// Whether a `sed` call only prints. Its options must be ones that change
/// no file and read no script file (`-n`, `-E`, `-r`, `-s`, `-u`, `-z` and
/// their long forms), and every script, from `-e` or the first operand,
/// must be a list of print-only commands that [`sed_script_prints`] reads.
fn sed_reads(args: &[String]) -> bool {
    const FLAGS: &[&str] = &[
        "--quiet",
        "--silent",
        "--regexp-extended",
        "--separate",
        "--unbuffered",
        "--null-data",
        "--posix",
    ];
    let mut scripts = Vec::new();
    let mut operands = Vec::new();
    let mut iter = args.iter();
    let mut options = true;
    while let Some(arg) = iter.next() {
        let a = arg.as_str();
        if !options || a == "-" || !a.starts_with('-') {
            operands.push(a.to_string());
            continue;
        }
        if a == "--" {
            options = false;
        } else if a == "-e" || a == "--expression" {
            match iter.next() {
                Some(script) => scripts.push(script.clone()),
                None => return false,
            }
        } else if let Some(script) = a.strip_prefix("--expression=") {
            scripts.push(script.to_string());
        } else if FLAGS.contains(&a) {
        } else if a.starts_with("--") {
            return false;
        } else {
            let cluster = &a[1..];
            match cluster.find(|c: char| !"nErsuz".contains(c)) {
                None => {}
                Some(at) if cluster[at..].starts_with('e') => {
                    let rest = &cluster[at + 1..];
                    if rest.is_empty() {
                        match iter.next() {
                            Some(script) => scripts.push(script.clone()),
                            None => return false,
                        }
                    } else {
                        scripts.push(rest.to_string());
                    }
                }
                Some(_) => return false,
            }
        }
    }
    if scripts.is_empty() {
        if operands.is_empty() {
            return false;
        }
        scripts.push(operands.remove(0));
    }
    scripts.iter().all(|s| sed_script_prints(s))
}

/// Whether a sed script is only addressed print commands: each command,
/// split at `;` or a newline, is an optional address (a line number, `$`
/// or `/regex/`, a range of two, an optional `!`) and one of `p`, `P`,
/// `d`, `D`, `q`, `Q`, `=`, `l`, `n`, `N`, `g`, `G`, `h`, `H`, `x`, `z`,
/// or `s/regex/text/` with only the `g`, `p`, `i`, `I` or number flags.
/// Anything else, a `w`, `r`, `e` or another delimiter included, is not.
fn sed_script_prints(script: &str) -> bool {
    fn address(s: &str) -> Option<&str> {
        let s = s.trim_start();
        if let Some(rest) = s.strip_prefix('$') {
            return Some(rest);
        }
        if let Some(rest) = s.strip_prefix('/') {
            let end = rest.find('/')?;
            return (!rest[..end].contains('\\')).then(|| &rest[end + 1..]);
        }
        let digits = s.len() - s.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        (digits > 0).then(|| &s[digits..])
    }
    let mut any = false;
    for command in script.split([';', '\n']) {
        let mut s = command.trim();
        if s.is_empty() {
            continue;
        }
        any = true;
        if let Some(rest) = address(s) {
            s = rest.trim_start();
            if let Some(rest) = s.strip_prefix(',') {
                let Some(rest) = address(rest) else {
                    return false;
                };
                s = rest.trim_start();
            }
            s = s.strip_prefix('!').unwrap_or(s).trim_start();
        }
        let ok = if let Some(rest) = s.strip_prefix("s/") {
            let parts: Vec<&str> = rest.splitn(3, '/').collect();
            parts.len() == 3
                && !parts[0].contains('\\')
                && !parts[1].contains('\\')
                && parts[2]
                    .trim_end()
                    .chars()
                    .all(|c| matches!(c, 'g' | 'p' | 'i' | 'I') || c.is_ascii_digit())
        } else {
            let mut chars = s.chars();
            let first = chars.next();
            let rest = chars.as_str().trim();
            first.is_some_and(|c| "pPdDqQ=lnNgGhHxz".contains(c))
                && (rest.is_empty()
                    || (matches!(first, Some('q' | 'Q'))
                        && rest.chars().all(|c| c.is_ascii_digit())))
        };
        if !ok {
            return false;
        }
    }
    any
}

/// Whether a command only reads the paths it names.
fn reads_only(name: &str, args: &[String]) -> bool {
    match name {
        "printf" => !args.iter().any(|a| a.starts_with("-v")),
        "file" => !args.iter().any(|a| a == "-C" || a == "--compile"),
        "source" | "." => true,
        // `sed -n 1,20p ~/.zshrc` reads. Only a script the guard can read
        // whole, made of commands that print, counts (review round three:
        // GNU sed's `e` runs a shell command).
        "sed" => sed_reads(args),
        // ripgrep writes nothing, but `--pre`, on the command line or in the
        // file `RIPGREP_CONFIG_PATH` names, runs a program on each file.
        "rg" => {
            args.iter().any(|a| a == "--no-config")
                && !args.iter().any(|a| a == "--pre" || a.starts_with("--pre="))
        }
        "git" => git_subcommand(args).is_some_and(|(sub, rest)| {
            GIT_READS.contains(&sub)
                && !rest
                    .iter()
                    .any(|a| a.starts_with("--output") || a == "-o" || a.starts_with("--ext-diff"))
        }),
        "find" => !args.iter().any(|a| {
            matches!(
                a.as_str(),
                "-delete"
                    | "-exec"
                    | "-execdir"
                    | "-ok"
                    | "-okdir"
                    | "-fprint"
                    | "-fprint0"
                    | "-fprintf"
                    | "-fls"
            )
        }),
        _ => READERS.contains(&name),
    }
}

/// A git subcommand after git's global options, with its arguments.
fn git_subcommand(args: &[String]) -> Option<(&str, &[String])> {
    let mut at = 0;
    while let Some(arg) = args.get(at) {
        if matches!(
            arg.as_str(),
            "-C" | "-c" | "--git-dir" | "--work-tree" | "--namespace" | "--exec-path"
        ) {
            at += 2;
        } else if arg.starts_with('-') {
            at += 1;
        } else {
            return Some((arg.as_str(), &args[at + 1..]));
        }
    }
    None
}

/// The `-C` directories among git's global options, in order.
fn git_dirs(args: &[String]) -> Vec<&str> {
    let mut dirs = Vec::new();
    let mut at = 0;
    while let Some(arg) = args.get(at) {
        if matches!(
            arg.as_str(),
            "-C" | "-c" | "--git-dir" | "--work-tree" | "--namespace" | "--exec-path"
        ) {
            if arg == "-C" {
                if let Some(dir) = args.get(at + 1) {
                    dirs.push(dir.as_str());
                }
            }
            at += 2;
        } else if arg.starts_with('-') {
            at += 1;
        } else {
            break;
        }
    }
    dirs
}

/// The value part of an option word (`--output=~/.zshrc`, `-o~/.zshrc`
/// is left as written), or the word itself.
fn value_of(word: &str) -> &str {
    if word.starts_with('-') {
        word.split_once('=').map_or(word, |(_, v)| v)
    } else {
        word
    }
}

/// Whether a word reads as one path, not code or prose: no blank, quote,
/// bracket or statement character.
fn path_like(word: &str) -> bool {
    !word.is_empty()
        && !word.contains(|c: char| {
            c.is_whitespace() || matches!(c, '\'' | '"' | '(' | ')' | ';' | '<' | '>' | '|' | '&')
        })
}

/// One argument of a program that may write: a path in the class, an
/// unresolved path near it, or code that names a startup file.
fn word_violation(name: &str, word: &str, line: &Line<'_>, dirs: &[PathBuf]) -> Option<Violation> {
    // `key=value` operands (`dd of=FILE`) name a path in their value.
    if !word.starts_with('-') {
        if let Some((key, value)) = word.split_once('=') {
            if !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                if let Some(v) = word_violation(name, value, line, dirs) {
                    return Some(v);
                }
            }
        }
    }
    // A short option with its value attached (`-o~/.zshrc`, `-so$HOME/x`):
    // every tail that can start a path is judged as one.
    if word.starts_with('-') && !word.starts_with("--") {
        for (at, c) in word.char_indices().skip(2) {
            if matches!(c, '~' | '/' | '$' | '.') {
                if let Some(v) = word_violation(name, &word[at..], line, dirs) {
                    return Some(v);
                }
            }
        }
    }
    let value = value_of(word);
    if !path_like(value) {
        return line.class.named_in(word).map(|file| {
            finding(format!(
                "`{name}` is given text that names the shell startup file `{file}`, which it can write"
            ))
        });
    }
    // `{}` is filled in by `find -exec`, `xargs -I` or `parallel`.
    let value_owned;
    let value = if value.contains("{}") {
        value_owned = value.replace("{}", "$__found");
        value_owned.as_str()
    } else {
        value
    };
    match line.judge(value, dirs) {
        Judged::Class(label) => Some(finding(format!(
            "`{name}` targets the shell startup file `{label}`"
        ))),
        Judged::Unresolved(w) => line.unresolved_near_class(&w, dirs).map(|why| {
            finding(format!(
                "`{name}` takes `{w}`, which the guard cannot resolve, and {why}"
            ))
        }),
        Judged::Placement(_) | Judged::Ordinary => {
            if line.unknown.is_some() && !value.starts_with(['/', '~', '$']) {
                if let Some(file) = line.class.named_in(value) {
                    return Some(finding(format!(
                        "`{name}` runs where the guard cannot tell the directory, and `{value}` could name the shell startup file `{file}`"
                    )));
                }
            }
            None
        }
    }
}

/// Whether an option of this copying program takes the next word as its
/// value, so that word is not read as a source or destination. Each program
/// has its own table: `cp -f` forces, `rsync -f` takes a filter (review
/// round two).
fn copy_value_option(name: &str, a: &str) -> bool {
    match name {
        "cp" | "mv" | "ln" => matches!(a, "-S" | "--suffix"),
        "install" => matches!(
            a,
            "-S" | "--suffix" | "-m" | "--mode" | "-o" | "--owner" | "-g" | "--group"
        ),
        "rsync" => matches!(
            a,
            "-e" | "--rsh"
                | "-f"
                | "--filter"
                | "--exclude"
                | "--include"
                | "--exclude-from"
                | "--include-from"
                | "--files-from"
                | "--backup-dir"
                | "--suffix"
                | "--link-dest"
                | "--compare-dest"
                | "--copy-dest"
                | "--chmod"
                | "--chown"
                | "--temp-dir"
                | "-T"
                | "--partial-dir"
                | "-B"
                | "--block-size"
        ),
        "scp" => matches!(a, "-i" | "-o" | "-P" | "-F" | "-c" | "-l" | "-S" | "-J"),
        "ditto" => matches!(a, "--arch" | "--bom"),
        _ => false,
    }
}

/// A copy, link or move with a source and a destination.
struct CopyCall {
    dest: String,
    sources: Vec<String>,
    recursive: bool,
    /// What the call makes of each source.
    mode: LinkMode,
    /// A source that is itself a symbolic link is copied as that link.
    keeps_links: bool,
}

/// Whether a copier copies its sources or links to them.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LinkMode {
    Copy,
    Hard,
    /// A symbolic link, whose text is read from where it lands.
    Symbolic,
}

/// The operands of a copy, link or move, or `None` for another program or
/// a single operand (`install -d DIR`, judged word by word instead).
fn copy_call(name: &str, args: &[String]) -> Option<CopyCall> {
    if !COPIERS.contains(&name) {
        return None;
    }
    let mut operands = Vec::new();
    let mut target = None;
    let mut recursive = name == "ditto";
    let mut flags = LinkFlags {
        preserve: name == "ditto",
        ..LinkFlags::default()
    };
    let mut iter = args.iter();
    let mut options = true;
    while let Some(arg) = iter.next() {
        let a = arg.as_str();
        if options && a == "--" {
            options = false;
            continue;
        }
        if options && a.starts_with('-') && a.len() > 1 {
            // The option letters of a short cluster, before any value.
            let mut letters = &a[1..];
            // `--target-directory` in any prefix GNU accepts, with the
            // value attached or next (review round six: `--target-dir=`).
            let names_target = matches!(name, "cp" | "mv" | "ln" | "install")
                && long_prefix(a, "target-directory");
            if names_target || a == "-t" {
                target = match a.split_once('=') {
                    Some((_, dir)) => Some(dir.to_string()),
                    None => iter.next().cloned(),
                };
            } else if copy_value_option(name, a) {
                iter.next();
            } else if !a.starts_with("--") && matches!(name, "cp" | "mv" | "ln" | "install") {
                // A short cluster: the first option letter that takes a
                // value takes the rest of the word, or the next word
                // (`-t"$HOME"`, `-ft DIR`; review round three).
                for (at, letter) in a[1..].char_indices() {
                    let value = a[1 + at + letter.len_utf8()..].to_string();
                    let takes = letter == 'S'
                        || letter == 't'
                        || (name == "install" && matches!(letter, 'm' | 'o' | 'g' | 'l'));
                    if !takes {
                        continue;
                    }
                    letters = &a[1..=at];
                    let value = if value.is_empty() {
                        iter.next().cloned()
                    } else {
                        Some(value)
                    };
                    if letter == 't' {
                        target = value;
                    }
                    // BSD `install -l` links instead of copying; any
                    // link flag is judged as a symbolic link.
                    if letter == 'l' {
                        flags.symbolic = true;
                    }
                    break;
                }
            }
            let short = !a.starts_with("--");
            let has = |set: &[char]| short && letters.contains(set);
            // GNU programs accept any unambiguous prefix of a long option
            // (`--sym` for `--symbolic-link`), so a long option that may
            // name one is read as it (review round five, builder sweep).
            let long = |full: &str| long_prefix(a, full);
            recursive |=
                long("recursive") || long("archive") || long("mirror") || has(&['r', 'R', 'a']);
            flags.read(name, a, letters);
            continue;
        }
        operands.push(arg.clone());
    }
    let dest = match target {
        Some(dir) => dir,
        None if operands.len() >= 2 => operands.pop()?,
        None => return None,
    };
    Some(CopyCall {
        dest,
        sources: operands,
        recursive,
        mode: if flags.symbolic {
            LinkMode::Symbolic
        } else if flags.hard || name == "ln" {
            LinkMode::Hard
        } else {
            LinkMode::Copy
        },
        keeps_links: flags.preserve && !flags.follow,
    })
}

/// The link options a copier has read so far.
#[derive(Default)]
#[allow(clippy::struct_excessive_bools)]
struct LinkFlags {
    hard: bool,
    symbolic: bool,
    follow: bool,
    preserve: bool,
}

impl LinkFlags {
    /// Read one option word, `letters` being a short cluster's option
    /// letters before any value (review rounds four and five: `cp -s`,
    /// `install -l s`, `cp --sym`, `cp -LP`).
    fn read(&mut self, name: &str, a: &str, letters: &str) {
        let short = !a.starts_with("--");
        let has = |set: &[char]| short && letters.contains(set);
        let long = |full: &str| long_prefix(a, full);
        match name {
            "ln" => self.symbolic |= long("symbolic") || has(&['s']),
            "cp" => {
                self.hard |= long("link") || has(&['l']);
                self.symbolic |= long("symbolic-link") || has(&['s']);
                self.preserve |= long("no-dereference")
                    || long("archive")
                    || long("preserve")
                    || has(&['P', 'd', 'a', 'r', 'R']);
                // The last of `-L`/`-H` and `-P`/`-d`/`-a` wins, as cp
                // reads them (security review F-6).
                if a == "--dereference" {
                    self.follow = true;
                } else if long("no-dereference") || long("archive") {
                    self.follow = false;
                } else if short {
                    for letter in letters.chars() {
                        match letter {
                            'L' | 'H' => self.follow = true,
                            'P' | 'd' | 'a' => self.follow = false,
                            _ => {}
                        }
                    }
                }
            }
            "rsync" => {
                self.preserve |= long("links") || long("archive") || has(&['l', 'a']);
                self.follow |= matches!(a, "--copy-links" | "--copy-unsafe-links") || has(&['L']);
            }
            "install" => self.symbolic |= long("link"),
            _ => {}
        }
    }
}

/// Whether `arg` is a long option that may name `full`: `--full` or any
/// prefix of it GNU would accept, with or without `=value`. Only following
/// options are matched exactly, so a prefix never relaxes the judgment.
fn long_prefix(arg: &str, full: &str) -> bool {
    arg.strip_prefix("--")
        .map(|name| name.split('=').next().unwrap_or(name))
        .is_some_and(|name| !name.is_empty() && full.starts_with(name))
}

/// The directories a symbolic link's text is read from: the command's own,
/// and the one the link lands in (`ln -s ../../.zshrc out/rc`; review round
/// three), so its sources are judged from both.
fn link_text_dirs(dest: &str, sources: usize, line: &Line<'_>, dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = dirs.to_vec();
    for dir in dirs {
        if let Some(landing) = line.expand(dest, dir) {
            let at = if landing.is_dir() || sources > 1 {
                landing
            } else {
                landing.parent().map_or(landing.clone(), Path::to_path_buf)
            };
            if !out.contains(&at) {
                out.push(at);
            }
        }
    }
    out
}

/// A source a copy, link or move gives a second name: a startup file moved
/// or linked by any program, or a link to one copied as the link (review
/// round four: `cp -s`, `install -l s`, `cp -P`).
fn source_link_violation(
    name: &str,
    call: &CopyCall,
    source: &str,
    line: &Line<'_>,
    dirs: &[PathBuf],
    link_dirs: &[PathBuf],
    landing: &[PathBuf],
) -> Option<Violation> {
    let is_link =
        |p: &PathBuf| std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink());
    let keeps = call.keeps_links || name == "mv";
    // Every path the word names, globs and braces included; one the guard
    // cannot list refuses where a kept link could reach the class.
    let paths = line.source_paths(source, dirs);
    if paths.is_none() && keeps {
        if let Some(why) = line.unresolved_near_class(source, dirs) {
            return Some(finding(format!(
                "`{name}` copies `{source}` with its links kept, which the guard cannot list, and {why}"
            )));
        }
    }
    let paths = paths.unwrap_or_default();
    if keeps {
        // Each kept link, with the directories its text is read from
        // where it lands: the named links, and those inside a tree the
        // call copies or moves whole, walked within the budget (review
        // round six: `rsync -a links/ out/`).
        let mut kept: Vec<(PathBuf, Vec<PathBuf>)> = paths
            .iter()
            .filter(|p| is_link(p))
            .map(|p| (p.clone(), landing.to_vec()))
            .collect();
        if call.recursive || name == "mv" {
            for root in paths.iter().filter(|p| !is_link(p) && p.is_dir()) {
                kept.extend(tree_links(root, landing));
            }
        }
        for (link, text_dirs) in &kept {
            if let Some(v) = kept_link_violation(name, source, link, text_dirs, line) {
                return Some(v);
            }
        }
    }
    if name != "mv" && call.mode == LinkMode::Copy {
        return None;
    }
    match line.judge(source, link_dirs) {
        Judged::Class(label) => Some(finding(format!(
            "`{name}` moves or links the shell startup file `{label}`, so a later write through the new name edits it"
        ))),
        Judged::Unresolved(w) => line.unresolved_near_class(&w, link_dirs).map(|why| {
            finding(format!(
                "`{name}` moves or links `{w}`, which the guard cannot resolve, and {why}"
            ))
        }),
        Judged::Placement(_) | Judged::Ordinary => None,
    }
}

/// The symbolic links inside a tree, each with the directories its text is
/// read from once the tree lands in one of `landing`, either as itself or
/// as its contents. A tree over the walk budget is not listed (a stated
/// residual).
fn tree_links(root: &Path, landing: &[PathBuf]) -> Vec<(PathBuf, Vec<PathBuf>)> {
    let top = root.file_name().map(PathBuf::from).unwrap_or_default();
    let mut found = Vec::new();
    let mut seen = 0usize;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            seen += 1;
            if seen > GLOB_LIMIT {
                return Vec::new();
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                stack.push(entry.path());
            } else if kind.is_symlink() {
                let rel = dir.strip_prefix(root).unwrap_or(Path::new(""));
                let text_dirs = landing
                    .iter()
                    .flat_map(|l| [l.join(rel), l.join(&top).join(rel)])
                    .collect();
                found.push((entry.path(), text_dirs));
            }
        }
    }
    found
}

/// A kept link that is, or whose text from where it lands reaches, a
/// startup file.
fn kept_link_violation(
    name: &str,
    source: &str,
    kept: &Path,
    text_dirs: &[PathBuf],
    line: &Line<'_>,
) -> Option<Violation> {
    if let Some(label) = line.class.target(kept) {
        return Some(finding(format!(
            "`{name}` copies a link to the shell startup file `{label}`, so a later write through the copy edits it"
        )));
    }
    // The link's text is read again where the copy lands, so a relative
    // one can reach a startup file from there (review round five).
    let text = std::fs::read_link(kept).ok()?;
    let text = text.to_string_lossy().into_owned();
    match line.judge(&text, text_dirs) {
        Judged::Class(label) => Some(finding(format!(
            "`{name}` copies the link `{source}`, whose text reaches the shell startup file `{label}` where it lands"
        ))),
        Judged::Unresolved(w) => line.unresolved_near_class(&w, text_dirs).map(|why| {
            finding(format!(
                "`{name}` copies the link `{source}` to `{w}`, which the guard cannot resolve, and {why}"
            ))
        }),
        Judged::Placement(_) | Judged::Ordinary => None,
    }
}

/// A copy, link or move judged as a whole: the destination and each
/// source's landing place are judged; a moved or hard-linked startup file
/// refuses (a later write through the new name edits it); a tree copied or
/// moved into the home or a startup directory refuses where it can carry a
/// startup file; a source that is only read stays allowed
/// (`cp ~/.bashrc ./backup`).
fn copy_judgment(
    name: &str,
    call: &CopyCall,
    line: &Line<'_>,
    dirs: &[PathBuf],
) -> Option<Violation> {
    let CopyCall {
        dest,
        sources,
        recursive,
        ..
    } = call;
    let (dest, recursive) = (dest.as_str(), *recursive);
    let moves = name == "mv";
    let dest_judged = line.judge(dest, dirs);
    match &dest_judged {
        Judged::Class(label) => {
            return Some(finding(format!(
                "`{name}` writes the shell startup file `{label}`"
            )))
        }
        Judged::Unresolved(w) => {
            if let Some(why) = line.unresolved_near_class(w, dirs) {
                return Some(finding(format!(
                    "`{name}` writes to `{w}`, which the guard cannot resolve, and {why}"
                )));
            }
        }
        Judged::Placement(_) | Judged::Ordinary => {}
    }
    let landing = link_text_dirs(dest, sources.len(), line, dirs);
    let link_dirs = if call.mode == LinkMode::Symbolic {
        landing.clone()
    } else {
        dirs.to_vec()
    };
    for source in sources {
        if let Some(v) = source_link_violation(name, call, source, line, dirs, &link_dirs, &landing)
        {
            return Some(v);
        }
        let existing_dir = dirs
            .iter()
            .filter_map(|d| line.expand(dest, d))
            .any(|p| p.is_dir());
        let base = Path::new(source.trim_end_matches('/'))
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        // Into any existing directory, the file lands at its base name
        // there, which can be a link to a startup file (`out/x -> ~/.zshrc`).
        if existing_dir && !base.is_empty() {
            let landing = format!("{}/{base}", dest.trim_end_matches('/'));
            if let Judged::Class(label) = line.judge(&landing, dirs) {
                return Some(finding(format!(
                    "`{name}` places `{source}` at the shell startup file `{label}`"
                )));
            }
        }
        let Judged::Placement(dest_dir) = &dest_judged else {
            continue;
        };
        // A tree the command may carry: a recursive copy, or a move.
        let tree = recursive || moves;
        let contents = !moves
            && (name == "ditto"
                || (recursive
                    && (source.ends_with('/') || source.ends_with("/.") || source == ".")));
        if contents {
            return Some(finding(format!(
                "`{name}` copies the contents of `{source}` into `{dest_dir}`, where shell startup files live"
            )));
        }
        if tree && !existing_dir {
            return Some(finding(format!(
                "`{name}` puts the tree `{source}` at `{dest_dir}`, above shell startup files"
            )));
        }
        if base.is_empty() || !existing_dir {
            continue;
        }
        let landing = format!("{}/{base}", dest.trim_end_matches('/'));
        match line.judge(&landing, dirs) {
            Judged::Class(label) => {
                return Some(finding(format!(
                    "`{name}` places `{source}` at the shell startup file `{label}`"
                )))
            }
            Judged::Placement(p) if tree => {
                return Some(finding(format!(
                    "`{name}` puts the tree `{source}` at `{p}`, above shell startup files"
                )))
            }
            _ => {}
        }
    }
    None
}

/// A program that can write files it does not name, given or run in the
/// home, `/etc` or a startup directory.
fn placing_violation(
    name: &str,
    args: &[String],
    line: &Line<'_>,
    dirs: &[PathBuf],
) -> Option<Violation> {
    let placing = PLACING.contains(&name)
        || (name == "find" && !reads_only(name, args))
        || matches!(name, "xargs" | "parallel");
    if !placing || COPIERS.contains(&name) && name != "rsync" {
        return None;
    }
    if name == "curl"
        && !args.iter().any(|a| {
            matches!(
                a.as_str(),
                "-O" | "--remote-name" | "-J" | "--remote-name-all"
            ) || (a.starts_with('-') && !a.starts_with("--") && a.contains('O'))
        })
    {
        // Without a remote name, curl writes only the files it names.
        return None;
    }
    // Where the program puts the files it does not name: its working
    // directory, unless an option moves it there or the call places none.
    let shifts = placing_dirs(name, args)?;
    let mut unknown = line.unknown.is_some();
    let mut dirs = dirs.to_vec();
    for dir in shifts {
        dirs = dirs
            .iter()
            .filter_map(|d| {
                let moved = line.expand(dir, d);
                unknown |= moved.is_none();
                moved
            })
            .collect();
    }
    let dirs = dirs.as_slice();
    for word in args {
        let value = value_of(word);
        if !path_like(value) {
            continue;
        }
        if let Judged::Placement(dir) = line.judge(value, dirs) {
            return Some(finding(format!(
                "`{name}` writes into `{dir}`, where it can place a shell startup file it does not name"
            )));
        }
    }
    for dir in dirs {
        if let Some(p) = line.class.placement(dir) {
            return Some(finding(format!(
                "`{name}` runs in `{p}`, where it can place a shell startup file it does not name"
            )));
        }
    }
    if unknown {
        let spelled = ["~", "$HOME", "${HOME}", "/etc"]
            .iter()
            .any(|marker| line.text.contains(marker))
            || line
                .env
                .home
                .as_deref()
                .is_some_and(|h| line.text.contains(&shown(h)));
        if spelled {
            return Some(finding(format!(
                "`{name}` runs where the guard cannot tell the directory, on a line that names the home or `/etc`"
            )));
        }
    }
    None
}

/// The directories, relative to the working directory and applied in
/// order, where a placing call puts files it does not name, or `None`
/// when the call places none there: a git subcommand that keeps the
/// working tree, a tar that does not extract, a download to a named file,
/// a copy into its named destination. An empty list means the working
/// directory itself. A path the call names is judged word by word either
/// way.
fn placing_dirs<'a>(name: &str, args: &'a [String]) -> Option<Vec<&'a str>> {
    let has = |names: &[&str]| {
        args.iter().any(|a| {
            names
                .iter()
                .any(|n| a == n || a.starts_with(&format!("{n}=")))
        })
    };
    match name {
        "git" => {
            if git_subcommand(args).is_some_and(|(sub, _)| GIT_KEEPS_WORKTREE.contains(&sub)) {
                return None;
            }
            Some(git_dirs(args))
        }
        "tar" | "bsdtar" | "gtar" => {
            tar_extracts(args).then(|| option_values(args, &["-C", "--directory"]))
        }
        "unzip" => Some(option_values(args, &["-d"])),
        "wget" if has(&["-O", "--output-document"]) => None,
        "wget" => Some(option_values(args, &["-P", "--directory-prefix"])),
        "curl" => Some(option_values(args, &["--output-dir"])),
        "rsync" | "scp" | "ditto" => None,
        _ => Some(Vec::new()),
    }
}

/// Whether a tar call extracts, or its mode cannot be told (fail closed).
fn tar_extracts(args: &[String]) -> bool {
    let mut other_mode = false;
    for (at, arg) in args.iter().enumerate() {
        if let Some(long) = arg.strip_prefix("--") {
            match long.split('=').next().unwrap_or(long) {
                "extract" | "get" => return true,
                "create" | "list" | "append" | "update" | "diff" | "compare" | "delete" => {
                    other_mode = true;
                }
                _ => {}
            }
        } else if let Some(cluster) = arg.strip_prefix('-').or((at == 0).then_some(arg.as_str())) {
            // A short cluster (`-xzf`), or the old style first word (`xzf`).
            if cluster.contains('x') {
                return true;
            }
            if cluster.contains(['c', 't', 'r', 'u', 'd']) {
                other_mode = true;
            }
        }
    }
    !other_mode
}

/// The values of the named options, as `-C DIR`, `-CDIR`, `--dir=DIR` or
/// `--dir DIR`.
fn option_values<'a>(args: &'a [String], names: &[&str]) -> Vec<&'a str> {
    let mut out = Vec::new();
    for (at, arg) in args.iter().enumerate() {
        for name in names {
            if arg == name {
                if let Some(value) = args.get(at + 1) {
                    out.push(value.as_str());
                }
            } else if let Some(value) = arg.strip_prefix(&format!("{name}=")) {
                out.push(value);
            } else if !name.starts_with("--") {
                if let Some(value) = arg.strip_prefix(name).filter(|v| !v.is_empty()) {
                    out.push(value);
                }
            }
        }
    }
    out
}

/// Whether a glob's tree can be read within the budget: the entries below
/// its literal directory, to the depth its wild parts reach (any depth for
/// `**`), number at most [`GLOB_LIMIT`], and no directory link sits where
/// the walk would follow it.
fn glob_within_budget(pattern: &str) -> bool {
    let parts: Vec<&str> = pattern.split('/').collect();
    let Some(first_wild) = parts.iter().position(|part| has_glob(part)) else {
        return true;
    };
    let prefix = parts[..first_wild].join("/");
    let root = if prefix.is_empty() {
        if pattern.starts_with('/') {
            PathBuf::from("/")
        } else {
            PathBuf::from(".")
        }
    } else {
        PathBuf::from(prefix)
    };
    let depth = if pattern.contains("**") {
        usize::MAX
    } else {
        parts.len() - first_wild - 1
    };
    let mut seen = 0usize;
    let mut stack = vec![(root, 0usize)];
    while let Some((dir, level)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            seen += 1;
            if seen > GLOB_LIMIT {
                return false;
            }
            if level >= depth {
                continue;
            }
            let Ok(kind) = entry.file_type() else {
                return false;
            };
            if kind.is_symlink() && entry.path().is_dir() {
                return false;
            }
            if kind.is_dir() {
                stack.push((entry.path(), level + 1));
            }
        }
    }
    true
}

/// The class entry a native edit targets, for edit-guard.
#[must_use]
pub fn class_target(path: &Path, env: &StartupEnv) -> Option<String> {
    Class::new(env).target(path)
}

#[cfg(test)]
#[path = "startup_tests.rs"]
mod tests;
