//! TSK-1489: the transport Git selects for each remote a command contacts.
//!
//! Each row writes a scratch repository configuration and reads it through
//! the same configuration reader the guards use, with the host's Git
//! configuration cut off. Nothing here contacts a remote or starts a helper.

use super::{contacts, select, Contact, Probe, Selected, UNPROVEN};
use crate::security::git_config::{current_branch, locate, read, Environment, Location};
use std::path::Path;

/// A scratch repository on branch `main` whose local configuration ends
/// with `config`.
struct Scratch {
    _dir: tempfile::TempDir,
    root: std::path::PathBuf,
}

/// The probe the runner reader uses, from the scratch repository's root.
struct RootProbe<'a>(&'a Path);

impl Probe for RootProbe<'_> {
    fn location(&mut self) -> Result<Location, String> {
        locate(self.0, &[], &Environment::new())
    }

    fn branch(&mut self) -> Result<String, String> {
        current_branch(self.0, &[], &Environment::new())
    }
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
        std::fs::write(root.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        let file = root.join(".git/config");
        let mut bytes = std::fs::read(&file).unwrap();
        bytes.extend_from_slice(b"\n");
        bytes.extend_from_slice(config);
        std::fs::write(&file, bytes).unwrap();
        Self { _dir: dir, root }
    }

    /// The contacts of a command given word by word, so a word may be empty.
    fn contacts_of(&self, words: &[&str]) -> Result<Vec<Contact>, String> {
        let mut env = Environment::new();
        env.insert("GIT_CONFIG_GLOBAL".into(), "/dev/null".into());
        env.insert("GIT_CONFIG_SYSTEM".into(), "/dev/null".into());
        env.insert("GIT_CONFIG_NOSYSTEM".into(), "1".into());
        env.insert("HOME".into(), self.root.display().to_string());
        let config = read(&self.root, &[], &env)?;
        let words: Vec<String> = words.iter().map(|word| (*word).to_string()).collect();
        let (verb, rest) = words.split_first().unwrap();
        contacts(verb, rest, &config, &mut RootProbe(&self.root))
    }

    /// The helper programs `command` starts, or the refusal.
    fn helpers(&self, command: &str) -> Result<Vec<String>, String> {
        let words: Vec<&str> = command.split_whitespace().collect();
        self.helpers_of(&words)
    }

    /// [`Scratch::helpers`] for a command given word by word.
    fn helpers_of(&self, words: &[&str]) -> Result<Vec<String>, String> {
        Ok(self
            .contacts_of(words)?
            .into_iter()
            .filter_map(|contact| match contact.selected {
                Selected::Helper(name) => Some(name),
                Selected::Ext => Some("ext".into()),
                Selected::Stock => None,
            })
            .collect())
    }

    /// Assert that `command` refuses as a form the guard cannot prove, with
    /// `part` in the reason.
    fn unproven(&self, command: &str, part: &str) {
        let error = self
            .helpers(command)
            .expect_err(&format!("`{command}` must refuse"));
        assert!(error.starts_with(UNPROVEN), "{command}: {error}");
        assert!(error.contains(part), "{command}: {error}");
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
        assert_eq!(select(address), expected, "{address}");
    }
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
            "pull",
            "fetch --prune",
            "fetch origin",
            "fetch -q origin main",
            "pull --rebase origin main",
            "push origin HEAD:refs/heads/task/x",
            "push -u origin HEAD",
            "ls-remote origin",
            "remote update origin",
            "maintenance run --task=gc",
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
    assert_eq!(
        plain.helpers("fetch -- fixture::x"),
        Ok(vec!["fixture".into()])
    );
}

#[test]
fn a_remote_with_a_helper_setting_refuses() {
    for setting in ["vcs = fixture", "vcs = https", "vcs ="] {
        let with_url = Scratch::new(&format!(
            "[remote \"fixture\"]\n\turl = https://example.test/r\n\t{setting}\n"
        ));
        with_url.unproven("fetch fixture", "helper through its configuration");
        with_url.unproven("push fixture HEAD", "helper through its configuration");
    }
    let without_url = Scratch::new("[remote \"fixture\"]\n\tvcs = fixture\n");
    without_url.unproven("fetch fixture", "remote `fixture`");
    let clone = Scratch::new("[remote \"other\"]\n\tvcs = fixture\n");
    clone.unproven("clone https://example.test/r copy", "a clone would use");
}

