//! TSK-1489: the transport Git selects for each remote a command contacts.
//!
//! Each row writes a scratch repository configuration and reads it through
//! the same configuration reader the guards use, with the host's Git
//! configuration cut off. Nothing here contacts a remote or starts a helper.

use super::{contacts, select, Contact, Selected};
use crate::security::git_config::{read, Environment};
use std::path::Path;

/// A scratch repository whose local configuration ends with `config`.
struct Scratch {
    _dir: tempfile::TempDir,
    root: std::path::PathBuf,
}

impl Scratch {
    fn new(config: &str) -> Self {
        Self::bytes(config.as_bytes())
    }

    fn bytes(config: &[u8]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = crate::portable_path::canonicalize(dir.path())
            .unwrap()
            .join("repo");
        git2::Repository::init(&root).unwrap();
        let file = root.join(".git/config");
        let mut bytes = std::fs::read(&file).unwrap();
        bytes.extend_from_slice(b"\n");
        bytes.extend_from_slice(config);
        std::fs::write(&file, bytes).unwrap();
        Self { _dir: dir, root }
    }

    fn contacts(&self, command: &str) -> Result<Vec<Contact>, String> {
        let mut env = Environment::new();
        env.insert("GIT_CONFIG_GLOBAL".into(), "/dev/null".into());
        env.insert("GIT_CONFIG_SYSTEM".into(), "/dev/null".into());
        env.insert("GIT_CONFIG_NOSYSTEM".into(), "1".into());
        env.insert("HOME".into(), self.root.display().to_string());
        let config = read(&self.root, &[], &env)?;
        let words: Vec<String> = command.split_whitespace().map(str::to_string).collect();
        let (verb, rest) = words.split_first().unwrap();
        contacts(verb, rest, &config)
    }

    /// The helper programs `command` starts, or the refusal.
    fn helpers(&self, command: &str) -> Result<Vec<String>, String> {
        Ok(self
            .contacts(command)?
            .into_iter()
            .filter_map(|contact| match contact.selected {
                Selected::Helper(name) => Some(name),
                Selected::Ext => Some("ext".into()),
                Selected::Stock => None,
            })
            .collect())
    }

    fn root(&self) -> &Path {
        &self.root
    }
}

const HTTPS: &str = "[remote \"origin\"]\n\turl = https://example.test/project.git\n";

#[test]
fn addresses_select_the_transport_git_selects() {
    for (address, expected) in [
        ("https://example.test/r.git", Selected::Stock),
        ("http://example.test/r.git", Selected::Stock),
        ("ftps://example.test/r.git", Selected::Stock),
        ("ssh://example.test/r.git", Selected::Stock),
        ("git+ssh://example.test/r.git", Selected::Stock),
        ("ssh+git://example.test/r.git", Selected::Stock),
        ("git@example.test:org/r.git", Selected::Stock),
        ("example.test:r.git", Selected::Stock),
        ("git://example.test/r.git", Selected::Stock),
        ("file:///srv/r.git", Selected::Stock),
        ("/srv/r.git", Selected::Stock),
        ("../r.git", Selected::Stock),
        ("C:\\work\\r.git", Selected::Stock),
        ("https::https://example.test/r.git", Selected::Stock),
        (
            "fixture::https://example.test/r.git",
            Selected::Helper("fixture".into()),
        ),
        (
            "fix-ture.2+x::anything",
            Selected::Helper("fix-ture.2+x".into()),
        ),
        ("::anything", Selected::Helper(String::new())),
        (
            "fixture://example.test/r",
            Selected::Helper("fixture".into()),
        ),
        ("HTTPS://example.test/r", Selected::Helper("HTTPS".into())),
        ("ext::sh -c true", Selected::Ext),
    ] {
        assert_eq!(select(address, None), expected, "{address}");
    }
    assert_eq!(
        select("https://example.test/r.git", Some("fixture")),
        Selected::Helper("fixture".into())
    );
    assert_eq!(
        select("", Some("fixture")),
        Selected::Helper("fixture".into())
    );
}

