//! The closed rule of TSK-242: on a line that names a protected target, a
//! form the guard cannot resolve refuses.
//!
//! Fifteen review rounds found new spellings that carry a command past a
//! text guard: another launcher, another option form, an expansion used as
//! the command. The set of programs that can run a command is open, so a
//! list of them never closes. This rule is its complement. Where a line
//! names a shell startup file (exec-guard) or a user or system git
//! configuration write (git-guard), every program on it must be one the
//! guard reads: a data reader used as one, git or gh, a writer whose
//! targets the guards judge and that runs nothing of its own, a shell or
//! launcher whose command the reader unwraps, or a builtin. Anything else
//! refuses, naming the form ([`unresolved_form`]). Each form the guard does
//! read is judged by what it writes, as before.
//!
//! The rule needs the target on the line. A spelling that names none (a
//! path built by a program, a value read from the environment, an include
//! file in the workspace) is a stated residual, held by the harness sandbox
//! where one runs.

use super::git::{config_kind, ConfigKind};
use super::startup::{mentions, reader_program};
use crate::hooks::git_guard::{
    basename, command_argv, expands, gnu_prefixed, is_shell, launcher_commands, launcher_name,
    shell_c_argument, strip_launchers, strip_reserved_words, xargs_command, Expanded,
    NESTING_UNREAD,
};

/// Shell builtins that run no command of their own and that the data
/// readers do not list.
const BUILTINS: &[&str] = &[
    "set", "wait", "read", "return", "exit", "break", "continue", "shopt", "setopt", "unsetopt",
    "umask", "hash", "jobs", "let", "getopts", "dirs", "[[", "]]",
];

/// Programs that write or remove the files they name, whose targets the
/// guards judge, and that run no command unless an option asks for one.
/// Their options are read through [`writer_option`].
const WRITERS: &[&str] = &[
    "cp",
    "mv",
    "ln",
    "install",
    "ditto",
    "tee",
    "dd",
    "truncate",
    "touch",
    "mkdir",
    "mktemp",
    "rm",
    "rmdir",
    "unlink",
    "chmod",
    "chown",
    "chgrp",
    "shred",
    "rsync",
    "scp",
    "tar",
    "bsdtar",
    "gtar",
    "unzip",
    "zip",
    "curl",
    "wget",
    "patch",
    "trash",
    "trash-put",
];

/// The options of a writer that the guards know: short letters that take
/// no value, short letters that take one (the rest of the word, or the next
/// word when the letter ends it), whole short words (`wget -nc`), and long
/// names. Each of them runs nothing and reads no configuration that could
/// name another target.
struct Known {
    flags: &'static str,
    valued: &'static str,
    words: &'static [&'static str],
    long: &'static [&'static str],
}

