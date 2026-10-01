//! Untracked local registry state, shared by every worktree of a clone:
//! the last verified authority tip (R-10), reservations whose record is not
//! written yet (R-18), and whether the host's data profile was applied.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::git::Git;
use super::IdsError;
use crate::file_lock::{atomic_write, lock_path_exclusive, PathLock};

/// A reservation whose record has not been written yet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Unwritten {
    pub id: String,
    pub uid: String,
    pub issuer: String,
    /// The creation request, so `--resume` writes the same record.
    pub request: serde_json::Value,
}

/// The state file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct State {
    /// Last verified registry tip per remote.
    #[serde(default)]
    pub last_verified: BTreeMap<String, String>,
    /// Reservations awaiting their record.
    #[serde(default)]
    pub unwritten: Vec<Unwritten>,
    /// When `remote protect` applied the data profile, per remote.
    #[serde(default)]
    pub data_profile: BTreeMap<String, String>,
}

fn dir(git: &Git) -> Result<PathBuf, IdsError> {
    Ok(git.common_dir()?.join("codeflow"))
}

/// Take the registry lock shared by every worktree of this clone.
///
/// # Errors
///
/// Returns an error when the lock cannot be taken.
pub fn lock(git: &Git) -> Result<PathLock, IdsError> {
    lock_path_exclusive(&dir(git)?.join("registry.lock")).map_err(IdsError::Git)
}

/// Load the state; a missing file is the empty state.
///
/// # Errors
///
/// Returns an error for an unreadable or malformed file.
pub fn load(git: &Git) -> Result<State, IdsError> {
    let path = dir(git)?.join("registry-state.json");
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|error| IdsError::Invalid(format!("{}: {error}", path.display()))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(State::default()),
        Err(error) => Err(error.into()),
    }
}

/// Save the state atomically.
///
/// # Errors
///
/// Returns an error when the file cannot be written.
pub fn save(git: &Git, state: &State) -> Result<(), IdsError> {
    let dir = dir(git)?;
    std::fs::create_dir_all(&dir)?;
    let text = serde_json::to_string_pretty(state)
        .map_err(|error| IdsError::Invalid(error.to_string()))?;
    atomic_write(&dir.join("registry-state.json"), text.as_bytes())?;
    Ok(())
}

/// Load, change and save the state.
///
/// # Errors
///
/// Returns an error when the state cannot be read or written.
pub fn update(git: &Git, change: impl FnOnce(&mut State)) -> Result<(), IdsError> {
    let mut state = load(git)?;
    change(&mut state);
    save(git, &state)
}
