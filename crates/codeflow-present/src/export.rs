use std::{fmt::Write as _, io::Write as _, path::Path};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    error::{PresentError, Result},
    limits,
    render::{render_document, render_unsupported, RenderOptions},
    service::{load_manifest, EmbeddedAssets},
    state::{discard_new_file, open_private_create_new, RevisionContent, SessionStore},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportTheme {
    Editorial,
    Technical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportMode {
    System,
    Light,
    Dark,
}

pub fn export_session(
    store: &SessionStore,
    session_id: Uuid,
    output: &Path,
    theme: ExportTheme,
    mode: ExportMode,
) -> Result<()> {
    let revision = store.current_revision(session_id)?;
    let utility_style = store.utility_tokens()?.map(|tokens| tokens.css());
    let html = match revision.content {
        RevisionContent::Supported { document } => {
            let static_html = render_document(
                &document,
                &RenderOptions {
                    session_id: "export",
                    revision: revision.revision,
                    event_sequence: 0,
                    script_path: None,
                    style_path: None,
                    prepaint_source: None,
                    utility_style: None,
                    identity: None,
                    feedback: None,
                    read_only_warning: None,
                    interactive: false,
                },
            );
            let base_bytes = static_html.len();
            let enhanced = enhance_export(
                static_html,
                theme,
                mode,
                utility_style.as_deref().unwrap_or_default(),
            )?;
            if u64::try_from(enhanced.len().saturating_sub(base_bytes)).unwrap_or(u64::MAX)
                > limits::MAX_EXPORT_SHELL_BYTES
            {
                return Err(PresentError::CorruptState(format!(
                    "generated export shell exceeds {} bytes",
                    limits::MAX_EXPORT_SHELL_BYTES
                )));
            }
            enhanced
        }
        RevisionContent::Unsupported {
            schema_version,
            raw,
        } => render_unsupported(&raw, schema_version),
    };
    write_new_private(output, html.as_bytes())
}

fn enhance_export(
    mut html: String,
    theme: ExportTheme,
    mode: ExportMode,
    utility_style: &str,
) -> Result<String> {
    let manifest = load_manifest()?;
    let asset = &manifest.export.renderer;
    if asset.content_encoding != "gzip"
        || !asset.media_type.starts_with("text/javascript")
        || asset.raw_bytes > limits::MAX_RAW_ASSET_BYTES
        || asset.encoded_bytes > limits::MAX_EXPORT_PAYLOAD_BYTES
        || !asset.stored_path.starts_with("export/")
        || asset.stored_path.contains("..")
    {
        return Err(PresentError::CorruptState(
            "offline export asset violates its manifest contract".to_string(),
        ));
    }
    let bytes = EmbeddedAssets::get(&asset.stored_path).ok_or_else(|| {
        PresentError::CorruptState("offline export renderer is not embedded".to_string())
    })?;
    if u64::try_from(bytes.data.len()).ok() != Some(asset.encoded_bytes) {
        return Err(PresentError::CorruptState(
            "offline export renderer size does not match its manifest".to_string(),
        ));
    }
    let digest = Sha256::digest(&bytes.data);
    let mut actual = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(actual, "{byte:02x}").expect("writing to a String cannot fail");
    }
    if actual != asset.sha256 {
        return Err(PresentError::CorruptState(
            "offline export renderer does not match its SHA-256".to_string(),
        ));
    }
    let encoded = STANDARD.encode(&bytes.data);
    let bootstrap = "(async()=>{const e=document.getElementById('cf-present-export-payload').textContent.trim();const b=Uint8Array.from(atob(e),c=>c.charCodeAt(0));if(!globalThis.DecompressionStream)return;const s=new Blob([b]).stream().pipeThrough(new DecompressionStream('gzip'));const j=await new Response(s).text();const u=URL.createObjectURL(new Blob([j],{type:'text/javascript'}));try{await import(u)}finally{URL.revokeObjectURL(u)}})()";
    let bootstrap_hash = STANDARD.encode(Sha256::digest(bootstrap.as_bytes()));
    let csp = format!(
        "default-src 'none'; script-src 'sha256-{bootstrap_hash}' blob:; style-src 'unsafe-inline'; img-src data: blob:; media-src data:; connect-src 'none'; frame-src 'self' blob:; object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'"
    );
    let marker = "</head>";
    let theme = match theme {
        ExportTheme::Editorial => "editorial",
        ExportTheme::Technical => "technical",
    };
    let (mode, resolved) = match mode {
        ExportMode::System => ("system", "light"),
        ExportMode::Light => ("light", "light"),
        ExportMode::Dark => ("dark", "dark"),
    };
    html = html.replacen(
        "<html",
        &format!(
            "<html data-cf-theme=\"{theme}\" data-cf-mode=\"{mode}\" data-cf-mode-resolved=\"{resolved}\""
        ),
        1,
    );
    let styles = include_str!("../web/src/styles.css");
    let system_fallback = include_str!("../web/src/export-fallback.css");
    let head = format!(
        "<meta http-equiv=\"Content-Security-Policy\" content=\"{csp}\"><meta name=\"referrer\" content=\"no-referrer\"><style data-cf-present-export-style=\"true\">{styles}\n{system_fallback}\n{utility_style}</style>"
    );
    html = html.replacen(marker, &format!("{head}{marker}"), 1);
    let payload = format!(
        "<template id=\"cf-present-export-payload\">{encoded}</template><script>{bootstrap}</script>"
    );
    Ok(html.replacen("</body>", &format!("{payload}</body>"), 1))
}

fn write_new_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| PresentError::UnsafePath(path.to_path_buf()))?;
    let metadata = parent
        .canonicalize()
        .map_err(|error| PresentError::io(parent, error))?;
    if !metadata.is_dir() {
        return Err(PresentError::UnsafePath(parent.to_path_buf()));
    }
    let mut file = open_private_create_new(path)?;
    if let Err(error) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        discard_new_file(&file, path);
        return Err(PresentError::io(path, error));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::write_new_private;

    #[test]
    fn export_bootstrap_never_contains_network_endpoint_or_auth_name() {
        let source = "(async()=>{const e=document.getElementById('cf-present-export-payload').textContent.trim();const b=Uint8Array.from(atob(e),c=>c.charCodeAt(0));if(!globalThis.DecompressionStream)return;const s=new Blob([b]).stream().pipeThrough(new DecompressionStream('gzip'));const j=await new Response(s).text();const u=URL.createObjectURL(new Blob([j],{type:'text/javascript'}));try{await import(u)}finally{URL.revokeObjectURL(u)}})()";
        assert!(!source.contains("http:"));
        assert!(!source.contains("cookie"));
        assert!(!source.contains("feedback"));
    }

    #[test]
    fn export_writer_is_private_and_refuses_an_existing_destination() {
        let temporary = tempdir().expect("temporary directory");
        let output = temporary.path().join("review.html");

        write_new_private(&output, b"first").expect("create private export");
        let error = write_new_private(&output, b"second").expect_err("refuse overwrite");

        assert_eq!(fs::read(&output).expect("read export"), b"first");
        assert!(error.to_string().contains("review.html"));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;

            let mode = fs::metadata(&output)
                .expect("export metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600);
        }
    }
}
