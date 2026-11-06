//! The untracked account profile: which config home each family's seats run
//! under, and an optional capped API-key fallback (TSK-431).
//!
//! The file holds folder paths, the *name* of an environment variable and a
//! spend cap. It never holds a token: a key whose name says token or
//! credential, a value shaped like a secret, or any unknown key refuses the
//! whole file. The profile moves no credential and switches no account; a
//! launch sets one variable to a folder the operator has already signed in to.
//!
//! ```toml
//! schema_version = 1
//!
//! [families.codex]
//! config_home = "~/.codex-second"
//!
//! [families.codex.api_fallback]
//! key_env = "CODEX_FALLBACK_API_KEY"   # the variable's name, never its value
//! spend_cap_usd = 25
//! ```

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

use super::{shared_root, Family, LOCAL_DIR};

/// File name of the profile, in the project's local folder or the user's
/// `CodeFlow` home.
pub const PROFILE_FILE: &str = "accounts.toml";
const MAX_PROFILE_BYTES: u64 = 64 * 1024;
const MAX_SPEND_CAP_USD: f64 = 1_000_000.0;

/// Key-name fragments that mark a credential. A table or key whose name
/// contains one is refused wherever it appears.
const CREDENTIAL_NAMES: [&str; 10] = [
    "token",
    "secret",
    "credential",
    "password",
    "passwd",
    "api_key",
    "apikey",
    "bearer",
    "oauth",
    "cookie",
];

/// A capped API-key fallback: the variable that holds the key and the most
/// the project allows it to spend.
#[derive(Clone, Debug, PartialEq)]
pub struct ApiFallback {
    /// Name of the environment variable that holds the key. The key itself is
    /// never read here.
    pub key_env: String,
    /// Spend cap in US dollars.
    pub spend_cap_usd: f64,
}

/// What a project says about one family's account.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FamilyAccount {
    /// Config home the family's seats launch under, absolute.
    pub config_home: Option<PathBuf>,
    /// Capped API-key fallback, when the project allows one.
    pub api_fallback: Option<ApiFallback>,
}

/// A parsed profile.
#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    /// The file the profile came from.
    pub source: PathBuf,
    /// Each mapped family.
    pub families: BTreeMap<Family, FamilyAccount>,
}

/// The one variable a launch of a family sets.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LaunchEnv {
    /// The family's config-home variable.
    pub variable: &'static str,
    /// The folder it points at.
    pub config_home: PathBuf,
}

impl LaunchEnv {
    /// A POSIX shell line that sets the variable for the shell and its
    /// children, quoted so the path is one literal word.
    ///
    /// # Errors
    /// Refuses a path with a control character, which no shell line carries
    /// safely.
    pub fn shell_export(&self) -> Result<String, String> {
        let text = self
            .config_home
            .to_str()
            .ok_or("the config home is not valid UTF-8")?;
        if text.chars().any(char::is_control) {
            return Err("the config home holds a control character".into());
        }
        Ok(format!(
            "export {}='{}'",
            self.variable,
            text.replace('\'', "'\\''")
        ))
    }
}

impl Profile {
    /// The launch variable for `family`, or `None` when the profile maps no
    /// config home for it. Only that family's own variable is ever returned.
    #[must_use]
    pub fn launch_env(&self, family: Family) -> Option<LaunchEnv> {
        let config_home = self.families.get(&family)?.config_home.clone()?;
        Some(LaunchEnv {
            variable: family.config_home_var(),
            config_home,
        })
    }

    /// The config home a seat of `family` runs under, or `None` for the
    /// CLI's default home.
    #[must_use]
    pub fn config_home(&self, family: Family) -> Option<&Path> {
        self.families.get(&family)?.config_home.as_deref()
    }

