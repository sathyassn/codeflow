//! `codeflow claude-session`: start Claude Code in Codeflow projects with a
//! Codeflow-owned settings file passed by `--settings` (TSK-1070, ADR-0093).
//!
//! A repository's settings can turn the sandbox off, add exclusions and
//! widen the host list, and Claude Code ignores `strictAllowlist` and
//! `bypassPermissions` from a repository file. Settings passed with
//! `--settings` make the sandbox binding: Claude Code then ignores the
//! repository settings that loosen it. `--install` writes these
//! under the person's home, each shown before it is written:
//!
//! - `~/.codeflow/claude/session.json`: the sandbox locks, the shipped
//!   exclusions and the host list with `strictAllowlist`, and bypass mode
//!   only on the person's typed yes;
//! - `~/.codeflow/shell/claude.sh`: a `claude` shell function that passes
//!   that file inside a Codeflow project and nothing anywhere else;
//! - one line in `~/.zshrc` or `~/.bashrc` that sources the function, only
//!   on the person's typed yes, recorded in `~/.codeflow/claude/install.json`
//!   so `--uninstall` removes exactly that line;
//! - optional additions to the person's `~/.claude/settings.json`, one
//!   item at a time and only on a typed yes ([`user_settings`]).
//!
//! Agent sessions cannot run the install: the shipped presets deny edits of
//! shell startup files and the sandbox denies writing them, so the person
//! runs it in their own terminal.

use std::io::BufRead;
use std::path::{Path, PathBuf};

use clap::Args;
use serde_json::{json, Map, Value};

use super::bypass_mode;

pub mod user_settings;

/// The shell function, written to `~/.codeflow/shell/claude.sh`.
pub const SCRIPT: &str = include_str!("claude_session/claude.sh");

/// The process wrapper for editors that start Claude Code themselves,
/// written to `~/.codeflow/shell/claude-wrapper.sh`.
pub const WRAPPER: &str = include_str!("claude_session/claude-wrapper.sh");

/// The shipped default preset: the source of the exclusions and hosts.
const PRESET: &str = include_str!("../../../../assets/base/settings/default.json");

/// The environment variable the function sets for the Claude process, which
/// the session-start hook reads to warn about a start without the file.
pub const MARKER_VAR: &str = "CODEFLOW_SESSION_SETTINGS";

/// The comment that marks the startup-file line as this command's.
const RC_TAG: &str = "# added by codeflow claude-session";

/// `codeflow claude-session` arguments.
#[derive(Debug, Args)]
pub struct ClaudeSessionArgs {
    /// Write the session settings and the `claude` shell function, and on
    /// your yes source it from your shell startup file and turn on bypass
    /// mode for Codeflow sessions.
    #[arg(long, conflicts_with = "uninstall")]
    pub install: bool,
    /// Remove what `--install` wrote, where it is unchanged, and exactly
    /// the startup-file line it added.
    #[arg(long)]
    pub uninstall: bool,
}

/// The files the command reads and writes.
#[derive(Debug, Clone)]
pub struct Layout {
    /// `~/.codeflow/claude/session.json`.
    pub session: PathBuf,
    /// `~/.codeflow/shell/claude.sh`.
    pub script: PathBuf,
    /// `~/.codeflow/shell/claude-wrapper.sh`.
    pub wrapper: PathBuf,
    /// `~/.codeflow/claude/install.json`: what was written.
    pub manifest: PathBuf,
    /// The startup files offered, for the person's shell.
    pub rc_files: Vec<PathBuf>,
    /// Where bypass mode can be turned off by managed or user settings.
    pub bypass: bypass_mode::Paths,
    /// `~/.codeflow/claude-user-settings.json`: what was added to the
    /// person's Claude Code user settings.
    pub user_record: PathBuf,
    /// What the host offers for the user settings.
    pub host: user_settings::Host,
}

impl Layout {
    /// The layout under `home`, offering the startup file of `shell`
    /// (`ZDOTDIR` moves the zsh one).
    #[must_use]
    pub fn new(
        home: &Path,
        shell: &str,
        zdotdir: Option<&Path>,
        bypass: bypass_mode::Paths,
    ) -> Self {
        let base = home.join(".codeflow");
        let name = Path::new(shell)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        let rc_files = match name {
            "zsh" => vec![zdotdir.unwrap_or(home).join(".zshrc")],
            "bash" => vec![home.join(".bashrc")],
            _ => Vec::new(),
        };
        Self {
            session: base.join("claude/session.json"),
            script: base.join("shell/claude.sh"),
            wrapper: base.join("shell/claude-wrapper.sh"),
            manifest: base.join("claude/install.json"),
            rc_files,
            bypass,
            user_record: base.join("claude-user-settings.json"),
            host: user_settings::Host::detect(),
        }
    }

    /// The layout for this process's home and shell.
    #[must_use]
    pub fn from_env() -> Option<Self> {
        let home = std::env::var_os("HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)?;
        let shell = std::env::var("SHELL").unwrap_or_default();
        let zdotdir = std::env::var_os("ZDOTDIR")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from);
        Some(Self::new(
            &home,
            &shell,
            zdotdir.as_deref(),
            bypass_mode::Paths::from_env()?,
        ))
    }
}

/// The startup-file line that sources the function.
#[must_use]
pub fn rc_line() -> String {
    format!(
        "[ -f \"$HOME/.codeflow/shell/claude.sh\" ] && . \"$HOME/.codeflow/shell/claude.sh\" {RC_TAG}"
    )
}