#[test]
fn an_operand_free_fetch_reads_the_current_branch_remote() {
    let default = Scratch::new("[remote \"origin\"]\n\turl = fixture::x\n");
    assert_eq!(default.helpers("fetch"), Ok(vec!["fixture".into()]));
    assert_eq!(default.helpers("pull"), Ok(vec!["fixture".into()]));
    let tracked = Scratch::new(&format!(
        "{HTTPS}[remote \"other\"]\n\turl = fixture::x\n[branch \"main\"]\n\tremote = other\n"
    ));
    assert_eq!(tracked.helpers("fetch"), Ok(vec!["fixture".into()]));
    assert_eq!(tracked.helpers("fetch origin"), Ok(vec![]));
    let off = Scratch::new(&format!("{HTTPS}[fetch]\n\tall = false\n"));
    assert_eq!(off.helpers("fetch"), Ok(vec![]));
}

#[test]
fn an_operand_free_contact_with_no_remote_configured_contacts_nothing() {
    let none = Scratch::new("");
    for command in ["fetch", "pull -r", "push", "ls-remote -h"] {
        assert_eq!(none.helpers(command), Ok(vec![]), "{command}");
    }
    std::fs::create_dir(none.root().join(".git/remotes")).unwrap();
    std::fs::write(none.root().join(".git/remotes/origin"), "URL: fixture::x\n").unwrap();
    none.unproven("fetch", "legacy remote file");
    none.unproven("push", "legacy remote file");

    let named = Scratch::new("[branch \"main\"]\n\tremote = elsewhere\n");
    named.unproven("fetch", "remote `elsewhere` is not configured");
    named.unproven("push", "names no remote");
    let pushed = Scratch::new("[remote]\n\tpushDefault = elsewhere\n");
    pushed.unproven("push", "names no remote");
}

#[test]
fn an_operand_free_contact_refuses_each_state_it_cannot_prove() {
    let ordinary = Scratch::new(HTTPS);
    ordinary.unproven("push", "names no remote");
    ordinary.unproven("ls-remote", "names no remote");
    ordinary.unproven("push --force-with-lease=main", "names no remote");

    let detached = Scratch::new(HTTPS);
    std::fs::write(
        detached.root().join(".git/HEAD"),
        "0123456789012345678901234567890123456789\n",
    )
    .unwrap();
    detached.unproven("fetch", "current branch");

    let missing = Scratch::new(&format!("{HTTPS}[branch \"main\"]\n\tremote = gone\n"));
    missing.unproven("fetch", "remote `gone` is not configured");

    let no_origin = Scratch::new("[remote \"other\"]\n\turl = https://example.test/r\n");
    no_origin.unproven("fetch", "remote `origin` is not configured");
    no_origin.unproven("pull", "remote `origin` is not configured");

    for value in ["true", "yes", "1"] {
        let all = Scratch::new(&format!("{HTTPS}[fetch]\n\tall = {value}\n"));
        all.unproven("fetch", "every configured remote");
    }
    let bare_key = Scratch::new(&format!("{HTTPS}[fetch]\n\tall\n"));
    bare_key.unproven("fetch", "every configured remote");

    let group = Scratch::new(&format!("{HTTPS}[remotes]\n\torigin = origin\n"));
    group.unproven("fetch", "remote group");
    group.unproven("fetch origin", "remote group");
    let folded = Scratch::new(&format!("{HTTPS}[remotes]\n\tOrigin = origin\n"));
    folded.unproven("fetch origin", "remote group");

    let twice = Scratch::new(&format!(
        "{HTTPS}[remote \"origin\"]\n\turl = https://example.test/r\n"
    ));
    twice.unproven("fetch", "more than one address");

    let unreadable = Scratch::bytes(
        b"[remote \"origin\"]\n\turl = https://example.test/r\n[branch \"main\"]\n\tremote = or\xffigin\n",
    );
    let error = unreadable.helpers("fetch").unwrap_err();
    assert!(error.contains("not UTF-8"), "{error}");
    let unclear = Scratch::new(&format!("{HTTPS}[fetch]\n\tall = maybe\n"));
    unclear.unproven("fetch", "cannot be read");
}