#[test]
fn ordinary_transports_select_no_helper() {
    for address in [
        "https://example.test/project.git",
        "ssh://example.test/project.git",
        "git@example.test:project.git",
        "/srv/project.git",
        "file:///srv/project.git",
    ] {
        let scratch = Scratch::new(&format!("[remote \"origin\"]\n\turl = {address}\n"));
        for command in [
            "fetch",
            "fetch origin",
            "pull origin main",
            "push origin HEAD:refs/heads/task/x",
            "ls-remote origin",
            "fetch --all",
            "remote update",
            "status",
        ] {
            assert_eq!(scratch.helpers(command), Ok(vec![]), "{address} {command}");
        }
    }
}

#[test]
fn a_remote_address_selects_a_helper() {
    let scratch = Scratch::new("[remote \"fixture\"]\n\turl = fixture::https://example.test/r\n");
    assert_eq!(scratch.helpers("fetch fixture"), Ok(vec!["fixture".into()]));
    assert_eq!(
        scratch.helpers("ls-remote fixture"),
        Ok(vec!["fixture".into()])
    );
    let scheme = Scratch::new("[remote \"fixture\"]\n\turl = fixture://example.test/r\n");
    assert_eq!(
        scheme.helpers("push fixture HEAD"),
        Ok(vec!["fixture".into()])
    );
    let plain = Scratch::new("");
    assert_eq!(
        plain.helpers("clone fixture::https://example.test/r copy"),
        Ok(vec!["fixture".into()])
    );
    assert_eq!(
        plain.helpers("fetch fixture://example.test/r main"),
        Ok(vec!["fixture".into()])
    );
    assert_eq!(
        plain.helpers("archive --remote=fixture::x HEAD"),
        Ok(vec!["fixture".into()])
    );
}

#[test]
fn a_remote_configuration_selects_a_helper() {
    let with_url =
        Scratch::new("[remote \"fixture\"]\n\turl = https://example.test/r\n\tvcs = fixture\n");
    assert_eq!(
        with_url.helpers("fetch fixture"),
        Ok(vec!["fixture".into()])
    );
    assert_eq!(
        with_url.helpers("push fixture HEAD"),
        Ok(vec!["fixture".into()])
    );
    let without_url = Scratch::new("[remote \"fixture\"]\n\tvcs = fixture\n");
    assert_eq!(
        without_url.helpers("fetch fixture"),
        Ok(vec!["fixture".into()])
    );
}

#[test]
fn an_implicit_contact_resolves_the_remotes_it_may_reach() {
    let default = Scratch::new("[remote \"origin\"]\n\tvcs = fixture\n");
    assert_eq!(default.helpers("fetch"), Ok(vec!["fixture".into()]));
    assert_eq!(default.helpers("pull"), Ok(vec!["fixture".into()]));
    assert_eq!(default.helpers("push"), Ok(vec!["fixture".into()]));
    let tracked = Scratch::new(&format!(
        "{HTTPS}[remote \"other\"]\n\tvcs = fixture\n[branch \"main\"]\n\tremote = other\n"
    ));
    assert_eq!(tracked.helpers("fetch"), Ok(vec!["fixture".into()]));
    let push_default = Scratch::new(&format!(
        "{HTTPS}[remote \"other\"]\n\tvcs = fixture\n[remote]\n\tpushDefault = other\n"
    ));
    assert_eq!(push_default.helpers("push"), Ok(vec!["fixture".into()]));
    assert_eq!(push_default.helpers("fetch"), Ok(vec![]));
    let promisor = Scratch::new(&format!(
        "{HTTPS}[remote \"lazy\"]\n\turl = fixture::x\n\tpromisor = true\n"
    ));
    assert_eq!(promisor.helpers("status"), Ok(vec!["fixture".into()]));
    assert_eq!(promisor.helpers("log"), Ok(vec!["fixture".into()]));
    let partial = Scratch::new(&format!(
        "{HTTPS}[remote \"lazy\"]\n\turl = fixture::x\n[extensions]\n\tpartialClone = lazy\n"
    ));
    assert_eq!(partial.helpers("show HEAD"), Ok(vec!["fixture".into()]));
}

