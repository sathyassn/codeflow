//! Release routes through `gh` and the GitHub API (TSK-1346).
//!
//! The release family in the action table refuses `gh release` and tag
//! pushes. This reader adds the routes that publish without them: a
//! workflow dispatch of a release workflow, a repository dispatch a release
//! workflow listens for, a pending deployment approval, a release or tag
//! creation through the API, a deployment write, the re-enabling of a
//! release workflow, a change to a variable or secret a release workflow
//! reads, and a write to the Actions policies, environments, rulesets or
//! branch protection that hold the remote floors. `gh`, `curl`, `wget`,
//! `HTTPie` and xh are read. Each returns the form it names, and every doubt
//! refuses.
//!
//! An HTTP client write to a parsed host other than GitHub's is allowed. A
//! write the guard cannot rule out as reaching GitHub is refused: an option
//! it does not know, a destination that does not parse, or a redirect. A
//! client also reads more than its arguments: when a config file it reads
//! exists in the environment the call runs with ([`ClientEnv`]), every call
//! is refused unless it skips the file (`curl -q`; `wget --no-config`,
//! always needed since wget's system `wgetrc` cannot be checked). A gh
//! alias is judged as its expansion from every config gh may read.
//!
//! A re-run of a release run is not judged here: a run id names no
//! workflow the guard can read without the network. The release workflow
//! refuses its own re-runs instead (`docs/releasing.md`).
//!
//! A release workflow is a workflow file at the policy authority (the
//! remote's default branch, [`crate::hooks::landed_policy`]) that runs on
//! `workflow_dispatch` or `repository_dispatch` and whose run can publish:
//! its token can write contents, packages, deployments, pages, attestations
//! or an OIDC token, or its permissions are left to the repository default,
//! or it deploys to an environment, calls a reusable workflow, or reads a
//! secret other than `GITHUB_TOKEN`. A file the guard cannot read or parse
//! counts as a release workflow.
use std::path::{Path, PathBuf};

/// Where a call runs, which repository `gh` defaults to, and the
/// environment it runs with.
#[derive(Clone, Copy)]
pub(crate) struct Ctx<'a> {
    cwd: Option<&'a Path>,
    /// `GH_REPO` as the line or the session sets it.
    gh_repo: Option<&'a str>,
    /// The repository a call acts on when its path names none.
    target: &'a Target,
    /// The environment that places a client's config file.
    env: Option<&'a ClientEnv>,
}

impl<'a> Ctx<'a> {
    pub(crate) fn new(cwd: Option<&'a Path>, gh_repo: Option<&'a str>) -> Self {
        Self {
            cwd,
            gh_repo,
            target: &Target::Literal,
            env: None,
        }
    }

    /// The same context, finding config files through `env`.
    pub(crate) fn with_env(self, env: &'a ClientEnv) -> Self {
        Self {
            env: Some(env),
            ..self
        }
    }
}

/// The variables that place a client's config file.
pub(crate) const CONFIG_VARIABLES: &[&str] = &[
    "HOME",
    "CURL_HOME",
    "XDG_CONFIG_HOME",
    "GH_CONFIG_DIR",
    "HTTPIE_CONFIG_DIR",
    "XH_CONFIG_DIR",
    "AppData",
    "APPDATA",
];

/// The environment a client call may run with, for finding its config
/// file: every value the line gives a variable in [`CONFIG_VARIABLES`]
/// (inline, through `env`, or by `export` or assignment earlier on the
/// line), the guard's own environment, and the account's home directory.
/// Any of them may be the one the call sees, so a lookup checks them all.
pub(crate) struct ClientEnv {
    values: Vec<(String, String)>,
    /// Why the line may set a variable the guard cannot see, such as
    /// `source`, `eval` or an `env` option it does not read.
    unresolved: Option<String>,
    account_home: Option<PathBuf>,
}

impl ClientEnv {
    pub(crate) fn new(line: &[(String, String)], unresolved: Option<String>) -> Self {
        let (session, account_home) = session_environment();
        let mut values = line.to_vec();
        values.extend(session);
        Self {
            values,
            unresolved,
            account_home,
        }
    }

    /// Every directory `name` may hold; an error when a value cannot be
    /// resolved.
    fn dirs(&self, name: &str) -> Result<Vec<PathBuf>, String> {
        if let Some(why) = &self.unresolved {
            return Err(why.clone());
        }
        let mut dirs = Vec::new();
        for (key, value) in &self.values {
            if key != name || value.is_empty() {
                continue;
            }
            // A command substitution reaches here as a control character.
            if opaque(value) || value.starts_with('~') || value.chars().any(char::is_control) {
                return Err(format!(
                    "`{name}` holds an expansion the guard cannot resolve"
                ));
            }
            dirs.push(PathBuf::from(value));
        }
        Ok(dirs)
    }

    /// Every home directory: each `HOME` and the account's own.
    fn homes(&self) -> Result<Vec<PathBuf>, String> {
        let mut homes = self.dirs("HOME")?;
        let Some(home) = &self.account_home else {
            return Err("the account's home directory cannot be read".into());
        };
        homes.push(home.clone());
        Ok(homes)
    }
}

/// The guard's own values for [`CONFIG_VARIABLES`], and the account's home.
fn session_environment() -> (Vec<(String, String)>, Option<PathBuf>) {
    #[cfg(test)]
    if let Some(session) = TEST_SESSION.with(|s| s.borrow().clone()) {
        return session;
    }
    let values = CONFIG_VARIABLES
        .iter()
        .filter_map(|name| {
            std::env::var_os(name).map(|value| {
                let value = value.into_string().unwrap_or_else(|_| format!("${name}"));
                ((*name).to_owned(), value)
            })
        })
        .collect();
    (values, account_home())
}

#[cfg(test)]
thread_local! {
    /// The guard's environment and account home as a test sets them.
    #[allow(clippy::type_complexity)]
    pub(super) static TEST_SESSION: std::cell::RefCell<Option<(Vec<(String, String)>, Option<PathBuf>)>> =
        const { std::cell::RefCell::new(None) };
}

/// The account's home directory from the password database, which a
/// client reads when `HOME` is unset.
#[cfg(unix)]
fn account_home() -> Option<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    let mut buffer = vec![0 as libc::c_char; 16 * 1024];
    // SAFETY: `getpwuid_r` writes only into `entry` and `buffer`, which
    // outlive the call, and sets `found` to `entry` or null.
    let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
    let mut found: *mut libc::passwd = std::ptr::null_mut();
    let status = unsafe {
        libc::getpwuid_r(
            libc::getuid(),
            &raw mut entry,
            buffer.as_mut_ptr(),
            buffer.len(),
            &raw mut found,
        )
    };
    if status != 0 || found.is_null() || entry.pw_dir.is_null() {
        return None;
    }
    // SAFETY: `pw_dir` points into `buffer` as a NUL-terminated string.
    let dir = unsafe { std::ffi::CStr::from_ptr(entry.pw_dir) };
    Some(PathBuf::from(std::ffi::OsStr::from_bytes(dir.to_bytes())))
}

#[cfg(not(unix))]
fn account_home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").map(PathBuf::from)
}

/// The files among `candidates` that exist, a relative one read from `cwd`;
/// an error names a file whose presence cannot be checked.
fn present(candidates: Vec<PathBuf>, cwd: Option<&Path>) -> Result<Vec<PathBuf>, String> {
    let mut found = Vec::new();
    for path in candidates {
        let path = match cwd {
            Some(cwd) if path.is_relative() => cwd.join(&path),
            _ => path,
        };
        match path.try_exists() {
            Ok(true) => found.push(path),
            Ok(false) => {}
            Err(e) => return Err(format!("{} cannot be checked ({e})", path.display())),
        }
    }
    Ok(found)
}

/// gh's config files as gh finds them, every candidate the environment
/// allows: `GH_CONFIG_DIR`, `XDG_CONFIG_HOME/gh`, `AppData/GitHub CLI` on
/// Windows, and `~/.config/gh`.
fn gh_config_files(ctx: Ctx<'_>) -> Result<Vec<PathBuf>, String> {
    let env = ctx
        .env
        .ok_or_else(|| "the environment the call runs with is not known".to_owned())?;
    let mut files: Vec<PathBuf> = env
        .dirs("GH_CONFIG_DIR")?
        .into_iter()
        .map(|dir| dir.join("config.yml"))
        .collect();
    files.extend(
        env.dirs("XDG_CONFIG_HOME")?
            .into_iter()
            .map(|dir| dir.join("gh").join("config.yml")),
    );
    if cfg!(windows) {
        for name in ["AppData", "APPDATA"] {
            files.extend(
                env.dirs(name)?
                    .into_iter()
                    .map(|dir| dir.join("GitHub CLI").join("config.yml")),
            );
        }
    }
    files.extend(
        env.homes()?
            .into_iter()
            .map(|dir| dir.join(".config").join("gh").join("config.yml")),
    );
    present(files, ctx.cwd)
}

/// gh's own commands and their short names; any other first word is an
/// alias from gh's config or an extension.
const GH_COMMANDS: &[&str] = &[
    "accessibility",
    "actions",
    "agent-task",
    "alias",
    "api",
    "attestation",
    "auth",
    "browse",
    "cache",
    "codespace",
    "copilot",
    "completion",
    "config",
    "cs",
    "ext",
    "extension",
    "extensions",
    "gist",
    "gpg-key",
    "help",
    "issue",
    "label",
    "org",
    "pr",
    "preview",
    "project",
    "release",
    "repo",
    "ruleset",
    "run",
    "search",
    "secret",
    "ssh-key",
    "status",
    "variable",
    "version",
    "workflow",
];

/// A `gh` command that is not built in. An alias in any config file gh may
/// read is judged as its expansion, the way gh expands it; a name no config
/// holds is an extension, which runs as its own program. A shell alias,
/// and config the guard cannot find or read, are refused.
fn gh_alias(args: &[String], sub: &[String], ctx: Ctx<'_>, depth: usize) -> Option<String> {
    let name = sub.first()?;
    let shown = crate::git::display_text(name);
    let unsure = |why: &str| {
        Some(format!(
            "gh {shown} may be an alias, and {why}, so the guard cannot tell whether it publishes a release"
        ))
    };
    if depth > 0 {
        return unsure("an alias that expands to another alias is not read");
    }
    let files = match gh_config_files(ctx) {
        Ok(files) => files,
        Err(why) => return unsure(&why),
    };
    for file in files {
        let text = match std::fs::read_to_string(&file) {
            Ok(text) => text,
            Err(e) => {
                return unsure(&format!(
                    "gh's config {} cannot be read ({e})",
                    file.display()
                ))
            }
        };
        let Ok(config) = serde_yaml::from_str::<serde_yaml::Value>(&text) else {
            return unsure(&format!("gh's config {} cannot be parsed", file.display()));
        };
        let expansion = match config.get("aliases").and_then(|a| a.get(name.as_str())) {
            None => continue,
            Some(serde_yaml::Value::String(text)) => text.clone(),
            Some(_) => return unsure("its alias is not text"),
        };
        if let Some(why) = gh_expansion(args, sub, &expansion, ctx, depth) {
            return Some(why);
        }
    }
    None
}

/// Judge the `gh` call `sub` names as the alias `expansion`.
fn gh_expansion(
    args: &[String],
    sub: &[String],
    expansion: &str,
    ctx: Ctx<'_>,
    depth: usize,
) -> Option<String> {
    let shown = crate::git::display_text(&sub[0]);
    if expansion.starts_with('!') {
        return Some(format!(
            "gh {shown} runs the shell alias `{}`, which the guard cannot judge",
            crate::git::display_text(expansion)
        ));
    }
    // gh substitutes `$1`, `$2` and so on when the expansion holds a `$`,
    // and otherwise appends the arguments; a placeholder left over is an
    // error, so nothing runs.
    let mut text = expansion.to_owned();
    let mut extra = Vec::new();
    for (index, arg) in sub[1..].iter().enumerate() {
        if text.contains('$') {
            text = text.replace(&format!("${}", index + 1), arg);
        } else {
            extra.push(arg.clone());
        }
    }
    if text
        .as_bytes()
        .windows(2)
        .any(|pair| pair[0] == b'$' && pair[1].is_ascii_digit())
    {
        return None;
    }
    let Some(mut words) = crate::hooks::git_guard::split_alias(&text) else {
        return Some(format!(
            "gh {shown} is an alias the guard cannot split into words, so it cannot tell whether it publishes a release"
        ));
    };
    words.extend(extra);
    let mut expanded = args[..args.len() - sub.len()].to_vec();
    expanded.extend(words.iter().cloned());
    gh_at(&expanded, &words, ctx, depth + 1)
}