#[test]
fn every_remote_forms_and_groups_refuse() {
    let scratch = Scratch::new(&format!("{HTTPS}[remote \"backup\"]\n\turl = fixture::x\n"));
    assert_eq!(scratch.helpers("fetch origin"), Ok(vec![]));
    assert_eq!(scratch.helpers("push origin HEAD"), Ok(vec![]));
    assert_eq!(scratch.helpers("fetch backup"), Ok(vec!["fixture".into()]));
    assert_eq!(
        scratch.helpers("fetch --multiple origin backup"),
        Ok(vec!["fixture".into()])
    );
    scratch.unproven("fetch --all", "every remote");
    scratch.unproven("fetch --al", "every remote");
    scratch.unproven("pull --all", "every remote");
    assert_eq!(scratch.helpers("remote update"), Ok(vec!["fixture".into()]));
    assert_eq!(
        scratch.helpers("remote update --prune"),
        Ok(vec!["fixture".into()])
    );
    scratch.unproven("maintenance run", "prefetch");
    scratch.unproven("maintenance run --task=prefetch", "prefetch");
    scratch.unproven("maintenance run --task gc --task prefetch", "prefetch");
    let group = Scratch::new(&format!(
        "{HTTPS}[remote \"backup\"]\n\turl = https://example.test/b\n[remotes]\n\tmirrors = origin backup\n"
    ));
    group.unproven("fetch mirrors", "remote group");
    group.unproven("remote update mirrors", "remote group");
}

#[test]
fn an_operand_free_remote_update_reads_the_remotes_git_selects() {
    let backup = format!("{HTTPS}[remote \"backup\"]\n\turl = fixture::x\n");
    // Every configured remote, unless it skips the default update.
    let all = Scratch::new(&backup);
    assert_eq!(all.helpers("remote update"), Ok(vec!["fixture".into()]));
    let skipped = Scratch::new(&format!("{backup}\tskipDefaultUpdate = true\n"));
    assert_eq!(skipped.helpers("remote update"), Ok(vec![]));
    assert_eq!(
        skipped.helpers("remote update backup"),
        Ok(vec!["fixture".into()])
    );
    let fetch_all = Scratch::new(&format!("{backup}\tskipFetchAll = yes\n"));
    assert_eq!(fetch_all.helpers("remote update"), Ok(vec![]));
    let disagree = Scratch::new(&format!(
        "{backup}\tskipDefaultUpdate = true\n\tskipFetchAll = false\n"
    ));
    assert_eq!(
        disagree.helpers("remote update"),
        Ok(vec!["fixture".into()])
    );
    let unclear = Scratch::new(&format!("{backup}\tskipDefaultUpdate = maybe\n"));
    unclear.unproven("remote update", "cannot be read");
    // The default group, when it is set, replaces every remote.
    let default = Scratch::new(&format!("{backup}[remotes]\n\tdefault = origin\n"));
    assert_eq!(default.helpers("remote update"), Ok(vec![]));
    let both = Scratch::new(&format!("{backup}[remotes]\n\tdefault = origin backup\n"));
    assert_eq!(both.helpers("remote update"), Ok(vec!["fixture".into()]));
    let tabbed = Scratch::new(&format!(
        "{backup}[remotes]\n\tdefault = \"origin\\tbackup\"\n"
    ));
    assert_eq!(tabbed.helpers("remote update"), Ok(vec!["fixture".into()]));
    let nested = Scratch::new(&format!(
        "{backup}[remotes]\n\tdefault = origin mirrors\n\tmirrors = backup\n"
    ));
    nested.unproven("remote update", "remote group");
    // A remote a legacy remote file may define refuses.
    let legacy = Scratch::new(HTTPS);
    assert_eq!(legacy.helpers("remote update"), Ok(vec![]));
    std::fs::create_dir_all(legacy.root().join(".git/remotes")).unwrap();
    assert_eq!(legacy.helpers("remote update"), Ok(vec![]));
    std::fs::write(legacy.root().join(".git/remotes/old"), "URL: fixture::x\n").unwrap();
    legacy.unproven("remote update", "legacy remote file");
}

#[test]
fn an_empty_default_group_refuses_since_git_falls_back_to_its_name() {
    const FALLBACK: &str = "[remote \"default\"]\n\turl = fixture::x\n";
    for group in ["\"\"", "\" \\t \""] {
        let configured = Scratch::new(&format!(
            "{HTTPS}{FALLBACK}[remotes]\n\tdefault = {group}\n"
        ));
        configured.unproven("remote update", "empty `default` remote group");
        let legacy = Scratch::new(&format!("{HTTPS}[remotes]\n\tdefault = {group}\n"));
        std::fs::create_dir_all(legacy.root().join(".git/remotes")).unwrap();
        std::fs::write(
            legacy.root().join(".git/remotes/default"),
            "URL: fixture::x\n",
        )
        .unwrap();
        legacy.unproven("remote update", "empty `default` remote group");
    }
    // A group that lists a remote still reads its members.
    let listed = Scratch::new(&format!("{HTTPS}{FALLBACK}[remotes]\n\tdefault = origin\n"));
    assert_eq!(listed.helpers("remote update"), Ok(vec![]));
}