#[test]
fn a_named_ordinary_remote_ignores_another_helper_remote() {
    let scratch = Scratch::new(&format!("{HTTPS}[remote \"backup\"]\n\turl = fixture::x\n"));
    assert_eq!(scratch.helpers("fetch origin"), Ok(vec![]));
    assert_eq!(scratch.helpers("push origin HEAD"), Ok(vec![]));
    assert_eq!(scratch.helpers("fetch backup"), Ok(vec!["fixture".into()]));
    assert_eq!(scratch.helpers("fetch --all"), Ok(vec!["fixture".into()]));
    assert_eq!(
        scratch.helpers("fetch --multiple origin backup"),
        Ok(vec!["fixture".into()])
    );
    assert_eq!(scratch.helpers("remote update"), Ok(vec!["fixture".into()]));
    assert_eq!(
        scratch.helpers("maintenance run --task=prefetch"),
        Ok(vec!["fixture".into()])
    );
    let group = Scratch::new(&format!(
        "{HTTPS}[remote \"backup\"]\n\turl = fixture::x\n[remotes]\n\tmirrors = origin backup\n"
    ));
    assert_eq!(group.helpers("fetch mirrors"), Ok(vec!["fixture".into()]));
}

#[test]
fn a_fetch_rewrite_selects_a_helper() {
    let scratch = Scratch::new(&format!(
        "{HTTPS}[url \"fixture::https://example.test/\"]\n\tinsteadOf = https://example.test/\n"
    ));
    assert_eq!(scratch.helpers("fetch origin"), Ok(vec!["fixture".into()]));
    assert_eq!(
        scratch.helpers("push origin HEAD"),
        Ok(vec!["fixture".into()])
    );
    let short = Scratch::new("[url \"fixture::https://example.test/\"]\n\tinsteadOf = fx:\n");
    assert_eq!(short.helpers("clone fx:r copy"), Ok(vec!["fixture".into()]));
}

#[test]
fn a_push_rewrite_selects_a_helper_for_push_only() {
    let scratch = Scratch::new(&format!(
        "{HTTPS}[url \"fixture::https://example.test/\"]\n\tpushInsteadOf = https://example.test/\n"
    ));
    assert_eq!(
        scratch.helpers("push origin HEAD"),
        Ok(vec!["fixture".into()])
    );
    assert_eq!(scratch.helpers("fetch origin"), Ok(vec![]));
}

#[test]
fn a_push_rewrite_takes_precedence_for_push() {
    // Both rewrites match: a push reads the push rewrite, a fetch the other.
    let scratch = Scratch::new(&format!(
        "{HTTPS}[url \"fixture::https://example.test/\"]\n\tpushInsteadOf = https://example.test/\n[url \"https://mirror.test/\"]\n\tinsteadOf = https://example.test/\n"
    ));
    assert_eq!(
        scratch.helpers("push origin HEAD"),
        Ok(vec!["fixture".into()])
    );
    assert_eq!(scratch.helpers("fetch origin"), Ok(vec![]));
    let reversed = Scratch::new(&format!(
        "{HTTPS}[url \"https://mirror.test/\"]\n\tpushInsteadOf = https://example.test/\n[url \"fixture::https://example.test/\"]\n\tinsteadOf = https://example.test/\n"
    ));
    assert_eq!(reversed.helpers("push origin HEAD"), Ok(vec![]));
    assert_eq!(reversed.helpers("fetch origin"), Ok(vec!["fixture".into()]));
    // A push URL takes the fetch rewrite, never the push one.
    let pushurl = Scratch::new(
        "[remote \"origin\"]\n\turl = https://example.test/r\n\tpushurl = https://push.test/r\n[url \"fixture::https://push.test/\"]\n\tinsteadOf = https://push.test/\n",
    );
    assert_eq!(
        pushurl.helpers("push origin HEAD"),
        Ok(vec!["fixture".into()])
    );
    assert_eq!(pushurl.helpers("fetch origin"), Ok(vec![]));
}

