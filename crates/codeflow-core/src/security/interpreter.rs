//! Conservative literal inspection inside interpreter code, never ordinary
//! data, and of the arguments an interpreter is given (TSK-302).
use super::search_pattern::CommandLine;
use super::{actions, outward, pattern::matches_extended_glob};
use crate::hooks::{PolicyLevel, SecuritySection, Violation};
use std::path::Path;

pub(crate) fn evaluate(
    command: &str,
    levels: &SecuritySection,
    integrity: PolicyLevel,
    cwd: &Path,
    root: &Path,
    home: Option<&Path>,
) -> Vec<Violation> {
    let _scope = crate::hooks::edit_guard::RepoFactsScope::enter();
    // A rooted Windows home without a drive inherits the caller's drive.
    // Keep absolute homes unchanged so kernel link/parent semantics survive.
    let resolved_home = home.map(|home| {
        let home = if cfg!(windows) && home.has_root() && !home.is_absolute() {
            cwd.join(home)
        } else {
            home.to_path_buf()
        };
        // The original spelling remains available when no mapping exists;
        // using it as an operand still takes guard_path's refusal below.
        crate::portable_path::guard_path(&home).unwrap_or(home)
    });
    let home = resolved_home.as_deref();
    // Position-scoped arguments carry more precise provenance than the broad
    // literal scan below. Keep their finding first when both name one path.
    let mut violations = super::arguments::evaluate(command, integrity, cwd, root, home);
    violations.extend(secret_reads(command, levels, cwd, home));
    let visible = if command.contains("<<") {
        super::composed::paths::execution_text(command, cwd, home)
    } else {
        command.to_owned()
    };
    let mut codes = outward::interpreter_bodies(&visible);
    codes.extend(
        outward::interpreter_calls_with_home(command, cwd, home)
            .into_iter()
            .flat_map(|call| call.code),
    );
    codes.sort();
    codes.dedup();
    for code in codes {
        if code == "__unresolved_shell_code" && levels.secret_reads.is_active() {
            violations.push(Violation::new(
                "security.secret_reads",
                levels.secret_reads,
                "cannot verify the executable input's secret reads".to_string(),
                crate::remedy::OUTWARD_ACTION.remedy(),
            ));
            continue;
        }
        if levels.headless_peer_runs.is_active() {
            if let Some(run) = super::headless::raw_run(&code) {
                violations.push(crate::hooks::exec_guard::headless_violation(
                    levels.headless_peer_runs,
                    &run,
                ));
            }
        }
        for word in outward::literal_words(&code) {
            if levels.secret_reads.is_active() && secret_path(&word, cwd, home, None) {
                violations.push(Violation::new(
                    "security.secret_reads",
                    levels.secret_reads,
                    format!("interpreter code names the secret store `{word}`"),
                    crate::remedy::OUTWARD_ACTION.remedy(),
                ));
            }
            // Only path-like literals need filesystem metadata. Ordinary code
            // identifiers cannot be enforcement paths.
            let authority = (word.contains('/') || word.starts_with('.'))
                && crate::hooks::edit_guard::repository_authority_target(
                    &cwd.join(&word),
                    root,
                    false,
                );
            if authority {
                violations.push(Violation::always_blocking("git.hook_integrity",
                    format!("interpreter code names policy authority metadata `{word}`"),
                    "the operator manages remote-tracking authority; use the configured remote's ordinary fetch"));
                continue;
            }
            if integrity.is_active() && (word.contains('/') || word.starts_with('.')) {
                match crate::hooks::edit_guard::enforcement_path(&word, cwd, root, home) {
                    Ok(false) => {}
                    result => violations.push(Violation::new(
                        "git.hook_integrity",
                        integrity,
                        match result {
                            Ok(true) => {
                                format!("interpreter code names the enforcement path `{word}`")
                            }
                            Err(why) => format!("cannot inspect interpreter path `{word}`: {why}"),
                            Ok(false) => unreachable!(),
                        },
                        crate::remedy::HOOK_INTEGRITY.remedy(),
                    )),
                }
            }
        }
    }
    violations
}