/// The repository a call acts on when its path names none, or names it
/// with gh's `{owner}` and `{repo}` placeholders.
enum Target {
    /// gh's current repository: no `-R` and no `GH_REPO`.
    Current,
    /// The repository every `-R`, `--repo` and `GH_REPO` names.
    Named(String),
    /// Why the guard cannot tell which repository gh acts on.
    Unresolved(String),
    /// A client that does not expand placeholders.
    Literal,
}

/// The repository a `gh` call acts on: every `-R`, `--repo` and `GH_REPO`
/// must name the same one.
fn gh_target(args: &[String], gh_repo: Option<&str>) -> Target {
    let mut named = Vec::new();
    for (at, word) in args.iter().enumerate() {
        let (name, attached) = split_option(word, &["-R"]);
        if matches!(name.as_str(), "-R" | "--repo") {
            match attached.or_else(|| args.get(at + 1).cloned()) {
                Some(repo) => named.push(repo),
                None => return Target::Unresolved("gives `-R` with no repository".into()),
            }
        }
    }
    named.extend(gh_repo.map(str::to_owned));
    let key = |repo: &str| github_slug(repo).unwrap_or_else(|| repo.to_ascii_lowercase());
    let Some(first) = named.first() else {
        return Target::Current;
    };
    if named.iter().all(|repo| key(repo) == key(first)) {
        Target::Named(first.clone())
    } else {
        let shown: Vec<String> = named.iter().map(|r| crate::git::display_text(r)).collect();
        Target::Unresolved(format!(
            "names repositories {} through `-R` and `GH_REPO`, which disagree",
            shown.join(", ")
        ))
    }
}

/// The refusal for `gh` arguments, or `None` when the call is not a release
/// route. `args` are the words after `gh`; `sub` starts at its subcommand.
pub(crate) fn gh(args: &[String], sub: &[String], ctx: Ctx<'_>) -> Option<String> {
    gh_at(args, sub, ctx, 0)
}

/// [`gh`], `depth` aliases deep.
fn gh_at(args: &[String], sub: &[String], ctx: Ctx<'_>, depth: usize) -> Option<String> {
    let outer = ctx;
    let target = gh_target(args, ctx.gh_repo);
    let ctx = Ctx {
        target: &target,
        ..ctx
    };
    let action = sub.get(1).map(String::as_str);
    match (sub.first().map(String::as_str), action) {
        (Some("workflow"), Some("run")) => workflow_run(&sub[2..], ctx),
        (Some("workflow"), Some("enable")) => workflow_enable(&sub[2..], ctx),
        (Some(kind @ ("variable" | "secret")), Some(verb @ ("set" | "delete"))) => {
            store_command(kind, verb, &sub[2..], ctx)
        }
        (Some("api"), _) => api(&sub[1..], ctx),
        (Some(command), _) if !command.starts_with('-') && !GH_COMMANDS.contains(&command) => {
            gh_alias(args, sub, outer, depth)
        }
        _ => None,
    }
}