#[test]
fn a_later_push_destination_selects_a_helper() {
    let pushurls = Scratch::new(
        "[remote \"origin\"]\n\turl = https://example.test/r\n\tpushurl = https://example.test/r\n\tpushurl = fixture::x\n",
    );
    assert_eq!(
        pushurls.helpers("push origin HEAD"),
        Ok(vec!["fixture".into()])
    );
    assert_eq!(pushurls.helpers("fetch origin"), Ok(vec![]));
    let urls =
        Scratch::new("[remote \"origin\"]\n\turl = https://example.test/r\n\turl = fixture::x\n");
    assert_eq!(urls.helpers("push origin HEAD"), Ok(vec!["fixture".into()]));
    assert_eq!(urls.helpers("fetch origin"), Ok(vec![]));
}

#[test]
fn an_unresolved_selection_refuses_naming_the_remote() {
    let unknown = Scratch::new(&format!("{HTTPS}[remotes]\n\tmirrors = origin missing\n"));
    let error = unknown.helpers("fetch mirrors").unwrap_err();
    assert!(error.contains("remote `missing`"), "{error}");
    let bytes = Scratch::bytes(b"[remote \"fixture\"]\n\turl = https://example.test/\xff\n");
    let error = bytes.helpers("fetch fixture").unwrap_err();
    assert!(error.contains("remote `fixture`"), "{error}");
    let legacy = Scratch::new(HTTPS);
    std::fs::create_dir_all(legacy.root().join(".git/remotes")).unwrap();
    std::fs::write(legacy.root().join(".git/remotes/old"), "URL: fixture::x\n").unwrap();
    let error = legacy.helpers("fetch old").unwrap_err();
    assert!(error.contains("remote `old`"), "{error}");
    assert_eq!(legacy.helpers("fetch origin"), Ok(vec![]));
}

#[test]
fn submodule_transports_are_resolved() {
    let configured = Scratch::new(&format!("{HTTPS}[submodule \"lib\"]\n\turl = fixture::x\n"));
    assert_eq!(
        configured.helpers("submodule update"),
        Ok(vec!["fixture".into()])
    );
    assert_eq!(
        configured.helpers("fetch origin"),
        Ok(vec!["fixture".into()])
    );
    assert_eq!(configured.helpers("log"), Ok(vec![]));
    for setting in [
        "[protocol]\n\tallow = always\n",
        "[protocol \"fixture\"]\n\tallow = always\n",
    ] {
        let allowed = Scratch::new(&format!("{HTTPS}{setting}"));
        assert!(allowed
            .helpers("clone --recurse-submodules https://example.test/r copy")
            .is_err());
        assert_eq!(allowed.helpers("log"), Ok(vec![]));
    }
    let safe = Scratch::new(&format!("{HTTPS}[protocol \"https\"]\n\tallow = always\n"));
    assert_eq!(
        safe.helpers("clone --recurse-submodules https://example.test/r copy"),
        Ok(vec![])
    );
}

#[test]
fn a_cleared_address_list_keeps_both_readings() {
    // Git releases differ on whether an empty `url` clears the list; the
    // fetch reads the first address under both readings.
    let scratch = Scratch::new(
        "[remote \"origin\"]\n\turl = fixture::x\n\turl =\n\turl = https://example.test/r\n",
    );
    assert_eq!(scratch.helpers("fetch origin"), Ok(vec!["fixture".into()]));
}