/// The options known for each writer that has an option which runs a
/// program or reads a configuration: `rsync -e` and `--rsync-path`, `scp
/// -S`, `-o`, `-F` and `-D`, `tar -I`, `-F` and `--to-command`, `zip -TT`,
/// `curl -K`, `wget -e`, `patch -e` and `-g`, `install -s`. Any option not
/// listed refuses on a line that names a protected target, so a writer
/// option the list misses fails closed (review round 16). The other
/// writers (`cp`, `mv`, `ln`, `tee`, `dd` and the like) have no such
/// option, and every option of theirs is read as known.
const KNOWN_OPTIONS: &[(&[&str], Known)] = &[
    (
        &["rsync"],
        Known {
            flags: "avrlptgoDzhPnquciHAXSxRWEmOJLkKbIyF80sCdUN",
            valued: "BTf@",
            words: &[],
            long: &[
                "archive",
                "verbose",
                "quiet",
                "recursive",
                "links",
                "perms",
                "times",
                "group",
                "owner",
                "devices",
                "specials",
                "compress",
                "human-readable",
                "progress",
                "partial",
                "partial-dir",
                "dry-run",
                "update",
                "checksum",
                "itemize-changes",
                "hard-links",
                "acls",
                "xattrs",
                "sparse",
                "one-file-system",
                "relative",
                "whole-file",
                "executability",
                "prune-empty-dirs",
                "omit-dir-times",
                "delete",
                "delete-after",
                "delete-before",
                "delete-during",
                "delete-excluded",
                "exclude",
                "exclude-from",
                "include",
                "include-from",
                "files-from",
                "filter",
                "from0",
                "backup",
                "backup-dir",
                "suffix",
                "inplace",
                "append",
                "append-verify",
                "copy-links",
                "copy-unsafe-links",
                "safe-links",
                "keep-dirlinks",
                "ignore-existing",
                "ignore-times",
                "size-only",
                "existing",
                "stats",
                "info",
                "mkpath",
                "chmod",
                "chown",
                "temp-dir",
                "timeout",
                "bwlimit",
                "max-size",
                "min-size",
                "out-format",
                "log-file",
                "link-dest",
                "compare-dest",
                "copy-dest",
                "numeric-ids",
                "protect-args",
                "secluded-args",
                "no-perms",
                "no-owner",
                "no-group",
                "no-times",
                "dirs",
                "cvs-exclude",
                "fuzzy",
                "atimes",
                "crtimes",
                "modify-window",
                "remove-source-files",
                "list-only",
            ],
        },
    ),
    (
        &["scp"],
        Known {
            flags: "rpqvC346BOTA",
            valued: "PlicJX",
            words: &[],
            long: &[],
        },
    ),
    (
        &["tar", "bsdtar", "gtar"],
        Known {
            flags: "AcdrtuxajJzZkmOpPSvwWhilBGMnoUqyRH",
            valued: "fCTXbgKLNVs",
            words: &[],
            long: &[
                "create",
                "extract",
                "get",
                "list",
                "append",
                "update",
                "diff",
                "compare",
                "file",
                "directory",
                "verbose",
                "gzip",
                "gunzip",
                "bzip2",
                "xz",
                "lzma",
                "zstd",
                "auto-compress",
                "keep-old-files",
                "skip-old-files",
                "overwrite",
                "preserve-permissions",
                "same-permissions",
                "absolute-names",
                "to-stdout",
                "touch",
                "dereference",
                "strip-components",
                "exclude",
                "exclude-from",
                "exclude-vcs",
                "files-from",
                "null",
                "owner",
                "group",
                "mode",
                "mtime",
                "numeric-owner",
                "no-same-owner",
                "no-same-permissions",
                "sort",
                "format",
                "wildcards",
                "anchored",
                "transform",
                "xattrs",
                "acls",
                "one-file-system",
                "totals",
                "no-recursion",
                "recursion",
                "warning",
                "blocking-factor",
                "label",
                "sparse",
                "ignore-zeros",
                "unlink-first",
                "show-transformed-names",
                "no-xattrs",
                "no-acls",
                "options",
            ],
        },
    ),
    (
        &["zip"],
        Known {
            flags: "rqvjyufmdDXlkoAgFeJ0123456789$@",
            valued: "xibntPZsO",
            words: &["-sf", "-FS", "-qq"],
            long: &[
                "recurse-paths",
                "quiet",
                "verbose",
                "junk-paths",
                "symlinks",
                "update",
                "freshen",
                "move",
                "no-dir-entries",
                "no-extra",
                "exclude",
                "include",
                "encrypt",
                "delete",
                "filesync",
                "test-only",
            ],
        },
    ),
    (
        &["unzip"],
        Known {
            flags: "onqvltjaCLXcpzZVMKUWDT",
            valued: "dPx",
            words: &["-qq", "-aa", "-LL", "-UU", "-DD"],
            long: &[],
        },
    ),
    (
        &["curl"],
        Known {
            flags: "sSfLOJkviIqgGZN#0123456RlBnjpaMh",
            valued: "oHXduAemCxwrTFbcDEUYyztQP",
            words: &[],
            long: &[
                "silent",
                "show-error",
                "fail",
                "fail-with-body",
                "fail-early",
                "location",
                "location-trusted",
                "output",
                "output-dir",
                "create-dirs",
                "remote-name",
                "remote-name-all",
                "remote-header-name",
                "insecure",
                "verbose",
                "include",
                "head",
                "header",
                "request",
                "data",
                "data-raw",
                "data-binary",
                "data-urlencode",
                "data-ascii",
                "form",
                "form-string",
                "json",
                "user-agent",
                "referer",
                "max-time",
                "connect-timeout",
                "retry",
                "retry-delay",
                "retry-max-time",
                "retry-all-errors",
                "compressed",
                "proto",
                "proto-redir",
                "tlsv1.2",
                "tlsv1.3",
                "http1.1",
                "http2",
                "url",
                "get",
                "upload-file",
                "dump-header",
                "write-out",
                "continue-at",
                "range",
                "user",
                "oauth2-bearer",
                "cacert",
                "capath",
                "cert",
                "key",
                "no-progress-meter",
                "progress-bar",
                "globoff",
                "max-redirs",
                "noproxy",
                "proxy",
                "ipv4",
                "ipv6",
                "disable",
                "no-buffer",
                "max-filesize",
                "limit-rate",
                "resolve",
                "cookie",
                "cookie-jar",
                "junk-session-cookies",
                "netrc",
                "netrc-optional",
                "stderr",
                "trace",
                "trace-ascii",
                "list-only",
                "append",
                "help",
                "version",
                "manual",
                "ssl-reqd",
                "speed-limit",
                "speed-time",
            ],
        },
    ),
    (
        &["wget"],
        Known {
            flags: "qvcNkprmKEShHLbxF",
            valued: "OoaPtTwUiBlARDQYIX",
            words: &["-nc", "-nv", "-nd", "-nH", "-np"],
            long: &[
                "quiet",
                "verbose",
                "no-verbose",
                "continue",
                "timestamping",
                "output-document",
                "output-file",
                "append-output",
                "directory-prefix",
                "tries",
                "timeout",
                "wait",
                "waitretry",
                "random-wait",
                "user-agent",
                "header",
                "no-check-certificate",
                "recursive",
                "level",
                "no-parent",
                "mirror",
                "page-requisites",
                "convert-links",
                "adjust-extension",
                "input-file",
                "no-clobber",
                "no-directories",
                "no-host-directories",
                "cut-dirs",
                "show-progress",
                "progress",
                "limit-rate",
                "user",
                "password",
                "post-data",
                "post-file",
                "method",
                "body-data",
                "body-file",
                "max-redirect",
                "https-only",
                "inet4-only",
                "inet6-only",
                "spider",
                "server-response",
                "accept",
                "reject",
                "domains",
                "content-disposition",
                "retry-connrefused",
                "quota",
                "no-cache",
                "no-cookies",
                "load-cookies",
                "save-cookies",
                "keep-session-cookies",
                "referer",
                "compression",
                "no-config",
                "force-directories",
                "span-hosts",
                "force-html",
                "base",
            ],
        },
    ),
    (
        &["patch"],
        Known {
            flags: "bcEflnNRstTuvZ",
            valued: "FVxYzBDdiopr",
            words: &[],
            long: &[
                "forward",
                "reverse",
                "batch",
                "force",
                "silent",
                "quiet",
                "dry-run",
                "unified",
                "context",
                "normal",
                "strip",
                "directory",
                "input",
                "output",
                "reject-file",
                "backup",
                "no-backup-if-mismatch",
                "backup-if-mismatch",
                "remove-empty-files",
                "posix",
                "verbose",
                "ignore-whitespace",
                "merge",
                "fuzz",
                "binary",
                "set-time",
                "set-utc",
                "follow-symlinks",
                "read-only",
                "prefix",
                "suffix",
                "basename-prefix",
                "version-control",
                "ifdef",
                "reject-format",
                "quoting-style",
            ],
        },
    ),
    (
        &["install"],
        Known {
            flags: "cdCDpvbTMUS",
            valued: "mogtBfhlN",
            words: &[],
            long: &[
                "mode",
                "owner",
                "group",
                "target-directory",
                "no-target-directory",
                "directory",
                "preserve-timestamps",
                "compare",
                "verbose",
                "backup",
                "suffix",
                "preserve-context",
            ],
        },
    ),
];