/// What an HTTP client option does, as far as the guard judges a write.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    /// Changes nothing the guard judges.
    Inert,
    /// Skips the client's default config file: `curl -q` when it comes
    /// first, `wget --no-config`.
    NoConfig,
    /// Sets the method.
    Method,
    /// Sends its value as the body; the method becomes POST.
    Data,
    /// Sends a body the guard does not read, with this method.
    Upload(&'static str),
    /// Names a destination.
    Url,
    Get,
    Head,
    Header,
    Globoff,
    /// Reads settings from a file or a command, which may set the method,
    /// body or destination.
    Config(&'static str),
    /// Sends the request somewhere other than its URL names.
    Elsewhere(&'static str),
    /// `wget --max-redirect`.
    MaxRedirect,
}

impl Role {
    /// Whether the role can make a call write.
    fn may_write(self) -> bool {
        matches!(
            self,
            Self::Method | Self::Data | Self::Upload(_) | Self::Config(_)
        )
    }
}

/// An option a client reads: its short letter, long name, whether it takes
/// a value, and its role.
type Spec = (Option<char>, &'static str, bool, Role);

const FROM_FILE: &str = "reads its options from a file";
const OTHER_HOST: &str = "sends the request to a host or path other than its URL names";
const REDIRECTS: &str = "follows redirects to a destination the guard cannot read";
const HOST_HEADER: &str = "sets the Host header or reads headers from a file";

/// The `curl` options the guard knows. Beside a write, any other option is
/// refused: it may change the method, body or destination.
const CURL_OPTIONS: &[Spec] = &[
    (Some('q'), "disable", false, Role::NoConfig),
    (Some('X'), "request", true, Role::Method),
    (Some('d'), "data", true, Role::Data),
    (None, "data-raw", true, Role::Data),
    (None, "data-binary", true, Role::Data),
    (None, "data-ascii", true, Role::Data),
    (None, "data-urlencode", true, Role::Data),
    (None, "json", true, Role::Data),
    (Some('F'), "form", true, Role::Upload("POST")),
    (None, "form-string", true, Role::Upload("POST")),
    (Some('T'), "upload-file", true, Role::Upload("PUT")),
    (None, "url", true, Role::Url),
    (Some('G'), "get", false, Role::Get),
    (Some('I'), "head", false, Role::Head),
    (Some('H'), "header", true, Role::Header),
    (Some('g'), "globoff", false, Role::Globoff),
    (Some('K'), "config", true, Role::Config(FROM_FILE)),
    (None, "connect-to", true, Role::Elsewhere(OTHER_HOST)),
    (None, "resolve", true, Role::Elsewhere(OTHER_HOST)),
    (None, "request-target", true, Role::Elsewhere(OTHER_HOST)),
    (Some('x'), "proxy", true, Role::Elsewhere(OTHER_HOST)),
    (None, "preproxy", true, Role::Elsewhere(OTHER_HOST)),
    (None, "doh-url", true, Role::Elsewhere(OTHER_HOST)),
    (None, "socks4", true, Role::Elsewhere(OTHER_HOST)),
    (None, "socks4a", true, Role::Elsewhere(OTHER_HOST)),
    (None, "socks5", true, Role::Elsewhere(OTHER_HOST)),
    (None, "socks5-hostname", true, Role::Elsewhere(OTHER_HOST)),
    (None, "unix-socket", true, Role::Elsewhere(OTHER_HOST)),
    (
        None,
        "abstract-unix-socket",
        true,
        Role::Elsewhere(OTHER_HOST),
    ),
    (None, "alt-svc", true, Role::Elsewhere(OTHER_HOST)),
    (Some('L'), "location", false, Role::Elsewhere(REDIRECTS)),
    (None, "location-trusted", false, Role::Elsewhere(REDIRECTS)),
    (Some('s'), "silent", false, Role::Inert),
    (Some('S'), "show-error", false, Role::Inert),
    (Some('f'), "fail", false, Role::Inert),
    (None, "fail-with-body", false, Role::Inert),
    (None, "fail-early", false, Role::Inert),
    (Some('v'), "verbose", false, Role::Inert),
    (Some('i'), "include", false, Role::Inert),
    (None, "show-headers", false, Role::Inert),
    (Some('k'), "insecure", false, Role::Inert),
    (None, "compressed", false, Role::Inert),
    (Some('N'), "no-buffer", false, Role::Inert),
    (Some('O'), "remote-name", false, Role::Inert),
    (None, "remote-name-all", false, Role::Inert),
    (Some('J'), "remote-header-name", false, Role::Inert),
    (None, "create-dirs", false, Role::Inert),
    (None, "no-progress-meter", false, Role::Inert),
    (Some('#'), "progress-bar", false, Role::Inert),
    (None, "retry-all-errors", false, Role::Inert),
    (None, "retry-connrefused", false, Role::Inert),
    (Some('4'), "ipv4", false, Role::Inert),
    (Some('6'), "ipv6", false, Role::Inert),
    (Some('0'), "http1.0", false, Role::Inert),
    (None, "http1.1", false, Role::Inert),
    (None, "http2", false, Role::Inert),
    (None, "http2-prior-knowledge", false, Role::Inert),
    (None, "http3", false, Role::Inert),
    (None, "tlsv1.2", false, Role::Inert),
    (None, "tlsv1.3", false, Role::Inert),
    (None, "ssl-reqd", false, Role::Inert),
    (None, "raw", false, Role::Inert),
    (Some('n'), "netrc", false, Role::Inert),
    (None, "netrc-optional", false, Role::Inert),
    (None, "path-as-is", false, Role::Inert),
    (None, "post301", false, Role::Inert),
    (None, "post302", false, Role::Inert),
    (None, "post303", false, Role::Inert),
    (None, "no-keepalive", false, Role::Inert),
    (None, "tcp-nodelay", false, Role::Inert),
    (Some('R'), "remote-time", false, Role::Inert),
    (Some('j'), "junk-session-cookies", false, Role::Inert),
    (None, "digest", false, Role::Inert),
    (None, "basic", false, Role::Inert),
    (None, "anyauth", false, Role::Inert),
    (Some('V'), "version", false, Role::Inert),
    (Some('o'), "output", true, Role::Inert),
    (None, "output-dir", true, Role::Inert),
    (Some('u'), "user", true, Role::Inert),
    (Some('A'), "user-agent", true, Role::Inert),
    (Some('e'), "referer", true, Role::Inert),
    (Some('b'), "cookie", true, Role::Inert),
    (Some('c'), "cookie-jar", true, Role::Inert),
    (Some('w'), "write-out", true, Role::Inert),
    (Some('m'), "max-time", true, Role::Inert),
    (None, "connect-timeout", true, Role::Inert),
    (Some('U'), "proxy-user", true, Role::Inert),
    (Some('E'), "cert", true, Role::Inert),
    (None, "cert-type", true, Role::Inert),
    (None, "key", true, Role::Inert),
    (None, "key-type", true, Role::Inert),
    (None, "cacert", true, Role::Inert),
    (None, "capath", true, Role::Inert),
    (None, "pinnedpubkey", true, Role::Inert),
    (None, "ciphers", true, Role::Inert),
    (None, "tls-max", true, Role::Inert),
    (Some('r'), "range", true, Role::Inert),
    (Some('C'), "continue-at", true, Role::Inert),
    (Some('D'), "dump-header", true, Role::Inert),
    (None, "trace", true, Role::Inert),
    (None, "trace-ascii", true, Role::Inert),
    (None, "stderr", true, Role::Inert),
    (None, "limit-rate", true, Role::Inert),
    (None, "max-filesize", true, Role::Inert),
    (None, "max-redirs", true, Role::Inert),
    (None, "interface", true, Role::Inert),
    (None, "retry", true, Role::Inert),
    (None, "retry-delay", true, Role::Inert),
    (None, "retry-max-time", true, Role::Inert),
    (Some('z'), "time-cond", true, Role::Inert),
    (None, "oauth2-bearer", true, Role::Inert),
    (None, "aws-sigv4", true, Role::Inert),
    (None, "netrc-file", true, Role::Inert),
    (None, "url-query", true, Role::Inert),
    (None, "variable", true, Role::Inert),
    (Some('Y'), "speed-limit", true, Role::Inert),
    (Some('y'), "speed-time", true, Role::Inert),
    (None, "keepalive-time", true, Role::Inert),
    (None, "expect100-timeout", true, Role::Inert),
    (None, "proto", true, Role::Inert),
    (None, "proto-redir", true, Role::Inert),
];

/// One option a client was given: how it was written, its role, its value,
/// and whether it was written `--expand-NAME`.
struct Given {
    shown: String,
    role: Role,
    value: Option<String>,
    expanded: bool,
}

/// A word of a client's arguments, as `read_options` reads it.
enum Word {
    Option(Given),
    Positional(String),
    /// An option the guard does not know; `true` when it may be a write
    /// option written short, as a client that takes a prefix reads it.
    Unknown(String, bool),
    /// `curl --next`: the words after it are another request.
    Next,
}

/// Read `args` against `specs`. Short options cluster, and a short option
/// that takes a value takes the rest of its word or the next word. `next`
/// words start another request, `inert` words are known flags written
/// whole, and `expand` reads `--expand-NAME` as `--NAME`.
fn read_options(
    args: &[String],
    specs: &[Spec],
    next: &[&str],
    inert: &[&str],
    expand: bool,
) -> Vec<Word> {
    let mut words = Vec::new();
    let mut at = 0;
    while let Some(word) = args.get(at) {
        at += 1;
        if next.contains(&word.as_str()) {
            words.push(Word::Next);
            continue;
        }
        if inert.contains(&word.as_str()) {
            continue;
        }
        if let Some(long) = word.strip_prefix("--") {
            let (name, attached) = match long.split_once('=') {
                Some((name, value)) => (name, Some(value.to_owned())),
                None => (long, None),
            };
            let (base, expanded) = match name.strip_prefix("expand-").filter(|_| expand) {
                Some(base) => (base, true),
                None => (name, false),
            };
            match specs.iter().find(|spec| spec.1 == base) {
                Some(&(_, _, takes, role))
                    if (takes || (!expanded && attached.is_none())) && !base.is_empty() =>
                {
                    let value = if takes {
                        attached.or_else(|| {
                            at += 1;
                            args.get(at - 1).cloned()
                        })
                    } else {
                        None
                    };
                    words.push(Word::Option(Given {
                        shown: format!("--{name}"),
                        role,
                        value,
                        expanded,
                    }));
                }
                _ => {
                    let negated = base.strip_prefix("no-").is_some_and(|flag| {
                        specs
                            .iter()
                            .any(|spec| spec.1 == flag && !spec.2 && spec.3 == Role::Inert)
                    });
                    if negated && !expanded && attached.is_none() {
                        continue;
                    }
                    let may_write = base.len() >= 2
                        && specs
                            .iter()
                            .any(|spec| spec.3.may_write() && spec.1.starts_with(base));
                    words.push(Word::Unknown(word.clone(), may_write));
                }
            }
            continue;
        }
        if word.len() > 1 && word.starts_with('-') {
            let letters: Vec<char> = word.chars().skip(1).collect();
            for (index, letter) in letters.iter().enumerate() {
                let Some(&(_, _, takes, role)) = specs.iter().find(|spec| spec.0 == Some(*letter))
                else {
                    words.push(Word::Unknown(word.clone(), false));
                    break;
                };
                let value = if takes {
                    let rest: String = letters[index + 1..].iter().collect();
                    if rest.is_empty() {
                        at += 1;
                        args.get(at - 1).cloned()
                    } else {
                        Some(rest)
                    }
                } else {
                    None
                };
                words.push(Word::Option(Given {
                    shown: format!("-{letter}"),
                    role,
                    value,
                    expanded: false,
                }));
                if takes {
                    break;
                }
            }
            continue;
        }
        words.push(Word::Positional(word.clone()));
    }
    words
}

/// Whether a header value sets the Host header or reads headers from a
/// file.
fn moves_host(header: Option<&str>) -> bool {
    header.is_some_and(|h| {
        h.starts_with('@') || h.get(..5).is_some_and(|h| h.eq_ignore_ascii_case("host:"))
    })
}

/// One request of a `curl` command; `--next` starts another, with the
/// options reset.
struct CurlRequest {
    method: Option<String>,
    implied: Option<&'static str>,
    get: bool,
    glob: bool,
    body: Body,
    urls: Vec<String>,
}

impl Default for CurlRequest {
    fn default() -> Self {
        Self {
            method: None,
            implied: None,
            get: false,
            glob: true,
            body: Body::default(),
            urls: Vec::new(),
        }
    }
}

impl CurlRequest {
    fn method(&self) -> String {
        match (&self.method, self.get) {
            (Some(method), _) => method.clone(),
            (None, true) => "GET".into(),
            (None, false) => self.implied.unwrap_or("GET").into(),
        }
    }
}

/// What a `curl` command does as far as the guard judges a write: its
/// requests, and why the guard cannot read them all.
#[derive(Default)]
struct CurlCall {
    requests: Vec<CurlRequest>,
    /// Some option in some request writes, or may.
    writes: bool,
    unknown: Vec<String>,
    elsewhere: Option<&'static str>,
    unreadable: Option<String>,
}

fn read_curl(args: &[String]) -> CurlCall {
    let mut call = CurlCall {
        requests: vec![CurlRequest::default()],
        ..CurlCall::default()
    };
    for word in read_options(args, CURL_OPTIONS, &["--next", "-:"], &[], true) {
        if let Word::Next = word {
            call.requests.push(CurlRequest::default());
            continue;
        }
        let Some(request) = call.requests.last_mut() else {
            continue;
        };
        let given = match word {
            Word::Next => continue,
            Word::Positional(url) => {
                request.urls.push(url);
                continue;
            }
            Word::Unknown(word, may_write) => {
                call.writes |= may_write;
                call.unknown.push(word);
                continue;
            }
            Word::Option(given) => given,
        };
        if given.expanded
            && given.role != Role::Inert
            && given.value.as_deref().is_some_and(|v| v.contains("{{"))
        {
            call.writes |= given.role == Role::Method;
            call.unreadable.get_or_insert_with(|| {
                format!(
                    "expands a variable in `{}`, which the guard cannot read",
                    given.shown
                )
            });
        }
        match given.role {
            Role::Inert | Role::NoConfig | Role::MaxRedirect => {}
            Role::Method => {
                let method = given.value.unwrap_or_default().to_ascii_uppercase();
                call.writes |= writes(&method);
                request.method = Some(method);
            }
            Role::Data => {
                call.writes = true;
                request.implied = Some("POST");
                request.body.text(given.value);
            }
            Role::Upload(method) => {
                call.writes = true;
                request.implied = Some(method);
                request.body.unknown = true;
            }
            Role::Url => request.urls.extend(given.value),
            Role::Get => request.get = true,
            Role::Head => request.method = Some("HEAD".into()),
            Role::Header => {
                if moves_host(given.value.as_deref()) {
                    call.elsewhere = Some(HOST_HEADER);
                }
            }
            Role::Globoff => request.glob = false,
            Role::Config(why) => {
                call.writes = true;
                call.elsewhere = Some(why);
            }
            Role::Elsewhere(why) => call.elsewhere = Some(why),
        }
    }
    call
}

/// Why a `curl` call without `-q` or `--disable` first is refused. curl
/// then reads a default config file, which can add a URL, a method or data,
/// so the arguments alone cannot rule out a write to GitHub: it reads
/// `$CURL_HOME/.curlrc`, `$XDG_CONFIG_HOME/curlrc`, `$HOME/.curlrc` and the
/// account home's `.curlrc`. `None` when none of them exists. A file the
/// guard cannot check counts as present. On Windows curl also reads a file
/// beside `curl.exe`, which is not checked, so every such call is refused
/// there. The decision is kept here alone so the scope can change.
fn curlrc_refusal(ctx: Ctx<'_>) -> Option<String> {
    let advice = "put `-q` first so curl skips it";
    let found = (|| -> Result<Vec<PathBuf>, String> {
        if cfg!(windows) {
            return Err(
                "curl on Windows also reads a config file beside `curl.exe`, which the guard does not check"
                    .into(),
            );
        }
        let env = ctx
            .env
            .ok_or_else(|| "the environment the call runs with is not known".to_owned())?;
        let mut files: Vec<PathBuf> = env
            .dirs("CURL_HOME")?
            .into_iter()
            .map(|dir| dir.join(".curlrc"))
            .collect();
        files.extend(
            env.dirs("XDG_CONFIG_HOME")?
                .into_iter()
                .map(|dir| dir.join("curlrc")),
        );
        files.extend(env.homes()?.into_iter().map(|dir| dir.join(".curlrc")));
        present(files, ctx.cwd)
    })();
    match found {
        Ok(files) => files.first().map(|file| {
            format!(
                "curl may read the default config file {}, which can add a URL, a method or data; {advice}",
                file.display()
            )
        }),
        Err(why) => Some(format!(
            "curl may read a default config file, and {why}; {advice}"
        )),
    }
}

/// The refusal for a `curl` call. A call without `-q` first is refused
/// when a default config file may be read ([`curlrc_refusal`]). A call
/// with a write option is refused when it writes to a release route, or
/// when its destination cannot be ruled out as GitHub: an option the guard
/// does not know, a destination that does not parse, a redirect, a proxy
/// or a remapped host. Each `--next` request is judged with its own method.
/// A write to a parsed host other than GitHub's is allowed.
pub(crate) fn curl(args: &[String], ctx: Ctx<'_>) -> Option<String> {
    let call = read_curl(args);
    // A release route the guard can read names itself first.
    if call.writes {
        for request in &call.requests {
            let method = request.method();
            if writes(&method) {
                let form = format!("curl -X {method}");
                if let Some(why) = request
                    .urls
                    .iter()
                    .find_map(|url| route_of(&form, url, request.glob, &method, &request.body, ctx))
                {
                    return Some(why);
                }
            }
        }
    }
    if !matches!(args.first().map(String::as_str), Some("-q" | "--disable")) {
        if let Some(why) = curlrc_refusal(ctx) {
            return Some(why);
        }
    }
    if !call.writes {
        return None;
    }
    let refuse = |why: &str| {
        Some(format!(
            "curl writes and {why}, so the guard cannot rule out a write to a GitHub release route"
        ))
    };
    if let Some(why) = call.elsewhere {
        return refuse(why);
    }
    if let Some(word) = call.unknown.first() {
        return refuse(&format!(
            "uses `{}`, an option the guard does not know, which may change the method, body or destination",
            crate::git::display_text(word)
        ));
    }
    if let Some(why) = &call.unreadable {
        return refuse(why);
    }
    for request in &call.requests {
        for url in &request.urls {
            if url.starts_with('@') {
                return refuse("reads its URLs from a file");
            }
            if let Err(why) = destination(url, request.glob) {
                return Some(format!("curl {why}"));
            }
        }
    }
    None
}

/// The refusal for a write by an HTTP client to `url`.
fn route_of(
    form: &str,
    url: &str,
    glob: bool,
    method: &str,
    body: &Body,
    ctx: Ctx<'_>,
) -> Option<String> {
    match destination(url, glob) {
        Err(why) => Some(format!("{form} {why}")),
        Ok(Dest::Other) => None,
        Ok(Dest::Api(path)) => api_route(form, method, &path, body, ctx),
    }
}

const WGET_SETTINGS: &str = "reads its URLs or settings from a file or command";

/// The `wget` options the guard knows. Beside a write, any other option is
/// refused, and so is a prefix of a write option, which wget reads as it.
const WGET_OPTIONS: &[Spec] = &[
    (None, "no-config", false, Role::NoConfig),
    (None, "max-redirect", true, Role::MaxRedirect),
    (None, "method", true, Role::Method),
    (None, "post-data", true, Role::Data),
    (None, "body-data", true, Role::Data),
    (None, "post-file", true, Role::Upload("POST")),
    (None, "body-file", true, Role::Upload("POST")),
    (None, "header", true, Role::Header),
    (Some('e'), "execute", true, Role::Config(WGET_SETTINGS)),
    (None, "config", true, Role::Config(WGET_SETTINGS)),
    (Some('i'), "input-file", true, Role::Config(WGET_SETTINGS)),
    (Some('B'), "base", true, Role::Elsewhere(OTHER_HOST)),
    (None, "bind-address", true, Role::Inert),
    (Some('q'), "quiet", false, Role::Inert),
    (Some('v'), "verbose", false, Role::Inert),
    (None, "no-verbose", false, Role::Inert),
    (Some('S'), "server-response", false, Role::Inert),
    (Some('c'), "continue", false, Role::Inert),
    (Some('N'), "timestamping", false, Role::Inert),
    (Some('d'), "debug", false, Role::Inert),
    (Some('V'), "version", false, Role::Inert),
    (Some('4'), "inet4-only", false, Role::Inert),
    (Some('6'), "inet6-only", false, Role::Inert),
    (None, "no-check-certificate", false, Role::Inert),
    (None, "no-cookies", false, Role::Inert),
    (None, "keep-session-cookies", false, Role::Inert),
    (None, "ignore-length", false, Role::Inert),
    (None, "no-cache", false, Role::Inert),
    (None, "save-headers", false, Role::Inert),
    (None, "content-disposition", false, Role::Inert),
    (None, "content-on-error", false, Role::Inert),
    (None, "auth-no-challenge", false, Role::Inert),
    (None, "https-only", false, Role::Inert),
    (None, "retry-connrefused", false, Role::Inert),
    (None, "no-proxy", false, Role::Inert),
    (None, "no-clobber", false, Role::Inert),
    (None, "no-dns-cache", false, Role::Inert),
    (None, "no-hsts", false, Role::Inert),
    (None, "show-progress", false, Role::Inert),
    (Some('O'), "output-document", true, Role::Inert),
    (Some('o'), "output-file", true, Role::Inert),
    (Some('a'), "append-output", true, Role::Inert),
    (Some('P'), "directory-prefix", true, Role::Inert),
    (Some('t'), "tries", true, Role::Inert),
    (Some('T'), "timeout", true, Role::Inert),
    (Some('w'), "wait", true, Role::Inert),
    (Some('U'), "user-agent", true, Role::Inert),
    (Some('Q'), "quota", true, Role::Inert),
    (None, "user", true, Role::Inert),
    (None, "password", true, Role::Inert),
    (None, "http-user", true, Role::Inert),
    (None, "http-password", true, Role::Inert),
    (None, "referer", true, Role::Inert),
    (None, "load-cookies", true, Role::Inert),
    (None, "save-cookies", true, Role::Inert),
    (None, "ca-certificate", true, Role::Inert),
    (None, "ca-directory", true, Role::Inert),
    (None, "certificate", true, Role::Inert),
    (None, "private-key", true, Role::Inert),
    (None, "secure-protocol", true, Role::Inert),
    (None, "read-timeout", true, Role::Inert),
    (None, "connect-timeout", true, Role::Inert),
    (None, "dns-timeout", true, Role::Inert),
    (None, "waitretry", true, Role::Inert),
    (None, "limit-rate", true, Role::Inert),
    (None, "compression", true, Role::Inert),
    (None, "progress", true, Role::Inert),
];

/// `wget`'s flags that start with `-n` and are read as one word.
const WGET_WORDS: &[&str] = &["-nv", "-nc", "-nd", "-nH", "-np"];

/// The refusal for a `wget` call. A call without `--no-config` is refused:
/// wget then reads a system `wgetrc` at a path built into it, which the
/// guard cannot check, and `WGETRC` or `~/.wgetrc`, any of which can set a
/// URL, a method or a body. A call with a write option is refused when it
/// writes to a release route, or when its destination cannot be ruled out:
/// an option the guard does not know, a URL that does not parse, or a
/// redirect, which wget follows unless `--max-redirect=0` is given.
fn wget(args: &[String], ctx: Ctx<'_>) -> Option<String> {
    let mut method: Option<String> = None;
    let mut implied: Option<&'static str> = None;
    let mut body = Body::default();
    let mut urls = Vec::new();
    let mut signal = false;
    let mut unknown = Vec::new();
    let mut elsewhere: Option<&'static str> = None;
    let mut no_config = false;
    let mut no_redirect = false;
    for word in read_options(args, WGET_OPTIONS, &[], WGET_WORDS, false) {
        let given = match word {
            Word::Next => continue,
            Word::Positional(url) => {
                urls.push(url);
                continue;
            }
            Word::Unknown(word, may_write) => {
                signal |= may_write;
                unknown.push(word);
                continue;
            }
            Word::Option(given) => given,
        };
        match given.role {
            Role::NoConfig => no_config = true,
            Role::MaxRedirect => no_redirect = given.value.as_deref().map(str::trim) == Some("0"),
            Role::Method => {
                let value = given.value.unwrap_or_default().to_ascii_uppercase();
                signal |= writes(&value);
                method = Some(value);
            }
            Role::Data => {
                signal = true;
                implied = Some("POST");
                body.text(given.value);
            }
            Role::Upload(post) => {
                signal = true;
                implied = Some(post);
                body.unknown = true;
            }
            Role::Header => {
                if moves_host(given.value.as_deref()) {
                    elsewhere = Some(HOST_HEADER);
                }
            }
            Role::Config(why) => {
                signal = true;
                elsewhere = Some(why);
            }
            Role::Elsewhere(why) => elsewhere = Some(why),
            Role::Inert | Role::Url | Role::Get | Role::Head | Role::Globoff => {}
        }
    }
    let method = method.unwrap_or_else(|| implied.unwrap_or("GET").into());
    if signal && writes(&method) {
        let form = format!("wget --method {method}");
        if let Some(why) = urls
            .iter()
            .find_map(|url| route_of(&form, url, false, &method, &body, ctx))
        {
            return Some(why);
        }
    }
    if !no_config {
        return Some(
            "wget may read a system `wgetrc` at a path built into it, which the guard cannot check, or `WGETRC` or `~/.wgetrc`, any of which can set a URL, a method or a body; pass `--no-config` so wget skips them"
                .into(),
        );
    }
    if !signal {
        return None;
    }
    let refuse = |why: &str| {
        Some(format!(
            "wget writes and {why}, so the guard cannot rule out a write to a GitHub release route"
        ))
    };
    if let Some(why) = elsewhere {
        return refuse(why);
    }
    if let Some(word) = unknown.first() {
        return refuse(&format!(
            "uses `{}`, an option the guard does not know, which may change the method, body or destination",
            crate::git::display_text(word)
        ));
    }
    if !no_redirect {
        return refuse(
            "follows redirects, as wget does unless `--max-redirect=0` is given, to a destination the guard cannot read",
        );
    }
    for url in &urls {
        if let Err(why) = destination(url, false) {
            return Some(format!("wget {why}"));
        }
    }
    None
}

const SESSION: &str = "reads headers from a session file";

/// The `HTTPie` and xh options the guard knows. Beside a write, any other
/// option is refused, and so is a prefix of `--raw`, which `HTTPie` reads
/// as it.
const HTTPIE_OPTIONS: &[Spec] = &[
    (None, "raw", true, Role::Data),
    (Some('F'), "follow", false, Role::Elsewhere(REDIRECTS)),
    (None, "session", true, Role::Elsewhere(SESSION)),
    (None, "session-read-only", true, Role::Elsewhere(SESSION)),
    (None, "proxy", true, Role::Elsewhere(OTHER_HOST)),
    (None, "resolve", true, Role::Elsewhere(OTHER_HOST)),
    (None, "unix-socket", true, Role::Elsewhere(OTHER_HOST)),
    (Some('v'), "verbose", false, Role::Inert),
    (Some('h'), "headers", false, Role::Inert),
    (Some('b'), "body", false, Role::Inert),
    (Some('m'), "meta", false, Role::Inert),
    (Some('j'), "json", false, Role::Inert),
    (Some('f'), "form", false, Role::Inert),
    (None, "multipart", false, Role::Inert),
    (Some('d'), "download", false, Role::Inert),
    (Some('c'), "continue", false, Role::Inert),
    (Some('q'), "quiet", false, Role::Inert),
    (Some('S'), "stream", false, Role::Inert),
    (None, "check-status", false, Role::Inert),
    (None, "ignore-stdin", false, Role::Inert),
    (Some('I'), "ignore-stdin", false, Role::Inert),
    (None, "offline", false, Role::Inert),
    (None, "all", false, Role::Inert),
    (Some('x'), "compress", false, Role::Inert),
    (None, "chunked", false, Role::Inert),
    (None, "sorted", false, Role::Inert),
    (None, "unsorted", false, Role::Inert),
    (None, "path-as-is", false, Role::Inert),
    (None, "ignore-netrc", false, Role::Inert),
    (None, "https", false, Role::Inert),
    (Some('4'), "ipv4", false, Role::Inert),
    (Some('6'), "ipv6", false, Role::Inert),
    (Some('a'), "auth", true, Role::Inert),
    (Some('A'), "auth-type", true, Role::Inert),
    (None, "bearer", true, Role::Inert),
    (Some('o'), "output", true, Role::Inert),
    (None, "cert", true, Role::Inert),
    (None, "cert-key", true, Role::Inert),
    (None, "cert-key-pass", true, Role::Inert),
    (None, "verify", true, Role::Inert),
    (None, "timeout", true, Role::Inert),
    (None, "pretty", true, Role::Inert),
    (Some('s'), "style", true, Role::Inert),
    (Some('p'), "print", true, Role::Inert),
    (Some('P'), "history-print", true, Role::Inert),
    (None, "max-redirects", true, Role::Inert),
    (None, "default-scheme", true, Role::Inert),
    (None, "format-options", true, Role::Inert),
    (None, "response-charset", true, Role::Inert),
    (None, "response-mime", true, Role::Inert),
    (None, "ssl", true, Role::Inert),
    (None, "ciphers", true, Role::Inert),
    (None, "boundary", true, Role::Inert),
    (None, "http-version", true, Role::Inert),
    (None, "interface", true, Role::Inert),
];

/// Why an `HTTPie` or xh call is refused for its config file. Both read
/// `default_options` from a `config.json`, which can add a URL, a method
/// or a body, and neither has a flag that skips it. `HTTPie` reads
/// `$HTTPIE_CONFIG_DIR`, `$XDG_CONFIG_HOME/httpie`, `~/.config/httpie` and
/// the older `~/.httpie`; xh reads `$XH_CONFIG_DIR`, `$XDG_CONFIG_HOME/xh`,
/// `~/.config/xh`, and on macOS `~/Library/Application Support/xh`. `None`
/// when none of them exists; a file the guard cannot check counts as
/// present.
fn client_config_refusal(name: &str, ctx: Ctx<'_>) -> Option<String> {
    let httpie = matches!(name, "http" | "https");
    let (variable, dir) = if httpie {
        ("HTTPIE_CONFIG_DIR", "httpie")
    } else {
        ("XH_CONFIG_DIR", "xh")
    };
    let found = (|| -> Result<Vec<PathBuf>, String> {
        let env = ctx
            .env
            .ok_or_else(|| "the environment the call runs with is not known".to_owned())?;
        let mut files: Vec<PathBuf> = env
            .dirs(variable)?
            .into_iter()
            .map(|d| d.join("config.json"))
            .collect();
        files.extend(
            env.dirs("XDG_CONFIG_HOME")?
                .into_iter()
                .map(|d| d.join(dir).join("config.json")),
        );
        for name in ["AppData", "APPDATA"] {
            files.extend(
                env.dirs(name)?
                    .into_iter()
                    .map(|d| d.join(dir).join("config.json")),
            );
        }
        for home in env.homes()? {
            files.push(home.join(".config").join(dir).join("config.json"));
            if httpie {
                files.push(home.join(".httpie").join("config.json"));
            } else {
                files.push(
                    home.join("Library")
                        .join("Application Support")
                        .join(dir)
                        .join("config.json"),
                );
            }
        }
        present(files, ctx.cwd)
    })();
    let advice = format!(
        "{name} has no flag that skips it, and `{variable}` only moves it; remove the file or send the request with `curl -q`"
    );
    match found {
        Ok(files) => files.first().map(|file| {
            format!(
                "{name} may read the config file {}, whose default options can add a URL, a method or a body; {advice}",
                file.display()
            )
        }),
        Err(why) => Some(format!(
            "{name} may read a config file whose default options can add a URL, a method or a body, and {why}; {advice}"
        )),
    }
}

/// A JSON string value without its quotes; other JSON stays as written.
fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .unwrap_or(value)
}

/// The refusal for a `wget`, `HTTPie` (`http`, `https`) or xh (`xh`, `xhs`)
/// call. An `HTTPie` or xh call is refused when a config file may add to
/// it ([`client_config_refusal`]); a write is refused when it reaches a
/// release route or its destination cannot be ruled out as GitHub.
pub(crate) fn http_client(name: &str, args: &[String], ctx: Ctx<'_>) -> Option<String> {
    if name == "wget" {
        return wget(args, ctx);
    }
    let mut implied = false;
    let mut redirected: Option<&'static str> = None;
    let mut unknown = Vec::new();
    let mut body = Body::default();
    let mut positional = Vec::new();
    for word in read_options(args, HTTPIE_OPTIONS, &[], &[], false) {
        match word {
            Word::Next => {}
            Word::Positional(word) => positional.push(word),
            Word::Unknown(word, may_write) => {
                implied |= may_write;
                unknown.push(word);
            }
            Word::Option(given) => match given.role {
                Role::Data => {
                    implied = true;
                    body.text(given.value);
                }
                Role::Elsewhere(why) => redirected = Some(why),
                _ => {}
            },
        }
    }
    let mut positional = positional.into_iter();
    // HTTPie and xh read a first word of letters as the method when a URL
    // follows it.
    let mut method: Option<String> = None;
    let url = match positional.next() {
        Some(word) if word.bytes().all(|b| b.is_ascii_alphabetic()) => match positional.next() {
            Some(url) => {
                method = Some(word);
                Some(url)
            }
            None => Some(word),
        },
        other => other,
    };
    implied |= httpie_items(positional, &mut body, &mut redirected);
    let method = method.map_or_else(
        || if implied { "POST".into() } else { "GET".into() },
        |m| m.to_ascii_uppercase(),
    );
    let form = format!("{name} {method}");
    let write = writes(&method);
    if write {
        if let Some(why) = url
            .as_deref()
            .and_then(|url| route_of(&form, url, false, &method, &body, ctx))
        {
            return Some(why);
        }
    }
    if let Some(why) = client_config_refusal(name, ctx) {
        return Some(why);
    }
    if !write {
        return None;
    }
    let refuse = |why: &str| {
        Some(format!(
            "{form} {why}, so the guard cannot rule out a write to a GitHub release route"
        ))
    };
    if let Some(why) = redirected {
        return refuse(why);
    }
    if let Some(word) = unknown.first() {
        return refuse(&format!(
            "uses `{}`, an option the guard does not know, which may change the method, body or destination",
            crate::git::display_text(word)
        ));
    }
    match url.as_deref().map(|url| destination(url, false)) {
        Some(Err(why)) => Some(format!("{form} {why}")),
        _ => None,
    }
}

/// Read `HTTPie` and xh request items into `body`; whether any sends a body.
/// A `Host` header item sets `redirected`.
fn httpie_items(
    items: impl Iterator<Item = String>,
    body: &mut Body,
    redirected: &mut Option<&'static str>,
) -> bool {
    let mut implied = false;
    for item in items {
        if item
            .split_once(':')
            .is_some_and(|(key, rest)| key.eq_ignore_ascii_case("host") && !rest.starts_with('='))
        {
            *redirected = Some("sets the Host header");
        } else if item.contains(":=@")
            || item.contains("=@")
            || item.contains('@') && !item.contains('=')
        {
            implied = true;
            body.unknown = true;
        } else if let Some((key, value)) = item.split_once(":=") {
            implied = true;
            body.fields
                .push((key.to_owned(), unquote(value).to_owned()));
        } else if item.contains("==") {
            // A query parameter, not a body field.
        } else if let Some((key, value)) = item.split_once('=') {
            implied = true;
            body.fields.push((key.to_owned(), value.to_owned()));
        }
    }
    implied
}

fn writes(method: &str) -> bool {
    !matches!(method, "GET" | "HEAD")
}

/// The option name and an attached value: `-XPOST`, `-X=POST` and
/// `--method=POST` give `("-X", Some("POST"))` and the like.
fn split_option(word: &str, shorts: &[&str]) -> (String, Option<String>) {
    if let Some((name, value)) = word.split_once('=').filter(|_| word.starts_with("--")) {
        return (name.to_owned(), Some(value.to_owned()));
    }
    for short in shorts {
        if let Some(rest) = word.strip_prefix(short).filter(|r| !r.is_empty()) {
            let rest = rest.strip_prefix('=').unwrap_or(rest);
            return ((*short).to_owned(), Some(rest.to_owned()));
        }
    }
    (word.to_owned(), None)
}

/// A request body as far as the guard can read it.
#[derive(Default)]
struct Body {
    fields: Vec<(String, String)>,
    /// Some part comes from a file, stdin or text the guard cannot parse.
    unknown: bool,
}

enum Field {
    Known(String),
    Unknown,
    Absent,
}

impl Body {
    fn text(&mut self, text: Option<String>) {
        let Some(text) = text else { return };
        match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(serde_json::Value::Object(map)) if !text.starts_with('@') => {
                for (key, value) in map {
                    match value {
                        serde_json::Value::String(s) => self.fields.push((key, s)),
                        other => self.fields.push((key, other.to_string())),
                    }
                }
            }
            _ => self.unknown = true,
        }
    }

    fn field(&self, key: &str) -> Field {
        match self.fields.iter().rev().find(|(k, _)| k == key) {
            Some((_, value)) if !opaque(value) => Field::Known(value.clone()),
            Some(_) => Field::Unknown,
            None if self.unknown => Field::Unknown,
            None => Field::Absent,
        }
    }
}

/// Text holding a shell expansion the guard did not resolve.
fn opaque(text: &str) -> bool {
    text.contains('$') || text.contains('`')
}

fn workflow_run(rest: &[String], ctx: Ctx<'_>) -> Option<String> {
    let cwd = ctx.cwd;
    let mut reference = None;
    let mut selector = None;
    let mut at = 0;
    let mut options = true;
    while let Some(word) = rest.get(at) {
        at += 1;
        if options && word == "--" {
            options = false;
            continue;
        }
        if options && word.starts_with('-') {
            let (name, attached) = split_option(word, &["-R", "-r", "-f", "-F"]);
            let mut value = || {
                attached.clone().or_else(|| {
                    at += 1;
                    rest.get(at - 1).cloned()
                })
            };
            match name.as_str() {
                "-r" | "--ref" => reference = value(),
                "-R" | "--repo" | "-f" | "-F" | "--field" | "--raw-field" => {
                    value();
                }
                _ => {}
            }
            continue;
        }
        if selector.is_none() {
            selector = Some(word.clone());
        }
    }
    let form = "gh workflow run";
    let Some(selector) = selector else {
        return Some(format!(
            "{form} names no workflow, so gh would pick one interactively; name a workflow that is not a release workflow"
        ));
    };
    let authority = match Authority::read(cwd) {
        Ok(authority) => authority,
        Err(why) => return Some(unreadable(form, &why)),
    };
    if let Some(why) = authority.target(ctx.target) {
        return Some(format!("{form} {why}"));
    }
    dispatch(form, &authority, &selector, reference.as_deref(), false)
}

/// A `gh` subcommand's operands and its options.
type Parts = (Vec<String>, Vec<(String, Option<String>)>);

/// The operands and options of a `gh` subcommand whose value options are
/// `values`.
fn operands(rest: &[String], values: &[&str]) -> Parts {
    let mut positional = Vec::new();
    let mut options = Vec::new();
    let mut at = 0;
    let mut open = true;
    while let Some(word) = rest.get(at) {
        at += 1;
        if open && word == "--" {
            open = false;
            continue;
        }
        if open && word.starts_with('-') && word != "-" {
            let shorts: Vec<&str> = values.iter().copied().filter(|v| v.len() == 2).collect();
            let (name, attached) = split_option(word, &shorts);
            let value = if attached.is_some() {
                attached
            } else if values.contains(&name.as_str()) {
                at += 1;
                rest.get(at - 1).cloned()
            } else {
                None
            };
            options.push((name, value));
            continue;
        }
        positional.push(word.clone());
    }
    (positional, options)
}

/// `gh workflow enable`: re-enabling a release workflow lets its triggers
/// start release runs again.
fn workflow_enable(rest: &[String], ctx: Ctx<'_>) -> Option<String> {
    let form = "gh workflow enable";
    let (positional, _) = operands(rest, &["-R", "--repo"]);
    let Some(selector) = positional.first() else {
        return Some(format!(
            "{form} names no workflow, so gh would pick one interactively; name a workflow that is not a release workflow"
        ));
    };
    let authority = match Authority::read(ctx.cwd) {
        Ok(authority) => authority,
        Err(why) => return Some(unreadable(form, &why)),
    };
    if let Some(why) = authority.target(ctx.target) {
        return Some(format!("{form} {why}"));
    }
    enable(form, &authority, selector, false)
}

fn enable(form: &str, authority: &Authority, selector: &str, api: bool) -> Option<String> {
    match matching(form, authority, selector, api) {
        Err(why) => Some(why),
        Ok(flows) => flows.iter().find(|flow| flow.release.is_some()).map(|flow| {
            format!(
                "{form} enables release workflow {} ({}), whose triggers would start release runs again",
                crate::git::display_text(&flow.path),
                flow.release.as_deref().unwrap_or_default()
            )
        }),
    }
}

/// `gh variable set|delete` and `gh secret set|delete`.
fn store_command(kind: &str, verb: &str, rest: &[String], ctx: Ctx<'_>) -> Option<String> {
    let form = format!("gh {kind} {verb}");
    let values = [
        "-R",
        "--repo",
        "-e",
        "--env",
        "-o",
        "--org",
        "-v",
        "--visibility",
        "-r",
        "--repos",
        "-b",
        "--body",
        "-f",
        "--env-file",
        "-a",
        "--app",
    ];
    let (positional, options) = operands(rest, &values);
    let option = |names: &[&str]| {
        options
            .iter()
            .find(|(name, _)| names.contains(&name.as_str()))
            .map(|(_, value)| value.clone().unwrap_or_default())
    };
    // Dependabot and Codespaces secrets and a user's Codespaces secrets
    // never reach an Actions run.
    if option(&["-a", "--app"]).is_some_and(|app| !app.eq_ignore_ascii_case("actions"))
        || options
            .iter()
            .any(|(name, _)| name == "-u" || name == "--user")
    {
        return None;
    }
    let org = option(&["-o", "--org"]).is_some();
    let mut names: Vec<Field> = positional
        .iter()
        .take(1)
        .map(|name| {
            if opaque(name) {
                Field::Unknown
            } else {
                Field::Known(name.clone())
            }
        })
        .collect();
    if option(&["-f", "--env-file"]).is_some() || names.is_empty() {
        names.push(Field::Unknown);
    }
    let authority = match Authority::read(ctx.cwd) {
        Ok(authority) => authority,
        Err(why) => return Some(unreadable(&form, &why)),
    };
    if !org {
        if let Some(why) = authority.target(ctx.target) {
            return Some(format!("{form} {why}"));
        }
    }
    let store = if kind == "secret" {
        Store::Secrets
    } else {
        Store::Vars
    };
    store_write(store, &names, &authority).map(|why| format!("{form} {why}"))
}

fn unreadable(form: &str, why: &str) -> String {
    format!(
        "{form}: the workflows at the policy authority cannot be read ({why}), so the guard cannot tell a release workflow from another"
    )
}

/// Judge a dispatch of the workflow `selector` at `reference`; `api` is
/// whether the selector came from an API path, which takes only a file name
/// or an id.
fn dispatch(
    form: &str,
    authority: &Authority,
    selector: &str,
    reference: Option<&str>,
    api: bool,
) -> Option<String> {
    let matches = match matching(form, authority, selector, api) {
        Ok(matches) => matches,
        Err(why) => return Some(why),
    };
    if let Some(flow) = matches.iter().find(|flow| flow.release.is_some()) {
        return Some(format!(
            "{form} dispatches release workflow {} ({}); a dry run is refused too, because the guard does not evaluate the workflow's expressions",
            crate::git::display_text(&flow.path),
            flow.release.as_deref().unwrap_or_default()
        ));
    }
    let reference = reference?;
    if authority.is_default(reference) {
        return None;
    }
    let shown_ref = crate::git::display_text(reference);
    for flow in matches {
        match authority.flow_at(reference, &flow.path) {
            Ok(Some(at_ref)) if at_ref.release.is_none() => {}
            Ok(Some(at_ref)) => {
                return Some(format!(
                    "{form} runs {} as ref {shown_ref} holds it, where it is a release workflow ({})",
                    crate::git::display_text(&flow.path),
                    at_ref.release.unwrap_or_default()
                ))
            }
            Ok(None) | Err(_) => {
                return Some(format!(
                    "{form} runs {} as ref {shown_ref} holds it, which the guard cannot read here; fetch that branch so its tracking ref holds the file",
                    crate::git::display_text(&flow.path)
                ))
            }
        }
    }
    None
}

/// The workflows at the authority that `selector` names, or the refusal
/// when it names none the guard can map.
fn matching<'a>(
    form: &str,
    authority: &'a Authority,
    selector: &str,
    api: bool,
) -> Result<Vec<&'a Flow>, String> {
    let shown = crate::git::display_text(selector);
    if opaque(selector) {
        return Err(format!(
            "{form} names workflow {shown}, which the guard cannot resolve"
        ));
    }
    if selector.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!(
            "{form} names workflow id {shown}, which the guard cannot map to a workflow file"
        ));
    }
    let wanted = selector.trim_start_matches("./");
    let matches: Vec<&Flow> = authority
        .flows
        .iter()
        .filter(|flow| {
            flow.path == wanted
                || flow.path.rsplit('/').next() == Some(wanted)
                || (!api
                    && flow
                        .name
                        .as_deref()
                        .is_some_and(|name| name.eq_ignore_ascii_case(wanted)))
        })
        .collect();
    if matches.is_empty() {
        return Err(format!(
            "{form} names workflow {shown}, which matches no workflow file at the policy authority {}",
            authority.shown
        ));
    }
    Ok(matches)
}

