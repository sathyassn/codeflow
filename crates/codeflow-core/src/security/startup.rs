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
//! Copier trees, link text at its landing place, preservation and dereference
//! semantics, and long-option prefixes are also outside this backstop.
//! A sandbox that denies the writes is the containment for those.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use super::actions;
use super::git::{self, ConfigKind, GLOBAL_VALUE_OPTIONS};
use crate::hooks::git_guard::{
    basename, command_argv, expand_commands_read, has_glob, is_shell, launcher_effects,
    redirect_writes, run_dirs_from_home, shell_tokens, startup_glob_paths, strip_launchers,
    strip_reserved_words, unresolved_word, word_readings,
};
use crate::hooks::Violation;

/// The rule id of every finding here.
pub const RULE: &str = "security.shell_startup";

/// The sanctioned path printed with every refusal.
pub const SANCTIONED: &str = "shell startup files belong to the operator: ask the operator to make this change by hand, outside the agent session; no policy key relaxes this rule, and the harness sandbox denies these writes where one runs (issue 86)";

/// The most symbolic links one path is followed through.
const LINK_LIMIT: usize = 40;

/// The zsh startup files, which `ZDOTDIR` moves.
const ZSH_FILES: &[&str] = &[".zshenv", ".zprofile", ".zshrc", ".zlogin", ".zlogout"];

/// Where the class lives for this guard: the home and the directories the
/// environment moves startup files to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StartupEnv {
    /// The home directory (`HOME`, else `USERPROFILE`), the one `~` and
    /// `$HOME` name.
    pub home: Option<PathBuf>,
    /// Other homes a shell of this user may read: on Windows, the profile
    /// when a Git Bash `HOME` names another directory. Their startup files
    /// are the class too.
    pub other_homes: Vec<PathBuf>,
    /// `ZDOTDIR`, when set: zsh reads its startup files there.
    pub zdotdir: Option<PathBuf>,
    /// `XDG_CONFIG_HOME`, when set: fish, `PowerShell`, tmux and direnv read
    /// their configuration there.
    pub xdg_config: Option<PathBuf>,
    /// On Windows, the install roots of Unix-like shells (Git for Windows,
    /// MSYS2, Cygwin) whose `etc` folder the shell reads as `/etc`, from
    /// `shell_roots`; empty elsewhere.
    pub etc_roots: Vec<PathBuf>,
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
        Self::from_vars(&|name| std::env::var_os(name), base)
    }

    /// The same, from the variables `var` reads, so a test can give them
    /// without changing this process's environment. The homes are
    /// [`crate::portable_path::homes_from`]: `HOME`, else `USERPROFILE`, is
    /// the home `~` names, and a `USERPROFILE` that names another directory
    /// is kept among the other homes, whose startup files are the class too.
    #[must_use]
    pub fn from_vars(var: &dyn Fn(&str) -> Option<OsString>, base: &Path) -> Self {
        let set = |name: &str| var(name).filter(|v| !v.is_empty()).map(PathBuf::from);
        let mut homes =
            crate::portable_path::homes_from(var("HOME"), var("USERPROFILE"), cfg!(windows))
                .into_iter();
        Self {
            home: homes.next(),
            other_homes: homes.collect(),
            zdotdir: set("ZDOTDIR"),
            xdg_config: set("XDG_CONFIG_HOME").map(
                |x| {
                    if x.is_absolute() {
                        x
                    } else {
                        base.join(&x)
                    }
                },
            ),
            etc_roots: if cfg!(windows) {
                shell_roots(var)
            } else {
                Vec::new()
            },
        }
    }

    /// The same locations with another home, a relative `XDG_CONFIG_HOME`
    /// read from `base`.
    #[must_use]
    pub fn with_home_at(home: &Path, base: &Path) -> Self {
        let process = Self::from_process_at(base);
        let other_homes = process
            .home
            .iter()
            .chain(&process.other_homes)
            .filter(|other| key(&lexical(other)) != key(&lexical(home)))
            .cloned()
            .collect();
        Self {
            home: Some(home.to_path_buf()),
            other_homes,
            ..process
        }
    }
}

/// Folder names of the standard Unix-like shell installs on Windows: Git
/// for Windows (`C:\Program Files\Git`, a portable `PortableGit`), MSYS2
/// (`C:\msys64`) and Cygwin (`C:\cygwin64`). An `etc` folder directly in
/// one is read as the shell's `/etc` even when the guard cannot see the
/// shell there, so a root it cannot resolve fails closed.
const SHELL_ROOT_NAMES: &[&str] = &[
    "git",
    "portablegit",
    "msys64",
    "msys32",
    "cygwin",
    "cygwin64",
];

/// The install roots of Unix-like shells on a Windows machine, from the
/// variables `var` reads: Git Bash's own root (`EXEPATH`), each `PATH`
/// folder holding `git.exe` or `bash.exe` and up to two folders above it
/// (`cmd`, `bin`, `mingw64\bin`, `usr\bin`), and the standard Git for
/// Windows locations (`%ProgramFiles%\Git`, `%ProgramW6432%\Git`,
/// `%ProgramFiles(x86)%\Git`, `%LOCALAPPDATA%\Programs\Git`). A
/// candidate counts only where [`shell_root`] finds the shell. A root none
/// of these names is still refused by [`Class::shell_etc_entry`].
#[must_use]
pub(crate) fn shell_roots(var: &dyn Fn(&str) -> Option<OsString>) -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = var("EXEPATH").map(PathBuf::from).into_iter().collect();
    if let Some(path) = var("PATH") {
        candidates.extend(std::env::split_paths(&path).filter(|dir| {
            ["git.exe", "bash.exe"]
                .iter()
                .any(|n| dir.join(n).is_file())
        }));
    }
    for (name, below) in [
        ("ProgramFiles", "Git"),
        ("ProgramW6432", "Git"),
        ("ProgramFiles(x86)", "Git"),
        ("LOCALAPPDATA", "Programs/Git"),
    ] {
        if let Some(base) = var(name).filter(|v| !v.is_empty()) {
            candidates.push(PathBuf::from(base).join(below));
        }
    }
    let mut roots: Vec<PathBuf> = Vec::new();
    for candidate in candidates {
        if let Some(root) = candidate.ancestors().take(3).find(|dir| shell_root(dir)) {
            if !roots.iter().any(|r| r == root) {
                roots.push(root.to_path_buf());
            }
        }
    }
    roots
}

/// Whether `dir` is the root of a Unix-like shell install: an `etc` folder
/// beside a `bin\bash.exe` or `usr\bin\bash.exe`, as Git for Windows,
/// MSYS2 and Cygwin lay it out.
fn shell_root(dir: &Path) -> bool {
    dir.join("etc").is_dir()
        && ["bin/bash.exe", "usr/bin/bash.exe"]
            .iter()
            .any(|bash| dir.join(bash).is_file())
}

