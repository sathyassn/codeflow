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
    /// The locations from this process's environment. A relative or empty
    /// value is ignored, as the shells ignore it.
    #[must_use]
    pub fn from_process() -> Self {
        let var = |name: &str| {
            std::env::var_os(name)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
        };
        Self {
            home: var("HOME").or_else(|| var("USERPROFILE")),
            zdotdir: var("ZDOTDIR"),
            xdg_config: var("XDG_CONFIG_HOME"),
        }
    }

    /// The same locations with another home.
    #[must_use]
    pub fn with_home(home: &Path) -> Self {
        Self {
            home: Some(home.to_path_buf()),
            ..Self::from_process()
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
            if let (true, Some(zdot)) = (zsh, &env.zdotdir) {
                add(format!("$ZDOTDIR/{rel}"), zdot.join(rel), dir);
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

    /// Judge `word` as a write target from every directory the command can
    /// run in, each brace reading of it, and each path a glob in it reaches.
    fn judge(&self, word: &str, dirs: &[PathBuf]) -> Judged {
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
        // The walk is bounded by the directories it lists, which the match
        // limit below does not count: a recursive `**` or more than two
        // wild components is not walked and stays unresolved (review F-1).
        let wild = pattern.split('/').filter(|part| has_glob(part)).count();
        if pattern.contains("**") || wild > 2 {
            return Judged::Unresolved(pattern.to_string());
        }
        let Ok(paths) = glob::glob_with(pattern, options) else {
            return Judged::Unresolved(pattern.to_string());
        };
        let mut placement = None;
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
        placement.map_or(Judged::Ordinary, Judged::Placement)
    }

    /// Whether an unresolved word may name a startup file: the line names
    /// one, or the literal directory before the part the shell fills in is
    /// the home, `/etc` or a startup directory.
    fn unresolved_near_class(&self, word: &str, dirs: &[PathBuf]) -> Option<String> {
        if let Some(name) = self.class.named_in(self.text) {
            return Some(format!("the line names `{name}`"));
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

/// Whether a zsh launch ends with its startup files off: `-f`,
/// `--no-rcs`, `--norcs` or `-o norcs` turn them off, and `+f`, `--rcs`,
/// `+o norcs` or `-o rcs` turn them back on; the last one wins. Anything
/// the guard does not read leaves them on.
fn zsh_skips_rcs(args: &[String]) -> bool {
    let mut off = false;
    let mut at = 0;
    while let Some(arg) = args.get(at) {
        let a = arg.as_str();
        match a {
            "--no-rcs" | "--norcs" => off = true,
            "--rcs" => off = false,
            "-o" | "+o" => {
                let option = args.get(at + 1).map(|o| o.to_lowercase().replace('_', ""));
                match option.as_deref() {
                    Some("norcs") => off = a == "-o",
                    Some("rcs") => off = a == "+o",
                    _ => {}
                }
                at += 1;
            }
            "--" | "-" => break,
            _ if a.starts_with("--") => {}
            _ if a.starts_with('-') || a.starts_with('+') => {
                if a[1..].contains('f') {
                    off = a.starts_with('-');
                }
                if a[1..].contains(['c', 's']) {
                    // `-c CODE` or `-s`: the rest are the command's.
                    break;
                }
            }
            _ => break,
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

/// Whether a command only reads the paths it names.
fn reads_only(name: &str, args: &[String]) -> bool {
    match name {
        "printf" => !args.iter().any(|a| a.starts_with("-v")),
        "file" => !args.iter().any(|a| a == "-C" || a == "--compile"),
        "source" | "." => true,
        // `sed -n 1,20p ~/.zshrc` reads; an in-place edit, a script file,
        // or anything that may hold a `w` command does not.
        "sed" => !args.iter().any(|a| {
            (a.starts_with('-') && !a.starts_with("--") && a.contains(['i', 'I', 'f']))
                || a.starts_with("--in-place")
                || a.starts_with("--file")
                || a.contains(['w', 'W'])
        }),
        // ripgrep writes nothing; `--pre` runs a program on each file.
        "rg" => !args.iter().any(|a| a == "--pre" || a.starts_with("--pre=")),
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

/// The options of the copying programs that take a value, so the value is
/// not read as a source or destination.
const COPY_VALUE_OPTIONS: &[&str] = &[
    "-t",
    "--target-directory",
    "-S",
    "--suffix",
    "-m",
    "--mode",
    "-o",
    "--owner",
    "-g",
    "--group",
    "-e",
    "--rsh",
    "--exclude",
    "--include",
    "--filter",
    "-f",
    "--backup-dir",
    "--link-dest",
    "--compare-dest",
    "--copy-dest",
    "--chmod",
    "--chown",
    "-B",
];

/// A copy, link or move with a source and a destination.
struct CopyCall {
    dest: String,
    sources: Vec<String>,
    recursive: bool,
    hard_link: bool,
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
    let mut hard_link = name == "ln";
    let mut iter = args.iter();
    let mut options = true;
    while let Some(arg) = iter.next() {
        let a = arg.as_str();
        if options && a == "--" {
            options = false;
            continue;
        }
        if options && a.starts_with('-') && a.len() > 1 {
            if let Some(dir) = a.strip_prefix("--target-directory=") {
                target = Some(dir.to_string());
            } else if a == "-t" || a == "--target-directory" {
                target = iter.next().cloned();
            } else if COPY_VALUE_OPTIONS.contains(&a) {
                iter.next();
            }
            let short = !a.starts_with("--");
            recursive |= matches!(a, "--recursive" | "--archive" | "--mirror")
                || (short && a[1..].contains(['r', 'R', 'a']));
            if name == "ln" && (a == "--symbolic" || (short && a[1..].contains('s'))) {
                hard_link = false;
            }
            if name == "cp" && (a == "--link" || (short && a[1..].contains('l'))) {
                hard_link = true;
            }
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
        hard_link,
    })
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
        hard_link,
    } = call;
    let (dest, recursive, hard_link) = (dest.as_str(), *recursive, *hard_link);
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
    for source in sources {
        if moves || hard_link {
            match line.judge(source, dirs) {
                Judged::Class(label) => {
                    return Some(finding(format!(
                        "`{name}` moves or hard-links the shell startup file `{label}`, so a later write through the new name edits it"
                    )))
                }
                Judged::Unresolved(w) => {
                    if let Some(why) = line.unresolved_near_class(&w, dirs) {
                        return Some(finding(format!(
                            "`{name}` moves or links `{w}`, which the guard cannot resolve, and {why}"
                        )));
                    }
                }
                Judged::Placement(_) | Judged::Ordinary => {}
            }
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

/// The class entry a native edit targets, for edit-guard.
#[must_use]
pub fn class_target(path: &Path, env: &StartupEnv) -> Option<String> {
    Class::new(env).target(path)
}

#[cfg(test)]
#[path = "startup_tests.rs"]
mod tests;
