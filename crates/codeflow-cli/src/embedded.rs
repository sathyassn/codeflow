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