    /// Parse profile text read from `source`.
    ///
    /// `project_root` is the folder the command runs in; a config home is
    /// checked against its main checkout and linked worktrees (see
    /// [`super::boundary`]). `home` expands a leading `~`.
    ///
    /// Diagnostics carry a line and column or a key name, never a value or a
    /// source excerpt, because a mistaken paste must not reach a log.
    ///
    /// # Errors
    /// Refuses a credential key, a secret-shaped key or value (escapes
    /// decoded), an unknown key, a relative or parent-walking path, a home
    /// that could be committed, a fallback without a valid cap or variable
    /// name, and a family table that maps nothing.
    pub fn parse(
        text: &str,
        source: &Path,
        project_root: &Path,
        home: Option<&Path>,
    ) -> Result<Profile, String> {
        let value: toml::Value = text
            .parse()
            .map_err(|error: toml::de::Error| not_toml(text, &error))?;
        // Secret shapes first: every later diagnostic may name a place, and a
        // key can itself be the pasted secret.
        refuse_secret_shapes(&value)?;
        refuse_credentials(&value)?;
        for line in text.split('\n') {
            if let Some(kind) = crate::hooks::scan::scan_line(line) {
                return Err(format!(
                    "a line looks like a secret ({kind}); the profile holds paths and caps only"
                ));
            }
        }
        let raw: Raw = toml::from_str(text).map_err(|error: toml::de::Error| {
            format!(
                "does not match the profile format{}; see docs/delegation.md",
                at(text, &error)
            )
        })?;
        if raw.schema_version != 1 {
            return Err(format!(
                "unsupported schema_version {}; this CodeFlow reads 1",
                raw.schema_version
            ));
        }
        let mut families = BTreeMap::new();
        for (id, account) in raw.families {
            let family = Family::from_id(&id).ok_or_else(|| {
                "unknown family name in `[families.*]`; the families are claude, codex and grok"
                    .to_string()
            })?;
            let config_home = account
                .config_home
                .map(|path| resolve_config_home(&path, project_root, home))
                .transpose()
                .map_err(|reason| format!("family {id}: {reason}"))?;
            let api_fallback = account
                .api_fallback
                .map(RawFallback::checked)
                .transpose()
                .map_err(|reason| format!("family {id}: {reason}"))?;
            if config_home.is_none() && api_fallback.is_none() {
                return Err(format!(
                    "family {id} maps nothing; give it a config_home or an api_fallback"
                ));
            }
            families.insert(
                family,
                FamilyAccount {
                    config_home,
                    api_fallback,
                },
            );
        }
        Ok(Profile {
            source: source.to_path_buf(),
            families,
        })
    }
}

/// ` at line L, column C` for a TOML error, or nothing when it has no span.
fn at(text: &str, error: &toml::de::Error) -> String {
    let Some(span) = error.span() else {
        return String::new();
    };
    let before = text.get(..span.start).unwrap_or("");
    let line = before.matches('\n').count() + 1;
    let column = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
    format!(" at line {line}, column {column}")
}

