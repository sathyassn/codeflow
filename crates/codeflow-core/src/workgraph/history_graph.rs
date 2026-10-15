//! Check cached graph parents against commit objects before libgit2 walks.
use std::collections::HashSet;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use git2::{Oid, Repository};

struct Entry {
    oid: Oid,
    tree: Oid,
    parents: Vec<usize>,
    generation: u32,
    time: u64,
}

fn invalid() -> String {
    "commit-graph metadata is malformed; remove the cache and retry".into()
}

fn word(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let bytes = bytes
        .get(offset..offset.checked_add(4).ok_or_else(invalid)?)
        .ok_or_else(invalid)?;
    Ok(u32::from_be_bytes(bytes.try_into().map_err(|_| invalid())?))
}

#[derive(Debug, PartialEq, Eq)]
struct Identity {
    size: u64,
    modified: std::time::SystemTime,
    #[cfg(unix)]
    inode: u64,
    #[cfg(unix)]
    device: u64,
    contents: Vec<u8>,
}

struct File {
    path: PathBuf,
    graph: bool,
    identity: Option<Identity>,
}

/// Verified cache files, including absent paths and manifests which select them.
#[derive(Default)]
pub(super) struct Verified {
    files: Vec<File>,
    directories: Vec<(PathBuf, PathBuf)>,
}

fn changed(path: &Path) -> String {
    format!(
        "commit-graph changed during judgment at {}; remove the cache and retry",
        path.display()
    )
}

fn identity(path: &Path, graph: bool) -> Result<Option<Identity>, String> {
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("{} ({error})", changed(path))),
    };
    let mut read = || -> std::io::Result<Identity> {
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err(std::io::Error::other("not a regular file"));
        }
        // Graphs carry a SHA-1 trailer. Manifests are small and retained whole.
        if graph {
            file.seek(SeekFrom::Start(metadata.len().saturating_sub(20)))?;
        }
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)?;
        Ok(Identity {
            size: metadata.len(),
            modified: metadata.modified()?,
            #[cfg(unix)]
            inode: std::os::unix::fs::MetadataExt::ino(&metadata),
            #[cfg(unix)]
            device: std::os::unix::fs::MetadataExt::dev(&metadata),
            contents,
        })
    };
    read()
        .map(Some)
        .map_err(|error| format!("{} ({error})", changed(path)))
}

impl Verified {
    fn read(&mut self, path: &Path, graph: bool) -> Result<Option<Vec<u8>>, String> {
        let before = identity(path, graph)?;
        let bytes = if before.is_some() {
            Some(std::fs::read(path).map_err(|error| format!("{} ({error})", changed(path)))?)
        } else {
            None
        };
        let after = identity(path, graph)?;
        if before != after {
            return Err(changed(path));
        }
        self.files.push(File {
            path: path.to_path_buf(),
            graph,
            identity: after,
        });
        Ok(bytes)
    }

    pub(super) fn check(&self) -> Result<(), String> {
        // Retain alias paths too: an alternate symlink may change its target.
        for (path, canonical) in &self.directories {
            if std::fs::canonicalize(path)
                .map_err(|error| format!("{} ({error})", changed(path)))?
                != *canonical
            {
                return Err(changed(path));
            }
        }
        for file in &self.files {
            if identity(&file.path, file.graph)? != file.identity {
                return Err(changed(&file.path));
            }
        }
        Ok(())
    }
}

fn chunk<'a>(bytes: &'a [u8], name: &[u8]) -> Result<Option<&'a [u8]>, String> {
    let count = usize::from(*bytes.get(6).ok_or_else(invalid)?);
    let table = bytes.get(8..8 + (count + 1) * 12).ok_or_else(invalid)?;
    let mut names = HashSet::new();
    let mut previous = 8 + (count + 1) * 12;
    for (index, entry) in table.chunks_exact(12).enumerate() {
        let offset = usize::try_from(u64::from_be_bytes(
            entry[4..12].try_into().map_err(|_| invalid())?,
        ))
        .map_err(|_| invalid())?;
        if offset < previous
            || offset > bytes.len().saturating_sub(20)
            || !names.insert(&entry[..4])
            || (index == count) != (entry[..4] == [0; 4])
        {
            return Err(invalid());
        }
        previous = offset;
    }
    for pair in table.windows(24).step_by(12) {
        if &pair[..4] == name {
            let offset = |part: &[u8]| -> Result<usize, String> {
                usize::try_from(u64::from_be_bytes(part.try_into().map_err(|_| invalid())?))
                    .map_err(|_| invalid())
            };
            let start = offset(&pair[4..12])?;
            let end = offset(&pair[16..24])?;
            return bytes.get(start..end).map(Some).ok_or_else(invalid);
        }
    }
    Ok(None)
}

