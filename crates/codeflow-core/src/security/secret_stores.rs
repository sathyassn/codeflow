//! Secret-store destruction uses the deletion reader and the permission
//! action table. This adds no blanket harness-folder rule (TSK-395).
use std::path::Path;
use std::sync::OnceLock;

use crate::hooks::Violation;

pub(crate) fn violation(command: &str, cwd: Option<&Path>) -> Option<Violation> {
    let found = super::deletion::secret_deletion_in(command, cwd)?;
    let message = match found.unproven {
        Some(why) => format!(
            "cannot verify secret-store deletion targets: {}",
            why.reason
        ),
        None => format!(
            "destruction of configured secret store `{}` is refused",
            found.target
        ),
    };
    Some(Violation::always_blocking("security.dangerous_commands", message,
        "the operator must inspect the target and perform any intentional secret-store deletion outside the agent session"))
}

fn configured() -> &'static [&'static str] {
    static STORES: OnceLock<Vec<&'static str>> = OnceLock::new();
    STORES.get_or_init(|| {
        let table = super::actions::table();
        let mut stores: Vec<_> = table
            .sandbox_read_denies
            .iter()
            .map(String::as_str)
            .chain(
                table
                    .claude_read_groups
                    .iter()
                    .filter(|group| group.carveouts.is_empty())
                    .flat_map(|group| &group.denies)
                    .filter_map(|rule| rule.strip_prefix("Read(")?.strip_suffix(')')),
            )
            .filter(|rule| rule.starts_with("~/"))
            .collect();
        stores.sort_unstable();
        stores.dedup();
        stores
    })
}

/// Absolute spelling and filesystem aliases of each configured rule. Only
/// path metadata is resolved; no credential contents are opened.
pub(super) struct Stores {
    paths: Vec<(&'static str, String)>,
    unplaced: bool,
}

fn text(path: &Path) -> Option<String> {
    let raw = path.to_str()?;
    let spelling = crate::portable_path::filesystem_spelling(raw);
    let path = super::outward::lexical_join(Path::new(""), Path::new(&spelling));
    let text = path.to_str()?.replace(std::path::MAIN_SEPARATOR, "/");
    Some(if cfg!(windows) {
        text.to_lowercase()
    } else {
        text
    })
}

impl Stores {
    pub(super) fn new(mut real: impl FnMut(&Path) -> Result<Option<String>, ()>) -> Self {
        let homes = crate::portable_path::user_homes();
        let mut stores = Self {
            paths: Vec::new(),
            unplaced: homes.is_empty(),
        };
        for home in homes {
            for rule in configured() {
                let path = home.join(rule.strip_prefix("~/").expect("home-qualified rule"));
                if let Some(path_text) = text(&path) {
                    stores.paths.push((rule, path_text));
                } else {
                    stores.unplaced = true;
                }
                match real(&path) {
                    Ok(Some(alias)) => {
                        if let Some(alias) = text(Path::new(&alias)) {
                            stores.paths.push((rule, alias));
                        } else {
                            stores.unplaced = true;
                        }
                    }
                    Ok(None) | Err(()) => stores.unplaced = true,
                }
            }
        }
        stores
    }

    /// Intersect the target and its subtree with each configured store and
    /// its subtree. This also protects removing a parent of a named file.
    pub(super) fn target(&self, path: &Path) -> Result<Option<&'static str>, ()> {
        let target = text(path).ok_or(())?;
        let descendant = format!("{}/**", target.trim_end_matches('/'));
        let mut unknown = self.unplaced;
        for (rule, store) in &self.paths {
            let root = store.strip_suffix("/**").unwrap_or(store);
            let subtree = format!("{}/**", root.trim_end_matches('/'));
            for (left, right) in [
                (&target, root),
                (&target, subtree.as_str()),
                (&descendant, root),
            ] {
                match super::pattern::glob_intersects(left, right) {
                    Some(true) => return Ok(Some(rule)),
                    Some(false) => {}
                    None => unknown = true,
                }
            }
        }
        if unknown {
            Err(())
        } else {
            Ok(None)
        }
    }
}
