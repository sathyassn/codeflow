//! Trusted context for comparing capability catalogs at immutable revisions.
use std::path::Path;

use crate::hooks::GitPolicy;
use crate::workgraph::classify::{stack_product_paths, ProjectPaths};

/// The unique baseline and authority-owned scope for an impact evaluation.
pub struct Context {
    pub merge_base: String,
    pub paths: ProjectPaths,
}

fn commit(repo: &git2::Repository, sha: &str) -> Result<git2::Oid, String> {
    if sha.len() != 40 || !sha.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err("capability check requires exact commit SHAs".into());
    }
    let id = git2::Oid::from_str(sha).map_err(|e| e.to_string())?;
    repo.find_commit(id)
        .map_err(|e| format!("cannot read capability commit {sha}: {e}"))?;
    Ok(id)
}

/// Resolve the unique merge base and read scope only from the authority tree.
///
/// # Errors
/// Refuses non-commit identities, incomplete or ambiguous history, unsafe
/// project modes and oversized or malformed authority configuration.
pub fn context(
    root: &Path,
    authority: &str,
    base: &str,
    head: &str,
    policy: &GitPolicy,
) -> Result<Context, String> {
    // This public reader also has callers outside CI; retain its own history
    // guard even when the CLI already guarded its shared range inventory.
    crate::workgraph::history::check_command(root)?;
    let repo = git2::Repository::discover(root).map_err(|e| e.to_string())?;
    let base_id = commit(&repo, base)?;
    let head_id = commit(&repo, head)?;
    let authority_id = commit(&repo, authority)?;
    if repo.is_shallow() {
        return Err("cannot verify capability impact with shallow history; fetch complete target and head history".into());
    }
    let bases = crate::workgraph::history::merge_bases(&repo, base_id, head_id)
        .map_err(|e| format!("cannot establish capability merge base: {e}"))?;
    if bases.len() != 1 {
        return Err("capability history needs exactly one merge base; missing or ambiguous history is not verified".into());
    }
    Ok(Context {
        merge_base: bases[0].to_string(),
        paths: project_paths(&repo, authority_id, policy)?,
    })
}

fn project_paths(
    repo: &git2::Repository,
    authority: git2::Oid,
    policy: &GitPolicy,
) -> Result<ProjectPaths, String> {
    let tree_id = repo
        .find_commit(authority)
        .map(|c| c.tree_id())
        .map_err(|e| e.to_string())?;
    let tree =
        super::bounded_catalog_tree(repo, tree_id, "<authority>").map_err(|e| e.to_string())?;
    let config = tree
        .get_name(".codeflow")
        .map(|entry| {
            if entry.filemode() != 0o040_000 {
                return Err("authority .codeflow must be a directory".to_owned());
            }
            super::bounded_catalog_tree(repo, entry.id(), ".codeflow").map_err(|e| e.to_string())
        })
        .transpose()?;
    let stack = match config
        .as_ref()
        .and_then(|tree| tree.get_name("project.toml"))
    {
        None => String::new(),
        Some(entry) => {
            if !matches!(entry.filemode(), 0o100_644 | 0o100_755) {
                return Err("authority project.toml must be a regular blob".into());
            }
            let (size, kind) = repo
                .odb()
                .and_then(|odb| odb.read_header(entry.id()))
                .map_err(|e| e.to_string())?;
            if kind != git2::ObjectType::Blob || size > super::CAPABILITY_MAX_FILE_BYTES {
                return Err("authority project.toml exceeds the catalog input bound".into());
            }
            let blob = repo.find_blob(entry.id()).map_err(|e| e.to_string())?;
            let text = std::str::from_utf8(blob.content()).map_err(|e| e.to_string())?;
            let value: toml::Value =
                toml::from_str(text).map_err(|e| format!("authority project: {e}"))?;
            match value.get("stack") {
                None => String::new(),
                Some(value) => value
                    .as_str()
                    .ok_or("authority stack must be a string")?
                    .to_owned(),
            }
        }
    };
    let paths = ProjectPaths {
        product: policy.product_paths.clone().unwrap_or_else(|| {
            stack_product_paths(&stack)
                .iter()
                .map(ToString::to_string)
                .collect()
        }),
        watched: policy.breaking_watch_paths.clone(),
        document_indexes: Vec::new(),
    };
    paths.validate()?;
    Ok(paths)
}