fn append_graph(bytes: &[u8], entries: &mut Vec<Entry>, base_hashes: &[u8]) -> Result<(), String> {
    // libgit2's supported graph format: version 1, SHA-1 object IDs.
    if bytes.get(..6) != Some(b"CGPH\x01\x01")
        || bytes.get(7).copied().map(usize::from) != Some(base_hashes.len() / 20)
    {
        return Err(invalid());
    }
    if chunk(bytes, b"BASE")?.unwrap_or_default() != base_hashes {
        return Err(invalid());
    }
    let ids = chunk(bytes, b"OIDL")?.ok_or_else(invalid)?;
    let data = chunk(bytes, b"CDAT")?.ok_or_else(invalid)?;
    let edges = chunk(bytes, b"EDGE")?.unwrap_or_default();
    if ids.len() % 20 != 0 || data.len() != ids.len() / 20 * 36 {
        return Err(invalid());
    }
    for (oid, row) in ids.chunks_exact(20).zip(data.chunks_exact(36)) {
        let first = word(row, 20)?;
        let second = word(row, 24)?;
        let mut parents = Vec::new();
        if first != 0x7000_0000 {
            parents.push(first as usize);
        }
        if second & 0x8000_0000 != 0 {
            let mut offset = ((second & 0x7fff_ffff) as usize)
                .checked_mul(4)
                .ok_or_else(invalid)?;
            loop {
                let edge = word(edges, offset)?;
                parents.push((edge & 0x7fff_ffff) as usize);
                if edge & 0x8000_0000 != 0 {
                    break;
                }
                offset += 4;
            }
        } else if second != 0x7000_0000 {
            parents.push(second as usize);
        }
        let generation = word(row, 28)?;
        entries.push(Entry {
            oid: Oid::from_bytes(oid).map_err(|_| invalid())?,
            tree: Oid::from_bytes(&row[..20]).map_err(|_| invalid())?,
            parents,
            generation: generation >> 2,
            time: (u64::from(generation & 3) << 32) | u64::from(word(row, 32)?),
        });
    }
    Ok(())
}

fn verify_entries(repo: &Repository, entries: &[Entry]) -> Result<(), String> {
    let legacy = entries.iter().all(|entry| entry.generation == 0);
    for entry in entries {
        let commit = repo.find_commit(entry.oid).map_err(|error| {
            format!(
                "commit-graph disagrees with commit {}: {error}; remove the cache and retry",
                entry.oid
            )
        })?;
        let parents: Vec<&Entry> = entry
            .parents
            .iter()
            .map(|index| entries.get(*index).ok_or_else(invalid))
            .collect::<Result<_, _>>()?;
        let generation = parents
            .iter()
            .map(|parent| parent.generation)
            .max()
            .unwrap_or(0)
            .saturating_add(1)
            .min(0x3fff_ffff);
        // Commit::parent_ids reads the object, not the revwalk's graph cache.
        if !commit
            .parent_ids()
            .eq(parents.iter().map(|parent| parent.oid))
            || entry.tree != commit.tree_id()
            || u64::try_from(commit.time().seconds()).ok() != Some(entry.time)
            || (!legacy && entry.generation != generation)
        {
            return Err(format!(
                "commit-graph disagrees with commit {}; remove the cache and retry",
                entry.oid
            ));
        }
    }
    Ok(())
}

