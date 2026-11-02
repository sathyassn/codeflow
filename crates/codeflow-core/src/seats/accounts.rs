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

use super::{local_dir, shared_root, Family, LOCAL_DIR};
use crate::absence::proven_absent;

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

    /// Parse profile text read from `source`.
    ///
    /// `project_root` is the project's main checkout: a config home inside it
    /// is refused unless it sits under the untracked local folder, so a
    /// credential store cannot land in tracked files. `home` expands a leading
    /// `~`.
    ///
    /// # Errors
    /// Refuses a credential key or value, an unknown key, a relative or
    /// parent-walking path, a fallback without a valid cap or variable name,
    /// and a family table that maps nothing.
    pub fn parse(
        text: &str,
        source: &Path,
        project_root: &Path,
        home: Option<&Path>,
    ) -> Result<Profile, String> {
        let value: toml::Value = text
            .parse()
            .map_err(|error| format!("not valid TOML: {error}"))?;
        refuse_credentials(&value)?;
        for line in text.split('\n') {
            if let Some(kind) = crate::hooks::scan::scan_line(line) {
                return Err(format!(
                    "a line looks like a secret ({kind}); the profile holds paths and caps only"
                ));
            }
        }
        let raw: Raw = value
            .try_into()
            .map_err(|error: toml::de::Error| error.message().to_string())?;
        if raw.schema_version != 1 {
            return Err(format!(
                "unsupported schema_version {}; this CodeFlow reads 1",
                raw.schema_version
            ));
        }
        let mut families = BTreeMap::new();
        for (id, account) in raw.families {
            let family = Family::from_id(&id).ok_or_else(|| {
                format!("unknown family `{id}`; the families are claude, codex and grok")
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
                return Err(format!(
                    "`{key}` names a credential ({word}); the profile never holds a token, \
                     a key or any secret"
                ));
            }
            refuse_credentials(inner)?;
        }
    }
    Ok(())
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
    let shared = shared_root(project_root)?;
    let local = shared.join(LOCAL_DIR);
    let inside_project = expanded.starts_with(&shared);
    if inside_project && !expanded.starts_with(&local) {
        return Err(format!(
            "config_home `{text}` is inside the project; a sign-in stored there could be \
             committed. Use a folder outside the project, or one under {LOCAL_DIR}/"
        ));
    }
    Ok(expanded)
}

/// The profile files to try, first match wins: the project's untracked file,
/// then the user's `CodeFlow` home.
///
/// # Errors
/// As [`super::shared_root`].
pub fn candidate_paths(root: &Path, codeflow_home: Option<&Path>) -> Result<Vec<PathBuf>, String> {
    let mut paths = vec![local_dir(root)?.join(PROFILE_FILE)];
    if let Some(home) = codeflow_home {
        paths.push(home.join(PROFILE_FILE));
    }
    Ok(paths)
}

/// Load the profile for the project at `root`: `None` when no file exists.
///
/// # Errors
/// A file that exists but cannot be read, is a link, is too large, or is
/// refused by [`Profile::parse`]; the message names the file.
pub fn load(
    root: &Path,
    codeflow_home: Option<&Path>,
    user_home: Option<&Path>,
) -> Result<Option<Profile>, String> {
    for path in candidate_paths(root, codeflow_home)? {
        let unreadable = |error: &dyn std::fmt::Display| {
            format!(
                "{}: cannot read ({error}); the profile must be a regular file under 64 KiB",
                crate::git::display_path(&path)
            )
        };
        // Only a leaf proven missing under a folder that resolves skips a
        // file; any other state is read, and a failure refuses.
        if proven_absent(&path).map_err(|error| unreadable(&error))? {
            continue;
        }
        let bytes = crate::bounded_file::read_bounded_regular(&path, MAX_PROFILE_BYTES)
            .map_err(|error| unreadable(&error))?;
        let text = String::from_utf8(bytes)
            .map_err(|_| format!("{}: not UTF-8 text", crate::git::display_path(&path)))?;
        return Profile::parse(&text, &path, root, user_home)
            .map(Some)
            .map_err(|reason| format!("{}: {reason}", crate::git::display_path(&path)));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<Profile, String> {
        Profile::parse(
            text,
            Path::new("accounts.toml"),
            Path::new("/work/project"),
            Some(Path::new("/home/me")),
        )
    }

    const GOOD: &str = r#"
schema_version = 1
[families.codex]
config_home = "~/.codex-second"
[families.codex.api_fallback]
key_env = "CODEX_FALLBACK_API_KEY"
spend_cap_usd = 25
[families.claude]
config_home = "/srv/claude-work"
"#;

    #[test]
    fn a_profile_maps_a_family_to_its_own_variable_only() {
        let profile = parse(GOOD).unwrap();
        let codex = profile.launch_env(Family::Codex).unwrap();
        assert_eq!(codex.variable, "CODEX_HOME");
        assert_eq!(codex.config_home, Path::new("/home/me/.codex-second"));
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
        for bad in ["relative/dir", "/a/../b", "/work/project/sub", "~"] {
            let text = format!("schema_version = 1\n[families.codex]\nconfig_home = \"{bad}\"\n");
            let result =
                Profile::parse(&text, Path::new("a.toml"), Path::new("/work/project"), None);
            assert!(result.is_err(), "{bad}");
        }
        let local = "schema_version = 1\n[families.codex]\n\
                     config_home = \"/work/project/.codeflow/local/codex\"\n";
        assert!(parse(local).is_ok());
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
            "schema_version = 1\n[families.grok]\nconfig_home = \"/srv/user-grok\"\n",
        )
        .unwrap();
        let user = load(&root, Some(&home), None).unwrap().unwrap();
        assert!(user.launch_env(Family::Grok).is_some());
        std::fs::write(
            root.join(LOCAL_DIR).join(PROFILE_FILE),
            "schema_version = 1\n[families.codex]\nconfig_home = \"/srv/proj-codex\"\n",
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
