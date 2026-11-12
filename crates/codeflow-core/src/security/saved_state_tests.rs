//! The saved-state pass (TSK-1461): the whole-corpus differential first,
//! then the scope rows, the controls, the identity and readings rows, and
//! the writes each location and variable refuses through both hooks.

use super::*;
use crate::hooks::policy::PolicyLevel;
use crate::hooks::sandbox_state::Outcome;
use crate::portable_path::slashed;
use crate::security::startup::RULE;

/// A home and a project in a temporary folder.
struct Fixture {
    _dir: tempfile::TempDir,
    home: PathBuf,
    project: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let project = dir.path().join("home/work/project");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        Self {
            _dir: dir,
            home,
            project,
        }
    }

    /// The locations for this fixture's home, with no variable set.
    fn places(&self) -> Places {
        Places {
            startup: StartupEnv {
                home: Some(self.home.clone()),
                ..StartupEnv::default()
            },
            ..Places::default()
        }
    }

    /// The pass's messages for `command` run in the project.
    fn pass(&self, command: &str) -> Vec<String> {
        self.pass_with(command, &self.places())
    }

    fn pass_with(&self, command: &str, places: &Places) -> Vec<String> {
        pass_in(command, &self.project, places)
    }

    /// What exec-guard finds for `command` run in the project, the hook
    /// reading `places` as its environment.
    fn exec(&self, command: &str, places: &Places) -> Vec<Violation> {
        fixed(places.clone(), || exec_judge(command, &self.project))
    }

    /// What git-guard finds for `command` run in the project, the hook
    /// reading `places` as its environment.
    fn git(&self, command: &str, places: &Places) -> Vec<Violation> {
        fixed(places.clone(), || git_judge(command, &self.project))
    }
}

fn mkdir(path: &Path) {
    std::fs::create_dir_all(path).unwrap();
}

fn pass_in(command: &str, cwd: &Path, places: &Places) -> Vec<String> {
    evaluate(command, cwd, places)
        .into_iter()
        .map(|finding| {
            assert_eq!(finding.rule, RULE, "{command}");
            assert!(finding.level_fixed, "{command}");
            finding.message
        })
        .collect()
}

fn exec_judge(command: &str, cwd: &Path) -> Vec<Violation> {
    exec_judge_with(command, cwd, &Outcome::Relaxed)
}

fn exec_judge_with(command: &str, cwd: &Path, outcome: &Outcome) -> Vec<Violation> {
    crate::hooks::exec_guard::evaluate_ruled(
        command,
        true,
        &crate::hooks::policy::SecuritySection::default(),
        PolicyLevel::Block,
        cwd,
        cwd,
        outcome,
    )
}

fn git_judge(command: &str, cwd: &Path) -> Vec<Violation> {
    let policy = crate::hooks::policy::GitPolicy::default();
    let context = crate::hooks::git_guard::GuardContext {
        policy: &policy,
        current_branch: "task/x",
        integrate_token: false,
        pr_base_lookup: None,
        dir_target_lookup: None,
        alias_lookup: None,
        branch_lookup: None,
        discard_lookup: Some(&|_| Ok(None)),
        root_checkout: None,
    };
    crate::hooks::git_guard::evaluate_report_at(command, &context, cwd).violations
}

/// The saved-state findings among `found`.
fn saved(found: &[Violation]) -> Vec<String> {
    found
        .iter()
        .filter(|v| v.rule == RULE && v.message.contains("saved shell state"))
        .map(|v| v.message.clone())
        .collect()
}

/// A finding as the differential compares it.
fn shown_findings(found: &[Violation]) -> Vec<String> {
    found
        .iter()
        .map(|v| format!("{} {:?} {}", v.rule, v.level, v.message))
        .collect()
}

// ---------------------------------------------------------------------------
// The whole-corpus differential
// ---------------------------------------------------------------------------

/// Every command of the guard corpus: the forms `guard_forms` pins, the
/// text-guard invariant's payloads, and the lines of the hook tests that
/// name a configuration folder beside saved state, with the directory each
/// runs in.
fn corpus(t: &Path) -> Vec<(String, PathBuf)> {
    use crate::security::guard_forms as forms;
    let repo = t.join("repo");
    let mut out: Vec<(String, PathBuf)> = Vec::new();
    let mut add = |command: &str| out.push((command.to_string(), repo.clone()));
    for (a, b) in forms::COMPOSED_PAIRS
        .iter()
        .chain(forms::REVIEW_SYMLINK_PROBES)
        .chain(forms::PACKAGE_RUNNER_PAIRS)
        .chain(forms::HELP_PAIRS)
    {
        add(a);
        add(b);
    }
    for command in forms::ROOTED_DELETIONS
        .iter()
        .chain(forms::RUN_TIME_PROJECT_DELETIONS)
        .chain(forms::PROJECT_DELETIONS)
        .chain(forms::NESTINGS)
        .chain(forms::SPACED_INPUT_FORMS)
    {
        add(command);
    }
    for (_, command, _) in forms::REVIEW_PROBES {
        add(command);
    }
    for (_, command, _) in forms::REVIEW_ROUND_TWO_PROBES
        .iter()
        .chain(forms::REVIEW_ROUND_THREE_PROBES)
        .chain(forms::REVIEW_ROUND_FOUR_PROBES)
    {
        add(command);
    }
    for (command, _) in forms::XARGS_OPTION_FORMS {
        add(command);
    }
    let root = slashed(&repo);
    for command in [
        format!("HOME={root}; git clone https://example.test/x/y.git ~/.claude/skills/x"),
        format!("export HOME={root}; git clone https://example.test/x/y.git ~/.claude/skills/x"),
        format!("HOME={root}; rm -rf ~/.claude"),
        format!("HOME={root}; printf x > ~/.claude/settings.json"),
        format!("HOME={root}; printf x > \"$HOME/.claude/settings.json\""),
        format!("HOME={root}; git init ~/.claude/x"),
        "HOME=$UNPROVEN; git clone https://example.test/x/y.git ~/scratch/y".to_string(),
        "HOME=/zz PATH=\"$HOME/bin:$PATH\" git status".to_string(),
        format!("HOME={root} bash -c 'rm -rf ~/.claude'"),
        format!("HOME={root}; python3 tool.py ~/.claude/settings.json"),
        "printf x > ~/.claude/settings.json".to_string(),
        "printf x > ~/.claude/notes.md".to_string(),
        "cp a.json ~/.claude/settings.json".to_string(),
        "rm -rf ~/.claude/projects".to_string(),
        "rm -rf ~/.claude".to_string(),
        "git clone https://example.test/x/y.git ~/.claude/skills/x".to_string(),
        "echo x >> ~/.codex/config.toml".to_string(),
        "cp t.toml ~/.config/herdr-core/config.toml".to_string(),
        "tar -xf a.tar -C ~/.claude/skills".to_string(),
    ] {
        add(&command);
    }
    let rows: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(invariant().join("rows.json")).unwrap())
            .unwrap();
    let root = slashed(t);
    for row in rows.as_array().unwrap() {
        let command = row["command"].as_str().unwrap().replace("@T@", &root);
        let cwd = row["cwd"].as_str().unwrap().replace("@T@", &root);
        out.push((command, PathBuf::from(cwd)));
    }
    out
}