/// A parse failure described by place only: the parser's own message quotes
/// the offending text, which may be a secret.
fn not_toml(text: &str, error: &toml::de::Error) -> String {
    format!("not valid TOML{}", at(text, error))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    schema_version: u32,
    #[serde(default)]
    families: BTreeMap<String, RawFamily>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFamily {
    config_home: Option<String>,
    api_fallback: Option<RawFallback>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFallback {
    key_env: String,
    spend_cap_usd: f64,
}

impl RawFallback {
    fn checked(self) -> Result<ApiFallback, String> {
        let name = &self.key_env;
        let valid = !name.is_empty()
            && name.len() <= 48
            && name.starts_with(|c: char| c.is_ascii_uppercase() || c == '_')
            && name
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
        if !valid {
            return Err(
                "api_fallback.key_env must be the NAME of an environment variable, such as \
                 CODEX_FALLBACK_API_KEY (capitals, digits and underscores, at most 48); \
                 never the key"
                    .into(),
            );
        }
        if !(self.spend_cap_usd.is_finite()
            && self.spend_cap_usd > 0.0
            && self.spend_cap_usd <= MAX_SPEND_CAP_USD)
        {
            return Err(format!(
                "api_fallback.spend_cap_usd must be above 0 and at most {MAX_SPEND_CAP_USD}"
            ));
        }
        Ok(ApiFallback {
            key_env: self.key_env,
            spend_cap_usd: self.spend_cap_usd,
        })
    }
}

/// Refuse any table or key whose name marks a credential.
fn refuse_credentials(value: &toml::Value) -> Result<(), String> {
    if let toml::Value::Table(table) = value {
        for (key, inner) in table {
            let lower = key.to_ascii_lowercase();
            if let Some(word) = CREDENTIAL_NAMES.iter().find(|word| lower.contains(*word)) {
                // The key text is untrusted and is never echoed.
                return Err(format!(
                    "a key in the file names a credential ({word}); the profile never holds \
                     a token, a key or any secret"
                ));
            }
            refuse_credentials(inner)?;
        }
    }
    Ok(())
}

/// Refuse a key or string whose decoded text looks like a secret. The raw
/// line scan cannot see through TOML escapes (`\u0073k-...`), so the decoded
/// text of every key and value, each line of a multi-line string included,
/// is scanned too. The message names the shape, never the text.
fn refuse_secret_shapes(value: &toml::Value) -> Result<(), String> {
    let shape = |text: &str| text.split('\n').find_map(crate::hooks::scan::scan_line);
    let refuse = |kind: &str| {
        format!(
            "a key or value looks like a secret ({kind}); the profile holds paths and caps only"
        )
    };
    match value {
        toml::Value::String(text) => shape(text).map_or(Ok(()), |kind| Err(refuse(kind))),
        toml::Value::Array(items) => items.iter().try_for_each(refuse_secret_shapes),
        toml::Value::Table(table) => table.iter().try_for_each(|(key, inner)| {
            shape(key).map_or(Ok(()), |kind| Err(refuse(kind)))?;
            refuse_secret_shapes(inner)
        }),
        _ => Ok(()),
    }
}

fn resolve_config_home(
    text: &str,
    project_root: &Path,
    home: Option<&Path>,
) -> Result<PathBuf, String> {
    if text.is_empty() || text.chars().any(char::is_control) {
        return Err("config_home must be a path with no control characters".into());
    }
    let expanded = if text == "~" || text.starts_with("~/") {
        let home = home.ok_or("config_home starts with ~ but no home folder is known")?;
        home.join(text.trim_start_matches('~').trim_start_matches('/'))
    } else {
        PathBuf::from(text)
    };
    if !expanded.is_absolute() {
        return Err(format!(
            "config_home `{text}` must be absolute or start with ~/; a relative path would \
             mean a different folder in each worktree"
        ));
    }
    if expanded
        .components()
        .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return Err(format!("config_home `{text}` must not use . or .. parts"));
    }
    super::boundary::check_config_home(&expanded, project_root)?;
    Ok(expanded)
}

/// Load the profile for the project at `root`: `None` when no file exists.
/// The project's untracked file wins over the one in the user's `CodeFlow`
/// home. Each file is read without following a link at any component.
///
/// # Errors
/// A file that exists but cannot be read, is under a link, is too large, or is
/// refused by [`Profile::parse`]; the message names the file.
pub fn load(
    root: &Path,
    codeflow_home: Option<&Path>,
    user_home: Option<&Path>,
) -> Result<Option<Profile>, String> {
    let project = shared_root(root)?;
    let local_file = format!("{LOCAL_DIR}/{PROFILE_FILE}");
    let mut places = vec![(project, local_file)];
    if let Some(home) = codeflow_home {
        places.push((home.to_path_buf(), PROFILE_FILE.to_string()));
    }
    for (base, relative) in places {
        let Some(bytes) = super::confined_read(&base, &relative, MAX_PROFILE_BYTES)? else {
            continue;
        };
        let path = base.join(&relative);
        let shown = crate::git::display_path(&path);
        let text = String::from_utf8(bytes).map_err(|_| format!("{shown}: not UTF-8 text"))?;
        return Profile::parse(&text, &path, root, user_home)
            .map(Some)
            .map_err(|reason| format!("{shown}: {reason}"));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An absolute path on every platform: Windows needs a drive prefix, so a
    /// bare `/srv/x` is not absolute there. Forward slashes are valid on both.
    const ROOT: &str = if cfg!(windows) { "C:/" } else { "/" };

    fn project() -> PathBuf {
        PathBuf::from(format!("{ROOT}work/project"))
    }

    fn user_home() -> PathBuf {
        PathBuf::from(format!("{ROOT}home/me"))
    }

    fn parse(text: &str) -> Result<Profile, String> {
        Profile::parse(
            text,
            Path::new("accounts.toml"),
            &project(),
            Some(&user_home()),
        )
    }

    fn good() -> String {
        format!(
            r#"
schema_version = 1
[families.codex]
config_home = "~/.codex-second"
[families.codex.api_fallback]
key_env = "CODEX_FALLBACK_API_KEY"
spend_cap_usd = 25
[families.claude]
config_home = "{ROOT}srv/claude-work"
"#
        )
    }

    #[test]
    fn a_profile_maps_a_family_to_its_own_variable_only() {
        let profile = parse(&good()).unwrap();
        let codex = profile.launch_env(Family::Codex).unwrap();
        assert_eq!(codex.variable, "CODEX_HOME");
        assert_eq!(codex.config_home, user_home().join(".codex-second"));
        let claude = profile.launch_env(Family::Claude).unwrap();
        assert_eq!(claude.variable, "CLAUDE_CONFIG_DIR");
        assert!(profile.launch_env(Family::Grok).is_none());
        assert_eq!(
            profile.families[&Family::Codex].api_fallback,
            Some(ApiFallback {
                key_env: "CODEX_FALLBACK_API_KEY".into(),
                spend_cap_usd: 25.0
            })
        );
    }

    #[test]
    fn a_file_that_names_a_token_or_credential_is_refused() {
        for (text, needle) in [
            (
                "schema_version = 1\n[families.codex]\nconfig_home = \"/a\"\ntoken = \"x\"\n",
                "token",
            ),
            (
                "schema_version = 1\n[families.codex]\nconfig_home = \"/a\"\n\
                 [families.codex.api_fallback]\nkey_env = \"K\"\nspend_cap_usd = 1\n\
                 api_key = \"x\"\n",
                "api_key",
            ),
            (
                "schema_version = 1\n[families.codex.oauth]\nconfig_home = \"/a\"\n",
                "oauth",
            ),
            (
                "schema_version = 1\n[families.codex]\nCredentials = \"/a\"\n",
                "credential",
            ),
        ] {
            let error = parse(text).unwrap_err();
            assert!(error.contains(needle), "{text:?} -> {error}");
        }
    }

    #[test]
    fn a_secret_shaped_value_is_refused_wherever_it_sits() {
        let key = format!("sk-{}", "a1".repeat(20));
        let as_path =
            format!("schema_version = 1\n[families.codex]\nconfig_home = \"/srv/{key}\"\n");
        assert!(parse(&as_path).unwrap_err().contains("looks like a secret"));
        let as_name = format!(
            "schema_version = 1\n[families.codex.api_fallback]\nkey_env = \"{key}\"\n\
             spend_cap_usd = 5\n"
        );
        assert!(parse(&as_name).is_err());
        let pasted = "schema_version = 1\n[families.codex.api_fallback]\n\
                      key_env = \"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOP0123456789\"\n\
                      spend_cap_usd = 5\n";
        assert!(parse(pasted).unwrap_err().contains("NAME"));
    }

    #[test]
    fn a_secret_shape_hidden_by_toml_escapes_is_refused() {
        let key = "a1".repeat(20);
        // `\u0073` decodes to `s`: the raw line has no `sk-` in it.
        for text in [
            format!("schema_version = 1\n[families.codex]\nconfig_home = \"/srv/\\u0073k-{key}\"\n"),
            format!(
                "schema_version = 1\n[families.codex]\nconfig_home = \"\"\"\n/srv/\\u0073k-{key}\"\"\"\n"
            ),
            format!("schema_version = 1\n[families.\"\\u0073k-{key}\"]\nconfig_home = \"/a\"\n"),
        ] {
            let error = parse(&text).unwrap_err();
            assert!(error.contains("looks like a secret"), "{error}");
            assert!(!error.contains(&key), "the diagnostic must not echo the value");
        }
    }

    #[test]
    fn diagnostics_name_a_place_and_never_quote_the_text() {
        for text in [
            "schema_version = 1\nsecret = \"SYNTHETIC_PRIVATE_VALUE\n",
            "schema_version = 1\n[families.codex]\nconfig_home = 5\nnote = \"SYNTHETIC_PRIVATE_VALUE\"\n",
            "schema_version = \"SYNTHETIC_PRIVATE_VALUE\"\n",
            "schema_version = 1\n[families.codex]\nconfig_home = [\"SYNTHETIC_PRIVATE_VALUE\"]\n",
        ] {
            let error = parse(text).unwrap_err();
            assert!(!error.contains("SYNTHETIC_PRIVATE_VALUE"), "{error}");
            assert!(error.contains("line") || error.contains("credential"), "{error}");
        }
    }

    #[test]
    fn a_secret_shaped_or_credential_named_key_is_never_echoed() {
        let secret = format!("sk-{}", "a1".repeat(20));
        // A quoted key, plain and with its first letter written as an escape,
        // both naming a credential and carrying a secret shape; plus a quoted
        // unknown family name and a quoted key under a family.
        for text in [
            format!("schema_version = 1\n\"token {secret}\" = 1\n"),
            format!("schema_version = 1\n\"token \\u0073k-{}\" = 1\n", "a1".repeat(20)),
            format!("schema_version = 1\n[families.codex]\nconfig_home = \"/a\"\n\"api key {secret}\" = 1\n"),
            format!("schema_version = 1\n[families.\"{secret}\"]\nconfig_home = \"/a\"\n"),
            "schema_version = 1\n[families.codex]\nconfig_home = \"/a\"\n\"password SYNTHETIC_PRIVATE_KEY_TEXT\" = 1\n"
                .to_string(),
        ] {
            let error = parse(&text).unwrap_err();
            assert!(!error.contains(&secret), "{error}");
            assert!(!error.contains(&"a1".repeat(20)), "{error}");
            assert!(!error.contains("SYNTHETIC_PRIVATE_KEY_TEXT"), "{error}");
        }
    }

    #[test]
    fn unknown_keys_families_and_schema_are_refused() {
        assert!(
            parse("schema_version = 1\n[families.codex]\nconfig_home = \"/a\"\nnote = 1\n")
                .is_err()
        );
        assert!(
            parse("schema_version = 1\n[families.gemini]\nconfig_home = \"/a\"\n")
                .unwrap_err()
                .contains("unknown family")
        );
        assert!(parse("schema_version = 2\n")
            .unwrap_err()
            .contains("schema_version"));
        assert!(parse("schema_version = 1\n[families.codex]\n")
            .unwrap_err()
            .contains("maps nothing"));
    }

    #[test]
    fn config_homes_must_be_absolute_plain_and_outside_tracked_files() {
        let inside = format!("{ROOT}work/project/sub");
        let parent = format!("{ROOT}a/../b");
        for bad in ["relative/dir", &parent, &inside, "~"] {
            let text = format!("schema_version = 1\n[families.codex]\nconfig_home = \"{bad}\"\n");
            let result = Profile::parse(&text, Path::new("a.toml"), &project(), None);
            assert!(result.is_err(), "{bad}");
        }
        let local = format!(
            "schema_version = 1\n[families.codex]\n\
             config_home = \"{ROOT}work/project/.codeflow/local/codex\"\n"
        );
        assert!(parse(&local).is_ok());
    }

    #[test]
    fn a_fallback_needs_a_variable_name_and_a_positive_cap() {
        for cap in ["0", "-3", "1e9"] {
            let text = format!(
                "schema_version = 1\n[families.codex.api_fallback]\nkey_env = \"CODEX_KEY\"\n\
                 spend_cap_usd = {cap}\n"
            );
            assert!(parse(&text).unwrap_err().contains("spend_cap_usd"), "{cap}");
        }
        let missing =
            "schema_version = 1\n[families.codex.api_fallback]\nkey_env = \"CODEX_KEY\"\n";
        assert!(parse(missing).is_err());
    }

    #[test]
    fn the_shell_line_quotes_the_path_as_one_word() {
        let env = LaunchEnv {
            variable: "CODEX_HOME",
            config_home: PathBuf::from("/a b/it's"),
        };
        assert_eq!(
            env.shell_export().unwrap(),
            "export CODEX_HOME='/a b/it'\\''s'"
        );
        let bad = LaunchEnv {
            variable: "CODEX_HOME",
            config_home: PathBuf::from("/a\nb"),
        };
        assert!(bad.shell_export().is_err());
    }

    #[test]
    fn the_project_file_wins_over_the_user_file_and_a_missing_file_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("proj");
        let home = dir.path().join("home");
        std::fs::create_dir_all(root.join(LOCAL_DIR)).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        assert!(load(&root, Some(&home), None).unwrap().is_none());
        std::fs::write(
            home.join(PROFILE_FILE),
            format!("schema_version = 1\n[families.grok]\nconfig_home = \"{ROOT}srv/user-grok\"\n"),
        )
        .unwrap();
        let user = load(&root, Some(&home), None).unwrap().unwrap();
        assert!(user.launch_env(Family::Grok).is_some());
        std::fs::write(
            root.join(LOCAL_DIR).join(PROFILE_FILE),
            format!(
                "schema_version = 1\n[families.codex]\nconfig_home = \"{ROOT}srv/proj-codex\"\n"
            ),
        )
        .unwrap();
        let project = load(&root, Some(&home), None).unwrap().unwrap();
        assert!(project.launch_env(Family::Grok).is_none());
        assert!(project.launch_env(Family::Codex).is_some());
        std::fs::write(root.join(LOCAL_DIR).join(PROFILE_FILE), "token = 1\n").unwrap();
        let refused = load(&root, Some(&home), None).unwrap_err();
        assert!(refused.contains(PROFILE_FILE) && refused.contains("credential"));
    }
}
