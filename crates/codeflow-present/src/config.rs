use std::{
    fmt::Write as _,
    fs::{self, OpenOptions},
    io::Read as _,
    path::{Component, Path, PathBuf},
};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};

use crate::{error::PresentError, limits, platform::is_link_like, Result};

const MAX_CONFIG_BYTES: u64 = 64 * 1024;
const MAX_TOKEN_BYTES: u64 = 256 * 1024;
const MAX_IDENTITY_BYTES: usize = 128 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProjectConfig {
    pub schema_version: u32,
    #[serde(default)]
    pub retention: RetentionPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primitive_tokens: Option<PrimitiveTokenSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RetentionPolicy {
    pub closed_days: u64,
    pub max_closed_sessions: usize,
    pub max_project_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PrimitiveTokenSource {
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct UtilityTokens {
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colors: Option<ModeColors>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub typography: Option<TypographyTokens>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spacing: Option<SpacingTokens>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radius: Option<RadiusTokens>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<IdentityToken>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ModeColors {
    pub light: ColorTokens,
    pub dark: ColorTokens,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ColorTokens {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canvas: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TypographyTokens {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sans_family: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mono_family: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reading_measure_ch: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SpacingTokens {
    pub scale_percent: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RadiusTokens {
    pub radius_px: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct IdentityToken {
    pub mime_type: String,
    pub data_base64: String,
    pub alt: String,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            schema_version: limits::SCHEMA_VERSION,
            retention: RetentionPolicy::default(),
            primitive_tokens: None,
        }
    }
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            closed_days: limits::CLOSED_RETENTION_DAYS,
            max_closed_sessions: limits::MAX_CLOSED_SESSIONS,
            max_project_bytes: limits::MAX_PROJECT_STATE_BYTES,
        }
    }
}

impl ProjectConfig {
    pub fn load(project_root: &Path) -> Result<Self> {
        let relative = Path::new(".codeflow/present/config.toml");
        let Some(path) = resolve_optional_project_file(project_root, relative, MAX_CONFIG_BYTES)?
        else {
            return Ok(Self::default());
        };
        let raw = read_bounded_file(&path, MAX_CONFIG_BYTES)?;
        let raw = String::from_utf8(raw).map_err(|_| {
            PresentError::InvalidDocument("present config is not UTF-8".to_string())
        })?;
        let config: Self = toml::from_str(&raw).map_err(|error| {
            PresentError::InvalidDocument(format!("invalid cf-present config: {error}"))
        })?;
        config.validate(project_root)?;
        Ok(config)
    }

    pub fn load_tokens(&self, project_root: &Path) -> Result<Option<UtilityTokens>> {
        let Some(source) = &self.primitive_tokens else {
            return Ok(None);
        };
        let path = resolve_existing_project_file(project_root, &source.path, MAX_TOKEN_BYTES)?;
        let raw = read_bounded_file(&path, MAX_TOKEN_BYTES)?;
        let tokens: UtilityTokens = serde_json::from_slice(&raw)?;
        tokens.validate()?;
        Ok(Some(tokens))
    }

    fn validate(&self, project_root: &Path) -> Result<()> {
        if self.schema_version != limits::SCHEMA_VERSION {
            return Err(PresentError::UnsupportedSchema {
                found: self.schema_version,
                supported: limits::SCHEMA_VERSION,
            });
        }
        if !(1..=3_650).contains(&self.retention.closed_days)
            || !(1..=10_000).contains(&self.retention.max_closed_sessions)
            || !(1024 * 1024..=10 * 1024 * 1024 * 1024).contains(&self.retention.max_project_bytes)
        {
            return Err(PresentError::InvalidDocument(
                "cf-present retention values are outside supported bounds".to_string(),
            ));
        }
        if let Some(source) = &self.primitive_tokens {
            resolve_existing_project_file(project_root, &source.path, MAX_TOKEN_BYTES)?;
        }
        Ok(())
    }
}

fn read_bounded_file(path: &Path, max_bytes: u64) -> Result<Vec<u8>> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options
        .open(path)
        .map_err(|error| PresentError::io(path, error))?;
    let metadata = file
        .metadata()
        .map_err(|error| PresentError::io(path, error))?;
    if !metadata.is_file() || metadata.len() > max_bytes {
        return Err(PresentError::UnsafePath(path.to_path_buf()));
    }
    let capacity = usize::try_from(metadata.len())
        .map_err(|_| PresentError::UnsafePath(path.to_path_buf()))?;
    let mut bytes = Vec::with_capacity(capacity);
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| PresentError::io(path, error))?;
    if bytes.len() as u64 > max_bytes {
        return Err(PresentError::UnsafePath(path.to_path_buf()));
    }
    Ok(bytes)
}

fn resolve_optional_project_file(
    project_root: &Path,
    relative: &Path,
    max_bytes: u64,
) -> Result<Option<PathBuf>> {
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(PresentError::UnsafePath(relative.to_path_buf()));
    }

    let components = relative.components().collect::<Vec<_>>();
    let mut candidate = project_root.to_path_buf();
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(component) = component else {
            return Err(PresentError::UnsafePath(relative.to_path_buf()));
        };
        candidate.push(component);
        match fs::symlink_metadata(&candidate) {
            Ok(metadata) => {
                if is_link_like(&metadata) {
                    return Err(PresentError::UnsafePath(candidate));
                }
                let leaf = index + 1 == components.len();
                if (leaf && (!metadata.is_file() || metadata.len() > max_bytes))
                    || (!leaf && !metadata.is_dir())
                {
                    return Err(PresentError::UnsafePath(candidate));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(PresentError::io(&candidate, error)),
        }
    }

    let canonical_root = project_root
        .canonicalize()
        .map_err(|error| PresentError::io(project_root, error))?;
    let canonical_path = candidate
        .canonicalize()
        .map_err(|error| PresentError::io(&candidate, error))?;
    if !canonical_path.starts_with(&canonical_root) {
        return Err(PresentError::UnsafePath(candidate));
    }
    Ok(Some(canonical_path))
}

impl UtilityTokens {
    fn validate(&self) -> Result<()> {
        if self.schema_version != limits::SCHEMA_VERSION {
            return Err(PresentError::UnsupportedSchema {
                found: self.schema_version,
                supported: limits::SCHEMA_VERSION,
            });
        }
        if let Some(colors) = &self.colors {
            validate_colors(&colors.light)?;
            validate_colors(&colors.dark)?;
        }
        if let Some(typography) = &self.typography {
            for family in [
                typography.sans_family.as_deref(),
                typography.mono_family.as_deref(),
            ]
            .into_iter()
            .flatten()
            {
                if family.is_empty()
                    || family.len() > 96
                    || !family.chars().all(|character| {
                        character.is_ascii_alphanumeric()
                            || matches!(character, ' ' | '-' | '_' | '.')
                    })
                {
                    return Err(PresentError::InvalidDocument(
                        "utility token font family is not a safe primitive".to_string(),
                    ));
                }
            }
            if typography
                .reading_measure_ch
                .is_some_and(|measure| !(45..=100).contains(&measure))
            {
                return Err(PresentError::InvalidDocument(
                    "utility token reading measure is outside 45..=100ch".to_string(),
                ));
            }
        }
        if self
            .spacing
            .as_ref()
            .is_some_and(|spacing| !(75..=150).contains(&spacing.scale_percent))
            || self
                .radius
                .as_ref()
                .is_some_and(|radius| radius.radius_px > 32)
        {
            return Err(PresentError::InvalidDocument(
                "utility spacing or radius token is outside its bound".to_string(),
            ));
        }
        if let Some(identity) = &self.identity {
            if !matches!(identity.mime_type.as_str(), "image/png" | "image/webp")
                || identity.alt.trim().is_empty()
                || identity.alt.len() > limits::MAX_TITLE_BYTES
            {
                return Err(PresentError::InvalidDocument(
                    "utility identity token has an unsupported MIME or alternative text"
                        .to_string(),
                ));
            }
            let decoded = STANDARD.decode(&identity.data_base64).map_err(|_| {
                PresentError::InvalidDocument(
                    "utility identity data_base64 is not valid base64".to_string(),
                )
            })?;
            if decoded.len() > MAX_IDENTITY_BYTES {
                return Err(PresentError::InvalidDocument(
                    "utility identity asset exceeds its decoded byte bound".to_string(),
                ));
            }
            let valid_signature = match identity.mime_type.as_str() {
                "image/png" => decoded.starts_with(b"\x89PNG\r\n\x1a\n"),
                "image/webp" => {
                    decoded.len() >= 12 && &decoded[..4] == b"RIFF" && &decoded[8..12] == b"WEBP"
                }
                _ => false,
            };
            if !valid_signature {
                return Err(PresentError::InvalidDocument(
                    "utility identity bytes do not match the declared MIME".to_string(),
                ));
            }
        }
        Ok(())
    }

    #[must_use]
    pub fn css(&self) -> String {
        let mut css = String::new();
        if let Some(typography) = &self.typography {
            css.push_str(":root[data-cf-theme]{");
            if let Some(family) = &typography.sans_family {
                css.push_str("--cf-font-sans:\"");
                css.push_str(family);
                css.push_str("\",ui-sans-serif,sans-serif;");
            }
            if let Some(family) = &typography.mono_family {
                css.push_str("--cf-font-mono:\"");
                css.push_str(family);
                css.push_str("\",ui-monospace,monospace;");
            }
            if let Some(measure) = typography.reading_measure_ch {
                write!(css, "--cf-reading-measure:{measure}ch;")
                    .expect("writing to a String cannot fail");
            }
            css.push('}');
        }
        if let Some(spacing) = &self.spacing {
            let scale = f64::from(spacing.scale_percent) / 100.0;
            css.push_str(":root[data-cf-theme]{");
            for (index, base) in [0.25, 0.5, 0.75, 1.0, 1.5, 2.0, 3.0].iter().enumerate() {
                write!(css, "--cf-space-{}:{:.4}rem;", index + 1, base * scale)
                    .expect("writing to a String cannot fail");
            }
            css.push('}');
        }
        if let Some(radius) = &self.radius {
            write!(
                css,
                ":root[data-cf-theme]{{--cf-radius:{}px;}}",
                radius.radius_px
            )
            .expect("writing to a String cannot fail");
        }
        if let Some(colors) = &self.colors {
            append_color_css(&mut css, "light", &colors.light);
            append_color_css(&mut css, "dark", &colors.dark);
            css.push_str(":root[data-cf-theme][data-cf-mode=\"system\"]{");
            append_color_declarations(&mut css, &colors.light);
            css.push_str(
                "}@media(prefers-color-scheme:dark){:root[data-cf-theme][data-cf-mode=\"system\"]{",
            );
            append_color_declarations(&mut css, &colors.dark);
            css.push_str("}}");
        }
        css
    }

    #[must_use]
    pub fn identity_data_url(&self) -> Option<String> {
        self.identity.as_ref().map(|identity| {
            format!(
                "data:{};base64,{}",
                identity.mime_type, identity.data_base64
            )
        })
    }
}

fn validate_colors(colors: &ColorTokens) -> Result<()> {
    let values = [
        colors.canvas.as_deref(),
        colors.surface.as_deref(),
        colors.text.as_deref(),
        colors.accent.as_deref(),
        colors.focus.as_deref(),
    ];
    if values.iter().any(Option::is_none) {
        return Err(PresentError::InvalidDocument(
            "utility color modes must define canvas, surface, text, accent, and focus together"
                .to_string(),
        ));
    }
    let [canvas, surface, text, accent, focus] = values
        .map(|value| parse_opaque_color(value.expect("the complete color set was checked above")));
    let [canvas, surface, text, accent, focus] = [canvas?, surface?, text?, accent?, focus?];
    if contrast_ratio(text, canvas) < 4.5 || contrast_ratio(text, surface) < 4.5 {
        return Err(PresentError::InvalidDocument(
            "utility text must have at least 4.5:1 contrast against canvas and surface".to_string(),
        ));
    }
    if contrast_ratio(accent, surface) < 4.5 {
        return Err(PresentError::InvalidDocument(
            "utility accent must have at least 4.5:1 contrast against surface".to_string(),
        ));
    }
    if contrast_ratio(focus, canvas) < 3.0 || contrast_ratio(focus, surface) < 3.0 {
        return Err(PresentError::InvalidDocument(
            "utility focus must have at least 3:1 contrast against canvas and surface".to_string(),
        ));
    }
    Ok(())
}

fn parse_opaque_color(value: &str) -> Result<[u8; 3]> {
    if value.len() != 7
        || !value.starts_with('#')
        || !value[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(PresentError::InvalidDocument(
            "utility colors must be opaque #RRGGBB values".to_string(),
        ));
    }
    let component = |start| {
        u8::from_str_radix(&value[start..start + 2], 16).map_err(|_| {
            PresentError::InvalidDocument("utility color component is invalid".to_string())
        })
    };
    Ok([component(1)?, component(3)?, component(5)?])
}

fn contrast_ratio(left: [u8; 3], right: [u8; 3]) -> f64 {
    let left = relative_luminance(left);
    let right = relative_luminance(right);
    (left.max(right) + 0.05) / (left.min(right) + 0.05)
}

fn relative_luminance(color: [u8; 3]) -> f64 {
    let [red, green, blue] = color.map(|component| {
        let channel = f64::from(component) / 255.0;
        if channel <= 0.040_45 {
            channel / 12.92
        } else {
            ((channel + 0.055) / 1.055).powf(2.4)
        }
    });
    0.2126 * red + 0.7152 * green + 0.0722 * blue
}

fn append_color_css(css: &mut String, mode: &str, colors: &ColorTokens) {
    write!(
        css,
        ":root[data-cf-theme][data-cf-mode-resolved=\"{mode}\"]{{"
    )
    .expect("writing to a String cannot fail");
    append_color_declarations(css, colors);
    css.push('}');
}

fn append_color_declarations(css: &mut String, colors: &ColorTokens) {
    for (name, value) in [
        ("canvas", colors.canvas.as_deref()),
        ("surface", colors.surface.as_deref()),
        ("text", colors.text.as_deref()),
        ("accent", colors.accent.as_deref()),
        ("focus", colors.focus.as_deref()),
    ] {
        if let Some(value) = value {
            write!(css, "--cf-{name}:{value};").expect("writing to a String cannot fail");
        }
    }
}

fn resolve_existing_project_file(
    project_root: &Path,
    relative: &Path,
    max_bytes: u64,
) -> Result<PathBuf> {
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(PresentError::UnsafePath(relative.to_path_buf()));
    }
    let path = project_root.join(relative);
    let metadata = fs::symlink_metadata(&path).map_err(|error| PresentError::io(&path, error))?;
    if !metadata.is_file() || is_link_like(&metadata) || metadata.len() > max_bytes {
        return Err(PresentError::UnsafePath(path));
    }
    let canonical_root = project_root
        .canonicalize()
        .map_err(|error| PresentError::io(project_root, error))?;
    let canonical_path = path
        .canonicalize()
        .map_err(|error| PresentError::io(&path, error))?;
    if !canonical_path.starts_with(&canonical_root) {
        return Err(PresentError::UnsafePath(path));
    }
    Ok(canonical_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_config(root: &Path, body: &str) {
        let dir = root.join(".codeflow/present");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("config.toml"), body).unwrap();
    }

    #[test]
    fn absent_config_uses_concrete_retention_defaults() {
        let temp = tempfile::tempdir().unwrap();
        assert_eq!(
            ProjectConfig::load(temp.path()).unwrap(),
            ProjectConfig::default()
        );
    }

    #[cfg(unix)]
    #[test]
    fn absent_config_rejects_dangling_leaf_and_parent_symlinks() {
        use std::os::unix::fs::symlink;

        let leaf = tempfile::tempdir().unwrap();
        fs::create_dir_all(leaf.path().join(".codeflow/present")).unwrap();
        symlink(
            leaf.path().join("missing.toml"),
            leaf.path().join(".codeflow/present/config.toml"),
        )
        .unwrap();
        assert!(matches!(
            ProjectConfig::load(leaf.path()),
            Err(PresentError::UnsafePath(_))
        ));

        let parent = tempfile::tempdir().unwrap();
        fs::create_dir_all(parent.path().join(".codeflow")).unwrap();
        symlink(
            parent.path().join("missing-present"),
            parent.path().join(".codeflow/present"),
        )
        .unwrap();
        assert!(matches!(
            ProjectConfig::load(parent.path()),
            Err(PresentError::UnsafePath(_))
        ));
    }

    #[test]
    fn config_is_closed() {
        let temp = tempfile::tempdir().unwrap();
        write_config(temp.path(), "schema_version=1\nunknown=true\n");
        assert!(ProjectConfig::load(temp.path()).is_err());
    }

    #[test]
    fn closed_primitive_tokens_render_only_validated_css_and_identity_data() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join("tokens.json"),
            r##"{
              "schema_version": 1,
              "colors": {
                "light": {"canvas":"#ffffff","surface":"#ffffff","text":"#000000","accent":"#0000aa","focus":"#005fcc"},
                "dark": {"canvas":"#000000","surface":"#000000","text":"#ffffff","accent":"#ffff00","focus":"#00ffff"}
              },
              "typography": {"sans_family":"Atkinson Hyperlegible","reading_measure_ch":68},
              "spacing": {"scale_percent":110},
              "radius": {"radius_px":8},
              "identity": {"mime_type":"image/png","data_base64":"iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=","alt":"Project mark"}
            }"##,
        )
        .unwrap();
        write_config(
            temp.path(),
            "schema_version=1\n[primitive_tokens]\npath=\"tokens.json\"\n",
        );

        let tokens = ProjectConfig::load(temp.path())
            .unwrap()
            .load_tokens(temp.path())
            .unwrap()
            .unwrap();
        let css = tokens.css();
        assert!(css.contains("--cf-canvas:#ffffff"));
        assert!(css.contains("--cf-font-sans:\"Atkinson Hyperlegible\""));
        assert!(css.contains("data-cf-mode=\"system\""));
        assert!(css.contains("@media(prefers-color-scheme:dark)"));
        assert!(!css.contains("</style>"));
        assert_eq!(
            tokens.identity_data_url().as_deref(),
            Some("data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=")
        );
    }

    #[test]
    fn primitive_tokens_reject_unknown_or_css_capable_values() {
        for raw in [
            r#"{"schema_version":1,"unknown":true}"#,
            r#"{"schema_version":1,"colors":{"light":{"canvas":"red"},"dark":{}}}"#,
            r##"{"schema_version":1,"colors":{"light":{"canvas":"#ffffff","surface":"#ffffff","text":"#999999","accent":"#aaaaaa","focus":"#cccccc"},"dark":{"canvas":"#000000","surface":"#000000","text":"#ffffff","accent":"#ffff00","focus":"#00ffff"}}}"##,
            r#"{"schema_version":1,"typography":{"sans_family":"safe;}</style><script>"}}"#,
            r#"{"schema_version":1,"spacing":{"scale_percent":151}}"#,
            r#"{"schema_version":1,"radius":{"radius_px":33}}"#,
            r#"{"schema_version":1,"identity":{"mime_type":"image/svg+xml","data_base64":"PHN2Zz4=","alt":"mark"}}"#,
            r#"{"schema_version":1,"identity":{"mime_type":"image/png","data_base64":"aGVsbG8=","alt":"mark"}}"#,
        ] {
            let parsed = serde_json::from_str::<UtilityTokens>(raw);
            assert!(parsed
                .and_then(|tokens| tokens.validate().map_err(|error| {
                    serde_json::Error::io(std::io::Error::other(error.to_string()))
                }))
                .is_err());
        }
    }

    #[cfg(unix)]
    #[test]
    fn primitive_token_source_rejects_leaf_and_parent_symlink_escape() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(
            outside.path().join("tokens.json"),
            r#"{"schema_version":1}"#,
        )
        .unwrap();

        symlink(
            outside.path().join("tokens.json"),
            temp.path().join("leaf.json"),
        )
        .unwrap();
        write_config(
            temp.path(),
            "schema_version=1\n[primitive_tokens]\npath=\"leaf.json\"\n",
        );
        assert!(matches!(
            ProjectConfig::load(temp.path()),
            Err(PresentError::UnsafePath(_))
        ));

        fs::remove_file(temp.path().join(".codeflow/present/config.toml")).unwrap();
        symlink(outside.path(), temp.path().join("linked-parent")).unwrap();
        write_config(
            temp.path(),
            "schema_version=1\n[primitive_tokens]\npath=\"linked-parent/tokens.json\"\n",
        );
        assert!(matches!(
            ProjectConfig::load(temp.path()),
            Err(PresentError::UnsafePath(_))
        ));
    }
}