fn known_options(name: &str) -> Option<&'static Known> {
    KNOWN_OPTIONS
        .iter()
        .find(|(names, _)| names.contains(&name))
        .map(|(_, known)| known)
}

/// The first form on the line the guard cannot resolve, as words for a
/// refusal; `None` when every command on it is read. `names_target` tells
/// whether a sourced file is the protected target itself, which the user's
/// own shell reads (`source ~/.zshrc`).
pub(crate) fn unresolved_form(
    expanded: &Expanded,
    names_target: &dyn Fn(&str) -> bool,
) -> Option<String> {
    if let Some(word) = expanded.unread_commands.first() {
        return Some(format!(
            "`{}` runs as a command, an expansion the guard cannot read",
            shown(word)
        ));
    }
    expanded.segments.iter().find_map(|segment| {
        if segment == NESTING_UNREAD {
            return Some("it nests commands deeper than the guard reads".to_string());
        }
        let mut words = command_argv(segment);
        strip_reserved_words(&mut words);
        segment_form(&words, names_target)
    })
}

/// The unresolved form of one simple command, or `None` when the guard
/// reads it.
fn segment_form(words: &[String], names_target: &dyn Fn(&str) -> bool) -> Option<String> {
    let launched = strip_launchers(words);
    let program_at = launched.map_or(words.len(), |(_, args)| words.len() - args.len() - 1);
    if let Some(prefixed) = words[..program_at].iter().find(|w| gnu_prefixed(w)) {
        return Some(format!(
            "`{prefixed}`, a GNU-prefixed launcher, runs the command after it"
        ));
    }
    // A search path set for the command picks which program its name runs.
    if let Some(path) = words[..program_at]
        .iter()
        .find(|w| assigns_path(w) || w.as_str() == "-P" || w.starts_with("--path"))
    {
        return Some(format!(
            "`{path}` picks which program the command's name runs"
        ));
    }
    // `env -S` and a bare assignment leave no program: the split string is
    // judged as its own commands.
    let (program, args) = launched?;
    // A command word that expands a name is read through its literal, or
    // listed among the unread commands.
    if expands(program) {
        return None;
    }
    let name = launcher_name(program).trim_end_matches(".exe");
    if is_shell(name) {
        if shell_c_argument(args).is_some() {
            return None;
        }
        let script = args.iter().find(|a| !a.starts_with(['-', '+']));
        return Some(match script {
            Some(file) => format!("`{name} {file}` runs a script the guard does not read"),
            None => format!("`{name}` reads the commands it runs from standard input"),
        });
    }
    match name {
        "eval" | "git" | "gh" => None,
        _ if BUILTINS.contains(&name) => None,
        "source" | "." => match args.first() {
            Some(file) if !expands(file) && names_target(file) => None,
            Some(file) => Some(format!(
                "`{name} {file}` runs a script the guard does not read"
            )),
            None => None,
        },
        "xargs" => xargs_command(args).and_then(|command| {
            (!command_reads(command)).then(|| {
                format!(
                    "`xargs` runs `{}` with arguments it reads from standard input",
                    command[0]
                )
            })
        }),
        "parallel" => Some(
            "`parallel` builds the commands it runs from a template and its inputs".to_string(),
        ),
        "find" => find_commands(args)
            .into_iter()
            .find(|command| !command_reads(command))
            .map(|command| {
                format!(
                    "`find -exec` runs `{}` on each path it finds",
                    command.first().map_or("", String::as_str)
                )
            }),
        "flock" | "script" | "watch" | "setsid" | "unbuffer" => launcher_commands(name, args)
            .is_empty()
            .then(|| format!("`{name}` runs a command the guard does not read")),
        _ if WRITERS.contains(&name) => writer_option(name, args).map(|option| {
            format!(
                "`{name}` is given `{option}`, an option the guard does not know, which could run a command or pick another target"
            )
        }),
        _ if reader_program(program, args) => None,
        _ => Some(format!(
            "`{program}` is a program the guard does not read, which can run a command of its own"
        )),
    }
}

