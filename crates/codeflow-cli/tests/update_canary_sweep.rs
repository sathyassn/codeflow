//! TSK-1029: `codeflow update` and `codeflow update --pin` never print a
//! value a project controls.
//!
//! The sweep plants one canary in every input those commands read: every
//! file a minimal install holds (the managed files, their baselines, the
//! policy, the manifest, the project state) and the registry in its home,
//! plus the inputs an install does not create on its own (a kept PR
//! template, a pending update record, task and epic records with and
//! without a recorded migration baseline). A baseline is planted together
//! with its file. Each input is planted five ways, one per run ([`Plant`]):
//!
//! - `Keys`: a canary key in every object or table, a canary item in every
//!   list of strings, and canary lines in a text file;
//! - `Values`: the canary added to every string value or text line;
//! - `Retyped`: every number and boolean replaced by the canary string (in
//!   a record, the whole frontmatter a quoted canary string);
//! - `Scalar`: the whole file one quoted canary string;
//! - `Malformed`: the file cut short and ended with an unterminated
//!   canary string (in a record, inside its frontmatter).
//!
//! The version cases plant it in `scaffold_version`, the `--pin` argument
//! and the release URL, and a served release whose checksum file and
//! downloads hold it.
//!
//! Every run checks stdout, stderr (the whole error chain the CLI prints)
//! and the `--diff` export for the canary, and checks that the run reached
//! its input: its output differs from a run on the unplanted install, or it
//! prints the diagnostic the case names. An input whose plant changes
//! nothing visible is listed in [`SILENT`] with the reason.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The canary every case plants.
const CANARY: &str = "opaque-neighbour-value-31337";

/// The canary in a checksum file: 64 hexadecimal digits, so it reads as a
/// SHA-256 digest.
const HEX_CANARY: &str = "c0ffee31337c0ffee31337c0ffee31337c0ffee31337c0ffee31337c0ffee313";

/// Planted inputs whose run prints nothing different from the unplanted
/// install (a part of the run's name, and its plant or empty for any), each
/// with why that is the expected outcome. A run listed here still must not
/// print the canary.
const SILENT: &[(&str, &str, &str)] = &[
    (
        "/sandbox.toml",
        "",
        "the Grok sandbox file is user-owned: init writes it once and update never reads it",
    ),
    (
        "proj/.codeflow/project.toml",
        "(Keys)",
        "update keeps keys of the project state it does not know, without a report",
    ),
    (
        ".gitignore",
        "(Keys)",
        "the lines added sit outside the managed region, which update keeps as the project's",
    ),
    (
        "AGENTS.md",
        "(Keys)",
        "the lines added sit outside the managed region, which update keeps as the project's",
    ),
    (
        "CLAUDE.md",
        "(Keys)",
        "the lines added sit outside the managed region, which update keeps as the project's",
    ),
    (
        "with a recorded migration baseline",
        "",
        "update reads the work records only to record the migration baseline, once",
    ),
];

fn codeflow_command(dir: &Path, home: &Path, args: &[&str]) -> Command {
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_codeflow"));
    let path = std::env::join_paths(exe.parent().map(Path::to_path_buf).into_iter().chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .expect("joinable PATH");
    let mut command = Command::new(&exe);
    command
        .args(args)
        .current_dir(dir)
        .env("CODEFLOW_HOME", home)
        .env("PATH", path)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .env_remove("CODEFLOW_RELEASE_URL")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("CODEFLOW_INTEGRATE_TOKEN")
        .env_remove("CODEFLOW_HUMAN_OVERRIDE");
    command
}

/// A minimal install with one commit, in `proj`, and its home, in `home`.
fn base() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let (root, home) = (dir.path().join("proj"), dir.path().join("home"));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    let out = codeflow_command(&root, &home, &["init", "--yes", "--minimal"])
        .output()
        .expect("codeflow runs");
    assert!(out.status.success(), "{}", text(&out));
    commit(&root);
    dir
}