fn invariant() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codeflow-cli/tests/fixtures/text-guard-invariant")
}

/// Rebuild the invariant's fixture tree under `t`: its folders, files and
/// links, without a repository.
fn invariant_tree(t: &Path) {
    let tree: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(invariant().join("tree.json")).unwrap())
            .unwrap();
    std::fs::create_dir_all(t.join("repo")).unwrap();
    std::fs::create_dir_all(t.join("home")).unwrap();
    for item in tree.as_array().unwrap() {
        let at = t.join(item["path"].as_str().unwrap());
        if let Some(parent) = at.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        match item["kind"].as_str().unwrap() {
            "dir" => std::fs::create_dir_all(&at).unwrap(),
            "file" => std::fs::write(&at, item["text"].as_str().unwrap_or_default()).unwrap(),
            "link" => {
                #[cfg(unix)]
                std::os::unix::fs::symlink(item["target"].as_str().unwrap(), &at).unwrap();
            }
            _ => {}
        }
    }
}

/// Whether `on` is `off` followed only by saved-state findings.
fn only_appended(off: &[String], on: &[String]) -> bool {
    on.get(..off.len()) == Some(off)
        && on[off.len()..]
            .iter()
            .all(|f| f.starts_with(&format!("{RULE} ")))
}

/// TSK-1461 class proof: over the whole corpus, both hooks, both outcomes,
/// the findings with the pass turned off are the guard as it stood before,
/// and turning it on keeps that list as it is, in order, and adds only
/// saved-state findings after it. The hooks read a fixture home with the
/// saved-state folders in place.
#[test]
fn saved_state_findings_follow_every_other_finding_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let t = dir.path();
    invariant_tree(t);
    let home = t.join("home");
    for folder in [
        ".claude/session-env/abc",
        ".claude/shell-snapshots",
        ".codex/shell_snapshots",
        ".config/herdr-core/templates",
    ] {
        std::fs::create_dir_all(home.join(folder)).unwrap();
    }
    let places = Places {
        startup: StartupEnv {
            home: Some(home),
            ..StartupEnv::default()
        },
        ..Places::default()
    };
    let rows = corpus(t);
    assert!(rows.len() > 2500, "corpus has {} rows", rows.len());
    let outcomes = [
        Outcome::Relaxed,
        Outcome::Strict("unsandboxed retry".into()),
    ];
    let chunks: Vec<&[(String, PathBuf)]> = rows.chunks(rows.len().div_ceil(4)).collect();
    let problems: Vec<String> = std::thread::scope(|scope| {
        let handles: Vec<_> = chunks
            .into_iter()
            .map(|chunk| {
                let outcomes = &outcomes;
                let places = places.clone();
                scope.spawn(move || {
                    fixed(places, || {
                        let mut problems = Vec::new();
                        for (command, cwd) in chunk {
                            for outcome in outcomes {
                                let judge = || exec_judge_with(command, cwd, outcome);
                                let off = shown_findings(&without(judge));
                                let on = shown_findings(&judge());
                                if !only_appended(&off, &on) {
                                    problems.push(format!(
                                        "exec-guard {outcome:?} {command}\n  off: {off:?}\n  on:  {on:?}"
                                    ));
                                } else if on.len() > off.len() {
                                    // Shown with `--nocapture`: each row
                                    // the pass adds a refusal to.
                                    eprintln!(
                                        "added exec-guard {outcome:?} {command}: {:?} (before: {})",
                                        &on[off.len()..],
                                        off.len()
                                    );
                                }
                            }
                            let judge = || git_judge(command, cwd);
                            let off = shown_findings(&without(judge));
                            let on = shown_findings(&judge());
                            if !only_appended(&off, &on) {
                                problems.push(format!(
                                    "git-guard {command}\n  off: {off:?}\n  on:  {on:?}"
                                ));
                            } else if on.len() > off.len() {
                                eprintln!(
                                    "added git-guard {command}: {:?} (before: {})",
                                    &on[off.len()..],
                                    off.len()
                                );
                            }
                        }
                        problems
                    })
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|handle| handle.join().unwrap())
            .collect()
    });
    assert!(
        problems.is_empty(),
        "{} rows changed:\n{}",
        problems.len(),
        problems.join("\n")
    );
}

// ---------------------------------------------------------------------------
// Scope: a target is judged where the command that names it runs
// ---------------------------------------------------------------------------

/// Each operation the pass judges, with a relative operand, run from each
/// directory-change form. `D` is the Claude configuration folder and `D/x`
/// the Codex home, so `D/session-env` and `D/x/shell_snapshots` are saved
/// state; the project holds none of it. A form whose directory change ends
/// before the operation adds no finding; a form whose operation runs in `D`
/// refuses with the operation's own message.
#[test]
fn a_target_is_judged_in_the_directory_its_command_runs_in() {
    let f = Fixture::new();
    let d = f.project.join("D");
    mkdir(&d.join("x/shell_snapshots"));
    mkdir(&d.join("session-env"));
    std::fs::write(d.join("session-env/f"), "x").unwrap();
    mkdir(&d.join("src"));
    let places = Places {
        claude_config: Some(d.clone()),
        codex_home: Some(d.join("x")),
        ..f.places()
    };
    let x = slashed(&d.join("x"));
    let operations: Vec<(&str, String)> = vec![
        (
            "rm -rf x",
            format!("`rm` removes `x`, which is or holds saved shell state: `{x}`"),
        ),
        (
            "rmdir session-env",
            "`rmdir` removes `session-env`, which is saved shell state: `$CLAUDE_CONFIG_DIR/session-env`"
                .into(),
        ),
        (
            "unlink session-env/f",
            "`unlink` removes `session-env/f`, which is saved shell state: `$CLAUDE_CONFIG_DIR/session-env`"
                .into(),
        ),
        (
            "find x -delete",
            format!("`find` removes `x`, which is or holds saved shell state: `{x}`"),
        ),
        (
            "cp -r src x/shell_snapshots",
            "`cp` places files into `x/shell_snapshots`, which is, holds or lies in saved shell state: `$CODEX_HOME/shell_snapshots`"
                .into(),
        ),
        (
            "mv x y",
            format!("`mv` moves `x`, which is or holds saved shell state: `{x}`"),
        ),
        (
            "tar -xf a.tar -C x",
            format!(
                "`tar` extracts into `{x}`, a folder above the saved shell state `$CODEX_HOME/shell_snapshots`, and the guard cannot see what the archive holds: extract into a named sub-folder that is not above saved shell state (`-C <dir>`)"
            ),
        ),
        (
            "rsync -a src/ x/shell_snapshots",
            "`rsync` places files into `x/shell_snapshots`, which is, holds or lies in saved shell state: `$CODEX_HOME/shell_snapshots`"
                .into(),
        ),
        (
            "echo x > session-env/f",
            "a redirect writes `session-env/f`, which is saved shell state: `$CLAUDE_CONFIG_DIR/session-env`"
                .into(),
        ),
    ];
    let unbound = [
        "(cd D; pwd); OP",
        "cd D | cat; OP",
        "p=$(cd D; pwd); OP",
        "sh -c 'cd D'; OP",
        "env -C D true; OP",
        "pushd D; popd; OP",
        "OP; cd D",
    ];
    let bound = [
        "cd D && OP",
        "{ cd D; }; OP",
        "(cd D && OP)",
        "env -C D OP",
        "sh -c 'cd D; OP'",
        "pushd D; OP",
    ];
    let mut wrong = Vec::new();
    for (op, message) in &operations {
        for form in unbound {
            let command = form.replace("OP", op);
            let found = f.pass_with(&command, &places);
            if !found.is_empty() {
                wrong.push(format!("refused: {command}: {found:?}"));
            }
        }
        for form in bound {
            let command = form.replace("OP", op);
            let found = f.pass_with(&command, &places);
            // The shell opens a redirect where the line runs, before `env`
            // changes directory for its program.
            if form.starts_with("env -C") && op.contains('>') {
                if !found.is_empty() {
                    wrong.push(format!("refused: {command}: {found:?}"));
                }
                continue;
            }
            if found != [message.clone()] {
                wrong.push(format!("{command}: {found:?}"));
            }
        }
    }
    assert!(wrong.is_empty(), "wrong verdicts:\n{}", wrong.join("\n"));
}

/// A directory or operand the reader cannot resolve leaves the other
/// checks' judgment as it is; a command run from several directories that
/// each hold a location refuses in each.
#[test]
fn an_unresolved_target_keeps_the_other_checks_judgment() {
    let f = Fixture::new();
    let d = f.home.join("D");
    mkdir(&d.join("x/shell_snapshots"));
    let places = Places {
        codex_home: Some(d.join("x")),
        ..f.places()
    };
    std::fs::write(f.project.join("README"), "x").unwrap();
    let to_d = slashed(&d);
    for command in [
        "cd \"$X\"; rm -rf *".to_string(),
        "rm -rf \"$X\"".to_string(),
        "eval \"$Y\"; rm -rf *".to_string(),
        format!("pushd {to_d}; popd; rm -rf x"),
        "rm -rf \"$UNSET\"/x".to_string(),
        "echo x > \"$UNSET\"/shell_snapshots/s.sh".to_string(),
    ] {
        assert_eq!(
            f.pass_with(&command, &places),
            Vec::<String>::new(),
            "{command}"
        );
        assert_eq!(
            shown_findings(&f.exec(&command, &places)),
            shown_findings(&without(|| f.exec(&command, &places))),
            "{command}"
        );
    }
    // Every reading refuses: both directories hold a location below `x`.
    let e = f.home.join("E");
    mkdir(&e.join("x/session-env"));
    let places = Places {
        codex_home: Some(d.join("x")),
        claude_config: Some(e.join("x")),
        ..f.places()
    };
    let command = format!(
        "if c; then cd {to_d}; else cd {}; fi; rm -rf x",
        slashed(&e)
    );
    assert_eq!(f.pass_with(&command, &places).len(), 2, "{command}");
}

/// The assignment forms a prefix may take do not hide the program after
/// them.
#[test]
fn assignment_prefixes_keep_the_program_after_them() {
    let f = Fixture::new();
    mkdir(&f.home.join(".codex/shell_snapshots"));
    let codex = slashed(&f.home.join(".codex"));
    for prefix in ["X=1", "X+=1", "a[0]=1"] {
        let command = format!("cd ~ && {prefix} rm -rf .codex");
        assert_eq!(
            f.pass(&command),
            [format!(
                "`rm` removes `.codex`, which is or holds saved shell state: `{codex}`"
            )],
            "{command}"
        );
        let command = format!("{prefix} cp evil.sh ~/.codex/shell_snapshots/s.sh");
        assert_eq!(
            f.pass(&command),
            ["`cp` writes `~/.codex/shell_snapshots/s.sh`, which is saved shell state: `~/.codex/shell_snapshots`"],
            "{command}"
        );
    }
}

// ---------------------------------------------------------------------------
// Controls: ordinary lines still run
// ---------------------------------------------------------------------------

/// Lines that ran before the pass existed still run: a removal or
/// extraction in a project, a copy of a project into the home, a folder of
/// the user's own named like a saved-state folder, and the neighbours of
/// saved state in a configuration folder.
#[test]
fn ordinary_lines_still_run() {
    let f = Fixture::new();
    for folder in [
        ".claude/session-env/abc",
        ".claude/shell-snapshots",
        ".claude/projects/p",
        ".codex/shell_snapshots",
        ".config/herdr-core/templates",
    ] {
        mkdir(&f.home.join(folder));
    }
    std::fs::write(f.project.join("README"), "x").unwrap();
    mkdir(&f.project.join("a"));
    mkdir(&f.project.join("target/debug"));
    mkdir(&f.project.join("build/out"));
    mkdir(&f.project.join("notes/session-env"));
    mkdir(&f.project.join("notes/shell_snapshots"));
    mkdir(&f.project.join("deep/a/b"));
    mkdir(&f.home.join("downloads"));
    let mut wrong = Vec::new();
    for command in [
        "rm -rf target",
        "rm -rf target build",
        "(cd /; pwd); rm -rf *",
        "(cd /usr; pwd); tar xf a.tar",
        "(cd /usr; ls); cp -r a b",
        "pushd /usr; popd; rm -rf build",
        "tar xf a.tar",
        "cd deep/a/b && tar -xzf ../../../a.tgz",
        "unzip -o t.zip -d deep/a",
        "tar -xf a.tar -C ~/downloads",
        "unzip t.zip -d ~/downloads",
        "cp -r a ~",
        "cp -r . ~/backup",
        "rm -rf notes/session-env",
        "echo x > notes/session-env/x.sh",
        "cp a/x notes/shell_snapshots/s.sh",
        "rm -rf notes",
        "rm -rf ~/.claude/projects/p",
        "echo x > ~/.claude/notes.md",
        "cp t.toml ~/.config/herdr-core/config.toml",
        "cat ~/.claude/session-env/abc/x.sh",
        "ls ~/.codex/shell_snapshots",
        "grep -r PATH ~/.claude/shell-snapshots",
        "tar -tf a.tar -C ~",
        "rm -rf ~/.codex/absent",
        "echo ~/.claude/session-env/x",
    ] {
        let found = f.pass(command);
        if !found.is_empty() {
            wrong.push(format!("{command}: {found:?}"));
        }
        for outcome in [
            Outcome::Relaxed,
            Outcome::Strict("unsandboxed retry".into()),
        ] {
            let found = fixed(f.places(), || {
                exec_judge_with(command, &f.project, &outcome)
            });
            if !saved(&found).is_empty() {
                wrong.push(format!("exec-guard {outcome:?} {command}: {found:?}"));
            }
        }
    }
    assert!(wrong.is_empty(), "refused:\n{}", wrong.join("\n"));
}

/// A location counts for a removal only once it exists: removing a
/// configuration folder with no saved state in it runs.
#[test]
fn a_removal_counts_a_location_only_once_it_exists() {
    let f = Fixture::new();
    let codex = f.home.join(".codex");
    mkdir(&codex);
    assert!(f.pass("rm -rf ~/.codex").is_empty());
    assert!(f.pass("mv ~/.codex ~/.codex-old").is_empty());
    mkdir(&codex.join("shell_snapshots"));
    let shown = slashed(&codex);
    assert_eq!(
        f.pass("rm -rf ~/.codex"),
        [format!(
            "`rm` removes `~/.codex`, which is or holds saved shell state: `{shown}`"
        )]
    );
    assert_eq!(
        f.pass("mv ~/.codex ~/.codex-old"),
        [format!(
            "`mv` moves `~/.codex`, which is or holds saved shell state: `{shown}`"
        )]
    );
    // A plain removal of a file or an empty folder removes only the path.
    assert!(f.pass("rmdir ~/.codex").is_empty());
    assert_eq!(
        f.pass("rmdir ~/.codex/shell_snapshots"),
        ["`rmdir` removes `~/.codex/shell_snapshots`, which is saved shell state: `~/.codex/shell_snapshots`"]
    );
}

// ---------------------------------------------------------------------------
// Extraction above saved state
// ---------------------------------------------------------------------------

/// An extraction into a folder above a location refuses whether or not the
/// location exists yet, and names the way through; one into a project or
/// into a folder of the home above no location runs.
#[test]
fn an_extraction_above_saved_state_refuses() {
    let f = Fixture::new();
    mkdir(&f.project.join("deep/a/b"));
    mkdir(&f.home.join("downloads"));
    mkdir(&f.home.join(".claude"));
    mkdir(&f.home.join(".config"));
    let home = slashed(&f.home);
    let refuse = [
        (
            "tar -xf a.tar -C ~",
            home.clone(),
            "~/.claude/session-env",
            "-C <dir>",
        ),
        (
            "cd ~ && tar xf work/a.tar",
            home.clone(),
            "~/.claude/session-env",
            "-C <dir>",
        ),
        (
            "unzip -o t.zip -d ~",
            home.clone(),
            "~/.claude/session-env",
            "-d <dir>",
        ),
        (
            "tar -xf a.tar -C ~/.claude",
            format!("{home}/.claude"),
            "~/.claude/session-env",
            "-C <dir>",
        ),
        (
            "bsdtar -xf a.tar -C ~/.claude",
            format!("{home}/.claude"),
            "~/.claude/session-env",
            "-C <dir>",
        ),
        (
            "tar -xf a.tar -C ~/.config",
            format!("{home}/.config"),
            "~/.config/herdr-core/templates",
            "-C <dir>",
        ),
        (
            "unzip t.zip -d ~/.config",
            format!("{home}/.config"),
            "~/.config/herdr-core/templates",
            "-d <dir>",
        ),
    ];
    let mut wrong = Vec::new();
    for (command, at, label, way) in refuse {
        // `bsdtar` is read, and named, as `tar`.
        let name = if command.contains("unzip") {
            "unzip"
        } else {
            "tar"
        };
        let expected = format!(
            "`{name}` extracts into `{at}`, a folder above the saved shell state `{label}`, and the guard cannot see what the archive holds: extract into a named sub-folder that is not above saved shell state (`{way}`)"
        );
        let found = f.pass(command);
        if found.first() != Some(&expected) {
            wrong.push(format!("{command}: {found:?}"));
        }
    }
    for command in [
        "cd deep/a/b && tar -xf a.tar",
        "tar -xf a.tar -C deep/a/b",
        "unzip -o t.zip -d deep/a",
        "tar -xf a.tar -C ~/downloads",
        "unzip -o t.zip -d ~/downloads",
        "cd ~/downloads && tar xf a.tar",
        "tar -xf a.tar -C ~/downloads/new",
    ] {
        let found = f.pass(command);
        if !found.is_empty() {
            wrong.push(format!("refused: {command}: {found:?}"));
        }
    }
    assert!(wrong.is_empty(), "wrong verdicts:\n{}", wrong.join("\n"));
}

/// A tree placed into a location, or over one, refuses; a file or a tree
/// that lands beside it runs.
#[test]
fn a_tree_placed_into_saved_state_refuses() {
    let f = Fixture::new();
    mkdir(&f.home.join(".claude/session-env/abc"));
    mkdir(&f.project.join("src"));
    mkdir(&f.project.join("session-env"));
    let mut wrong = Vec::new();
    for command in [
        "cp -r src ~/.claude/session-env",
        "cp -R src/. ~/.claude/session-env/abc",
        "rsync -a src/ ~/.claude/session-env/",
        "ditto src ~/.claude/session-env/abc",
        "cp -r session-env ~/.claude",
        "mv session-env ~/.claude/",
        "tar -xf a.tar -C ~/.claude/session-env",
    ] {
        if f.pass(command).len() != 1 {
            wrong.push(format!("{command}: {:?}", f.pass(command)));
        }
    }
    for command in [
        "cp -r src ~/.claude",
        "rsync -a src ~/.claude/",
        "cp -r src ~/.claude/backup",
    ] {
        let found = f.pass(command);
        if !found.is_empty() {
            wrong.push(format!("refused: {command}: {found:?}"));
        }
    }
    assert!(wrong.is_empty(), "wrong verdicts:\n{}", wrong.join("\n"));
}

// ---------------------------------------------------------------------------
// Identity: the node the kernel reaches
// ---------------------------------------------------------------------------

/// A target is the node the kernel reaches: through a link before its last
/// component, through `..` after a `cd` to a link, and in another case on
/// a file system that folds case.
#[cfg(unix)]
#[test]
fn a_target_is_the_node_the_kernel_reaches() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    let state = f.home.join(".claude/session-env");
    mkdir(&state.join("abc"));
    symlink(&state, f.project.join("link")).unwrap();
    symlink(state.join("abc"), f.project.join("into")).unwrap();
    let label = "~/.claude/session-env";
    assert_eq!(
        f.pass("echo x > link/hook.sh"),
        [format!(
            "a redirect writes `link/hook.sh`, which is saved shell state: `{label}`"
        )]
    );
    assert_eq!(
        f.pass("cp evil.sh link/abc/hook.sh"),
        [format!(
            "`cp` writes `link/abc/hook.sh`, which is saved shell state: `{label}`"
        )]
    );
    assert_eq!(
        f.pass("cd into && echo x > ../hook.sh"),
        [format!(
            "a redirect writes `../hook.sh`, which is saved shell state: `{label}`"
        )]
    );
    // The lexical reading of `into/../hook.sh` is the project's own file;
    // the kernel reaches the saved state.
    assert_eq!(f.pass("cp evil.sh into/../hook.sh").len(), 1);
    // A link to the state removed is the link itself; with a trailing `/`
    // the removal follows it.
    assert!(f.pass("rm -rf link").is_empty());
    assert_eq!(f.pass("rm -rf link/").len(), 1);
    // A link on the way to a location is a way to it: removing it refuses.
    symlink(f.home.join(".claude"), f.project.join("conf")).unwrap();
    let places = Places {
        claude_config: Some(f.project.join("conf")),
        ..f.places()
    };
    assert_eq!(
        f.pass_with("rm -rf conf", &places),
        [format!(
            "`rm` removes `conf`, which is or holds saved shell state: `{}`",
            slashed(&f.project.join("conf"))
        )]
    );
    // A file system that folds case reaches the same node.
    let folded = f.home.join(".CLAUDE");
    if folded.exists() {
        assert_eq!(f.pass("echo x > ~/.CLAUDE/Session-Env/hook.sh").len(), 1);
    }
}

/// With no file identity to compare, as on Windows, the pass compares
/// readings: as written, with `.` and `..` folded, and as reached through
/// links. That still finds a direct write, one through a link before the
/// last component, one through `..` after a link, a removal of a folder
/// above, and a variable the guard holds.
#[cfg(unix)]
#[test]
fn readings_find_direct_and_linked_targets() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    let state = f.home.join(".claude/session-env");
    mkdir(&state.join("abc"));
    symlink(&state, f.project.join("link")).unwrap();
    symlink(state.join("abc"), f.project.join("into")).unwrap();
    let places = Places {
        claude_env_file: Some(state.join("abc/hook-1.sh")),
        ..f.places()
    };
    by_readings(|| {
        for command in [
            "echo x > ~/.claude/session-env/abc/hook.sh",
            "echo x > link/hook.sh",
            "cd into && echo x > ../hook.sh",
            "cp evil.sh into/../hook.sh",
            "rm -rf ~/.claude",
            "echo x >> \"$CLAUDE_ENV_FILE\"",
        ] {
            assert_eq!(f.pass_with(command, &places).len(), 1, "{command}");
        }
        for command in [
            "rm -rf link",
            "echo x > ~/.claude/notes.md",
            "rm -rf target",
        ] {
            assert!(f.pass_with(command, &places).is_empty(), "{command}");
        }
    });
}

