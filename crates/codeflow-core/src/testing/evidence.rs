//! Equality evidence for one executed check. Display data never holds secrets.
use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::config::{EnvironmentClass, TargetConfig};
use super::delivery;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Identity {
    pub inputs: BTreeMap<String, String>,
    pub tools: BTreeMap<String, String>,
    pub platform: String,
    pub architecture: String,
    pub environment: BTreeMap<String, String>,
    pub membership: String,
    pub producers: BTreeMap<String, BTreeMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Observation {
    pub identity: Option<Identity>,
    pub unavailable: Option<String>,
}

impl From<Result<Identity, String>> for Observation {
    fn from(value: Result<Identity, String>) -> Self {
        match value {
            Ok(identity) => Self {
                identity: Some(identity),
                unavailable: None,
            },
            Err(reason) => Self {
                identity: None,
                unavailable: Some(reason),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Execution {
    pub name: String,
    pub mode: String,
    pub command: String,
    pub git_tree: Option<String>,
    pub before: Observation,
    pub after: Observation,
}

pub(super) fn git_tree(root: &Path) -> Option<String> {
    let repo = git2::Repository::discover(root).ok()?;
    let tree = repo
        .head()
        .ok()?
        .peel_to_commit()
        .ok()?
        .tree_id()
        .to_string();
    Some(tree)
}

/// Snapshot declared dependencies at the target boundary, after its producers.
pub(super) fn observe(
    root: &Path,
    target: &TargetConfig,
    targets: &[TargetConfig],
    home: Option<&Path>,
) -> Observation {
    identity(root, target, targets, home).into()
}

fn identity(
    root: &Path,
    target: &TargetConfig,
    targets: &[TargetConfig],
    home: Option<&Path>,
) -> Result<Identity, String> {
    let config = target
        .evidence
        .as_ref()
        .ok_or("missing dependency evidence declaration")?;
    if !config.environment_complete {
        return Err("undeclared environment dependencies: audit is incomplete".into());
    }
    for name in target.env.keys() {
        if !config.environment.contains_key(name) {
            return Err(format!("undeclared environment dependency {name}"));
        }
    }
    let mut environment = BTreeMap::new();
    for (name, class) in &config.environment {
        if generated_environment(name) {
            return Err(format!(
                "gate-generated environment dependency {name} requires execution"
            ));
        }
        let value = target
            .env
            .get(name)
            .map(std::ffi::OsString::from)
            .or_else(|| std::env::var_os(name));
        // Distinguish an absent variable from a present empty value.
        let bytes = value.map_or_else(
            || vec![0],
            |value| [b"\x01".as_slice(), value.as_os_str().as_encoded_bytes()].concat(),
        );
        let fingerprint = match class {
            EnvironmentClass::AuditedNonSecret => delivery::digest(&bytes),
            EnvironmentClass::Fingerprint | EnvironmentClass::Sensitive => {
                let key = fingerprint_key(root, home).ok_or_else(|| {
                    format!("no safe fingerprint available for environment dependency {name}: CodeFlow home must be outside every Git repository and the key must be owner-only")
                })?;
                keyed_digest(&key, &[name.as_bytes(), b"\0", &bytes].concat())
            }
            EnvironmentClass::Unclassified => {
                return Err(format!("unclassified environment dependency {name}"))
            }
        };
        environment.insert(name.clone(), fingerprint);
    }
    let inputs = declared_inputs(root, &target.narrow)?;
    let mut tools = BTreeMap::new();
    for (name, command) in &config.tools {
        tools.insert(name.clone(), probe(root, target, command)?);
    }
    let membership = if config.membership.is_empty() {
        delivery::digest(b"declared: no test membership")
    } else {
        probe(root, target, &config.membership)?
    };
    let mut producers = BTreeMap::new();
    for name in &target.requires {
        let producer = targets
            .iter()
            .find(|t| &t.name == name)
            .ok_or_else(|| format!("unknown declared target {name}"))?;
        let mut outputs = BTreeMap::new();
        if producer.outputs.is_empty() {
            return Err(format!("missing producer output declaration for {name}"));
        }
        for output in &producer.outputs {
            let path = output.strip_prefix("target/").map_or_else(
                || root.join(output),
                |relative| delivery::target_dir(root).join(relative),
            );
            let bytes = regular_bytes(&path)
                .map_err(|_| format!("missing or unreadable producer output for {name}"))?;
            outputs.insert(output.clone(), delivery::digest(&bytes));
        }
        producers.insert(name.clone(), outputs);
    }
    Ok(Identity {
        inputs,
        tools,
        platform: std::env::consts::OS.into(),
        architecture: std::env::consts::ARCH.into(),
        environment,
        membership,
        producers,
    })
}

fn declared_inputs(root: &Path, patterns: &[String]) -> Result<BTreeMap<String, String>, String> {
    if patterns.is_empty() {
        return Err("missing declared inputs".into());
    }
    let repo = git2::Repository::discover(root).map_err(|_| "input repository unavailable")?;
    let index = repo.index().map_err(|_| "input index unavailable")?;
    let mut inputs = BTreeMap::<String, String>::new();
    let files = crate::contained::Tree::open(root).map_err(|_| "input root unavailable")?;
    for entry in index.iter() {
        let path = std::str::from_utf8(&entry.path).map_err(|_| "unmatched non-UTF8 input")?;
        if !delivery::matches(patterns, path) {
            continue;
        }
        if entry.mode == 0o160_000 {
            return Err("submodule input requires execution".into());
        }
        let bytes = files
            .read(path, u64::MAX)
            .map_err(|_| "deleted, linked or unreadable input")?;
        #[cfg(unix)]
        let mode = {
            use std::os::unix::fs::PermissionsExt;
            std::fs::metadata(root.join(path))
                .map_err(|_| "cannot inspect input mode")?
                .permissions()
                .mode()
        };
        #[cfg(not(unix))]
        let mode = entry.mode;
        inputs.insert(
            path.into(),
            delivery::digest(&[mode.to_le_bytes().as_slice(), &bytes].concat()),
        );
    }
    for pattern in patterns {
        if !inputs
            .keys()
            .any(|path| delivery::matches(std::slice::from_ref(pattern), path))
        {
            return Err("declared input pattern has no tracked match".into());
        }
    }
    // Restrict the ignored-file scan to declared globs; walking all build
    // output on every target would dominate a small gate.
    let pathspecs: Vec<_> = patterns
        .iter()
        .map(|pattern| format!(":(glob){pattern}"))
        .collect();
    let mut args = vec![
        "ls-files",
        "--others",
        "--ignored",
        "--exclude-standard",
        "-z",
        "--",
    ];
    args.extend(pathspecs.iter().map(String::as_str));
    if !delivery::git_output(root, &args)?.is_empty() {
        return Err("untracked declared input".into());
    }
    Ok(inputs)
}

fn regular_bytes(path: &Path) -> std::io::Result<Vec<u8>> {
    // Links may read outside the audited input set; no equality credit for them.
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(std::io::Error::other("not a regular file"));
    }
    let bytes = std::fs::read(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        Ok([
            metadata.permissions().mode().to_le_bytes().as_slice(),
            &bytes,
        ]
        .concat())
    }
    #[cfg(not(unix))]
    Ok(bytes)
}

fn probe(root: &Path, target: &TargetConfig, argv: &[String]) -> Result<String, String> {
    let executable = argv.first().ok_or("missing tool or membership probe")?;
    let cwd = root.join(target.cwd.as_deref().unwrap_or("."));
    let path = which::which_in(
        executable,
        target
            .env
            .get("PATH")
            .map(std::ffi::OsString::from)
            .or_else(|| std::env::var_os("PATH")),
        &cwd,
    )
    .map_err(|_| "tool or membership probe unavailable")?;
    let mut command = crate::git::process(&path);
    command.args(&argv[1..]).current_dir(&cwd);
    super::runner::configure_environment(&mut command, &target.env, &target.name)
        .map_err(|_| "invalid tool or membership environment")?;
    let output = command
        .output()
        .map_err(|_| "tool or membership probe could not run")?;
    if !output.status.success() {
        return Err("tool or membership probe failed".into());
    }
    if output
        .stdout
        .iter()
        .chain(&output.stderr)
        .all(u8::is_ascii_whitespace)
    {
        return Err("tool or membership probe returned no evidence".into());
    }
    // Probe output is opaque equality data: it can contain environment values.
    Ok(delivery::digest(
        &[
            path.as_os_str().as_encoded_bytes(),
            b"\0",
            &output.stdout,
            b"\0",
            &output.stderr,
        ]
        .concat(),
    ))
}

/// A per-user key outside any repository. Unsupported protection fails closed.
#[cfg(unix)]
fn fingerprint_key(root: &Path, home: Option<&Path>) -> Option<Vec<u8>> {
    use std::io::{Read, Write};
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
    let home = home?.canonicalize().ok()?;
    if home.starts_with(root.canonicalize().ok()?) {
        return None;
    }
    match git2::Repository::discover(&home) {
        Err(error) if error.code() == git2::ErrorCode::NotFound => {}
        _ => return None,
    }
    let path = home.join("gate-environment.key");
    let mut options = std::fs::OpenOptions::new();
    options.read(true).custom_flags(libc::O_NOFOLLOW);
    let mut file = match options.open(&path) {
        Ok(file) => file,
        Err(_) if crate::absence::proven_absent(&path).ok()? => {
            let mut options = std::fs::OpenOptions::new();
            options
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW);
            match options.open(&path) {
                Ok(mut file) => {
                    let key = [
                        uuid::Uuid::new_v4().as_bytes().as_slice(),
                        uuid::Uuid::new_v4().as_bytes().as_slice(),
                    ]
                    .concat();
                    file.write_all(&key).ok()?;
                    file.sync_all().ok()?;
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(_) => return None,
            }
            std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW)
                .open(&path)
                .ok()?
        }
        Err(_) => return None,
    };
    let metadata = file.metadata().ok()?;
    // SAFETY: geteuid has no preconditions and does not access memory.
    if !metadata.is_file()
        || metadata.permissions().mode() & 0o077 != 0
        || metadata.uid() != unsafe { libc::geteuid() }
    {
        return None;
    }
    let mut key = Vec::new();
    file.read_to_end(&mut key).ok()?;
    (key.len() == 32).then_some(key)
}

#[cfg(not(unix))]
fn fingerprint_key(_root: &Path, _home: Option<&Path>) -> Option<Vec<u8>> {
    None
}

/// HMAC-SHA256 with a fixed 32-byte key (RFC 2104).
fn keyed_digest(key: &[u8], bytes: &[u8]) -> String {
    let mut inner = [0x36; 64];
    let mut outer = [0x5c; 64];
    for (index, byte) in key.iter().enumerate() {
        inner[index] ^= byte;
        outer[index] ^= byte;
    }
    let inner = Sha256::digest([inner.as_slice(), bytes].concat());
    delivery::digest(&[outer.as_slice(), inner.as_slice()].concat())
}

/// Only declared secret classes suppress live output and redact stored artifacts.
pub(super) fn redacted_values(target: &TargetConfig) -> Vec<String> {
    target.evidence.as_ref().map_or_else(Vec::new, |config| {
        config
            .environment
            .iter()
            .filter(|(_, class)| {
                matches!(
                    class,
                    EnvironmentClass::Fingerprint | EnvironmentClass::Sensitive
                )
            })
            .filter_map(|(name, _)| {
                target
                    .env
                    .get(name)
                    .cloned()
                    .or_else(|| std::env::var(name).ok())
            })
            .filter(|value| !value.is_empty())
            .collect()
    })
}

/// Encode a URL component, optionally preserving path separators.
fn percent_encode(value: &str, slash_safe: bool, lower: bool) -> String {
    value
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric()
                || b"-_.~".contains(&byte)
                || (slash_safe && byte == b'/')
            {
                char::from(byte).to_string()
            } else if lower {
                format!("%{byte:02x}")
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

fn json_unicode(value: impl IntoIterator<Item = u16>) -> String {
    use std::fmt::Write;
    value.into_iter().fold(String::new(), |mut text, unit| {
        let _ = write!(text, "\\u{unit:04x}");
        text
    })
}

/// Redact the documented representations before writing gate-owned artifacts.
pub(super) fn redact(target: &TargetConfig, text: &str) -> String {
    use base64::Engine;
    let mut values = Vec::new();
    for value in redacted_values(target) {
        let xml_text = value
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        values.push(xml_text.clone());
        let attribute = xml_text.replace('"', "&quot;");
        values.push(attribute.clone());
        for apostrophe in ["&apos;", "&#x27;", "&#39;"] {
            values.push(attribute.replace('\'', apostrophe));
        }
        // Numeric references can encode text, attributes, or every character.
        for escaped in ["&<>", "&<>\"", "&<>\"'", "all"] {
            for non_ascii in [false, true] {
                for hex_case in [None, Some(false), Some(true)] {
                    values.push(
                        value
                            .chars()
                            .map(|c| {
                                if escaped == "all"
                                    || escaped.contains(c)
                                    || (non_ascii && !c.is_ascii())
                                {
                                    match hex_case {
                                        None => format!("&#{};", u32::from(c)),
                                        Some(false) => format!("&#x{:x};", u32::from(c)),
                                        Some(true) => format!("&#x{:X};", u32::from(c)),
                                    }
                                } else {
                                    c.to_string()
                                }
                            })
                            .collect(),
                    );
                }
            }
        }
        let json = serde_json::to_string(&value).expect("strings serialize");
        let json = &json[1..json.len() - 1];
        for ascii in [false, true] {
            for html_escaped in ["", "&<>", "&<>'"] {
                let escaped: String = json
                    .chars()
                    .map(|c| {
                        if (ascii && !c.is_ascii()) || html_escaped.contains(c) {
                            json_unicode(c.encode_utf16(&mut [0; 2]).iter().copied())
                        } else {
                            c.to_string()
                        }
                    })
                    .collect();
                values.push(escaped.replace("\\\"", "\\u0022"));
                values.push(escaped);
            }
        }
        values.push(json_unicode(value.encode_utf16()));
        for slash_safe in [false, true] {
            for lower in [false, true] {
                let url = percent_encode(&value, slash_safe, lower);
                for form in [url.clone(), url.replace("%20", "+")] {
                    values.push(percent_encode(&form, slash_safe, lower));
                    values.push(form);
                }
            }
        }
        values.push(base64::engine::general_purpose::STANDARD.encode(value.as_bytes()));
        values.push(base64::engine::general_purpose::URL_SAFE.encode(value.as_bytes()));
        values.push(value);
    }
    values.sort_by_key(|value| std::cmp::Reverse(value.len()));
    values.dedup();
    let mut text = text.to_owned();
    for value in values {
        text = text.replace(&value, "[redacted]");
    }
    text
}

pub(super) fn mismatch(before: &Identity, now: &Identity) -> Option<&'static str> {
    if before.tools != now.tools {
        Some("tool identity changed")
    } else if before.platform != now.platform {
        Some("platform changed")
    } else if before.architecture != now.architecture {
        Some("architecture changed")
    } else if before.environment != now.environment {
        Some("declared environment changed")
    } else if before.membership != now.membership {
        Some("test membership changed")
    } else if before.producers != now.producers {
        Some("producer output changed")
    } else if before.inputs != now.inputs {
        Some("declared inputs changed")
    } else {
        None
    }
}

/// Values inserted by the runner have no stable selection identity.
pub(super) fn generated_environment(name: &str) -> bool {
    matches!(
        name,
        "TARGET"
            | "CODEFLOW_BIN"
            | "CF_PRESENT_CODEFLOW"
            | "CODEFLOW_GATE_RESULTS"
            | "CODEFLOW_GATE_STARTED_AT"
            | "CODEFLOW_GATE_RUN_ID"
            | "CARGO_LLVM_COV_TARGET_DIR"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_fingerprint_matches_rfc_4231_and_binds_the_key() {
        let key = [0x0b; 20];
        assert_eq!(
            keyed_digest(&key, b"Hi There"),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
        assert_ne!(
            keyed_digest(&key, b"Hi There"),
            keyed_digest(&[0x0c; 20], b"Hi There")
        );
    }
}