/// Commits the whole tree (git2, so no hook runs).
fn commit(root: &Path) -> git2::Oid {
    let repo =
        git2::Repository::open(root).unwrap_or_else(|_| git2::Repository::init(root).unwrap());
    let mut index = repo.index().unwrap();
    index
        .add_all(
            ["*"],
            git2::IndexAddOption::DEFAULT | git2::IndexAddOption::FORCE,
            None,
        )
        .unwrap();
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let signature = git2::Signature::now("test", "test@example.com").unwrap();
    let parents: Vec<git2::Commit> = repo
        .head()
        .ok()
        .and_then(|head| head.peel_to_commit().ok())
        .into_iter()
        .collect();
    let parents: Vec<&git2::Commit> = parents.iter().collect();
    repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        "base",
        &tree,
        &parents,
    )
    .unwrap()
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

/// Every file under `dir` but `.git`, relative to it, with `/` separators.
fn files(dir: &Path, prefix: &str, out: &mut Vec<String>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let name = entry.file_name().to_string_lossy().to_string();
        let relative = format!("{prefix}{name}");
        if entry.file_type().unwrap().is_dir() {
            if name != ".git" {
                files(&entry.path(), &format!("{relative}/"), out);
            }
        } else {
            out.push(relative);
        }
    }
}

fn text(out: &Output) -> String {
    format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[derive(Clone, Copy, Debug)]
enum Plant {
    /// A canary key in every object, a canary item in every list of
    /// strings, canary lines in text.
    Keys,
    /// The canary added to every string value or text line.
    Values,
    /// Every number and boolean replaced by the canary string; in a record,
    /// the whole frontmatter a quoted canary string.
    Retyped,
    /// The whole file one quoted canary string.
    Scalar,
    /// The file cut short and ended with an unterminated canary string; in
    /// a record, an unterminated canary string in the frontmatter.
    Malformed,
}

const PLANTS: [Plant; 5] = [
    Plant::Keys,
    Plant::Values,
    Plant::Retyped,
    Plant::Scalar,
    Plant::Malformed,
];

/// The leaves of a structured file: a string, or a number or boolean.
enum Leaf<'a> {
    Text(&'a mut String),
    Other,
}

/// The planted leaf, or `None` to keep it.
fn leaf(leaf: Leaf<'_>, plant: Plant) -> Option<String> {
    match (leaf, plant) {
        (Leaf::Text(string), Plant::Values) => Some(format!("{string} {CANARY}")),
        (Leaf::Other, Plant::Retyped) => Some(CANARY.into()),
        _ => None,
    }
}

fn json(value: &mut serde_json::Value, plant: Plant) {
    use serde_json::Value;
    let planted = match value {
        Value::Object(map) => {
            for item in map.values_mut() {
                json(item, plant);
            }
            if matches!(plant, Plant::Keys) {
                map.insert(format!("{CANARY}-key"), Value::String(CANARY.into()));
            }
            None
        }
        Value::Array(items) => {
            for item in items.iter_mut() {
                json(item, plant);
            }
            if matches!(plant, Plant::Keys) && items.iter().all(Value::is_string) {
                items.push(Value::String(CANARY.into()));
            }
            None
        }
        Value::String(string) => leaf(Leaf::Text(string), plant),
        Value::Null => None,
        Value::Bool(_) | Value::Number(_) => leaf(Leaf::Other, plant),
    };
    if let Some(planted) = planted {
        *value = Value::String(planted);
    }
}

fn toml_value(value: &mut toml::Value, plant: Plant) {
    use toml::Value;
    let planted = match value {
        Value::Table(table) => {
            for (_, item) in table.iter_mut() {
                toml_value(item, plant);
            }
            if matches!(plant, Plant::Keys) {
                table.insert(format!("{CANARY}-key"), Value::String(CANARY.into()));
            }
            None
        }
        Value::Array(items) => {
            for item in items.iter_mut() {
                toml_value(item, plant);
            }
            if matches!(plant, Plant::Keys) && items.iter().all(Value::is_str) {
                items.push(Value::String(CANARY.into()));
            }
            None
        }
        Value::String(string) => leaf(Leaf::Text(string), plant),
        _ => leaf(Leaf::Other, plant),
    };
    if let Some(planted) = planted {
        *value = Value::String(planted);
    }
}

fn yaml(value: &mut serde_yaml::Value, plant: Plant) {
    use serde_yaml::Value;
    let planted = match value {
        Value::Mapping(map) => {
            for (_, item) in map.iter_mut() {
                yaml(item, plant);
            }
            if matches!(plant, Plant::Keys) {
                map.insert(
                    Value::String(format!("{CANARY}-key")),
                    Value::String(CANARY.into()),
                );
            }
            None
        }
        Value::Sequence(items) => {
            for item in items.iter_mut() {
                yaml(item, plant);
            }
            if matches!(plant, Plant::Keys) && items.iter().all(Value::is_string) {
                items.push(Value::String(CANARY.into()));
            }
            None
        }
        Value::String(string) => leaf(Leaf::Text(string), plant),
        Value::Null | Value::Tagged(_) => None,
        Value::Bool(_) | Value::Number(_) => leaf(Leaf::Other, plant),
    };
    if let Some(planted) = planted {
        *value = Value::String(planted);
    }
}

/// `text` cut short and ended with an unterminated canary string.
fn cut(text: &str) -> String {
    let mut end = text.len() / 2;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\"{CANARY}", &text[..end])
}

