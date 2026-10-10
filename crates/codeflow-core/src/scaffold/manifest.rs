//! The scaffold manifest — `assets/base/scaffold-manifest.toml`.
//!
//! The manifest is the single map from shipped assets to consumer-project
//! files. Each entry declares where an asset lands (`dest`), which
//! file-ownership class governs it (charter §4.3), at which tiers it is
//! installed (charter §4.2), whether it receives `{{PLACEHOLDER}}`
//! substitution, whether it needs the executable bit, and — for settings
//! presets — which permission preset selects it.
//!
//! The full field-by-field specification lives as comments at the top of
//! `assets/base/scaffold-manifest.toml`; this module is its typed mirror.
//! Entries may reference assets that do not exist yet (parallel authoring):
//! the engine skips them with a warning instead of failing.

use serde::{Deserialize, Serialize};

use super::{AssetSource, ScaffoldError};

/// Path of the manifest inside the asset tree.
pub const MANIFEST_ASSET_PATH: &str = "base/scaffold-manifest.toml";

/// Scaffold tier (charter §4.2). Ordered: `Minimal < Standard < Full`;
/// re-running init at a higher tier is an additive upgrade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Minimal,
    Standard,
    Full,
}

impl Tier {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Minimal => "minimal",
            Self::Standard => "standard",
            Self::Full => "full",
        }
    }
}

impl std::str::FromStr for Tier {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "minimal" => Ok(Self::Minimal),
            "standard" => Ok(Self::Standard),
            "full" => Ok(Self::Full),
            other => Err(format!("unknown tier: {other}")),
        }
    }
}

impl std::fmt::Display for Tier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// File-ownership class — the update contract (charter §4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Ownership {
    /// Fully managed: hash recorded, baseline kept, replaced when unmodified,
    /// 3-way merged when user-modified, `.new` + report on conflict.
    Managed,
    /// Only a marked region (markdown/hash markers) or a known key set (JSON)
    /// belongs to codeflow; the rest of the file is never touched.
    ManagedRegion,
    /// Written only if absent; never mutated. Schema-versioned JSON may gain
    /// new keys with defaults, reported.
    UserOwned,
}

/// How a `managed-region` entry marks its region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RegionFormat {
    /// `<!-- codeflow:managed:begin ... -->` … `<!-- codeflow:managed:end -->`
    /// (short `codeflow:begin`/`codeflow:end` forms also recognized).
    Markdown,
    /// `# codeflow:managed:begin` … `# codeflow:managed:end` comment markers.
    Hash,
    /// Structured JSON merge (`.claude/settings.json`): codeflow hook entries
    /// (command prefix `codeflow `) and permission entries are managed;
    /// every other key is preserved.
    Json,
}

/// One asset → consumer-file mapping.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry {
    /// Asset path relative to `assets/base/`.
    pub src: String,
    /// Destination path relative to the consumer project root.
    pub dest: String,
    /// Ownership class governing install and update behavior.
    pub ownership: Ownership,
    /// Tiers at which this entry is installed.
    pub tiers: Vec<Tier>,
    /// Set the executable mode bit on the written file (git hook shims).
    #[serde(default)]
    pub exec: bool,
    /// Apply `{{PLACEHOLDER}}` substitution (known keys only; unknown
    /// placeholders survive untouched).
    #[serde(default)]
    pub template: bool,
    /// When set, the entry applies only if the permission preset chosen at
    /// init matches.
    #[serde(default)]
    pub preset: Option<String>,
    /// Region marker format; required when `ownership = "managed-region"`.
    #[serde(default)]
    pub region: Option<RegionFormat>,
}

impl ManifestEntry {
    /// Whether this entry is installed at `tier` under `preset`.
    #[must_use]
    pub fn applies(&self, tier: Tier, preset: &str) -> bool {
        self.tiers.contains(&tier) && self.preset.as_deref().is_none_or(|p| p == preset)
    }
}

/// The parsed scaffold manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScaffoldManifest {
    pub schema_version: u32,
    #[serde(rename = "entry", default)]
    pub entries: Vec<ManifestEntry>,
}