fn strings(value: &Value) -> Vec<Value> {
    value.as_array().cloned().unwrap_or_default()
}

/// Add each of `shipped` missing from `existing`, keeping the person's own
/// entries after the shipped ones.
fn union(shipped: &[Value], existing: &Value) -> Value {
    let mut out: Vec<Value> = shipped.to_vec();
    for entry in strings(existing) {
        if !out.contains(&entry) {
            out.push(entry);
        }
    }
    Value::Array(out)
}

/// Exclusions an earlier build of this command wrote and the security
/// review narrowed (TSK-1070 SEC-03): a reinstall drops them, so a session
/// file never keeps a wider exclusion than the shipped list.
const RETIRED_EXCLUSIONS: [&str; 5] = [
    "gh *",
    "gh pr *",
    "gh run *",
    "gh issue *",
    "git -C * push *",
];

/// The session settings: the existing file's own entries kept, the shipped
/// exclusions and hosts added, the locks set, and bypass mode set or
/// removed by the person's answer.
#[must_use]
pub fn session_settings(existing: Option<&Value>, bypass: bool) -> Value {
    let preset = preset();
    let sandbox = &preset["sandbox"];
    let old = existing.cloned().unwrap_or_else(|| json!({}));
    let mut out = match old {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    let old_sandbox = out.get("sandbox").cloned().unwrap_or_else(|| json!({}));
    let mut new_sandbox = match old_sandbox.clone() {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    new_sandbox.insert("enabled".into(), json!(true));
    // A missing sandbox, such as Linux without bubblewrap, stops the
    // session instead of running commands unsandboxed; this file is
    // CodeFlow's alone, so no other project or platform inherits it
    // (security review SEC-04).
    new_sandbox.insert("failIfUnavailable".into(), json!(true));
    new_sandbox.insert("allowUnsandboxedCommands".into(), json!(false));
    let mut kept_exclusions = old_sandbox["excludedCommands"].clone();
    if let Some(entries) = kept_exclusions.as_array_mut() {
        entries.retain(|e| !e.as_str().is_some_and(|s| RETIRED_EXCLUSIONS.contains(&s)));
    }
    new_sandbox.insert(
        "excludedCommands".into(),
        union(&strings(&sandbox["excludedCommands"]), &kept_exclusions),
    );
    let old_network = &old_sandbox["network"];
    let mut network = match old_network.clone() {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    network.insert("strictAllowlist".into(), json!(true));
    for key in ["allowedDomains", "deniedDomains"] {
        network.insert(
            key.into(),
            union(&strings(&sandbox["network"][key]), &old_network[key]),
        );
    }
    new_sandbox.insert("network".into(), Value::Object(network));
    let old_fs = &old_sandbox["filesystem"];
    let mut filesystem = match old_fs.clone() {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    for key in ["denyRead", "denyWrite"] {
        filesystem.insert(
            key.into(),
            union(&strings(&sandbox["filesystem"][key]), &old_fs[key]),
        );
    }
    new_sandbox.insert("filesystem".into(), Value::Object(filesystem));
    let mut credentials = match old_sandbox["credentials"].clone() {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    credentials.insert(
        "envVars".into(),
        union(
            &strings(&sandbox["credentials"]["envVars"]),
            &old_sandbox["credentials"]["envVars"],
        ),
    );
    new_sandbox.insert("credentials".into(), Value::Object(credentials));
    out.insert("sandbox".into(), Value::Object(new_sandbox));

    let mut permissions = match out.get("permissions").cloned() {
        Some(Value::Object(map)) => map,
        _ => Map::new(),
    };
    for key in ["deny", "ask"] {
        let theirs = permissions.get(key).cloned().unwrap_or(Value::Null);
        let merged = union(&strings(&preset["permissions"][key]), &theirs);
        permissions.insert(key.into(), merged);
    }
    if bypass {
        permissions.insert("defaultMode".into(), json!("bypassPermissions"));
    } else {
        permissions.remove("defaultMode");
    }
    out.insert("permissions".into(), Value::Object(permissions));
    Value::Object(out)
}

/// What `--install` recorded.
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Manifest {
    /// SHA-256 of the session file as written.
    pub session: Option<String>,
    /// SHA-256 of the script as written.
    pub script: Option<String>,
    /// SHA-256 of the editor wrapper as written.
    #[serde(default)]
    pub wrapper: Option<String>,
    /// Each startup file a line was added to, with the line.
    pub rc: Vec<RcEntry>,
    /// The session file's content before the first install wrote it, put
    /// back by `--uninstall`; `None` when there was no file.
    #[serde(default)]
    pub session_prior: Option<String>,
    /// The script's content before the first install wrote it.
    #[serde(default)]
    pub script_prior: Option<String>,
    /// The editor wrapper's content before the first install wrote it.
    #[serde(default)]
    pub wrapper_prior: Option<String>,
}

/// One startup-file line the install added.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RcEntry {
    /// The startup file.
    pub file: PathBuf,
    /// The exact line added.
    pub line: String,
}

fn sha(bytes: &[u8]) -> String {
    codeflow_core::scaffold::sha256_hex(bytes)
}

/// The install record: empty when there is none yet, an error when it
/// exists but cannot be read or parsed, so what it tracks is never lost.
fn read_manifest(layout: &Layout) -> Result<Manifest, String> {
    let text = match std::fs::read_to_string(&layout.manifest) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Manifest::default()),
        Err(err) => return Err(format!("cannot read {} ({err})", layout.manifest.display())),
    };
    serde_json::from_str(&text).map_err(|err| {
        format!(
            "{} is not a valid record ({err})",
            layout.manifest.display()
        )
    })
}

/// The embedded preset. A test parses it, so it is always valid JSON.
fn preset() -> Value {
    serde_json::from_str(PRESET).expect("the embedded preset is valid JSON")
}

/// Replace `path` with `text` through a temporary file. A link, such as a
/// startup file kept in a dotfiles directory, is followed so the file it
/// points at is written and the link stays. The temporary file is a new,
/// exclusively created name beside the target, so a planted file or link
/// of that name is never written through (security review SEC-06). An
/// existing file keeps its permission bits; a new one is readable by its
/// owner only.
fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    let resolved = match std::fs::canonicalize(path) {
        Ok(target) => target,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => path.to_path_buf(),
        Err(err) => return Err(err),
    };
    let path = resolved.as_path();
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let permissions = match std::fs::metadata(path) {
        Ok(meta) => Some(meta.permissions()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => return Err(err),
    };
    let (temp, mut file) = create_unique(parent, path.file_name().unwrap_or_default())?;
    let written = file
        .write_all(text.as_bytes())
        .and_then(|()| file.sync_all())
        .and_then(|()| match permissions {
            Some(permissions) => std::fs::set_permissions(&temp, permissions),
            None => Ok(()),
        })
        .and_then(|()| std::fs::rename(&temp, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    written
}

/// A new file beside the target, created with `create_new` (which fails
/// on any existing entry, a link included) under a name nobody can know in
/// advance, with owner-only permissions.
fn create_unique(dir: &Path, name: &std::ffi::OsStr) -> std::io::Result<(PathBuf, std::fs::File)> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut last = None;
    for _ in 0..64 {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        // The name is joined as bytes, never decoded.
        let mut file_name = std::ffi::OsString::from(".");
        file_name.push(name);
        file_name.push(format!(
            ".codeflow-{}-{nanos}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let temp = dir.join(file_name);
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&temp) {
            Ok(file) => return Ok((temp, file)),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => last = Some(err),
            Err(err) => return Err(err),
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("no free temporary name")))
}

/// Save the install record. It is written before each change it records,
/// so an interrupted install leaves nothing unrecorded: a recorded change
/// that never happened is harmless to `--uninstall`, which removes only
/// what matches the record.
fn save_manifest(layout: &Layout, manifest: &Manifest) -> Result<(), String> {
    let record = serde_json::to_string_pretty(manifest).unwrap_or_default() + "\n";
    write_atomic(&layout.manifest, &record)
        .map_err(|err| format!("Cannot write {}: {err}", layout.manifest.display()))
}

/// The content at `path` that predates Codeflow, for `prior`: read once,
/// on the first install over a file this command did not write.
fn note_prior(
    path: &Path,
    recorded: Option<&String>,
    prior: &mut Option<String>,
) -> Result<(), String> {
    if recorded.is_some() || prior.is_some() {
        return Ok(());
    }
    match std::fs::read_to_string(path) {
        Ok(text) => {
            *prior = Some(text);
            Ok(())
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(format!("Cannot read {}: {err}", path.display())),
    }
}

/// Write one of Codeflow's files: note what it replaces, record the new
/// content's hash, then write.
fn write_owned(
    layout: &Layout,
    manifest: &mut Manifest,
    which: Owned,
    text: &str,
) -> Result<(), String> {
    let (path, recorded, prior) = match which {
        Owned::Session => (
            &layout.session,
            &mut manifest.session,
            &mut manifest.session_prior,
        ),
        Owned::Script => (
            &layout.script,
            &mut manifest.script,
            &mut manifest.script_prior,
        ),
        Owned::Wrapper => (
            &layout.wrapper,
            &mut manifest.wrapper,
            &mut manifest.wrapper_prior,
        ),
    };
    note_prior(path, recorded.as_ref(), prior)?;
    *recorded = Some(sha(text.as_bytes()));
    let path = path.clone();
    save_manifest(layout, manifest)?;
    write_atomic(&path, text).map_err(|err| format!("Cannot write {}: {err}", path.display()))
}

/// The files `--install` owns.
#[derive(Clone, Copy)]
enum Owned {
    Session,
    Script,
    Wrapper,
}

fn ask(input: &mut impl BufRead, question: &str) -> bool {
    print!("{question} Type yes to accept [no]: ");
    let _ = std::io::Write::flush(&mut std::io::stdout());
    let mut line = String::new();
    let _ = input.read_line(&mut line);
    matches!(line.trim().to_ascii_lowercase().as_str(), "yes" | "y")
}

/// Whether `file` already holds `line` as a whole line.
fn has_line(file: &Path, line: &str) -> bool {
    std::fs::read_to_string(file).is_ok_and(|text| text.lines().any(|l| l == line))
}

/// Append `line` to `file`, creating it when absent.
fn append_line(file: &Path, line: &str) -> std::io::Result<()> {
    let existing = match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(err) => return Err(err),
    };
    let mut text = existing;
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(line);
    text.push('\n');
    write_atomic(file, &text)
}

/// Remove every line of `file` equal to `line`; returns how many.
fn remove_line(file: &Path, line: &str) -> std::io::Result<usize> {
    let text = match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(err) => return Err(err),
    };
    let kept: Vec<&str> = text.lines().filter(|l| *l != line).collect();
    let removed = text.lines().count() - kept.len();
    if removed > 0 {
        let mut out = kept.join("\n");
        if !out.is_empty() {
            out.push('\n');
        }
        write_atomic(file, &out)?;
    }
    Ok(removed)
}

/// Offer the startup-file line for each of the person's startup files,
/// adding it only on a typed yes and recording it in `manifest`.
fn offer_rc_lines(
    input: &mut impl BufRead,
    layout: &Layout,
    manifest: &mut Manifest,
) -> Result<(), String> {
    let line = rc_line();
    if layout.rc_files.is_empty() {
        println!("  Your shell is not zsh or bash, so no startup file is offered. To load the function, source {} from your shell's startup file.", layout.script.display());
    }
    for file in &layout.rc_files {
        if has_line(file, &line) {
            // A line this command did not add stays the person's: it is
            // not recorded, so `--uninstall` leaves it (SEC-07).
            println!("  {} already sources it.", file.display());
            continue;
        }
        println!(
            "  To load the function in new shells, this line would be added to the end of {}:",
            file.display()
        );
        println!("    {line}");
        if !ask(input, "  Add it?") {
            println!(
                "  Not added. Source {} yourself to use it.",
                layout.script.display()
            );
            continue;
        }
        manifest.rc.push(RcEntry {
            file: file.clone(),
            line: line.clone(),
        });
        save_manifest(layout, manifest)?;
        if let Err(err) = append_line(file, &line) {
            println!("  Cannot change {}: {err}", file.display());
            manifest.rc.pop();
            save_manifest(layout, manifest)?;
            continue;
        }
        println!(
            "  Added. Open a new terminal, or run: . {}",
            layout.script.display()
        );
    }
    Ok(())
}

/// Write the editor wrapper, executable, and print the VS Code setting
/// that uses it; the person adds the setting, since editor settings files
/// may hold comments this command cannot keep.
fn write_wrapper(layout: &Layout, manifest: &mut Manifest) -> bool {
    if let Err(why) = write_owned(layout, manifest, Owned::Wrapper, WRAPPER) {
        println!("{why}");
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&layout.wrapper, std::fs::Permissions::from_mode(0o755));
    }
    println!("  Wrote {}. To have the VS Code extension start Claude Code with the session file, add this to your VS Code user settings:", layout.wrapper.display());
    println!(
        "    \"claudeCode.claudeProcessWrapper\": \"{}\"",
        layout.wrapper.display()
    );
    true
}

/// Install: write the session file and the script, then offer bypass mode
/// and the startup-file line, each only on a typed yes. Returns the exit
/// code.
pub fn install(input: &mut impl BufRead, layout: &Layout) -> i32 {
    let mut manifest = match read_manifest(layout) {
        Ok(manifest) => manifest,
        Err(why) => {
            println!("{why}; fix or remove it first. Nothing written.");
            return 1;
        }
    };
    let existing: Option<Value> = match std::fs::read_to_string(&layout.session) {
        Ok(text) => match serde_json::from_str(&text) {
            Ok(value) => Some(value),
            Err(err) => {
                println!(
                    "{} is not valid JSON ({err}); fix or remove it first. Nothing written.",
                    layout.session.display()
                );
                return 1;
            }
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => {
            println!(
                "Cannot read {} ({err}); nothing written.",
                layout.session.display()
            );
            return 1;
        }
    };
    let had_bypass = existing
        .as_ref()
        .and_then(|v| v["permissions"]["defaultMode"].as_str())
        == Some("bypassPermissions");

    println!("CodeFlow session settings: {}", layout.session.display());
    println!("  Inside a CodeFlow project, `claude` will start with --settings pointing at this file, so the sandbox stays on, a refused command never reruns outside it, and sandboxed commands reach only the listed hosts. A repository file cannot loosen these.");
    let bypass = match bypass_mode::check_allowed(&layout.bypass) {
        Err(refusal) => {
            println!("  Bypass mode is not offered: {refusal}");
            false
        }
        Ok(()) => ask(
            input,
            &format!(
                "  Start CodeFlow sessions in bypass mode (no permission prompts; the guards, deny rules and sandbox still hold)? It applies only to sessions started with this file{}.",
                if had_bypass { "; it is on now" } else { "" }
            ),
        ),
    };
    let settings = session_settings(existing.as_ref(), bypass);
    let mut text = serde_json::to_string_pretty(&settings).unwrap_or_default();
    text.push('\n');
    if let Err(why) = write_owned(layout, &mut manifest, Owned::Session, &text) {
        println!("{why}");
        return 1;
    }
    println!(
        "  Wrote {}{}.",
        layout.session.display(),
        if bypass { " with bypass mode" } else { "" }
    );
    if let Err(why) = write_owned(layout, &mut manifest, Owned::Script, SCRIPT) {
        println!("{why}");
        return 1;
    }
    println!("  Wrote {}.", layout.script.display());
    if !write_wrapper(layout, &mut manifest) {
        return 1;
    }

    if let Err(why) = offer_rc_lines(input, layout, &mut manifest) {
        println!("{why}");
        return 1;
    }
    let preset = preset();
    let code = user_settings::offer(
        input,
        &layout.bypass.user,
        &layout.user_record,
        &preset,
        layout.host,
    );
    println!("  `command claude` starts Claude Code without the file; `codeflow claude-session --uninstall` removes what this wrote.");
    code
}

/// Uninstall: remove the startup-file lines the manifest records, and the
/// script and session file when they are unchanged since written.
pub fn uninstall(layout: &Layout) -> i32 {
    let manifest = match read_manifest(layout) {
        Ok(manifest) => manifest,
        Err(why) => {
            println!("{why}; nothing removed.");
            return 1;
        }
    };
    let mut code = 0;
    for entry in &manifest.rc {
        match remove_line(&entry.file, &entry.line) {
            Ok(0) => println!(
                "  {} no longer has the line; nothing to remove.",
                entry.file.display()
            ),
            Ok(_) => println!("  Removed the line from {}.", entry.file.display()),
            Err(err) => {
                println!("  Cannot change {}: {err}", entry.file.display());
                code = 1;
            }
        }
    }
    for (path, recorded, prior) in [
        (&layout.script, &manifest.script, &manifest.script_prior),
        (&layout.wrapper, &manifest.wrapper, &manifest.wrapper_prior),
        (&layout.session, &manifest.session, &manifest.session_prior),
    ] {
        match std::fs::read(path) {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                println!("  Cannot read {}: {err}", path.display());
                code = 1;
            }
            Ok(bytes) if recorded.as_deref() == Some(sha(&bytes).as_str()) => {
                // A file that was there before the install gets its old
                // content back; one the install created is removed.
                let done = match prior {
                    Some(text) => write_atomic(path, text).map(|()| "Restored"),
                    None => std::fs::remove_file(path).map(|()| "Removed"),
                };
                match done {
                    Ok(verb) => println!("  {verb} {}.", path.display()),
                    Err(err) => {
                        println!("  Cannot change {}: {err}", path.display());
                        code = 1;
                    }
                }
            }
            Ok(_) => println!(
                "  Kept {}: it changed since codeflow wrote it.",
                path.display()
            ),
        }
    }
    if user_settings::remove(&layout.user_record) != 0 {
        code = 1;
    }
    if code == 0 {
        let _ = std::fs::remove_file(&layout.manifest);
    }
    code
}

/// Run `codeflow claude-session`.
#[must_use]
pub fn run(args: &ClaudeSessionArgs) -> i32 {
    let Some(layout) = Layout::from_env() else {
        eprintln!("codeflow claude-session: the home directory is not set");
        return 1;
    };
    if args.install {
        return install(&mut std::io::stdin().lock(), &layout);
    }
    if args.uninstall {
        return uninstall(&layout);
    }
    println!("Claude Code takes bypass mode and a binding sandbox only from settings above the repository. CodeFlow keeps them in {} and starts `claude` with it inside CodeFlow projects.", layout.session.display());
    println!(
        "  Session file: {}",
        if layout.session.exists() {
            "present"
        } else {
            "absent"
        }
    );
    println!(
        "  Shell function: {}",
        if layout.script.exists() {
            "present"
        } else {
            "absent"
        }
    );
    match read_manifest(&layout) {
        Ok(manifest) => {
            for entry in manifest.rc {
                println!("  Sourced from: {}", entry.file.display());
            }
        }
        Err(why) => println!("  Install record: {why}"),
    }
    println!("  Install or refresh: codeflow claude-session --install");
    println!("  One session without it: command claude");
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(dir: &Path, shell: &str) -> Layout {
        let home = dir.join("home");
        std::fs::create_dir_all(&home).unwrap();
        let managed = dir.join("managed");
        std::fs::create_dir_all(&managed).unwrap();
        let mut layout = Layout::new(
            &home,
            shell,
            None,
            bypass_mode::Paths {
                user: home.join(".claude/settings.json"),
                managed_dir: managed,
            },
        );
        layout.host = user_settings::Host {
            sandbox_supported: true,
            herdr: false,
        };
        layout
    }

    fn read(path: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn the_session_file_locks_the_sandbox_and_keeps_the_persons_entries() {
        let existing = json!({
            "sandbox": {"excludedCommands": ["docker *"], "network": {"allowedDomains": ["registry.example.com"]}},
            "env": {"X": "1"}
        });
        let out = session_settings(Some(&existing), false);
        assert_eq!(out["sandbox"]["enabled"], true);
        assert_eq!(out["sandbox"]["allowUnsandboxedCommands"], false);
        assert_eq!(out["sandbox"]["network"]["strictAllowlist"], true);
        let excluded = strings(&out["sandbox"]["excludedCommands"]);
        assert!(excluded.contains(&json!("git push *")));
        assert!(excluded.contains(&json!("docker *")));
        let hosts = strings(&out["sandbox"]["network"]["allowedDomains"]);
        assert!(hosts.contains(&json!("github.com")));
        assert!(hosts.contains(&json!("registry.example.com")));
        assert!(!hosts.contains(&json!("*")));
        assert!(strings(&out["sandbox"]["network"]["deniedDomains"]).contains(&json!("169.254.*")));
        assert_eq!(out["env"]["X"], "1");
        assert!(out["permissions"].get("defaultMode").is_none());
        let deny = strings(&out["permissions"]["deny"]);
        assert!(
            deny.contains(&json!("Bash(gh extension *)")),
            "the forge denies are binding"
        );
        assert!(strings(&out["permissions"]["ask"]).contains(&json!("Bash(gh api graphql*)")));
        assert!(strings(&out["sandbox"]["filesystem"]["denyWrite"])
            .contains(&json!("**/.codeflow/git-hooks")));
        assert!(out["sandbox"]["credentials"]["envVars"]
            .as_array()
            .unwrap()
            .contains(&json!({"name": "NPM_TOKEN", "mode": "deny"})));
        let bypass = session_settings(Some(&out), true);
        assert_eq!(bypass["permissions"]["defaultMode"], "bypassPermissions");
        let again = session_settings(Some(&bypass), false);
        assert!(again["permissions"].get("defaultMode").is_none());
        assert_eq!(
            strings(&again["permissions"]["deny"]).len(),
            deny.len(),
            "no duplicates"
        );
    }

    #[test]
    fn install_adds_the_line_only_on_yes_and_uninstall_removes_only_it() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout(dir.path(), "/bin/zsh");
        let rc = layout.rc_files[0].clone();
        std::fs::write(&rc, "export A=1\n").unwrap();

        // No to bypass, no to the line: files written, startup file untouched.
        assert_eq!(install(&mut "no\nno\n".as_bytes(), &layout), 0);
        assert_eq!(std::fs::read_to_string(&rc).unwrap(), "export A=1\n");
        assert_eq!(std::fs::read_to_string(&layout.script).unwrap(), SCRIPT);
        assert!(read(&layout.session)["permissions"]
            .get("defaultMode")
            .is_none());

        // Yes to both: the line is appended once and recorded.
        assert_eq!(install(&mut "yes\nyes\n".as_bytes(), &layout), 0);
        let text = std::fs::read_to_string(&rc).unwrap();
        assert_eq!(text, format!("export A=1\n{}\n", rc_line()));
        assert_eq!(
            read(&layout.session)["permissions"]["defaultMode"],
            "bypassPermissions"
        );
        assert_eq!(install(&mut "yes\n".as_bytes(), &layout), 0);
        assert_eq!(
            std::fs::read_to_string(&rc).unwrap(),
            text,
            "not added twice"
        );

        // The person adds a line after it; uninstall removes only ours.
        std::fs::write(&rc, format!("{text}export B=2\n")).unwrap();
        assert_eq!(uninstall(&layout), 0);
        assert_eq!(
            std::fs::read_to_string(&rc).unwrap(),
            "export A=1\nexport B=2\n"
        );
        assert!(!layout.script.exists() && !layout.session.exists());
        assert!(!layout.wrapper.exists());
        assert!(!layout.manifest.exists());
    }

    #[test]
    fn uninstall_keeps_a_session_file_the_person_changed() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout(dir.path(), "/bin/bash");
        assert_eq!(install(&mut "no\nno\n".as_bytes(), &layout), 0);
        std::fs::write(&layout.session, "{\"mine\": true}\n").unwrap();
        assert_eq!(uninstall(&layout), 0);
        assert!(layout.session.exists());
        assert!(!layout.script.exists());
    }

    #[test]
    fn bypass_is_not_offered_when_managed_settings_disable_it() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout(dir.path(), "/bin/zsh");
        std::fs::write(
            dir.path().join("managed/managed-settings.json"),
            r#"{"permissions": {"disableBypassPermissionsMode": "disable"}}"#,
        )
        .unwrap();
        // The first answer goes to the startup-file question.
        assert_eq!(install(&mut "yes\nno\n".as_bytes(), &layout), 0);
        assert!(read(&layout.session)["permissions"]
            .get("defaultMode")
            .is_none());
        assert!(has_line(&layout.rc_files[0], &rc_line()));
    }

    /// The function passes the session file only inside a Codeflow
    /// project, keeps the person's own arguments, sets the marker only
    /// there, and refuses with a message when the file is missing. Run in
    /// bash and zsh, with a stub `claude` that prints what it got.
    #[cfg(unix)]
    #[test]
    fn the_function_adds_the_file_only_in_a_codeflow_project() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let stub = bin.join("claude");
        std::fs::write(
            &stub,
            "#!/bin/sh\necho \"MARK=${CODEFLOW_SESSION_SETTINGS:-unset} ARGS=$*\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        let script = dir.path().join("claude.sh");
        std::fs::write(&script, SCRIPT).unwrap();
        let adopted = dir.path().join("adopted");
        let plain = dir.path().join("plain");
        for repo in [&adopted, &plain] {
            std::fs::create_dir_all(repo.join("sub")).unwrap();
            let status = codeflow_core::git::command()
                .args(["init", "-q"])
                .current_dir(repo)
                .status()
                .unwrap();
            assert!(status.success());
        }
        std::fs::create_dir_all(adopted.join(".codeflow")).unwrap();
        std::fs::write(adopted.join(".codeflow/project.toml"), "").unwrap();
        let session = home.join(".codeflow/claude/session.json");
        let path = format!(
            "{}:{}",
            bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let run_with = |shell: &str, cwd: &Path, args: &str| {
            let out = std::process::Command::new(shell)
                .arg("-c")
                .arg(format!(". '{}'; claude {args}", script.display()))
                .current_dir(cwd)
                .env("HOME", &home)
                .env("PATH", &path)
                // Inherited, as from a parent session: only the function
                // may pass it on, and only with the file.
                .env("CODEFLOW_SESSION_SETTINGS", "1")
                .output()
                .unwrap();
            (
                out.status.success(),
                String::from_utf8_lossy(&out.stdout).trim().to_string(),
                String::from_utf8_lossy(&out.stderr).to_string(),
            )
        };
        let run = |shell: &str, cwd: &Path| run_with(shell, cwd, "--model x 'two words'");
        for shell in ["bash", "zsh"] {
            if std::process::Command::new(shell)
                .arg("-c")
                .arg("true")
                .status()
                .is_err()
            {
                continue;
            }
            // Missing session file: refused with the way out named.
            let (ok, stdout, stderr) = run(shell, &adopted.join("sub"));
            assert!(!ok && stdout.is_empty(), "{shell}: {stdout}");
            assert!(stderr.contains("command claude"), "{shell}: {stderr}");
            std::fs::create_dir_all(session.parent().unwrap()).unwrap();
            std::fs::write(&session, "{}").unwrap();
            let (ok, stdout, _) = run(shell, &adopted.join("sub"));
            assert!(ok);
            assert_eq!(
                stdout,
                format!(
                    "MARK=1 ARGS=--settings {} --model x two words",
                    session.display()
                ),
                "{shell}"
            );
            // A second settings file would replace CodeFlow's: refused.
            for args in ["--settings mine.json -p hi", "-p hi --settings=mine.json"] {
                let (ok, stdout, stderr) = run_with(shell, &adopted.join("sub"), args);
                assert!(!ok && stdout.is_empty(), "{shell} {args}: {stdout}");
                assert!(stderr.contains("only one --settings"), "{shell}: {stderr}");
            }
            let (ok, stdout, _) = run_with(shell, &plain.join("sub"), "--settings mine.json");
            assert!(ok);
            assert_eq!(stdout, "MARK=unset ARGS=--settings mine.json", "{shell}");
            let (ok, stdout, _) = run(shell, &plain.join("sub"));
            assert!(ok);
            assert_eq!(stdout, "MARK=unset ARGS=--model x two words", "{shell}");
            let (ok, stdout, _) = run(shell, dir.path());
            assert!(ok);
            assert_eq!(stdout, "MARK=unset ARGS=--model x two words", "{shell}");
            std::fs::remove_file(&session).unwrap();
        }
    }

    /// The editor wrapper adds the session file only inside a Codeflow
    /// project, refuses there when the file is missing or another settings
    /// file is passed (as the function does), runs the command unchanged
    /// anywhere else, and never passes on an inherited marker.
    #[cfg(unix)]
    #[test]
    fn the_editor_wrapper_adds_the_file_only_where_it_can() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let exe = dir.path().join("bundled-claude");
        std::fs::write(
            &exe,
            "#!/bin/sh\necho \"MARK=${CODEFLOW_SESSION_SETTINGS:-unset} ARGS=$*\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        let wrapper = dir.path().join("claude-wrapper.sh");
        std::fs::write(&wrapper, WRAPPER).unwrap();
        let adopted = dir.path().join("adopted");
        std::fs::create_dir_all(adopted.join(".codeflow")).unwrap();
        let status = codeflow_core::git::command()
            .args(["init", "-q"])
            .current_dir(&adopted)
            .status()
            .unwrap();
        assert!(status.success());
        std::fs::write(adopted.join(".codeflow/project.toml"), "").unwrap();
        // An inherited marker is set for every start, so only the wrapper
        // can make it 1.
        let attempt = |cwd: &Path, args: &[&str]| {
            let out = std::process::Command::new("sh")
                .arg(&wrapper)
                .arg(&exe)
                .args(args)
                .current_dir(cwd)
                .env("HOME", &home)
                .env("CODEFLOW_SESSION_SETTINGS", "1")
                .output()
                .unwrap();
            (
                out.status.success(),
                String::from_utf8_lossy(&out.stdout).trim().to_string(),
                String::from_utf8_lossy(&out.stderr).to_string(),
            )
        };
        let run = |cwd: &Path, args: &[&str]| {
            let (ok, stdout, stderr) = attempt(cwd, args);
            assert!(ok, "{stderr}");
            stdout
        };
        // No session file yet: refused with the way out named, as the
        // function refuses.
        let (ok, stdout, stderr) = attempt(&adopted, &["--output-format", "stream-json"]);
        assert!(!ok && stdout.is_empty(), "{stdout}");
        assert!(
            stderr.contains("codeflow claude-session --install"),
            "{stderr}"
        );
        let session = home.join(".codeflow/claude/session.json");
        std::fs::create_dir_all(session.parent().unwrap()).unwrap();
        std::fs::write(&session, "{}").unwrap();
        assert_eq!(
            run(&adopted, &["--output-format", "stream-json"]),
            format!(
                "MARK=1 ARGS=--settings {} --output-format stream-json",
                session.display()
            )
        );
        // Another settings file would replace CodeFlow's: refused.
        for args in [
            &["--settings", "x.json"][..],
            &["--settings=x.json", "-p"][..],
        ] {
            let (ok, stdout, stderr) = attempt(&adopted, args);
            assert!(!ok && stdout.is_empty(), "{args:?}: {stdout}");
            assert!(stderr.contains("only one --settings"), "{stderr}");
        }
        assert_eq!(
            run(dir.path(), &["auth", "status"]),
            "MARK=unset ARGS=auth status"
        );
    }

    #[test]
    fn an_unknown_shell_gets_no_startup_file() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout(dir.path(), "/usr/bin/fish");
        assert!(layout.rc_files.is_empty());
        assert_eq!(install(&mut "no\n".as_bytes(), &layout), 0);
        assert!(layout.script.exists());
    }

    /// A startup file kept as a link (a dotfiles directory) is written
    /// through the link, which stays a link.
    #[cfg(unix)]
    #[test]
    fn a_linked_startup_file_keeps_its_link() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("dotfiles/zshrc");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, "export A=1\n").unwrap();
        let link = dir.path().join(".zshrc");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        append_line(&link, "# added").unwrap();
        assert!(std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "export A=1\n# added\n"
        );
        assert_eq!(remove_line(&link, "# added").unwrap(), 1);
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "export A=1\n");
    }

    /// A damaged install record refuses install and uninstall, so what it
    /// tracks is never overwritten or forgotten.
    #[test]
    fn a_damaged_install_record_refuses() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout(dir.path(), "/bin/zsh");
        std::fs::create_dir_all(layout.manifest.parent().unwrap()).unwrap();
        std::fs::write(&layout.manifest, "{ not json").unwrap();
        assert_eq!(install(&mut "yes\nyes\n".as_bytes(), &layout), 1);
        assert!(!layout.session.exists());
        assert_eq!(uninstall(&layout), 1);
        assert!(layout.manifest.exists());
    }

    /// Security review SEC-04: the session file stops the session when the
    /// sandbox cannot run, over an earlier `false`, and a reinstall drops
    /// the exclusions the review narrowed while keeping the person's own.
    #[test]
    fn the_session_file_fails_closed_and_drops_retired_exclusions() {
        let existing = json!({"sandbox": {
            "failIfUnavailable": false,
            "excludedCommands": ["gh run *", "gh pr *", "docker *"]
        }});
        let out = session_settings(Some(&existing), false);
        assert_eq!(out["sandbox"]["failIfUnavailable"], true);
        let excluded = strings(&out["sandbox"]["excludedCommands"]);
        assert!(!excluded.contains(&json!("gh run *")));
        assert!(!excluded.contains(&json!("gh pr *")));
        assert!(excluded.contains(&json!("gh run view *")));
        assert!(excluded.contains(&json!("docker *")));
        assert_eq!(
            session_settings(None, false)["sandbox"]["failIfUnavailable"],
            true
        );
    }

    /// Security review SEC-06: a file or link planted at a temporary name
    /// is never written through, an existing file keeps its permission
    /// bits, and a new file is owner-only.
    #[cfg(unix)]
    #[test]
    fn a_planted_temporary_file_is_never_written_through() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("zshrc");
        std::fs::write(&target, "export A=1\n").unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
        let elsewhere = dir.path().join("elsewhere");
        std::fs::write(&elsewhere, "untouched\n").unwrap();
        for name in ["zshrc.codeflow-tmp", ".zshrc.codeflow-tmp"] {
            std::os::unix::fs::symlink(&elsewhere, dir.path().join(name)).unwrap();
        }
        write_atomic(&target, "export B=2\n").unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "export B=2\n");
        assert_eq!(std::fs::read_to_string(&elsewhere).unwrap(), "untouched\n");
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&target), 0o600, "the existing mode is kept");
        let fresh = dir.path().join("new/session.json");
        write_atomic(&fresh, "{}\n").unwrap();
        assert_eq!(mode(&fresh), 0o600, "a new file is owner-only");
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains("codeflow-") && !n.ends_with("codeflow-tmp"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    /// Security review SEC-07: a startup-file line that was already there,
    /// and session, script and wrapper files that predate the install,
    /// survive an uninstall.
    #[test]
    fn uninstall_keeps_what_was_there_before_the_install() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout(dir.path(), "/bin/zsh");
        let rc = layout.rc_files[0].clone();
        let had = format!("export A=1\n{}\n", rc_line());
        std::fs::write(&rc, &had).unwrap();
        for (path, text) in [
            (&layout.session, "{\"mine\": true}\n"),
            (&layout.script, "# mine\n"),
            (&layout.wrapper, "# my wrapper\n"),
        ] {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        // The line is there, so only the bypass question is asked.
        assert_eq!(install(&mut "no\n".as_bytes(), &layout), 0);
        assert_eq!(read(&layout.session)["mine"], true);
        assert_eq!(std::fs::read_to_string(&layout.script).unwrap(), SCRIPT);
        // A second install records nothing new as the person's.
        assert_eq!(install(&mut "no\n".as_bytes(), &layout), 0);
        assert_eq!(uninstall(&layout), 0);
        assert_eq!(std::fs::read_to_string(&rc).unwrap(), had);
        assert_eq!(
            std::fs::read_to_string(&layout.session).unwrap(),
            "{\"mine\": true}\n"
        );
        assert_eq!(std::fs::read_to_string(&layout.script).unwrap(), "# mine\n");
        assert_eq!(
            std::fs::read_to_string(&layout.wrapper).unwrap(),
            "# my wrapper\n"
        );
        assert!(!layout.manifest.exists());
    }

    /// Review nit: the record is written before each change, so an install
    /// that stops part way leaves what it wrote recorded for removal.
    #[test]
    fn an_interrupted_install_leaves_its_writes_recorded() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout(dir.path(), "/bin/zsh");
        // A folder where the script goes stops the install after the
        // session file.
        std::fs::create_dir_all(layout.script.join("blocker")).unwrap();
        assert_eq!(install(&mut "no\nno\n".as_bytes(), &layout), 1);
        assert!(layout.session.exists());
        assert!(layout.manifest.exists(), "the session file is recorded");
        uninstall(&layout);
        assert!(!layout.session.exists());
    }
}