/// The frontmatter of a record and the rest, when `text` has one.
fn frontmatter(text: &str) -> Option<(&str, &str)> {
    let rest = text.strip_prefix("---\n")?;
    let end = rest.find("\n---\n")?;
    Some((&rest[..end], &rest[end + 5..]))
}

/// A text file planted, or `None` when the plant does not apply to it.
fn text_file(text: &str, plant: Plant) -> Option<String> {
    let front = frontmatter(text);
    Some(match (plant, front) {
        (Plant::Keys, Some((front, body))) => {
            format!("---\n{CANARY}-key: {CANARY}\n{front}\n---\n{body}\n## {CANARY} heading\n\n{CANARY}\n")
        }
        (Plant::Keys, None) => format!("{text}\n## {CANARY} heading\n\n{CANARY}\n"),
        (Plant::Values, _) => text
            .lines()
            .map(|line| {
                if line.is_empty() || line == "---" {
                    line.to_string()
                } else {
                    format!("{line} {CANARY}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        (Plant::Retyped, Some((_, body))) => format!("---\n\"{CANARY}\"\n---\n{body}"),
        (Plant::Retyped, None) => return None,
        (Plant::Scalar, _) => format!("\"{CANARY}\"\n"),
        (Plant::Malformed, Some((front, body))) => {
            format!("---\ntitle: \"x {CANARY}\n{front}\n---\n{body}")
        }
        (Plant::Malformed, None) => cut(text),
    })
}

/// `text`, the content of `name`, with the canary planted, or `None` when
/// the plant does not apply to it.
fn planted(name: &str, text: &str, plant: Plant) -> Option<String> {
    let structured = |rendered: Option<String>| match plant {
        Plant::Malformed => Some(cut(text)),
        Plant::Scalar => Some(format!("\"{CANARY}\"\n")),
        _ => rendered,
    };
    let extension = Path::new(name)
        .extension()
        .map(|extension| extension.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if extension == "json" {
        return structured(serde_json::from_str(text).ok().map(|mut value| {
            json(&mut value, plant);
            serde_json::to_string_pretty(&value).unwrap()
        }));
    }
    if extension == "toml" {
        return structured(toml::from_str(text).ok().map(|mut value| {
            toml_value(&mut value, plant);
            toml::to_string(&value).unwrap()
        }));
    }
    if extension == "yml" || extension == "yaml" {
        return structured(serde_yaml::from_str(text).ok().map(|mut value| {
            yaml(&mut value, plant);
            serde_yaml::to_string(&value).unwrap()
        }));
    }
    text_file(text, plant)
}

/// Changes a copy of the base install (its project, its home).
type Setup = Box<dyn Fn(&Path, &Path)>;

/// One run: a copy of the base install, changed by `setup`, then
/// `args` with the `--diff` export when the run is an update.
struct Run {
    name: String,
    setup: Setup,
    args: Vec<String>,
    url: Option<String>,
    /// Texts the run's own diagnostic prints; empty means "its output
    /// differs from the unplanted run".
    expect: Vec<&'static str>,
}

struct Outcome {
    outputs: [(&'static str, String); 3],
}

impl Outcome {
    fn shown(&self) -> String {
        format!("{}\n{}", self.outputs[0].1, self.outputs[1].1)
    }
}

fn run(base: &Path, case: &Run) -> Outcome {
    let dir = tempfile::tempdir().unwrap();
    copy_tree(base, dir.path());
    let (root, home) = (dir.path().join("proj"), dir.path().join("home"));
    (case.setup)(&root, &home);
    let export = dir.path().join("update.diff");
    let mut args: Vec<String> = case.args.clone();
    if !args.iter().any(|arg| arg == "--pin") {
        args.extend(["--diff".into(), export.to_string_lossy().to_string()]);
    }
    let mut command = codeflow_command(&root, &home, &[]);
    command.args(&args);
    if let Some(url) = &case.url {
        command.env("CODEFLOW_RELEASE_URL", url);
    }
    let out = command.output().expect("codeflow runs");
    let normalize = |text: String| text.replace(&*dir.path().to_string_lossy(), "<case>");
    Outcome {
        outputs: [
            (
                "stdout",
                normalize(String::from_utf8_lossy(&out.stdout).to_string()),
            ),
            (
                "stderr",
                normalize(String::from_utf8_lossy(&out.stderr).to_string()),
            ),
            (
                "export",
                normalize(std::fs::read_to_string(&export).unwrap_or_default()),
            ),
        ],
    }
}

fn write(root: &Path, relative: &str, text: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

const TASK: &str = "---\nid: TSK-001\nepic_id: EPC-001\nstandalone_reason: null\nintegration_target: main\ntitle: \"work\"\nstatus: planned\nwork_type: feat\nspecs: []\ndepends_on: []\ncreated: 2026-09-26\n---\n\n# TSK-001: work\n\n## Description\n\nWork.\n\n## Acceptance Criteria\n\n- AC-1 When run, the system shall work.\n";

const EPIC: &str = "---\nid: EPC-001\ntitle: \"outcome\"\nstatus: planning\nwork_type: feat\nspecs: []\ncreated: 2026-09-26\n---\n\n# EPC-001: outcome\n\n## Summary\n\nAn outcome.\n\n## Acceptance Criteria\n\n- AC-1 (journey) On a fresh project, an adopter shall finish the flow.\n";

const KEPT_TEMPLATE: &str = ".github/PULL_REQUEST_TEMPLATE/custom.md";

const PENDING: &str = ".codeflow/.pending-update/.codeflow/rules/writing.md.json";

/// Inputs an install does not create on its own: each is planted valid,
/// then planted with the canary. The second item sets up what the input
/// needs (a commit of the records, a recorded migration baseline).
/// An input an install does not create on its own.
struct Extra {
    path: &'static str,
    valid: &'static str,
    prepare: fn(&Path),
    label: &'static str,
}

fn extras() -> Vec<Extra> {
    fn none(_: &Path) {}
    fn baseline(root: &Path) {
        let head = commit(root);
        let path = root.join(".codeflow/project.toml");
        let state = std::fs::read_to_string(&path).unwrap();
        std::fs::write(
            &path,
            format!("work_records_baseline = \"{head}\"\n{state}"),
        )
        .unwrap();
    }
    const WITH_BASELINE: &str = " with a recorded migration baseline";
    let extra = |path, valid, prepare: fn(&Path), label| Extra {
        path,
        valid,
        prepare,
        label,
    };
    vec![
        extra(KEPT_TEMPLATE, "## Overview\n\n## Rollout\n", none, ""),
        extra(
            PENDING,
            "{\"old_sha256\": \"0\", \"new_sha256\": \"1\", \"shipped\": \"text\"}",
            none,
            "",
        ),
        extra("project-management/tasks/TSK-001.md", TASK, none, ""),
        extra("project-management/epics/EPC-001.md", EPIC, none, ""),
        extra(
            "project-management/tasks/TSK-001.md",
            TASK,
            baseline,
            WITH_BASELINE,
        ),
        extra(
            "project-management/epics/EPC-001.md",
            EPIC,
            baseline,
            WITH_BASELINE,
        ),
    ]
}

/// Every installed file and home file, and every extra input, planted each
/// way.
fn file_runs(base: &Path) -> Vec<Run> {
    let mut runs = Vec::new();
    let mut installed = Vec::new();
    files(&base.join("proj"), "proj/", &mut installed);
    files(&base.join("home"), "home/", &mut installed);
    for relative in installed {
        // A baseline is read beside its file: update merges from it only
        // where the file differs from the shipped version, so both carry
        // the canary.
        let mut targets = vec![relative.clone()];
        if let Some(live) = relative.strip_prefix("proj/.codeflow/.baseline/") {
            let live = format!("proj/{live}");
            if base.join(&live).is_file() {
                targets.push(live);
            }
        }
        let Ok(originals) = targets
            .iter()
            .map(|target| std::fs::read_to_string(base.join(target)))
            .collect::<Result<Vec<_>, _>>()
        else {
            continue;
        };
        let label = if targets.len() > 1 {
            format!("{relative} with its file")
        } else {
            relative.clone()
        };
        for plant in PLANTS {
            let texts: Vec<(String, String)> = targets
                .iter()
                .zip(&originals)
                .filter_map(|(target, original)| {
                    planted(target, original, plant).map(|text| (target.clone(), text))
                })
                .collect();
            if texts.is_empty() {
                continue;
            }
            runs.push(Run {
                name: format!("{label} ({plant:?})"),
                setup: Box::new(move |root, _| {
                    let case = root.parent().unwrap();
                    for (target, text) in &texts {
                        std::fs::write(case.join(target), text).unwrap();
                    }
                }),
                args: vec!["update".into()],
                url: None,
                expect: Vec::new(),
            });
        }
    }
    for Extra {
        path: relative,
        valid,
        prepare,
        label,
    } in extras()
    {
        for plant in PLANTS {
            let Some(text) = planted(relative, valid, plant) else {
                continue;
            };
            runs.push(Run {
                name: format!("{relative}{label} ({plant:?})"),
                setup: Box::new(move |root, _| {
                    write(root, relative, valid);
                    prepare(root);
                    write(root, relative, &text);
                }),
                args: vec!["update".into()],
                url: None,
                expect: Vec::new(),
            });
        }
    }
    runs
}

/// A release served from a folder, at version 99.0.0: a checksum file and
/// the archives it lists.
fn served(root: &Path, sums: &str, archive: &[u8]) -> String {
    let release = root.parent().unwrap().join(format!("release-{CANARY}"));
    let folder = release.join("v99.0.0");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("sha256.sum"), sums).unwrap();
    for triple in codeflow_core::scaffold::release_pin::TRIPLES {
        std::fs::write(
            folder.join(format!("codeflow-cli-{triple}.tar.xz")),
            archive,
        )
        .unwrap();
    }
    let path = release.to_string_lossy().replace('\\', "/");
    format!("file:///{}", path.trim_start_matches('/'))
}

fn sums(digest: &str) -> String {
    use std::fmt::Write as _;
    let mut sums = String::new();
    for triple in codeflow_core::scaffold::release_pin::TRIPLES {
        writeln!(sums, "{digest}  codeflow-cli-{triple}.tar.xz").unwrap();
    }
    sums
}

/// The version, the `--pin` argument, the release URL and the served
/// release.
fn version_runs(base: &Path) -> Vec<Run> {
    let state = std::fs::read_to_string(base.join("proj/.codeflow/project.toml")).unwrap();
    let pin_version = move |value: &'static str| -> Setup {
        let state = state.clone();
        Box::new(move |root, _| {
            let line = state
                .lines()
                .find(|line| line.starts_with("scaffold_version"))
                .expect("scaffold_version");
            let value = value.replace("CANARY", CANARY);
            std::fs::write(
                root.join(".codeflow/project.toml"),
                state.replace(line, &format!("scaffold_version = \"{value}\"")),
            )
            .unwrap();
        })
    };
    let update = || vec!["update".to_string()];
    let pin = |value: &str| vec!["update".to_string(), "--pin".into(), value.into()];
    vec![
        Run {
            name: "scaffold_version with a build suffix".into(),
            setup: pin_version("999.0.0+CANARY"),
            args: update(),
            url: None,
            expect: vec!["999.0.0 (with a suffix)"],
        },
        Run {
            name: "scaffold_version with a prerelease suffix".into(),
            setup: pin_version("999.0.0-CANARY"),
            args: update(),
            url: None,
            expect: vec!["999.0.0 (with a suffix)"],
        },
        Run {
            name: "scaffold_version not a version, with a pin".into(),
            setup: pin_version("x CANARY"),
            args: pin("99.0.0"),
            url: None,
            expect: vec!["scaffold_version is not a parseable version"],
        },
        Run {
            name: "pin argument".into(),
            setup: Box::new(|_, _| {}),
            args: pin(&format!("x {CANARY}")),
            url: None,
            expect: vec!["the `--pin` value is not a codeflow version"],
        },
        Run {
            name: "pin argument with a build suffix".into(),
            setup: Box::new(|_, _| {}),
            args: pin(&format!("99.0.0+{CANARY}")),
            url: Some(format!("file:///nonexistent/{CANARY}")),
            expect: vec!["sha256.sum"],
        },
        Run {
            name: "release URL with a credential, file download fails".into(),
            setup: Box::new(|_, _| {}),
            args: pin("99.0.0"),
            url: Some(format!("file:///nonexistent/{CANARY}?token={CANARY}")),
            expect: vec!["sha256.sum"],
        },
        Run {
            name: "release URL with userinfo, connection fails".into(),
            setup: Box::new(|_, _| {}),
            args: pin("99.0.0"),
            url: Some(format!("http://user:{CANARY}@127.0.0.1:9/releases")),
            expect: vec!["sha256.sum"],
        },
    ]
}

/// A served release: the checksum file lists the canary digest and the
/// archives hash to something else; or its lines are not checksum lines.
fn served_runs() -> Vec<(&'static str, String, Vec<u8>, &'static str)> {
    vec![
        (
            "served release whose checksum does not match",
            sums(HEX_CANARY),
            format!("archive {CANARY}").into_bytes(),
            "the archive hashes to",
        ),
        (
            "served release with unreadable checksum lines",
            format!("{CANARY} {CANARY}\n{}", sums(&format!("x{CANARY}"))),
            format!("archive {CANARY}").into_bytes(),
            "lists no checksum",
        ),
    ]
}

fn check(
    name: &str,
    outcome: &Outcome,
    clean: &Outcome,
    expect: &[&str],
    problems: &mut Vec<String>,
) {
    for (sink, text) in &outcome.outputs {
        for canary in [CANARY, HEX_CANARY] {
            if text.contains(canary) {
                problems.push(format!("{name}: {sink} shows the canary:\n{text}"));
            }
        }
    }
    let shown = outcome.shown();
    if expect.is_empty() {
        let silent = SILENT
            .iter()
            .any(|(input, plant, _)| name.contains(input) && name.ends_with(plant));
        let same = outcome
            .outputs
            .iter()
            .zip(&clean.outputs)
            .all(|((_, a), (_, b))| a == b);
        if same && !silent {
            problems.push(format!(
                "{name}: the run shows nothing the unplanted run does not; list it in SILENT \
                 with the reason, or make update read it:\n{shown}"
            ));
        }
    }
    for text in expect {
        if !shown.contains(text) {
            problems.push(format!(
                "{name}: its diagnostic did not fire ({text:?} missing):\n{shown}"
            ));
        }
    }
}

#[test]
fn update_and_pin_never_print_a_planted_canary() {
    let base = base();
    let clean = run(
        base.path(),
        &Run {
            name: "clean".into(),
            setup: Box::new(|_, _| {}),
            args: vec!["update".into()],
            url: None,
            expect: Vec::new(),
        },
    );
    let mut problems = Vec::new();
    let mut count = 0;
    for case in file_runs(base.path())
        .into_iter()
        .chain(version_runs(base.path()))
    {
        count += 1;
        let outcome = run(base.path(), &case);
        check(&case.name, &outcome, &clean, &case.expect, &mut problems);
    }
    for (name, sums, archive, expect) in served_runs() {
        count += 1;
        // The release is served from its own folder, named with the canary.
        let dir = tempfile::tempdir().unwrap();
        let url = served(&dir.path().join("proj"), &sums, &archive);
        let outcome = run(
            base.path(),
            &Run {
                name: name.into(),
                setup: Box::new(|_, _| {}),
                args: vec!["update".into(), "--pin".into(), "99.0.0".into()],
                url: Some(url),
                expect: vec![expect],
            },
        );
        check(name, &outcome, &clean, &[expect], &mut problems);
    }
    assert!(count > 100, "the sweep ran only {count} cases");
    assert!(
        problems.is_empty(),
        "{} of {count} runs failed:\n\n{}",
        problems.len(),
        problems.join("\n\n")
    );
}