impl ScaffoldManifest {
    /// Loads and validates the manifest from the asset source.
    ///
    /// # Errors
    ///
    /// Fails when the manifest asset is missing, not valid TOML, or violates
    /// the structural rules (region format, tiers, path hygiene).
    pub fn load(source: &dyn AssetSource) -> Result<Self, ScaffoldError> {
        let text = super::assets::read_text(source, MANIFEST_ASSET_PATH)?
            .ok_or_else(|| ScaffoldError::ManifestMissing(MANIFEST_ASSET_PATH.to_string()))?;
        let manifest: Self =
            toml::from_str(&text).map_err(|e| ScaffoldError::ManifestInvalid(e.to_string()))?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Whether `bytes` are exactly what the scaffold installs verbatim at
    /// `dest`: a whole-file `managed` entry for `dest`, with no template
    /// substitution, whose asset in `source` has these bytes. The proof is the
    /// shipped asset itself, never a project's installed-file record, which
    /// the project can write. Managed regions, templated and user-owned files,
    /// and assets missing from `source` never match.
    #[must_use]
    pub fn installs_verbatim(&self, source: &dyn AssetSource, dest: &str, bytes: &[u8]) -> bool {
        self.entries.iter().any(|entry| {
            entry.dest == dest
                && entry.ownership == Ownership::Managed
                && !entry.template
                && source
                    .read(&format!("base/{}", entry.src))
                    .is_ok_and(|asset| asset.is_some_and(|asset| asset == bytes))
        })
    }

    fn validate(&self) -> Result<(), ScaffoldError> {
        for entry in &self.entries {
            if entry.ownership == Ownership::ManagedRegion && entry.region.is_none() {
                return Err(ScaffoldError::ManifestInvalid(format!(
                    "managed-region entry {} must declare a region format",
                    entry.dest
                )));
            }
            if entry.tiers.is_empty() {
                return Err(ScaffoldError::ManifestInvalid(format!(
                    "entry {} declares no tiers",
                    entry.dest
                )));
            }
            for bad in ["..", "//"] {
                if entry.dest.contains(bad) || entry.src.contains(bad) {
                    return Err(ScaffoldError::ManifestInvalid(format!(
                        "entry {} contains a suspicious path segment",
                        entry.dest
                    )));
                }
            }
            if entry.dest.starts_with('/') || entry.src.starts_with('/') {
                return Err(ScaffoldError::ManifestInvalid(format!(
                    "entry {} must use relative paths",
                    entry.dest
                )));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_manifest_parses_and_validates() {
        // The real manifest at assets/base/ must always stay loadable.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        let source = DirSourceForTest(root);
        let manifest = ScaffoldManifest::load(&source).expect("shipped manifest loads");
        assert_eq!(manifest.schema_version, 1);
        assert!(
            manifest.entries.len() >= 20,
            "expected a populated manifest"
        );
        // AGENTS.md is present at every tier: exactly one entry applies per tier
        // (two entries, one dest, selected by tier — the tier-honest split), and
        // the minimal tier gets the trimmed template.
        for tier in [Tier::Minimal, Tier::Standard, Tier::Full] {
            let applicable: Vec<&ManifestEntry> = manifest
                .entries
                .iter()
                .filter(|e| e.dest == "AGENTS.md" && e.applies(tier, "default"))
                .collect();
            assert_eq!(applicable.len(), 1, "one AGENTS.md entry applies at {tier}");
            assert_eq!(applicable[0].ownership, Ownership::ManagedRegion);
        }
        let minimal_agents = manifest
            .entries
            .iter()
            .find(|e| e.dest == "AGENTS.md" && e.applies(Tier::Minimal, "default"))
            .expect("a minimal-tier AGENTS.md entry");
        assert_eq!(minimal_agents.src, "AGENTS.minimal.md.tmpl");
        // Exactly one settings entry applies per preset.
        for preset in ["default", "acceptEdits", "bypassPermissions"] {
            let count = manifest
                .entries
                .iter()
                .filter(|e| e.dest == ".claude/settings.json" && e.applies(Tier::Standard, preset))
                .count();
            assert_eq!(count, 1, "one settings entry for preset {preset}");
        }
    }

    #[test]
    fn tier_ordering_and_parsing() {
        assert!(Tier::Minimal < Tier::Standard && Tier::Standard < Tier::Full);
        assert_eq!("standard".parse::<Tier>().unwrap(), Tier::Standard);
        assert!("nope".parse::<Tier>().is_err());
    }

    #[test]
    fn managed_region_requires_region_format() {
        let toml = r#"
            schema_version = 1
            [[entry]]
            src = "a"
            dest = "b"
            ownership = "managed-region"
            tiers = ["standard"]
        "#;
        let manifest: ScaffoldManifest = toml::from_str(toml).unwrap();
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn installs_verbatim_needs_a_whole_file_managed_asset_with_these_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("base");
        std::fs::create_dir_all(&base).unwrap();
        for name in ["skill.md", "region.md", "template.md", "owned.md"] {
            std::fs::write(base.join(name), "shipped\n").unwrap();
        }
        let toml = r#"
            schema_version = 1
            [[entry]]
            src = "skill.md"
            dest = ".agents/skills/cf-x/SKILL.md"
            ownership = "managed"
            tiers = ["standard"]
            [[entry]]
            src = "region.md"
            dest = "AGENTS.md"
            ownership = "managed-region"
            region = "markdown"
            tiers = ["standard"]
            [[entry]]
            src = "template.md"
            dest = "docs/product.md"
            ownership = "managed"
            template = true
            tiers = ["standard"]
            [[entry]]
            src = "owned.md"
            dest = "docs/owned.md"
            ownership = "user-owned"
            tiers = ["standard"]
            [[entry]]
            src = "absent.md"
            dest = "docs/absent.md"
            ownership = "managed"
            tiers = ["standard"]
        "#;
        let manifest: ScaffoldManifest = toml::from_str(toml).unwrap();
        let source = DirSourceForTest(dir.path().to_path_buf());
        let verbatim = |dest: &str, bytes: &[u8]| manifest.installs_verbatim(&source, dest, bytes);
        assert!(verbatim(".agents/skills/cf-x/SKILL.md", b"shipped\n"));
        assert!(!verbatim(".agents/skills/cf-x/SKILL.md", b"edited\n"));
        for dest in [
            "AGENTS.md",
            "docs/product.md",
            "docs/owned.md",
            "docs/absent.md",
        ] {
            assert!(!verbatim(dest, b"shipped\n"), "{dest}");
        }
        assert!(!verbatim("docs/notes.md", b"shipped\n"));
    }

    struct DirSourceForTest(std::path::PathBuf);

    impl AssetSource for DirSourceForTest {
        fn read(&self, path: &str) -> std::io::Result<Option<Vec<u8>>> {
            std::fs::read(self.0.join(path)).map(Some)
        }
    }
}