fn api(rest: &[String], ctx: Ctx<'_>) -> Option<String> {
    let mut method = None;
    let mut body = Body::default();
    let mut implied = false;
    let mut endpoint = None;
    let mut at = 0;
    while let Some(word) = rest.get(at) {
        at += 1;
        if !word.starts_with('-') || word == "-" {
            if endpoint.is_none() {
                endpoint = Some(word.clone());
            }
            continue;
        }
        let (name, attached) =
            split_option(word, &["-X", "-f", "-F", "-H", "-q", "-t", "-p", "-R"]);
        let mut value = || {
            attached.clone().or_else(|| {
                at += 1;
                rest.get(at - 1).cloned()
            })
        };
        match name.as_str() {
            "-X" | "--method" => method = value(),
            "-f" | "--raw-field" | "-F" | "--field" => {
                implied = true;
                let typed = matches!(name.as_str(), "-F" | "--field");
                match value().as_deref().and_then(|v| v.split_once('=')) {
                    Some((_, v)) if typed && v.starts_with('@') => body.unknown = true,
                    Some((k, v)) => body.fields.push((k.to_owned(), v.to_owned())),
                    None => body.unknown = true,
                }
            }
            "--input" => {
                implied = true;
                value();
                body.unknown = true;
            }
            "-H" | "--header" | "-q" | "--jq" | "-t" | "--template" | "--hostname" | "-p"
            | "--preview" | "--cache" | "-R" | "--repo" => {
                value();
            }
            _ => {}
        }
    }
    let method = method.map_or_else(
        || if implied { "POST".into() } else { "GET".into() },
        |m| m.to_ascii_uppercase(),
    );
    if !writes(&method) {
        return None;
    }
    let endpoint = endpoint?;
    let form = format!("gh api -X {method}");
    // gh sends a URL as written and joins any other endpoint to its host.
    let path = if endpoint.contains("://") {
        match destination(&endpoint, false) {
            Err(why) => return Some(format!("{form} {why}")),
            Ok(Dest::Other) => return None,
            Ok(Dest::Api(path)) => path,
        }
    } else {
        let path = endpoint.split(['?', '#']).next().unwrap_or_default();
        if let Some(why) = dot_segment(path) {
            let shown = crate::git::display_text(&endpoint);
            return Some(format!("{form} {shown}: {why}"));
        }
        path.trim_start_matches('/').to_owned()
    };
    api_route(&form, &method, &path, &body, ctx)
}