/// A location the guard cannot read is compared by its readings, so a
/// write into it or a removal above it still refuses.
#[cfg(unix)]
#[test]
fn a_location_the_guard_cannot_read_fails_closed() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let claude = f.home.join(".claude");
    mkdir(&claude.join("session-env"));
    std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o000)).unwrap();
    // A user the permission does not stop (root) reads it anyway.
    let unreadable = std::fs::symlink_metadata(claude.join("session-env")).is_err();
    let write = f.pass("echo x > ~/.claude/session-env/hook.sh");
    let removal = f.pass("rm -rf ~/.claude");
    std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755)).unwrap();
    if unreadable {
        assert_eq!(
            write,
            ["a redirect writes `~/.claude/session-env/hook.sh`, which is saved shell state: `~/.claude/session-env`"]
        );
        assert_eq!(removal.len(), 1, "{removal:?}");
    }
}

// ---------------------------------------------------------------------------
// The variables that move saved state
// ---------------------------------------------------------------------------

/// A configuration folder the session sets moves Claude Code's saved state
/// with it; the default folder stays saved state too, and the rest of the
/// moved folder is ordinary.
#[test]
fn a_non_default_claude_configuration_folder_is_saved_state() {
    let f = Fixture::new();
    let profile = f.home.join("Library/profiles/one/home");
    mkdir(&profile.join("session-env/abc"));
    mkdir(&profile.join("shell-snapshots"));
    let vars = |name: &str| match name {
        "HOME" => Some(f.home.clone().into_os_string()),
        "CLAUDE_CONFIG_DIR" => Some(profile.clone().into_os_string()),
        _ => None,
    };
    let places = Places::from_vars(&vars, &f.project);
    assert_eq!(places.claude_config.as_deref(), Some(profile.as_path()));
    let p = slashed(&profile);
    let mut wrong = Vec::new();
    for (command, label) in [
        (
            format!("echo x >> {p}/session-env/abc/hook.sh"),
            "$CLAUDE_CONFIG_DIR/session-env",
        ),
        (
            "echo x >> \"$CLAUDE_CONFIG_DIR/session-env/abc/hook.sh\"".to_string(),
            "$CLAUDE_CONFIG_DIR/session-env",
        ),
        (
            format!("cp evil.sh {p}/shell-snapshots/snapshot-zsh-1.sh"),
            "$CLAUDE_CONFIG_DIR/shell-snapshots",
        ),
        (
            "printf x > ~/.claude/session-env/hook.sh".to_string(),
            "~/.claude/session-env",
        ),
    ] {
        let found = f.pass_with(&command, &places);
        if found.len() != 1 || !found[0].ends_with(&format!("saved shell state: `{label}`")) {
            wrong.push(format!("{command}: {found:?}"));
        }
    }
    assert_eq!(
        f.pass_with(&format!("tar -xf a.tar -C {p}"), &places),
        [format!(
            "`tar` extracts into `{p}`, a folder above the saved shell state `$CLAUDE_CONFIG_DIR/session-env`, and the guard cannot see what the archive holds: extract into a named sub-folder that is not above saved shell state (`-C <dir>`)"
        )]
    );
    for command in [
        format!("echo x > {p}/notes.md"),
        format!("cp a.json {p}/settings.json"),
    ] {
        let found = f.pass_with(&command, &places);
        if !found.is_empty() {
            wrong.push(format!("refused: {command}: {found:?}"));
        }
    }
    assert!(wrong.is_empty(), "wrong verdicts:\n{}", wrong.join("\n"));
}