/// Validate every cached entry, a superset of the commits a judgment may walk.
/// Alternate object stores and split chains are checked with the same rule.
pub(super) fn verify(repo: &Repository) -> Result<Verified, String> {
    let mut verified = Verified::default();
    let mut pending = vec![repo.commondir().join("objects")];
    let mut seen = HashSet::new();
    while let Some(objects) = pending.pop() {
        let canonical = std::fs::canonicalize(&objects).map_err(|error| {
            format!(
                "cannot read object directory {}: {error}",
                objects.display()
            )
        })?;
        if !seen.insert(canonical.clone()) {
            // Files below a first-seen path retain their own identities.
            // A duplicate alias has no file entries, so retain its target.
            verified.directories.push((objects, canonical));
            continue;
        }
        if let Some(bytes) = verified.read(&objects.join("info/commit-graph"), true)? {
            let mut entries = Vec::new();
            append_graph(&bytes, &mut entries, &[])?;
            verify_entries(repo, &entries)?;
        }
        if let Some(chain) = verified.read(
            &objects.join("info/commit-graphs/commit-graph-chain"),
            false,
        )? {
            let mut entries = Vec::new();
            let mut base_hashes = Vec::new();
            for hash in chain
                .split(|byte| *byte == b'\n')
                .filter(|line| !line.is_empty())
            {
                if hash.len() != 40 || !hash.iter().all(u8::is_ascii_hexdigit) {
                    return Err(invalid());
                }
                let hash = std::str::from_utf8(hash).map_err(|_| invalid())?;
                let path = objects.join(format!("info/commit-graphs/graph-{hash}.graph"));
                let bytes = verified.read(&path, true)?.ok_or_else(invalid)?;
                append_graph(&bytes, &mut entries, &base_hashes)?;
                base_hashes
                    .extend_from_slice(Oid::from_str(hash).map_err(|_| invalid())?.as_bytes());
            }
            verify_entries(repo, &entries)?;
        }
        if let Some(alternates) = verified.read(&objects.join("info/alternates"), false)? {
            for line in alternates
                .split(|byte| *byte == b'\n')
                .filter(|line| !line.is_empty())
            {
                // Git permits quoted alternate paths. Decode with its path
                // quoting rules so spaces and non-UTF8 names remain exact.
                let path = alternate_path(line)?;
                pending.push(objects.join(path));
            }
        }
    }
    verified.check()?;
    Ok(verified)
}

fn alternate_path(line: &[u8]) -> Result<PathBuf, String> {
    let mut bytes = Vec::new();
    if line.first() == Some(&b'"') {
        let inner = line
            .strip_prefix(b"\"")
            .and_then(|line| line.strip_suffix(b"\""))
            .ok_or_else(invalid)?;
        let mut chars = inner.iter().copied();
        while let Some(byte) = chars.next() {
            if byte != b'\\' {
                bytes.push(byte);
                continue;
            }
            let escape = chars.next().ok_or_else(invalid)?;
            bytes.push(match escape {
                b'a' => 7,
                b'b' => 8,
                b't' => 9,
                b'n' => 10,
                b'v' => 11,
                b'f' => 12,
                b'r' => 13,
                b'\\' | b'"' => escape,
                b'0'..=b'3' => {
                    let second = chars
                        .next()
                        .filter(|c| matches!(c, b'0'..=b'7'))
                        .ok_or_else(invalid)?;
                    let third = chars
                        .next()
                        .filter(|c| matches!(c, b'0'..=b'7'))
                        .ok_or_else(invalid)?;
                    (escape - b'0') * 64 + (second - b'0') * 8 + (third - b'0')
                }
                _ => return Err(invalid()),
            });
        }
    } else {
        bytes.extend_from_slice(line);
    }
    crate::git::GitName::from_bytes(&bytes)
        .os_path()
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_change_refuses_even_when_size_inode_and_mtime_match() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("commit-graph");
        std::fs::write(&path, [0; 40]).unwrap();
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        let mut verified = Verified::default();
        verified.read(&path, true).unwrap();
        verified.check().unwrap();
        let mut file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.seek(SeekFrom::End(-1)).unwrap();
        std::io::Write::write_all(&mut file, &[1]).unwrap();
        file.set_times(std::fs::FileTimes::new().set_modified(modified))
            .unwrap();
        assert!(verified
            .check()
            .unwrap_err()
            .contains("remove the cache and retry"));
    }

    #[test]
    fn alternate_paths_keep_quoted_bytes_and_reject_bad_escapes() {
        assert_eq!(
            alternate_path(b"../objects").unwrap(),
            Path::new("../objects")
        );
        assert_eq!(
            alternate_path(br#""../with\tspace/objects""#).unwrap(),
            Path::new("../with\tspace/objects")
        );
        assert!(alternate_path(br#""bad\q""#).is_err());
        assert!(alternate_path(b"\"unterminated").is_err());
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            assert_eq!(
                alternate_path(br#""\377""#).unwrap().as_os_str().as_bytes(),
                &[255]
            );
        }
    }

    #[test]
    fn malformed_graphs_never_panic_or_supply_parents() {
        for bytes in [vec![], b"CGPH\x01\x01\xff\x00".to_vec(), vec![255; 256]] {
            assert!(append_graph(&bytes, &mut Vec::new(), &[]).is_err());
        }
        assert!(word(&[], usize::MAX).is_err());
    }
}
