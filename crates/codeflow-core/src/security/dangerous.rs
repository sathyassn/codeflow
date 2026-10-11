//! Detects and blocks destructive commands.
//!
//! Checks for: recursive deletion of root/system dirs, disk operations,
//! dangerous permission changes, fork bombs.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;

use super::{block, CheckContext, SecurityModule, Verdict};

/// Substring patterns that always block.
const DANGEROUS_SUBSTRINGS: &[&str] =
    &["dd if=/dev/zero", "dd if=/dev/random", "mkfs.", "> /dev/sd"];

/// System directories whose recursive deletion is catastrophic, including the
/// native macOS roots in addition to the Unix FHS roots.
const SYSTEM_DIRS: &[&str] = &[
    "etc",
    "var",
    "usr",
    "bin",
    "sbin",
    "boot",
    "lib",
    "lib64",
    "opt",
    "root",
    "sys",
    "proc",
    "dev",
    "System",
    "Library",
    "Applications",
    "private",
    "cores",
    "run",
    "srv",
    "snap",
];

/// Top-level user or mount collections whose *whole-tree* deletion is
/// catastrophic. Descendants are not classified here because a consuming
/// project may legitimately live below `/Users`, `/home`, or `/Volumes`; the
/// harness workspace sandbox remains the boundary for those scoped paths.
const ROOT_COLLECTION_DIRS: &[&str] = &["Users", "home", "Volumes", "mnt", "media"];

/// The collections whose direct children are home directories.
const HOME_COLLECTION_DIRS: &[&str] = &["Users", "home"];

