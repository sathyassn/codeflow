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
use super::startup::{mentions, mentions_path_end, reader_program, PATH_END_NEEDLES};
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
///
/// The table records each option's arity, so one reader chooses the
/// operands for every guard (review round 22): a long name or whole word
/// written with a trailing `=` (`exclude=`, `-lf=`) takes a value, attached
/// with `=` or as the next word; a bare long name takes none, or only an
/// attached `=` value (`--backup=numbered`). `writes` names the options
/// whose value is a file the program writes (`rsync --log-file`, `curl
/// --trace`), judged as a written path.
///
/// A writer also reads its environment, which the argument vector does not
/// show (review round 18). Every entry declares it: `env_options` are
/// variables whose value the writer reads as more options (`ZIPOPT`,
/// `TAR_OPTIONS`), judged by the same lists; `env_refused` are variables
/// that name a program, a configuration file or a file it writes
/// (`RSYNC_RSH`, `CURL_HOME`, `TAPE`), which refuse whenever the line
/// assigns one. Both fields are required, and a writer that reads neither
/// is named with its reason in the test `writers_declare_their_environment`.
struct Known {
    flags: &'static str,
    valued: &'static str,
    words: &'static [&'static str],
    long: &'static [&'static str],
    env_options: &'static [&'static str],
    env_refused: &'static [&'static str],
    /// The options whose value is a directory the writer puts files it
    /// does not name into (`-C`, `--directory`), judged by the placement
    /// check from the same scan (review round 19).
    dirs: &'static [&'static str],
    /// The options whose value is a file the writer writes, judged as a
    /// written path wherever it sits on the line (review round 22).
    writes: &'static [&'static str],
}