/// Whether a command another program runs (`xargs`, `find -exec`) only
/// reads: a data reader used as one, under any launchers.
fn command_reads(command: &[String]) -> bool {
    strip_launchers(command).is_some_and(|(program, args)| {
        !expands(program) && !gnu_prefixed(program) && reader_program(program, args)
    })
}

/// The commands `find` runs with `-exec`, `-execdir`, `-ok` and `-okdir`.
fn find_commands(args: &[String]) -> Vec<&[String]> {
    args.iter()
        .enumerate()
        .filter(|(_, word)| matches!(word.as_str(), "-exec" | "-execdir" | "-ok" | "-okdir"))
        .map(|(at, _)| {
            let tail = &args[at + 1..];
            let end = tail
                .iter()
                .position(|w| w == ";" || w == "+")
                .unwrap_or(tail.len());
            &tail[..end]
        })
        .collect()
}

/// The first option of a writer call that [`known_options`] does not list,
/// or `None` when every option is known. A `tar` key without a dash
/// (`tar cf x.tar`) is read as a run of short letters.
fn writer_option(name: &str, args: &[String]) -> Option<String> {
    let known = known_options(name)?;
    let tar = matches!(name, "tar" | "bsdtar" | "gtar");
    let mut skip = false;
    for (at, arg) in args.iter().enumerate() {
        if std::mem::take(&mut skip) {
            continue;
        }
        let a = arg.as_str();
        if a == "--" {
            break;
        }
        if let Some(long) = a.strip_prefix("--") {
            let option = long.split_once('=').map_or(long, |(n, _)| n);
            if !known.long.contains(&option) {
                return Some(a.to_string());
            }
            continue;
        }
        let letters = match a.strip_prefix('-') {
            Some(rest) if !rest.is_empty() => rest,
            _ if at == 0 && tar && !a.is_empty() && a.chars().all(|c| c.is_ascii_alphabetic()) => a,
            _ => continue,
        };
        if known.words.contains(&a) {
            continue;
        }
        for (i, c) in letters.char_indices() {
            if known.valued.contains(c) {
                // The value is the rest of the word, or the next word.
                skip = i + c.len_utf8() == letters.len() && letters != a;
                break;
            }
            if !known.flags.contains(c) {
                return Some(a.to_string());
            }
        }
    }
    None
}