/// Split a shell command into tokens without treating Windows path backslashes
/// as escapes. This is intentionally a small classifier, not a shell parser:
/// it preserves quoted paths such as `C:\Program Files` and is conservative
/// around malformed quotes.
fn command_tokens(command: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    for ch in command.chars() {
        match (quote, ch) {
            (Some(active), c) if c == active => quote = None,
            (None, '\'' | '"') => quote = Some(ch),
            // This fallback also reads raw PowerShell, whose lexical grammar
            // includes Unicode separators. POSIX deletion has its own reader.
            (None, c) if c.is_whitespace() => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

pub(super) fn program_name(token: &str) -> String {
    token
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(token)
        .to_ascii_lowercase()
}

/// Resolve the common launch layers agents use on supported hosts. The hard
/// catastrophe floor must inspect the command that will actually execute, not
/// stop at `sudo`, Windows `runas`/`gsudo`, a shell `-c`, or `PowerShell`'s
/// `Start-Process`. This deliberately handles only structured, well-known
/// launch forms; it is not intended to emulate a shell.
pub(super) fn effective_invocation(tokens: &[String]) -> Vec<String> {
    let mut current = tokens.to_vec();
    for _ in 0..4 {
        let Some(program) = current.first().map(|token| program_name(token)) else {
            break;
        };

        let next = match program.as_str() {
            "sudo" | "doas" | "pkexec" | "gsudo" => {
                invocation_after_privilege_launcher(&current, &program)
            }
            "runas" | "runas.exe" => current
                .iter()
                .skip(1)
                .position(|arg| !arg.starts_with('/'))
                .map(|offset| nested_invocation(&current[offset + 1..])),
            "bash" | "sh" | "zsh" | "powershell" | "powershell.exe" | "pwsh" | "pwsh.exe" => {
                invocation_after_flag(&current, &["-c", "-command"])
            }
            "cmd" | "cmd.exe" => invocation_after_flag(&current, &["/c"]),
            "start-process" => start_process_invocation(&current),
            "eval" => (current.len() > 1).then(|| nested_invocation(&current[1..])),
            "env" => invocation_after_env(&current),
            "command" | "nohup" | "time" => invocation_after_simple_wrapper(&current, &program),
            _ => None,
        };

        match next {
            Some(next) if !next.is_empty() && next != current => current = next,
            _ => break,
        }
    }
    current
}

fn nested_invocation(tokens: &[String]) -> Vec<String> {
    if tokens.len() == 1 {
        command_tokens(&tokens[0])
    } else {
        tokens.to_vec()
    }
}

fn invocation_after_flag(tokens: &[String], flags: &[&str]) -> Option<Vec<String>> {
    let command_index = tokens.iter().enumerate().skip(1).find_map(|(index, arg)| {
        let exact = flags.iter().any(|flag| arg.eq_ignore_ascii_case(flag));
        let bundled_c = flags.contains(&"-c")
            && arg.starts_with('-')
            && !arg.starts_with("--")
            && arg.len() > 2
            && arg[1..].chars().all(|ch| ch.is_ascii_alphabetic())
            && arg[1..].chars().any(|ch| ch.eq_ignore_ascii_case(&'c'));
        (exact || bundled_c).then_some(index + 1)
    })?;
    (command_index < tokens.len()).then(|| nested_invocation(&tokens[command_index..]))
}

fn invocation_after_simple_wrapper(tokens: &[String], wrapper: &str) -> Option<Vec<String>> {
    let mut index = 1;
    while index < tokens.len() {
        let arg = &tokens[index];
        if arg == "--" {
            index += 1;
            break;
        }
        let time_option_with_value =
            wrapper == "time" && matches!(arg.as_str(), "-o" | "--output" | "-f" | "--format");
        if time_option_with_value {
            index += 2;
        } else if arg.starts_with('-') {
            index += 1;
        } else {
            break;
        }
    }
    (index < tokens.len()).then(|| nested_invocation(&tokens[index..]))
}

fn invocation_after_env(tokens: &[String]) -> Option<Vec<String>> {
    let mut index = 1;
    while index < tokens.len() {
        let arg = &tokens[index];
        if arg == "--" {
            index += 1;
            break;
        }
        if matches!(arg.as_str(), "-S" | "--split-string") {
            let value = tokens.get(index + 1)?;
            let mut invocation = command_tokens(value);
            invocation.extend_from_slice(&tokens[index + 2..]);
            return (!invocation.is_empty()).then_some(invocation);
        }
        if let Some(value) = arg.strip_prefix("--split-string=") {
            let mut invocation = command_tokens(value);
            invocation.extend_from_slice(&tokens[index + 1..]);
            return (!invocation.is_empty()).then_some(invocation);
        }
        let option_with_value = matches!(arg.as_str(), "-u" | "--unset" | "-C" | "--chdir");
        if option_with_value {
            index += 2;
        } else if arg.starts_with('-') || arg.contains('=') {
            index += 1;
        } else {
            break;
        }
    }
    (index < tokens.len()).then(|| nested_invocation(&tokens[index..]))
}

fn invocation_after_privilege_launcher(tokens: &[String], launcher: &str) -> Option<Vec<String>> {
    let options_with_values: &[&str] = match launcher {
        "sudo" => &[
            "-u",
            "--user",
            "-g",
            "--group",
            "-h",
            "--host",
            "-p",
            "--prompt",
            "-c",
            "--close-from",
            "-t",
            "--command-timeout",
            "-r",
            "--chroot",
            "-d",
            "--chdir",
        ],
        "doas" => &["-a", "-c", "-u"],
        "pkexec" => &["--user"],
        "gsudo" => &["-u", "--user", "-i", "--integrity"],
        _ => &[],
    };
    let mut index = 1;
    while index < tokens.len() {
        let arg = &tokens[index];
        if arg == "--" {
            index += 1;
            break;
        }
        if !arg.starts_with('-') {
            break;
        }
        let consumes_value = options_with_values
            .iter()
            .any(|option| arg.eq_ignore_ascii_case(option));
        index += if consumes_value { 2 } else { 1 };
    }
    (index < tokens.len()).then(|| nested_invocation(&tokens[index..]))
}

fn start_process_invocation(tokens: &[String]) -> Option<Vec<String>> {
    let mut file_path = None;
    let mut arguments = Vec::new();
    let mut index = 1;
    while index < tokens.len() {
        let arg = &tokens[index];
        if arg.eq_ignore_ascii_case("-filepath") && index + 1 < tokens.len() {
            file_path = Some(tokens[index + 1].clone());
            index += 2;
        } else if arg.eq_ignore_ascii_case("-argumentlist") && index + 1 < tokens.len() {
            arguments.extend(command_tokens(&tokens[index + 1]));
            index += 2;
        } else if matches!(
            arg.to_ascii_lowercase().as_str(),
            "-verb" | "-workingdirectory" | "-credential" | "-windowstyle"
        ) && index + 1 < tokens.len()
        {
            index += 2;
        } else if !arg.starts_with('-') && file_path.is_none() {
            file_path = Some(arg.clone());
            index += 1;
        } else {
            index += 1;
        }
    }
    file_path.map(|program| {
        let mut invocation = vec![program];
        invocation.extend(arguments);
        invocation
    })
}

/// Split a command line into simple-command segments so an `rm` after `;`,
/// `&&`, `||`, `|`, `&`, or a newline is still classified on its own.
fn command_segments(cmd: &str) -> Vec<&str> {
    let mut out = vec![cmd];
    for sep in [";", "\n", "&&", "||", "|", "&"] {
        out = out.into_iter().flat_map(|s| s.split(sep)).collect();
    }
    out
}

/// Strip shell quoting and escaping noise from a single token so equivalent
/// spellings collapse: `"/"`→`/`, `'/etc'`→`/etc`, `"$HOME"/`→`$HOME/`,
/// `\/`→`/`. Applied to `rm` *operands* only (not to command-name detection),
/// so ordinary strings like `git commit -m "rm -rf / fix"` — where the token is
/// `"rm`, which does not basename to `rm` — are never misread as a command.
fn unquote_unescape(tok: &str) -> String {
    tok.chars()
        .filter(|&c| c != '"' && c != '\'' && c != '\\')
        .collect()
}

/// Normalize a path operand so filesystem-equivalent spellings reduce to their
/// real target: collapse repeated and `.` slashes and resolve leading `..`, so
/// `//`, `/./`, `//etc`, `/./etc`, `/usr/..`, and `/tmp/..` classify correctly.
pub(super) fn normalize_path(op: &str) -> String {
    let leading = op.starts_with('/');
    let mut segs: Vec<&str> = Vec::new();
    for s in op.split('/') {
        match s {
            "" | "." => {}
            ".." if leading => {
                segs.pop();
            }
            _ => segs.push(s),
        }
    }
    if leading {
        format!("/{}", segs.join("/"))
    } else {
        segs.join("/")
    }
}

/// The home root or a glob that selects the whole home tree. Descendants are
/// intentionally not included: a consuming project commonly lives at
/// `~/code/app`, and its task-scoped build directory must behave the same as
/// the equivalent `/Users/alice/code/app` path.
fn is_home_root_operand(op: &str) -> bool {
    matches!(
        op,
        "~" | "~/" | "~/*" | "$HOME" | "$HOME/" | "$HOME/*" | "${HOME}" | "${HOME}/" | "${HOME}/*"
    )
}

/// Classify an `rm` operand as a protected target (root, root glob, a system
/// directory, or the home directory) — regardless of how the path is spelled.
/// De-quotes then normalizes so `rm -rf //`, `/./`, `//etc`, `/usr/..`, `"/"`,
/// and `"$HOME"/` are all caught, not only the literal `/` / `/etc` forms.
///
/// Scope: this guards against *accidental* destructive commands (defense in
/// depth). It recognizes ordinary command paths and direct `sh -c`/`eval`
/// quoting without stripping quotes from an entire line, which would
/// false-positive on ordinary `echo` and commit-message strings.
pub(super) fn dangerous_rm_target(raw: &str) -> Option<&'static str> {
    let op = unquote_unescape(raw);
    let op = op.as_str();
    if is_home_root_operand(op) {
        return Some("home directory");
    }
    let tmpdir = std::env::var_os("TMPDIR").map(PathBuf::from);
    let place = if has_known_identity(raw) {
        temp_place(op, tmpdir.as_deref())
    } else {
        TempPlace::Outside(None)
    };
    match place {
        TempPlace::Root => Some("temp root"),
        TempPlace::Below => None,
        TempPlace::Outside(canonical) => {
            let norm = normalize_path(op);
            let reads_as_temp = temp_root_depth(&norm).is_some();
            let Some(canonical) = canonical else {
                // Temp space whose target cannot be established is refused
                // through every spelling, not left to the system-prefix
                // rule, which `/tmp` does not match.
                return protected_path(&norm)
                    .or_else(|| reads_as_temp.then_some("unresolved temp path"));
            };
            // A path that reads as temp space but leads elsewhere (a link
            // under `/tmp`) is judged where it lands. Other paths keep their
            // lexical reading: `/home` resolves below `/System` on macOS.
            protected_path(&norm)
                .or_else(|| reads_as_temp.then(|| protected_path(&canonical)).flatten())
        }
    }
}

/// Whether the operand token is the path the shell passes. The tokenizer
/// has already removed the quoting it models, so a backslash, quote,
/// expansion or substitution left in the token is something this
/// classifier cannot resolve: a literal backslash in a single-quoted name,
/// for one, would otherwise be read as a different, absent path.
fn has_known_identity(raw: &str) -> bool {
    !raw.contains(['\\', '$', '`', '\'', '"'])
}

/// The names directly below `dir` whose deletion is refused whole, so a
/// glob the disk does not resolve is still matched against them.
pub(super) fn protected_names(dir: &str) -> Vec<&'static str> {
    match dir {
        "/" => SYSTEM_DIRS
            .iter()
            .chain(ROOT_COLLECTION_DIRS)
            .chain(&["tmp"])
            .copied()
            .collect(),
        "/private" => vec!["tmp", "var"],
        "/var" | "/private/var" => vec!["tmp"],
        _ => Vec::new(),
    }
}

/// Classify a normalized absolute path as the root, a system directory or a
/// whole top-level user or mount collection.
fn protected_path(norm: &str) -> Option<&'static str> {
    if norm == "/" || norm == "/*" {
        return Some("/");
    }
    let rest = norm.strip_prefix('/')?;
    let mut parts = rest.split('/');
    let first = parts.next().unwrap_or("");
    if SYSTEM_DIRS.contains(&first) {
        return Some("system directory");
    }
    let rest: Vec<&str> = parts.collect();
    let remainder = rest.join("/");
    if ROOT_COLLECTION_DIRS.contains(&first) && (remainder.is_empty() || remainder == "*") {
        return Some("top-level user or mount collection");
    }
    // A home directory spelled as its literal path (`/Users/<name>`,
    // `/home/<name>`), or a glob over its whole tree, is the home directory
    // (TSK-141). Its descendants stay project-scoped.
    let whole_home = matches!(rest.as_slice(), [name] | [name, "*"] if *name != "*");
    if HOME_COLLECTION_DIRS.contains(&first) && whole_home {
        return Some("home directory");
    }
    None
}

