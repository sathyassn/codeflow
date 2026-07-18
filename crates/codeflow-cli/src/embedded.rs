//! The shipped scaffold, embedded via rust-embed (charter §10).
//!
//! Debug builds read `assets/` from disk (rust-embed's default) for instant
//! scaffold iteration; release builds embed the bytes, keeping init offline,
//! instant, and version-locked: scaffold version ≡ binary version.

use codeflow_core::scaffold::AssetSource;

#[derive(rust_embed::RustEmbed)]
#[folder = "../../assets/"]
struct Raw;

/// [`AssetSource`] over the embedded `assets/` tree.
pub struct EmbeddedAssets;

impl AssetSource for EmbeddedAssets {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        Raw::get(path).map(|file| file.data.into_owned())
    }
}

const TEST_TEMPLATE_PREFIX: &str = "base/testing/templates/";

/// Sorted names of test-config templates embedded in this binary.
pub fn test_template_names() -> Vec<String> {
    let mut names: Vec<String> = Raw::iter()
        .filter_map(|path| path.strip_prefix(TEST_TEMPLATE_PREFIX).map(str::to_string))
        .filter(|name| {
            !name.contains('/')
                && std::path::Path::new(name)
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
        })
        .collect();
    names.sort();
    names
}

/// Read one embedded test-config template by basename.
pub fn read_test_template(name: &str) -> Option<String> {
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return None;
    }
    let bytes = Raw::get(&format!("{TEST_TEMPLATE_PREFIX}{name}"))?;
    String::from_utf8(bytes.data.into_owned()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_test_templates_are_embedded_and_sorted() {
        let names = test_template_names();
        assert!(names.len() >= 9, "embedded templates: {names:?}");
        assert!(names.windows(2).all(|pair| pair[0] < pair[1]));
        for name in names {
            let content = read_test_template(&name).expect("listed template is readable");
            let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
            assert_eq!(parsed["schema_version"], "1.0", "template {name}");
        }
    }

    #[test]
    fn embedded_template_read_rejects_paths() {
        for name in ["../minimal.json", "/minimal.json", "x\\minimal.json"] {
            assert!(read_test_template(name).is_none(), "accepted {name}");
        }
    }
}