/// The options known for each writer that has an option which runs a
/// program or reads a configuration: `rsync -e` and `--rsync-path`, `scp
/// -S`, `-o`, `-F` and `-D`, `tar -I`, `-F` and `--to-command`, `zip -TT`,
/// `curl -K`, `wget -e`, `patch -e` and `-g`, `install -s`. Any option not
/// listed refuses on a line that names a protected target, so a writer
/// option the list misses fails closed (review round 16). The copiers
/// (`cp`, `mv`, `ln`, `ditto`) have no such option; their entries give the
/// arity their destination is read with (review round 22). The other
/// writers (`tee`, `dd` and the like) take no option that moves a target.
///
/// An option that changes where a file lands is either a judged `dirs`
/// entry or left out, so the placement check refuses it on a line that
/// names the home, `/etc` or a startup directory (review round 21): a
/// rename by pattern (`tar -s`, `--transform`), a path prefix or a suffix
/// that makes a new name (`patch -B`, `-Y`, `-z`, `install -B`, `-S`,
/// `rsync --suffix`), a path kept from the source (`rsync -R`,
/// `--files-from`) and a temporary location (`zip -b`) are left out.
/// Options that add no directory themselves stay known (`tar
/// --strip-components`, `-P`, `unzip -j`, `patch -p`, `wget -x`, `-nd`,
/// `-nH`, `--cut-dirs`), but a name an archive, a diff or a server
/// supplies can still leave the judged directory (`tar -P` with an
/// absolute member, `patch -p0` with an absolute or `..` path in the
/// diff): the content residual the policy page lists (review round 22).
const KNOWN_OPTIONS: &[(&[&str], Known)] = &[
    (
        &["rsync"],
        Known {
            flags: "avrlptgoDzhPnquciHAXSxWEmOJLkKbIyF80sCdUN",
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
                "partial-dir=",
                "dry-run",
                "update",
                "checksum",
                "itemize-changes",
                "hard-links",
                "acls",
                "xattrs",
                "sparse",
                "one-file-system",
                "whole-file",
                "executability",
                "prune-empty-dirs",
                "omit-dir-times",
                "delete",
                "delete-after",
                "delete-before",
                "delete-during",
                "delete-excluded",
                "exclude=",
                "exclude-from=",
                "include=",
                "include-from=",
                "filter=",
                "from0",
                "backup",
                "backup-dir=",
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
                "info=",
                "mkpath",
                "chmod=",
                "chown=",
                "temp-dir=",
                "timeout=",
                "bwlimit=",
                "compress-level=",
                "max-size=",
                "min-size=",
                "out-format=",
                "log-file=",
                "link-dest=",
                "compare-dest=",
                "copy-dest=",
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
                "modify-window=",
                "remove-source-files",
                "list-only",
            ],
            env_options: &[],
            env_refused: &[
                "RSYNC_RSH",
                "RSYNC_CONNECT_PROG",
                "RSYNC_SHELL",
                "SSH_ASKPASS",
            ],
            dirs: &["-T", "--temp-dir", "--partial-dir", "--backup-dir"],
            writes: &["--log-file"],
        },
    ),
    (
        &["scp"],
        Known {
            flags: "rpqvC346BOTA",
            valued: "PlicJX",
            words: &[],
            long: &[],
            env_options: &[],
            env_refused: &["SSH_ASKPASS"],
            dirs: &[],
            writes: &[],
        },
    ),
    (
        &["tar", "bsdtar", "gtar"],
        Known {
            flags: "AcdrtuxajJzZkmOpPSvwWhilBGMnoUqyRH",
            valued: "fCTXbgKLNV",
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
                "file=",
                "directory=",
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
                "strip-components=",
                "exclude=",
                "exclude-from=",
                "exclude-vcs",
                "files-from=",
                "null",
                "owner=",
                "group=",
                "mode=",
                "mtime=",
                "numeric-owner",
                "no-same-owner",
                "no-same-permissions",
                "sort=",
                "format=",
                "wildcards",
                "anchored",
                "xattrs",
                "acls",
                "one-file-system",
                "totals",
                "no-recursion",
                "recursion",
                "warning=",
                "blocking-factor=",
                "label=",
                "sparse",
                "ignore-zeros",
                "unlink-first",
                "show-transformed-names",
                "no-xattrs",
                "no-acls",
                "options=",
            ],
            env_options: &["TAR_OPTIONS"],
            env_refused: &["TAPE", "TAR_READER_OPTIONS", "TAR_WRITER_OPTIONS"],
            dirs: &["-C", "--directory"],
            writes: &["-f", "--file", "-g"],
        },
    ),
    (
        &["zip"],
        Known {
            flags: "rqvjyufmdDXlkoAgFeJ0123456789$@",
            valued: "xintPZsO",
            words: &["-sf", "-FS", "-qq", "-lf="],
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
                "exclude=",
                "include=",
                "encrypt",
                "delete",
                "filesync",
                "test-only",
            ],
            env_options: &["ZIPOPT", "ZIP", "ZIP_OPTS"],
            env_refused: &[],
            dirs: &[],
            writes: &["-O", "-lf"],
        },
    ),
    (
        &["unzip"],
        Known {
            flags: "onqvltjaCLXcpzZVMKUWDT",
            valued: "dPx",
            words: &["-qq", "-aa", "-LL", "-UU", "-DD"],
            long: &[],
            env_options: &["UNZIP", "UNZIPOPT", "UNZIP_OPTS"],
            env_refused: &[],
            dirs: &["-d"],
            writes: &[],
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
                "output=",
                "output-dir=",
                "create-dirs",
                "remote-name",
                "remote-name-all",
                "remote-header-name",
                "insecure",
                "verbose",
                "include",
                "head",
                "header=",
                "request=",
                "data=",
                "data-raw=",
                "data-binary=",
                "data-urlencode=",
                "data-ascii=",
                "form=",
                "form-string=",
                "json=",
                "user-agent=",
                "referer=",
                "max-time=",
                "connect-timeout=",
                "retry=",
                "retry-delay=",
                "retry-max-time=",
                "retry-all-errors",
                "compressed",
                "proto=",
                "proto-redir=",
                "tlsv1.2",
                "tlsv1.3",
                "http1.1",
                "http2",
                "url=",
                "get",
                "upload-file=",
                "dump-header=",
                "write-out=",
                "continue-at=",
                "range=",
                "user=",
                "oauth2-bearer=",
                "cacert=",
                "capath=",
                "cert=",
                "key=",
                "no-progress-meter",
                "progress-bar",
                "globoff",
                "max-redirs=",
                "noproxy=",
                "proxy=",
                "ipv4",
                "ipv6",
                "disable",
                "no-buffer",
                "max-filesize=",
                "limit-rate=",
                "resolve=",
                "cookie=",
                "cookie-jar=",
                "junk-session-cookies",
                "netrc",
                "netrc-optional",
                "stderr=",
                "trace=",
                "trace-ascii=",
                "list-only",
                "append",
                "help",
                "version",
                "manual",
                "ssl-reqd",
                "speed-limit=",
                "speed-time=",
            ],
            env_options: &[],
            env_refused: &["CURL_HOME", "XDG_CONFIG_HOME", "SSLKEYLOGFILE", "QLOGDIR"],
            dirs: &["--output-dir"],
            writes: &[
                "-o",
                "--output",
                "-D",
                "--dump-header",
                "-c",
                "--cookie-jar",
                "--stderr",
                "--trace",
                "--trace-ascii",
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
                "output-document=",
                "output-file=",
                "append-output=",
                "directory-prefix=",
                "tries=",
                "timeout=",
                "wait=",
                "waitretry=",
                "random-wait",
                "user-agent=",
                "header=",
                "no-check-certificate",
                "recursive",
                "level=",
                "no-parent",
                "mirror",
                "page-requisites",
                "convert-links",
                "adjust-extension",
                "input-file=",
                "no-clobber",
                "no-directories",
                "no-host-directories",
                "cut-dirs=",
                "show-progress",
                "progress=",
                "limit-rate=",
                "user=",
                "password=",
                "post-data=",
                "post-file=",
                "method=",
                "body-data=",
                "body-file=",
                "max-redirect=",
                "https-only",
                "inet4-only",
                "inet6-only",
                "spider",
                "server-response",
                "accept=",
                "reject=",
                "domains=",
                "content-disposition",
                "retry-connrefused",
                "quota=",
                "no-cache",
                "no-cookies",
                "load-cookies=",
                "save-cookies=",
                "keep-session-cookies",
                "referer=",
                "compression=",
                "no-config",
                "force-directories",
                "span-hosts",
                "force-html",
                "base=",
            ],
            env_options: &[],
            env_refused: &["WGETRC", "SYSTEM_WGETRC"],
            dirs: &["-P", "--directory-prefix"],
            writes: &[
                "-O",
                "--output-document",
                "-o",
                "--output-file",
                "-a",
                "--append-output",
                "--save-cookies",
            ],
        },
    ),
    (
        &["patch"],
        Known {
            flags: "bcEflnNRstTuvZ",
            valued: "FVxDdiopr",
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
                "strip=",
                "directory=",
                "input=",
                "output=",
                "reject-file=",
                "backup",
                "no-backup-if-mismatch",
                "backup-if-mismatch",
                "remove-empty-files",
                "posix",
                "verbose",
                "ignore-whitespace",
                "merge",
                "fuzz=",
                "binary",
                "set-time",
                "set-utc",
                "follow-symlinks",
                "read-only=",
                "version-control=",
                "ifdef=",
                "reject-format=",
                "quoting-style=",
            ],
            env_options: &[],
            env_refused: &[
                "PATCH_GET",
                "SIMPLE_BACKUP_SUFFIX",
                "VERSION_CONTROL",
                "PATCH_VERSION_CONTROL",
            ],
            dirs: &["-d", "--directory"],
            writes: &["-o", "--output", "-r", "--reject-file"],
        },
    ),
    (
        &["install"],
        Known {
            flags: "cdCDpvbTMU",
            valued: "mogtfhlN",
            words: &[],
            long: &[
                "mode=",
                "owner=",
                "group=",
                "target-directory=",
                "no-target-directory",
                "directory",
                "preserve-timestamps",
                "compare",
                "verbose",
                "backup",
                "preserve-context",
            ],
            env_options: &[],
            env_refused: &["STRIPBIN", "SIMPLE_BACKUP_SUFFIX", "VERSION_CONTROL"],
            dirs: &[],
            writes: &[],
        },
    ),
    (
        &["cp"],
        Known {
            flags: "abdfiHlLnPpRrsTuvxZcXN",
            valued: "t",
            words: &[],
            long: &[
                "archive",
                "attributes-only",
                "backup",
                "copy-contents",
                "dereference",
                "force",
                "interactive",
                "link",
                "no-dereference",
                "no-clobber",
                "preserve",
                "no-preserve=",
                "recursive",
                "reflink",
                "remove-destination",
                "sparse=",
                "strip-trailing-slashes",
                "symbolic-link",
                "target-directory=",
                "no-target-directory",
                "update",
                "verbose",
                "one-file-system",
                "context",
                "keep-directory-symlink",
                "debug",
            ],
            env_options: &[],
            env_refused: &["SIMPLE_BACKUP_SUFFIX", "VERSION_CONTROL"],
            dirs: &[],
            writes: &[],
        },
    ),
    (
        &["mv"],
        Known {
            flags: "bfinTuvZh",
            valued: "t",
            words: &[],
            long: &[
                "backup",
                "force",
                "interactive",
                "no-clobber",
                "no-copy",
                "strip-trailing-slashes",
                "target-directory=",
                "no-target-directory",
                "update",
                "verbose",
                "context",
                "exchange",
                "debug",
            ],
            env_options: &[],
            env_refused: &["SIMPLE_BACKUP_SUFFIX", "VERSION_CONTROL"],
            dirs: &[],
            writes: &[],
        },
    ),
    (
        &["ln"],
        Known {
            flags: "bdFfiLnPrsTvhw",
            valued: "t",
            words: &[],
            long: &[
                "backup",
                "directory",
                "force",
                "interactive",
                "logical",
                "no-dereference",
                "physical",
                "relative",
                "symbolic",
                "target-directory=",
                "no-target-directory",
                "verbose",
            ],
            env_options: &[],
            env_refused: &["SIMPLE_BACKUP_SUFFIX", "VERSION_CONTROL"],
            dirs: &[],
            writes: &[],
        },
    ),
    (
        &["ditto"],
        Known {
            flags: "hvVXcxkzj",
            valued: "",
            words: &[],
            long: &[
                "keepParent",
                "arch=",
                "bom=",
                "rsrc",
                "norsrc",
                "extattr",
                "noextattr",
                "qtn",
                "noqtn",
                "acl",
                "noacl",
                "nocache",
                "hfsCompression",
                "nohfsCompression",
                "preserveHFSCompression",
                "nopreserveHFSCompression",
                "sequesterRsrc",
                "zlibCompressionLevel=",
                "password",
            ],
            env_options: &[],
            env_refused: &["DITTOKEEPBINARIESDIR"],
            dirs: &[],
            writes: &[],
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
    let assigned = line_assignments(&expanded.segments);
    expanded.segments.iter().find_map(|segment| {
        if segment == NESTING_UNREAD {
            return Some("it nests commands deeper than the guard reads".to_string());
        }
        let mut words = command_argv(segment);
        strip_reserved_words(&mut words);
        segment_form(&words, names_target, &assigned)
    })
}

/// Every `NAME=value` on the line, prefixed, exported or bare: a writer
/// reads its environment as well as its arguments (review round 18).
pub(crate) fn line_assignments(segments: &[String]) -> Vec<(String, String)> {
    segments
        .iter()
        .flat_map(|segment| command_argv(segment))
        .filter_map(|word| {
            let (name, value) = word.split_once('=')?;
            (name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
            .then(|| (name.to_string(), value.to_string()))
        })
        .collect()
}

/// The unresolved form of one simple command, or `None` when the guard
/// reads it.
fn segment_form(
    words: &[String],
    names_target: &dyn Fn(&str) -> bool,
    assigned: &[(String, String)],
) -> Option<String> {
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
        _ if WRITERS.contains(&name) => writer_form(name, args, assigned),
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
/// or `None` when every option is known ([`scan_writer`]).
fn writer_option(name: &str, args: &[String]) -> Option<String> {
    scan_writer(name, args)?.unknown
}

/// What one pass over a writer's arguments reads: the first option the
/// table does not list, each option with its value, each option read
/// without one, and the operands.
struct Scan {
    unknown: Option<String>,
    values: Vec<(String, String)>,
    flags: Vec<String>,
    operands: Vec<String>,
}

impl Scan {
    /// Note an option the table does not list; the first one names the form.
    fn unlisted(&mut self, word: &str) {
        self.unknown.get_or_insert_with(|| word.to_string());
    }
}

/// Read a writer's arguments with [`known_options`]; `None` for a program
/// with no entry. A short option takes its value from the rest of its
/// word, or from the next word when it ends the word. A `tar` key without
/// a dash (`tar cfI x.tar cmd`) is a run of letters read whole: each letter
/// that takes a value takes the next word, in order, so a letter after `f`
/// is still judged (review round 17).
///
/// A letter takes the next word as its value only when that word is not
/// an option itself (review round 18): the tar dialects disagree on which
/// letters take one (BSD `-L` is a flag, GNU `-L` takes a length), and a word the
/// table took as a value would otherwise hide an option that runs a
/// program. A lone `-` is an operand, and a value such as rsync's
/// `- *.o` filter rule is not an option.
///
/// An option the table does not list is recorded and the scan goes on,
/// through the rest of its short cluster and the later words, read as a
/// flag: a directory option after it is still read, so an unlisted option
/// cannot hide where the call places files (review round 20). The values
/// feed the placement check, so an option the table reads is never read
/// without its value (review round 19).
fn scan_writer(name: &str, args: &[String]) -> Option<Scan> {
    let known = known_options(name)?;
    let tar = matches!(name, "tar" | "bsdtar" | "gtar");
    let mut scan = Scan {
        unknown: None,
        values: Vec::new(),
        flags: Vec::new(),
        operands: Vec::new(),
    };
    let mut pending: std::collections::VecDeque<String> = std::collections::VecDeque::new();
    let mut words = args.iter().enumerate();
    while let Some((at, arg)) = words.next() {
        let a = arg.as_str();
        if !option_like(a) {
            if let Some(option) = pending.pop_front() {
                scan.values.push((option, a.to_string()));
                continue;
            }
        }
        if a == "--" {
            scan.operands.extend(words.map(|(_, w)| w.clone()));
            break;
        }
        if let Some(long) = a.strip_prefix("--") {
            let (option, value) = long
                .split_once('=')
                .map_or((long, None), |(n, v)| (n, Some(v)));
            let arity = known
                .long
                .iter()
                .find(|entry| entry.trim_end_matches('=') == option);
            if arity.is_none() {
                scan.unlisted(a);
            }
            let option = format!("--{option}");
            match value {
                Some(value) => scan.values.push((option, value.to_string())),
                // A long option that takes a value takes the next word,
                // whatever it is, as getopt does (review round 22).
                None if arity.is_some_and(|entry| entry.ends_with('=')) => {
                    let value = words.next().map(|(_, w)| w.clone()).unwrap_or_default();
                    scan.values.push((option, value));
                }
                None => scan.flags.push(option),
            }
            continue;
        }
        if known.words.contains(&a) {
            scan.flags.push(a.to_string());
            continue;
        }
        if known.words.iter().any(|w| w.strip_suffix('=') == Some(a)) {
            let value = words.next().map(|(_, w)| w.clone()).unwrap_or_default();
            scan.values.push((a.to_string(), value));
            continue;
        }
        if at == 0 && tar && !a.is_empty() && !a.starts_with('-') {
            for c in a.chars() {
                if known.valued.contains(c) {
                    pending.push_back(format!("-{c}"));
                } else {
                    if !known.flags.contains(c) {
                        scan.unlisted(a);
                    }
                    scan.flags.push(format!("-{c}"));
                }
            }
            continue;
        }
        if !option_like(a) {
            scan.operands.push(a.to_string());
            continue;
        }
        let letters = &a[1..];
        for (i, c) in letters.char_indices() {
            if known.valued.contains(c) {
                let rest = &letters[i + c.len_utf8()..];
                if rest.is_empty() {
                    pending.push_back(format!("-{c}"));
                } else {
                    scan.values.push((format!("-{c}"), rest.to_string()));
                }
                break;
            }
            if !known.flags.contains(c) {
                scan.unlisted(a);
            }
            scan.flags.push(format!("-{c}"));
        }
    }
    Some(scan)
}

/// A writer call as the scan reads it, from its arguments and from each
/// variable it reads as options ([`writer_read`]).
pub(crate) struct WriterRead {
    /// Where the call is told to put files it does not name: the values
    /// of the options its entry lists in `dirs`.
    pub(crate) dirs: Vec<String>,
    /// Every option read without a value (`-x`, `--extract`).
    pub(crate) flags: Vec<String>,
    /// Every option read with its value.
    pub(crate) values: Vec<(String, String)>,
    /// The operands on the command line.
    pub(crate) operands: Vec<String>,
    /// The first option the table does not list.
    pub(crate) unknown: Option<String>,
    /// The values of the options the entry lists in `writes`: files the
    /// call writes besides its operands.
    pub(crate) written: Vec<String>,
}

/// Read a writer call with the shared scan: its command line, then each
/// variable the line assigns that the writer reads as more options. `None`
/// for a program with no entry, whose options the guard does not read.
pub(crate) fn writer_read(
    name: &str,
    args: &[String],
    assigned: &[(String, String)],
) -> Option<WriterRead> {
    let known = known_options(name)?;
    let mut read = WriterRead {
        dirs: Vec::new(),
        flags: Vec::new(),
        values: Vec::new(),
        operands: Vec::new(),
        unknown: None,
        written: Vec::new(),
    };
    // The writer reads its option variables before its command line (GNU
    // tar prepends `TAR_OPTIONS`, Info-ZIP reads `UNZIP` and `ZIPOPT`
    // first), so their directories come first and a command-line directory
    // is the last one folded (review round 21). The `-` in front keeps a
    // variable's first word from being read as a dashless tar key.
    let mut lists = Vec::new();
    for (var, value) in assigned {
        if known.env_options.contains(&var.as_str()) {
            let mut list = vec!["-".to_string()];
            list.extend(value.split_whitespace().map(str::to_string));
            lists.push(list);
        }
    }
    lists.push(args.to_vec());
    let last = lists.len() - 1;
    for (index, list) in lists.iter().enumerate() {
        let scan = scan_writer(name, list)?;
        if read.unknown.is_none() {
            read.unknown = scan.unknown;
        }
        read.dirs.extend(
            scan.values
                .iter()
                .filter(|(option, _)| known.dirs.contains(&option.as_str()))
                .map(|(_, value)| value.clone()),
        );
        read.written.extend(
            scan.values
                .iter()
                .filter(|(option, _)| known.writes.contains(&option.as_str()))
                .map(|(_, value)| value.clone()),
        );
        read.values.extend(scan.values);
        read.flags.extend(scan.flags);
        if index == last {
            read.operands = scan.operands;
        }
    }
    Some(read)
}

/// A copy, link, move or install call as the shared judge reads it
/// ([`writer_effects`]).
pub(crate) struct CopyRead {
    /// The destination: the target directory (`-t DIR`), else the last
    /// operand when there are two or more.
    pub(crate) dest: Option<String>,
    /// Whether the destination is a target directory every source lands in.
    pub(crate) target: bool,
    /// The operands that are not the destination.
    pub(crate) sources: Vec<String>,
    /// `install -d`: every operand is a directory the call makes.
    pub(crate) directories: bool,
    /// Files the call writes through its options (`rsync --log-file`).
    pub(crate) written: Vec<String>,
    /// Every option read without a value (`-r`, `--archive`).
    pub(crate) flags: Vec<String>,
    /// The first option the table does not list. Its arity is unknown, so
    /// any operand could be the destination.
    pub(crate) unknown: Option<String>,
}

/// Read a copier's operands with the writer table's arity: every option
/// that takes a word takes it before the destination is chosen, wherever
/// it sits on the line, so `rsync -a src DEST --exclude foo` writes DEST
/// (review round 22). `None` for a program with no entry. The guards read
/// it through [`judge_writer`]; tests read it directly.
#[cfg(test)]
pub(crate) fn copy_read(name: &str, args: &[String]) -> Option<CopyRead> {
    copy_of(name, &writer_read(name, args, &[])?)
}

/// The copy a writer read describes, for a program that copies, links,
/// moves or installs named sources (`None` for the others).
fn copy_of(name: &str, read: &WriterRead) -> Option<CopyRead> {
    if !COPIES.contains(&name) {
        return None;
    }
    let target = matches!(name, "cp" | "mv" | "ln" | "install")
        .then(|| {
            read.values
                .iter()
                .rev()
                .find(|(option, _)| matches!(option.as_str(), "-t" | "--target-directory"))
                .map(|(_, value)| value.clone())
        })
        .flatten();
    let directories = name == "install"
        && read
            .flags
            .iter()
            .any(|flag| flag == "-d" || flag == "--directory");
    let mut sources = read.operands.clone();
    let dest = match &target {
        Some(dir) => Some(dir.clone()),
        None if sources.len() >= 2 => sources.pop(),
        None => None,
    };
    Some(CopyRead {
        dest,
        target: target.is_some(),
        sources,
        directories,
        written: read.written.clone(),
        flags: read.flags.clone(),
        unknown: read.unknown.clone(),
    })
}

/// The writers that copy, link, move or install named sources to a
/// destination.
const COPIES: &[&str] = &["cp", "ln", "install", "mv", "rsync", "scp", "ditto"];

/// The order a writer applies its directory options in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum DirOrder {
    /// Each directory from where the last one left (`tar -C a -C b`).
    Sequential,
    /// The last directory given holds (`unzip -d`, `wget -P`, `patch -d`).
    Last,
    /// Each directory is its own place, relative to the destination when
    /// relative (rsync's backup, partial and temporary directories).
    Each,
}

/// Where a writer call puts files it does not name.
pub(crate) struct Places {
    /// The directory option values, in the order the call gives them;
    /// empty for the working directory.
    pub(crate) dirs: Vec<String>,
    pub(crate) order: DirOrder,
}

/// Everything a writer call does to the file system, read once from the
/// table with the line's option environment ([`writer_effects`]).
pub(crate) struct WriterEffects {
    /// The table's reading of the call.
    pub(crate) read: WriterRead,
    /// The copy, for a writer that copies named sources.
    pub(crate) copy: Option<CopyRead>,
    /// The files the call writes besides a copy's destination: each
    /// `writes` value (from the command line or the option environment)
    /// and each operand the program writes (`zip ARCHIVE`, `patch FILE`),
    /// as spelled and joined with the writer's directory options in
    /// program order (`patch -d DIR -o OUT` writes `DIR/OUT`).
    pub(crate) writes: Vec<String>,
    /// Where it puts files it does not name, or `None` when it places
    /// none: a tar extract, a curl remote name, a wget download without
    /// `-O`, rsync's own directories, an unzip or a patch.
    pub(crate) places: Option<Places>,
}

/// Read a writer call once, with the line's option environment, into the
/// effects both guards judge (review round 23). `None` for a program with
/// no entry.
pub(crate) fn writer_effects(
    name: &str,
    args: &[String],
    assigned: &[(String, String)],
) -> Option<WriterEffects> {
    let read = writer_read(name, args, assigned)?;
    let copy = copy_of(name, &read);
    let flagged = |names: &[&str]| read.flags.iter().any(|f| names.contains(&f.as_str()));
    let valued = |names: &[&str]| read.values.iter().any(|(o, _)| names.contains(&o.as_str()));
    let tar = matches!(name, "tar" | "bsdtar" | "gtar");
    // tar writes its archive only when it creates or adds to one; a list
    // or an extract reads it.
    let tar_writes = flagged(&["-c", "-r", "-u", "-A", "--create", "--append", "--update"]);
    // The mode comes from the letters the scan read as options, so an
    // attached archive name is never read as one (review round 20); a call
    // with no mode it can tell extracts (fail closed).
    let tar_extracts = flagged(&["-x", "--extract", "--get"])
        || !flagged(&[
            "-c",
            "-t",
            "-r",
            "-u",
            "-d",
            "--create",
            "--list",
            "--append",
            "--update",
            "--diff",
            "--compare",
            "--delete",
        ]);
    let order = match name {
        _ if tar => DirOrder::Sequential,
        "rsync" => DirOrder::Each,
        _ => DirOrder::Last,
    };
    let mut written: Vec<String> = read
        .values
        .iter()
        .filter(|(option, _)| {
            known_options(name).is_some_and(|k| k.writes.contains(&option.as_str()))
                && !(tar && matches!(option.as_str(), "-f" | "--file") && !tar_writes)
        })
        .map(|(_, value)| value.clone())
        .collect();
    // The operand a non-copying writer writes: zip's archive, the file
    // patch changes.
    if matches!(name, "zip" | "patch") {
        written.extend(read.operands.first().cloned());
    }
    let base = match order {
        DirOrder::Sequential => read.dirs.iter().fold(None::<String>, |at, dir| {
            Some(match at {
                Some(at) if !rooted(dir) => format!("{}/{dir}", at.trim_end_matches('/')),
                _ => dir.clone(),
            })
        }),
        DirOrder::Last => read.dirs.last().cloned(),
        DirOrder::Each => None,
    };
    let mut writes = Vec::new();
    for path in written {
        if let Some(base) = base.as_deref().filter(|_| !rooted(&path)) {
            writes.push(format!("{}/{path}", base.trim_end_matches('/')));
        }
        writes.push(path);
    }
    let curl_template = read.values.iter().any(|(option, value)| {
        matches!(option.as_str(), "-o" | "--output")
            && value
                .match_indices('#')
                .any(|(at, _)| value[at + 1..].starts_with(|c: char| c.is_ascii_digit()))
    });
    let places = match name {
        _ if tar => tar_extracts.then_some(order),
        // Without a remote name or an output template (`-o '#1'`), curl
        // writes only the files it names.
        "curl" => (flagged(&[
            "-O",
            "--remote-name",
            "-J",
            "--remote-header-name",
            "--remote-name-all",
        ]) || curl_template)
            .then_some(order),
        // A download to a named file places nothing else.
        "wget" => (!valued(&["-O", "--output-document"])).then_some(order),
        // rsync places files it does not name only in its own directories;
        // its destination is judged as a copy.
        "rsync" => (!read.dirs.is_empty()).then_some(order),
        "unzip" | "patch" => Some(order),
        _ => None,
    }
    .map(|order| Places {
        dirs: read.dirs.clone(),
        order,
    });
    Some(WriterEffects {
        read,
        copy,
        writes,
        places,
    })
}

/// Whether a path does not hang off the working directory: absolute, from
/// a home, or starting with an expansion.
fn rooted(path: &str) -> bool {
    path.starts_with(['/', '~', '$', '\\'])
        || path.get(1..3) == Some(":/")
        || path.get(1..3) == Some(":\\")
}

/// What a guard judges a writer's effects against: exec-guard the startup
/// and user git configuration classes, git-guard the integrity files and
/// prefixes and the repository configuration (review round 23).
pub(crate) trait WriterGuard {
    type Found;
    /// A file the call writes.
    fn writes(&self, path: &str) -> Option<Self::Found>;
    /// A copy's destination and sources.
    fn copies(&self, copy: &CopyRead) -> Option<Self::Found>;
    /// The directories the call puts files it does not name into.
    fn places(&self, places: &Places, effects: &WriterEffects) -> Option<Self::Found>;
    /// An option the table does not list: its arity is unknown, so any
    /// operand could be a target.
    fn unread(&self, option: &str, effects: &WriterEffects) -> Option<Self::Found>;
}

/// The one judge of a writer call, for every program the table lists and
/// both guards (review round 23): it reads the table once with the line's
/// option environment and hands the guard the copy, every written path
/// joined with the writer's directories, an unlisted option, and the
/// directories the call places into. `None` for a program with no entry.
pub(crate) fn judge_writer<G: WriterGuard>(
    name: &str,
    args: &[String],
    assigned: &[(String, String)],
    guard: &G,
) -> Option<G::Found> {
    let effects = writer_effects(name, args, assigned)?;
    if let Some(found) = effects.copy.as_ref().and_then(|copy| guard.copies(copy)) {
        return Some(found);
    }
    if let Some(found) = effects.writes.iter().find_map(|path| guard.writes(path)) {
        return Some(found);
    }
    if let Some(found) = effects
        .read
        .unknown
        .as_deref()
        .and_then(|option| guard.unread(option, &effects))
    {
        return Some(found);
    }
    effects
        .places
        .as_ref()
        .and_then(|places| guard.places(places, &effects))
}

/// Whether the table lists `name`, so the shared judge reads its options.
pub(crate) fn has_writer_entry(name: &str) -> bool {
    known_options(name).is_some()
}

/// Every writer the table lists, each with a call that writes `file` (or,
/// for one that writes no named file, places into `dir`), for the test
/// that both guards judge every entry (review round 23). A new entry
/// without a spelling fails that test.
#[cfg(test)]
pub(crate) fn writer_spellings(file: &str, dir: &str) -> Vec<(&'static str, String)> {
    KNOWN_OPTIONS
        .iter()
        .flat_map(|(names, _)| names.iter().copied())
        .map(|name| {
            let call = match name {
                "rsync" => format!("rsync -a payload {file} --exclude foo"),
                "scp" | "cp" | "mv" | "ln" | "install" | "ditto" => {
                    format!("{name} payload {file}")
                }
                "tar" | "bsdtar" | "gtar" => format!("{name} -cf {file} payload"),
                "zip" => format!("zip {file} payload"),
                "unzip" => format!("unzip -o payload.zip -d {dir}"),
                "curl" => format!("curl -o {file} https://example.invalid/x"),
                "wget" => format!("wget -O {file} https://example.invalid/x"),
                "patch" => format!("patch -o {file} a.txt fix.diff"),
                _ => panic!("give `{name}` a spelling for the every-entry test"),
            };
            (name, call)
        })
        .collect()
}

/// Whether `word` is an option rather than a value: a `-` followed by a
/// letter, a digit or a second `-`.
fn option_like(word: &str) -> bool {
    word.strip_prefix('-')
        .and_then(|rest| rest.chars().next())
        .is_some_and(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// The unread form of a writer call: an option [`writer_option`] does not
/// know, on the command line or in a variable the writer reads as options,
/// or a variable that names a program, a configuration or a file the
/// writer uses. `assigned` is every `NAME=value` the line sets before or
/// for the call.
fn writer_form(name: &str, args: &[String], assigned: &[(String, String)]) -> Option<String> {
    if let Some(option) = writer_option(name, args) {
        return Some(format!(
            "`{name}` is given `{option}`, an option the guard does not know, which could run a command or pick another target"
        ));
    }
    let known = known_options(name)?;
    for (var, value) in assigned {
        if known.env_refused.contains(&var.as_str()) {
            return Some(format!(
                "`{name}` reads `{var}`, which names a program, a configuration or a file the guard does not read"
            ));
        }
        if known.env_options.contains(&var.as_str()) {
            let words: Vec<String> = value.split_whitespace().map(str::to_string).collect();
            // The value comes before the command line, so a tar key there
            // is not the first word.
            let mut read = vec!["-".to_string()];
            read.extend(words);
            if let Some(option) = writer_option(name, &read) {
                return Some(format!(
                    "`{name}` reads `{var}` as options, which give it `{option}`, an option the guard does not know"
                ));
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
        .find(|needle| {
            if PATH_END_NEEDLES.contains(&needle.as_str()) {
                mentions_path_end(&lower, needle)
            } else {
                mentions(&lower, needle)
            }
        });
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
            // Review round 17: each letter of a dashless tar key that takes
            // a value takes the next word, and the letters after it are
            // still judged.
            ("tar cfI a.tar x b", "`tar` is given `cfI`"),
            ("tar cfF a.tar x b", "`tar` is given `cfF`"),
            ("tar cfLF a.tar 1 x b", "`tar` is given `cfLF`"),
            ("scp -S x a b", "`scp` is given `-S`"),
            ("curl -K cfg -o out", "`curl` is given `-K`"),
            ("wget -e x -O out", "`wget` is given `-e`"),
            ("patch -g1 a", "`patch` is given `-g1`"),
            ("install -s a b", "`install` is given `-s`"),
            ("Path=/tmp/x cat ~/.zshrc", "`Path=/tmp/x` picks"),
            // Review round 18: a word that is an option is never taken as
            // a value, and the environment a writer reads is judged.
            (
                "tar -L --use-compress-program p -cf a.tar b",
                "`--use-compress-program`",
            ),
            (
                "tar cL --use-compress-program p -f a.tar b",
                "`--use-compress-program`",
            ),
            ("tar -L -I p -xf a.tar", "`-I`"),
            // A rename by pattern is left out of the table (review round 21).
            ("tar -s 's/a/b/' -cf a.tar README", "`-s`"),
            ("tar --transform=s/a/b/ -xf a.tar", "`--transform=s/a/b/`"),
            ("rsync -R a/b c/", "`-R`"),
            ("patch -B bak/ -p1 -i f.diff", "`-B`"),
            ("install -S .bak a b", "`-S`"),
            ("ZIPOPT='-T -TT p' zip a.zip f", "reads `ZIPOPT` as options"),
            (
                "TAR_OPTIONS='--use-compress-program=p' tar -cf a.tar f",
                "reads `TAR_OPTIONS`",
            ),
            (
                "export TAR_OPTIONS=--to-command=p; tar -xf a.tar",
                "reads `TAR_OPTIONS`",
            ),
            ("RSYNC_RSH=p rsync -a f host:d", "reads `RSYNC_RSH`"),
            ("RSYNC_SHELL=p rsync -a f d", "reads `RSYNC_SHELL`"),
            (
                "CURL_HOME=/tmp/x curl -o out https://example.invalid",
                "reads `CURL_HOME`",
            ),
            ("TAPE=out tar -c f", "reads `TAPE`"),
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
            "tar cfv a.tar README",
            "tar -L 1024 -cf a.tar README",
            "tar -cf - README",
            "rsync -f '- *.o' a/ b/",
            "ZIPOPT=-q zip a.zip f",
            "LANG=C cat ~/.zshrc",
            "tar cf a.tar README",
            "tar -czf a.tgz README",
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

    /// Writers of [`KNOWN_OPTIONS`] that read no option, program or file
    /// from their environment, each with the reason. None does today.
    const READS_NO_ENVIRONMENT: &[(&str, &str)] = &[];

    /// Placement directories come from the same scan as the option check:
    /// clustered letters, attached values, dashless tar keys and option
    /// variables (review round 19).
    #[test]
    fn placement_directories_come_from_the_writer_scan() {
        let words = |line: &str| {
            line.split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>()
        };
        let dirs = |name: &str, line: &str, env: &[(&str, &str)]| {
            let env: Vec<(String, String)> = env
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect();
            writer_read(name, &words(line), &env).map(|read| read.dirs)
        };
        let some = |list: &[&str]| Some(list.iter().map(|d| (*d).to_string()).collect::<Vec<_>>());
        assert_eq!(dirs("tar", "-xC$HOME -f a", &[]), some(&["$HOME"]));
        assert_eq!(dirs("tar", "xfC a ~", &[]), some(&["~"]));
        assert_eq!(dirs("tar", "-x --directory d -f a", &[]), some(&["d"]));
        assert_eq!(
            dirs("tar", "-xf a", &[("TAR_OPTIONS", "-C ~")]),
            some(&["~"])
        );
        assert_eq!(dirs("unzip", "-od~ a", &[]), some(&["~"]));
        assert_eq!(dirs("unzip", "-o a", &[("UNZIP", "-d ~")]), some(&["~"]));
        assert_eq!(dirs("unzip", "a", &[("UNZIP", "-qq")]), some(&[]));
        assert_eq!(dirs("patch", "-d$HOME -p1", &[]), some(&["$HOME"]));
        assert_eq!(dirs("wget", "-P ~ u", &[]), some(&["~"]));
        assert_eq!(dirs("curl", "--output-dir=d -O u", &[]), some(&["d"]));
        assert_eq!(dirs("tar", "-xC ./build -f a", &[]), some(&["./build"]));
        assert_eq!(dirs("7z", "x a -o~", &[]), None);
        assert_eq!(dirs("cpio", "-idm -D ~", &[]), None);
        // An unlisted option does not end the scan: a directory after it,
        // or later in its cluster, is still read (review round 20).
        assert_eq!(
            dirs("tar", "-x --no-mac-metadata -C$HOME -f a", &[]),
            some(&["$HOME"])
        );
        assert_eq!(dirs("unzip", "-uod~ a", &[]), some(&["~"]));
        assert_eq!(dirs("patch", "-t -g0 -d$HOME -i f", &[]), some(&["$HOME"]));
        assert_eq!(
            dirs("tar", "-xf a", &[("TAR_OPTIONS", "--no-mac-metadata -C~")]),
            some(&["~"])
        );
        assert_eq!(dirs("wget", "--connect-timeout=5 -P~ u", &[]), some(&["~"]));
        let read = writer_read("tar", &words("-czf/tmp/box.tar -C d ."), &[]).unwrap();
        assert!(read.flags.contains(&"-c".to_string()), "{:?}", read.flags);
        assert!(!read.flags.contains(&"-x".to_string()), "{:?}", read.flags);
        // The option variables come first, so the command line's directory
        // is the last one (review round 21).
        assert_eq!(
            dirs("tar", "-C/tmp/s -xf a", &[("TAR_OPTIONS", "-C ~")]),
            some(&["~", "/tmp/s"])
        );
        assert_eq!(
            dirs("rsync", "-a --backup-dir=b -T t --partial-dir p s d", &[]),
            some(&["b", "t", "p"])
        );
        let read = writer_read("unzip", &words("-uod d a"), &[]).unwrap();
        assert_eq!(read.unknown.as_deref(), Some("-uod"));
    }

    /// One reader chooses a copier's operands with the table's arity, so a
    /// known option that takes a word cannot move the destination, and a
    /// file an option writes is read as one (review round 22).
    #[test]
    fn copy_operands_follow_the_table_arity() {
        let words = |line: &str| {
            line.split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>()
        };
        for (name, line, dest) in [
            ("rsync", "-a src D --exclude foo", "D"),
            ("rsync", "-a src D --chmod 644", "D"),
            ("rsync", "-a src D -T tmp", "D"),
            ("rsync", "-a --exclude foo src D", "D"),
            ("rsync", "-a src D --exclude=foo", "D"),
            ("cp", "src D --no-preserve mode", "D"),
            ("install", "-m 644 src D", "D"),
            ("ln", "-s src D", "D"),
            ("cp", "-t D src", "D"),
        ] {
            let read = copy_read(name, &words(line)).unwrap();
            assert_eq!(read.dest.as_deref(), Some(dest), "{name} {line}");
            assert!(read.unknown.is_none(), "{name} {line}");
        }
        let read = copy_read("rsync", &words("-a src D --log-file L")).unwrap();
        assert_eq!(read.written, vec!["L".to_string()]);
        assert_eq!(read.dest.as_deref(), Some("D"));
        let read = copy_read("cp", &words("src D --suffix .bak")).unwrap();
        assert_eq!(read.unknown.as_deref(), Some("--suffix"));
    }

    /// Review round 18: a writer entry declares the environment it reads,
    /// or is named here with the reason it reads none.
    #[test]
    fn writers_declare_their_environment() {
        for (names, known) in KNOWN_OPTIONS {
            let declared = !known.env_options.is_empty() || !known.env_refused.is_empty();
            let excused = names.iter().any(|n| {
                READS_NO_ENVIRONMENT
                    .iter()
                    .any(|(name, why)| name == n && !why.is_empty())
            });
            assert!(
                declared != excused,
                "{names:?}: declare its environment or the reason it reads none"
            );
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
            "vim git/config",
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
            // Review round 17: `git/config` ends the path it names.
            "vim src/git/config.rs",
            "vim docs/git/config.md",
            "ls lib/git/config/",
        ] {
            assert_eq!(gitconfig_target(line), None, "{line}");
        }
    }
}