/// The shared temp roots in canonical form: macOS keeps them below
/// `/private`, where `/tmp` and `/var/tmp` are symlinks.
const TEMP_ROOTS: &[&str] = &["/tmp", "/var/tmp", "/private/tmp", "/private/var/tmp"];

/// Where an operand lands relative to the temp roots (TSK-137).
#[derive(Debug, PartialEq, Eq)]
enum TempPlace {
    /// A temp root itself, or a glob over one: always protected.
    Root,
    /// Canonically below a temp root: an agent's scratch space, exempt.
    Below,
    /// Not established below a temp root; carries the canonical path when
    /// it resolved, so a symlinked system target is still classified.
    Outside(Option<String>),
}

/// Place an operand relative to the temp roots by where it really lands.
///
/// Containment is canonical, never lexical: the longest existing prefix is
/// canonicalized (following symlinks) and the rest appended; a `..` in that
/// rest, a failed canonicalization or a component that exists but does not
/// resolve (a dangling link) establishes nothing. On native Windows, where
/// these paths name no fixed place, nothing is established (`canonical_operand`). A glob is judged by its
/// text before the first glob character, and only when the glob is in the
/// last component, since a match in the middle may itself be a link.
///
/// The roots are the fixed [`TEMP_ROOTS`] and the macOS per-user
/// `/private/var/folders/<xx>/<id>/T`. `$TMPDIR` adds no root: it is honoured
/// only when it canonically is one of those or lies below one, and then only
/// to protect the directory itself.
fn temp_place(op: &str, tmpdir: Option<&Path>) -> TempPlace {
    if !op.starts_with('/') {
        return TempPlace::Outside(None);
    }
    let judged = match op.find(['*', '?', '[']) {
        Some(index) if op[index..].contains('/') => return TempPlace::Outside(None),
        Some(index) => &op[..index],
        None => op,
    };
    let Some(canonical) = canonical_operand(judged) else {
        return TempPlace::Outside(None);
    };
    let tmpdir = tmpdir
        .and_then(|dir| std::fs::canonicalize(dir).ok())
        // A temp folder that is not valid UTF-8 cannot be compared as text,
        // so it is not a root (OS text rule, issue 79).
        .and_then(|dir| dir.to_str().map(str::to_string))
        .filter(|dir| temp_root_depth(dir).is_some());
    // `/tmp/*` is judged as `/tmp/`, so a glob over a whole root is the root.
    match temp_root_depth(&canonical) {
        Some(0) => TempPlace::Root,
        _ if tmpdir.as_deref() == Some(canonical.as_str()) => TempPlace::Root,
        Some(_) => TempPlace::Below,
        None => TempPlace::Outside(Some(canonical)),
    }
}

/// How many components a canonical path lies below a temp root: `Some(0)`
/// for a root itself, `None` outside every root.
fn temp_root_depth(canonical: &str) -> Option<usize> {
    let below = |root: &str| {
        let rest = canonical.strip_prefix(root)?;
        if rest.is_empty() {
            return Some(0);
        }
        rest.strip_prefix('/')
            .map(|rest| rest.split('/').filter(|part| !part.is_empty()).count())
    };
    if let Some(depth) = TEMP_ROOTS.iter().find_map(|root| below(root)) {
        return Some(depth);
    }
    let parts: Vec<&str> = canonical.split('/').skip(1).collect();
    let name = |part: &str, len: Option<usize>| {
        !part.is_empty()
            && len.is_none_or(|len| part.len() == len)
            && part
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '+')
    };
    match parts.as_slice() {
        ["private", "var", "folders", bucket, id, "T", rest @ ..]
            if name(bucket, Some(2)) && name(id, None) =>
        {
            Some(rest.iter().filter(|part| !part.is_empty()).count())
        }
        _ => None,
    }
}

/// On native Windows a path that starts with `/` names no fixed place: Git
/// Bash reads it below its own install root (`/tmp` is the user's temp
/// folder) and other shells below the current drive, and neither is the
/// path this process would resolve. A junction can also redirect any part of
/// it. Its containment cannot be established, so nothing below a Unix temp
/// root is exempt there and such a deletion is refused as an unresolved
/// temp path.
#[cfg(windows)]
fn canonical_operand(_path: &str) -> Option<String> {
    None
}

/// Canonicalize the longest existing prefix of an absolute path and append
/// the rest, or `None` when that cannot be established.
#[cfg(not(windows))]
fn canonical_operand(path: &str) -> Option<String> {
    let parts: Vec<&str> = path
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect();
    for existing in (0..=parts.len()).rev() {
        let prefix = format!("/{}", parts[..existing].join("/"));
        match std::fs::canonicalize(&prefix) {
            Ok(base) => {
                let rest = &parts[existing..];
                if rest.contains(&"..") {
                    return None;
                }
                // A real path that is not valid UTF-8 cannot be judged as text
                // (OS text rule, issue 79), so it is not established.
                let mut canonical = base.to_str()?.to_string();
                for part in rest {
                    if !canonical.ends_with('/') {
                        canonical.push('/');
                    }
                    canonical.push_str(part);
                }
                return Some(canonical);
            }
            Err(_)
                if crate::absence::proven_absent(Path::new(&prefix)).is_ok_and(|absent| absent) => {
            }
            Err(_) => return None,
        }
    }
    None
}

/// Classify Windows drive, system, profile, and share roots. Comparisons are
/// case-insensitive and accept either separator because `PowerShell` accepts
/// both. A descendant project under a user profile remains task-scoped; only
/// the profile itself (or its whole-tree glob) is protected here.
fn dangerous_windows_target(op: &str) -> Option<&'static str> {
    let normalized = op
        .trim_matches([',', ';'])
        .replace('/', "\\")
        .to_ascii_lowercase();
    let path = normalized.strip_suffix("\\*").unwrap_or(&normalized);
    let path = if path.len() > 3 {
        path.trim_end_matches('\\')
    } else {
        path
    };

    if matches!(
        path,
        "$env:systemroot" | "$env:windir" | "%systemroot%" | "%windir%"
    ) || path.starts_with("$env:systemroot\\")
        || path.starts_with("$env:windir\\")
        || path.starts_with("%systemroot%\\")
        || path.starts_with("%windir%\\")
    {
        return Some("Windows system directory");
    }
    if matches!(
        path,
        "~" | "$home" | "$env:userprofile" | "%userprofile%" | "%homedrive%%homepath%"
    ) {
        return Some("Windows user profile");
    }

    let bytes = path.as_bytes();
    if bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\' {
        let rest = &path[3..];
        if rest.is_empty() {
            return Some("Windows drive root");
        }
        let parts: Vec<&str> = rest.split('\\').filter(|part| !part.is_empty()).collect();
        if matches!(
            parts.first().copied(),
            Some("windows" | "program files" | "program files (x86)" | "programdata")
        ) {
            return Some("Windows system directory");
        }
        if parts.first().copied() == Some("users") && parts.len() <= 2 {
            return Some("Windows user collection or profile");
        }
    }

    // A UNC share root (`\\server\share`) is a cross-machine blast radius;
    // descendants can still be a legitimate task-scoped workspace.
    if let Some(rest) = path.strip_prefix("\\\\") {
        let parts: Vec<&str> = rest.split('\\').filter(|part| !part.is_empty()).collect();
        if parts.len() == 2 {
            return Some("Windows network share root");
        }
    }
    None
}