/// Whether `dir` is a shell install's `etc` folder: named `etc`, in a
/// [`shell_root`] or a folder named as a standard install.
fn shell_etc(dir: &Path) -> bool {
    let named = |p: &Path, names: &[&str]| {
        p.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| names.iter().any(|name| n.eq_ignore_ascii_case(name)))
    };
    named(dir, &["etc"])
        && dir
            .parent()
            .is_some_and(|root| named(root, SHELL_ROOT_NAMES) || shell_root(root))
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
    /// Every home, keyed.
    homes: Vec<String>,
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
            for home in env.home.iter().chain(&env.other_homes) {
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
            // A Windows shell install reads its own `etc` as `/etc`.
            for root in &env.etc_roots {
                add(
                    path.to_string(),
                    root.join(path.trim_start_matches('/')),
                    dir,
                );
            }
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
            homes: env
                .home
                .iter()
                .chain(&env.other_homes)
                .map(|h| key(&lexical(h)))
                .collect(),
            needles,
        }
    }

    /// The class entry `path` is or lies in, read lexically and through
    /// symbolic links. A path whose links cannot be followed is judged by
    /// its lexical reading and by the link text it holds.
    #[must_use]
    pub(crate) fn target(&self, path: &Path) -> Option<String> {
        let mut readings = vec![key(&lexical(path))];
        let real = resolve(path);
        if let Some(real) = &real {
            readings.push(key(real));
        }
        readings
            .iter()
            .find_map(|reading| self.entry_of(reading))
            .or_else(|| {
                if !cfg!(windows) {
                    return None;
                }
                self.shell_etc_entry(&lexical(path))
                    .or_else(|| real.as_deref().and_then(|r| self.shell_etc_entry(r)))
            })
    }

    /// The system class entry `path` names inside a Windows shell install's
    /// `etc` folder ([`shell_etc`]), read as the same path below `/etc`:
    /// `C:\Program Files\Git\etc\profile` is `/etc/profile`. It covers an
    /// install [`shell_roots`] did not resolve.
    fn shell_etc_entry(&self, path: &Path) -> Option<String> {
        path.ancestors()
            .skip(1)
            .find(|dir| shell_etc(dir))
            .and_then(|etc| {
                let rest = path.strip_prefix(etc).ok()?;
                let reading = format!("/etc/{}", key(rest));
                self.entry_of(reading.trim_end_matches('/'))
                    .filter(|label| label.starts_with("/etc"))
            })
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
        if cfg!(windows) && shell_etc(&lexical(path)) {
            return Some(shown(path));
        }
        readings.iter().find_map(|reading| {
            let reading = reading.trim_end_matches('/');
            if reading.is_empty() || self.homes.iter().any(|h| h == reading) || reading == "/etc" {
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

    /// Whether `name` is the last component of a class entry: a class file
    /// name anywhere, or a class directory such as `fish`.
    fn knows_name(&self, name: &str) -> bool {
        let name = name.to_lowercase();
        self.entries.iter().any(|entry| match entry {
            Entry::At { readings, .. } => readings
                .iter()
                .any(|p| p.trim_end_matches('/').rsplit('/').next() == Some(name.as_str())),
            Entry::Name(n) => n.to_lowercase() == name,
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

/// Whether `text` holds a tilde word that names a home: `~`, `~/x` or `~user`.
/// A directory-stack word (`~1`, `~+1`, `~-`) does not.
fn tilde_names_a_home(text: &str) -> bool {
    text.match_indices('~').any(|(at, _)| {
        let word_start = text[..at].chars().next_back().is_none_or(|c| {
            c.is_whitespace()
                || matches!(
                    c,
                    '\'' | '"' | '=' | ':' | ';' | '(' | '|' | '&' | '<' | '>'
                )
        });
        let prefix: String = text[at + 1..]
            .chars()
            .take_while(|c| {
                !(c.is_whitespace()
                    || matches!(c, '/' | '\'' | '"' | ';' | ')' | '|' | '&' | '<' | '>'))
            })
            .collect();
        word_start && !stack_token(&prefix)
    })
}

/// A directory-stack tilde prefix: `+`, `-`, `N`, `+N` or `-N`.
fn stack_token(prefix: &str) -> bool {
    let digits = prefix.strip_prefix(['+', '-']).unwrap_or(prefix);
    matches!(prefix, "+" | "-")
        || (!digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()))
}

/// Whether `needle` occurs in `text` as a path or a name: not preceded by
/// a name character, and not followed by one.
pub(crate) fn mentions(text: &str, needle: &str) -> bool {
    let name_char = |c: char| c.is_alphanumeric() || matches!(c, '_' | '-');
    text.match_indices(needle).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + needle.len()..].chars().next();
        before.is_none_or(|c| !(name_char(c) || c == '.')) && after.is_none_or(|c| !name_char(c))
    })
}

/// A path as the class compares it: slashes, lower case, and on Windows
/// without the verbatim prefix or a drive.
pub(crate) fn key(path: &Path) -> String {
    let path = crate::portable_path::without_verbatim(path.to_path_buf());
    let text = crate::portable_path::slashed(&path).to_lowercase();
    if cfg!(windows) {
        drive_free(&text).to_string()
    } else {
        text
    }
}

/// `text`, a slashed lower-case Windows path, without its drive: `c:/x`,
/// the Git Bash spelling `/c/x` and that spelling joined to a working
/// directory's drive (`c:/c/x`) all read as `/x`. The class's system
/// entries are spelled from the root (`/etc/zshenv`, which Git Bash reads
/// below its own install), and a drive joined from the working directory
/// must not hide them. A path on another drive, or below a one-letter
/// folder at a drive's root, can then match a class entry too, which only
/// refuses more.
fn drive_free(text: &str) -> &str {
    let text = match text.as_bytes() {
        [letter, b':', ..] if letter.is_ascii_alphabetic() => &text[2..],
        _ => text,
    };
    match text.as_bytes() {
        [b'/', letter] if letter.is_ascii_alphabetic() => "/",
        [b'/', letter, b'/', ..] if letter.is_ascii_alphabetic() => &text[2..],
        _ => text,
    }
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

/// A glob a word spells: a literal directory the shell does not expand,
/// and the pattern below it.
struct Pattern {
    base: PathBuf,
    rest: String,
}

impl Pattern {
    fn shown(&self) -> String {
        format!("{}/{}", shown(&self.base).trim_end_matches('/'), self.rest)
    }
}

/// `word` with each of `vars` replaced by its value, repeated until
/// nothing changes (at most six passes).
pub(crate) fn substitute_with(word: &str, vars: &[(String, String)]) -> String {
    let mut text = word.to_string();
    for _ in 0..6 {
        let before = text.clone();
        for (name, value) in vars {
            for form in [format!("${{{name}}}"), format!("${name}")] {
                let mut from = 0;
                while let Some(found) = text[from..].find(&form) {
                    let at = from + found;
                    let end = at + form.len();
                    let continues = form.starts_with("${")
                        || !text[end..]
                            .chars()
                            .next()
                            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
                    if !continues {
                        from = end;
                        continue;
                    }
                    text.replace_range(at..end, value);
                    from = at + value.len();
                }
            }
        }
        if text == before {
            break;
        }
    }
    text
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
    /// The line names a class file and produces text: a command on it may
    /// run what the call wrote ([`staged_run`]).
    staged: bool,
}

impl Line<'_> {
    /// The startup file `text` names, read as written and with the line's
    /// literal assignments filled in: a body that spells `$p` after
    /// `p=src/.envrc` names the file the shell hands it (review of
    /// e4536456b).
    fn names(&self, text: &str) -> Option<String> {
        self.class
            .named_in(text)
            .or_else(|| self.class.named_in(&self.substitute(text)))
    }

    /// Whether the shell fills in part of `segment` at run time from a
    /// value the guard cannot read: an expansion outside single quotes
    /// (`$NAME`, `${NAME}`, `$(...)`, a backquote) that is not a literal
    /// the line assigns or a location of the class. Single-quoted text is
    /// data to the shell, so an `awk` or `perl` variable there does not
    /// count.
    fn expands_unread(&self, segment: &str) -> bool {
        let known = self.vars();
        let chars: Vec<char> = segment.chars().collect();
        let (mut single, mut double, mut at) = (false, false, 0);
        while at < chars.len() {
            let c = chars[at];
            at += 1;
            match c {
                '\\' if !single => at += 1,
                '\'' if !double => single = !single,
                '"' if !single => double = !double,
                // A substitution the segmenter lifted out stands as a marker.
                '`' | '\u{1}' | '\u{2}' if !single => return true,
                '$' if !single => {
                    let rest = &chars[at..];
                    match rest.first() {
                        Some('(') => return true,
                        Some('{') => {
                            let name: String = rest[1..]
                                .iter()
                                .take_while(|c| c.is_ascii_alphanumeric() || **c == '_')
                                .collect();
                            if !name.is_empty() && !known.iter().any(|(n, _)| *n == name) {
                                return true;
                            }
                        }
                        Some(first) if first.is_ascii_alphabetic() || *first == '_' => {
                            let name: String = rest
                                .iter()
                                .take_while(|c| c.is_ascii_alphanumeric() || **c == '_')
                                .collect();
                            if !known.iter().any(|(n, _)| *n == name) {
                                return true;
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        false
    }

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

    /// The glob `word` spells from `dir`: `None` unless a part the line
    /// spells holds pattern syntax. The run directory, the home a `~` names
    /// and the values of `$HOME`, `$ZDOTDIR` and `$XDG_CONFIG_HOME` are
    /// read as the paths they are, whatever characters they hold, so a
    /// project in `proj (1)` or a Windows `\\?\` path is never a pattern
    /// (the Windows run of PR 113).
    fn pattern(&self, word: &str, dir: &Path) -> Option<Pattern> {
        let assigned: Vec<(String, String)> = self
            .vars()
            .into_iter()
            .filter(|(name, _)| self.assigned.contains_key(name))
            .collect();
        let mut spelled = substitute_with(word, &assigned);
        if cfg!(windows) {
            // A backslash the shell kept (in double quotes) separates there.
            spelled = spelled.replace('\\', "/");
        }
        let (anchor, rest) = match spelled.as_str() {
            "~" | "~+" => (spelled.as_str(), ""),
            text => text
                .strip_prefix("~/")
                .map(|rest| ("~", rest))
                .or_else(|| text.strip_prefix("~+/").map(|rest| ("~+", rest)))
                .unwrap_or(("", text)),
        };
        let parts: Vec<&str> = rest.split('/').collect();
        let at = parts.iter().position(|part| has_glob(part))?;
        let base = if anchor.is_empty() && at == 0 {
            dir.to_path_buf()
        } else {
            let literal = parts[..at].join("/");
            let base_word = match (anchor, literal.as_str()) {
                ("", "") => "/".to_string(),
                ("", literal) => literal.to_string(),
                (anchor, "") => anchor.to_string(),
                (anchor, literal) => format!("{anchor}/{literal}"),
            };
            self.expand(&base_word, dir)?
        };
        Some(Pattern {
            base,
            rest: self.substitute(&parts[at..].join("/")),
        })
    }

    /// `word` with each variable the guard can read replaced by its value.
    /// An assigned value can itself use `$HOME` or another assigned name
    /// (`A=$HOME; F=$A/.zshrc`), so the pass repeats until nothing changes.
    fn substitute(&self, word: &str) -> String {
        substitute_with(word, &self.vars())
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
                if let Some(pattern) = self.pattern(reading, dir) {
                    match self.glob(&pattern) {
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
    fn glob(&self, pattern: &Pattern) -> Judged {
        let folded = self.text.to_lowercase().replace('_', "");
        let dots = ["dotglob", "globignore", "globdots"]
            .iter()
            .any(|option| folded.contains(option));
        let options = glob::MatchOptions {
            case_sensitive: false,
            require_literal_separator: true,
            require_literal_leading_dot: !dots,
        };
        // The literal directory is escaped and keyed as the class paths
        // are, so only the spelled part matches as a pattern.
        let text = format!(
            "{}/{}",
            glob::Pattern::escape(key(&lexical(&pattern.base)).trim_end_matches('/')),
            pattern.rest.to_lowercase()
        );
        let Ok(compiled) = glob::Pattern::new(&text) else {
            return Judged::Unresolved(pattern.shown());
        };
        // A glob in a class directory writes there whatever it matches, and
        // the shell keeps a word that matches nothing as its own name
        // (`conf.d/*.fish`, which fish then reads).
        if let Some(label) = self.class.target(&pattern.base) {
            return Judged::Class(label);
        }
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
        let Some(paths) = startup_glob_paths(&pattern.base, Path::new(&pattern.rest), dots) else {
            return Judged::Unresolved(pattern.shown());
        };
        let mut placement = None;
        for path in paths {
            if let Some(label) = self.class.target(&path) {
                return Judged::Class(label);
            }
            if placement.is_none() {
                placement = self.class.placement(&path);
            }
        }
        placement.map_or(Judged::Ordinary, Judged::Placement)
    }

    /// Whether every alternative of a glob's last segment is a literal that
    /// is neither a class file name nor the name of a class directory, so
    /// the glob can only reach a startup file through its directory part
    /// (`**/node_modules`). `**/.*`, a trailing `**` and `**/fish` are not.
    fn glob_ends_plain(&self, pattern: &str) -> bool {
        let last = pattern
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or_default();
        let alternatives: Vec<&str> = match last.strip_prefix('{').and_then(|r| r.strip_suffix('}'))
        {
            Some(inner) => inner.split(',').collect(),
            None => vec![last],
        };
        alternatives.iter().all(|alt| {
            !alt.is_empty()
                && !alt.contains(['*', '?', '[', '{', '}', '$', '`', '\u{1}', '\u{2}'])
                && self.class.named_in(alt).is_none()
                && !self.class.knows_name(alt)
        })
    }

    /// Whether an unresolved word may name a startup file: the line names
    /// one, or the literal directory before the part the shell fills in is
    /// the home, `/etc` or a startup directory.
    fn unresolved_near_class(&self, word: &str, dirs: &[PathBuf]) -> Option<String> {
        if let Some(name) = self.class.named_in(self.text) {
            return Some(format!("the line names `{name}`"));
        }
        if dirs
            .iter()
            .filter_map(|dir| self.expand(word, dir).and_then(|_| self.pattern(word, dir)))
            .any(|pattern| {
                startup_glob_paths(&pattern.base, Path::new(&pattern.rest), false).is_none()
                    && !self.glob_ends_plain(&pattern.rest)
            })
        {
            return Some("its glob exceeds the shell reader's bounded expansion".to_string());
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
        // Only `~user/rest` names a home the guard cannot read. The
        // directory stack (`~+`, `~-`, `~N`, `~+N`, `~-N`) is not a home, and
        // a tilde word without a slash is the home itself or a plain name.
        if let Some((user, _)) = word.strip_prefix('~').and_then(|r| r.split_once('/')) {
            if !user.is_empty() && !stack_token(user) {
                return Some(format!("`{word}` names a home the guard cannot read"));
            }
        }
        let known = self.substitute(word);
        let cut = known
            .find(['$', '`', '\u{1}', '\u{2}', '*', '?', '[', '{'])
            .unwrap_or(known.len());
        if cut == known.len() && self.uses_assigned(word) {
            // Nothing is left to fill in: the assigned reading was judged
            // as a plain path, and the inherited reading above. A literal
            // like `/scratch` in `R=/scratch; git -C "$R" ...` is not a
            // path under the root that may hold a class entry.
            return None;
        }
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
    let expanded = expand_commands_read(command);
    let segments = &expanded.segments;
    // `cd ~` lands in the home the class is built from.
    let run = run_dirs_from_home(segments, cwd, env.home.clone());
    let mut line = Line {
        class: &class,
        env,
        text: command,
        assigned: BTreeMap::new(),
        dirs: run.dirs.clone(),
        unknown: run.unknown.clone(),
        staged: false,
    };
    line.assigned = literal_assignments(segments);
    line.staged = staged_name(segments, &line).is_some();
    if let Some(v) = direnv_trust(segments) {
        return vec![v];
    }
    if let Some(v) = startup_environment(segments) {
        return vec![v];
    }
    for code in super::outward::interpreter_bodies(command) {
        if let Some(name) = line.names(&code) {
            return vec![finding(format!(
                "interpreter code names the shell startup file `{name}`, which it can write"
            ))];
        }
    }
    if let Some(v) = staged_run(segments, &line) {
        return vec![v];
    }
    for segment in segments {
        if let Some(v) = command_valued_variable(segment, &line) {
            return vec![v];
        }
        if let Some(v) = segment_violation(segment, &line) {
            return vec![v];
        }
    }
    closed_rule(segments, &expanded, &line)
        .into_iter()
        .collect()
}

/// Variables whose value is a command that a reader or git runs: an editor,
/// a pager, an ssh or askpass program, an external diff, a preview filter, or
/// configuration passed in the environment.
const COMMAND_VARIABLES: &[&str] = &[
    "EDITOR",
    "VISUAL",
    "FCEDIT",
    "PAGER",
    "MANPAGER",
    "BROWSER",
    "LESSOPEN",
    "LESSCLOSE",
    "GIT_EDITOR",
    "GIT_SEQUENCE_EDITOR",
    "GIT_PAGER",
    "GIT_SSH",
    "GIT_SSH_COMMAND",
    "GIT_EXTERNAL_DIFF",
    "GIT_ASKPASS",
    "SSH_ASKPASS",
    "GIT_PROXY_COMMAND",
    "GIT_CONFIG_PARAMETERS",
];

/// A command-valued variable set on a segment (`GIT_EDITOR='cmd' git commit`,
/// `export PAGER=cmd`) whose command names a startup file. This reads every
/// segment, not only staged lines: the command runs without any produced text.
fn command_valued_variable(segment: &str, line: &Line<'_>) -> Option<Violation> {
    let words = command_argv(segment);
    // `git --config-env=core.fsmonitor=VAR` takes the setting from `VAR`.
    let mut config_env = words.iter().enumerate().filter_map(|(at, word)| {
        word.strip_prefix("--config-env=")
            .or_else(|| (word == "--config-env").then(|| words.get(at + 1).map(String::as_str))?)
            .and_then(|setting| setting.split_once('=').map(|(_, var)| var))
    });
    if let Some((var, class)) = config_env.find_map(|var| {
        let value = line.assigned.get(var)?;
        Some((var, line.names(value)?))
    }) {
        return Some(finding(format!(
            "`--config-env` takes a git setting from `{var}`, a command that names the shell startup file `{class}`"
        )));
    }
    words.iter().find_map(|word| {
        let (name, value) = word.split_once('=')?;
        if !(COMMAND_VARIABLES.contains(&name) || name.starts_with("GIT_CONFIG_VALUE_")) {
            return None;
        }
        let class = line.names(value)?;
        Some(finding(format!(
            "`{name}` is set to a command that names the shell startup file `{class}`, which a program the line runs would execute"
        )))
    })
}

/// Whether an assigned value can be read without running anything: no
/// command substitution, and every expansion is `$HOME`, `$ZDOTDIR`,
/// `$XDG_CONFIG_HOME` or a name the line assigned earlier with a readable
/// value (`A=$HOME; F=$A/.zshrc`). Any other `$` leaves the value unread.
fn readable_assignment(value: &str, earlier: &BTreeMap<String, Option<String>>) -> bool {
    if value.contains(['`', '\u{1}', '\u{2}']) {
        return false;
    }
    let chars: Vec<char> = value.chars().collect();
    let mut at = 0;
    while at < chars.len() {
        if chars[at] != '$' {
            at += 1;
            continue;
        }
        let (name, next): (String, usize) = if chars.get(at + 1) == Some(&'{') {
            let name: String = chars[at + 2..]
                .iter()
                .take_while(|c| c.is_ascii_alphanumeric() || **c == '_')
                .collect();
            let close = at + 2 + name.chars().count();
            if chars.get(close) != Some(&'}') {
                return false;
            }
            (name, close + 1)
        } else {
            let name: String = chars[at + 1..]
                .iter()
                .take_while(|c| c.is_ascii_alphanumeric() || **c == '_')
                .collect();
            let next = at + 1 + name.chars().count();
            (name, next)
        };
        let known = matches!(name.as_str(), "HOME" | "ZDOTDIR" | "XDG_CONFIG_HOME")
            || earlier.get(&name).is_some_and(Option::is_some);
        if name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit()) || !known {
            return false;
        }
        at = next;
    }
    true
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
        let value = value
            .filter(|v| readable_assignment(v, values))
            .map(str::to_string);
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

/// Whether the call produces text a later command could take as its input or
/// its script: a pipe, a heredoc, a process substitution, or a file other than
/// `/dev/null` the call writes (a redirect or `tee`).
fn produces_text(segments: &[String], text: &str) -> bool {
    if text.contains('|') || text.contains("<<") || text.contains("<(") || text.contains(">(") {
        return true;
    }
    segments.iter().any(|segment| {
        let redirects = redirect_writes(segment);
        let mut words = command_argv(segment);
        strip_reserved_words(&mut words);
        let tee = strip_launchers(&words).is_some_and(|(program, args)| {
            basename(program) == "tee"
                && args.iter().any(|a| !a.starts_with('-') && a != "/dev/null")
        });
        tee || redirects
            .targets
            .iter()
            .chain(&redirects.unread)
            .any(|target| target != "/dev/null")
    })
}

/// Shell builtins and keywords that run no command of their own: they move,
/// wait, set variables or open a control structure.
const NO_COMMAND: &[&str] = &[
    "cd", "pushd", "popd", "pwd", ":", "sleep", "shift", "export", "declare", "typeset", "local",
    "readonly", "unset", "for", "case", "select", "done", "fi", "esac", "{", "}", "(", ")",
    "command",
];

/// Filters that read standard input or named files and write only standard
/// output. A write or exec path of their own is refused in [`data_reader`].
const FILTERS: &[&str] = &[
    "sort", "uniq", "cut", "tr", "paste", "column", "fold", "nl", "tac", "rev", "comm", "join",
    "xxd", "base64", "tee",
];

/// Whether a word assigns a variable that picks the program or the code that
/// runs, whatever its value can be: the search path, preloaded libraries, the
/// shell and its startup hooks, and the places git reads programs and
/// configuration from (`GIT_EXEC_PATH`, `GIT_TEMPLATE_DIR`,
/// `GIT_CONFIG_GLOBAL`, `GIT_CONFIG_SYSTEM`). Command-valued variables
/// (`EDITOR`, `PAGER`, `GIT_*` commands) are read by value
/// ([`command_valued_variable`], [`staged_variable`]).
fn assigns_program_variable(word: &str) -> bool {
    let Some((name, _)) = word.split_once('=') else {
        return false;
    };
    name.starts_with("LD_")
        || name.starts_with("DYLD_")
        || matches!(
            name,
            "PATH"
                | "SHELL"
                | "BASH_ENV"
                | "ENV"
                | "PROMPT_COMMAND"
                | "GIT_EXEC_PATH"
                | "GIT_TEMPLATE_DIR"
                | "GIT_CONFIG_GLOBAL"
                | "GIT_CONFIG_SYSTEM"
        )
}

/// Viewers, editors and the ordinary ssh and diff tools an `EDITOR`, `PAGER`,
/// `GIT_SSH_COMMAND` or `GIT_EXTERNAL_DIFF` names. On a line that names a class
/// file and produces text, a command-valued setting may only start with one of
/// these: any other command could run a script the call wrote.
const VIEWERS: &[&str] = &[
    "vim",
    "vi",
    "nvim",
    "view",
    "vimdiff",
    "nano",
    "pico",
    "micro",
    "emacs",
    "less",
    "more",
    "most",
    "cat",
    "bat",
    "batcat",
    "delta",
    "head",
    "tail",
    "true",
    "false",
    "open",
    "code",
    "subl",
    "ssh",
    "diff",
    "colordiff",
    "meld",
    "opendiff",
    "kdiff3",
];

/// Words in an option that run something of their own.
const RUNNING_WORDS: &[&str] = &["cmd", "command", "exec", "script", "eval", "load", "source"];

/// Whether a command-valued setting is an ordinary viewer or tool: its first
/// word is in [`VIEWERS`], and the rest are plain options (`-w`, `-R`,
/// `--wait`, `-o BatchMode=yes`) that name no class file, hold no shell
/// character and run nothing of their own. `vim -S x`, `sh r.sh` and
/// `ssh -o ProxyCommand=sh` are not.
fn viewer_command(value: &str, line: &Line<'_>) -> bool {
    let mut words = value.split_whitespace();
    let Some(viewer) = words.next().filter(|w| VIEWERS.contains(w)) else {
        return false;
    };
    let plain_flag = |w: &str| {
        if let Some(own) = plain_letters(viewer) {
            if let Some(run) = w.strip_prefix('-').filter(|r| !r.starts_with('-')) {
                return !run.is_empty() && run.chars().all(|c| own.contains(c));
            }
        }
        let letters = w
            .strip_prefix("--")
            .map(|r| r.chars().all(|c| c.is_ascii_lowercase() || c == '-'))
            .or_else(|| {
                w.strip_prefix('-')
                    .map(|r| r.len() == 1 && r.chars().all(|c| c.is_ascii_alphabetic()))
            })
            .unwrap_or(false);
        letters
            && !matches!(
                w,
                "-c" | "-S" | "-s" | "-u" | "-U" | "-x" | "-X" | "-e" | "-E"
            )
    };
    let option_value = |w: &str| {
        w.split_once('=').is_some_and(|(k, v)| {
            !k.is_empty()
                && !v.is_empty()
                && k.chars().all(|c| c.is_ascii_alphanumeric())
                && v.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        })
    };
    words.all(|w| {
        let lower = w.to_lowercase();
        (plain_flag(w) || option_value(w))
            && !RUNNING_WORDS.iter().any(|r| lower.contains(r))
            && line.names(w).is_none()
    })
}

/// The letters a pager or diff takes without a value, which may run
/// together in one word (`less -RF`, `diff -u`). Each formats or moves
/// through output; none loads a file, writes one or runs a command, so
/// less's `-k`, `-o` and `+command` and diff's `-l` (which runs `pr`) are
/// not among them. Other viewers take one plain letter per word, and vim's
/// `-c`, `-S`, `-s`, `-u` and `+command` refuse.
fn plain_letters(viewer: &str) -> Option<&'static str> {
    match viewer {
        "less" => Some("aABcCdeEfFgGiIJKLmMnNqQrRsSuUVwWX"),
        "diff" | "colordiff" => Some("aBbcdEeiNnPpqrsTtuwyZ"),
        _ => None,
    }
}

/// Whether `name` is a variable whose value is a command.
fn command_valued_name(name: &str) -> bool {
    COMMAND_VARIABLES.contains(&name)
}

/// A command-valued variable on a staged line whose value is not an ordinary
/// viewer or tool, or cannot be read; and a `GIT_CONFIG_VALUE_n` whose key
/// is not known to leave commands alone ([`config_setting_problem`]).
fn staged_variable(word: &str, line: &Line<'_>) -> Option<String> {
    let (name, value) = word.split_once('=')?;
    if let Some(n) = name.strip_prefix("GIT_CONFIG_VALUE_") {
        let key = line
            .assigned
            .get(&format!("GIT_CONFIG_KEY_{n}"))
            .map_or("", String::as_str);
        return config_setting_problem(&format!("`{name}`"), key, value, line)
            .map(|_| word.to_string());
    }
    if !command_valued_name(name) {
        return None;
    }
    let value = line.substitute(value);
    (!viewer_command(&value, line)).then(|| word.to_string())
}

/// Directories a program named by an absolute path may live in and still be
/// read as the system program of that name.
const SYSTEM_DIRS: &[&str] = &[
    "/bin",
    "/usr/bin",
    "/sbin",
    "/usr/sbin",
    "/usr/local/bin",
    "/opt/homebrew/bin",
];

/// Whether a short-option word (`-rno`) has `letter` before any letter that
/// takes the rest of the word as its value.
fn cluster_has(arg: &str, letter: char, valued: &str) -> bool {
    let Some(cluster) = arg.strip_prefix('-').filter(|c| !c.starts_with('-')) else {
        return false;
    };
    for c in cluster.chars() {
        if c == letter {
            return true;
        }
        if valued.contains(c) {
            return false;
        }
    }
    false
}

/// The operands among `args`: `-` and words that are not options and not a
/// bare number (the value of `-c 16`).
fn operands(args: &[String]) -> usize {
    args.iter()
        .filter(|a| {
            (a.as_str() == "-" || !a.starts_with('-')) && !a.chars().all(|c| c.is_ascii_digit())
        })
        .count()
}

/// awk used as a filter: an inline program that starts no program, writes no
/// file and loads no library, over files and `var=value` operands. A program
/// file or library (`-f`, `-i`, `-E`, `-e`, `--file`, `--include`, `--exec`,
/// `--source`), `system`, a pipe, `>>`, a `>` after `print` and any `@`
/// (`@include`, `@load`) are all refused. Any other option fails closed.
fn awk_reads(args: &[String]) -> bool {
    let risky = |a: &str| a.contains('@');
    let mut iter = args.iter();
    let mut program = None;
    while let Some(arg) = iter.next() {
        let a = arg.as_str();
        if risky(a) {
            return false;
        }
        match a {
            "-F" | "-v" => match iter.next() {
                Some(value) if !risky(value) => {}
                _ => return false,
            },
            "-n" | "--posix" | "--traditional" | "--re-interval" => {}
            _ if (a.starts_with("-F") || a.starts_with("-v")) && a.len() > 2 => {}
            _ if a.starts_with('-') && a != "-" => return false,
            _ => {
                program = Some(a);
                break;
            }
        }
    }
    let Some(program) = program else {
        return false;
    };
    let unsafe_text = program.contains('|')
        || program.contains("system")
        || program.contains(">>")
        || program
            .find("print")
            .is_some_and(|at| program[at..].contains('>'));
    !unsafe_text && iter.all(|a| !risky(a))
}

/// sed used as a filter: no script file, no in-place suffix confusion, and a
/// script of print-only commands (no `e`, `w`, `W`, `r`, `R`). `-i` edits the
/// file operand in place; that write is judged on its own.
fn sed_filter_reads(args: &[String]) -> bool {
    // BSD sed takes `-i ''` as an empty backup suffix; an empty word is no
    // script either way.
    let kept: Vec<String> = args
        .iter()
        .enumerate()
        .filter(|(at, a)| !(a.is_empty() && *at > 0 && args[at - 1] == "-i"))
        .map(|(_, a)| a)
        .filter_map(|a| {
            if a == "--in-place" || a.starts_with("--in-place=") {
                None
            } else if a.starts_with('-') && !a.starts_with("--") {
                match a.find('i') {
                    Some(1) => None,
                    Some(at) => Some(a[..at].to_string()),
                    None => Some(a.clone()),
                }
            } else {
                Some(a.clone())
            }
        })
        .collect();
    sed_reads(&kept)
}

/// The git subcommands that run no program of their own beyond the hooks
/// installed in the repository (a hook written by the call is a hook write).
const GIT_PLAIN: &[&str] = &[
    "checkout",
    "switch",
    "merge",
    "pull",
    "reset",
    "restore",
    "stash",
    "cherry-pick",
    "revert",
    "mv",
    "clone",
];

/// git used as a data reader: a built-in subcommand other than `apply` and
/// `am`, with no global setting that names a class file or, on a staged line,
/// runs a command (`-c alias.x=!cmd`), no `--exec`, pager, external diff or
/// output option, and no path list read from standard input or a file.
fn git_reads(args: &[String], line: &Line<'_>) -> bool {
    let Some((sub, rest)) = git_subcommand(args) else {
        return false;
    };
    if git_globals(args, rest).iter().any(|a| a == "-p")
        || git_setting_violation(args, line).is_some()
    {
        return false;
    }
    if !(GIT_READS.contains(&sub) || GIT_KEEPS_WORKTREE.contains(&sub) || GIT_PLAIN.contains(&sub))
    {
        return false;
    }
    // A repository setting can name a program (`core.fsmonitor`, a filter,
    // `core.hooksPath`), so `config` only reads, and `ext::` URLs run a command.
    if sub == "config"
        && !rest.iter().any(|a| {
            matches!(
                a.as_str(),
                "--get" | "--get-all" | "--get-regexp" | "--list" | "-l"
            )
        })
    {
        return false;
    }
    if args.iter().any(|a| a.contains("ext::")) {
        return false;
    }
    // A path or object list read from standard input or a file the call
    // wrote (`checkout --pathspec-from-file=-`, `rm`, `restore`, `reset`,
    // `update-index --stdin`, `checkout-index --stdin`) lets produced text
    // pick the files git rewrites or deletes. Interactive modes read their
    // answers from standard input the same way.
    // `hash-object` without `-w` only prints the hash.
    let hash_only = sub == "hash-object"
        && !rest.iter().any(|a| {
            a == "--write" || (a.starts_with('-') && !a.starts_with("--") && a.contains('w'))
        });
    if !hash_only
        && rest.iter().any(|a| {
            [
                "--pathspec-from-file",
                "--stdin",
                "--stdin-paths",
                "--index-info",
                "--index-filter",
                "--tree-filter",
                "--batch-command",
            ]
            .iter()
            .any(|opt| a == opt || a.strip_prefix(opt).is_some_and(|r| r.starts_with('=')))
        })
    {
        return false;
    }
    if matches!(
        sub,
        "checkout" | "restore" | "reset" | "stash" | "clean" | "add" | "commit"
    ) && rest.iter().any(|a| {
        matches!(a.as_str(), "--patch" | "--interactive")
            || (a.starts_with('-') && !a.starts_with("--") && a.contains(['p', 'i']))
    }) {
        return false;
    }
    !rest.iter().any(|a| {
        a == "-o"
            || a == "-O"
            || a.starts_with("--output")
            || a.starts_with("--exec")
            || a.starts_with("--upload-pack")
            || a.starts_with("--receive-pack")
            || a.starts_with("--open-files-in-pager")
            || a.starts_with("--ext-diff")
            || a.starts_with("--pager")
    })
}

/// Whether `program` with `args` only reads the text it is given: a listed
/// data reader used as one. Everything else, an unknown program or option
/// included, is not.
fn data_reader(program: &str, args: &[String], line: &Line<'_>) -> bool {
    if basename(program) == "git" {
        return system_program(program) && git_reads(args, line);
    }
    reader_program(program, args)
}

/// Whether `program` names a program by its name or from a system
/// directory ([`SYSTEM_DIRS`]), not a relative path or another directory.
fn system_program(program: &str) -> bool {
    if program.starts_with("./") || (program.contains('/') && !program.starts_with('/')) {
        return false;
    }
    !program.starts_with('/')
        || SYSTEM_DIRS.contains(&program.rsplit_once('/').map_or("", |(dir, _)| dir))
}

/// [`data_reader`] for a program other than git, which needs the line's
/// facts: a listed data reader used as one, judged by operation. The
/// closed rule of TSK-242 ([`super::unresolved`]) uses it on every guard.
pub(crate) fn reader_program(program: &str, args: &[String]) -> bool {
    let name = basename(program);
    if !system_program(program) {
        return false;
    }
    match name {
        _ if NO_COMMAND.contains(&name) => true,
        "source" | "." | "eval" | "exec" | "git" => false,
        "sed" | "gsed" => sed_filter_reads(args),
        "awk" | "gawk" | "mawk" | "nawk" => awk_reads(args),
        "sort" => !args.iter().any(|a| {
            a.starts_with("--output")
                || a.starts_with("--compress-program")
                || cluster_has(a, 'o', "ktTS")
        }),
        "uniq" | "xxd" => operands(args) <= 1,
        "base64" => !args
            .iter()
            .any(|a| a.starts_with("--output") || cluster_has(a, 'o', "")),
        _ if FILTERS.contains(&name) => true,
        _ => reads_only(name, args),
    }
}

/// The startup file a staged line names, when the line produces text and
/// names a class file; `None` for any other line.
fn staged_name(segments: &[String], line: &Line<'_>) -> Option<String> {
    if !produces_text(segments, line.text) {
        return None;
    }
    class_named(segments, line)
}

/// The startup file a line names, read as written and in its decoded
/// words: `$'\\x2ezshrc'` and `'.zs''hrc'` name the file although the raw
/// text does not spell it.
fn class_named(segments: &[String], line: &Line<'_>) -> Option<String> {
    line.names(line.text).or_else(|| {
        segments
            .iter()
            .find_map(|segment| line.names(&command_argv(segment).join(" ")))
    })
}

/// The closed rule of TSK-242 for the startup class. A line that names a
/// startup file refuses when it also carries a form the guard cannot
/// resolve ([`super::unresolved::unresolved_form`]): a command word whose
/// expansion it cannot fill in, a shell or `eval` reading a script it does
/// not see, a command read from standard input or built from a template,
/// a launcher whose command it does not parse, a GNU-prefixed launcher, or
/// a program it does not know to run nothing of its own. Every form the
/// guard reads is judged by what it writes instead.
///
/// A line that names a startup file and also produces text (a pipe,
/// heredoc, substitution or written file) is held to more: every program
/// on it is a data reader used as one ([`data_reader`]), so the text may be
/// read but cannot be run or applied. The allowlist replaces the list of
/// programs that run text (shells, interpreters, script tools, wrappers,
/// `find -exec`, `env -S`), which an unlisted spelling always escaped.
fn closed_rule(
    segments: &[String],
    expanded: &crate::hooks::git_guard::Expanded,
    line: &Line<'_>,
) -> Option<Violation> {
    let name = class_named(segments, line)?;
    let reads_class = |word: &str| line.names(word).is_some();
    super::unresolved::unresolved_form(expanded, &reads_class).map(|form| {
        finding(format!(
            "the line names the shell startup file `{name}`, and {form}, so it could write that file out of the guard's sight; run that command in a call that does not name the file"
        ))
    })
}

/// The staged-run half of the closed rule ([`closed_rule`]): a line that
/// names a startup file and produces text.
fn staged_run(segments: &[String], line: &Line<'_>) -> Option<Violation> {
    let name = staged_name(segments, line)?;
    segments.iter().find_map(|segment| {
        let mut words = command_argv(segment);
        strip_reserved_words(&mut words);
        if let Some(word) = words
            .iter()
            .find(|w| assigns_program_variable(w))
            .cloned()
            .or_else(|| words.iter().find_map(|w| staged_variable(w, line)))
        {
            return Some(finding(format!(
                "text this call produces names the shell startup file `{name}`, and the line sets `{word}`, which changes the program that runs; read the file in its own call"
            )));
        }
        // `env -S 'cmd'` leaves no program behind: the reader judges the
        // string as its own segments, each held to this rule in turn.
        let (program, args) = strip_launchers(&words)?;
        (!data_reader(program, args, line)).then(|| {
            finding(format!(
                "text this call produces names the shell startup file `{name}`, and the line runs `{program}`, which is not a data reader, so it could run or apply that text; read the file in its own call"
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
                    if let Some(file) = line.names(target) {
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
    if name == "git" {
        if let Some(why) = git_setting_violation(args, line) {
            return Some(finding(format!(
                "{why}, which a program the line runs would execute"
            )));
        }
    }
    if reads_only(name, args) {
        return None;
    }
    if is_shell(name) || name == "eval" {
        // The script a shell is given is judged as its own commands; the
        // reader expands `-c` strings and `eval` arguments into segments.
        // A script the shell fills in from a substitution cannot be read
        // (`eval "$(echo ...)"`): on a line that names a class file it refuses.
        if line.names(line.text).is_some()
            && line.expands_unread(segment)
            && args
                .iter()
                .any(|w| line.substitute(w).contains(['$', '`', '\u{1}', '\u{2}']))
        {
            return Some(finding(format!(
                "`{name}` runs a script the guard cannot read, and the line names a shell startup file"
            )));
        }
        return None;
    }
    if let Some(call) = copy_call(name, args) {
        return copy_judgment(name, args, &call, line, &dirs).or_else(|| {
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
    // Code whose shell expansion the guard cannot read, on a line that names
    // a class file, refuses as an unresolved redirect does. A word that
    // reads as a path was judged above.
    if line.names(line.text).is_some() && line.expands_unread(segment) {
        if let Some(code) = args.iter().find(|word| {
            !path_like(value_of(word))
                && line.substitute(word).contains(['$', '`', '\u{1}', '\u{2}'])
        }) {
            return Some(finding(format!(
                "`{name}` is given text `{code}` whose expansion the guard cannot resolve, and the line names a shell startup file"
            )));
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
        // Sourcing reads. Declarations set shell variables and write no
        // file; a redirect on the line is judged before this, and startup
        // variables set for a shell are judged on their own (review round
        // seven: `export PATH=$PATH:./bin` from the home).
        "source" | "." | "export" | "declare" | "typeset" | "local" | "readonly" | "unset" => true,
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

/// The global options in front of a git subcommand.
fn git_globals<'a>(args: &'a [String], rest: &[String]) -> &'a [String] {
    &args[..args.len() - rest.len() - 1]
}

/// What the global git setting `key` can do ([`git::config_kind`]): a safe
/// setting runs nothing; a command setting may only name an ordinary viewer
/// on a line that produces text; any other setting refuses there.
fn config_setting_problem(label: &str, key: &str, value: &str, line: &Line<'_>) -> Option<String> {
    let value = line.substitute(value);
    if let Some(class) = line.names(&value) {
        return Some(format!("{label} names the shell startup file `{class}`"));
    }
    if line.names(line.text).is_some() && unresolved_word(&value) {
        return Some(format!("{label} has a value the guard cannot read"));
    }
    if !line.staged {
        return None;
    }
    match git::config_kind(key, &value) {
        ConfigKind::Safe => None,
        ConfigKind::Command => (!viewer_command(&value, line))
            .then(|| format!("{label} is a command that could run text this call produced")),
        ConfigKind::Unknown => Some(format!(
            "{label} sets `{}`, a setting the guard does not know, on a line that produces text",
            key.to_lowercase()
        )),
    }
}

/// Directories git's own programs live in, for `--exec-path`.
const GIT_EXEC_DIRS: &[&str] = &[
    "/usr/lib/git-core",
    "/usr/libexec/git-core",
    "/opt/homebrew/opt/git/libexec/git-core",
    "/Library/Developer/CommandLineTools/usr/libexec/git-core",
];

/// What the global settings of a git call do (`-c key=value`,
/// `--config-env key=VAR` in either spelling, `--exec-path=DIR`): why the
/// call is not a reader, or `None` when every setting is harmless. See
/// [`config_setting_problem`] for the key and value rules.
fn git_setting_violation(args: &[String], line: &Line<'_>) -> Option<String> {
    let (_, rest) = git_subcommand(args)?;
    let globals = git_globals(args, rest);
    let near_class = line.names(line.text).is_some();
    let mut at = 0;
    while at < globals.len() {
        let arg = globals[at].as_str();
        at += 1;
        let found = if arg == "-c" {
            let setting = globals.get(at).map(String::as_str).unwrap_or_default();
            at += 1;
            let (key, value) = setting.split_once('=').unwrap_or((setting, ""));
            config_setting_problem(&format!("`git -c {key}`"), key, value, line)
        } else if arg == "--config-env" || arg.starts_with("--config-env=") {
            let setting = arg.strip_prefix("--config-env=").map_or_else(
                || {
                    at += 1;
                    globals.get(at - 1).cloned().unwrap_or_default()
                },
                str::to_string,
            );
            let (key, var) = setting.split_once('=').unwrap_or((setting.as_str(), ""));
            match line.assigned.get(var) {
                Some(value) => {
                    config_setting_problem(&format!("`git --config-env {key}`"), key, value, line)
                }
                None => near_class.then(|| {
                    format!("`git --config-env {key}` takes its value from `{var}`, which the guard cannot read")
                }),
            }
        } else if let Some(value) = arg.strip_prefix("--exec-path=") {
            // Without `=`, `--exec-path` prints git's program directory and
            // exits; the word after it is read as the subcommand.
            let value = line.substitute(value);
            if let Some(class) = line.names(&value) {
                Some(format!(
                    "`--exec-path` names the shell startup file `{class}`"
                ))
            } else if near_class && unresolved_word(&value) {
                Some("`--exec-path` has a value the guard cannot read".to_string())
            } else {
                (line.staged
                    && (value.is_empty()
                        || !(GIT_EXEC_DIRS.contains(&value.as_str())
                            || SYSTEM_DIRS.contains(&value.as_str()))))
                .then(|| "`--exec-path` picks where git's programs run from".to_string())
            }
        } else {
            None
        };
        if found.is_some() {
            return found;
        }
    }
    None
}

/// A git subcommand after git's global options, with its arguments.
fn git_subcommand(args: &[String]) -> Option<(&str, &[String])> {
    let mut at = 0;
    while let Some(arg) = args.get(at) {
        if GLOBAL_VALUE_OPTIONS.contains(&arg.as_str()) {
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
        if GLOBAL_VALUE_OPTIONS.contains(&arg.as_str()) {
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
        if let Some(file) = line.names(word) {
            return Some(finding(format!(
                "`{name}` is given text that names the shell startup file `{file}`, which it can write"
            )));
        }
        return None;
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
                if let Some(file) = line.names(value) {
                    return Some(finding(format!(
                        "`{name}` runs where the guard cannot tell the directory, and `{value}` could name the shell startup file `{file}`"
                    )));
                }
            }
            None
        }
    }
}

/// Named operands only. Copier option effects and links inside trees are
/// outside this text backstop; the sandbox holds writes through them.
struct CopyCall {
    dest: String,
    sources: Vec<String>,
}

/// Split sources from the destination, recognizing exact target-directory
/// options and a short table of operand-taking options. Do not emulate a
/// copier's dereference, preservation or long-option abbreviation rules.
fn copy_call(name: &str, args: &[String]) -> Option<CopyCall> {
    if !COPIERS.contains(&name) {
        return None;
    }
    let mut operands = Vec::new();
    let mut target = None;
    let mut iter = args.iter();
    let mut options = true;
    let targeted = matches!(name, "cp" | "mv" | "ln" | "install");
    while let Some(arg) = iter.next() {
        if options && arg == "--" {
            options = false;
        } else if options && arg.starts_with('-') && arg.len() > 1 {
            if targeted && arg == "--target-directory" {
                target = iter.next().cloned();
            } else if targeted && arg.starts_with("--target-directory=") {
                target = arg.split_once('=').map(|(_, v)| v.to_string());
            } else if (targeted && arg == "--suffix")
                || (name == "install" && matches!(arg.as_str(), "--mode" | "--owner" | "--group"))
            {
                iter.next();
            } else if targeted && !arg.starts_with("--") {
                for (at, letter) in arg[1..].char_indices() {
                    if !(matches!(letter, 't' | 'S')
                        || name == "install" && matches!(letter, 'm' | 'o' | 'g' | 'l'))
                    {
                        continue;
                    }
                    let rest = &arg[at + 2..];
                    let value = if rest.is_empty() {
                        iter.next().cloned()
                    } else {
                        Some(rest.to_string())
                    };
                    if letter == 't' {
                        target = value;
                    }
                    break;
                }
            }
        } else {
            operands.push(arg.clone());
        }
    }
    let dest = target.or_else(|| (operands.len() >= 2).then(|| operands.pop()).flatten())?;
    Some(CopyCall {
        dest,
        sources: operands,
    })
}

/// Exact flags and short clusters, stopping at a value-taking option.
fn copy_flag(name: &str, args: &[String], short: &[char], long: &[&str]) -> bool {
    args.iter().take_while(|a| a.as_str() != "--").any(|arg| {
        if arg.starts_with("--") {
            long.contains(&arg.as_str())
        } else if let Some(letters) = arg.strip_prefix('-') {
            letters
                .chars()
                .take_while(|c| {
                    !(matches!(c, 't' | 'S')
                        || name == "install" && matches!(c, 'm' | 'o' | 'g' | 'l'))
                })
                .any(|c| short.contains(&c))
        } else {
            false
        }
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
    args: &[String],
    call: &CopyCall,
    line: &Line<'_>,
    dirs: &[PathBuf],
) -> Option<Violation> {
    let CopyCall { dest, sources } = call;
    let dest = dest.as_str();
    let recursive =
        name == "ditto" || copy_flag(name, args, &['r', 'R', 'a'], &["--recursive", "--archive"]);
    let moves = name == "mv";
    let links = name == "ln"
        || (name == "cp" && copy_flag(name, args, &['s', 'l'], &["--symbolic-link", "--link"]));
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
        if moves || links {
            if let Some(v) = word_violation(name, source, line, dirs) {
                return Some(v);
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
        let spelled = tilde_names_a_home(line.text)
            || ["$HOME", "${HOME}", "/etc"]
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