/// GraphQL mutations that create or move a tag, approve or create a
/// deployment, or change an environment, ruleset or branch protection rule.
const MUTATIONS: &[&str] = &[
    "createRef",
    "updateRef",
    "updateRefs",
    "approveDeployments",
    "createEnvironment",
    "updateEnvironment",
    "deleteEnvironment",
    "createRepositoryRuleset",
    "updateRepositoryRuleset",
    "deleteRepositoryRuleset",
    "createDeployment",
    "createBranchProtectionRule",
    "updateBranchProtectionRule",
    "deleteBranchProtectionRule",
];

/// A GraphQL write: refused when its query runs a mutation in
/// [`MUTATIONS`], or when the guard cannot read the query from the body:
/// one held elsewhere, such as in the URL, is not read.
fn graphql(form: &str, body: &Body) -> Option<String> {
    match body.field("query") {
        Field::Known(query) => MUTATIONS
            .iter()
            .find(|name| query.contains(**name))
            .map(|name| format!("{form} graphql runs the {name} mutation")),
        Field::Unknown | Field::Absent => Some(format!(
            "{form} graphql sends a query the guard cannot read from its body, which may publish or approve a release"
        )),
    }
}

/// Where a request goes, as the guard reads its URL.
enum Dest {
    /// A GitHub API host, with the API path: no host, leading slash or
    /// query.
    Api(String),
    /// Another host.
    Other,
}