fn is_windows_drive_designator(op: &str) -> bool {
    let op = op.trim_matches([',', ';']);
    let bytes = op.as_bytes();
    bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

fn dd_disk_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"dd\s.*of=/dev/((sd|hd|vd)[a-z]|nvme[0-9]|(r?disk)[0-9])").expect("valid")
    })
}

fn format_cmd_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(mkfs|mke2fs|mkswap|newfs_[a-zA-Z0-9_]+)\s").expect("valid"))
}

fn chmod_777_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"chmod\s.*777\s+/(etc|var|usr|bin|sbin|boot|lib|opt|root)(\s|$|/)|chmod\s.*777\s+/(\s|$)",
        )
        .expect("valid")
    })
}

fn chmod_r_root_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"chmod\s+-R\s.*\s/($|\s)").expect("valid"))
}

fn chown_r_root_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"chown\s+-R\s.*\s/($|\s)").expect("valid"))
}

fn fork_bomb_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r":\(\)\s*\{.*\|.*:\s*&\s*\}\s*;\s*:").expect("valid"))
}

fn fork_bomb_dot_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\.\(\)\s*\{.*\|.*\.\s*&\s*\}\s*;\s*\.").expect("valid"))
}

pub struct DangerousModule;

impl SecurityModule for DangerousModule {
    fn name(&self) -> &'static str {
        "dangerous-commands"
    }

    fn check(&self, ctx: &CheckContext<'_>) -> Option<Verdict> {
        let cmd = ctx.command;

        // Substring pattern checks.
        for &pattern in DANGEROUS_SUBSTRINGS {
            if cmd.contains(pattern) {
                return Some(block(
                    "Dangerous Command",
                    "Destructive operation detected",
                    pattern,
                ));
            }
        }

        // Recursive deletion checks.
        if let Some(v) = check_recursive_delete(cmd) {
            return Some(v);
        }
        if let Some(found) = super::deletion::composed_deletion(cmd) {
            return Some(composed_verdict(&found));
        }

        // Disk operation checks.
        if let Some(v) = check_disk_operations(cmd) {
            return Some(v);
        }

        // Permission change checks.
        if let Some(v) = check_permission_changes(cmd) {
            return Some(v);
        }

        // Fork bomb checks.
        if let Some(v) = check_fork_bomb(cmd) {
            return Some(v);
        }

        None
    }
}

/// The refusal for a deletion the composed reader finds. A target reached
/// on only some paths through the line is still refused (TSK-141 round
/// 1), and one the reader cannot prove is refused as unproven (round 2);
/// each message says why.
fn composed_verdict(found: &super::deletion::Found) -> Verdict {
    let reason = match &found.unproven {
        Some(unproven) if unproven.cwd => format!(
            "Recursive deletion whose target cannot be proven: it runs in a working \
             directory that depends on {}, which the guard does not follow exactly; \
             name the project path literally, for example `rm -rf ./build` after a \
             literal `cd` to the project",
            unproven.reason
        ),
        Some(unproven) => format!(
            "Recursive deletion whose target cannot be proven: it depends on {}, which \
             the guard does not follow exactly; name the project path literally, for \
             example `rm -rf ./build`",
            unproven.reason
        ),
        None if found.ambiguous => "Recursive deletion of a protected location: it is one \
             of several values the command may reach here (after a branch, loop, \
             subshell or earlier value), and any of them is refused; name the target \
             directly"
            .to_string(),
        None => "Recursive deletion of a protected location".to_string(),
    };
    block("Dangerous Command", &reason, found.target)
}

/// Block recursive `rm` of a protected location, however the flags and operand
/// are spelled. Tokenizes each simple command instead of pattern-matching the
/// raw string, so `rm -r -f /`, `rm --recursive --force /`, `rm -rf -- /`, and
/// `rm -rf "$HOME"` are all caught, not only the exact `rm -rf` form.
fn check_recursive_delete(cmd: &str) -> Option<Verdict> {
    for seg in command_segments(cmd) {
        let tokens = effective_invocation(&command_tokens(seg));
        if tokens.first().map(|token| program_name(token)).as_deref() != Some("rm") {
            continue;
        }
        let Some(operands) = recursive_rm_operands(&tokens[1..]) else {
            continue;
        };
        for op in operands {
            if let Some(target) = dangerous_rm_target(op) {
                return Some(block(
                    "Dangerous Command",
                    "Recursive deletion of a protected location",
                    target,
                ));
            }
        }
    }
    check_windows_recursive_delete(cmd)
}

/// The operands of an `rm` whose arguments are `args`, when it is recursive;
/// `None` when it is not.
pub(super) fn recursive_rm_operands(args: &[String]) -> Option<Vec<&str>> {
    let mut recursive = false;
    let mut operands: Vec<&str> = Vec::new();
    let mut operands_only = false; // everything after a lone `--`
    for a in args.iter().map(String::as_str) {
        if operands_only {
            operands.push(a);
            continue;
        }
        if a == "--" {
            operands_only = true;
        } else if let Some(long) = a.strip_prefix("--") {
            if matches!(long, "recursive" | "dir") {
                recursive = true;
            }
        } else if let Some(short) = a.strip_prefix('-') {
            if !short.is_empty() && short.chars().all(|c| c.is_ascii_alphabetic()) {
                if short.contains('r') || short.contains('R') {
                    recursive = true;
                }
            } else {
                operands.push(a); // not a clean flag bundle → treat as operand
            }
        } else {
            operands.push(a);
        }
    }
    recursive.then_some(operands)
}

fn check_windows_recursive_delete(cmd: &str) -> Option<Verdict> {
    for segment in command_segments(cmd) {
        let tokens = effective_invocation(&command_tokens(segment));
        // PowerShell can carry a destructive cmdlet after a launcher such as
        // `Start-Process ... -ArgumentList`, so this intentionally scans the
        // invocation rather than requiring the cmdlet to be token zero. That
        // fail-safe choice is narrower than raw substring matching: quoted
        // prose remains one non-command token, while an unquoted example may
        // be blocked and should be quoted by the caller.
        let Some(command_index) = tokens.iter().position(|token| {
            matches!(
                token
                    .rsplit(['/', '\\'])
                    .next()
                    .unwrap_or(token)
                    .to_ascii_lowercase()
                    .as_str(),
                "remove-item" | "rm" | "del" | "erase" | "rd" | "rmdir"
            )
        }) else {
            continue;
        };
        let args = &tokens[command_index + 1..];
        let recursive = args.iter().any(|arg| {
            let arg = arg.to_ascii_lowercase();
            arg == "-recurse"
                || arg.starts_with("-recurse:")
                || arg == "-r"
                || arg == "/s"
                || (arg.starts_with('-') && arg[1..].contains('r'))
        });
        if !recursive {
            continue;
        }
        if let Some(target) = args.iter().find_map(|arg| dangerous_windows_target(arg)) {
            return Some(block(
                "Dangerous Command",
                "Recursive deletion of a protected Windows location",
                target,
            ));
        }
    }
    None
}