/// The five locations below the home and the four variables, each written
/// by a redirect, `tee`, `cp`, `mv` and `ln`, refuse through exec-guard and
/// git-guard; with the pass turned off, neither hook refuses them.
#[test]
#[allow(clippy::too_many_lines)] // one table of rows and its loop
fn saved_state_writes_refuse_through_both_hooks() {
    let f = Fixture::new();
    let elsewhere = f.home.join("elsewhere");
    let mut rows: Vec<(Places, String, String)> = Vec::new();
    let mut entries = vec![
        (".claude/session-env/abc/hook.sh", "~/.claude/session-env"),
        (
            ".claude/shell-snapshots/snapshot-zsh-1.sh",
            "~/.claude/shell-snapshots",
        ),
        (".codex/shell_snapshots/s.1.sh", "~/.codex/shell_snapshots"),
        (
            ".config/herdr-core/templates/build.toml",
            "~/.config/herdr-core/templates",
        ),
        (
            "AppData/Roaming/herdr-core/templates/build.toml",
            "~/AppData/Roaming/herdr-core/templates",
        ),
    ];
    for (target, label) in entries.drain(..) {
        rows.push((f.places(), format!("~/{target}"), label.to_string()));
    }
    let var = |name: &'static str, value: PathBuf| {
        let value = value.into_os_string();
        let home = f.home.clone().into_os_string();
        Places::from_vars(
            &move |asked| match asked {
                "HOME" => Some(home.clone()),
                _ if asked == name => Some(value.clone()),
                _ => None,
            },
            &f.project,
        )
    };
    rows.push((
        var("CLAUDE_CONFIG_DIR", elsewhere.join("claude")),
        slashed(&elsewhere.join("claude/session-env/hook.sh")),
        "$CLAUDE_CONFIG_DIR/session-env".into(),
    ));
    rows.push((
        var("CODEX_HOME", elsewhere.join("codex")),
        slashed(&elsewhere.join("codex/shell_snapshots/s.sh")),
        "$CODEX_HOME/shell_snapshots".into(),
    ));
    rows.push((
        var("CLAUDE_ENV_FILE", elsewhere.join("env/hook-1.sh")),
        slashed(&elsewhere.join("env/hook-1.sh")),
        "$CLAUDE_ENV_FILE".into(),
    ));
    if cfg!(windows) {
        rows.push((
            var("APPDATA", elsewhere.join("roaming")),
            slashed(&elsewhere.join("roaming/herdr-core/templates/build.toml")),
            "$APPDATA/herdr-core/templates".into(),
        ));
    }
    std::fs::write(f.project.join("evil.sh"), "x").unwrap();
    let mut wrong = Vec::new();
    for (places, target, label) in &rows {
        for (command, message) in [
            (
                format!("echo 'export PATH=/tmp/x:$PATH' >> {target}"),
                format!("a redirect writes the saved shell state file `{label}`"),
            ),
            (
                format!("tee -a {target} < /dev/null"),
                format!("`tee` targets the saved shell state file `{label}`"),
            ),
            (
                format!("cp evil.sh {target}"),
                format!("`cp` writes the saved shell state file `{label}`"),
            ),
            (
                format!("mv evil.sh {target}"),
                format!("`mv` writes the saved shell state file `{label}`"),
            ),
            (
                format!("ln -sf /tmp/evil.sh {target}"),
                format!("`ln` writes the saved shell state file `{label}`"),
            ),
        ] {
            both_hooks(&f, places, &command, &[message], &mut wrong);
        }
    }
    // One row for each form the guard reads a write by. `P` is a Claude
    // configuration folder the session moved outside the home, and `L`
    // the location in it, so no row spells the home or a startup folder.
    // The rows from the removal on are refusals near saved state: each
    // names its way through.
    let conf = f.home.parent().unwrap().join("conf");
    mkdir(&conf.join("session-env/abc"));
    mkdir(&conf.join("projects/p"));
    std::fs::write(conf.join("session-env/h"), "x").unwrap();
    mkdir(&f.project.join("s"));
    mkdir(&f.project.join("src/old"));
    mkdir(&f.project.join("target"));
    let moved = Places {
        claude_config: Some(conf.clone()),
        ..f.places()
    };
    let p = slashed(&conf);
    let l = format!("{p}/session-env");
    let label = "$CLAUDE_CONFIG_DIR/session-env";
    // The text rule names the location by its full path, as it compares it.
    let needle = key(&lexical(&conf.join("session-env")));
    let near = "aim the program at a named sub-folder that holds no saved shell state, or name the file it writes";
    let forms: Vec<(String, String)> = vec![
        (
            format!("echo x > {l}/h"),
            format!("a redirect writes the saved shell state file `{label}`"),
        ),
        (
            format!("dd of={l}/h"),
            format!("`dd` targets the saved shell state file `{label}`"),
        ),
        (
            format!("curl --output-dir {l} -o h https://example.test/x"),
            format!("`curl` targets the saved shell state file `{label}`"),
        ),
        (
            format!("cp e {l}/h"),
            format!("`cp` writes the saved shell state file `{label}`"),
        ),
        (
            format!("ln {l}/h a"),
            format!("`ln` targets the saved shell state file `{label}`"),
        ),
        (
            format!("cp --parents e {l}/h"),
            format!("`cp` writes the saved shell state file `{label}`"),
        ),
        (
            format!("find . -fprint {l}/h"),
            format!("`find` targets the saved shell state file `{label}`"),
        ),
        (
            format!("cd {l} && curl -O https://example.test/x"),
            format!("`curl` runs in `{l}`, where it can place a saved shell state file it does not name; {near}"),
        ),
        (
            format!("python3 -c \"open('{l}/h','w')\""),
            format!("interpreter code names the saved shell state file `{needle}`, which it can write"),
        ),
        (
            format!("echo 'x > {l}/h' | sh"),
            format!("text this call produces names the saved shell state file `{needle}`, and the line runs `sh`, which is not a data reader, so it could run or apply that text; read the file in its own call"),
        ),
        (
            format!("cp e {p}/*/h"),
            format!("`cp` writes the saved shell state file `{label}`"),
        ),
        (
            format!("PAGER='sh {l}/h' git log"),
            format!("`PAGER` is set to a command that names the saved shell state file `{needle}`, which a program the line runs would execute"),
        ),
        (
            format!("echo x >! {l}/h"),
            format!("a redirect writes the saved shell state file `{label}`"),
        ),
        (
            format!("echo x >>! {l}/h"),
            format!("a redirect writes the saved shell state file `{label}`"),
        ),
        (
            format!("echo x >>| {l}/h"),
            format!("a redirect writes the saved shell state file `{label}`"),
        ),
        (
            format!("rm -rf {p}"),
            format!("`rm` removes `{p}`, which is or holds saved shell state: `{p}`"),
        ),
        (
            format!("echo x > {p}/$f"),
            format!("a redirect writes `{p}/$f`, which the guard cannot resolve, and its directory is `{p}`; {near}"),
        ),
        (
            format!("cp -r s/. {p}/"),
            format!("`cp` copies the contents of `s/.` into `{p}/`, where saved shell state files live; copy into a named sub-folder that holds no saved shell state"),
        ),
        (
            format!("wget -P {p} https://example.test/x"),
            format!("`wget` writes into `{p}`, where it can place a saved shell state file it does not name; {near}"),
        ),
        (
            format!("cd {p} && tar xf a.tar"),
            format!("`tar` runs in `{p}`, where it can place a saved shell state file it does not name; {near}"),
        ),
        (
            format!("cd {p} && patch -p1 < fix.diff"),
            format!("`patch` runs in `{p}`, where it can place a saved shell state file it does not name; {near}"),
        ),
        (
            format!("find {p} -name x -exec chmod 600 {{}} +"),
            format!("`find` writes into `{p}`, where it can place a saved shell state file it does not name; {near}"),
        ),
    ];
    for (command, message) in &forms {
        both_hooks(&f, &moved, command, &[message.clone()], &mut wrong);
    }
    // A held variable whose value has a space, used unquoted: zsh does not
    // split it, so the write lands in the location.
    let spaced = f.home.join("Library/Application Support/claude");
    mkdir(&spaced.join("session-env"));
    let places = Places {
        claude_config: Some(spaced),
        ..f.places()
    };
    both_hooks(
        &f,
        &places,
        "echo x > $CLAUDE_CONFIG_DIR/session-env/h",
        &["a redirect writes the saved shell state file `$CLAUDE_CONFIG_DIR/session-env`".to_string()],
        &mut wrong,
    );
    // Controls near saved state: a named file beside it, a removal of a
    // neighbour, removals in a project and a search that runs nothing.
    for command in [
        format!("echo x > {p}/notes.md"),
        format!("rm -rf {p}/projects/p"),
        "rm -rf target".to_string(),
        "rm -rf src/old".to_string(),
        format!("find {p} -name x"),
    ] {
        both_hooks(&f, &moved, &command, &[], &mut wrong);
    }
    assert!(wrong.is_empty(), "wrong verdicts:\n{}", wrong.join("\n"));
}