#[test]
fn a_remote_section_with_an_empty_name_refuses() {
    let empty = Scratch::new(&format!("{HTTPS}[remote \"\"]\n\turl = fixture::x\n"));
    empty.unproven("remote update", "empty name");
    // A dotted name and names that differ only in case are each read.
    let dotted = Scratch::new(&format!("{HTTPS}[remote \"a.b\"]\n\turl = fixture::x\n"));
    assert_eq!(dotted.helpers("remote update"), Ok(vec!["fixture".into()]));
    let cased = Scratch::new(&format!(
        "{HTTPS}[remote \"Up\"]\n\turl = fixture::x\n[remote \"up\"]\n\turl = https://example.test/r\n\tskipDefaultUpdate = true\n"
    ));
    assert_eq!(cased.helpers("remote update"), Ok(vec!["fixture".into()]));
    // The same section, or an empty name, as a promisor remote.
    let promisor = Scratch::new(&format!(
        "{HTTPS}[remote \"\"]\n\turl = https://example.test/r\n\tpromisor = true\n"
    ));
    promisor.unproven("status", "promisor remote with an empty name");
    let partial = Scratch::new(&format!("{HTTPS}[extensions]\n\tpartialClone = \"\"\n"));
    partial.unproven("status", "promisor remote with an empty name");
}

/// Replace the legacy remote folder `folder` of `scratch` with `form`.
#[cfg(unix)]
fn legacy_folder(scratch: &Scratch, folder: &str, form: &str) {
    let path = scratch.root().join(".git").join(folder);
    // A link or file goes with `remove_file`; a folder needs the other call.
    if std::fs::remove_file(&path).is_err() {
        let _ = std::fs::remove_dir_all(&path);
    }
    let target = scratch.root().join("empty-target");
    std::fs::create_dir_all(&target).unwrap();
    match form {
        "empty" => std::fs::create_dir(&path).unwrap(),
        "link to empty" => std::os::unix::fs::symlink(&target, &path).unwrap(),
        "dangling link" => std::os::unix::fs::symlink(target.join("gone"), &path).unwrap(),
        "file" => std::fs::write(&path, "").unwrap(),
        "populated" => {
            std::fs::create_dir(&path).unwrap();
            std::fs::write(path.join("old"), "URL: fixture::x\n").unwrap();
        }
        _ => unreachable!("{form}"),
    }
}

#[cfg(unix)]
#[test]
fn a_legacy_remote_folder_passes_only_as_a_real_empty_folder() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new(HTTPS);
    for folder in ["remotes", "branches"] {
        legacy_folder(&scratch, folder, "empty");
        assert_eq!(scratch.helpers("remote update"), Ok(vec![]), "{folder}");
        for form in ["link to empty", "dangling link", "file", "populated"] {
            legacy_folder(&scratch, folder, form);
            scratch.unproven("remote update", "legacy remote file");
        }
        // A real folder that cannot be read; skipped where the reader can
        // read it anyway, as a superuser can.
        legacy_folder(&scratch, folder, "empty");
        let path = scratch.root().join(".git").join(folder);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::read_dir(&path).is_err() {
            scratch.unproven("remote update", "legacy remote file");
        }
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        legacy_folder(&scratch, folder, "empty");
    }
}