fn secret_reads(
    command: &str,
    levels: &SecuritySection,
    cwd: &Path,
    home: Option<&Path>,
) -> Vec<Violation> {
    let mapped_home = home
        .map(|home| crate::portable_path::guard_path(home).unwrap_or_else(|_| home.to_path_buf()));
    let home = mapped_home.as_deref();
    // Spell the working directory as the home is spelled: store and glob
    // checks compare cwd-joined text with the home's text, and a Windows cwd
    // under `RUNNER~1` would never match a mapped home (hosted run
    // 40620672525, `cat auth.{json}` in the store).
    let mapped_cwd = crate::portable_path::guard_path(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    let cwd = mapped_cwd.as_path();
    let mut violations = Vec::new();
    // Every call on the line shares the line's facts, read once.
    let line = CommandLine::new(command);
    if levels.secret_reads.is_active() {
        for unwrapped in outward::unwrapped_commands(command, cwd, home) {
            let args = &unwrapped.args;
            let name = args
                .first()
                .map_or("", |s| super::unresolved::writer_name(s));
            let reader_cwd = unwrapped.cwd.as_deref().unwrap_or(cwd);
            let mut recursive = recursive_read(name, &unwrapped, reader_cwd, home, &line);
            if recursive.is_some() && unwrapped.cwd.is_none() {
                recursive = Some(Err(
                    "cannot establish the recursive reader's working directory",
                ));
            }
            if let Some(result) = &recursive {
                if result.as_ref().map_or(true, |secret| *secret) {
                    violations.push(Violation::new(
                        "security.secret_reads",
                        levels.secret_reads,
                        match result {
                            Ok(_) => "a recursive reader selects a secret file or store".into(),
                            Err(why) => format!("cannot verify every recursive read path: {why}"),
                        },
                        crate::remedy::SECRET_SELECTION.remedy(),
                    ));
                    continue;
                }
            }
            for (at, value, glob) in
                file_operands(name, args, unwrapped.expands.as_deref(), &line, reader_cwd)
            {
                if glob && recursive.is_some() {
                    continue;
                }
                let secret =
                    inspect_operand(&line, &unwrapped, (at, value, glob), reader_cwd, home);
                if secret.as_ref().map_or(true, |value| *value) {
                    violations.push(Violation::new(
                        "security.secret_reads",
                        levels.secret_reads,
                        match secret {
                            Err(why) => format!("cannot verify file operand `{value}`: {why}"),
                            Ok(_) => format!("file operand names a secret file or store `{value}`"),
                        },
                        if secret.is_err() {
                            crate::remedy::SECRET_SELECTION.remedy()
                        } else {
                            crate::remedy::OUTWARD_ACTION.remedy()
                        },
                    ));
                    break;
                }
            }
        }
    }
    violations
}

/// File operands retain their command-position provenance. Declared links
/// add possible real paths before they exist on disk; virtual script
/// descriptors keep the interpreter's existing best-effort code boundary.
fn inspect_operand(
    line: &CommandLine<'_>,
    call: &outward::Unwrapped,
    operand: (usize, &str, bool),
    cwd: &Path,
    home: Option<&Path>,
) -> Result<bool, &'static str> {
    let (at, value, glob) = operand;
    if value == "__unverified_transfer_input" {
        return Err("cannot establish a transfer option's quoted input filename");
    }
    if git_revision_word(call, at, value, cwd) {
        return Ok(false);
    }
    let name = call
        .args
        .first()
        .map_or("", |program| super::unresolved::writer_name(program));
    if matches!(name, "rsync" | "scp")
        && remote_endpoint(value)
        && super::unresolved::writer_effects(name, &call.args[1..], &[])
            .and_then(|effects| effects.copy)
            .and_then(|copy| copy.dest)
            .as_deref()
            == Some(value)
    {
        // A parsed remote destination is not a local content-read operand.
        return Ok(false);
    }
    if outward::descriptor_program(value)
        && (crate::hooks::git_guard::is_shell(name) || outward::interpreter_family(name).is_some())
    {
        return Ok(false);
    }
    let supplied = matches!(call.supplier, Some(outward::Supplier::Find { .. }))
        && matches!(value, "$1" | "${1}" | "$@" | "${@}" | "$*" | "${*}");
    if !supplied
        && call.unresolved.iter().any(|word| {
            crate::hooks::git_guard::command_argv(word)
                .iter()
                .any(|part| part.contains(value))
        })
    {
        return Err("the file operand is supplied at run time and its paths cannot be established");
    }
    let expands = glob
        || call
            .expands
            .as_ref()
            .is_none_or(|flags| flags.get(at).copied().unwrap_or(true));
    if expands
        && crate::hooks::git_guard::has_glob(value)
        && crate::hooks::git_guard::dot_glob_option(line.text()).is_some()
    {
        return Err("shell options change which hidden paths the glob selects");
    }
    if call.cwd.is_none() && (glob || expands && crate::hooks::git_guard::has_glob(value)) {
        return uncertain_glob(
            value,
            cwd,
            home,
            "cannot establish the glob's working directory",
        );
    }
    if glob {
        return secret_search_glob(
            value,
            name,
            &call.args,
            call.expands.as_deref(),
            cwd,
            home,
            line,
            &call.links,
        );
    }
    let symbolic_link = name == "ln"
        && call.args.iter().skip(1).any(|arg| {
            arg == "--symbolic"
                || arg == "--symbolic-link"
                || arg.starts_with('-') && !arg.starts_with("--") && arg.contains('s')
        });
    let reading = (!symbolic_link).then_some(expands);
    if secret_path_result(value, cwd, home, reading)? {
        return Ok(true);
    }
    for mapped in super::composed::paths::linked_paths(value, cwd, &call.links)
        .map_err(|_| "cannot resolve a read through a link declared on this line")?
    {
        if secret_path_result(&secret_path_text(&mapped)?, cwd, home, reading)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// A Git operand in revision form that names a short-name component but no
/// path is a revision, not a file operand, so it needs no Windows
/// short-name mapping (`git rebase -x 'cargo test' HEAD~3`). Git reads a
/// word before `--` as a revision unless a path by that name exists: a
/// non-revision operand must name an existing path, and a word that is both
/// is refused as ambiguous. The word stays a file operand when a path by
/// that name exists in the directory Git runs in or its work tree, after
/// `--`, with `--no-index`, with a `-C` or work tree the guard cannot read,
/// beside a link made earlier on the line, or when its path cannot be
/// classified.
fn git_revision_word(call: &outward::Unwrapped, at: usize, value: &str, cwd: &Path) -> bool {
    let Some((program, rest)) = call.args.split_first() else {
        return false;
    };
    if !call.links.is_empty()
        || !crate::portable_path::has_short_component(value)
        || !crate::hooks::git_guard::revision_word(value)
    {
        return false;
    }
    let Some(git) = crate::hooks::git_guard::git_arguments(program, rest) else {
        return false;
    };
    let Some((_, tail)) = crate::hooks::git_guard::git_subcommand(&git) else {
        return false;
    };
    let start = call.args.len() - tail.len();
    if at < start
        || call.args[start..at].iter().any(|word| word == "--")
        || tail.iter().any(|word| word == "--no-index")
    {
        return false;
    }
    // Git checks the word against the directory it runs in and, with a
    // work tree option, against that tree's top. The word must name nothing
    // in either; `--git-dir` alone moves neither.
    let mut directory = cwd.to_path_buf();
    let mut trees = Vec::new();
    let globals = &git[..git.len() - tail.len() - 1];
    let mut words = globals.iter();
    while let Some(word) = words.next() {
        let (tree, location) = match word.as_str() {
            "-C" => (false, words.next().map(String::as_str)),
            "--work-tree" => (true, words.next().map(String::as_str)),
            "-c" => match words.next().and_then(|setting| setting.split_once('=')) {
                Some((key, value)) if key.eq_ignore_ascii_case("core.worktree") => {
                    (true, Some(value))
                }
                _ => continue,
            },
            _ => match word.strip_prefix("--work-tree=") {
                Some(value) => (true, Some(value)),
                None => (false, word.strip_prefix("-C")),
            },
        };
        let Some(location) = location else {
            continue;
        };
        if crate::hooks::git_guard::expands(location)
            || location.starts_with('~')
            || crate::hooks::git_guard::has_glob(location)
        {
            return false;
        }
        let joined = outward::lexical_join(&directory, Path::new(location));
        if tree {
            trees.push(joined);
        } else {
            directory = joined;
        }
    }
    std::iter::once(directory).chain(trees).all(|base| {
        matches!(
            crate::absence::entry(&outward::lexical_join(&base, Path::new(value))),
            Ok(crate::absence::Entry::Creatable | crate::absence::Entry::Impossible)
        )
    })
}

/// `reader` is `Some` for a reader's argument, saying whether the shell
/// expands it as a pattern, and `None` for a literal in interpreter code.
fn secret_path(word: &str, cwd: &Path, home: Option<&Path>, reader: Option<bool>) -> bool {
    secret_path_result(word, cwd, home, reader).unwrap_or(true)
}

fn secret_path_result(
    word: &str,
    cwd: &Path,
    home: Option<&Path>,
    reader: Option<bool>,
) -> Result<bool, &'static str> {
    // A directory change on the line can name a short-name directory; spell
    // it as the home is spelled before any store or glob comparison.
    let mapped_cwd = crate::portable_path::guard_path(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    let cwd = mapped_cwd.as_path();
    let spelling = crate::portable_path::filesystem_spelling(word);
    let word = spelling.as_str();
    let expanded_word = word
        .strip_prefix("$HOME/")
        .or_else(|| word.strip_prefix("${HOME}/"))
        .map(|suffix| format!("~/{suffix}"));
    let word = expanded_word.as_deref().unwrap_or(word);
    // A named home may be another user's, but the configured store suffix
    // remains sensitive. Never let a leading slash replace the joined home.
    let tilde_word = (reader != Some(false))
        .then(|| {
            word.strip_prefix('~')
                .and_then(|tail| tail.split_once('/'))
                .map(|(_, suffix)| format!("~/{}", suffix.trim_start_matches('/')))
        })
        .flatten();
    let word = tilde_word.as_deref().unwrap_or(word);
    if reader.is_none() && word == "~" {
        return Ok(false);
    }
    // Keep the literal spelling for home patterns even when HOME is unknown.
    // Also judge relative store names from the payload cwd, then inspect
    // real targets when needed. Do not reduce paths to their basename:
    // auth.json is protected in its store, not in every project directory.
    let literal_tilde = word == "~" && reader != Some(true);
    let absolute = if word == "~" && reader == Some(true) {
        home.map(Path::to_path_buf)
    } else if let Some(suffix) = word.strip_prefix("~/").filter(|_| reader != Some(false)) {
        home.map(|home| outward::lexical_join(home, Path::new(suffix)))
    } else {
        Some(outward::lexical_join(cwd, Path::new(word)))
    };
    // A UNC path names a file in a share whose layout the guard cannot
    // see. Judge it before any mapping or metadata read, so the guard never
    // contacts a host a command names before refusing a store spelling.
    if cfg!(windows)
        && absolute.as_deref().is_some_and(|path| {
            unc_store_suffix(&crate::portable_path::slashed(path), home, reader)
        })
    {
        return Ok(true);
    }
    // Map the path the word names, after its home is resolved, never
    // `cwd.join(word)`: a Windows cwd under `RUNNER~1` would otherwise turn
    // `~/store` into `<cwd>/~/store` and hide the store (hosted run
    // 40602309629). Namespace and stream spellings refuse here for every
    // word; only a short name in the word itself replaces its spelling.
    let absolute = match absolute
        .map(|path| crate::portable_path::guard_path(&path))
        .transpose()
    {
        Ok(absolute) => absolute,
        // A literal in interpreter code is often no path at all (Perl's
        // `File::Path`). Judge the file or directory a stream spelling
        // opens; a spelling that still cannot be mapped refuses (hosted
        // run 40623450157).
        Err(_) if reader.is_none() => {
            let base = stream_base(word);
            return if base == word {
                Err("cannot establish a Windows path's mapped filename")
            } else {
                secret_path_result(&base, cwd, home, reader)
            };
        }
        Err(_) => return Err("cannot establish a Windows path's mapped filename"),
    };
    let mapped_word = absolute
        .as_deref()
        .filter(|_| cfg!(windows) && crate::portable_path::has_short_component(word))
        .map(crate::portable_path::slashed);
    let word = mapped_word.as_deref().unwrap_or(word);
    if reader == Some(true) && (crate::hooks::git_guard::has_glob(word) || word.contains('{')) {
        return secret_glob(word, cwd, home);
    }
    Ok(
        // Flat operands name one file or store. Only recursive root
        // inspection applies the ancestor rule for descendant reads.
        (!literal_tilde && secret_path_spelling(word, home, None))
            || absolute.as_deref().is_some_and(|path| {
                secret_path_spelling(&crate::portable_path::slashed(path), home, None)
            })
            || match absolute.as_deref() {
                Some(path) => real_secret_path(path, home)?,
                None => false,
            },
    )
}

/// Whether `text` (with `/` separators) is a UNC path, in any namespace
/// spelling, that ends in a protected home-relative path. A share can map
/// any directory of any machine, the home of this user or another, so every
/// trailing part from the share down is read as relative to a home: a store
/// reached through `//host/c$/Users/<u>/`, through a share of the home
/// itself, or through `//?/UNC/host/share/` refuses whatever the host or
/// share. Other UNC paths, a UNC home's ordinary files included, are judged
/// as usual.
fn unc_store_suffix(text: &str, home: Option<&Path>, reader: Option<bool>) -> bool {
    let lower = text.to_ascii_lowercase();
    let rest = if let Some(rest) = lower
        .strip_prefix("//?/unc/")
        .or_else(|| lower.strip_prefix("//./unc/"))
    {
        rest
    } else if lower.starts_with("//?/") || lower.starts_with("//./") {
        return false;
    } else if let Some(rest) = lower.strip_prefix("//") {
        rest
    } else {
        return false;
    };
    let parts: Vec<&str> = rest.split('/').filter(|part| !part.is_empty()).collect();
    (1..parts.len())
        .any(|at| secret_path_spelling(&format!("~/{}", parts[at..].join("/")), home, reader))
}

/// `word` (with `/` separators) without NTFS stream syntax: each component
/// ends at its first `:` unless it is a drive (`C:`). `x::$DATA`,
/// `dir:$I30:$INDEX_ALLOCATION/f` and `f:name` open `x`, `dir/f` and `f`.
fn stream_base(word: &str) -> String {
    word.split('/')
        .map(|part| {
            let drive = part.len() == 2
                && part.ends_with(':')
                && part.starts_with(|ch: char| ch.is_ascii_alphabetic());
            if drive {
                part
            } else {
                part.split(':').next().unwrap_or(part)
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn secret_path_spelling(word: &str, home: Option<&Path>, reader: Option<bool>) -> bool {
    let word = crate::portable_path::filesystem_spelling(word).to_ascii_lowercase();
    let home_relative = home.and_then(|home| {
        let home = crate::portable_path::slashed(home).to_ascii_lowercase();
        word.strip_prefix(home.trim_end_matches('/'))
            .filter(|tail| tail.is_empty() || tail.starts_with('/'))
            .map(|tail| format!("~{tail}"))
    });
    let word = home_relative.as_deref().unwrap_or(&word);
    // A directory rule also protects the store itself, even when it is
    // absent. This is equality, not the ancestor rule for descendant reads.
    secret_stores()
        .iter()
        .any(|store| word.trim_end_matches('/') == store)
        || actions::table().sandbox_read_denies.iter().any(|path| {
            word == path
                || word
                    .strip_prefix(path)
                    .is_some_and(|tail| tail.starts_with('/'))
                || reader.is_some_and(|expands| secret_prefix(word, path, expands))
        })
        || actions::table().claude_read_groups.iter().any(|group| {
            // This group has no carve-outs; source-name and env-file groups do.
            group.carveouts.is_empty()
                && group.denies.iter().any(|rule| {
                    let Some(pattern) =
                        rule.strip_prefix("Read(").and_then(|s| s.strip_suffix(')'))
                    else {
                        return false;
                    };
                    matches_extended_glob(word, pattern)
                        || reader.is_some_and(|expands| secret_prefix(word, pattern, expands))
                })
        })
}

/// Expand shell words without substituting a directory for an incomplete
/// listing. Store directories refuse before any filesystem enumeration.
fn secret_glob(word: &str, cwd: &Path, home: Option<&Path>) -> Result<bool, &'static str> {
    if home_rule_glob(word, cwd, home) {
        return Ok(true);
    }
    let readings = crate::hooks::git_guard::word_readings(word)
        .ok_or("cannot inspect every brace alternative")?;
    for reading in readings {
        let directory = reading.rsplit_once('/').map_or("", |(dir, _)| dir);
        if secret_directory(directory, cwd, home) {
            return Ok(true);
        }
        let Some((base, pattern)) = glob_base(&reading, cwd, home) else {
            return uncertain_glob(
                &reading,
                cwd,
                home,
                "cannot resolve the glob's home directory",
            );
        };
        if !crate::hooks::git_guard::has_glob(pattern) {
            if secret_path(pattern, base, home, Some(false)) {
                return Ok(true);
            }
            continue;
        }
        match crate::hooks::git_guard::secret_glob_paths(pattern, base) {
            Ok(paths) => {
                for path in paths {
                    let text = match secret_path_text(&path) {
                        Ok(text) => text,
                        Err(why) => return uncertain_glob(&reading, cwd, home, why),
                    };
                    if secret_path_spelling(&text, home, Some(false))
                        || real_secret_path(&path, home)?
                    {
                        return Ok(true);
                    }
                }
            }
            Err(why) => {
                if secret_intersection(&reading, cwd, home) {
                    return Err(why);
                }
            }
        }
    }
    Ok(false)
}

/// A named home rule protects its own filename, including conservative
/// brace readings. This does not turn its parent into a whole-directory
/// store or apply name intersection to ordinary workspace source globs.
fn home_rule_glob(word: &str, cwd: &Path, home: Option<&Path>) -> bool {
    let Some((base, relative)) = glob_base(word, cwd, home) else {
        return false;
    };
    let absolute = outward::lexical_join(base, Path::new(relative));
    let word = home_spelling(&crate::portable_path::slashed(&absolute), home);
    let Some(readings) = secret_pattern_readings(&word) else {
        return true;
    };
    // File selectors must share an actual filename. Adding descendants
    // here would turn a public *.md leaf into an ancestor of private jsonl.
    let intersects = |rule: &str| {
        rule.starts_with("~/")
            && readings
                .iter()
                .any(|reading| super::pattern::glob_intersects(reading, rule).unwrap_or(true))
    };
    let table = actions::table();
    table
        .sandbox_read_denies
        .iter()
        .map(String::as_str)
        .any(intersects)
        || table.claude_read_groups.iter().any(|group| {
            group.carveouts.is_empty()
                && group.denies.iter().any(|rule| {
                    rule.strip_prefix("Read(")
                        .and_then(|rule| rule.strip_suffix(')'))
                        .is_some_and(intersects)
                })
        })
}

fn uncertain_glob(
    word: &str,
    cwd: &Path,
    home: Option<&Path>,
    why: &'static str,
) -> Result<bool, &'static str> {
    if secret_intersection(word, cwd, home) {
        Err(why)
    } else {
        Ok(false)
    }
}

fn glob_base<'a>(
    word: &'a str,
    cwd: &'a Path,
    home: Option<&'a Path>,
) -> Option<(&'a Path, &'a str)> {
    if let Some(tail) = word.strip_prefix("~/") {
        home.map(|home| (home, tail))
    } else {
        Some((cwd, word))
    }
}

fn home_spelling(word: &str, home: Option<&Path>) -> String {
    let word = word.to_ascii_lowercase();
    home.and_then(|home| {
        let home = crate::portable_path::slashed(home).to_ascii_lowercase();
        word.strip_prefix(home.trim_end_matches('/'))
            .filter(|tail| tail.is_empty() || tail.starts_with('/'))
            .map(|tail| format!("~{tail}"))
    })
    .unwrap_or(word)
}

fn secret_stores() -> &'static [String] {
    static STORES: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    STORES.get_or_init(|| {
        let table = actions::table();
        let mut stores = Vec::new();
        let rules = table
            .claude_read_groups
            .iter()
            .filter(|group| group.carveouts.is_empty())
            .flat_map(|group| group.denies.iter())
            .filter_map(|rule| {
                rule.strip_prefix("Read(")
                    .and_then(|rule| rule.strip_suffix(')'))
            });
        for rule in rules.filter(|rule| rule.starts_with("~/")) {
            let Some(directory) = rule.strip_suffix("/**") else {
                continue;
            };
            if directory != "~" && !crate::hooks::git_guard::has_glob(directory) {
                stores.push(directory.to_owned());
            }
        }
        stores.sort();
        stores.dedup();
        stores
    })
}

fn secret_directory(directory: &str, cwd: &Path, home: Option<&Path>) -> bool {
    let Some((base, directory)) = glob_base(directory, cwd, home) else {
        return true;
    };
    let absolute = outward::lexical_join(base, Path::new(directory));
    let spelling = home_spelling(&crate::portable_path::slashed(&absolute), home);
    secret_stores().iter().any(|store| {
        spelling == *store
            || spelling
                .strip_prefix(store)
                .is_some_and(|tail| tail.starts_with('/'))
            || (crate::hooks::git_guard::has_glob(directory)
                && (super::pattern::glob_intersects(&spelling, store).unwrap_or(true)
                    || super::pattern::glob_intersects(&spelling, &format!("{store}/**"))
                        .unwrap_or(true)))
    })
}

fn secret_intersection(word: &str, cwd: &Path, home: Option<&Path>) -> bool {
    secret_path_spelling(word, home, Some(true))
        || glob_base(word, cwd, home).is_none_or(|(base, word)| {
            secret_path_spelling(
                &crate::portable_path::slashed(&outward::lexical_join(base, Path::new(word))),
                home,
                Some(true),
            )
        })
}

/// Existing names are judged through their real target as well as their
/// visible spelling. Proven absence is the only metadata failure that clears
/// a literal operand; unreadable and dangling links cannot certify a read.
fn real_secret_path(path: &Path, home: Option<&Path>) -> Result<bool, &'static str> {
    if secret_path_spelling(&secret_path_text(path)?, home, None) {
        return Ok(true);
    }
    if crate::absence::cannot_exist(path)
        .map_err(|_| "cannot establish whether a read path exists")?
    {
        return Ok(false);
    }
    let real = crate::portable_path::canonicalize(path)
        .map_err(|_| "cannot resolve a read path's real target")?;
    let text = secret_path_text(&real)?;
    // A link can resolve into a share (`\\?\UNC\host\share\...`).
    if secret_path_spelling(&text, home, None)
        || cfg!(windows) && unc_store_suffix(&text, home, None)
    {
        return Ok(true);
    }
    let real_home = home
        .map(|home| {
            crate::hooks::edit_guard::normalized(home, true)
                .map_err(|_| "cannot resolve the secret stores' home directory")
        })
        .transpose()?;
    Ok(secret_path_spelling(&text, real_home.as_deref(), None))
}

#[allow(clippy::too_many_arguments)] // Search syntax and shell provenance share one context.
fn secret_search_glob(
    pattern: &str,
    name: &str,
    args: &[String],
    expands: Option<&[bool]>,
    cwd: &Path,
    home: Option<&Path>,
    line: &CommandLine<'_>,
    links: &[super::composed::paths::Link],
) -> Result<bool, &'static str> {
    let rest = args.get(1..).unwrap_or_default();
    let literal: Vec<_> = (1..args.len())
        .map(|at| expands.is_some_and(|flags| !flags.get(at).copied().unwrap_or(true)))
        .collect();
    let mut roots =
        super::search_pattern::search_roots(name, rest, &literal, line.names_ripgrep_config())
            .ok_or("cannot establish the search roots")?;
    if roots.is_empty() {
        roots.push(".");
    }
    let mut read = 0;
    for root in roots {
        if secret_directory(root, cwd, home) {
            return Ok(true);
        }
        let (base, root) =
            glob_base(root, cwd, home).ok_or("cannot resolve a search root's home directory")?;
        let roots = if crate::hooks::git_guard::has_glob(root) {
            crate::hooks::git_guard::secret_glob_paths(root, base)?
        } else {
            vec![outward::lexical_join(base, Path::new(root))]
        };
        for root in roots {
            if search_root_matches(pattern, &root, home, links, &mut read)? {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Search tools recurse by default (grep only with a recursive flag). Copy
/// and archive creation tools consume every descendant of their input roots.
fn recursive_read(
    name: &str,
    call: &outward::Unwrapped,
    cwd: &Path,
    home: Option<&Path>,
    line: &CommandLine<'_>,
) -> Option<Result<bool, &'static str>> {
    let args = &call.args;
    let expands = call.expands.as_deref();
    let links = &call.links;
    let rest = args.get(1..).unwrap_or_default();
    let recursive = recursive_option(name, rest);
    if matches!(name, "rg" | "ag" | "ack") || name == "grep" && recursive {
        let literal: Vec<_> = (1..args.len())
            .map(|at| expands.is_some_and(|flags| !flags.get(at).copied().unwrap_or(true)))
            .collect();
        let selectors =
            super::search_pattern::path_values(name, rest, &literal, line.names_ripgrep_config());
        let Some(selectors) = selectors else {
            return Some(Err("cannot establish a recursive reader's selectors"));
        };
        // A negated selector cannot clear an otherwise selected secret. With
        // no positive selector the reader selects every file under its roots.
        let patterns: Vec<_> = selectors
            .iter()
            .filter(|(at, value, glob)| *glob && positive_selector(rest, *at, value))
            .map(|(_, value, _)| *value)
            .collect();
        if patterns.is_empty() {
            return Some(secret_search_glob(
                "*", name, args, expands, cwd, home, line, links,
            ));
        }
        for pattern in patterns {
            match secret_search_glob(pattern, name, args, expands, cwd, home, line, links) {
                Ok(false) => {}
                result => return Some(result),
            }
        }
        return Some(Ok(false));
    }
    let selected = archive_sources(name, rest, call.input.as_deref());
    let (roots, literal): (Vec<String>, bool) = if let Some(selected) = selected {
        match selected {
            Ok(sources) => sources,
            Err(why) => return Some(Err(why)),
        }
    } else {
        (
            match name {
                "grep" => return None, // Without recursion it reads only named files.
                "cp" if recursive => match super::unresolved::writer_effects(name, rest, &[])
                    .and_then(|effects| effects.copy)
                {
                    Some(copy) => copy.sources,
                    None => return Some(Err("cannot establish recursive copy sources")),
                },
                "zip" if recursive => rest
                    .iter()
                    .filter(|arg| !arg.starts_with('-'))
                    .skip(1)
                    .cloned()
                    .collect(),
                "tar" => match archive_read_roots(rest, cwd) {
                    Ok(Some(roots)) => roots,
                    Ok(None) => return None,
                    Err(why) => return Some(Err(why)),
                },
                _ => match conservative_read_roots(name, args, expands, cwd, home, line, links) {
                    Some(Ok(roots)) => roots,
                    Some(Err(why)) => return Some(Err(why)),
                    None => return None,
                },
            },
            false,
        )
    };
    Some(inspect_recursive_roots(&roots, literal, cwd, home, links))
}

fn inspect_recursive_roots(
    roots: &[String],
    literal: bool,
    cwd: &Path,
    home: Option<&Path>,
    links: &[super::composed::paths::Link],
) -> Result<bool, &'static str> {
    let mut read = 0;
    for root in roots {
        let (base, pattern) = if literal {
            (cwd, root.as_str())
        } else {
            glob_base(root, cwd, home).ok_or("cannot establish a recursive root")?
        };
        let roots = if !literal && crate::hooks::git_guard::has_glob(pattern) {
            crate::hooks::git_guard::secret_glob_paths(pattern, base)?
        } else {
            vec![base.join(pattern)]
        };
        for root in roots {
            if search_root_matches("*", &root, home, links, &mut read)? {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// These archive modes consume filename lists, not the contents of stdin.
/// Keep literal list bytes separate from shell-expanded argv roots.
fn archive_sources(
    name: &str,
    args: &[String],
    input: Option<&str>,
) -> Option<Result<(Vec<String>, bool), &'static str>> {
    if !matches!(name, "cpio" | "pax") {
        return None;
    }
    let mut flags = String::new();
    let mut operands = Vec::new();
    let mut unknown = false;
    let mut at = 0;
    while let Some(arg) = args.get(at) {
        at += 1;
        if arg == "--" {
            operands.extend_from_slice(&args[at..]);
            break;
        }
        if !arg.starts_with('-') || arg == "-" {
            operands.push(arg.clone());
        } else if let Some(long) = arg.strip_prefix("--") {
            let (option, attached) = long
                .split_once('=')
                .map_or((long, false), |(name, _)| (name, true));
            match option {
                "create" => flags.push('o'),
                "pass-through" => flags.push('p'),
                "null" => flags.push('0'),
                "owner" | "format" | "file" | "message" => at += usize::from(!attached),
                "quiet"
                | "verbose"
                | "make-directories"
                | "preserve-modification-time"
                | "unconditional" => {}
                _ => unknown = true,
            }
        } else {
            for (offset, flag) in arg[1..].char_indices() {
                flags.push(flag);
                let valued = if name == "pax" {
                    "bfopsxBEGTU"
                } else {
                    "CEFfHIORM"
                };
                if valued.contains(flag) {
                    at += usize::from(offset + 2 == arg.len());
                    break;
                }
                unknown |= !"0abBcdhilmoprtuvwHL".contains(flag);
            }
        }
    }
    let reads_names = if name == "pax" {
        flags.contains('w')
    } else {
        flags.contains(['o', 'p'])
    };
    if !reads_names {
        return None;
    }
    if unknown {
        return Some(Err("cannot establish archive filename-list options"));
    }
    if name == "pax" && flags.contains('r') {
        operands.pop(); // Pass-through destination is written, not read.
    }
    if name == "pax" && !operands.is_empty() {
        return Some(Ok((operands, false)));
    }
    let Some(input) = input else {
        return Some(Err("cannot establish the archive's stdin filename list"));
    };
    Some(Ok((
        input
            .split(if flags.contains('0') { '\0' } else { '\n' })
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .collect(),
        true,
    )))
}

/// Directory-capable copy operations and unmodelled recursive/archive syntax
/// retain every candidate source. Unknown input streams cannot certify roots.
fn conservative_read_roots(
    name: &str,
    args: &[String],
    expands: Option<&[bool]>,
    cwd: &Path,
    home: Option<&Path>,
    line: &CommandLine<'_>,
    links: &[super::composed::paths::Link],
) -> Option<Result<Vec<String>, &'static str>> {
    if matches!(
        name,
        "ls" | "stat"
            | "test"
            | "["
            | "du"
            | "find"
            | "rm"
            | "rmdir"
            | "unlink"
            | "mv"
            | "ln"
            | "install"
            | "chmod"
            | "chown"
            | "chgrp"
            | "echo"
            | "printf"
            | "print"
    ) {
        return None;
    }
    let rest = args.get(1..).unwrap_or_default();
    let program_args = outward::recursive_arguments(name, rest);
    let effects = super::unresolved::writer_effects(name, rest, &[]);
    let flags = program_args.as_deref().unwrap_or_else(|| {
        effects
            .as_ref()
            .map_or(rest, |effects| effects.read.flags.as_slice())
    });
    let unknown = effects
        .as_ref()
        .is_some_and(|effects| effects.read.unknown.is_some());
    let archive = archive_syntax(flags) || unknown;
    if matches!(name, "cp" | "scp" | "rsync") && !archive {
        return None;
    }
    let copy = effects.and_then(|effects| effects.copy);
    if copy.is_none() && !archive {
        return None;
    }
    let roots: Vec<String> = if let Some(copy) = copy.filter(|_| !unknown) {
        copy.sources
    } else if let Some(program_args) = &program_args {
        program_args
            .iter()
            .flat_map(|word| super::arguments::argument_values(word))
            .map(str::to_owned)
            .collect()
    } else {
        file_operands(name, args, expands, line, cwd)
            .into_iter()
            .filter(|(at, _, _)| *at != 1 || !rest.first().is_some_and(|arg| archive_verb(arg)))
            .map(|(_, word, _)| word.to_owned())
            .collect()
    };
    let directories = match recursive_directories(roots, cwd, home, links) {
        Ok(directories) => directories,
        Err(why) => return Some(Err(why)),
    };
    if directories.is_empty() {
        return None;
    }
    if unknown
        || directories.iter().any(|root| {
            crate::hooks::git_guard::unresolved_word(root)
                || root == "-"
                || matches!(name, "rsync" | "scp") && remote_endpoint(root)
        })
    {
        Some(Err(
            "cannot establish every source of an unmodelled recursive or archive operation",
        ))
    } else {
        Some(Ok(directories))
    }
}

fn recursive_directories(
    roots: Vec<String>,
    cwd: &Path,
    home: Option<&Path>,
    links: &[super::composed::paths::Link],
) -> Result<Vec<String>, &'static str> {
    let mut directories = Vec::new();
    for root in roots {
        let Some((base, word)) = glob_base(&root, cwd, home) else {
            return Err("cannot establish an unmodelled recursive root");
        };
        let path = base.join(word);
        let planned = links
            .iter()
            .any(|link| link.path().starts_with(&path) || path.starts_with(link.path()));
        if crate::hooks::git_guard::has_glob(word) || planned {
            directories.push(root);
        } else {
            if crate::absence::cannot_exist(&path)
                .map_err(|_| "cannot establish an unmodelled operand's file type")?
            {
                continue;
            }
            match crate::absence::is_kind(&path, true) {
                Some(true) => directories.push(root),
                Some(false) => {}
                None => return Err("cannot establish an unmodelled operand's file type"),
            }
        }
    }
    Ok(directories)
}

fn remote_endpoint(path: &str) -> bool {
    path.split_once(':').is_some_and(|(host, tail)| {
        !(host.contains(['/', '\\']) || (host.len() == 1 && tail.starts_with(['/', '\\'])))
    })
}

fn archive_syntax(flags: &[String]) -> bool {
    flags.iter().enumerate().any(|(at, arg)| {
        at == 0 && archive_verb(arg)
            || matches!(
                arg.as_str(),
                "--recursive" | "--archive" | "--create" | "--append"
            )
            || arg.strip_prefix('-').is_some_and(|flags| {
                !flags.starts_with('-')
                    && flags.chars().all(|ch| ch.is_ascii_alphabetic())
                    && flags.contains(['r', 'R', 'a', 'c', 'w', 'o', 'u'])
            })
    })
}

fn archive_verb(word: &str) -> bool {
    matches!(word, "a" | "u" | "c" | "create" | "archive")
}

/// Read option letters only up to a value-taking flag; a filename containing
/// r is not a recursive option. cp archive mode also implies recursion.
fn recursive_option(name: &str, args: &[String]) -> bool {
    args.iter().enumerate().any(|(at, arg)| {
        if arg == "--recursive"
            || arg == "--dereference-recursive"
            || name == "cp" && arg == "--archive"
        {
            return true;
        }
        if name == "grep"
            && (arg == "--directories=recurse"
                || matches!(arg.as_str(), "-d" | "--directories")
                    && args.get(at + 1).is_some_and(|value| value == "recurse"))
        {
            return true;
        }
        let Some(flags) = arg
            .strip_prefix('-')
            .filter(|flags| !flags.starts_with('-'))
        else {
            return false;
        };
        let valued = match name {
            "grep" => "efmABCDd",
            "cp" => "tS",
            "zip" => "Pnxs",
            _ => "",
        };
        for flag in flags.chars() {
            if matches!(flag, 'r' | 'R') || name == "cp" && flag == 'a' {
                return true;
            }
            if valued.contains(flag) {
                break;
            }
        }
        false
    })
}

/// Only inclusion options restrict the selected files. Exclusions and
/// preprocessing selectors must not turn an unrestricted read into a subset.
fn positive_selector(args: &[String], at: usize, value: &str) -> bool {
    if value.starts_with('!') {
        return false;
    }
    let Some(word) = args.get(at) else {
        return false;
    };
    let option = if word != value && word.ends_with(value) {
        word[..word.len() - value.len()].trim_end_matches('=')
    } else {
        at.checked_sub(1)
            .and_then(|previous| args.get(previous))
            .map_or("", String::as_str)
    };
    matches!(option, "--glob" | "--iglob" | "--include")
        || option.starts_with('-') && !option.starts_with("--") && option.ends_with('g')
}

/// A create archive reads whole input trees. Read its root and archive-file
/// options separately so the destination never becomes an input root.
fn archive_read_roots(args: &[String], cwd: &Path) -> Result<Option<Vec<String>>, &'static str> {
    let creates = args.iter().enumerate().any(|(at, arg)| {
        if arg == "--create" {
            return true;
        }
        if arg.starts_with("--") || at != 0 && !arg.starts_with('-') {
            return false;
        }
        for flag in arg.trim_start_matches('-').chars() {
            if flag == 'c' {
                return true;
            }
            if "fCTXIbLs".contains(flag) {
                break;
            }
        }
        false
    });
    if !creates {
        return Ok(None);
    }
    let mut base = cwd.to_path_buf();
    let mut roots = Vec::new();
    let mut at = 0;
    let mut literal = false;
    while let Some(arg) = args.get(at) {
        at += 1;
        if !literal && arg == "--" {
            literal = true;
            continue;
        }
        if !literal
            && matches!(
                arg.as_str(),
                "--directory" | "--file" | "--blocking-factor" | "--tape-length"
            )
        {
            let value = args
                .get(at)
                .ok_or("cannot establish an archive option value")?;
            at += 1;
            if arg == "--directory" {
                base = outward::lexical_join(&base, Path::new(value));
            }
        } else if !literal && arg.starts_with("--directory=") {
            base = outward::lexical_join(&base, Path::new(&arg[12..]));
        } else if !literal
            && (arg.starts_with("--file=")
                || matches!(
                    arg.as_str(),
                    "--create"
                        | "--gzip"
                        | "--bzip2"
                        | "--xz"
                        | "--verbose"
                        | "--dereference"
                        | "--one-file-system"
                        | "--preserve-permissions"
                ))
        {
        } else if !literal
            && (arg.starts_with('-') || at == 1 && arg.chars().all(|ch| ch.is_ascii_alphabetic()))
        {
            let flags = arg.trim_start_matches('-');
            for (offset, flag) in flags.char_indices() {
                if matches!(flag, 'f' | 'C' | 'b' | 'L' | 's') {
                    let attached = &flags[offset + 1..];
                    let value = if attached.is_empty() {
                        let value = args
                            .get(at)
                            .ok_or("cannot establish an archive option value")?;
                        at += 1;
                        value.as_str()
                    } else {
                        attached
                    };
                    if flag == 'C' {
                        base = outward::lexical_join(&base, Path::new(value));
                    }
                    break;
                }
                if !"czjJavhpmSo".contains(flag) {
                    return Err("cannot establish every archive input root");
                }
            }
        } else {
            roots.push(secret_path_text(&outward::lexical_join(
                &base,
                Path::new(arg),
            ))?);
        }
    }
    Ok(Some(roots))
}

fn search_root_matches(
    pattern: &str,
    root: &Path,
    home: Option<&Path>,
    links: &[super::composed::paths::Link],
    read: &mut usize,
) -> Result<bool, &'static str> {
    if pattern == "*" && secret_path_spelling(&secret_path_text(root)?, home, Some(false)) {
        return Ok(true);
    }
    let patterns = crate::hooks::git_guard::brace_expand(pattern)
        .ok_or("cannot inspect every search-glob brace alternative")?;
    let patterns: Vec<_> = patterns
        .iter()
        .filter(|pattern| !pattern.starts_with('!'))
        .map(|pattern| pattern.trim_start_matches('/').to_ascii_lowercase())
        .collect();
    let patterns = if patterns.is_empty() {
        vec!["*".into()]
    } else {
        patterns
    };
    // The guard cannot reproduce every ignore source or same-line edit.
    // Inspect ignored names too, under the same metadata budget.
    let root_path = crate::hooks::git_guard::prospective_path(root)
        .map_err(|_| "cannot resolve a recursive root")?;
    for link in links {
        let from = crate::hooks::git_guard::prospective_path(link.path())
            .map_err(|_| "cannot resolve a link declared before a recursive read")?;
        if from.starts_with(&root_path) || root_path.starts_with(&from) {
            return Err("a recursive root or descendant may traverse a link declared on this line");
        }
    }
    let paths = crate::hooks::git_guard::secret_search_paths(root, read, |_| Ok(false))?;
    for path in paths {
        if !real_secret_path(&path, home)? {
            continue;
        }
        let relative = path.strip_prefix(root).unwrap_or(&path);
        for pattern in &patterns {
            let candidate = if pattern.contains('/') {
                relative
            } else {
                Path::new(path.file_name().unwrap_or(std::ffi::OsStr::new("")))
            };
            let name = secret_path_text(candidate)?;
            let matcher = glob::Pattern::new(pattern)
                .map_err(|_| "cannot inspect the search glob's syntax")?;
            if matcher.matches_with(
                &name,
                glob::MatchOptions {
                    case_sensitive: false,
                    require_literal_separator: true,
                    require_literal_leading_dot: false,
                },
            ) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Pattern rules require actual text, never an escaped display or storage key.
fn secret_path_text(path: &Path) -> Result<String, &'static str> {
    let name = crate::git::GitName::from_os_str(path.as_os_str());
    let text = name
        .rule_text()
        .map_err(|_| "cannot inspect a non-UTF-8 glob path")?;
    Ok(if cfg!(windows) {
        text.replace('\\', "/")
    } else {
        text.to_owned()
    })
}

/// A program may accept a file operand even when it is not a known reader.
/// Only syntax known to be text is excluded. Unknown short-option clusters
/// retain every possible attached value; long options retain their = value.
fn file_operands<'a>(
    name: &str,
    args: &'a [String],
    expands: Option<&[bool]>,
    line: &CommandLine<'_>,
    cwd: &Path,
) -> Vec<(usize, &'a str, bool)> {
    if matches!(
        name,
        "echo"
            | "printf"
            | "print"
            // These operands are written or removed, not read for contents.
            // Input redirections are separate cat operands from outward::visit.
            | "tee"
            | "rm"
            | "rmdir"
            | "unlink"
            | "alias"
            | "unalias"
            | "export"
            | ":"
            | "true"
            | "false"
            | "cd"
            | "chdir"
            | "pushd"
            | "popd"
            | "declare"
            | "typeset"
            | "readonly"
            | "unset"
            | "getln"
            | "read"
            | "readarray"
            | "mapfile"
    ) {
        return Vec::new();
    }
    let mut git_start = 0;
    let mut git_root = None;
    let mut global_files = Vec::new();
    if name == "git" {
        if let Some((sub, tail)) =
            crate::hooks::git_guard::git_subcommand(args.get(1..).unwrap_or_default())
        {
            git_start = args.len() - tail.len() - 1;
            global_files = git_global_files(args, git_start);
            if matches!(
                sub,
                "status" | "log" | "rev-parse" | "branch" | "for-each-ref" | "ls-files"
            ) {
                global_files.extend(git_metadata_paths(tail, git_start));
                return global_files;
            }
            git_root = git_pathspec_root(sub, tail, &args[1..git_start], cwd);
        }
    }
    let rest = args.get(1..).unwrap_or_default();
    // Inline code is program text for the code-body checks, never a path.
    // On Windows, `use File::Path` would otherwise refuse as stream syntax.
    let code = outward::interpreter_family(name)
        .filter(|family| matches!(*family, "python" | "node" | "nodejs" | "perl" | "ruby"))
        .map(|family| outward::interpreter_code_positions(family, rest))
        .unwrap_or_default();
    let literal: Vec<_> = (1..args.len())
        .map(|at| expands.is_some_and(|flags| !flags.get(at).copied().unwrap_or(true)))
        .collect();
    let configured = line.names_ripgrep_config();
    if let Some(paths) = super::search_pattern::path_values(name, rest, &literal, configured) {
        return paths
            .into_iter()
            .map(|(at, value, glob)| (at + 1, value, glob))
            .collect();
    }
    // A parser that cannot read an option clears no argument as search text.
    let patterns = super::search_pattern::pattern_words(name, rest, &literal, configured);
    let mut paths: Vec<_> = rest
        .iter()
        .enumerate()
        .filter(|(at, word)| {
            !patterns.contains(at)
                && !code.contains(at)
                && *at >= git_start
                && !directory_option(name, rest, *at)
                && !git_root.as_deref().is_some_and(|root| {
                    Path::new(word).is_absolute()
                        && !Path::new(word).starts_with(root)
                        && crate::portable_path::canonicalize(Path::new(word))
                            .is_ok_and(|path| !path.starts_with(root))
                })
        })
        .flat_map(|(at, word)| {
            file_values(name, rest, at, word)
                .into_iter()
                .map(move |value| (at + 1, value, false))
        })
        .collect();
    paths.extend(global_files);
    paths
}

/// Transfer file syntax has its own filename quoting and field delimiters.
fn file_values<'a>(name: &str, args: &[String], at: usize, word: &'a str) -> Vec<&'a str> {
    let mut values = super::arguments::argument_values(word);
    let mut parsed_file = false;
    if name == "curl" {
        let files: Vec<_> = values
            .iter()
            .copied()
            .filter_map(|value| curl_file_value(args, at, value))
            .collect();
        if !files.is_empty() {
            values = files;
            parsed_file = true;
        }
    }
    if matches!(name, "curl" | "wget") && !parsed_file {
        values.extend(values.clone().into_iter().filter_map(|value| {
            value
                .strip_prefix(['@', '<'])
                .map(|file| file.split(';').next().unwrap_or(file))
        }));
    }
    values
}