/// Read `url` as an HTTP client does: a scheme (`http` when none is
/// given), userinfo, host, port and path. The host is compared lower case
/// and without a trailing dot, whatever userinfo and port it carries. A URL
/// that clients could read differently is an error, which a write refuses:
/// a shell expansion or a byte outside printable ASCII in the scheme or
/// host, more than one `@`, a port that is not a number, a dot segment in
/// the path, or, when `glob` (curl without `--globoff`), braces or
/// brackets, which curl expands.
fn destination(url: &str, glob: bool) -> Result<Dest, String> {
    let shown = crate::git::display_text(url);
    let bad = |why: &str| {
        Err(format!(
            "sends to {shown}, which the guard cannot read: {why}"
        ))
    };
    if url.bytes().any(|b| !b.is_ascii_graphic()) || url.contains('\\') {
        return bad("it holds a space, a backslash or a byte outside printable ASCII");
    }
    if glob && url.contains(['{', '}', '[', ']']) {
        return bad("curl expands the braces or brackets in it");
    }
    let (scheme, rest) = if let Some(at) = url.find("://") {
        let scheme = &url[..at];
        if !scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            || !scheme
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"+-.".contains(&b))
        {
            return bad("its scheme is not one the guard reads");
        }
        (scheme.to_ascii_lowercase(), &url[at + 3..])
    } else if url.starts_with(':') {
        // HTTPie and xh read `:port/path` as localhost.
        return Ok(Dest::Other);
    } else {
        let head = &url[..url.find(['/', '?', '#']).unwrap_or(url.len())];
        if head
            .split_once(':')
            .is_some_and(|(_, port)| !port.bytes().all(|b| b.is_ascii_digit()))
        {
            return bad("it names a scheme without `//`, which clients read differently");
        }
        ("http".to_owned(), url)
    };
    if !matches!(scheme.as_str(), "http" | "https") {
        return Ok(Dest::Other);
    }
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(end);
    if authority.matches('@').count() > 1 {
        return bad("its userinfo holds an `@`, which clients split differently");
    }
    let host_port = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    if host_port.starts_with('[') {
        // An IPv6 address names no GitHub host.
        return Ok(Dest::Other);
    }
    let Some(host) = host_name(host_port) else {
        return bad("its host or port is not one the guard reads");
    };
    let path = tail.split(['?', '#']).next().unwrap_or_default();
    if let Some(why) = dot_segment(path) {
        return bad(why);
    }
    let path = path.trim_start_matches('/');
    let api = if matches!(host.as_str(), "api.github.com" | "uploads.github.com") {
        Some(path)
    } else if path == "api/graphql" {
        Some("graphql")
    } else {
        path.strip_prefix("api/v3/")
            .or_else(|| path.strip_prefix("api/uploads/"))
    };
    Ok(api.map_or(Dest::Other, |path| Dest::Api(path.to_owned())))
}

/// A host as clients compare it: without its port, lower case and without a
/// trailing dot. `None` for an empty host, a port that is not a number, or
/// a character a host name does not hold, a `%` or shell expansion
/// included.
fn host_name(host_port: &str) -> Option<String> {
    let (host, port) = host_port.rsplit_once(':').unwrap_or((host_port, ""));
    let host = host.strip_suffix('.').unwrap_or(host);
    (!host.is_empty()
        && port.bytes().all(|b| b.is_ascii_digit())
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._".contains(&b)))
    .then(|| host.to_ascii_lowercase())
}

/// Why `path` cannot be read: a `.` or `..` segment, raw or percent-encoded,
/// which one client resolves and another sends as written.
fn dot_segment(path: &str) -> Option<&'static str> {
    path.split('/')
        .any(|seg| {
            matches!(
                seg.to_ascii_lowercase().replace("%2e", ".").as_str(),
                "." | ".."
            )
        })
        .then_some("its path has a `.` or `..` segment, which clients resolve differently")
}

/// The refusal for a write to the API `path`: a GraphQL path is judged by
/// its query, and every path by its route.
fn api_route(form: &str, method: &str, path: &str, body: &Body, ctx: Ctx<'_>) -> Option<String> {
    let last = path.split(['?', '#']).next().unwrap_or_default();
    let last = last
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or_default();
    if last.eq_ignore_ascii_case("graphql") {
        if let Some(why) = graphql(form, body) {
            return Some(why);
        }
    }
    judge(form, method, path, body, ctx)
}

/// One path segment; `wild` when it holds a shell expansion the guard did
/// not resolve, which may stand for any segment or, last, for several.
struct Seg<'a> {
    text: &'a str,
    wild: bool,
}