fn check_disk_operations(cmd: &str) -> Option<Verdict> {
    for segment in command_segments(cmd) {
        let tokens = effective_invocation(&command_tokens(segment));
        let Some(program) = tokens.first().map(|token| program_name(token)) else {
            continue;
        };
        let normalized = tokens.join(" ");
        let args = tokens
            .iter()
            .skip(1)
            .map(|arg| arg.to_ascii_lowercase())
            .collect::<Vec<_>>();

        if program == "dd" && dd_disk_re().is_match(&normalized) {
            return Some(block(
                "Dangerous Command",
                "Direct disk write operation",
                "dd of=/dev/*",
            ));
        }
        if format_cmd_re().is_match(&format!("{normalized} ")) {
            return Some(block(
                "Dangerous Command",
                "Disk format operation",
                "mkfs/mke2fs/mkswap/newfs",
            ));
        }

        let destructive_unix = (program == "diskutil"
            && (args.first().is_some_and(|arg| {
                matches!(
                    arg.as_str(),
                    "erasedisk"
                        | "erasevolume"
                        | "partitiondisk"
                        | "zerodisk"
                        | "randomdisk"
                        | "secureerase"
                )
            }) || (args.first().is_some_and(|arg| arg == "apfs")
                && args.get(1).is_some_and(|arg| {
                    matches!(arg.as_str(), "deletecontainer" | "deletevolume")
                }))))
            || matches!(
                program.as_str(),
                "wipefs" | "blkdiscard" | "pvremove" | "vgremove" | "lvremove"
            )
            || (matches!(program.as_str(), "zpool" | "zfs")
                && args.first().is_some_and(|arg| arg == "destroy"))
            || (program == "cryptsetup"
                && args
                    .first()
                    .is_some_and(|arg| matches!(arg.as_str(), "luksformat" | "erase")))
            || (program == "sgdisk"
                && args
                    .iter()
                    .any(|arg| matches!(arg.as_str(), "--zap-all" | "-z" | "--clear" | "-o")))
            || (program == "parted"
                && args
                    .iter()
                    .any(|arg| matches!(arg.as_str(), "rm" | "mklabel" | "mkpart" | "resizepart")))
            || (program == "sfdisk"
                && args
                    .iter()
                    .any(|arg| arg == "--delete" || arg.starts_with("--wipe")))
            || (program == "mdadm" && args.iter().any(|arg| arg == "--zero-superblock"))
            || (program == "shred" && tokens.iter().skip(1).any(|arg| arg.starts_with("/dev/")));
        if destructive_unix {
            return Some(block(
                "Dangerous Command",
                "Destructive disk-management operation",
                &program,
            ));
        }

        if matches!(
            program.as_str(),
            "diskpart"
                | "clear-disk"
                | "initialize-disk"
                | "format-volume"
                | "remove-partition"
                | "remove-virtualdisk"
                | "disable-bitlocker"
        ) || (program == "format"
            && tokens
                .iter()
                .skip(1)
                .any(|arg| is_windows_drive_designator(arg)))
            || (program == "manage-bde" && args.iter().any(|arg| arg == "-off"))
            || (program == "vssadmin"
                && args.iter().any(|arg| arg == "delete")
                && args.iter().any(|arg| arg == "shadows"))
            || (program == "wbadmin"
                && args.iter().any(|arg| arg == "delete")
                && args
                    .iter()
                    .any(|arg| matches!(arg.as_str(), "catalog" | "systemstatebackup")))
            || (program == "wmic"
                && args.iter().any(|arg| arg == "shadowcopy")
                && args.iter().any(|arg| arg == "delete"))
        {
            return Some(block(
                "Dangerous Command",
                "Destructive Windows disk or recovery operation",
                &program,
            ));
        }
    }
    None
}

fn check_permission_changes(cmd: &str) -> Option<Verdict> {
    if chmod_777_re().is_match(cmd) {
        return Some(block(
            "Dangerous Command",
            "Dangerous permission change on system path",
            "chmod 777 /system-path",
        ));
    }
    if chmod_r_root_re().is_match(cmd) {
        return Some(block(
            "Dangerous Command",
            "Recursive permission change on root",
            "chmod -R /",
        ));
    }
    if chown_r_root_re().is_match(cmd) {
        return Some(block(
            "Dangerous Command",
            "Recursive ownership change on root",
            "chown -R /",
        ));
    }
    for segment in command_segments(cmd) {
        let tokens = effective_invocation(&command_tokens(segment));
        let Some(program) = tokens.first().map(|token| program_name(token)) else {
            continue;
        };
        let args = &tokens[1..];
        let unix_recursive = args.iter().any(|arg| {
            arg == "-R"
                || arg == "--recursive"
                || (arg.starts_with('-')
                    && !arg.starts_with("--")
                    && arg[1..].chars().any(|ch| ch == 'R'))
        });
        if matches!(program.as_str(), "chmod" | "chown" | "chgrp") && unix_recursive {
            if let Some(target) = args.iter().find_map(|arg| dangerous_rm_target(arg)) {
                return Some(block(
                    "Dangerous Command",
                    "Recursive permission or ownership change on protected path",
                    target,
                ));
            }
        }

        let windows_modifier = match program.as_str() {
            "takeown" => args.iter().any(|arg| arg.eq_ignore_ascii_case("/r")),
            "icacls" => args.iter().any(|arg| {
                let arg = arg.to_ascii_lowercase();
                ["/grant", "/deny", "/remove", "/reset", "/inheritance"]
                    .iter()
                    .any(|prefix| arg.starts_with(prefix))
            }),
            "set-acl" => true,
            _ => false,
        };
        if windows_modifier {
            if let Some(target) = args.iter().find_map(|arg| dangerous_windows_target(arg)) {
                return Some(block(
                    "Dangerous Command",
                    "Windows permission or ownership change on protected path",
                    target,
                ));
            }
        }
    }
    None
}

fn check_fork_bomb(cmd: &str) -> Option<Verdict> {
    if fork_bomb_re().is_match(cmd) {
        return Some(block(
            "Dangerous Command",
            "Fork bomb detected",
            ":(){:|:&};:",
        ));
    }
    if fork_bomb_dot_re().is_match(cmd) {
        return Some(block(
            "Dangerous Command",
            "Fork bomb variation detected",
            ".(){.|.&};.",
        ));
    }
    None
}

#[cfg(test)]
mod tests {

    #[test]
    fn unicode_blanks_stay_in_dangerous_operands() {
        assert_eq!(
            command_tokens("Remove-Item\u{a0}-Recurse C:\\Windows"),
            ["Remove-Item", "-Recurse", "C:\\Windows"]
        );
        assert_eq!(dangerous_rm_target("'/etc\u{a0}'"), None);
        assert_eq!(dangerous_rm_target("'/etc '"), None);
        assert!(dangerous_rm_target("/etc").is_some());
        assert!(check_windows_recursive_delete("Remove-Item\u{a0}-Recurse C:\\Windows").is_some());
    }
    use super::*;
    use std::sync::OnceLock;

    use crate::security::SecurityPolicy;