fn curl_file_value<'a>(args: &[String], at: usize, value: &'a str) -> Option<&'a str> {
    let option = args.get(at).map(String::as_str).unwrap_or_default();
    let previous = at
        .checked_sub(1)
        .and_then(|at| args.get(at))
        .map(String::as_str);
    let urlencode = option.starts_with("--data-urlencode") || previous == Some("--data-urlencode");
    let form = option.starts_with("-F")
        || option.starts_with("--form=")
        || matches!(previous, Some("-F" | "--form"));
    if urlencode {
        return value.split_once('@').map(|(_, file)| file);
    }
    let file = if form {
        let value = value.split_once('=').map_or(value, |(_, value)| value);
        value.strip_prefix(['@', '<'])?
    } else {
        return None;
    };
    let (file, attributes) = if let Some(quote @ ('\'' | '"')) = file.chars().next() {
        let Some((file, tail)) = file[1..].split_once(quote) else {
            return Some("__unverified_transfer_input");
        };
        if file.contains('\\') {
            return Some("__unverified_transfer_input");
        }
        (file, tail)
    } else {
        file.split_once(';').unwrap_or((file, ""))
    };
    if file.contains(',')
        || attributes
            .split(';')
            .filter(|attribute| !attribute.is_empty())
            .any(|attribute| !attribute.starts_with("type=") && !attribute.starts_with("filename="))
    {
        return Some("__unverified_transfer_input");
    }
    Some(file)
}

fn git_global_files(args: &[String], end: usize) -> Vec<(usize, &str, bool)> {
    let mut files = Vec::new();
    let mut at = 1;
    while at < end {
        let word = &args[at];
        if matches!(word.as_str(), "-C" | "--work-tree") {
            at += 2;
            continue;
        }
        if word.starts_with("--work-tree=") || word.starts_with("-C") {
            at += 1;
            continue;
        }
        files.extend(
            super::arguments::argument_values(word)
                .into_iter()
                .map(|value| (at, value, false)),
        );
        at += 1;
    }
    files
}

fn git_metadata_paths(args: &[String], start: usize) -> Vec<(usize, &str, bool)> {
    args.iter()
        .enumerate()
        .filter_map(|(at, word)| {
            // Keep explicit named paths protected under AC-6. Namespace words
            // and unknown xargs streams remain metadata arguments.
            (word.contains('/') || word.starts_with('.')).then_some((
                start + at + 1,
                word.as_str(),
                false,
            ))
        })
        .collect()
}

/// One ordinary diff operand is a repository pathspec. Two paths can
/// implicitly enable --no-index and keep real-file judgments. Retargeted
/// or unresolved repositories earn no exemption.
fn git_pathspec_root(
    sub: &str,
    tail: &[String],
    globals: &[String],
    cwd: &Path,
) -> Option<std::path::PathBuf> {
    if sub != "diff"
        || tail.iter().any(|arg| arg == "--no-index")
        || tail.iter().filter(|arg| !arg.starts_with('-')).count() != 1
        || globals.iter().any(|arg| {
            arg == "-C"
                || arg.starts_with("-C")
                || arg.starts_with("--git-dir")
                || arg.starts_with("--work-tree")
        })
    {
        return None;
    }
    crate::hooks::repo::open(cwd)
        .ok()
        .flatten()
        .and_then(|repo| repo.workdir().map(Path::to_path_buf))
}

/// A destination directory is not a source-file operand. Writer protection
/// judges it separately; a reader's redirections remain explicit operands.
fn directory_option(name: &str, args: &[String], at: usize) -> bool {
    let (long, short) = match name {
        "curl" => ("--output-dir", ""),
        "wget" => ("--directory-prefix", "-P"),
        "tar" | "make" | "gmake" => ("--directory", "-C"),
        _ => return false,
    };
    args[at].starts_with(&format!("{long}="))
        || (!short.is_empty() && args[at].starts_with(short) && args[at].len() > short.len())
        || at
            .checked_sub(1)
            .and_then(|before| args.get(before))
            .is_some_and(|arg| arg == long || !short.is_empty() && arg == short)
}

/// Bounded shell readings shared by leaf and ancestor comparisons.
fn secret_pattern_readings(word: &str) -> Option<Vec<String>> {
    let mut readings = crate::hooks::git_guard::word_readings(word)?;
    // Keep the existing conservative singleton-brace reading too. Shell
    // options can give braces meanings beyond the default comma grammar.
    if word.contains('{') && !word.contains([',', '$']) {
        readings.push(word.replace(['{', '}'], ""));
    }
    Some(readings)
}