/// Whether `tail` can name `pattern` (`*` is any one segment); `open` lets
/// the tail run longer. Returns `Some(true)` for a certain match,
/// `Some(false)` for one only an unresolved segment allows.
fn fits(tail: &[Seg<'_>], pattern: &[&str], open: bool) -> Option<bool> {
    let mut certain = true;
    for (at, token) in pattern.iter().enumerate() {
        let seg = tail.get(at)?;
        if seg.wild {
            if *token != "*" {
                certain = false;
            }
            if at + 1 == tail.len() {
                return Some(certain && at + 1 == pattern.len());
            }
            continue;
        }
        if *token != "*" && seg.text != *token {
            return None;
        }
    }
    (open || tail.len() == pattern.len()).then_some(certain)
}

fn judge(form: &str, method: &str, path: &str, body: &Body, ctx: Ctx<'_>) -> Option<String> {
    // An empty inner segment is a variable the line's environment left
    // empty; the command's own shell may not, so it stands for any segment.
    let segs: Vec<Seg<'_>> = path
        .trim_matches('/')
        .split('/')
        .map(|text| Seg {
            text,
            wild: text.is_empty() || opaque(text),
        })
        .collect();
    let shown = crate::git::display_text(path);
    if segs.first().is_some_and(|s| s.wild) {
        return Some(format!(
            "{form} {shown}: the guard cannot read the endpoint, which may publish or approve a release"
        ));
    }
    match segs.first().map(|s| s.text) {
        Some("repos") if segs.len() >= 3 => {
            let tail = judge_tail(
                form,
                method,
                &shown,
                Some((&segs[1], &segs[2])),
                &segs[3..],
                body,
                ctx,
            );
            // An unresolved owner may hold `owner/name` in one word, so the
            // path after it is read from one segment earlier too.
            if tail.is_none() && (segs[1].wild || segs[2].wild) {
                return judge_tail(
                    form,
                    method,
                    &shown,
                    Some((&segs[1], &segs[1])),
                    &segs[2..],
                    body,
                    ctx,
                );
            }
            tail
        }
        Some("orgs") if segs.len() >= 2 => {
            judge_tail(form, method, &shown, None, &segs[2..], body, ctx)
        }
        // `repositories/{id}` names a repository by number, which the
        // guard cannot match to the authority.
        Some("repositories") if segs.len() >= 2 => {
            let id = Seg {
                text: segs[1].text,
                wild: true,
            };
            judge_tail(
                form,
                method,
                &shown,
                Some((&id, &id)),
                &segs[2..],
                body,
                ctx,
            )
        }
        _ => None,
    }
}

fn judge_tail(
    form: &str,
    method: &str,
    shown: &str,
    repo: Option<(&Seg<'_>, &Seg<'_>)>,
    tail: &[Seg<'_>],
    body: &Body,
    ctx: Ctx<'_>,
) -> Option<String> {
    let org = repo.is_none();
    let unsure = |what: &str| {
        Some(format!(
            "{form} {shown}: an unresolved part of the path may make it {what}"
        ))
    };
    let refuse = |what: &str| Some(format!("{form} {shown} {what}"));
    for (pattern, open, what, orgs_too) in FLOORS {
        if org && !orgs_too {
            continue;
        }
        match fits(tail, pattern, *open) {
            Some(true) => return refuse(what),
            Some(false) => return unsure(what),
            None => {}
        }
    }
    match fits(tail, &["git", "refs"], false) {
        Some(true) if method == "POST" => {
            return match body.field("ref") {
                Field::Known(r) if r.starts_with("refs/heads/") => None,
                Field::Known(r) => refuse(&format!(
                    "creates ref {}, which is not a branch",
                    crate::git::display_text(&r)
                )),
                _ => refuse("creates a ref the guard cannot read, which may be a release tag"),
            }
        }
        Some(false) => return unsure("create a release tag"),
        _ => {}
    }
    if let Some(certain) = fits(tail, &["git", "refs", "*"], true) {
        if !certain || tail.get(2).is_some_and(|s| s.text != "heads") {
            return unsure("move a release tag");
        }
    }
    let workflow = fits(tail, &["actions", "workflows", "*", "dispatches"], false);
    let enabling = fits(tail, &["actions", "workflows", "*", "enable"], false);
    let repository = fits(tail, &["dispatches"], false);
    let store = store_route(tail, body);
    if workflow.is_none() && enabling.is_none() && repository.is_none() && store.is_none() {
        return None;
    }
    if [workflow, enabling, repository].contains(&Some(false))
        || matches!(store, Some((_, _, false)))
    {
        return unsure("dispatch, enable or configure a release workflow");
    }
    let authority = match Authority::read(ctx.cwd) {
        Ok(authority) => authority,
        Err(why) => return Some(unreadable(form, &why)),
    };
    if let Some((owner, name)) = repo {
        if owner.wild || name.wild {
            return refuse(
                "names a repository the guard cannot resolve, whose workflows it cannot read",
            );
        }
        if let Some(why) = authority.path_repo(owner.text, name.text, ctx.target) {
            return Some(format!("{form} {shown} {why}"));
        }
    }
    if let Some((store, names, _)) = store {
        return store_write(store, &names, &authority).map(|why| format!("{form} {shown} {why}"));
    }
    // Workflow and repository dispatches name one repository.
    repo?;
    if enabling.is_some() {
        return enable(form, &authority, tail[2].text, true);
    }
    if workflow.is_some() {
        // The workflow is judged first, whatever the body holds.
        let selector = tail[2].text;
        if let Some(why) = dispatch(form, &authority, selector, None, true) {
            return Some(why);
        }
        return match body.field("ref") {
            Field::Known(r) => dispatch(form, &authority, selector, Some(&r), true),
            Field::Unknown => refuse("dispatches at a ref the guard cannot read"),
            Field::Absent => None,
        };
    }
    repository_dispatch(&authority, body).map(|what| format!("{form} {shown} {what}"))
}

/// An Actions variable or secret write: what it writes, the names it
/// changes, and whether the path is certain.
fn store_route(tail: &[Seg<'_>], body: &Body) -> Option<(Store, Vec<Field>, bool)> {
    let segment_name = |seg: &Seg<'_>| {
        if seg.wild {
            Field::Unknown
        } else {
            Field::Known(seg.text.to_owned())
        }
    };
    if let Some(certain) = fits(tail, &["actions", "variables"], false) {
        return Some((Store::Vars, vec![body.field("name")], certain));
    }
    if let Some(certain) = fits(tail, &["actions", "variables", "*"], false) {
        // A PATCH can rename the variable, so the body's name counts too.
        let mut names = vec![segment_name(&tail[2])];
        if !matches!(body.field("name"), Field::Absent) {
            names.push(body.field("name"));
        }
        return Some((Store::Vars, names, certain));
    }
    if let Some(certain) = fits(tail, &["actions", "secrets", "*"], false) {
        return Some((Store::Secrets, vec![segment_name(&tail[2])], certain));
    }
    None
}

/// Whether a write to `names` changes what a release workflow reads.
fn store_write(store: Store, names: &[Field], authority: &Authority) -> Option<String> {
    let what = match store {
        Store::Vars => "variable",
        Store::Secrets => "secret",
    };
    for flow in authority.flows.iter().filter(|flow| flow.release.is_some()) {
        let reads = match store {
            Store::Vars => &flow.reads.vars,
            Store::Secrets => &flow.reads.secrets,
        };
        let path = crate::git::display_text(&flow.path);
        for name in names {
            match name {
                Field::Known(name) if reads.covers(name) => {
                    return Some(format!(
                        "changes {what} {}, which release workflow {path} reads",
                        crate::git::display_text(name)
                    ))
                }
                Field::Unknown if reads.any() => {
                    return Some(format!(
                        "changes a {what} the guard cannot name, and release workflow {path} reads {what}s"
                    ))
                }
                _ => {}
            }
        }
    }
    None
}

#[derive(Clone, Copy)]
enum Store {
    Vars,
    Secrets,
}

/// Remote floors and deployment approval, refused whatever the repository:
/// the path pattern, whether the path may run longer, what it does, and
/// whether the organization path is refused too.
const FLOORS: &[(&[&str], bool, &str, bool)] = &[
    (
        &["actions", "policies"],
        true,
        "writes an Actions policy, which holds the actor and event floors",
        true,
    ),
    (
        &["rulesets"],
        true,
        "writes a ruleset, which holds the release tag floor",
        true,
    ),
    (
        &["environments"],
        true,
        "writes an environment, which holds the required-reviewer floor",
        false,
    ),
    (
        &["actions", "runs", "*", "pending_deployments"],
        false,
        "approves or rejects a pending deployment",
        false,
    ),
    (
        &["actions", "runs", "*", "deployment_protection_rule"],
        false,
        "answers a deployment protection rule",
        false,
    ),
    (
        &["releases"],
        true,
        "creates, publishes or changes a release",
        false,
    ),
    (&["git", "tags"], false, "creates a tag object", false),
    (
        &["deployments"],
        true,
        "creates or changes a deployment, which starts the runs that listen for deployment events",
        false,
    ),
    (
        &["branches", "*", "protection"],
        true,
        "writes branch protection, which holds the push floor",
        false,
    ),
    (
        &["git", "refs", "tags"],
        true,
        "creates, moves or deletes a release tag",
        false,
    ),
];

/// What a repository dispatch with `body` would start, when a release
/// workflow at the authority listens for it.
fn repository_dispatch(authority: &Authority, body: &Body) -> Option<String> {
    let listeners: Vec<(&Flow, &Listen)> = authority
        .flows
        .iter()
        .filter(|flow| flow.release.is_some())
        .filter_map(|flow| flow.repository_dispatch.as_ref().map(|l| (flow, l)))
        .collect();
    let (first, _) = listeners.first()?;
    let Field::Known(event) = body.field("event_type") else {
        return Some(format!(
            "sends a repository dispatch whose event type the guard cannot read, and release workflow {} listens for repository dispatches",
            crate::git::display_text(&first.path)
        ));
    };
    listeners
        .iter()
        .find(|(_, listen)| match listen {
            Listen::Any => true,
            Listen::Types(types) => types.contains(&event),
        })
        .map(|(flow, _)| {
            format!(
                "sends repository dispatch {}, which release workflow {} listens for",
                crate::git::display_text(&event),
                crate::git::display_text(&flow.path)
            )
        })
}

/// The repository dispatch types a workflow listens for.
#[derive(Debug)]
enum Listen {
    Any,
    Types(Vec<String>),
}

/// One workflow file at the authority.
#[derive(Debug)]
struct Flow {
    path: String,
    name: Option<String>,
    /// Why it is a release workflow, or `None`.
    release: Option<String>,
    /// The repository dispatch types it listens for, if it listens.
    repository_dispatch: Option<Listen>,
    /// The variables and secrets it reads.
    reads: Reads,
}

impl Flow {
    /// A file the guard cannot read counts as a release workflow that
    /// listens to every repository dispatch and reads every stored value.
    fn unreadable(path: String, why: &str) -> Self {
        Self {
            path,
            name: None,
            release: Some(why.to_owned()),
            repository_dispatch: Some(Listen::Any),
            reads: Reads::ALL,
        }
    }
}

/// The variables and secrets a workflow reads.
#[derive(Debug)]
struct Reads {
    vars: Names,
    secrets: Names,
}

impl Reads {
    const ALL: Self = Self {
        vars: Names::All,
        secrets: Names::All,
    };

    /// What `text` reads: `vars.NAME` and `secrets.NAME`, or every name
    /// when it indexes or serializes the context or passes secrets on with
    /// `secrets: inherit`.
    fn of(text: &str) -> Self {
        // YAML and expression blanks: space, tab and line ends.
        let squeezed: String = text
            .chars()
            .filter(|c| !matches!(c, ' ' | '\t' | '\n' | '\r'))
            .collect();
        let whole = |context: &str| {
            squeezed.contains(&format!("{context}["))
                || squeezed.contains(&format!("toJSON({context})"))
                || squeezed.contains(&format!("tojson({context})"))
        };
        let names = |context: &str, all: bool| {
            if all {
                return Names::All;
            }
            let prefix = format!("{context}.");
            let mut found = Vec::new();
            let mut at = 0;
            while let Some(offset) = text[at..].find(&prefix) {
                let start = at + offset;
                at = start + prefix.len();
                let before = text[..start].chars().next_back();
                if before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.') {
                    continue;
                }
                let name: String = text[at..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                if !name.is_empty() && !name.eq_ignore_ascii_case("GITHUB_TOKEN") {
                    found.push(name.to_ascii_uppercase());
                }
            }
            found.sort();
            found.dedup();
            Names::Some(found)
        };
        Self {
            vars: names("vars", whole("vars")),
            secrets: names(
                "secrets",
                whole("secrets") || squeezed.contains("secrets:inherit"),
            ),
        }
    }
}

/// Names a workflow reads from one context.
#[derive(Debug)]
enum Names {
    All,
    Some(Vec<String>),
}

impl Names {
    /// GitHub stores variable and secret names in upper case.
    fn covers(&self, name: &str) -> bool {
        match self {
            Self::All => true,
            Self::Some(names) => names.iter().any(|n| n.eq_ignore_ascii_case(name)),
        }
    }

    fn any(&self) -> bool {
        match self {
            Self::All => true,
            Self::Some(names) => !names.is_empty(),
        }
    }
}

/// The workflows at the policy authority and what names that authority.
struct Authority {
    repo: git2::Repository,
    flows: Vec<Flow>,
    /// The remote and default branch; `None` in the labelled states.
    remote: Option<(String, String)>,
    /// `owner/name` of the remote, when its URL names a GitHub repository.
    slug: Option<String>,
    shown: String,
}

impl Authority {
    fn read(cwd: Option<&Path>) -> Result<Self, String> {
        let cwd = cwd.ok_or("the command's directory is not known")?;
        let repo = crate::hooks::repo::open(cwd)?.ok_or("no repository here")?;
        let root = repo.workdir().unwrap_or(repo.path()).to_path_buf();
        let landed = crate::hooks::landed_policy::load(&root)?;
        let commit = landed.commit.ok_or("no committed authority")?;
        let remote = landed.authority.as_ref().and_then(|authority| {
            let rest = authority.reference.strip_prefix("refs/remotes/")?;
            let (remote, branch) = rest.split_once('/')?;
            Some((remote.to_owned(), branch.to_owned()))
        });
        let slug = remote
            .as_ref()
            .and_then(|(name, _)| repo.find_remote(name).ok())
            .and_then(|remote| remote.url().ok().and_then(github_slug));
        let flows = {
            let tree = repo
                .find_commit(commit)
                .and_then(|commit| commit.tree())
                .map_err(|e| e.to_string())?;
            flows_in(&repo, &tree)?
        };
        Ok(Self {
            repo,
            flows,
            remote,
            slug,
            shown: landed.source,
        })
    }

    /// Why the repository `target` names is not the authority's, or
    /// `None`.
    fn target(&self, target: &Target) -> Option<String> {
        match target {
            Target::Named(named) => self.foreign(named),
            Target::Unresolved(why) => Some(why.clone()),
            Target::Current => self.current(),
            Target::Literal => None,
        }
    }

    /// Why the repository an API path names, `owner/name` or gh's
    /// `{owner}` and `{repo}` placeholders, is not the authority's or
    /// disagrees with `-R` and `GH_REPO`, or `None`.
    fn path_repo(&self, owner: &str, name: &str, target: &Target) -> Option<String> {
        let placeholder = owner == "{owner}" || name == "{repo}";
        if placeholder && !matches!(target, Target::Literal) {
            // gh fills both from the repository it acts on, which must be
            // the authority's; a literal part must then match it too.
            if let Some(why) = self.target(target) {
                return Some(why);
            }
            let slug = self.slug.as_deref().unwrap_or_default();
            let (slug_owner, slug_name) = slug.split_once('/').unwrap_or_default();
            let owner = if owner == "{owner}" {
                slug_owner
            } else {
                owner
            };
            let name = if name == "{repo}" { slug_name } else { name };
            return self.foreign(&format!("{owner}/{name}"));
        }
        let named = format!("{owner}/{name}");
        if let Target::Named(flag) | Target::Unresolved(flag) = target {
            let key = |repo: &str| github_slug(repo).unwrap_or_else(|| repo.to_ascii_lowercase());
            if matches!(target, Target::Unresolved(_)) || key(flag) != key(&named) {
                return Some(format!(
                    "names repository {} in its path, which `-R` or `GH_REPO` contradicts ({})",
                    crate::git::display_text(&named),
                    crate::git::display_text(flag)
                ));
            }
        }
        self.foreign(&named)
    }

    /// Why gh's current repository may not be the authority's: every
    /// GitHub remote of this repository must name the authority's
    /// repository, since gh picks among them by rules the guard does not
    /// repeat.
    fn current(&self) -> Option<String> {
        let unsure = |why: String| {
            Some(format!(
                "acts on gh's current repository, which the guard cannot tell is the policy authority's: {why}"
            ))
        };
        let names = match self.repo.remotes() {
            Ok(names) => names,
            Err(e) => return unsure(format!("the remotes cannot be read ({})", e.message())),
        };
        let mut github = 0;
        for name in &names {
            let Ok(Some(name)) = name else {
                return unsure("a remote name is not UTF-8".into());
            };
            let url = self
                .repo
                .find_remote(name)
                .ok()
                .and_then(|r| r.url().ok().map(str::to_owned));
            let Some(url) = url else {
                return unsure(format!(
                    "remote {} has no readable URL",
                    crate::git::display_text(name)
                ));
            };
            if !url.to_ascii_lowercase().contains("github") {
                continue;
            }
            github += 1;
            if self.slug.is_none() || github_slug(&url) != self.slug {
                return unsure(format!(
                    "remote {} names another repository",
                    crate::git::display_text(name)
                ));
            }
        }
        (github == 0).then(|| {
            "acts on gh's current repository, and no remote here names a GitHub repository".into()
        })
    }

    /// Why `named` is not the authority's repository, or `None`.
    fn foreign(&self, named: &str) -> Option<String> {
        let shown = crate::git::display_text(named);
        if opaque(named) {
            return Some(format!(
                "names repository {shown}, which the guard cannot resolve, so it cannot read its workflows"
            ));
        }
        let wanted = github_slug(named).unwrap_or_else(|| named.to_ascii_lowercase());
        (self.slug.as_deref() != Some(wanted.as_str())).then(|| {
            format!(
                "names repository {shown}, whose workflows the guard cannot read; only the repository of the policy authority is read"
            )
        })
    }

    fn is_default(&self, reference: &str) -> bool {
        let reference = reference.strip_prefix("refs/heads/").unwrap_or(reference);
        self.remote
            .as_ref()
            .is_some_and(|(_, branch)| branch == reference)
    }

    /// The workflow `path` as the remote's tracking ref for `reference`
    /// holds it.
    fn flow_at(&self, reference: &str, path: &str) -> Result<Option<Flow>, String> {
        let (remote, _) = self.remote.as_ref().ok_or("no remote")?;
        let branch = reference.strip_prefix("refs/heads/").unwrap_or(reference);
        let tree = self
            .repo
            .find_reference(&format!("refs/remotes/{remote}/{branch}"))
            .and_then(|r| r.peel_to_tree())
            .map_err(|e| e.to_string())?;
        Ok(flows_in(&self.repo, &tree)?
            .into_iter()
            .find(|flow| flow.path == path))
    }
}

/// `owner/name`, lower case, from a github.com URL (`https://`,
/// `ssh://` or scp-style `git@github.com:`), gh's `HOST/OWNER/REPO` or a
/// bare `owner/name`. The host is compared as [`host_name`] reads it, so
/// a look-alike such as `evilgithub.com` names no GitHub repository.
fn github_slug(text: &str) -> Option<String> {
    let github = |host: &str| {
        host_name(host).is_some_and(|h| matches!(h.as_str(), "github.com" | "www.github.com"))
    };
    let path = if let Some((_, after)) = text.split_once("://") {
        let (authority, path) = after.split_once('/')?;
        let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
        if authority.matches('@').count() > 1 || !github(host) {
            return None;
        }
        path
    } else if let Some((user_host, path)) = text.split_once(':') {
        let host = user_host.rsplit_once('@').map_or(user_host, |(_, h)| h);
        if user_host.contains('/') || !github(host) {
            return None;
        }
        path
    } else {
        match text.split('/').collect::<Vec<_>>().as_slice() {
            [_, _] => text,
            [host, _, _] if github(host) => text.split_once('/')?.1,
            _ => return None,
        }
    };
    let path = path.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let (owner, name) = path.split_once('/')?;
    let part = |p: &str| {
        !matches!(p, "" | "." | "..")
            && p.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-._".contains(&b))
    };
    (part(owner) && part(name)).then(|| format!("{owner}/{name}").to_ascii_lowercase())
}

const WORKFLOWS: &str = ".github/workflows";
const READ_LIMIT: usize = 1024 * 1024;

fn flows_in(repo: &git2::Repository, tree: &git2::Tree<'_>) -> Result<Vec<Flow>, String> {
    let dir = match tree.get_path(Path::new(WORKFLOWS)) {
        Ok(entry) => entry,
        Err(e) if e.code() == git2::ErrorCode::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.to_string()),
    };
    let dir = dir
        .to_object(repo)
        .and_then(|o| o.peel_to_tree())
        .map_err(|e| format!("{WORKFLOWS} is not a directory: {e}"))?;
    let mut flows = Vec::new();
    for entry in &dir {
        let Ok(name) = entry.name() else {
            return Err(format!("{WORKFLOWS} holds a name that is not UTF-8"));
        };
        let extension = Path::new(name).extension().and_then(|e| e.to_str());
        if !extension
            .is_some_and(|e| e.eq_ignore_ascii_case("yml") || e.eq_ignore_ascii_case("yaml"))
        {
            continue;
        }
        let path = format!("{WORKFLOWS}/{name}");
        let text = (entry.kind() == Some(git2::ObjectType::Blob) && entry.filemode() != 0o120_000)
            .then(|| repo.find_blob(entry.id()).ok())
            .flatten()
            .filter(|blob| blob.size() <= READ_LIMIT)
            .and_then(|blob| String::from_utf8(blob.content().to_vec()).ok());
        flows.push(match text {
            Some(text) => classify(path, &text),
            None => Flow::unreadable(path, "the guard cannot read the file"),
        });
    }
    Ok(flows)
}

/// Token scopes whose write lets a run publish, or start or unblock a run
/// that publishes: `actions` dispatches and re-runs workflows, and
/// `administration` changes rulesets, environments and branch protection.
const PUBLISHING: &[&str] = &[
    "actions",
    "administration",
    "contents",
    "packages",
    "deployments",
    "pages",
    "attestations",
    "id-token",
];

/// Whether permissions write a publishing scope: `None` when unset.
fn permissions_publish(value: Option<&serde_yaml::Value>) -> Option<bool> {
    use serde_yaml::Value;
    Some(match value? {
        Value::String(s) => s != "read-all",
        Value::Mapping(map) => map.iter().any(|(key, level)| {
            key.as_str().is_none_or(|k| PUBLISHING.contains(&k))
                && level.as_str().is_none_or(|l| l != "read" && l != "none")
        }),
        Value::Null => return None,
        _ => true,
    })
}

fn classify(path: String, text: &str) -> Flow {
    use serde_yaml::Value;
    let unreadable = |path| Flow::unreadable(path, "the guard cannot parse the file");
    let Ok(Value::Mapping(doc)) = serde_yaml::from_str::<Value>(text) else {
        return unreadable(path);
    };
    let name = doc.get("name").and_then(Value::as_str).map(str::to_owned);
    let on = doc
        .get("on")
        .or_else(|| doc.get(Value::Bool(true)))
        .cloned()
        .unwrap_or(Value::Null);
    let events: Vec<(String, Value)> = match on {
        Value::String(event) => vec![(event, Value::Null)],
        Value::Sequence(list) => list
            .into_iter()
            .filter_map(|v| v.as_str().map(|s| (s.to_owned(), Value::Null)))
            .collect(),
        Value::Mapping(map) => map
            .into_iter()
            .filter_map(|(k, v)| k.as_str().map(|s| (s.to_owned(), v)))
            .collect(),
        _ => return unreadable(path),
    };
    let event = |name: &str| events.iter().find(|(e, _)| e == name).map(|(_, v)| v);
    // A types value the guard cannot read listens for any type.
    let repository_dispatch =
        event("repository_dispatch").map(|config| match config.get("types") {
            Some(Value::Sequence(list)) if list.iter().all(|v| v.as_str().is_some()) => {
                Listen::Types(
                    list.iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect(),
                )
            }
            Some(Value::String(one)) => Listen::Types(vec![one.clone()]),
            _ => Listen::Any,
        });
    if event("workflow_dispatch").is_none() && repository_dispatch.is_none() {
        return Flow {
            path,
            name,
            release: None,
            repository_dispatch: None,
            reads: Reads::of(text),
        };
    }
    let mut reasons = Vec::new();
    let top = permissions_publish(doc.get("permissions"));
    if top == Some(true) {
        reasons.push("its token can write a publishing scope".to_owned());
    }
    match doc.get("jobs") {
        Some(Value::Mapping(jobs)) => {
            for (job_name, job) in jobs {
                let job_name = job_name.as_str().unwrap_or("?");
                let own = permissions_publish(job.get("permissions"));
                if own == Some(true) {
                    reasons.push(format!("job {job_name} can write a publishing scope"));
                } else if own.is_none() && top.is_none() {
                    reasons.push(format!(
                        "job {job_name} takes the repository's default token permissions"
                    ));
                }
                if job.get("uses").is_some() {
                    reasons.push(format!("job {job_name} calls a reusable workflow"));
                }
                if job.get("environment").is_some() {
                    reasons.push(format!("job {job_name} deploys to an environment"));
                }
            }
        }
        _ => reasons.push("its jobs cannot be read".into()),
    }
    let mut rest = text;
    while let Some(at) = rest.find("secrets.") {
        rest = &rest[at + "secrets.".len()..];
        let secret: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if !secret.eq_ignore_ascii_case("GITHUB_TOKEN") {
            reasons.push(format!("it reads secret {secret}"));
            break;
        }
    }
    reasons.dedup();
    Flow {
        path,
        name,
        release: (!reasons.is_empty()).then(|| {
            reasons.truncate(2);
            reasons.join("; ")
        }),
        repository_dispatch,
        reads: Reads::of(text),
    }
}

/// Whether a bare push refspec part has the shape of a release tag: the
/// `v*` pattern of this repository's `release tags` ruleset, or a leading
/// digit as version tags elsewhere take. The guard cannot read an adopter's
/// ruleset, so these shapes stand for every adopter.
pub(crate) fn release_tag_shaped(part: &str) -> bool {
    !part.starts_with("refs/")
        && part != "HEAD"
        && part
            .chars()
            .next()
            .is_some_and(|c| c == 'v' || c.is_ascii_digit())
}

#[cfg(test)]
#[path = "release_routes_tests.rs"]
mod tests;