#[test]
fn a_remote_with_more_than_one_address_value_refuses() {
    for remote in [
        "\turl = https://example.test/r\n\turl = fixture::x\n",
        "\turl = fixture::x\n\turl =\n\turl = https://example.test/r\n",
        "\turl =\n",
        "\tfetch = +refs/heads/*:refs/remotes/origin/*\n",
    ] {
        let scratch = Scratch::new(&format!("[remote \"origin\"]\n{remote}"));
        assert!(
            scratch
                .helpers("fetch origin")
                .unwrap_err()
                .starts_with(UNPROVEN),
            "{remote}"
        );
    }
    for pushurls in [
        "\tpushurl = https://example.test/r\n\tpushurl = fixture::x\n",
        "\tpushurl = fixture::x\n\tpushurl =\n",
        "\tpushurl =\n",
    ] {
        let scratch = Scratch::new(&format!(
            "[remote \"origin\"]\n\turl = https://example.test/r\n{pushurls}"
        ));
        scratch.unproven("push origin HEAD", "push address");
        scratch.unproven("fetch origin", "push address");
    }
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
    // A push URL takes the fetch rewrite.
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
fn an_unresolved_selection_refuses_naming_the_remote() {
    let bytes = Scratch::bytes(b"[remote \"fixture\"]\n\turl = https://example.test/\xff\n");
    let error = bytes.helpers("fetch fixture").unwrap_err();
    assert!(error.contains("remote `fixture`"), "{error}");
    for folder in ["remotes", "branches"] {
        let legacy = Scratch::new(HTTPS);
        std::fs::create_dir_all(legacy.root().join(".git").join(folder)).unwrap();
        std::fs::write(
            legacy.root().join(".git").join(folder).join("old"),
            "fixture::x\n",
        )
        .unwrap();
        legacy.unproven("fetch old", "legacy remote file");
        legacy.unproven("ls-remote old", "legacy remote file");
        assert_eq!(legacy.helpers("fetch origin"), Ok(vec![]));
    }
}

#[test]
fn submodule_recursion_refuses_unless_the_line_turns_it_off() {
    let configured = Scratch::new(&format!("{HTTPS}[submodule \"lib\"]\n\turl = fixture::x\n"));
    let active = Scratch::new(&format!("{HTTPS}[submodule]\n\tactive = .\n"));
    let listed = Scratch::new(HTTPS);
    std::fs::write(listed.root().join(".gitmodules"), "").unwrap();
    let cloned = Scratch::new(HTTPS);
    std::fs::create_dir_all(cloned.root().join(".git/modules/lib")).unwrap();
    for scratch in [&configured, &active, &listed, &cloned] {
        scratch.unproven("fetch origin", "submodule");
        scratch.unproven("pull origin main", "submodule");
        scratch.unproven(
            "fetch origin --recurse-submodules=no --recurse-submodules",
            "submodule",
        );
        scratch.unproven("remote update origin", "submodule");
        scratch.unproven("submodule update", "submodule");
        scratch.unproven(
            "submodule --quiet add https://example.test/lib lib",
            "submodule",
        );
        assert_eq!(
            scratch.helpers("fetch --no-recurse-submodules origin"),
            Ok(vec![])
        );
        assert_eq!(
            scratch.helpers("pull --recurse-submodules=no origin main"),
            Ok(vec![])
        );
        assert_eq!(scratch.helpers("push origin HEAD"), Ok(vec![]));
        assert_eq!(scratch.helpers("log"), Ok(vec![]));
    }
    for setting in [
        "[push]\n\trecurseSubmodules = on-demand\n",
        "[push]\n\trecurseSubmodules = check\n",
        "[submodule]\n\trecurse = true\n",
    ] {
        let scratch = Scratch::new(&format!("{HTTPS}{setting}"));
        std::fs::write(scratch.root().join(".gitmodules"), "").unwrap();
        scratch.unproven("push origin HEAD", "submodule");
        assert_eq!(
            scratch.helpers("push --no-recurse-submodules origin HEAD"),
            Ok(vec![])
        );
    }
    let plain = Scratch::new("");
    plain.unproven(
        "clone --recurse-submodules https://example.test/r copy",
        "clone",
    );
    plain.unproven("clone --recursive https://example.test/r copy", "clone");
    plain.unproven("clone --recurse https://example.test/r copy", "clone");
    assert_eq!(
        plain.helpers("clone https://example.test/r copy"),
        Ok(vec![])
    );
    let recursing = Scratch::new("[submodule]\n\trecurse = true\n");
    recursing.unproven("clone https://example.test/r copy", "clone");
}

#[test]
fn promisor_remotes_pass_the_same_rule() {
    let promisor = Scratch::new(&format!(
        "{HTTPS}[remote \"lazy\"]\n\turl = fixture::x\n\tpromisor = true\n"
    ));
    assert_eq!(promisor.helpers("status"), Ok(vec!["fixture".into()]));
    assert_eq!(promisor.helpers("log"), Ok(vec!["fixture".into()]));
    let partial = Scratch::new(&format!(
        "{HTTPS}[remote \"lazy\"]\n\turl = fixture::x\n[extensions]\n\tpartialClone = lazy\n"
    ));
    assert_eq!(partial.helpers("show HEAD"), Ok(vec!["fixture".into()]));
    let set = Scratch::new(&format!(
        "{HTTPS}[remote \"lazy\"]\n\turl = https://example.test/r\n\tvcs = fixture\n\tpromisor = true\n"
    ));
    set.unproven("status", "helper through its configuration");
    let missing = Scratch::new(&format!("{HTTPS}[extensions]\n\tpartialClone = gone\n"));
    missing.unproven("status", "remote `gone` is not configured");
}

#[test]
fn every_promisor_registration_git_reads_is_judged() {
    const SHADOW: &str = "[remote \"shadow\"]\n\turl = fixture::x\n";
    // A partial clone filter registers the remote whatever its promisor
    // setting, and a true value counts even when a later one is false.
    for extra in [
        "\tpartialCloneFilter = blob:none\n",
        "\tpromisor = false\n\tpartialCloneFilter = blob:none\n",
        "\tpromisor = true\n\tpromisor = false\n",
    ] {
        let scratch = Scratch::new(&format!("{HTTPS}{SHADOW}{extra}"));
        assert_eq!(
            scratch.helpers("status"),
            Ok(vec!["fixture".into()]),
            "{extra}"
        );
    }
    // A remote with none of them is no promisor.
    let plain = Scratch::new(&format!("{HTTPS}{SHADOW}"));
    assert_eq!(plain.helpers("status"), Ok(vec![]));
    let unset = Scratch::new(&format!("{HTTPS}{SHADOW}\tpromisor = false\n"));
    assert_eq!(unset.helpers("status"), Ok(vec![]));
    // The empty name, from an empty section name or from a key with no
    // section name, refuses.
    for config in [
        "[remote \"\"]\n\tpartialCloneFilter = blob:none\n",
        "[remote]\n\tpartialCloneFilter = blob:none\n",
        "[remote]\n\tpromisor = true\n",
    ] {
        let scratch = Scratch::new(&format!("{HTTPS}{config}"));
        scratch.unproven("status", "promisor remote with an empty name");
    }
}

#[test]
fn an_empty_operand_reads_as_the_address_git_builds_for_it() {
    // Git gives an empty operand a remote with no address, so it fetches
    // the operand itself after the rewrites.
    const REWRITES: &str = "[url \"https://ok.test/\"]\n\tinsteadOf = https://\n[url \"fixture::x\"]\n\tinsteadOf = \"\"\n";
    let scratch = Scratch::new(&format!("{HTTPS}{REWRITES}"));
    assert_eq!(
        scratch.helpers_of(&["fetch", ""]),
        Ok(vec!["fixture".into()])
    );
    // Beside a remote section with an empty name, it refuses.
    let section = Scratch::new(&format!(
        "{HTTPS}[remote \"\"]\n\turl = https://example.test/r\n{REWRITES}"
    ));
    let error = section.helpers_of(&["fetch", ""]).unwrap_err();
    assert!(
        error.starts_with(UNPROVEN) && error.contains("`remote..url`"),
        "{error}"
    );
}

#[test]
fn a_remote_section_with_an_empty_name_refuses_every_contact_with_remotes() {
    // Each key under `[remote ""]` defines a remote the guard does not
    // resolve, so every contact that reads the configuration's remotes
    // refuses, naming the key, whatever name or address the command gives.
    const CONTACTS: &[&str] = &[
        "fetch origin",
        "ls-remote origin",
        "fetch https://example.test/r",
        "ls-remote https://example.test/r",
        "remote update origin",
        "clone https://example.test/r copy",
        "remote add -f fresh https://example.test/r",
    ];
    const SECTION: &str = "[remote \"\"]\n\turl = fixture::x\n";
    for (key, line, selecting) in [
        (
            "remote..url",
            "\turl = fixture::x\n",
            &["ls-remote .url", "fetch .url", "archive --remote=.url HEAD"][..],
        ),
        (
            "remote..pushurl",
            "\tpushurl = fixture::x\n",
            &["push .pushurl HEAD:refs/heads/task/x"][..],
        ),
        ("remote..vcs", "\tvcs = fixture\n", &["ls-remote .vcs"][..]),
    ] {
        let scratch = Scratch::new(&format!("{HTTPS}[remote \"\"]\n{line}"));
        for command in selecting.iter().chain(CONTACTS) {
            scratch.unproven(command, &format!("`{key}`"));
        }
        // A command that contacts no remote is not judged by it.
        assert_eq!(scratch.helpers("status"), Ok(vec![]), "{key}");
    }
    // A remote the configuration selects by the same name refuses too.
    for (config, command) in [
        ("[branch \"main\"]\n\tremote = .url\n", "fetch"),
        ("[extensions]\n\tpartialClone = .url\n", "status"),
        ("[remotes]\n\tdefault = .url\n", "remote update"),
        ("", "remote update"),
    ] {
        let scratch = Scratch::new(&format!("{HTTPS}{SECTION}{config}"));
        scratch.unproven(command, "`remote..url`");
    }
    // A section whose name starts with a dot is an ordinary remote.
    let dotted = Scratch::new(&format!("{HTTPS}[remote \".a\"]\n\turl = fixture::x\n"));
    assert_eq!(dotted.helpers("ls-remote .a"), Ok(vec!["fixture".into()]));
    assert_eq!(dotted.helpers("fetch origin"), Ok(vec![]));
}

#[test]
fn every_recursion_option_spelling_git_accepts_reads_as_on() {
    let plain = Scratch::new("");
    for command in [
        "clone --recurse-submodules=no https://example.test/r copy",
        "clone --recurse-submodules=false https://example.test/r copy",
        "clone --recurs https://example.test/r copy",
        "clone --recursive=lib https://example.test/r copy",
        "clone --no-recurse-submodules --recurse-submodules=no https://example.test/r copy",
    ] {
        plain.unproven(command, "clone without recursion");
    }
    for command in [
        "clone -c submodule.recurse=true https://example.test/r copy",
        "clone -qc submodule.active=. https://example.test/r copy",
        "clone --config=Submodule.Recurse=1 https://example.test/r copy",
        "clone --con submodule.recurse=1 https://example.test/r copy",
        "clone -c",
    ] {
        plain.unproven(command, "submodule setting");
    }
    assert_eq!(
        plain.helpers("clone --no-recurse-submodules https://example.test/r copy"),
        Ok(vec![])
    );
    assert_eq!(
        plain.helpers("clone -c core.autocrlf=false https://example.test/r copy"),
        Ok(vec![])
    );

    let listed = Scratch::new(HTTPS);
    std::fs::write(listed.root().join(".gitmodules"), "").unwrap();
    for command in [
        "push --no-recurse-submodules --recu=on-demand origin HEAD:refs/heads/task/x",
        "push --no-recurse-submodules --r=check origin HEAD",
        "push --recurse-submodules=only origin HEAD",
        "push --no-recurse-submodules --no-recurse origin HEAD",
        "fetch --no-recurse-submodules --recurse=no origin",
    ] {
        listed.unproven(command, "submodule");
    }
    for order in [
        "[push]\n\trecurseSubmodules = no\n[submodule]\n\trecurse = true\n",
        "[submodule]\n\trecurse = true\n[push]\n\trecurseSubmodules = no\n",
    ] {
        let scratch = Scratch::new(&format!("{HTTPS}{order}"));
        std::fs::write(scratch.root().join(".gitmodules"), "").unwrap();
        scratch.unproven("push origin HEAD", "submodule");
    }
    let off = Scratch::new(&format!("{HTTPS}[push]\n\trecurseSubmodules = no\n"));
    std::fs::write(off.root().join(".gitmodules"), "").unwrap();
    assert_eq!(off.helpers("push origin HEAD"), Ok(vec![]));
}

#[test]
fn indirect_contact_paths_refuse_unless_proven() {
    let plain = Scratch::new(HTTPS);
    for command in [
        "submodule--helper update --init",
        "submodule--helper add https://example.test/lib lib",
        "submodule--helper clone --path lib --url https://example.test/lib",
        "submodule--helper --quiet update",
        "submodule--helper",
    ] {
        plain.unproven(command, "internal submodule builtin");
    }
    for command in [
        "submodule--helper foreach --quiet true",
        "submodule--helper status",
    ] {
        assert_eq!(plain.helpers(command), Ok(vec![]), "{command}");
    }

    let fresh = "remote add -f fresh https://example.test/r";
    assert_eq!(plain.helpers(fresh), Ok(vec![]));
    assert_eq!(
        plain.helpers("remote add --tags --fetch fresh https://example.test/r"),
        Ok(vec![])
    );
    plain.unproven(
        "remote add -f -t main fresh https://example.test/r",
        "fetches with settings",
    );
    plain.unproven(
        "remote add -vf fresh https://example.test/r",
        "fetches with settings",
    );
    plain.unproven("remote add -f fresh", "exactly one name and one address");

    let helper = Scratch::new(&format!("{HTTPS}[remote \"fresh\"]\n\tvcs = fixture\n"));
    for command in [
        fresh,
        "remote add --fe fresh https://example.test/r",
        "remote add --no-fetch fresh https://example.test/r",
    ] {
        helper.unproven(command, "already holds settings for remote `fresh`");
    }
    assert_eq!(
        helper.helpers("remote add fresh https://example.test/r"),
        Ok(vec![])
    );
    let group = Scratch::new(&format!("{HTTPS}[remotes]\n\tfresh = origin\n"));
    group.unproven(fresh, "already holds settings");
    let legacy = Scratch::new(HTTPS);
    std::fs::create_dir_all(legacy.root().join(".git/remotes")).unwrap();
    std::fs::write(
        legacy.root().join(".git/remotes/fresh"),
        "URL: fixture::x\n",
    )
    .unwrap();
    legacy.unproven(fresh, "legacy remote file");
}

/// Git takes the word after a value-taking option as its value whatever it
/// looks like, so in these rows a turn-off word or a terminator it consumes
/// proves nothing, and the reading goes on past it.
#[test]
fn a_word_an_option_consumes_proves_nothing() {
    let listed = Scratch::new(HTTPS);
    std::fs::write(listed.root().join(".gitmodules"), "").unwrap();
    for command in [
        "fetch --recurse-submodules=on-demand --server-option --no-recurse-submodules origin",
        "fetch -o --no-recurse-submodules origin",
        "fetch --server-option --recurse-submodules=no origin",
        "pull origin main --strategy --no-recurse-submodules",
        "push --recurse-submodules --no-recurse-submodules origin HEAD",
    ] {
        listed.unproven(command, "submodule");
    }
    for command in [
        "fetch -q --no-recurse-submodules origin",
        "fetch --depth=1 --no-recurse-submodules origin",
        "fetch --recurse-submodules --no-recurse-submodules origin",
        "pull origin main --no-recurse-submodules",
    ] {
        assert_eq!(listed.helpers(command), Ok(vec![]), "{command}");
    }

    let plain = Scratch::new("");
    for command in [
        "clone --recurse-submodules --server-option --no-recurse-submodules https://example.test/r copy",
        "clone --server-option -- --recurse-submodules https://example.test/r copy",
        "clone --recursive -u --no-recurse-submodules https://example.test/r copy",
    ] {
        plain.unproven(command, "clone without recursion");
    }
    plain.unproven(
        "clone --server-option -- -c submodule.recurse=true https://example.test/r copy",
        "submodule setting",
    );
    assert_eq!(
        plain.helpers("clone -q --no-recurse-submodules https://example.test/r copy"),
        Ok(vec![])
    );

    let origin = Scratch::new(HTTPS);
    for command in [
        "push -o -- --dry-run",
        "push --recurse-submodules no",
        "ls-remote -u x",
    ] {
        origin.unproven(command, "names no remote");
    }
    assert_eq!(origin.helpers("push origin -- HEAD"), Ok(vec![]));

    let helper = Scratch::new(&format!("{HTTPS}[remote \"fresh\"]\n\tvcs = fixture\n"));
    for scratch in [&origin, &helper] {
        scratch.unproven(
            "remote add -t -- -f fresh https://example.test/r",
            "fetches with settings",
        );
    }
}

/// Every verb the contacting-verb inventory lists either resolves its
/// contact here or refuses by name.
#[test]
fn a_listed_contacting_verb_without_a_resolution_refuses() {
    let plain = Scratch::new(HTTPS);
    plain.unproven("request-pull HEAD fixture::x HEAD", "`request-pull`");
    plain.unproven(
        "request-pull HEAD https://example.test/r HEAD",
        "`request-pull`",
    );
    assert_eq!(plain.helpers("credential fill"), Ok(vec![]));
    assert_eq!(plain.helpers("ls-remote origin"), Ok(vec![]));
}

#[test]
fn a_maintenance_run_passes_only_a_proven_local_task_list() {
    let scratch = Scratch::new(&format!("{HTTPS}[remote \"backup\"]\n\turl = fixture::x\n"));
    for command in [
        "maintenance run --task=PREFETCH",
        "maintenance run --task=Prefetch",
        "maintenance run --task=gc --ta=prefetch",
        "maintenance run --task=gc --t prefetch",
        "maintenance run --task=gc --auto",
        "maintenance run --task=gc --schedule=hourly",
        "maintenance run --task=gc -x",
        "maintenance run --task=gc extra",
        "maintenance run --task",
        "maintenance run --quiet",
        "maintenance --quiet run --task=gc",
    ] {
        scratch.unproven(command, "prefetch");
    }
    for command in [
        "maintenance run --task=GC",
        "maintenance run --ta=gc --task commit-graph",
        "maintenance run --task=gc --quiet --no-detach",
        "maintenance run -q --task=pack-refs",
        "maintenance register",
    ] {
        assert_eq!(scratch.helpers(command), Ok(vec![]), "{command}");
    }
}

#[cfg(unix)]
#[test]
fn a_legacy_remote_name_with_a_backslash_is_checked() {
    let scratch = Scratch::new(HTTPS);
    std::fs::create_dir_all(scratch.root().join(".git/remotes")).unwrap();
    std::fs::write(
        scratch.root().join(".git/remotes/old\\name"),
        "URL: fixture::x\n",
    )
    .unwrap();
    scratch.unproven("ls-remote old\\name", "legacy remote file");
    scratch.unproven("fetch old\\name", "legacy remote file");
}