/// Compare complete glob languages rather than their literal prefixes. The
/// shared brace reader bounds alternatives; unknown pattern syntax refuses.
fn secret_prefix(word: &str, store: &str, expands: bool) -> bool {
    if !expands {
        return !store.starts_with("**/")
            && store
                .strip_prefix(word.trim_end_matches('/'))
                .is_some_and(|tail| tail.starts_with('/'));
    }
    let Some(readings) = secret_pattern_readings(word) else {
        return true;
    };
    readings.iter().any(|reading| {
        let intersects =
            |left: &str, right: &str| super::pattern::glob_intersects(left, right).unwrap_or(true);
        intersects(reading, store)
            || (!store.starts_with("**/")
                && (intersects(reading, &format!("{store}/**"))
                    || intersects(&format!("{}/**", reading.trim_end_matches('/')), store)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn inline_interpreter_code_keeps_its_role_before_path_mapping() {
        let cwd = Path::new("/fixture");
        for command in [
            "perl -e 'use File::Path qw(remove_tree); remove_tree(\"./\")' public.txt",
            "python3 -c 'print(\"File::Path\")' public.txt",
            "node -e 'console.log(\"File::Path\")' public.txt",
            "ruby -e 'puts \"File::Path\"' public.txt",
        ] {
            let args = crate::hooks::git_guard::command_argv(command);
            let name = super::super::unresolved::writer_name(&args[0]);
            let values: Vec<_> = file_operands(name, &args, None, &CommandLine::new(command), cwd)
                .into_iter()
                .map(|(_, value, _)| value)
                .collect();
            assert_eq!(values, ["public.txt"], "{command}");
        }
        // Only the code word leaves the operands: a directory an option
        // changes into, or a library it loads, stays a file operand.
        for (command, kept) in [
            ("ruby -C ~/.codex -e 'puts 1'", "~/.codex"),
            ("perl -x~/.codex -e 'print 1'", "~/.codex"),
            ("ruby -r ./lib.rb -e 'puts 1'", "./lib.rb"),
            ("node --eval 'x::y' --require ./hook.js", "./hook.js"),
        ] {
            let args = crate::hooks::git_guard::command_argv(command);
            let name = super::super::unresolved::writer_name(&args[0]);
            let values: Vec<_> = file_operands(name, &args, None, &CommandLine::new(command), cwd)
                .into_iter()
                .map(|(_, value, _)| value)
                .collect();
            assert!(values.contains(&kept), "{command}: {values:?}");
            assert!(
                !values.iter().any(|value| value.contains("puts")
                    || value.contains("print")
                    || value.contains("::")),
                "{command}: {values:?}"
            );
        }
    }

    /// TSK-1002 UNC finding: a share path that ends in a protected
    /// home-relative path refuses whatever the host or share, in every UNC
    /// spelling; a UNC home's ordinary files and a UNC project stay readable.
    #[test]
    fn unc_paths_ending_in_a_store_refuse_under_any_share() {
        let home = Some(Path::new("//server/homes/u"));
        for text in [
            "//localhost/c$/Users/u/.codex/auth.json",
            "//build-host/C$/Users/other/.codex/auth.json",
            "//server/homes/u/.codex/auth.json",
            "//server/u/.ssh/id_rsa",
            "//?/UNC/server/share/.codex/auth.json",
            "//./UNC/server/share/Users/u/.codex/auth.json",
            "//server/share/.ssh/id_ed25519",
        ] {
            assert!(unc_store_suffix(text, home, None), "{text}");
        }
        for text in [
            "//server/homes/u/notes.txt",
            "//server/share/project/src/main.rs",
            "//server/share/project/.codex-notes/todo.md",
            "C:/Users/u/.codex/auth.json",
            "//?/C:/Users/u/notes.txt",
            "//server",
        ] {
            assert!(!unc_store_suffix(text, home, None), "{text}");
        }
    }

    /// Hosted run 40623450157: a code literal with stream syntax is judged by
    /// the entry it opens, so `File::Path` is the name `File`, while a stream
    /// spelling of a store still names the store. Drives are kept.
    #[test]
    fn stream_spellings_name_the_entry_they_open() {
        for (word, base) in [
            ("File::Path", "File"),
            ("~/.codex/auth.json::$DATA", "~/.codex/auth.json"),
            (
                "C:/Users/u/.codex:$I30:$INDEX_ALLOCATION/auth.json",
                "C:/Users/u/.codex/auth.json",
            ),
            (
                "//?/C:/Users/u/.codex/auth.json:x",
                "//?/C:/Users/u/.codex/auth.json",
            ),
            ("notes.txt", "notes.txt"),
        ] {
            assert_eq!(stream_base(word), base, "{word}");
        }
        let home = Path::new("/fixture-home");
        assert!(secret_path_result(
            &stream_base("~/.codex/auth.json::$DATA"),
            Path::new("/w"),
            Some(home),
            None
        )
        .unwrap());
    }

    /// TSK-1002 Windows repair (hosted run 40602309629): a Git revision
    /// such as `HEAD~1` names no path, so the short-name mapper never sees
    /// it; a short-looking word that is a path, or could be, stays an
    /// operand.
    #[test]
    fn git_revisions_with_short_name_shapes_are_not_file_operands() {
        let temp = tempfile::tempdir().unwrap();
        let cwd = crate::portable_path::canonicalize(temp.path()).unwrap();
        std::fs::create_dir(cwd.join("sub")).unwrap();
        std::fs::write(cwd.join("sub/TAG~1"), "x").unwrap();
        let revision = |command: &str, value: &str| {
            let calls = outward::unwrapped_commands(command, &cwd, None);
            let call = calls
                .iter()
                .find(|call| call.args.iter().any(|arg| arg.contains(value)))
                .unwrap_or_else(|| panic!("{command}"));
            let at = call
                .args
                .iter()
                .position(|arg| arg.contains(value))
                .unwrap();
            git_revision_word(call, at, value, &cwd)
        };
        for command in [
            "git format-patch -o patches/ HEAD~1",
            "git rebase -x 'cargo test' HEAD~3",
            "git-rebase -x 'cargo test' HEAD~3",
            "git -c color.ui=never diff HEAD~1",
            "git -C sub log HEAD~2",
            "git --git-dir=.git --work-tree=sub rebase -x 'cargo test' HEAD~3",
            "git -c core.worktree=sub log HEAD~2",
        ] {
            let value = command.rsplit(' ').next().unwrap();
            assert!(revision(command, value), "{command}");
        }
        for (command, value) in [
            ("git diff -- HEAD~1", "HEAD~1"),
            ("git diff --no-index HEAD~1 other", "HEAD~1"),
            ("git -C sub log TAG~1", "TAG~1"),
            ("git --work-tree=sub log TAG~1", "TAG~1"),
            ("git -c core.worktree=sub log TAG~1", "TAG~1"),
            ("git --work-tree=\"$W\" log HEAD~1", "HEAD~1"),
            ("git -C \"$D\" log HEAD~1", "HEAD~1"),
            ("ln -s ~/.codex 'a b' && git log AB~1", "AB~1"),
            ("cat HEAD~1", "HEAD~1"),
            ("git log main", "main"),
        ] {
            assert!(!revision(command, value), "{command}");
        }
    }

    #[test]
    fn recursive_operands_that_cannot_be_names_are_not_directories() {
        let temp = tempfile::tempdir().unwrap();
        let unnameable = if cfg!(windows) {
            "not \"a path\"".to_string()
        } else {
            "x".repeat(4096)
        };
        assert_eq!(
            recursive_directories(vec![unnameable], temp.path(), None, &[]),
            Ok(Vec::new())
        );
    }

    #[test]
    fn r23_planned_public_link_refuses_at_the_recursive_selection() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("clean")).unwrap();
        for line in [
            "ln -s clean alias; rg x alias",
            "ln -s clean alias; grep -r x alias",
        ] {
            let findings = secret_reads(
                line,
                &SecuritySection::default(),
                temp.path(),
                Some(temp.path()),
            );
            assert!(
                findings
                    .iter()
                    .any(|finding| finding.message.contains("declared on this line")),
                "{line}: {findings:?}"
            );
        }
    }

    #[test]
    fn review3_writer_table_sources_are_walked_or_refused() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::create_dir(root.join("payload")).unwrap();
        std::fs::write(root.join("payload/id_rsa"), "fake private fixture").unwrap();
        for (name, _) in super::super::unresolved::writer_spellings("out", "dest") {
            let line = match name {
                "cp" | "scp" => format!("{name} -r payload out"),
                "rsync" => "rsync -a payload out".into(),
                "ditto" => "ditto payload out".into(),
                "tar" | "gtar" | "bsdtar" => format!("{name} cf out payload"),
                "zip" => "zip -r out.zip payload".into(),
                // Extraction's options and input cannot certify directory
                // sources here; retain the conservative recursive fallback.
                "unzip" => "unzip -r payload out".into(),
                // These named-source writers do not consume a directory's
                // contents. AC-6 still protects their explicit file operand.
                "ln" | "mv" | "install" => format!("{name} payload/id_rsa out"),
                // Downloads and patches are not recursive tree sources.
                "curl" | "wget" | "patch" => continue,
                _ => panic!("classify {name}'s source role in this test"),
            };
            let findings = secret_reads(&line, &SecuritySection::default(), root, Some(root));
            assert!(
                findings.iter().any(|v| v.rule == "security.secret_reads"),
                "{line}: {findings:?}"
            );
        }
    }

    #[test]
    fn review3_unread_executable_inputs_keep_the_secret_read_boundary() {
        let temp = tempfile::tempdir().unwrap();
        for line in [
            "pwsh -e RwBlAHQA",
            "pwsh -unknown value",
            "deno eval --unknown value 'console.log(1)'",
            "osascript -e 'do shell script commandFromElsewhere'",
            "cat <<'E' > x.py; python3 x.py\nprint(1)\nE",
        ] {
            let findings = evaluate(
                line,
                &SecuritySection::default(),
                PolicyLevel::Off,
                temp.path(),
                temp.path(),
                Some(temp.path()),
            );
            assert!(
                findings.iter().any(|v| v.rule == "security.secret_reads"),
                "{line}: {findings:?}"
            );
        }
    }

    #[test]
    fn tsk1002_ci1_and_or_paths_preserve_reader_verdicts() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path();
        std::fs::create_dir(root.join("build")).unwrap();
        std::fs::write(root.join("public.txt"), "ordinary fixture").unwrap();
        for command in [
            "false || D=build && find $D -delete",
            "false || D=public.txt && cat $D",
            "false && D=~/.codex/auth.json || D=public.txt; cat $D",
            "D=public.txt; true || D=~/.codex/auth.json; cat $D",
        ] {
            let findings = secret_reads(command, &SecuritySection::default(), root, Some(root));
            assert!(findings.is_empty(), "{command}: {findings:?}");
        }
        for command in [
            "false || D=~/.codex/auth.json && cat $D",
            "D=~/.codex/auth.json; true || D=public.txt && cat $D",
            "D=~/.codex/auth.json; unknown || D=public.txt && cat $D",
            "D=~/.codex/auth.json; alias false=true; false || D=public.txt && cat $D",
            "D=~/.codex/auth.json; false() { true; }; false || D=public.txt && cat $D",
            "false || find build -files0-from ~/.codex/auth.json -delete",
            "false || find build -exec cat ~/.codex/auth.json \\;",
        ] {
            let findings = secret_reads(command, &SecuritySection::default(), root, Some(root));
            assert!(
                findings.iter().any(|v| v.rule == "security.secret_reads"),
                "input still reads: {command}: {findings:?}"
            );
        }
    }

    /// Every call on a line shares one read of whether the line names
    /// `RIPGREP_CONFIG_PATH`. The read expands the whole line, so a read
    /// per call made a long chain quadratic; the verdicts stay the same.
    #[test]
    fn a_long_chain_reads_the_ripgrep_config_name_once() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path();
        std::fs::create_dir(root.join("docs")).unwrap();
        std::fs::write(root.join("public.txt"), "ordinary fixture").unwrap();
        let reads = |command: &str| {
            super::super::search_pattern::RIPGREP_CONFIG_READS.with(|reads| reads.set(0));
            let findings = secret_reads(command, &SecuritySection::default(), root, Some(root));
            assert!(findings.is_empty(), "{command}: {findings:?}");
            super::super::search_pattern::RIPGREP_CONFIG_READS.with(std::cell::Cell::get)
        };
        let one = "rg needle docs && cat public.txt";
        assert_eq!(reads(one), 1, "{one}");
        let long = vec![one; 40].join(" && ");
        assert_eq!(reads(&long), 1, "reads grew with the segment count");
        let findings = secret_reads(
            &format!("{long} && cat ~/.ssh/id_rsa"),
            &SecuritySection::default(),
            root,
            Some(root),
        );
        assert!(
            findings.iter().any(|v| v.rule == "security.secret_reads"),
            "a store read at the end of the chain still refuses: {findings:?}"
        );
    }

    #[test]
    fn tsk1002_ci1_removal_operands_are_not_content_reads() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path();
        for command in [
            "trap 'D=/' DEBUG; trap - DEBUG; D=build; rm -rf \"$D\"",
            "ls ./build | xargs rm -rf",
            "rm -rf ~",
            "grm -f ~/.codex/auth.json",
            "unlink ~/.codex/auth.json",
            "rmdir ~/.ssh",
        ] {
            let findings = secret_reads(command, &SecuritySection::default(), root, Some(root));
            assert!(findings.is_empty(), "{command}: {findings:?}");
        }
        for command in ["cat ~/.ssh/id_rsa", "rm build < ~/.ssh/id_rsa"] {
            let findings = secret_reads(command, &SecuritySection::default(), root, Some(root));
            assert!(
                findings.iter().any(|v| v.rule == "security.secret_reads"),
                "input still reads: {command}: {findings:?}"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn tsk1002_ci1_rooted_home_globs_use_the_callers_drive() {
        let fixture = tempfile::tempdir().unwrap();
        let rooted: PathBuf = fixture
            .path()
            .join("home")
            .components()
            .filter(|part| !matches!(part, std::path::Component::Prefix(_)))
            .collect();
        let forward = crate::portable_path::slashed(&rooted);
        for home in [rooted.as_path(), Path::new(&forward)] {
            let findings = evaluate(
                "cat ~/.codex/auth.*",
                &SecuritySection::default(),
                PolicyLevel::Block,
                fixture.path(),
                fixture.path(),
                Some(home),
            );
            assert!(
                findings.iter().any(|v| v.rule == "security.secret_reads"),
                "{home:?}: {findings:?}"
            );
            let ordinary = evaluate(
                "cat ~/.codex/*.rs",
                &SecuritySection::default(),
                PolicyLevel::Block,
                fixture.path(),
                fixture.path(),
                Some(home),
            );
            assert!(
                !ordinary.iter().any(|v| v.rule == "security.secret_reads"),
                "ordinary sibling: {home:?}: {ordinary:?}"
            );
        }
    }
    fn check(code: &str, levels: &SecuritySection) -> Vec<Violation> {
        evaluate(
            code,
            levels,
            PolicyLevel::Block,
            Path::new("/fixture"),
            Path::new("/fixture"),
            Some(Path::new("/fixture-home")),
        )
    }

    fn glob_fixture_read(command: &str, root: &Path, home: &Path) -> bool {
        evaluate(
            command,
            &SecuritySection::default(),
            PolicyLevel::Block,
            root,
            root,
            Some(home),
        )
        .iter()
        .any(|v| v.rule == "security.secret_reads")
    }

    #[test]
    fn tsk1002_review2_b02_home_ancestor_spellings_agree() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repo");
        let home = temp.path().join("home");
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir_all(home.join(".codex")).unwrap();
        std::fs::write(home.join(".codex/auth.json"), "fake secret fixture").unwrap();
        let wrong: Vec<_> = ["grep -r token ~", "rg token ~/", "cp -r ~ out"]
            .into_iter()
            .filter(|command| !glob_fixture_read(command, &root, &home))
            .collect();
        assert!(
            wrong.is_empty(),
            "home ancestor reads were allowed: {wrong:?}"
        );
        for command in ["cat ~", "cat ~/", "cat \"$HOME/\""] {
            assert!(
                !glob_fixture_read(command, &root, &home),
                "a flat directory operand does not read descendants: {command}"
            );
        }
    }

    #[test]
    fn tsk1002_review2_b05_file_rule_siblings_remain_available() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repo");
        let home = temp.path().join("home");
        std::fs::create_dir(&root).unwrap();
        for directory in [".claude/skills/fixture", ".claude/sessions", ".cargo/bin"] {
            std::fs::create_dir_all(home.join(directory)).unwrap();
        }
        for file in [
            ".claude/skills/fixture/SKILL.md",
            ".claude/sessions/fixture.jsonl",
            ".cargo/bin/cargo-fixture",
        ] {
            std::fs::write(home.join(file), "ordinary or fake secret fixture").unwrap();
        }
        let cases = [
            ("cat ~/.claude/skills/*/SKILL.md", false),
            ("ls ~/.cargo/bin/*", false),
            ("cat ~/.claude/sessions/*", true),
        ];
        let wrong: Vec<_> = cases
            .into_iter()
            .filter(|(command, refuse)| glob_fixture_read(command, &root, &home) != *refuse)
            .collect();
        assert!(
            wrong.is_empty(),
            "file and directory rules differ: {wrong:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn tsk1002_review2_b08_unreadable_recursive_root_refuses_with_remedy() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let locked = root.join("locked");
        std::fs::create_dir(&locked).unwrap();
        let saved = std::fs::metadata(&locked).unwrap().permissions();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        let unreadable = std::fs::read_dir(&locked).is_err();
        let violations = evaluate(
            "rg token locked",
            &SecuritySection::default(),
            PolicyLevel::Block,
            root,
            root,
            Some(root),
        );
        std::fs::set_permissions(&locked, saved).unwrap();
        if !unreadable {
            eprintln!("permissions do not make this directory unreadable on this host");
            return;
        }
        assert!(
            violations.iter().any(|v| v.rule == "security.secret_reads"
                && v.remedy.to_ascii_lowercase().contains("narrow")),
            "recursive root inspection must fail closed with a narrowing remedy: {violations:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn tsk1002_review_f09_unverified_search_names_narrowing() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let locked = root.join("locked");
        std::fs::create_dir(&locked).unwrap();
        let saved = std::fs::metadata(&locked).unwrap().permissions();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        let unreadable = std::fs::read_dir(&locked).is_err();
        let violations = evaluate(
            "rg -g 'id_*' token locked",
            &SecuritySection::default(),
            PolicyLevel::Block,
            root,
            root,
            Some(root),
        );
        std::fs::set_permissions(&locked, saved).unwrap();
        if !unreadable {
            eprintln!("permissions do not make this directory unreadable on this host");
            return;
        }
        assert!(
            violations.iter().any(|v| v.rule == "security.secret_reads"
                && v.remedy.to_ascii_lowercase().contains("narrow")),
            "unverified search must name the narrowing remedy: {violations:?}"
        );
    }

    #[test]
    fn tsk1002_glob_expansion_uses_actual_matches() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::create_dir(root.join("src")).unwrap();
        std::fs::write(root.join("src/public.rs"), "fixture").unwrap();
        for pattern in ["src/*.rs", "src/[ip]*.rs", "src/*.{rs,md}"] {
            assert!(
                !glob_fixture_read(&format!("cat {pattern}"), root, root),
                "ordinary {pattern}"
            );
        }
        std::fs::write(root.join("src/id_rsa.rs"), "planted fixture").unwrap();
        for pattern in ["src/*.rs", "src/[ip]*.rs", "src/*.{rs,md}"] {
            assert!(
                glob_fixture_read(&format!("cat {pattern}"), root, root),
                "secret {pattern}"
            );
        }
    }

    #[test]
    fn tsk1002_glob_store_directory_is_protected_when_empty() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repo");
        let home = temp.path().join("home");
        std::fs::create_dir(&root).unwrap();
        for store in [".ssh", ".aws", ".gnupg"] {
            std::fs::create_dir_all(home.join(store)).unwrap();
            let command = format!("cat $HOME/{store}/*.rs");
            assert!(
                glob_fixture_read(&command, &root, &home),
                "empty store {store}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn tsk1002_glob_unreadable_secret_pattern_fails_closed() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let locked = root.join("locked");
        std::fs::create_dir(&locked).unwrap();
        let saved = std::fs::metadata(&locked).unwrap().permissions();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        let unreadable = std::fs::read_dir(&locked).is_err();
        let refused = glob_fixture_read("cat locked/[i]d_rsa*", root, root);
        std::fs::set_permissions(&locked, saved).unwrap();
        if !unreadable {
            eprintln!("permissions do not make this directory unreadable on this host");
            return;
        }
        assert!(refused, "unreadable intersecting glob must refuse");
    }

    #[test]
    fn tsk1002_glob_search_options_use_search_roots() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        for dir in ["src", "other"] {
            std::fs::create_dir(root.join(dir)).unwrap();
            std::fs::write(root.join(dir).join("public.rs"), "fixture").unwrap();
        }
        let forms = [
            "rg -g '*.rs' token",
            "rg --glob='*.rs' token",
            "rg --iglob '*.RS' token",
            "grep -r --include='*.rs' token",
        ];
        for form in forms {
            assert!(
                !glob_fixture_read(&format!("{form} src"), root, root),
                "ordinary {form}"
            );
        }
        std::fs::write(root.join("src/id_rsa.rs"), "planted fixture").unwrap();
        for form in forms {
            assert!(
                glob_fixture_read(&format!("{form} src"), root, root),
                "secret {form}"
            );
            assert!(
                !glob_fixture_read(&format!("{form} other"), root, root),
                "other root {form}"
            );
        }
    }

    #[test]
    fn tsk1002_secret_operands_globs_and_redirections() {
        let fixture = tempfile::tempdir().unwrap();
        for name in ["id_rsa", "fixture.pem", "fixture.key"] {
            std::fs::write(fixture.path().join(name), "planted fixture").unwrap();
        }
        assert_secret_commands(
            &[
                "D=$HOME/.codex/auth.json; wc -c \"$D\"",
                "D=~/.ssh; od \"$D/id_ed25519\"",
                "git diff --no-index /dev/null $HOME/.codex/auth.json",
                "xxd id_rsa",
                "tar cf - ~/.ssh",
                "zip -r out.zip ~/.aws",
                "python3 -c 'print(1)' id_rsa",
                "rg -g 'id_*' token .",
                "rg -ug'id_*' token .",
                "grep -nf$HOME/.codex/auth.json x",
                "grep -r --include='id_*' token .",
                "rg --iglob='*.PEM' token .",
                "rg --glob='*.{pem,txt}' token .",
                "cat {id_rsa,README.md}",
                "cat [i]d_rsa",
                "cat *.{pem,key}",
                "cat ~/{.codex/auth.json,notes}",
                "cat ~/.ss[h]/config",
                "cat<~/.codex/auth.json",
                "exec {fd}<~/.codex/auth.json; cat <&$fd",
                "exec 3<~/.codex/auth.json; cat <&3",
                "cat 0<~/.netrc",
                "while read l; do echo \"$l\"; done<~/.netrc",
                "base64 --input=$HOME/.codex/auth.json",
                "grep -f$HOME/.codex/auth.json x",
                "rg --file=$HOME/.codex/auth.json x",
                "sort --files0-from=id_rsa",
                "cat 'dir\nname/id_rsa'",
            ],
            fixture.path(),
        );
    }

    #[test]
    fn tsk1002_text_and_literal_paths_remain_data() {
        for line in [
            "rg 'id_rsa' README.md",
            "grep -n id_rsa docs/x.md",
            "rg -e id_rsa README.md",
            "rg -m 1 id_rsa README.md",
            "grep -neid_rsa README.md",
            r"rg -n 'Read\(\*\*/id_rsa|id_rsa' assets/base/security-actions.json crates/codeflow-core/src/security/actions* | head -10",
            "grep --regexp=id_rsa README.md",
            "rg -g '*.rs' foo .",
            "cat src/*.rs",
            "cat '~somebody/.codex/auth.json'",
            "cat < 'id_rs?'",
            "cat > notes.txt <<'EOF'\npython3 tool.py .claude/settings.json\nEOF",
        ] {
            assert!(
                check(line, &SecuritySection::default()).is_empty(),
                "{line}"
            );
        }
    }

    #[test]
    fn hard_secret_filename_spellings_receive_the_same_verdict() {
        for name in [
            "id_rsa",
            "id_rsa.pub",
            "id_ed25519",
            "id_ed25519.pub",
            "certificate.pem",
            "private.key",
            "bundle.p12",
            "bundle.pfx",
            ".netrc",
        ] {
            for prefix in ["", "./", "nested/", "/fixture/", "~/", "$HOME/", "${HOME}/"] {
                let path = format!("{prefix}{name}");
                for command in [
                    format!("cat {path}"),
                    format!("python3 -c 'open(\"{path}\")'"),
                ] {
                    assert!(
                        check(&command, &SecuritySection::default())
                            .iter()
                            .any(|v| v.rule == "security.secret_reads"),
                        "{command}"
                    );
                }
            }
        }
    }

    #[test]
    fn configured_secret_stores_match_relative_to_the_session_directory() {
        let table = actions::table();
        let stores = table.sandbox_read_denies.iter().cloned().chain(
            table
                .claude_read_groups
                .iter()
                .filter(|group| group.carveouts.is_empty())
                .flat_map(|group| &group.denies)
                .filter_map(|rule| {
                    rule.strip_prefix("Read(~/")
                        .and_then(|rule| rule.strip_suffix(')'))
                })
                .map(|path| {
                    format!(
                        "~/{}",
                        path.replace("**/", "nested/")
                            .replace("**", "entry")
                            .replace('*', "fixture")
                    )
                }),
        );
        for store in stores {
            let suffix = store.strip_prefix("~/").unwrap();
            let home = Path::new("/fixture-home");
            let absolute = home.join(suffix);
            let parent = absolute.parent().unwrap();
            let basename = absolute.file_name().unwrap().to_str().unwrap();
            for (cwd, spelling) in [
                (parent, basename.to_string()),
                (parent, format!("./{basename}")),
                (home, suffix.to_string()),
                (home, crate::portable_path::slashed(&absolute)),
                (home, store.clone()),
                (home, format!("$HOME/{suffix}")),
                (home, format!("${{HOME}}/{suffix}")),
            ] {
                let findings = evaluate(
                    &format!("cat {spelling}"),
                    &SecuritySection::default(),
                    PolicyLevel::Block,
                    cwd,
                    Path::new("/fixture"),
                    Some(home),
                );
                assert!(
                    findings.iter().any(|v| v.rule == "security.secret_reads"),
                    "{cwd:?}: {spelling}"
                );
            }
        }
    }

    #[test]
    fn ordinary_names_and_carveout_groups_keep_their_shell_verdict() {
        for name in [
            "README.md",
            "auth.json",
            "./auth.json",
            "id_ordinary",
            "identity_rsa",
            "certificate.pem.txt",
            "private.key.rs",
            ".netrc-notes",
            "~/.ssh-notes/file",
            "~/.codex/auth.json.old",
            "~/.password-store-notes/item",
            ".env",
            ".env.production",
            ".env.example",
            ".env.sample",
            ".env.template",
            ".env.dist",
            "secret.txt",
            "credentials.json",
            "secret_scan.rs",
            "credentials.py",
        ] {
            assert!(
                !check(&format!("cat {name}"), &SecuritySection::default())
                    .iter()
                    .any(|v| v.rule == "security.secret_reads"),
                "{name}"
            );
        }
    }

    fn assert_secret_commands(commands: &[&str], cwd: &Path) {
        let fixture = tempfile::tempdir().unwrap();
        let home = fixture.path().join("home");
        let root = fixture.path().join("repo");
        let cwd = if let Ok(tail) = cwd.strip_prefix("/fixture-home") {
            home.join(tail)
        } else if cwd.exists() {
            cwd.to_path_buf()
        } else {
            root.clone()
        };
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::create_dir_all(home.join(".codex")).unwrap();
        std::fs::write(home.join(".pypirc"), "planted fixture").unwrap();
        std::fs::write(home.join(".codex/auth.json"), "planted fixture").unwrap();
        for name in ["id_rsa", "fixture.pem", "fixture.key"] {
            std::fs::write(cwd.join(name), "planted fixture").unwrap();
        }
        for command in commands {
            // Keep the literal absolute-cd case pointed at this fixture's
            // HOME after replacing the old fixed /fixture-home directory.
            let command = command.replace(
                "cd /fixture-home",
                &format!(
                    "cd '{}'",
                    crate::portable_path::slashed(&home).replace('\'', "'\\''")
                ),
            );
            assert!(
                evaluate(
                    &command,
                    &SecuritySection::default(),
                    PolicyLevel::Block,
                    &cwd,
                    &root,
                    Some(&home)
                )
                .iter()
                .any(|v| v.rule == "security.secret_reads"),
                "{command}"
            );
        }
    }

    #[test]
    fn review_f1_home_aliases_cannot_hide_stores() {
        assert_secret_commands(
            &[
                "cat ~//.codex/auth.json",
                "cat $HOME//.codex/auth.json",
                "cat ${HOME}///.codex/auth.json",
                "cat ~someone/.codex/auth.json",
                "cat ~$USER/.codex/auth.json",
                "cat ~someone/.aws/credentials",
            ],
            Path::new("/fixture"),
        );
    }

    #[test]
    fn review_f2_secret_spelling_is_ascii_case_insensitive() {
        assert_secret_commands(
            &[
                "cat ID_RSA",
                "cat ~/.Codex/auth.json",
                "cat ~/.codex/AUTH.JSON",
                "cat ~/.AWS/credentials",
                "cat .PYPIRC",
            ],
            Path::new("/fixture-home"),
        );
        for store in &actions::table().sandbox_read_denies {
            assert_secret_commands(
                &[&format!("cat {}", store.to_ascii_uppercase())],
                Path::new("/fixture"),
            );
        }
    }

    #[test]
    fn review_f3_patterns_directories_and_input_redirects_reach_stores() {
        assert_secret_commands(
            &[
                "cat ~/.codex/auth.*",
                "cat ~/.codex/auth.jso?",
                "cat ~/.pypir?",
                "cat id_rs?",
                "cat ~/.codex/auth.{json}",
                "cat ~/.codex/*",
                "grep -r token ~/.codex",
                "cat ~/.ssh",
                "cat < ~/.ssh/id_rsa",
                "sh -c 'cat < ~/.codex/auth.json'",
                "wc 0<~/.codex/auth.json",
                "cat auth.*",
                "cat auth.jso?",
                "cat auth.{json}",
            ],
            Path::new("/fixture-home/.codex"),
        );
    }

    #[test]
    fn review_f4_reader_paths_follow_directory_changes() {
        assert_secret_commands(
            &[
                "cd ~/.codex && cat auth.json",
                "cd ~/.gem && cat credentials",
                "sh -c 'cd ~/.codex && cat auth.json'",
                "cd /fixture-home && cd .codex && cat auth.json",
                "cd $HOME/.codex && cat < auth.json",
            ],
            Path::new("/fixture"),
        );
    }

    #[test]
    fn review_controls_keep_patterns_and_redirect_data_readable() {
        for command in [
            "cat src/*.rs",
            "cat *.md",
            "cat src/{a,b}.rs",
            "cat ~/.codex-notes/*",
            "cat ~/.codex/auth.json.old",
            "cat < README.md",
            "echo '< ~/.codex/auth.json'",
            "cat <<'~/.codex/auth.json'",
            "cat <<< '~/.codex/auth.json'",
            "cd src && cat auth.json",
            "cat ID_ORDINARY",
        ] {
            assert!(
                !check(command, &SecuritySection::default())
                    .iter()
                    .any(|v| v.rule == "security.secret_reads"),
                "{command}"
            );
        }
    }

    #[test]
    fn a_quoted_wildcard_is_text_and_an_unquoted_one_expands() {
        let fixture = tempfile::tempdir().unwrap();
        std::fs::create_dir(fixture.path().join("etc")).unwrap();
        std::fs::write(fixture.path().join("id_rsa"), "planted fixture").unwrap();
        std::fs::write(fixture.path().join("etc/.netrc"), "planted fixture").unwrap();
        let reads = |command: &str| {
            evaluate(
                command,
                &SecuritySection::default(),
                PolicyLevel::Block,
                fixture.path(),
                fixture.path(),
                Some(Path::new("/fixture-home")),
            )
            .iter()
            .any(|v| v.rule == "security.secret_reads")
        };
        for command in [
            "rg '^/etc/.*' docs",
            "grep -e '^/etc/.*' docs",
            "echo docs/a.md | xargs rg '^/etc/.*'",
            "cat 'id_rs?'",
            "cat \"~/.codex/auth.*\"",
        ] {
            assert!(!reads(command), "{command}");
        }
        for command in [
            "cat etc/.*",
            // F4: search text stays data, but an external filename stream
            // cannot certify the files the child will open.
            "rg --files | xargs rg '^/etc/.*'",
            "cat id_rs?",
            "cat ~/.codex/auth.*",
            "rg x ~/.codex/*",
        ] {
            assert!(reads(command), "{command}");
        }
    }

    #[test]
    fn reader_prefixes_do_not_turn_code_fragments_into_store_reads() {
        for command in ["cat *.md", "cat src/*.rs", r#"python3 -c 'print("/")'"#] {
            assert!(
                !evaluate(
                    command,
                    &SecuritySection::default(),
                    PolicyLevel::Block,
                    Path::new("/fixture-home"),
                    Path::new("/fixture"),
                    Some(Path::new("/fixture-home"))
                )
                .iter()
                .any(|v| v.rule == "security.secret_reads"),
                "{command}"
            );
        }
    }

    #[test]
    fn f15_home_variable_secret_spellings() {
        for command in [
            r"cat $HOME/.codex/auth.json",
            r"cat ${HOME}/.codex/auth.json",
            r#"python3 -c 'open("$HOME/.codex/auth.json")'"#,
            r#"python3 -c 'open("${HOME}/.codex/auth.json")'"#,
        ] {
            assert!(
                check(command, &SecuritySection::default())
                    .iter()
                    .any(|v| v.rule == "security.secret_reads"),
                "{command}"
            );
        }
        assert!(check("cat $HOME/notes.md", &SecuritySection::default()).is_empty());
    }

    #[test]
    fn interpreter_headless_paths_and_secret_pairs() {
        for command in [
            "python3 -c 'run(\"claude\", \"-p\", \"x\")'",
            "node -e 'exec(\"codex exec x\")'",
            "ruby -e 'system(\"grok -p x\")'",
            "perl -e 'system(\"claude -p x\")'",
            "python3 <<'PY'\nrun('codex exec x')\nPY",
        ] {
            let findings = check(command, &SecuritySection::default());
            assert!(
                findings
                    .iter()
                    .any(|v| v.rule == "security.headless_peer_runs"),
                "{command}: {findings:?}"
            );
            let off = SecuritySection {
                headless_peer_runs: PolicyLevel::Off,
                ..SecuritySection::default()
            };
            assert!(check(command, &off).is_empty(), "{command}");
        }
        for store in &actions::table().sandbox_read_denies {
            let command = format!("python3 -c 'open(\"{store}\")'");
            assert!(
                check(&command, &SecuritySection::default())
                    .iter()
                    .any(|v| v.rule == "security.secret_reads"),
                "{command}"
            );
        }
        for command in [
            "cat ~/.codex/auth.json",
            "nohup cat ~/.grok/auth.json",
            "bash -c 'cat ~/.pypirc'",
        ] {
            assert!(
                check(command, &SecuritySection::default())
                    .iter()
                    .any(|v| v.rule == "security.secret_reads"),
                "{command}"
            );
        }
        assert!(check(
            "node -e 'write(\".codeflow/policy.json\")'",
            &SecuritySection::default()
        )
        .iter()
        .any(|v| v.rule == "git.hook_integrity"));
        for command in [
            "cat README.md",
            "cat .env.example",
            "rg 'claude -p' assets/",
            "python3 -c 'print(\"hello\")'",
            "cat <<'EOF'\nclaude -p x\nEOF",
        ] {
            assert!(
                check(command, &SecuritySection::default()).is_empty(),
                "{command}"
            );
        }
    }

    /// A repository with enforcement files, a linked worktree at
    /// `.worktrees/w` and a home, at canonical paths.
    struct Repo {
        _temp: tempfile::TempDir,
        root: PathBuf,
        linked: PathBuf,
        home: PathBuf,
    }

    fn repo() -> Repo {
        let temp = tempfile::tempdir().unwrap();
        let base = std::fs::canonicalize(temp.path()).unwrap();
        let root = base.join("repo");
        let home = base.join("home");
        for dir in [".claude", ".codeflow", "src"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(root.join(".claude/settings.json"), "{}\n").unwrap();
        std::fs::write(root.join(".codeflow/policy.json"), "{}\n").unwrap();
        std::fs::write(root.join("src/app.py"), "\n").unwrap();
        let git = git2::Repository::init(&root).unwrap();
        // A linked worktree registered the way `git worktree add` does.
        let linked = root.join(".worktrees/w");
        let admin = git.path().join("worktrees/w");
        std::fs::create_dir_all(&admin).unwrap();
        std::fs::create_dir_all(linked.join(".claude")).unwrap();
        std::fs::write(
            admin.join("gitdir"),
            format!("{}\n", linked.join(".git").display()),
        )
        .unwrap();
        std::fs::write(admin.join("commondir"), "../..\n").unwrap();
        std::fs::write(admin.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::write(
            linked.join(".git"),
            format!("gitdir: {}\n", admin.display()),
        )
        .unwrap();
        std::fs::write(linked.join(".claude/settings.json"), "{}\n").unwrap();
        // A scaffolded hook directory, which `find -execdir` can run in.
        std::fs::create_dir_all(root.join(".codeflow/git-hooks")).unwrap();
        std::fs::write(root.join(".codeflow/git-hooks/pre-commit"), "#!/bin/sh\n").unwrap();
        std::fs::write(root.join("src/notes.txt"), "\n").unwrap();
        Repo {
            _temp: temp,
            root,
            linked,
            home,
        }
    }

    /// The findings for `command` run from `dir` in the checkout `root`.
    fn judge_in(
        repo: &Repo,
        dir: &Path,
        root: &Path,
        command: &str,
        integrity: PolicyLevel,
    ) -> Vec<Violation> {
        // These placeholders occur unquoted, inside shell quotes and inside
        // code literals. Use slash paths so shell escape removal cannot
        // change their target. The Windows matrix below quotes native forms.
        let shell_path = |path: &Path| {
            crate::portable_path::slashed(&crate::portable_path::without_verbatim(
                path.to_path_buf(),
            ))
        };
        let command = command
            .replace("@ROOT@", &shell_path(&repo.root))
            .replace("@LINKED@", &shell_path(&repo.linked));
        evaluate(
            &command,
            &SecuritySection::default(),
            integrity,
            dir,
            root,
            Some(&repo.home),
        )
    }

    fn integrity_refusal(findings: &[Violation]) -> Option<&Violation> {
        findings.iter().find(|v| v.rule == "git.hook_integrity")
    }

    fn argument_refusal(findings: &[Violation]) -> bool {
        integrity_refusal(findings).is_some_and(|v| {
            v.message.contains("interpreter") && !v.message.contains("interpreter code")
        })
    }

    /// Every command refuses under `git.hook_integrity` from `dir`, each
    /// with a finding about an interpreter argument, not its code.
    fn assert_refused(repo: &Repo, dir: &Path, root: &Path, commands: &[&str]) {
        let mut wrong = Vec::new();
        for command in commands {
            let findings = judge_in(repo, dir, root, command, PolicyLevel::Block);
            if !argument_refusal(&findings) {
                wrong.push(format!("should refuse: {command}: {findings:?}"));
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    fn assert_allowed(repo: &Repo, commands: &[&str]) {
        let mut wrong = Vec::new();
        for command in commands {
            let findings = judge_in(repo, &repo.root, &repo.root, command, PolicyLevel::Block);
            if !findings.is_empty() {
                wrong.push(format!("should allow: {command}: {findings:?}"));
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    /// TSK-302 AC-1 (issue 173): an argument naming an enforcement path
    /// refuses for every interpreter, after inline code, after stdin `-`,
    /// as a script's argument and in a linked worktree. The heredoc code
    /// here names no path, so the argument alone decides.
    #[test]
    fn interpreter_arguments_naming_an_enforcement_path_refuse() {
        let repo = repo();
        assert_refused(
            &repo,
            &repo.root,
            &repo.root,
            &[
                "python3 -I -B - \"@ROOT@/.claude/settings.json\" <<'PY'\nimport sys\nopen(sys.argv[1], 'a')\nPY",
                "python3 - \"@ROOT@/.codeflow/policy.json\" <<'PY'\nimport sys\nPY",
                "python3 edit.py \"@ROOT@/.claude/settings.json\"",
                "python3 edit.py .codeflow/policy.json",
                "python edit.py .claude/settings.json",
                "python3.13 edit.py .claude/settings.json",
                "python3 -c 'import sys; open(sys.argv[1], \"a\")' .claude/settings.json",
                "python3 -c 'import sys' -- .claude/settings.json",
                "node -e 'require(\"fs\").appendFileSync(process.argv[1], \"\")' .claude/settings.json",
                "node --eval='x' .codeflow/policy.json",
                "nodejs script.js .codeflow/policy.json",
                "perl -i -pe 's/a/b/' .claude/settings.json",
                "perl -e 'print' .codeflow/policy.json",
                "ruby -e 'File.write(ARGV[0], \"\")' .codeflow/policy.json",
                "ruby -i -pe 'x' .claude/settings.json",
                "python3 tool.py --out=.codeflow/policy.json",
                "python3 edit.py ~/.claude/settings.json",
                "python3 edit.py .claude/*.json",
                "python3 edit.py .codeflow/{policy,other}.json",
                "python3 edit.py \"@LINKED@/.claude/settings.json\"",
            ],
        );
        // From inside the linked worktree, its own files and the main
        // checkout's are enforcement paths.
        assert_refused(
            &repo,
            &repo.linked,
            &repo.linked,
            &[
                "python3 edit.py .claude/settings.json",
                "python3 edit.py \"@ROOT@/.claude/settings.json\"",
                "python3 edit.py ../../.codeflow/policy.json",
            ],
        );
    }

    /// Every resolved reading uses the writer's directory and checkout
    /// protection, including literal and brace-expanded operands.
    #[test]
    fn interpreter_directory_and_checkout_arguments_refuse() {
        let mut repo = repo();
        repo.home = repo.root.parent().unwrap().to_path_buf();
        assert_refused(
            &repo,
            &repo.root,
            &repo.root,
            &[
                "python3 tool.py .worktrees/w/{.git,.claude}",
                "python3 tool.py .worktrees/{w,x}/.git",
                "python3 tool.py .worktrees/w/.git{,}",
                "python3 tool.py .worktrees/w/.git",
                "node tool.js .worktrees/w/.git",
                "python3 tool.py ~/repo/.worktrees/w/.git",
                "python3 tool.py .claude",
                "python3 tool.py .worktrees/w",
                "python3 tool.py .worktrees",
            ],
        );
        assert_refused(&repo, &repo.linked, &repo.linked, &["python3 tool.py .git"]);
        std::fs::write(repo.root.join("case-probe"), "probe").unwrap();
        let folds_case = repo.root.join("CASE-PROBE").exists();
        let findings = judge_in(
            &repo,
            &repo.root,
            &repo.root,
            "python3 tool.py .WORKTREES/W/.GIT",
            PolicyLevel::Block,
        );
        assert_eq!(argument_refusal(&findings), folds_case, "{findings:?}");
        // A launcher can run in a different repository. Its registered
        // checkout roots must qualify for the same candidate checks.
        let other = self::repo();
        let moved = other.root.join("away/w");
        std::fs::create_dir(moved.parent().unwrap()).unwrap();
        std::fs::rename(&other.linked, &moved).unwrap();
        std::fs::write(
            other.root.join(".git/worktrees/w/gitdir"),
            format!("{}\n", moved.join(".git").display()),
        )
        .unwrap();
        assert_refused(
            &repo,
            &repo.root,
            &repo.root,
            &[&format!(
                "env -C '{}' python3 tool.py away/w",
                other.root.display()
            )],
        );
    }

    #[test]
    fn unquoted_shell_patterns_reach_protected_paths() {
        let repo = repo();
        assert_refused(
            &repo,
            &repo.root,
            &repo.root,
            &[
                "python3 tool.py .worktrees/w/*(D)",
                "python3 tool.py .claude/*(D)",
                "python3 tool.py .cla(ude)/settings.json",
                "python3 tool.py .worktrees/w/(.git|x)",
                "python3 tool.py .worktrees/w/.g(i)t",
                "python3 tool.py .worktrees/w/@(.git|.claude)",
                "python3 tool.py .worktrees/w/^src(D)",
                "python3 tool.py *(D)",
                "python3 tool.py .worktrees/w/<1-9>",
                "env python3 tool.py \".worktrees/w/\"*(D)",
                "nice python3 tool.py .worktrees/w/*(D)",
            ],
        );
    }

    #[test]
    fn interpreter_project_locations_stay_allowed() {
        let repo = repo();
        let standalone = repo.root.parent().unwrap().join("standalone");
        git2::Repository::init(&standalone).unwrap();
        let mut wrong = Vec::new();
        for root in [&standalone, &repo.root, &repo.linked] {
            let discovered = crate::hooks::repo::open(root).unwrap().unwrap();
            assert!(
                crate::hooks::edit_guard::is_own_checkout_location(root, root, None).unwrap(),
                "checkout {root:?}, discovered workdir {:?}",
                discovered.workdir()
            );
            std::fs::create_dir_all(root.join("src")).unwrap();
            for (dir, command) in [
                (root.clone(), "python3 tool.py ."),
                (root.clone(), "python3 -m pytest ."),
                (root.clone(), "python3 -m black ."),
                (root.clone(), "python3 -m pip install -e ."),
                (root.clone(), "python3 -m ruff check ."),
                (root.clone(), "node x.js ."),
                (root.join("src"), "python3 tool.py .."),
                (root.join("src"), "python3 tool.py ."),
            ] {
                let findings = judge_in(&repo, &dir, root, command, PolicyLevel::Block);
                if !findings.is_empty() {
                    wrong.push(format!("{dir:?}: {command}: {findings:?}"));
                }
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    #[test]
    fn code_tilde_is_literal_when_home_contains_the_checkout() {
        let mut repo = repo();
        repo.home = repo.root.parent().unwrap().to_path_buf();
        assert_allowed(&repo, &[r#"python3 -c 'print("~")'"#]);
        assert_refused(&repo, &repo.root, &repo.root, &["python3 tool.py ~"]);
        assert_allowed(
            &repo,
            &[
                r#"python3 -c 'import os; open(os.path.expanduser("~")+"/."+"zsh"+"rc","a")'"#,
                r#"node -e 'require("fs").rmSync(require("os").homedir()+"/repo/.worktrees/w", {recursive:true})'"#,
            ],
        );
    }

    #[test]
    fn code_tilde_paths_judge_literal_and_home_readings() {
        let mut repo = repo();
        repo.home = repo.root.parent().unwrap().to_path_buf();
        assert_refused(
            &repo,
            &repo.root,
            &repo.root,
            &[
                r#"python3 -c 'import shutil,os; shutil.rmtree(os.path.expanduser("~/repo/.worktrees/w"))'"#,
                r#"python3 -c 'import shutil,os; shutil.rmtree(os.path.expanduser("~/repo/.worktrees"))'"#,
                r#"python3 -c 'import shutil,os; shutil.rmtree(os.path.expanduser("~/repo/.claude"))'"#,
            ],
        );
        // The literal reading still protects a repository beneath a directory
        // actually named `~`, even when its home-expanded counterpart is absent.
        let nested = repo.root.join("~/literal");
        std::fs::create_dir(nested.parent().unwrap()).unwrap();
        std::fs::rename(&repo.linked, &nested).unwrap();
        std::fs::write(
            repo.root.join(".git/worktrees/w/gitdir"),
            format!("{}\n", nested.join(".git").display()),
        )
        .unwrap();
        assert_refused(
            &repo,
            &repo.root,
            &repo.root,
            &[r#"python3 -c 'open("~/literal/.claude/settings.json", "w")'"#],
        );
    }

    #[test]
    fn escaped_native_code_paths_keep_checkout_identity() {
        let repo = repo();
        for (path, refuse) in [
            (repo.linked.clone(), true),
            (repo.root.join("src/app.py"), false),
        ] {
            for literal in [
                serde_json::to_string(path.to_str().unwrap()).unwrap(),
                format!("\"{}\"", path.display()),
            ] {
                let command = format!("python3 -c 'open({literal}, \"w\")'");
                let findings =
                    judge_in(&repo, &repo.root, &repo.root, &command, PolicyLevel::Block);
                assert_eq!(
                    integrity_refusal(&findings).is_some(),
                    refuse,
                    "{command}: {findings:?}"
                );
            }
        }
    }

    #[test]
    fn linked_checkout_ancestors_refuse() {
        let repo = repo();
        let commands = [
            "python3 tool.py ../..".to_string(),
            "python3 tool.py \"@ROOT@\"".to_string(),
            "python3 tool.py ../../.worktrees".to_string(),
            "python3 tool.py ..".to_string(),
        ];
        #[cfg(unix)]
        let commands = {
            let mut commands = Vec::from(commands);
            let alias = repo.root.parent().unwrap().join("ancestor-alias");
            std::os::unix::fs::symlink(&repo.root, &alias).unwrap();
            commands.push(format!("python3 tool.py \"{}\"", alias.display()));
            commands
        };
        assert_refused(
            &repo,
            &repo.linked,
            &repo.linked,
            &commands.iter().map(String::as_str).collect::<Vec<_>>(),
        );
    }

    #[cfg(unix)]
    #[test]
    fn external_aliases_cannot_hide_linked_git_pointers() {
        let repo = repo();
        let base = repo.root.parent().unwrap();
        let alias = base.join("alias");
        let plain = base.join("plain");
        std::fs::create_dir(&plain).unwrap();
        std::os::unix::fs::symlink(&repo.root, &alias).unwrap();
        std::os::unix::fs::symlink(&repo.linked, plain.join("x")).unwrap();
        for path in [
            alias.join(".worktrees/w/.git"),
            alias.join(".worktrees/w/.g*"),
            alias.join(".worktrees/*/.git"),
            plain.join("x/.git"),
        ] {
            assert_refused(
                &repo,
                &repo.root,
                &repo.root,
                &[&format!("python3 tool.py {}", path.display())],
            );
            for command in [
                format!("cp x {}", path.display()),
                format!("echo hi > {}", path.display()),
            ] {
                let policy = crate::hooks::policy::GitPolicy::default();
                let context = crate::hooks::git_guard::GuardContext {
                    policy: &policy,
                    current_branch: "task/alias",
                    integrate_token: false,
                    pr_base_lookup: None,
                    dir_target_lookup: None,
                    alias_lookup: None,
                    discard_lookup: None,
                    branch_lookup: None,
                    root_checkout: None,
                };
                let findings =
                    crate::hooks::git_guard::evaluate_report_at(&command, &context, &repo.root)
                        .violations;
                assert!(
                    integrity_refusal(&findings).is_some(),
                    "{command}: {findings:?}"
                );
            }
        }
    }

    /// An expanded directory operand cannot be followed as literal text.
    #[test]
    fn interpreter_pattern_directory_changes_are_unknown() {
        let repo = repo();
        assert_refused(
            &repo,
            &repo.root,
            &repo.root,
            &[
                "cd .cla* && python3 tool.py settings.json",
                "cd .c?aude && python3 tool.py settings.json",
                "cd .[c]laude && python3 tool.py settings.json",
                "cd {.claude,x} && python3 tool.py settings.json",
                "pushd .cla* && python3 tool.py settings.json",
                "env -C .cla* python3 tool.py settings.json",
                "ruby -C .cla* tool.rb settings.json",
                "cd .wor* && python3 tool.py w/.g*",
            ],
        );
    }

    /// Integrity still needs a safe prefix past 4096 entries. B10's larger
    /// secret-read budget fully verifies 5000 ordinary matches; escapes and
    /// held checkouts retain the integrity entry-budget refusal.
    #[test]
    fn interpreter_large_glob_requires_a_safe_prefix() {
        let repo = repo();
        let many = repo.root.join("many");
        std::fs::create_dir(&many).unwrap();
        for n in 0..5000 {
            std::fs::write(many.join(format!("f{n}")), "").unwrap();
        }
        let mut wrong = Vec::new();
        let mut check = |command: &str, refuse: bool| {
            let findings = judge_in(&repo, &repo.root, &repo.root, command, PolicyLevel::Block);
            let correct = if refuse {
                integrity_refusal(&findings)
                    .is_some_and(|v| v.message.contains("more than 4096 entries"))
            } else {
                findings.is_empty()
            };
            if !correct {
                wrong.push(format!("{command}: refuse={refuse}: {findings:?}"));
            }
        };
        check("python3 tool.py many/*", false);
        check("python3 tool.py many/*/../../.claude/settings.json", true);
        check("python3 tool.py many/*/../.claude/x", true);
        let moved = many.join("linked");
        std::fs::rename(&repo.linked, &moved).unwrap();
        std::fs::write(
            repo.root.join(".git/worktrees/w/gitdir"),
            format!("{}\n", moved.join(".git").display()),
        )
        .unwrap();
        check("python3 tool.py many/*", true);
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    /// Code string fragments do not acquire recursive checkout-operand
    /// semantics. Shell arguments after the code still take those checks.
    #[test]
    fn interpreter_code_fragments_are_not_checkout_operands() {
        let repo = repo();
        assert_allowed(
            &repo,
            &[
                r#"python3 -c 'import os; open(os.path.expanduser("~")+"/."+"zsh"+"rc","a")'"#,
                r#"python3 -c 'print("/.")'"#,
                r#"python3 -c 'print("/..")'"#,
            ],
        );
        assert_refused(
            &repo,
            &repo.linked,
            &repo.linked,
            &[
                "python3 -c 'pass' ../..",
                "python3 tool.py ../..",
                "python3 tool.py .git",
                "python3 tool.py .claude/*(D)",
            ],
        );
        assert_refused(
            &repo,
            &repo.root,
            &repo.root,
            &["python3 -c 'open(\"@LINKED@\", \"w\")'"],
        );
        let main = repo.root.to_string_lossy().replace('\\', "/");
        let parent = repo
            .root
            .parent()
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let src = repo.root.join("src");
        let mut wrong = Vec::new();
        for (cwd, paths) in [
            (
                &repo.linked,
                vec!["../..", "..", "../../..", "../../.worktrees", &main],
            ),
            (&repo.root, vec!["..", &parent]),
            (&src, vec!["../.."]),
        ] {
            for path in paths {
                for command in [
                    format!(r#"python3 -c 'import shutil; shutil.rmtree("{path}")'"#),
                    format!(r#"node -e 'require("fs").rmSync("{path}",{{recursive:true}})'"#),
                    format!(r#"perl -e 'use File::Path qw(remove_tree); remove_tree("{path}")'"#),
                    format!(r#"ruby -e 'require "fileutils"; FileUtils.rm_rf("{path}")'"#),
                ] {
                    let findings = judge_in(&repo, cwd, cwd, &command, PolicyLevel::Block);
                    if integrity_refusal(&findings).is_none() {
                        wrong.push(format!("should refuse in {cwd:?}: {command}: {findings:?}"));
                    }
                }
            }
        }
        // Code roots remain fragments, including normalized dot components.
        let filesystem_root = repo.root.ancestors().last().unwrap().to_string_lossy();
        let plain_root = filesystem_root.trim_start_matches(r"\\?\");
        #[cfg(windows)]
        let native_roots = [plain_root.to_string(), format!(r"\\?\{plain_root}")];
        #[cfg(not(windows))]
        let native_roots = [plain_root.to_string()];
        for cwd in [&repo.root, &repo.linked, &src] {
            // Exercise native normalization directly for verbatim roots too.
            for root in &native_roots {
                for suffix in ["", ".", ".."] {
                    let path = format!("{root}{suffix}");
                    assert!(
                        crate::hooks::edit_guard::is_own_checkout_location(
                            Path::new(&path),
                            cwd,
                            Some(&path),
                        )
                        .unwrap(),
                        "code root {path:?} in {cwd:?}"
                    );
                    assert!(
                        !crate::hooks::edit_guard::is_own_checkout_location(
                            Path::new(&path),
                            cwd,
                            None,
                        )
                        .unwrap(),
                        "argv root {path:?} in {cwd:?}"
                    );
                }
            }
            for suffix in ["", ".", ".."] {
                let path = format!("{}{suffix}", plain_root.replace('\\', "/"));
                let command = format!(r#"python3 -c 'print("{path}")'"#);
                let findings = judge_in(&repo, cwd, cwd, &command, PolicyLevel::Block);
                if !findings.is_empty() {
                    wrong.push(format!("should allow in {cwd:?}: {command}: {findings:?}"));
                }
            }
            let command =
                r#"python3 -c 'import os; open(os.path.expanduser("~")+"/."+"zsh"+"rc","a")'"#;
            let findings = judge_in(&repo, cwd, cwd, command, PolicyLevel::Block);
            if !findings.is_empty() {
                wrong.push(format!("should allow in {cwd:?}: {command}: {findings:?}"));
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    /// TSK-302 AC-2: the interpreter is found through the wrappers the
    /// parser reads, and `xargs` and `find -exec` supply its path.
    #[test]
    fn wrapped_interpreter_arguments_refuse() {
        let repo = repo();
        assert_refused(
            &repo,
            &repo.root,
            &repo.root,
            &[
                "env python3 edit.py .claude/settings.json",
                "env FOO=1 python3 edit.py .claude/settings.json",
                "env -S 'python3 edit.py .claude/settings.json'",
                "command python3 edit.py .claude/settings.json",
                "nohup node x.js .codeflow/policy.json",
                "timeout 5 python3 edit.py .claude/settings.json",
                "bash -c 'python3 edit.py .claude/settings.json'",
                "sh -c 'python3 edit.py \"$1\"' sh .codeflow/policy.json",
                "echo .claude/settings.json | xargs python3 edit.py",
                "printf '%s\\n' \"@LINKED@/.claude/settings.json\" | xargs -I {} python3 edit.py {}",
                "find .claude -name settings.json -exec python3 edit.py {} \\;",
                "find . -name 'settings.json' -exec python3 edit.py {} +",
                "find . -name 'set*' -exec node x.js {} \\;",
                "find . -name settings.json -exec sh -c 'python3 edit.py \"$1\"' sh {} \\;",
                "cd .claude && python3 edit.py settings.json",
                "cd .codeflow; ruby x.rb policy.json",
                "cd \"$D\" && python3 edit.py .claude/settings.json",
                "cd \"$D\" && python3 edit.py policy.json",
            ],
        );
    }

    /// TSK-302 AC-3: policy authority metadata refuses even with
    /// `git.hook_integrity` off; an enforcement path then passes.
    #[test]
    fn interpreter_arguments_naming_authority_refuse_at_every_level() {
        let repo = repo();
        for command in [
            "python3 -c 'import sys' .git/config",
            "python3 edit.py .git/refs/remotes/origin/main",
        ] {
            let findings = judge_in(&repo, &repo.root, &repo.root, command, PolicyLevel::Off);
            assert!(
                findings
                    .iter()
                    .any(|v| v.rule == "git.hook_integrity" && v.level_fixed),
                "{command}: {findings:?}"
            );
        }
        assert!(judge_in(
            &repo,
            &repo.root,
            &repo.root,
            "python3 edit.py .claude/settings.json",
            PolicyLevel::Off
        )
        .is_empty());
    }

    #[test]
    fn interpreter_hidden_worktree_glob_refuses() {
        let repo = repo();
        let word = format!("{}/.*", repo.linked.display());
        let paths = crate::hooks::git_guard::glob_paths(&word, &repo.root, &repo.root).unwrap();
        for name in [".git", ".claude"] {
            let expected = crate::portable_path::canonicalize(&repo.linked.join(name)).unwrap();
            assert!(
                paths.contains(&expected),
                "expected {expected:?}: {paths:?}"
            );
        }
        assert_refused(
            &repo,
            &repo.root,
            &repo.root,
            &[
                "python3 tool.py @LINKED@/.*",
                "python3 tool.py \"@LINKED@/\".*",
                "python3 tool.py @LINKED@/.cla*",
                "python3 tool.py @LINKED@/.g*",
                "python3 tool.py @ROOT@/.worktrees/*",
            ],
        );
        for command in ["python3 tool.py src/*", "python3 tool.py src/missing*"] {
            let findings = judge_in(&repo, &repo.root, &repo.root, command, PolicyLevel::Block);
            assert!(findings.is_empty(), "{command}: {findings:?}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_repository_spellings_keep_ordinary_arguments_and_enforcement_refusals() {
        let repo = repo();
        let plain = crate::portable_path::canonicalize(&repo.root).unwrap();
        let verbatim = repo.root.canonicalize().unwrap();
        let spellings = [
            plain.display().to_string(),
            verbatim.display().to_string(),
            plain.display().to_string().replace('\\', "/"),
            plain.display().to_string().to_uppercase(),
        ];
        let mut results = Vec::new();
        let mut mismatches = Vec::new();
        for root in [&plain, &verbatim] {
            for spelling in &spellings {
                // Preserve the native spelling through POSIX double quotes.
                // In particular, `\\` must reach the path reader as two
                // separators in a verbatim namespace prefix.
                let quoted = spelling.replace('\\', "\\\\");
                let ordinary = format!(r#"W="{quoted}"; python3 tool.py "$W/src/app.py""#);
                let findings = judge_in(&repo, root, root, &ordinary, PolicyLevel::Block);
                let result = format!("cwd={root:?}; {ordinary}: {findings:?}");
                results.push(result.clone());
                if !findings.is_empty() {
                    mismatches.push(result);
                }
                for suffix in [
                    ".claude/settings.json",
                    ".worktrees/w/.claude/settings.json",
                    ".worktrees/w/.claude/*",
                    ".worktrees/w/.*",
                ] {
                    let command = format!(r#"python3 tool.py "{quoted}/"{suffix}"#);
                    let findings = judge_in(&repo, root, root, &command, PolicyLevel::Block);
                    let result = format!("cwd={root:?}; {command}: {findings:?}");
                    results.push(result.clone());
                    if integrity_refusal(&findings).is_none() {
                        mismatches.push(result);
                    }
                }
            }
        }
        println!("{}", results.join("\n"));
        assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
    }

    /// TSK-302 AC-4: a value the guard cannot read takes git-guard's rule
    /// for writer operands, and the refusal names the sanctioned route.
    #[test]
    fn unreadable_interpreter_arguments_take_the_writer_rule() {
        let repo = repo();
        assert_refused(
            &repo,
            &repo.root,
            &repo.root,
            &[
                // The issue's command, its main checkout and linked forms.
                "W=@ROOT@; python3 -I -B - \"$W/.claude/settings.json\" <<'PY'\nimport json, sys\np = sys.argv[1]\nopen(p, 'w').write('')\nPY",
                "W=@LINKED@; python3 - \"$W/.claude/settings.json\" <<'PY'\nimport sys\nPY",
                "W=@ROOT@; python3 - \"$W/.codeflow/policy.json\"",
                "python3 edit.py \"$REPO/.claude/settings.json\"",
                "python3 edit.py \"${REPO}/.codeflow/policy.json\"",
                "python3 edit.py \"$HOME/.claude/settings.json\"",
                "P=.codeflow/policy.json; python3 edit.py \"$P\"",
                "python3 edit.py \"$(echo .codeflow/policy.json)\"",
            ],
        );
        let findings = judge_in(
            &repo,
            &repo.root,
            &repo.root,
            "P=.codeflow/policy.json; python3 edit.py \"$P\"",
            PolicyLevel::Block,
        );
        let refusal = integrity_refusal(&findings).unwrap();
        assert!(
            refusal.message.contains(".codeflow/policy.json"),
            "{}",
            refusal.message
        );
        assert!(
            refusal
                .remedy
                .contains("redirect it into the interpreter's stdin"),
            "{}",
            refusal.remedy
        );
        assert_allowed(
            &repo,
            &[
                "python3 tool.py \"$HOME/notes.md\"",
                "for f in src/*.py; do python3 lint.py \"$f\"; done",
                "W=@ROOT@; python3 tool.py \"$W/src/app.py\"",
            ],
        );
    }

    /// TSK-302 AC-5: ordinary data and output files, reading an
    /// enforcement file with a reader or on stdin, and text that only
    /// shows such a command stay allowed.
    #[test]
    fn ordinary_interpreter_arguments_stay_allowed() {
        let repo = repo();
        let supplied = judge_in(
            &repo,
            &repo.root,
            &repo.root,
            "ls | xargs python3 edit.py",
            PolicyLevel::Block,
        );
        assert!(
            supplied
                .iter()
                .any(|finding| finding.rule == "security.secret_reads"),
            "{supplied:?}"
        );
        assert_allowed(
            &repo,
            &[
                "python3 tool.py \"foo (bar)\"",
                "python3 tool.py \"x (y)\"",
                "python3 tool.py \"x (y)\" notes.txt",
                "python3 tool.py \"print(x)\"",
                "python3 tool.py \"my file(1).txt\"",
                "python3 tool.py 'notes (draft).md'",
                "node x.js --filter \"(a|b)\"",
                "python3 tool.py .",
                "python3 -m pytest .",
                "python3 -m black .",
                "python3 -m pip install -e .",
                "python3 -m ruff check .",
                "node x.js .",
                "python3 edit.py notes.txt out.json",
                "python3 -m json.tool data.json out.json",
                "python3 -W ignore src/app.py",
                "node build.js --out=dist/app.js",
                "python3 - <<'PY'\nprint(1)\nPY",
                "python3 -c 'import json, sys; print(json.load(sys.stdin))' < .codeflow/policy.json",
                "cat .codeflow/policy.json | python3 -c 'import sys; print(sys.stdin.read())'",
                "jq . .codeflow/policy.json",
                "find src -name '*.py' -exec python3 -m py_compile {} +",
                "find . -name '*.py' -exec python3 -m py_compile {} +",
                "find src -name '*.py' -exec sh -c 'python3 lint.py \"$1\"' sh {} \\;",
                "for f in src/*.py; do python3 lint.py \"$f\"; done",
                "echo notes.txt | xargs python3 edit.py",
                "cat > notes.md <<'EOF'\nexample: python3 edit.py .claude/settings.json\nEOF",
                "printf '%s\\n' 'python3 edit.py .claude/settings.json' > notes.md",
                "cd src && python3 app.py settings.json",
                "python3 edit.py \"@ROOT@/src/app.py\"",
            ],
        );
    }

    /// The arguments the parser hands over: inline code is left out, and
    /// the directory follows `cd`, including one it cannot follow.
    #[test]
    fn interpreter_calls_leave_out_code_and_follow_cd() {
        let cwd = Path::new("/w");
        let calls = outward::interpreter_calls("python3 -c 'x = 1' a.py b", cwd);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].args, ["a.py", "b"]);
        // Every word after the code or script boundary is an argument, a
        // second `-c` included; option values are kept, flags are not.
        let args = |line: &str| outward::interpreter_calls(line, cwd)[0].args.clone();
        assert_eq!(args("python3 -c 'x' -c s.json"), ["-c", "s.json"]);
        assert_eq!(args("python3 -Ic'x' a"), ["a"]);
        // `-m` is a module name, not a path; the words after it are argv.
        assert_eq!(
            args("python3 -W ignore -m json.tool a b"),
            ["ignore", "a", "b"]
        );
        assert_eq!(args("python3 -B - a"), ["-", "a"]);
        assert_eq!(args("python3 -- s.py -o"), ["s.py", "-o"]);
        assert_eq!(args("node -pe 'x' a -e b"), ["a", "-e", "b"]);
        assert_eq!(
            args("node --require ./r.js app.js a"),
            ["./r.js", "app.js", "a"]
        );
        assert_eq!(args("perl -i.bak -pe 's/a/b/' -e 'c' f"), ["f"]);
        assert_eq!(
            args("perl -I lib -Mstrict -e 'x' f"),
            ["lib", "strict", "f"]
        );
        assert_eq!(args("ruby -C d -e 'x' f")[..], ["f"]);
        assert_eq!(
            outward::interpreter_calls("ruby -C d -C e -e 'x' f", cwd)[0].chdir,
            ["d", "e"]
        );
        assert_eq!(
            outward::interpreter_calls("node --eval=code x", cwd)[0].args,
            ["x"]
        );
        // The line's directory set: every directory a change may reach,
        // whether or not it succeeds, through every spelling.
        let set = |line: &str| outward::line_directories(line, cwd, Some(Path::new("/h")));
        let has = |line: &str, dir: &str| set(line).dirs.contains(&PathBuf::from(dir));
        for (line, dir) in [
            ("cd sub && python3 x", "/w/sub"),
            ("cd -- sub && python3 x", "/w/sub"),
            ("cd -P -L sub && python3 x", "/w/sub"),
            ("cd -q sub; python3 x", "/w/sub"),
            ("command cd sub && python3 x", "/w/sub"),
            ("command -p cd sub && python3 x", "/w/sub"),
            ("builtin cd sub && python3 x", "/w/sub"),
            ("! cd sub; python3 x", "/w/sub"),
            ("pushd sub >/dev/null && python3 x", "/w/sub"),
            ("chdir sub && python3 x", "/w/sub"),
            ("env -C sub python3 x", "/w/sub"),
            ("env -Csub python3 x", "/w/sub"),
            ("env --chdir=sub python3 x", "/w/sub"),
            ("git -C sub status; python3 x", "/w/sub"),
            ("cd a && cd b && python3 x", "/w/a/b"),
            ("cd a && cd .. && python3 x", "/w/a"),
            ("cd && python3 x", "/h"),
        ] {
            assert!(has(line, dir), "{line}: {:?}", set(line));
            assert!(has(line, "/w"), "{line}: {:?}", set(line));
        }
        for line in [
            "cd \"$D\" && python3 x",
            "cd - && python3 x",
            "cd ~u && python3 x",
        ] {
            assert!(set(line).unknown, "{line}");
        }
        assert!(outward::interpreter_calls("rustc x.rs", cwd).is_empty());
        // A version suffix is read past; a multi-byte name ending is not cut
        // inside a character.
        assert_eq!(outward::interpreter_calls("ruby3.3 x", cwd).len(), 1);
        assert!(outward::interpreter_calls("gre\u{a0}1 x", cwd).is_empty());
    }

    /// Every command refuses with a finding about an interpreter argument
    /// that keeps its level fixed (authority metadata) at `level`.
    fn assert_authority(repo: &Repo, commands: &[&str], level: PolicyLevel) {
        let mut wrong = Vec::new();
        for command in commands {
            let findings = judge_in(repo, &repo.root, &repo.root, command, level);
            if !findings
                .iter()
                .any(|v| v.rule == "git.hook_integrity" && v.level_fixed)
            {
                wrong.push(format!(
                    "should refuse at {level:?}: {command}: {findings:?}"
                ));
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    /// TSK-302 review F1: a word after the interpreter's code or script
    /// boundary is an argument, a second `-c` included, and a short
    /// option's attached value is read as a path; ordinary options pass.
    #[test]
    fn arguments_after_code_and_attached_values_refuse() {
        let repo = repo();
        assert_refused(
            &repo,
            &repo.root,
            &repo.root,
            &[
                "cd .claude && python3 -c 'import sys; open(sys.argv[2], \"a\").write(\"\")' -c settings.json",
                "python3 -c 'import argparse; p=argparse.ArgumentParser(); p.add_argument(\"-o\"); open(p.parse_args().o,\"a\").write(\"\")' -o.claude/settings.json",
                "python3 -c 'x' -- .claude/settings.json",
                "python3 - -o.codeflow/policy.json <<'PY'\nimport sys\nPY",
                "python3 tool.py -o.claude/settings.json",
                "python3 -m tool .codeflow/policy.json",
                "node app.js -o.claude/settings.json",
                "perl -e 'x' -- -o.claude/settings.json",
                "ruby -C .claude -e 'x' settings.json",
            ],
        );
        assert_allowed(
            &repo,
            &[
                "python3 -c 'x' -c data.json",
                "python3 tool.py -v -n3 --level=2 -o out.json",
                "python3 -W ignore -X dev tool.py data.json",
                "node --require ./setup.js app.js data.json",
                "perl -I lib -e 'x' notes.txt",
                "ruby -C src -e 'x' app.rb",
            ],
        );
    }

    /// TSK-302 review F2: `cd` options are read past, an `&&` chain runs in
    /// the directory its changes lead to, and `find -execdir` runs its
    /// command in each directory it visits.
    #[test]
    fn the_directory_an_interpreter_runs_in_is_followed() {
        let repo = repo();
        assert_refused(
            &repo,
            &repo.root,
            &repo.root,
            &[
                "cd -- .claude && python3 -c 'import sys; open(sys.argv[1], \"a\").write(\"\")' settings.json",
                "cd -P .claude && python3 -c 'import sys; open(sys.argv[1], \"a\").write(\"\")' settings.json",
                "cd -L -- .claude && python3 tool.py settings.json",
                "cd .claude || true; python3 tool.py settings.json",
                "find . -name '*.json' -execdir python3 tool.py settings.json \\;",
                "cd \"$D\" && python3 tool.py policy.json",
            ],
        );
        assert_allowed(
            &repo,
            &[
                "cd src && python3 tool.py out.json",
                "find src -name '*.py' -execdir python3 -m py_compile {} +",
                "find src -name '*.py' -execdir python3 lint.py out.txt \\;",
            ],
        );
    }

    /// TSK-302 review F3: authority metadata refuses in every operand form,
    /// supplied, filled in at run time or globbed, whatever the integrity
    /// level. Unresolved shell input near a checkout is covered by TSK-406.
    #[test]
    fn authority_refuses_in_every_operand_form_at_every_level() {
        let repo = repo();
        for level in [
            PolicyLevel::Block,
            PolicyLevel::Warn,
            PolicyLevel::Allow,
            PolicyLevel::Off,
        ] {
            assert_authority(
                &repo,
                &[
                    "python3 tool.py .git/config",
                    "echo .git/config | xargs python3 tool.py",
                    "find .git -name config -exec python3 tool.py {} \\;",
                    "P=.git/config; python3 tool.py \"$P\"",
                    "python3 tool.py .git/con*",
                    "python3 tool.py .{claude,git}/*",
                    "python3 tool.py \"$(echo .git/config)\"",
                ],
                level,
            );
            for command in [
                "for f in src/*.py; do python3 lint.py \"$f\"; done",
                "find src -exec python3 tool.py {} \\;",
            ] {
                let findings = judge_in(&repo, &repo.root, &repo.root, command, level);
                assert!(findings.is_empty(), "{level:?}: {command}: {findings:?}");
            }
            let findings = judge_in(
                &repo,
                &repo.root,
                &repo.root,
                "ls | xargs python3 tool.py",
                level,
            );
            assert!(findings
                .iter()
                .any(|finding| finding.rule == "security.secret_reads"));
            assert!(
                findings.iter().all(|finding| !finding.level_fixed),
                "{findings:?}"
            );
        }
    }

    /// TSK-406 AC-3 supersedes the old allowances for unresolved shell input.
    #[test]
    fn unresolved_shell_expansions_refuse_near_a_checkout() {
        let repo = repo();
        assert_public_refused(
            &repo,
            &[
                "python3 tool.py \"$INPUT\"",
                "python3 tool.py \"$ROOT/$NAME\"",
                "python3 tool.py \"$ROOT/notes.txt\"",
                "ROOT=$(git rev-parse --show-toplevel); python3 \"$ROOT/src/app.py\"",
                "while read f; do python3 lint.py \"$f\"; done < files.txt",
                "python3 lint.py \"$(echo src/*.py)\"",
                "python3 lint.py \"$(echo ok src/*.py)\"",
                "python3 -c 'import sys; print(sys.argv[1:])' \"$config\"",
                "cd \"$D\" && python3 -c 'import sys; open(sys.argv[1])' \"$IN\"",
            ],
            false,
        );
    }

    /// TSK-302 review F4: a line with several suppliers reads what the line
    /// names once, so five `xargs` interpreters stay within the hook's
    /// 10-second budget. TSK-453 counts the reads instead of timing them,
    /// so host load cannot fail the test.
    #[test]
    fn several_suppliers_read_the_line_once() {
        use super::super::arguments::{LineReads, LINE_READS};
        let repo = repo();
        let line = (0..5)
            .map(|n| format!("ls | xargs python3 check{n}.py"))
            .collect::<Vec<_>>()
            .join("; ");
        let none = LineReads {
            asked: 0,
            enforcement: 0,
            authority: 0,
        };
        LINE_READS.with(|reads| reads.set(none));
        let findings = judge_in(&repo, &repo.root, &repo.root, &line, PolicyLevel::Block);
        assert!(
            findings
                .iter()
                .any(|finding| finding.rule == "security.secret_reads"),
            "{findings:?}"
        );
        // Each of the five calls asks for both readings, and one enforcement
        // reading and one authority reading answer all of them.
        assert_eq!(
            LINE_READS.with(std::cell::Cell::get),
            LineReads {
                asked: 10,
                enforcement: 1,
                authority: 1,
            }
        );
    }

    /// The findings the public exec-guard boundary gives `command` from
    /// `dir` at `level`, after it merges the findings of each rule.
    fn public(repo: &Repo, dir: &Path, command: &str, level: PolicyLevel) -> Vec<Violation> {
        let command = command
            .replace("@ROOT@", &repo.root.display().to_string())
            .replace("@LINKED@", &repo.linked.display().to_string());
        crate::hooks::exec_guard::evaluate_in(
            &command,
            true,
            &SecuritySection::default(),
            level,
            dir,
            &repo.root,
        )
    }

    fn integrity_finding(findings: &[Violation]) -> Option<&Violation> {
        findings.iter().find(|v| v.rule == "git.hook_integrity")
    }

    /// Each command refuses under `git.hook_integrity` through the public
    /// boundary at block; with `fixed`, it also keeps a fixed block at warn,
    /// allow and off.
    fn assert_public_refused(repo: &Repo, commands: &[&str], fixed: bool) {
        let mut wrong = Vec::new();
        for command in commands {
            for level in [
                PolicyLevel::Block,
                PolicyLevel::Warn,
                PolicyLevel::Allow,
                PolicyLevel::Off,
            ] {
                if !fixed && level != PolicyLevel::Block {
                    continue;
                }
                let findings = public(repo, &repo.root, command, level);
                let ok = integrity_finding(&findings)
                    .is_some_and(|v| v.level == PolicyLevel::Block && (!fixed || v.level_fixed));
                if !ok {
                    wrong.push(format!(
                        "should refuse at {level:?}: {command}: {findings:?}"
                    ));
                }
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    fn assert_public_allowed(repo: &Repo, commands: &[&str]) {
        let mut wrong = Vec::new();
        for command in commands {
            let findings = public(repo, &repo.root, command, PolicyLevel::Block);
            if let Some(v) = integrity_finding(&findings) {
                wrong.push(format!("should allow: {command}: {}", v.message));
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    /// TSK-302 review round 2, Codex R2-1: the fixed authority block
    /// survives the merge of findings, whatever came first, and a broad
    /// unresolved glob that can reach `.git/config` is authority too.
    #[test]
    fn the_authority_block_survives_merged_findings() {
        let repo = repo();
        assert_public_refused(
            &repo,
            &[
                "python3 tool.py .claude/settings.json .git/config",
                "python3 tool.py .git/config .claude/settings.json",
                "python3 tool.py .claude/settings.json; python3 tool.py .git/config",
                "python3 -c 'open(\".claude/settings.json\")' .git/config",
                "W=.; python3 tool.py \"$W/.git/\"*",
                "W=.; python3 tool.py \"$W/.git/con\"*",
            ],
            true,
        );
    }

    /// Codex R2-2 and Grok F2: every directory change a line can make,
    /// whatever spelling and whether or not it succeeds, is a directory the
    /// interpreter may run in.
    #[test]
    fn every_directory_change_spelling_is_followed() {
        let repo = repo();
        assert_public_refused(
            &repo,
            &[
                "cd .claude && ! cd nonexistent && python3 tool.py settings.json",
                "ruby -C .claude -C . -e 'File.write(ARGV[0], \"\")' settings.json",
                "find ../repo -name '*.json' -execdir python3 tool.py settings.json \\;",
                "find .claude -name settings.json -execdir ruby -C ../.codeflow -e 'File.write(ARGV[0], \"\")' policy.json \\;",
                "command cd .claude && python3 tool.py settings.json",
                "builtin cd .claude && python3 tool.py settings.json",
                "command -p cd .claude && python3 tool.py settings.json",
                "pushd .claude >/dev/null && python3 tool.py settings.json",
                "chdir .claude && python3 tool.py settings.json",
                "cd -q .claude && python3 tool.py settings.json",
                "env -C.claude python3 tool.py settings.json",
                "cd .claude && command cd .. && python3 tool.py .claude/settings.json",
                "cd .claude && builtin cd .. && python3 tool.py .claude/settings.json",
                "cd .claude && pushd .. >/dev/null && python3 tool.py .claude/settings.json",
                "cd .claude && cd -q .. && python3 tool.py .claude/settings.json",
                // Known conservative refusals: the set keeps `.claude`.
                "cd .claude && cd .. && python3 tool.py settings.json",
                "find . -execdir python3 lint.py notes.txt \\;",
            ],
            false,
        );
        assert_public_refused(
            &repo,
            &[
                "command cd .git && python3 tool.py config",
                "builtin cd .git && python3 tool.py config",
                "pushd .git >/dev/null && python3 tool.py config",
                "env -C.git python3 tool.py config",
                "cd \"$D\" && python3 tool.py config",
            ],
            true,
        );
        assert_public_allowed(
            &repo,
            &[
                "cd src && python3 app.py settings.json",
                "find . -name '*.py' -execdir python3 -m py_compile {} +",
                "find . -name '*.rs' -execdir python3 lint.py {} +",
                "find src -execdir python3 lint.py notes.txt \\;",
                "pushd src >/dev/null && python3 tool.py out.json && popd",
            ],
        );
    }

    /// Grok F1: code text is judged from every directory the line may run
    /// it in, bare names included.
    #[test]
    fn code_text_is_judged_from_every_directory() {
        let repo = repo();
        assert_public_refused(
            &repo,
            &[
                "cd .claude && python3 -c 'open(\"settings.json\",\"w\").write(\"\")'",
                "cd .claude && python3 -c 'open(\"./settings.json\",\"w\").write(\"\")'",
                "cd -- .claude && python3 -c 'open(\"./settings.json\",\"w\").write(\"\")'",
                "cd -P .claude && python3 -c 'open(\"./settings.json\",\"w\").write(\"\")'",
                "cd .codeflow && python3 -c 'open(\"policy.json\",\"w\").write(\"\")'",
                "cd .claude && python3 -c 'open(\"settings.local.json\",\"w\").write(\"\")'",
                "cd .claude && node -e 'require(\"fs\").writeFileSync(\"settings.json\",\"\")'",
                "cd .claude && perl -e 'open F, \">\", \"settings.json\"'",
                "ruby -C .claude -e 'File.write(\"settings.json\",\"\")'",
                "ruby -C.claude -e 'File.write(\"./settings.json\",\"\")'",
            ],
            false,
        );
        assert_public_refused(
            &repo,
            &[
                "cd .git && python3 -c 'open(\"config\",\"w\").write(\"\")'",
                "cd .git && python3 -c 'open(\"./config\",\"a\").write(\"\")'",
            ],
            true,
        );
        assert_public_allowed(
            &repo,
            &[
                "cd .claude && python3 -c 'print(1)'",
                "cd src && python3 -c 'open(\"out.txt\",\"w\")'",
                "python3 -c 'import json, sys\\; print(json.dumps({\"a\": 1}))'",
            ],
        );
    }

    /// Codex R2-3: an attached value after a short-option cluster is read.
    #[test]
    fn an_attached_value_after_an_option_cluster_is_read() {
        let repo = repo();
        assert_public_refused(
            &repo,
            &[
                "python3 -c 'import argparse\\; p=argparse.ArgumentParser(); p.add_argument(\"-v\",action=\"store_true\"); p.add_argument(\"-o\"); open(p.parse_args().o,\"a\").write(\"\")' -vo.claude/settings.json",
                "python3 tool.py -xvo.codeflow/policy.json",
            ],
            false,
        );
        assert_public_allowed(
            &repo,
            &[
                "python3 tool.py -vvv -xz out.json",
                "python3 tool.py -vo out.json",
                "python3 tool.py -n3",
            ],
        );
    }

    /// TSK-302 review round 3: within the modeled grammar the directory set
    /// is exact, and anything outside it makes the set unknown, so known
    /// words are judged by their trailing components; it never fails open.
    #[test]
    fn launchers_and_unmodeled_grammar_never_fail_open() {
        let repo = repo();
        let go = repo.root.join("go.sh");
        std::fs::write(&go, "cd .claude\n").unwrap();
        let go = go.display().to_string();
        let source = |rest: &str| format!("source {go} && {rest}");
        let dot = |rest: &str| format!(". {go} && {rest}");
        let owned = [
            source("python3 tool.py settings.json"),
            source("python3 -c 'open(\"settings.json\",\"w\").write(\"\")'"),
            dot("python3 tool.py settings.json"),
        ];
        let owned: Vec<&str> = owned.iter().map(String::as_str).collect();
        assert_public_refused(&repo, &owned, false);
        let authority = source("python3 tool.py config");
        assert_public_refused(&repo, &[authority.as_str()], true);
        assert_public_allowed(&repo, &[source("python3 tool.py notes.txt").as_str()]);
        // Codex R3-1: options that consume values, attached spellings,
        // clusters and outer launchers.
        assert_public_refused(
            &repo,
            &[
                "env -u FOO -C.claude python3 -c 'import sys; open(sys.argv[1],\"w\")' settings.json",
                "nice env -C.claude python3 -c 'import sys; open(sys.argv[1],\"w\")' settings.json",
                "env -iC.claude python3 tool.py settings.json",
                "timeout 5 env -C.claude python3 tool.py settings.json",
                "nohup env --chdir .claude python3 tool.py settings.json",
                "stdbuf -oL env -C .claude python3 tool.py settings.json",
                "env -S 'python3 tool.py' settings.json",
                "env --frobnicate -C.claude python3 tool.py settings.json",
            ],
            false,
        );
        assert_public_refused(
            &repo,
            &["env -u FOO -C.git python3 -c 'import sys; open(sys.argv[1],\"w\")' config"],
            true,
        );
        assert_public_allowed(
            &repo,
            &[
                "nice python3 tool.py data.json",
                "nice -n 5 python3 tool.py data.json",
                "timeout 5 node x.js out.json",
                "timeout -s KILL 5 python3 app.py",
                "env FOO=1 python3 app.py",
                "env -u FOO python3 app.py data.json",
                "stdbuf -oL python3 app.py out.txt",
                "nohup python3 worker.py",
                "command python3 app.py out.json",
                "time python3 app.py",
            ],
        );
    }

    /// Codex R3-2: a code literal whose directory is unknown is judged by
    /// its trailing components; authority keeps its fixed level.
    #[test]
    fn code_literals_in_an_unknown_directory_are_judged() {
        let repo = repo();
        assert_public_refused(
            &repo,
            &[
                "D=.claude; cd \"$D\" && python3 -c 'open(\"settings.json\",\"w\")'",
                "D=.claude; cd \"$D\" && python3 tool.py settings.json",
            ],
            false,
        );
        assert_public_refused(
            &repo,
            &[
                "D=.git; cd \"$D\" && python3 -c 'open(\"config\",\"w\")'",
                "D=.git; cd \"$D\" && python3 tool.py config",
            ],
            true,
        );
        assert_public_allowed(
            &repo,
            &[
                "D=src; cd \"$D\" && python3 -c 'print(1)'",
                "D=src; cd \"$D\" && python3 -c 'open(\"out.txt\",\"w\")'",
            ],
        );
    }

    /// Codex R3-3 and Grok F1: `find -execdir` starts from every directory
    /// in the line's set, a file start runs in its parent, and `-path`
    /// sees find's own spelling.
    #[test]
    fn execdir_follows_the_line_set_and_find_spelling() {
        let repo = repo();
        assert_public_refused(
            &repo,
            &[
                "find . -path '*cla*json' -execdir python3 -c 'import sys; open(sys.argv[1],\"w\")' settings.json \\;",
                "find . -ipath '*CLA*JSON' -execdir python3 tool.py settings.json \\;",
                "find .claude/settings.json -execdir python3 tool.py settings.json \\;",
                "command cd .claude && find settings.json -execdir python3 tool.py settings.json \\;",
                "builtin cd .claude && find settings.json -execdir python3 tool.py settings.json \\;",
                "pushd .claude >/dev/null && find settings.json -execdir python3 tool.py settings.json \\;",
                "chdir .claude && find settings.json -execdir python3 tool.py settings.json \\;",
                "cd -q .claude && find settings.json -execdir python3 tool.py settings.json \\;",
                "env -C.claude find settings.json -execdir python3 tool.py settings.json \\;",
                "bash -c 'command cd .claude && find settings.json -execdir python3 tool.py settings.json \\;'",
                "command cd .codeflow && find policy.json -execdir python3 tool.py policy.json \\;",
            ],
            false,
        );
        assert_public_refused(
            &repo,
            &[
                "find . -path '*git*config' -execdir python3 -c 'import sys; open(sys.argv[1],\"w\")' config \\;",
                "command cd .git && find config -execdir python3 tool.py config \\;",
            ],
            true,
        );
        assert_public_allowed(
            &repo,
            &[
                "command cd .claude && find settings.json -execdir python3 tool.py notes.txt \\;",
                "find src -execdir python3 lint.py notes.txt \\;",
                "find . -name '*.py' -execdir python3 -m py_compile {} +",
                "find . -path './src/*' -execdir python3 lint.py notes.txt \\;",
            ],
        );
    }

    /// Codex R3-4: only string literals in code are read as file names, so
    /// identifiers and numbers in a directory-wide protected path pass.
    #[test]
    fn code_identifiers_are_not_file_names() {
        let repo = repo();
        assert_public_allowed(
            &repo,
            &[
                "cd .codex && python3 -c 'print(1)'",
                "cd .codeflow/git-hooks && python3 -c 'print(1)'",
                "cd .codeflow/git-hooks && node -e 'console.log(process.version)'",
            ],
        );
        assert_public_refused(
            &repo,
            &[
                "cd .codeflow/git-hooks && python3 -c 'open(\"pre-commit\",\"w\")'",
                "cd .claude && python3 -c 'open(\"settings.json\",\"w\")'",
            ],
            false,
        );
    }

    /// TSK-302 review round 4 (Grok F1, F2): an unmodeled word followed by
    /// a directory-change token makes the directory unknown, never
    /// unchanged; zsh precommands, `builtin --`, `caffeinate`, `xcrun`,
    /// `ionice` and the GNU-prefixed coreutils names are read as launchers.
    #[test]
    fn unmodeled_launcher_words_never_hide_a_directory_change() {
        let repo = repo();
        assert_public_refused(
            &repo,
            &[
                "noglob cd .claude && python3 tool.py settings.json",
                "noglob cd .claude && python3 -c 'open(\"settings.json\",\"w\").write(\"\")'",
                "nocorrect cd .claude && python3 tool.py settings.json",
                "bash -c 'builtin -- cd .claude && python3 tool.py settings.json'",
                "caffeinate env -C.claude python3 tool.py settings.json",
                "caffeinate env -C .claude python3 tool.py settings.json",
                "caffeinate env -C.claude python3 -c 'open(\"settings.json\",\"w\").write(\"\")'",
                "genv -C.claude python3 tool.py settings.json",
                "gtimeout 5 env -C.claude python3 tool.py settings.json",
                "ionice -c 3 env -C.claude python3 tool.py settings.json",
                "frobnicate cd .claude && python3 tool.py settings.json",
                "frobnicate env -C.claude x && python3 tool.py settings.json",
            ],
            false,
        );
        assert_public_refused(
            &repo,
            &[
                "noglob cd .git && python3 tool.py config",
                "true; - cd .git && python3 tool.py config",
                "bash -c 'builtin -- cd .git && python3 tool.py config'",
                "caffeinate env -C.git python3 tool.py config",
                "xcrun env -C.git python3 tool.py config",
                "genv -C.git python3 tool.py config",
                "gtimeout 5 env -C.git python3 tool.py config",
            ],
            true,
        );
        assert_public_allowed(
            &repo,
            &[
                "noglob cd .claude && python3 tool.py notes.txt",
                "caffeinate python3 tool.py data.json",
                "xcrun python3 tool.py data.json",
                "gtimeout 5 node x.js out.json",
                "genv FOO=1 python3 app.py",
                "echo done && python3 tool.py data.json",
            ],
        );
    }

    /// After review round 4: an unmodeled program followed by a modeled
    /// launcher or an interpreter in a command position may launch it, so
    /// the rest of the segment is judged as a call whose directory is
    /// unknown. Quoted arguments, words after `--` and `-m`/`-c` values
    /// never start a command.
    #[test]
    fn an_unknown_program_that_launches_an_interpreter_is_judged() {
        let repo = repo();
        assert_public_refused(
            &repo,
            &[
                "frobnicate env -C.claude python3 tool.py settings.json",
                "frobnicate python3 tool.py settings.json",
                "cd .claude && frobnicate python3 -c 'open(\"settings.json\",\"w\")'",
                "frobnicate --flag nice python3 tool.py settings.json",
            ],
            false,
        );
        assert_public_refused(
            &repo,
            &[
                "frobnicate env -C.git python3 tool.py config",
                "cd .git && frobnicate python3 -c 'open(\"config\",\"w\")'",
            ],
            true,
        );
        assert_public_allowed(
            &repo,
            &[
                "make test",
                "git commit -m \"python3 x\"",
                "cargo run -- python3",
                "cargo run -- python3 tool.py notes.txt",
                "npm run x -- --flag",
                "npm run build",
                "docker run img python3 app.py data.json",
                "frobnicate -m python3 settings.json",
                "frobnicate 'python3 tool.py notes.txt'",
                "frobnicate env -C.claude python3 tool.py notes.txt",
            ],
        );
    }

    /// Grok round 5 F1: a bare `--` after an unmodeled program does not
    /// hide the interpreter it runs (`sandbox-exec ... --`, `taskpolicy --`).
    /// The call is judged with an unknown directory, so a bare protected
    /// tail after `--` refuses too (`cargo run -- python3 tool.py
    /// settings.json`), as after `cd "$DIR"`.
    #[test]
    fn a_double_dash_does_not_hide_the_interpreter_a_program_runs() {
        let repo = repo();
        assert_public_refused(
            &repo,
            &[
                "sandbox-exec -p '(version 1)(allow default)' -- python3 tool.py .claude/settings.json",
                "sandbox-exec -p '(version 1)(allow default)' -- python3 -c 'open(\"settings.json\",\"w\").write(\"\")'",
                "taskpolicy -- python3 tool.py .claude/settings.json",
                "docker run --rm -v .:/w -w /w -- python:3 python3 tool.py .claude/settings.json",
                "frobnicate -- python3 tool.py settings.json",
                "cargo run -- python3 tool.py settings.json",
            ],
            false,
        );
        assert_public_refused(
            &repo,
            &[
                "sandbox-exec -p '(version 1)(allow default)' -- python3 tool.py .git/config",
                "taskpolicy -- python3 tool.py .git/config",
                "frobnicate -- python3 tool.py .git/config",
                "frobnicate -- env -C.git python3 tool.py config",
            ],
            true,
        );
        assert_public_allowed(
            &repo,
            &[
                "sandbox-exec -p '(version 1)(allow default)' -- python3 tool.py notes.txt",
                "taskpolicy -- python3 tool.py notes.txt",
                "cargo run -- python3",
                "cargo run -- python3 tool.py notes.txt",
                "git commit -m \"python3 x\"",
                "npm run x -- --flag",
                "make test",
                "npm run build",
            ],
        );
    }

    /// Grok round 6 F1: a script string an unmodeled program runs, its
    /// `-c` or `-e` value or any quoted word, is read as a command line
    /// with an unknown directory (`expect -c`, `osascript -e`, `ssh host`).
    /// `echo` and `printf` only print, so their operands stay data.
    #[test]
    fn a_script_string_an_unknown_program_runs_is_judged() {
        let repo = repo();
        assert_public_refused(
            &repo,
            &[
                "expect -c 'spawn python3 tool.py .claude/settings.json'",
                "osascript -e 'do shell script \"python3 tool.py .claude/settings.json\"'",
                "frobnicate -c 'python3 tool.py .claude/settings.json'",
                "frobnicate 'python3 tool.py settings.json'",
                "frobnicate -- 'python3 tool.py settings.json'",
                "ssh host 'cd .claude && python3 tool.py settings.json'",
                "frobnicate -m 'python3 tool.py .claude/settings.json'",
            ],
            false,
        );
        assert_public_refused(
            &repo,
            &[
                "expect -c 'spawn python3 tool.py .git/config'",
                "osascript -e 'do shell script \"python3 tool.py .git/config\"'",
                "frobnicate -c 'python3 tool.py .git/config'",
            ],
            true,
        );
        assert_public_allowed(
            &repo,
            &[
                "echo -e 'hello'",
                "echo -e \"python3 tool.py .claude/settings.json\"",
                "printf '%s\\n' 'python3 edit.py .claude/settings.json' > notes.md",
                "npm run x -- --flag",
                "cargo test -- --nocapture",
                "git commit -m \"python3 x\"",
                "cargo run -- python3 -c 'print(1)'",
                "make test",
                "expect -c 'spawn python3 tool.py notes.txt'",
                "osascript -e 'display notification \"done\"'",
                "frobnicate -m python3 settings.json",
                "awk '{print $1}' data.txt",
                "sed -e 's/a/b/' notes.txt",
                "grep -e 'python3 tool.py' notes.txt",
            ],
        );
    }

    /// Grok round 7 F1: a shell or interpreter that takes its code from a
    /// pipe, a here-string, a process substitution or a `-c` value holding a
    /// substitution runs code the guard cannot read in full, so every word
    /// on the line is judged by its trailing components. F2: a word loses
    /// one `{}` pair before Tcl runs it.
    #[test]
    fn code_the_guard_cannot_read_makes_every_word_judged() {
        let repo = repo();
        assert_public_refused(
            &repo,
            &[
                "python3 -c \"$(printf '%s\\n' 'open(\".claude/settings.json\",\"w\").write(\"x\")')\"",
                "bash <(printf '%s\\n' 'python3 tool.py .claude/settings.json')",
                "python3 <(printf '%s\\n' 'open(\".claude/settings.json\",\"w\").write(\"x\")')",
                "sh <<< 'python3 tool.py .claude/settings.json'",
                "bash <<< 'python3 tool.py .claude/settings.json'",
                "python3 <<< 'open(\".claude/settings.json\",\"w\").write(\"x\")'",
                "python3 - <<< 'open(\".claude/settings.json\",\"w\").write(\"x\")'",
                "node <<< 'require(\"fs\").writeFileSync(\".claude/settings.json\",\"\")'",
                "echo 'open(\".claude/settings.json\",\"w\").write(\"x\")' | python3",
                "sh <<< 'cd .claude && python3 tool.py settings.json'",
                "expect -c 'spawn python3 tool.py {.claude/settings.json}'",
            ],
            false,
        );
        assert_public_refused(
            &repo,
            &[
                "bash <(printf '%s\\n' 'python3 tool.py .git/config')",
                "python3 <(printf '%s\\n' 'open(\".git/config\").read()')",
                "sh <<< 'python3 tool.py .git/config'",
                "python3 <<< 'open(\".git/config\").read()'",
                "echo 'open(\".git/config\").read()' | python3",
                "expect -c 'spawn python3 tool.py {.git/config}'",
            ],
            true,
        );
        assert_public_allowed(
            &repo,
            &[
                "python3 <<< 'print(1)'",
                "echo 'print(1)' | python3",
                "echo -e \"python3 tool.py .claude/settings.json\"",
                "printf '%s\\n' 'python3 edit.py .claude/settings.json' > notes.md",
                "sh <<< 'python3 tool.py notes.txt'",
                "bash <(printf '%s\\n' 'python3 tool.py notes.txt')",
                "echo 'open(\"notes.txt\").read()' | python3",
                "expect -c 'spawn python3 tool.py {notes.txt}'",
                "cat .codeflow/policy.json | python3 -c 'import sys; print(sys.stdin.read())'",
                "python3 -c \"$(printf '%s\\n' 'print(1)')\"",
                "diff <(sort a.txt) <(sort b.txt)",
            ],
        );
    }

    /// Grok round 8: where a shell (`source` and `.` included) or an
    /// interpreter runs, every quoted string on the line is judged as
    /// possible code (F1 descriptors, `source <(...)`, fifos; F2 a literal
    /// `-c` that runs its stdin), and a descriptor program is code the
    /// guard cannot read. F3: the bare word `x` is not a remote ref.
    #[test]
    fn quoted_strings_are_judged_where_a_shell_or_interpreter_runs() {
        let repo = repo();
        assert_public_refused(
            &repo,
            &[
                "python3 /dev/stdin <<< 'open(\".claude/settings.json\",\"w\").write(\"x\")'",
                "python3 /dev/fd/3 3<<< 'open(\".claude/settings.json\",\"w\").write(\"x\")'",
                "exec 3<<< 'open(\".claude/settings.json\",\"w\").write(\"x\")'; python3 /dev/fd/3",
                "source <(echo 'python3 tool.py .claude/settings.json')",
                ". <(echo 'python3 tool.py .claude/settings.json')",
                "mkfifo p; echo 'open(\".claude/settings.json\",\"w\").write(\"x\")' > p & python3 p",
                "echo 'open(\".claude/settings.json\",\"w\").write(\"x\")' | python3 -c 'exec(sys.stdin.read())'",
                "python3 -c 'exec(sys.stdin.read())' <<< 'open(\".claude/settings.json\",\"w\").write(\"x\")'",
                "python3 -c 'import sys; exec(sys.stdin.read())' <<< 'open(\".claude/settings.json\",\"a\").write(\"ok\")'",
                "node -e 'eval(require(\"fs\").readFileSync(0,\"utf8\"))' <<< 'require(\"fs\").writeFileSync(\".claude/settings.json\",\"\")'",
                "python3 /proc/self/fd/0 <<< 'open(\".claude/settings.json\",\"w\").write(\"x\")'",
            ],
            false,
        );
        assert_public_refused(
            &repo,
            &[
                "python3 /dev/stdin <<< 'open(\".git/config\").read()'",
                "python3 /dev/fd/3 3<<< 'open(\".git/config\").read()'",
                "exec 3<<< 'open(\".git/config\").read()'; python3 /dev/fd/3",
                "source <(echo 'python3 tool.py .git/config')",
                ". <(echo 'python3 tool.py .git/config')",
                "mkfifo p; echo 'open(\".git/config\").read()' > p & python3 p",
                "echo 'open(\".git/config\").read()' | python3 -c 'exec(sys.stdin.read())'",
                "python3 -c 'exec(sys.stdin.read())' <<< 'open(\".git/config\").read()'",
                "python3 -c 'import sys; exec(sys.stdin.read())' <<< 'open(\".git/config\").read()'",
                "python3 <<< 'open(\".git/refs/remotes/origin/main\").read()'",
            ],
            true,
        );
        assert_public_allowed(
            &repo,
            &[
                "python3 /dev/stdin <<< 'print(1)'",
                "python3 /dev/fd/3 3<<< 'print(1)'",
                "source <(echo 'python3 tool.py notes.txt')",
                "python3 -c 'import sys; exec(sys.stdin.read())' <<< 'print(1)'",
                "cat .codeflow/policy.json | python3 -c 'import sys; print(sys.stdin.read())'",
                "python3 <<< 'print(x)'",
                "echo x | python3",
                "python3 <<< 'print(y)'",
                "echo -e \"python3 tool.py .claude/settings.json\"",
                "printf '%s\\n' 'python3 edit.py .claude/settings.json' > notes.md",
                "env python3 <<< 'print(1)'",
                "xargs python3 tool.py <<< 'notes.txt'",
                "python3 -c 'print(1)' | cat",
                "cat notes.txt | python3 -c 'import sys; print(len(sys.stdin.read()))'",
                "echo -e \"python3 tool.py .claude/settings.json\" | cat",
            ],
        );
    }

    /// Grok round 9: `$'...'` is decoded before a here-string body or a
    /// quoted string is split (F2); adjacent quoted pieces are judged as the
    /// one shell word they form, and a `printf` of `%s` pieces as the value
    /// it joins (F1, shell part).
    #[test]
    fn shell_quoting_and_printf_joins_are_judged_as_the_shell_builds_them() {
        let repo = repo();
        assert_public_refused(
            &repo,
            &[
                "python3 <<< $'open(\".cla\\x75de/settings.json\",\"w\").write(\"x\")'",
                "python3 <<< $'open(\"\\056claude/settings.json\",\"w\").write(\"x\")'",
                "printf -v p '%s%s' '.cla' 'ude/settings.json'; python3 -c \"open(\\\"$p\\\",\\\"w\\\").write(\\\"x\\\")\"",
                "printf -vp '%s%s' '.cla' 'ude/settings.json'; python3 tool.py \"$p\"",
                "echo '.cla''ude/settings.json' | python3 -c 'import sys; open(sys.stdin.read().strip(),\"w\")'",
                "python3 tool.py '.cla''ude/settings.json'",
                "python3 tool.py \".cla\"'ude/settings.json'",
                "x=\".cla\"'ude/settings.json'; python3 -c \"open('$x','w')\"",
            ],
            false,
        );
        assert_public_refused(
            &repo,
            &[
                "python3 <<< $'open(\".gi\\x74/config\").read()'",
                "printf -v p '%s%s' '.gi' 't/config'; python3 -c \"print(open(\\\"$p\\\").read())\"",
                "python3 tool.py '.gi''t/config'",
                "echo \".gi\"'t/config' | python3 -c 'import sys; print(open(sys.stdin.read().strip()).read())'",
            ],
            true,
        );
        assert_public_allowed(
            &repo,
            &[
                "python3 <<< $'print(17330\\x32)'",
                "python3 -c $'print(1)'",
                "printf -v p '%s%s' 'no' 'tes.txt'; python3 tool.py \"$p\"",
                "printf -v p '%s%s' '.cla' 'ude/settings.json'",
                "printf '%s\\n' 'python3 edit.py .claude/settings.json' > notes.md",
                "python3 tool.py 'no''tes.txt'",
                "cat .codeflow/policy.json | python3 -c 'import sys; print(sys.stdin.read())'",
                "python3 -c 'print(\"173\" \"302\")'",
            ],
        );
    }

    /// Grok round 10: a shell construct that composes a path from pieces
    /// the guard sees apart. Exact where cheap (printf escapes and `%b`,
    /// `echo -e`/`print` decoding, empty-separator joins of on-line
    /// assignments); cannot-verify otherwise on a line that runs a shell
    /// or interpreter.
    #[test]
    fn composed_shell_text_is_resolved_or_refused_as_cannot_verify() {
        let repo = repo();
        assert_public_refused(
            &repo,
            &[
                "printf '%b' '.cla\\x75de/settings.json' | python3 -c 'import sys; open(sys.stdin.read().strip(),\"w\")'",
                "printf -v p '%b' '.cla\\x75de/settings.json'; python3 tool.py \"$p\"",
                "printf '%s%b' '.cla' '\\x75de/settings.json' | python3 -c 'import sys; open(sys.stdin.read().strip(),\"w\")'",
                "printf '.cla\\x75de/settings.json\\n' | python3 -c 'import sys; open(sys.stdin.read().strip(),\"w\")'",
                "printf '%q%s' '.cla' 'ude/settings.json' | python3 -c 'import sys; open(sys.stdin.read().strip(),\"w\")'",
                "IFS=; arr=('.cla' 'ude/settings.json'); python3 -c \"open(\\\"${arr[*]}\\\",\\\"w\\\")\"",
                "a='.cla'; b='ude/settings.json'; python3 -c \"open(\\\"${a}${b}\\\",\\\"w\\\")\"",
                "arr=('.cla' 'ude/settings.json'); python3 -c \"open(\\\"${(j::)arr}\\\",\\\"w\\\")\"",
                "echo -e '.cla\\x75de/settings.json' | python3 -c 'import sys; open(sys.stdin.read().strip(),\"w\")'",
                "print '.cla\\x75de/settings.json' | python3 -c 'import sys; open(sys.stdin.read().strip(),\"w\")'",
            ],
            false,
        );
        assert_public_refused(
            &repo,
            &[
                "printf '%b' '.gi\\x74/config'; python3 tool.py \"$p\"",
                "printf '.gi\\x74/config\\n' | python3 -c 'import sys; print(open(sys.stdin.read().strip()).read())'",
                "IFS=; arr=('.gi' 't/config'); python3 -c \"print(open(\\\"${arr[*]}\\\").read())\"",
                "a='.gi'; b='t/config'; python3 -c \"print(open(\\\"${a}${b}\\\").read())\"",
                "echo -e '.gi\\x74/config' | python3 -c 'import sys; print(open(sys.stdin.read().strip()).read())'",
            ],
            true,
        );
        // A text-composing construct the guard cannot resolve, on a line
        // that runs an interpreter: cannot-verify.
        assert_public_refused(
            &repo,
            &[
                "printf '%d' 1 | python3 -c 'import sys; print(sys.stdin.read())'",
                "p=.claude; python3 -c \"open('${p/clau/cla}')\"",
                "x=abcdef; python3 -c \"print('${x:0:3}')\"",
                "arr=(a b); python3 -c \"print('${(U)arr}')\"",
            ],
            false,
        );
        assert_public_allowed(
            &repo,
            &[
                "printf '%b' 'notes.txt' | python3 -c 'import sys; print(sys.stdin.read())'",
                "printf '%s\\n' 'python3 edit.py .claude/settings.json' > notes.md",
                "printf -v p '%s%s' 'no' 'tes.txt'; python3 tool.py \"$p\"",
                "echo -e \"python3 tool.py .claude/settings.json\"",
                "print -r '.cla\\x75de/settings.json'",
                "IFS=; arr=('no' 'tes.txt'); python3 tool.py \"${arr[*]}\"",
                "arr=('no' 'tes.txt'); python3 tool.py \"${arr[*]}\"",
            ],
        );
    }

    /// Grok round 11: layer 1 is closed-world. One payload per construct
    /// family it used to resolve wrongly composes an enforcement path on a
    /// line where an interpreter runs, and refuses, resolved exactly or as
    /// cannot-verify; the same constructs over ordinary files stay allowed.
    #[test]
    fn each_composing_family_resolves_exactly_or_refuses() {
        let repo = repo();
        assert_public_refused(
            &repo,
            &[
                // An assignment whose value holds an expansion.
                "a=.cla; b=\"${a}ude/settings.json\"; python3 -c \"open('$b','w')\"",
                // A declaration builtin, and an indexed element.
                "export a=.cla; python3 -c \"open('${a}ude/settings.json','w')\"",
                "a[0]=.cla; python3 -c \"open('${a}ude/settings.json','w')\"",
                // An echo option cluster that decodes.
                "echo -ne '.cla\\x75de/settings.json' | python3 -c 'import sys; open(sys.stdin.read(),\"w\")'",
                // A printf format given through a variable.
                "f='.cla\\x75de/settings.json'; printf \"$f\" | python3 -c 'import sys; open(sys.stdin.read(),\"w\")'",
                // A unicode escape.
                "printf '.cla\\u0075de/settings.json' | python3 -c 'import sys; open(sys.stdin.read(),\"w\")'",
                // Indirect expansion.
                "n=p; p=.claude/settings.json; python3 -c \"open('${!n}','w')\"",
                // A command substitution over a modelled builtin.
                "python3 -c \"open('$(printf .cla)ude/settings.json','w')\"",
                // Round 12: a path-qualified program's output is outside
                // text, never its spelled arguments, so a fixed absolute
                // argument no longer hides the path the shell builds.
                "python3 -c \"open('$(/bin/cat /tmp/dir.txt)/.claude/settings.json','w')\"",
                // Round 13: a glob an output builtin writes is judged by
                // its pattern, directly and through a substitution.
                "echo .cl?ude/settings.js?n | python3 -c 'import sys; open(sys.stdin.read().strip(),\"w\")'",
                "echo .cla* | python3 -c 'import sys; open(sys.stdin.read().strip()+\"/settings.json\",\"w\")'",
                "python3 -c \"open('$(echo .[c]laude)'+'/settings.json','w')\"",
                // Round 14: the glob in a later field, beside outside text,
                // through a substitution, and as a later printf argument.
                "echo ok .cla* | python3 -c 'import sys; open(sys.stdin.read().split()[1]+\"/settings.json\",\"w\")'",
                "echo \"$TAG\" .cl?ude/settings.js?n | python3 -c 'import sys; open(sys.stdin.read().split()[-1],\"w\")'",
                "python3 -c \"open('$(echo ok .[c]laude)'.split()[1]+'/settings.json','w')\"",
                "printf '%s %s\\n' ok .cla* | python3 -c 'import sys; open(sys.stdin.read().split()[1]+\"/settings.json\",\"w\")'",
                // Round 15: an empty expansion inside one written word
                // joins the spelled directory and final component.
                "echo .claude/$(/usr/bin/true)settings.json | python3 -c 'import sys; print(sys.stdin.read())'",
                "print -r -- .claude/$(/bin/cat /dev/null)settings.json | python3 -c 'import sys; print(sys.stdin.read())'",
            ],
            false,
        );
        // The same join naming policy authority refuses at every level.
        assert_public_refused(
            &repo,
            &[
                "echo .git/$(/bin/cat /dev/null)config | python3 -c 'import sys; print(sys.stdin.read())'",
                "echo .git/${nope}config | python3 -c 'import sys; print(sys.stdin.read())'",
            ],
            true,
        );
        assert_public_allowed(
            &repo,
            &[
                "printf '%s\\n' 'python3 edit.py .claude/settings.json' > notes.md",
                "echo -e \"python3 tool.py .claude/settings.json\"",
                "print -r '.cla\\x75de/settings.json'",
                "IFS=; arr=('no' 'tes.txt'); python3 tool.py \"${arr[*]}\"",
                "a=no; b=\"${a}tes.txt\"; python3 tool.py \"$b\"",
                "export a=notes.txt; python3 tool.py \"$a\"",
                "f='%s%s\\n'; printf \"$f\" no tes.txt | python3 -c 'import sys; print(sys.stdin.read())'",
                "echo -ne 'notes.txt' | python3 -c 'import sys; print(sys.stdin.read())'",
                "find src -name '*.py' -exec sh -c 'python3 lint.py \"$1\"' sh {} \\;",
                "for f in src/*.py; do python3 lint.py \"$f\"; done",
                // Program output is outside text (operator boundary, 2026-10-15).
                "echo \"Done $(date)\"; python3 src/app.py",
                // Ordinary globs an output builtin writes.
                "echo src/*.py | python3 -c 'import sys; print(sys.stdin.read())'",
                "echo *.txt; python3 src/app.py",
                "echo ok src/*.py | python3 -c 'import sys; print(sys.stdin.read())'",
                "echo \"$TAG\" *.txt; python3 src/app.py",
                "echo notes$(/bin/cat /dev/null).txt | python3 -c 'import sys; print(sys.stdin.read())'",
                "echo notes.txt \"$(/bin/cat /dev/null)\"; python3 src/app.py",
                // Round 16: a quoted parameter named like a protected
                // component is a name, not spelled text.
                "echo \"$config\" | python3 -c 'import sys; print(sys.stdin.read())'",
                "echo \"$hooks\" | python3 -c 'import sys; print(sys.stdin.read())'",
                "echo \"$policy.json\" | python3 -c 'import sys; print(sys.stdin.read())'",
                "echo \"$settings.json\" | python3 -c 'import sys; print(sys.stdin.read())'",
            ],
        );
        assert_public_allowed(
            &repo,
            &["source venv/bin/activate && echo ok && python3 src/app.py"],
        );
    }
}