    fn test_policy() -> &'static SecurityPolicy {
        static POLICY: OnceLock<SecurityPolicy> = OnceLock::new();
        POLICY.get_or_init(SecurityPolicy::defaults)
    }

    fn ctx(cmd: &str) -> CheckContext<'_> {
        CheckContext {
            command: cmd,
            sandbox_bypass: false,
            current_branch: "feat/test",
            policy: test_policy(),
        }
    }

    #[test]
    fn test_safe_command() {
        assert!(DangerousModule.check(&ctx("ls -la")).is_none());
    }

    #[test]
    fn test_rm_rf_root() {
        let v = DangerousModule.check(&ctx("rm -rf /")).unwrap();
        assert!(!v.allow);
        assert_eq!(v.category, "Dangerous Command");
    }

    #[test]
    fn test_rm_rf_root_star() {
        assert!(DangerousModule.check(&ctx("rm -rf /*")).is_some());
    }

    #[test]
    fn test_rm_fr_root() {
        assert!(DangerousModule.check(&ctx("rm -fr /")).is_some());
    }

    #[test]
    fn test_rm_r_home() {
        for cmd in [
            "rm -r ~",
            "rm -rf ~/",
            "rm -rf ~/*",
            "rm -rf $HOME",
            "rm -rf $HOME/*",
            "rm -rf ${HOME}/*",
        ] {
            assert!(
                DangerousModule.check(&ctx(cmd)).is_some(),
                "should block the whole home tree: {cmd}"
            );
        }
    }

    #[test]
    fn test_rm_r_system_dir() {
        assert!(DangerousModule.check(&ctx("rm -rf /etc")).is_some());
        assert!(DangerousModule.check(&ctx("rm -r /usr/")).is_some());
        assert!(DangerousModule.check(&ctx("rm -rf /var")).is_some());
    }

    #[test]
    fn test_rm_r_macos_system_roots() {
        for cmd in [
            "rm -rf /System",
            "rm -rf /Library/Preferences",
            "rm -rf /Applications",
            "rm -rf /private/etc",
            "rm -rf /Users",
            "rm -rf /Users/*",
            "rm -rf /Volumes",
            "rm -rf /Volumes/*",
        ] {
            assert!(
                DangerousModule.check(&ctx(cmd)).is_some(),
                "should block: {cmd}"
            );
        }
    }

    #[test]
    fn test_root_collection_descendant_can_be_project_scoped() {
        for cmd in [
            "rm -rf /Users/alice/project/target",
            "rm -rf /home/alice/project/target",
            "rm -rf /Volumes/Data/project/target",
        ] {
            // On native Windows a `/`-rooted path names no fixed place, so
            // the deletion is refused there.
            assert_eq!(
                DangerousModule.check(&ctx(cmd)).is_some(),
                cfg!(windows),
                "project-scoped descendant should reach the harness boundary: {cmd}"
            );
        }
    }

    #[test]
    fn test_windows_recursive_delete_protected_roots() {
        for cmd in [
            r"Remove-Item -Recurse -Force C:\",
            r#"Remove-Item "C:\Windows\System32" -Recurse"#,
            r#"rm -r "C:\Program Files""#,
            r"rd /s /q C:\Users",
            r"rmdir /s \\server\share",
            r"Remove-Item -Recurse $env:SystemRoot",
            r"Remove-Item $env:USERPROFILE -Recurse",
            r"Remove-Item -Recurse ~",
            r"Remove-Item -Recurse $HOME",
            r#"powershell -Command "Remove-Item -Recurse C:\Windows""#,
        ] {
            assert!(
                DangerousModule.check(&ctx(cmd)).is_some(),
                "should block: {cmd}"
            );
        }
    }

    #[test]
    fn test_windows_project_scoped_delete_allowed() {
        for cmd in [
            r"Remove-Item -Recurse .\target",
            r"Remove-Item -Recurse C:\Users\alice\project\target",
            r"rmdir /s /q D:\work\project\node_modules",
            r"Remove-Item C:\Windows\Temp\one.log", // not recursive
        ] {
            assert!(
                DangerousModule.check(&ctx(cmd)).is_none(),
                "task-scoped delete should remain available: {cmd}"
            );
        }
    }

    // Regression: flag re-spellings that the old regex classifier missed
    // (codex pre-flip review — exec-guard bypass).
    #[test]
    fn test_rm_flag_respellings_blocked() {
        for cmd in [
            "rm -r -f /",
            "rm -f -r /",
            "rm --recursive --force /",
            "rm --recursive /",
            "rm -rf -- /",
            "rm -r -- /etc",
            "rm -R /usr",
            "rm -rf \"$HOME\"",
            "rm -rf $HOME",
            "rm -rf ${HOME}",
            "rm -rf $HOME/",
            "rm -rf --no-preserve-root /",
            "true && rm -r -f /",
            "sudo rm -rf --force /var",
        ] {
            assert!(
                DangerousModule.check(&ctx(cmd)).is_some(),
                "should block: {cmd}"
            );
        }
    }

    // Regression: path-equivalence spellings that reduce to a protected target
    // (codex round-2 review — normalization bypass). `//`, `/./`, and `..` are
    // filesystem-equivalent to root / a system dir and must still be blocked.
    #[test]
    fn test_rm_path_equivalence_blocked() {
        for cmd in [
            "rm -rf //",
            "rm -rf /./",
            "rm -rf ///",
            "rm -rf //etc",
            "rm -rf /./etc",
            "rm -rf /usr/..", // == /
            "rm -rf /tmp/..", // == /
            "rm -rf /var/../etc",
            "rm -rf \"/\"",
            "rm -rf '/'",
            "rm -rf \"$HOME\"/",
            "rm -rf //*",
        ] {
            assert!(
                DangerousModule.check(&ctx(cmd)).is_some(),
                "should block: {cmd}"
            );
        }
    }

    // TSK-141 AC-1 (amended): this row was allowed, commented "== /home/user,
    // not protected". It resolves to a home directory, so it is refused as
    // its `rm -rf /home/user` equivalent is.
    #[test]
    fn test_rm_home_directory_through_dot_dot_blocked() {
        let form = DangerousModule
            .check(&ctx("rm -rf /home/user/proj/.."))
            .expect("refused");
        let equivalent = DangerousModule
            .check(&ctx("rm -rf /home/user"))
            .expect("refused");
        assert_eq!(form.pattern, equivalent.pattern);
        assert_eq!(form.pattern, "home directory");
    }

    // Recursive rm of a non-protected path stays allowed (no false positives).
    #[test]
    fn test_rm_recursive_safe_paths_allowed() {
        for cmd in [
            "rm -rf ./build",
            "rm -rf target",
            "rm -rf node_modules",
            "rm -f /etc/hosts.bak", // not recursive
            "rm -rf ./scratch/..",  // relative, not protected
            "rm -rf ~/code/app/target",
            "rm -rf $HOME/code/app/build",
            "rm -rf ${HOME}/work/project/node_modules",
        ] {
            assert!(
                DangerousModule.check(&ctx(cmd)).is_none(),
                "should allow: {cmd}"
            );
        }
        // `/home/user/proj`, a project; on native Windows a `/`-rooted path
        // names no fixed place, so the deletion is refused there.
        let verdict = DangerousModule.check(&ctx("rm -rf /home/user/proj/target/.."));
        assert_eq!(verdict.is_some(), cfg!(windows), "{verdict:?}");
    }

    // Over-block guard: when the quote attaches to the `rm` token itself (the
    // common case for a dangerous string inside a message/argument), the raw
    // token is `"rm` / `'rm`, which does not basename to `rm`, so it is never
    // misread as a command and stays allowed.
    #[test]
    fn test_rm_quoted_strings_not_misread_as_command() {
        for cmd in [
            "git commit -m \"rm -rf / fix\"",
            "echo \"rm -rf /\"",
            "grep -r 'rm -rf /' .",
        ] {
            assert!(
                DangerousModule.check(&ctx(cmd)).is_none(),
                "should allow (not a real rm): {cmd}"
            );
        }
    }

    // A quoted example is data, not a command. Wrapper resolution starts from
    // the actual program so routine documentation and diagnostics stay usable.
    #[test]
    fn test_bare_rm_in_string_is_not_misread_as_command() {
        assert!(DangerousModule
            .check(&ctx("echo 'do not run rm -rf /'"))
            .is_none());
    }

    #[test]
    fn test_windows_delete_examples_require_quoting() {
        for cmd in [
            r#"echo "Remove-Item -Recurse C:\Windows""#,
            r#"git commit -m "Remove-Item -Recurse C:\Windows""#,
        ] {
            assert!(
                DangerousModule.check(&ctx(cmd)).is_none(),
                "quoted Windows example should stay data: {cmd}"
            );
        }

        assert!(DangerousModule
            .check(&ctx(r"echo Remove-Item -Recurse C:\Windows"))
            .is_some());
    }

    #[test]
    fn test_dd_dev_zero() {
        assert!(DangerousModule
            .check(&ctx("dd if=/dev/zero of=file"))
            .is_some());
    }

    #[test]
    fn test_dd_disk() {
        assert!(DangerousModule
            .check(&ctx("dd if=image of=/dev/sda"))
            .is_some());
        assert!(DangerousModule
            .check(&ctx("dd if=image of=/dev/disk0"))
            .is_some());
    }

    #[test]
    fn test_mkfs() {
        assert!(DangerousModule.check(&ctx("mkfs.ext4 /dev/sda1")).is_some());
        assert!(DangerousModule
            .check(&ctx("newfs_apfs /dev/disk9s1"))
            .is_some());
    }

    #[test]
    fn test_destructive_disk_management() {
        for cmd in [
            "diskutil eraseDisk APFS Test /dev/disk9",
            "/usr/sbin/diskutil eraseVolume APFS Test /dev/disk9s1",
            "diskutil apfs deleteContainer /dev/disk9",
            "sudo diskutil eraseDisk APFS Test /dev/disk9",
            "wipefs --all /dev/sda",
            "sudo wipefs --all /dev/sda",
            "bash -lc 'wipefs --all /dev/sda'",
            "env LC_ALL=C wipefs --all /dev/sda",
            "blkdiscard /dev/nvme0n1",
            "sgdisk --zap-all /dev/sda",
            "parted /dev/sda mklabel gpt",
            "sfdisk --delete /dev/sda",
            "mdadm --zero-superblock /dev/sda1",
            "shred /dev/sda",
        ] {
            assert!(
                DangerousModule.check(&ctx(cmd)).is_some(),
                "should block: {cmd}"
            );
        }
        for cmd in ["diskutil list", "diskutil verifyDisk /dev/disk9"] {
            assert!(
                DangerousModule.check(&ctx(cmd)).is_none(),
                "read-only disk inspection should remain available: {cmd}"
            );
        }
    }

    #[test]
    fn test_windows_destructive_disk_and_recovery_operations() {
        for cmd in [
            "diskpart /s destructive.txt",
            "gsudo diskpart /s destructive.txt",
            "runas /user:Administrator \"diskpart /s destructive.txt\"",
            "Start-Process diskpart -Verb RunAs -ArgumentList '/s destructive.txt'",
            "Clear-Disk -Number 0 -RemoveData",
            "powershell -Command \"Clear-Disk -Number 0 -RemoveData\"",
            "Start-Process powershell -Verb RunAs -ArgumentList '-Command Clear-Disk -Number 0 -RemoveData'",
            "Initialize-Disk -Number 0",
            "Format-Volume -DriveLetter C",
            "format C:",
            "Disable-BitLocker -MountPoint C:",
            "manage-bde -off C:",
            "vssadmin delete shadows /all",
            "wbadmin delete catalog -quiet",
            "wmic shadowcopy delete",
        ] {
            assert!(
                DangerousModule.check(&ctx(cmd)).is_some(),
                "should block: {cmd}"
            );
        }
    }

    #[test]
    fn test_windows_recursive_permission_changes() {
        for cmd in [
            r"takeown /f C:\Windows /r",
            r"gsudo takeown /f C:\Windows /r",
            r#"icacls "C:\Program Files" /reset /t"#,
            r#"icacls "C:\Windows" /grant:r Everyone:F /t"#,
            r#"cmd /c "icacls C:\Windows /reset /t""#,
            r"Set-Acl -Path C:\Windows -AclObject $acl",
        ] {
            assert!(
                DangerousModule.check(&ctx(cmd)).is_some(),
                "should block: {cmd}"
            );
        }
        assert!(DangerousModule
            .check(&ctx(r"icacls .\build /reset /t"))
            .is_none());
    }

    #[test]
    fn test_shell_wrapped_catastrophic_rm_blocked() {
        for cmd in [
            "bash -c 'rm -rf /System'",
            "bash -lc 'rm -rf /System'",
            "sh -c \"rm -rf /Library\"",
            "sh -ic \"rm -rf /Library\"",
            "zsh -c 'rm -rf /'",
            "eval 'rm -rf /Applications'",
            "env CLEAN=1 rm -rf /Applications",
            "env -u CLEAN rm -rf /Applications",
            "env --chdir /tmp rm -rf /System",
            "env -S 'rm -rf /Library'",
            "command rm -rf /System",
            "time -o timing.txt rm -rf /System",
            "time --format elapsed rm -rf /Applications",
        ] {
            assert!(
                DangerousModule.check(&ctx(cmd)).is_some(),
                "should block wrapped command: {cmd}"
            );
        }
    }

    #[test]
    fn test_chmod_777_system() {
        assert!(DangerousModule.check(&ctx("chmod 777 /etc")).is_some());
    }

    #[test]
    fn test_chmod_r_root() {
        assert!(DangerousModule.check(&ctx("chmod -R 755 /")).is_some());
    }

    #[test]
    fn test_chown_r_root() {
        assert!(DangerousModule
            .check(&ctx("chown -R root:root /"))
            .is_some());
    }

    #[test]
    fn test_recursive_permission_changes_on_system_trees() {
        for cmd in [
            "chmod -R 755 /System",
            "chown --recursive root:wheel /Library",
            "sudo chgrp -Rv staff /etc",
            "bash -lc 'chmod -R 755 /Applications'",
        ] {
            assert!(
                DangerousModule.check(&ctx(cmd)).is_some(),
                "should block: {cmd}"
            );
        }
        assert!(DangerousModule
            .check(&ctx("chmod -R 755 ~/code/app/target"))
            .is_none());
    }

    #[test]
    fn test_fork_bomb() {
        assert!(DangerousModule.check(&ctx(":(){ :|:& };:")).is_some());
    }

    #[test]
    fn test_fork_bomb_dot_variant() {
        assert!(DangerousModule.check(&ctx(".(){ .|.& };.")).is_some());
    }

    #[test]
    fn test_safe_rm() {
        // On native Windows a Unix temp path names no fixed place and is
        // refused (`canonical_operand`).
        let verdict = DangerousModule.check(&ctx("rm -rf /tmp/test"));
        assert_eq!(verdict.is_some(), cfg!(windows), "{verdict:?}");
    }

    #[test]
    fn temp_roots_are_fixed_and_protected_themselves() {
        for root in [
            "/tmp",
            "/tmp/",
            "/tmp/*",
            "/tmp*",
            "/private/tmp",
            "/var/tmp",
            "/private/var/tmp/",
            "/private/var/folders/ab/cd123/T",
            "/private/var/folders/ab/cd123/T/*",
        ] {
            // On native Windows nothing below a Unix temp root is placed,
            // and every such deletion is refused (`canonical_operand`).
            let place = if cfg!(windows) {
                TempPlace::Outside(None)
            } else {
                TempPlace::Root
            };
            assert_eq!(temp_place(root, None), place, "{root}");
            assert!(DangerousModule
                .check(&ctx(&format!("rm -rf {root}")))
                .is_some());
        }
        for below in [
            "/tmp/scratch",
            "/private/tmp/claude-501/work",
            "/private/var/tmp/cache",
            "/private/var/folders/ab/cd123/T/scratch",
            "/private/var/folders/a_/x+y_0/T/build/*",
        ] {
            let place = if cfg!(windows) {
                TempPlace::Outside(None)
            } else {
                TempPlace::Below
            };
            assert_eq!(temp_place(below, None), place, "{below}");
            let verdict = DangerousModule.check(&ctx(&format!("rm -r {below}")));
            assert_eq!(verdict.is_some(), cfg!(windows), "{below}: {verdict:?}");
        }
        // Lookalikes of the per-user root are not roots.
        for outside in [
            "/private/var/folders/not-xx/id/T/cache",
            "/private/var/foldersXX/ab/id/T/cache",
            "/private/var/folders/ab/id/T-not/cache",
            "/private/var/folders/ab/cd123/C/cache",
            "/private/var/db/x",
        ] {
            assert!(
                matches!(temp_place(outside, None), TempPlace::Outside(_)),
                "{outside}"
            );
        }
    }

    #[test]
    fn tmpdir_adds_no_root_and_only_a_valid_one_is_protected() {
        // A system directory as `$TMPDIR` confers nothing.
        for (tmpdir, path) in [
            ("/private/etc", "/private/etc/hosts"),
            ("/private/var", "/private/var/db"),
            ("/usr/local", "/usr/local/bin"),
            ("/usr", "/usr/lib"),
            ("/", "/etc/x"),
        ] {
            assert!(
                matches!(
                    temp_place(path, Some(Path::new(tmpdir))),
                    TempPlace::Outside(_)
                ),
                "{tmpdir} {path}"
            );
        }
        // A custom `$TMPDIR` nested below a root is protected itself, also
        // through an alias spelling, and its descendants stay exempt. On
        // native Windows the temp folder lies below no Unix temp root, so
        // there is no such `$TMPDIR` to protect.
        if cfg!(windows) {
            return;
        }
        let scratch = tempfile::tempdir().unwrap();
        let tmpdir = scratch.path().join("agent-tmp");
        std::fs::create_dir(&tmpdir).unwrap();
        let canonical = std::fs::canonicalize(&tmpdir).unwrap();
        let canonical = canonical.to_str().unwrap();
        for tmpdir_spelling in [tmpdir.clone(), scratch.path().join("./agent-tmp/")] {
            let tmpdir = Some(tmpdir_spelling.as_path());
            assert_eq!(temp_place(canonical, tmpdir), TempPlace::Root);
            assert_eq!(
                temp_place(&format!("{canonical}/"), tmpdir),
                TempPlace::Root
            );
            assert_eq!(
                temp_place(&format!("{canonical}/*"), tmpdir),
                TempPlace::Root
            );
            assert_eq!(
                temp_place(&format!("{canonical}/x"), tmpdir),
                TempPlace::Below
            );
        }
        #[cfg(unix)]
        {
            let alias = scratch.path().join("alias");
            std::os::unix::fs::symlink(&tmpdir, &alias).unwrap();
            assert_eq!(temp_place(canonical, Some(&alias)), TempPlace::Root);
        }
    }

    #[test]
    fn a_lexical_escape_from_a_temp_root_is_not_below_it() {
        for path in [
            "/private/tmp/../etc/hosts",
            "//private//tmp/../etc/hosts",
            "/private/tmp/claude-501/../../etc",
        ] {
            assert!(
                matches!(temp_place(path, None), TempPlace::Outside(_)),
                "{path}"
            );
            assert!(DangerousModule
                .check(&ctx(&format!("rm -rf {path}")))
                .is_some());
        }
        // `..` below a component that does not exist cannot be resolved.
        let scratch = tempfile::tempdir().unwrap();
        let base = std::fs::canonicalize(scratch.path()).unwrap();
        let escape = format!("{}/missing/../../../etc/hosts", base.display());
        assert_eq!(temp_place(&escape, None), TempPlace::Outside(None));
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_below_a_temp_root_is_judged_by_its_target() {
        let scratch = tempfile::tempdir().unwrap();
        let base = std::fs::canonicalize(scratch.path()).unwrap();
        let base = base.to_str().unwrap();
        let link = format!("{base}/outside-link");
        std::os::unix::fs::symlink("/etc", &link).unwrap();
        std::os::unix::fs::symlink("/nonexistent-codeflow-target/x", format!("{base}/dangling"))
            .unwrap();

        assert_eq!(temp_place(&format!("{base}/work"), None), TempPlace::Below);
        assert_eq!(temp_place(&format!("{base}/*"), None), TempPlace::Below);
        for command in [
            "rm -rf /etc/hosts".to_string(),
            format!("rm -rf {link}/hosts"),
            format!("rm -rf \"{link}/hosts\""),
            format!("rm -rf '{link}'/hosts"),
            format!("rm -rf {link}/*"),
            format!("rm -rf {link}/"),
            format!("rm -rf {link}/../var/db"),
            format!("chmod -R 777 {link}/ssh"),
        ] {
            assert!(DangerousModule.check(&ctx(&command)).is_some(), "{command}");
        }
        // A glob before the last component may match a link: not exempt,
        // and refused outright, whichever way temp space is spelled.
        assert_eq!(
            temp_place(&format!("{base}/out*/hosts"), None),
            TempPlace::Outside(None)
        );
        assert!(DangerousModule
            .check(&ctx("rm -rf /tmp/scratch/glob-*/hosts"))
            .is_some());
        assert!(DangerousModule
            .check(&ctx("rm -rf /tmp/scratch/glob-*/../var/db"))
            .is_some());
        // A literal backslash in a quoted name makes the path unknowable.
        std::os::unix::fs::symlink("/etc", format!("{base}/odd\\link")).unwrap();
        for command in [
            format!("rm -rf '{base}/odd\\link/hosts'"),
            "rm -rf '/tmp/scratch/odd\\link/hosts'".to_string(),
            "rm -rf '/tmp/scratch/a\"b/hosts'".to_string(),
            "rm -rf \"/tmp/scratch/`echo x`/hosts\"".to_string(),
        ] {
            assert!(DangerousModule.check(&ctx(&command)).is_some(), "{command}");
        }
        assert!(!has_known_identity("/tmp/odd\\link"));
        assert!(has_known_identity("/tmp/quoted dir"));
        // Quoting the tokenizer removes keeps the operand's identity.
        assert!(DangerousModule
            .check(&ctx(&format!(
                "rm -rf '{base}/x'\"y\" \"{base}/quoted dir\""
            )))
            .is_none());
        // A component that exists but does not resolve establishes nothing.
        assert_eq!(
            temp_place(&format!("{base}/dangling/y"), None),
            TempPlace::Outside(None)
        );
        assert!(DangerousModule
            .check(&ctx(&format!("rm -rf {base}/work \"{base}/quoted dir\"")))
            .is_none());
    }

    #[test]
    fn test_safe_chmod() {
        assert!(DangerousModule
            .check(&ctx("chmod 755 ./script.sh"))
            .is_none());
    }
}