/// Check that `command` gives exactly `messages` as its saved-state
/// findings through exec-guard and git-guard, and none with the pass
/// turned off.
fn both_hooks(
    f: &Fixture,
    places: &Places,
    command: &str,
    messages: &[String],
    wrong: &mut Vec<String>,
) {
    for (hook, found, before) in [
        (
            "exec-guard",
            f.exec(command, places),
            without(|| f.exec(command, places)),
        ),
        (
            "git-guard",
            f.git(command, places),
            without(|| f.git(command, places)),
        ),
    ] {
        if saved(&found) != messages {
            wrong.push(format!("{hook} {command}: {:?}", shown_findings(&found)));
        }
        if !saved(&before).is_empty() {
            wrong.push(format!("{hook} refused before: {command}"));
        }
    }
}

/// The lead in Director ruling C: main's judge returns the first finding a
/// segment gives, so with `git.hook_integrity` at warn, a warn-level git
/// finding in an earlier segment is all it returns, and a write to a shell
/// startup file later on the line gives no finding of its own. This row
/// only reads main's judge, with main's class, and changes nothing.
/// Settled: confirmed. The startup write is hidden behind the warn
/// finding; fixing that is main's, outside this unit.
#[test]
fn a_warn_level_git_finding_comes_before_a_later_startup_write() {
    let f = Fixture::new();
    let env = StartupEnv {
        home: Some(f.home.clone()),
        ..StartupEnv::default()
    };
    let found = crate::security::startup::evaluate_with(
        "git config core.hooksPath /tmp/h; cp evil.sh ~/.zshrc",
        &f.project,
        &env,
        PolicyLevel::Warn,
    );
    let rules: Vec<(String, PolicyLevel)> =
        found.iter().map(|v| (v.rule.clone(), v.level)).collect();
    assert_eq!(
        rules,
        [("git.hook_integrity".to_string(), PolicyLevel::Warn)],
        "{:?}",
        shown_findings(&found)
    );
}

/// Each finding of the pass is one always-blocking `security.shell_startup`
/// finding, and each hook puts it after every other finding.
#[test]
fn the_hooks_append_saved_state_findings_last() {
    let f = Fixture::new();
    mkdir(&f.home.join(".codex/shell_snapshots"));
    let message = format!(
        "`rm` removes `{0}/.codex`, which is or holds saved shell state: `{0}/.codex`",
        slashed(&f.home)
    );
    let command = format!("rm -rf {}/.codex", slashed(&f.home));
    for found in [f.exec(&command, &f.places()), f.git(&command, &f.places())] {
        let last = found.last().expect("a finding");
        assert_eq!(last.rule, RULE);
        assert!(last.level_fixed);
        assert_eq!(last.message, message);
    }
    assert!(saved(&without(|| f.exec(&command, &f.places()))).is_empty());
}