/// Whether `word` assigns the search path. The name is compared without
/// case: Windows reads `Path` as `PATH` (review round 16).
fn assigns_path(word: &str) -> bool {
    word.split_once('=')
        .is_some_and(|(name, _)| name.eq_ignore_ascii_case("PATH"))
}

/// A word as a refusal shows it: a lifted substitution reads as `$(...)`.
fn shown(word: &str) -> String {
    word.replace(['\u{1}', '\u{2}'], "$(...)")
}

/// The user or system git configuration a line names, as words for a
/// refusal: a default path of the table (`~/.gitconfig`,
/// `~/.config/git/config`, `/etc/gitconfig` and the Homebrew and
/// `/usr/local` system files), a `gitconfig` in any `etc` directory, a
/// `git/config` below a configuration directory, a `GIT_CONFIG` variable,
/// or a `git config` write at `--global`, `--system`, `--file` or `--blob`
/// scope of a key not known to run nothing, or of a key the line does not
/// show. `None` when the line names none of them.
pub(crate) fn gitconfig_target(text: &str) -> Option<String> {
    let lower = text.replace('\\', "/").to_lowercase();
    let table = &super::actions::table().git_config_paths;
    let named = table
        .home
        .iter()
        .chain(&table.absolute)
        .map(|entry| entry.trim_end_matches('/').to_lowercase())
        .chain(["etc/gitconfig".to_string(), "git/config".to_string()])
        .find(|needle| mentions(&lower, needle));
    if let Some(needle) = named {
        return Some(format!("the git configuration file `{needle}`"));
    }
    if text.contains("GIT_CONFIG") {
        return Some("a `GIT_CONFIG` variable, which names the file git configures".to_string());
    }
    // Words as env, xargs or a shell may split them: at blanks, quotes,
    // operators, `=` and env's `\_`.
    let spaced = text.replace("\\_", " ");
    let words: Vec<&str> = spaced
        .split(|c: char| c.is_whitespace() || "'\";|&()<>`=".contains(c))
        .filter(|w| !w.is_empty())
        .collect();
    let has = |wanted: &[&str]| words.iter().any(|w| wanted.contains(w));
    if !(words.iter().any(|w| basename(w) == "git")
        && has(&["config"])
        && has(&["--global", "--system", "--file", "--blob"]))
    {
        return None;
    }
    let mut keys = words
        .iter()
        .enumerate()
        .filter(|(_, w)| key_shaped(w))
        .peekable();
    if keys.peek().is_none() {
        return Some(
            "a user or system `git config` write whose key the guard cannot see".to_string(),
        );
    }
    keys.find(|(at, key)| {
        let value = words.get(at + 1).copied().unwrap_or_default();
        config_kind(key, value) != ConfigKind::Safe
    })
    .map(|(_, key)| {
        format!("a user or system `git config` write of `{key}`, a key not known to run nothing")
    })
}

