//! The shipped scaffold, embedded via rust-embed (charter §10).
//!
//! Debug builds read `assets/` from disk (rust-embed's default) for instant
//! scaffold iteration; release builds embed the bytes, keeping init offline,
//! instant, and version-locked: scaffold version ≡ binary version.

use codeflow_core::scaffold::AssetSource;

#[derive(rust_embed::RustEmbed)]
#[folder = "../../assets/"]
#[exclude = "docs-portal/starter/node_modules/**"]
struct Raw;

/// [`AssetSource`] over the embedded `assets/` tree.
pub struct EmbeddedAssets;

impl AssetSource for EmbeddedAssets {
    fn read(&self, path: &str) -> std::io::Result<Option<Vec<u8>>> {
        #[cfg(debug_assertions)]
        {
            let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../assets")
                .join(path);
            match std::fs::read(file) {
                Ok(bytes) => Ok(Some(bytes)),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(error) => Err(error),
            }
        }
        #[cfg(not(debug_assertions))]
        {
            Ok(Raw::get(path).map(|file| file.data.into_owned()))
        }
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
///
/// # Errors
/// Returns why a present template cannot be read or decoded.
pub fn read_test_template(name: &str) -> Result<Option<String>, String> {
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return Ok(None);
    }
    let Some(bytes) = EmbeddedAssets
        .read(&format!("{TEST_TEMPLATE_PREFIX}{name}"))
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|error| format!("cannot decode embedded template {name}: {error}"))
}

#[cfg(test)]
mod tests {

    #[cfg(debug_assertions)]
    #[test]
    fn r16_embedded_debug_read_error_is_not_missing() {
        assert!(super::EmbeddedAssets.read("base").is_err());
    }

    use super::*;

    #[test]
    fn shipped_test_templates_are_embedded_and_sorted() {
        let names = test_template_names();
        assert!(names.len() >= 9, "embedded templates: {names:?}");
        assert!(names.windows(2).all(|pair| pair[0] < pair[1]));
        for name in names {
            let content = read_test_template(&name)
                .expect("listed template is readable")
                .expect("listed template exists");
            let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
            assert_eq!(parsed["schema_version"], "1.0", "template {name}");
        }
    }

    #[test]
    fn embedded_template_read_rejects_paths() {
        for name in ["../minimal.json", "/minimal.json", "x\\minimal.json"] {
            assert!(
                read_test_template(name).unwrap().is_none(),
                "accepted {name}"
            );
        }
    }

    #[test]
    fn embedded_asset_inventory_excludes_portal_dependency_trees() {
        assert!(Raw::iter().all(|path| !path.starts_with("docs-portal/starter/node_modules/")));
    }
}