/// Whether `word` has the shape of a git configuration key:
/// `section.name` or `section.subsection.name`, with a section and a name
/// of letters, digits and `-` that start with a letter.
fn key_shaped(word: &str) -> bool {
    let Some((section, rest)) = word.split_once('.') else {
        return false;
    };
    let name = rest.rsplit('.').next().unwrap_or(rest);
    let plain = |s: &str| {
        s.starts_with(|c: char| c.is_ascii_alphabetic())
            && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    };
    plain(section) && plain(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::git_guard::expand_commands_read;

    fn form(line: &str) -> Option<String> {
        unresolved_form(&expand_commands_read(line), &|w: &str| w.contains(".zshrc"))
    }

    #[test]
    fn every_form_the_guard_cannot_read_is_named() {
        for (line, expect) in [
            ("$CMD", "`$CMD` runs as a command"),
            ("eval \"$X\"", "`$X` runs as a command"),
            ("sh -c \"$CMD\"", "`$CMD` runs as a command"),
            ("${X:-sh} -c true", "runs as a command"),
            ("$(echo sh) -c true", "runs as a command"),
            ("gtimeout 5 true", "GNU-prefixed launcher"),
            ("gnice true", "GNU-prefixed launcher"),
            ("printf x | xargs git config", "`xargs` runs `git`"),
            ("xargs -I{} sh -c {}", "`xargs` runs `sh`"),
            ("parallel ' {1}' ::: x", "`parallel` builds"),
            ("find . -exec rm {} \\;", "`find -exec` runs `rm`"),
            ("tmux new-session -d true", "`tmux` is a program"),
            ("npm test", "`npm` is a program"),
            ("bash r.sh", "`bash r.sh` runs a script"),
            ("echo x | sh", "`sh` reads the commands"),
            ("source ./r.sh", "`source ./r.sh` runs a script"),
            ("rsync -e 'sh x' a b", "`rsync` is given `-e`"),
            (
                "tar --to-command=sh -xf a.tar",
                "`tar` is given `--to-command=sh`",
            ),
            // Review round 16: a writer option the guard does not know
            // refuses without a keyword in its name.
            ("zip -TT x a.zip b", "`zip` is given `-TT`"),
            (
                "rsync --rsync-path x a b",
                "`rsync` is given `--rsync-path`",
            ),
            ("rsync -avM--x a b", "`rsync` is given `-avM--x`"),
            ("tar cIf x a.tar b", "`tar` is given `cIf`"),
            ("scp -S x a b", "`scp` is given `-S`"),
            ("curl -K cfg -o out", "`curl` is given `-K`"),
            ("wget -e x -O out", "`wget` is given `-e`"),
            ("patch -g1 a", "`patch` is given `-g1`"),
            ("install -s a b", "`install` is given `-s`"),
            ("Path=/tmp/x cat ~/.zshrc", "`Path=/tmp/x` picks"),
            ("path=/tmp/x cat ~/.zshrc", "`path=/tmp/x` picks"),
            ("script -q /dev/null", "`script` runs a command"),
            ("trap 'true' EXIT", "`trap` is a program"),
            ("PATH=/tmp/x cat ~/.zshrc", "`PATH=/tmp/x` picks"),
            ("env -P /tmp/x cat ~/.zshrc", "`-P` picks"),
        ] {
            let found = form(line);
            assert!(
                found.as_deref().is_some_and(|f| f.contains(expect)),
                "{line}: {found:?}"
            );
        }
    }

    #[test]
    fn forms_the_guard_reads_are_resolved() {
        for line in [
            // The narrow reading: an expansion outside the command word, in
            // a resolved read, is not an unresolved form (challenge finding 4).
            "grep \"$PAT\" ~/.zshrc",
            "sh -c 'grep \"$PAT\" ~/.zshrc'",
            "cat ~/.zshrc | sort",
            // A literal the line assigned before it is read as written.
            "CMD='git status'; $CMD",
            "CMD='git status'; sh -c \"$CMD\"",
            "CMD='cat x'; eval \"$CMD\"",
            "source ~/.zshrc",
            "git config --global user.name x",
            "cp ~/.zshrc backup.txt",
            "sed -i '' 's/a/b/' notes.txt",
            "find . -name x -exec grep -l y {} +",
            "printf '%s\\n' a | xargs ls",
            "xargs",
            "env -S 'git status'",
            "flock /tmp/l -c 'true'",
            "timeout 5 git status",
            "cd /tmp && ls",
            "export X=1; echo $X",
            "FOO=1 cat ~/.zshrc",
            // Writer options the guard knows, with their values.
            "curl -fsSL -o out.txt https://example.invalid/x",
            "curl -s -H 'Accept: x' --output out.txt https://example.invalid/x",
            "wget -q -nc -O out.txt https://example.invalid/x",
            "tar -czf a.tgz -C src .",
            "tar xzf a.tgz",
            "rsync -av --delete --exclude target a/ b/",
            "zip -rq a.zip src",
            "unzip -qo a.zip -d out",
            "patch -p1 -i fix.diff",
            "install -m 644 a b",
            "scp -P 2222 a host:b",
            "cp -a a b",
        ] {
            assert_eq!(form(line), None, "{line}");
        }
    }

    #[test]
    fn a_literal_assigned_after_its_use_stays_unread() {
        assert!(form("sh -c \"$CMD\"; CMD='git status'").is_some());
        assert!(form("CMD=a; CMD=b; $CMD").is_some());
        assert!(form("CMD=$OTHER; $CMD").is_some());
        // A prefix assignment lasts for its command only.
        assert!(form("CMD=true env; $CMD").is_some());
    }

    #[test]
    fn git_configuration_targets_are_named() {
        for line in [
            "cat ~/.gitconfig",
            "echo x >> \"$HOME/.config/git/config\"",
            "printf x > /opt/homebrew/etc/gitconfig",
            "printf x > /some/prefix/etc/gitconfig",
            "printf x > \"$XDG_CONFIG_HOME/git/config\"",
            "GIT_CONFIG_GLOBAL=/tmp/x git status",
            "env -S 'git\\_config\\_--global\\_alias.x\\_!id'",
            "printf '%s\\n' --global alias.x '!id' | xargs git config",
            "printf '%s\\n' --global | xargs git config",
            "tmux new-session -d 'git config --global core.pager x'",
            "git config --file=/tmp/x include.path y",
        ] {
            assert!(gitconfig_target(line).is_some(), "{line}");
        }
        for line in [
            "git status",
            "cat .git/config",
            "git config --global user.name x",
            "git config --global user.email a@b.c",
            "git config alias.x '!id'",
            "pip config --global set global.index-url x",
            "gtimeout 5 git status",
        ] {
            assert_eq!(gitconfig_target(line), None, "{line}");
        }
    }
}
