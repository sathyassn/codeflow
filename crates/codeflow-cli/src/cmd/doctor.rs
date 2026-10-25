//! `codeflow doctor` — run the health checks and render the results.

use std::io::Write;

use clap::Args;
use codeflow_core::doctor::{self, Options, Status};
use codeflow_core::registry;

#[derive(Args)]
pub struct DoctorArgs {
    /// Run a single named check (see `doctor --list`).
    #[arg(long)]
    pub check: Option<String>,
    /// List available check names.
    #[arg(long)]
    pub list: bool,
    /// Print the seat, sandbox and tool evidence the orchestrator preflight
    /// reads, each value marked configured, observed or unavailable.
    #[arg(long, conflicts_with_all = ["check", "list"])]
    pub preflight: bool,
}

/// Run doctor checks; exit 0 when healthy (warnings allowed), 1 on failures.
pub fn run(args: &DoctorArgs) -> i32 {
    let home = registry::codeflow_home();
    // OS text rule (issue 79): doctor takes the project folder as text, so one
    // that is not valid UTF-8 is refused, not read as a lossy lookalike.
    let root = match super::repo_root() {
        Ok(root) => root,
        Err(error) => {
            eprintln!("codeflow: {error}", error = crate::cmd::shown(&error));
            return 2;
        }
    };
    if args.preflight {
        let mut out = std::io::stdout();
        for line in preflight_report(&root, &|name| find_on_path(name)) {
            let _ = term_writeln!(out, "{line}");
        }
        return 0;
    }
    let Some(project_dir) = root.to_str() else {
        eprintln!(
            "codeflow doctor: the project folder is not valid UTF-8, so it cannot be checked"
        );
        return 1;
    };
    let opts = Options {
        project_dir: project_dir.to_string(),
        qualification_dir: home.as_deref().map(registry::qualified_bindings_path),
        codeflow_home: home,
        // The grok guard canary judges the exec-guard in this process; it
        // never executes a binary, which a swapped path could answer for.
        guard_canary: Some(super::hook::exec_guard_canary),
        update_plan: Some(|root, dest| {
            codeflow_core::scaffold::update::decide(
                &crate::embedded::EmbeddedAssets,
                root,
                dest,
                env!("CARGO_PKG_VERSION"),
            )
        }),
        // Only reported beside where PATH resolves codeflow; never run.
        codeflow_exe: std::env::current_exe().ok(),
        ..Options::default()
    };
    run_with(args, &opts, &mut std::io::stdout())
}

/// Where the orchestrator's seat catalog is installed, in reading order.
const CATALOG_PATHS: [&str; 2] = [
    ".agents/skills/cf-model-orchestrator/resources/current-ensemble.json",
    ".claude/skills/cf-model-orchestrator/resources/current-ensemble.json",
];

/// The tools the preflight asks about beyond the seat executables.
const PREFLIGHT_TOOLS: [&str; 3] = ["git", "gh", "herdr"];

/// The executable a catalog harness runs, by its name's first word
/// (`claude-code` runs `claude`, `codex-cli` runs `codex`).
fn harness_executable(harness: &str) -> &str {
    harness.split('-').next().unwrap_or(harness)
}

/// The first `name` on `PATH`, as the operating system would find it; never
/// executed.
fn find_on_path(name: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|dir| {
        let candidate = dir.join(name);
        candidate.is_file().then_some(candidate)
    })
}

/// One executable's line: observed on `PATH`, or unavailable.
fn executable_line(
    label: &str,
    name: &str,
    look: &dyn Fn(&str) -> Option<std::path::PathBuf>,
) -> String {
    match look(name) {
        Some(path) => format!(
            "{label} {name}: observed on PATH at {path}; {run}",
            path = shown(&path.display()),
            // The builder profile line loads codex's config (TSK-1003).
            run = if name == "codex" {
                "doctor runs it only to load its config for the builder profile line"
            } else {
                "doctor does not run it"
            }
        ),
        None => format!("{label} {name}: unavailable: not found on PATH"),
    }
}

fn shown(text: &dyn std::fmt::Display) -> String {
    crate::cmd::shown(text)
}

/// The seat lines of the preflight report, from the installed catalog.
fn seat_lines(
    root: &std::path::Path,
    look: &dyn Fn(&str) -> Option<std::path::PathBuf>,
) -> Vec<String> {
    let Some((path, text)) = CATALOG_PATHS.iter().find_map(|path| {
        std::fs::read_to_string(root.join(path))
            .ok()
            .map(|text| (*path, text))
    }) else {
        return vec![format!(
            "seats: unavailable: no seat catalog at {}",
            CATALOG_PATHS[0]
        )];
    };
    let Ok(catalog) = serde_json::from_str::<serde_json::Value>(&text) else {
        return vec![format!("seats: unavailable: {path} is not valid JSON")];
    };
    let list = |key: &str| catalog[key].as_array().cloned().unwrap_or_default();
    let find = |key: &str, id: &str| {
        list(key)
            .into_iter()
            .find(|item| item["id"].as_str() == Some(id))
    };
    let mut lines = Vec::new();
    for seat_id in list("standing_seats")
        .iter()
        .filter_map(|seat| seat.as_str())
    {
        let Some(seat) = find("seats", seat_id) else {
            lines.push(format!(
                "seat {seat_id}: unavailable: {path} does not define it"
            ));
            continue;
        };
        let family_id = seat["family"].as_str().unwrap_or_default();
        let harness = find("families", family_id)
            .and_then(|family| family["harnesses"].as_array().cloned())
            .and_then(|harnesses| {
                harnesses
                    .last()
                    .and_then(|h| h.as_str().map(str::to_string))
            })
            .unwrap_or_default();
        let line_id = seat["lines"][0].as_str().unwrap_or_default();
        let configured = find("lines", line_id).and_then(|line| {
            let adopted = line["adopted_version"].as_str()?.to_string();
            let version = line["versions"]
                .as_array()?
                .iter()
                .find(|version| version["id"].as_str() == Some(adopted.as_str()))?
                .clone();
            let efforts: Vec<&str> = version["efforts"]
                .as_array()
                .map(|efforts| efforts.iter().filter_map(|e| e.as_str()).collect())
                .unwrap_or_default();
            Some(format!(
                "{adopted} through {harness}, efforts {efforts}",
                efforts = efforts.join("|")
            ))
        });
        lines.push(match configured {
            Some(configured) => format!("seat {seat_id}: configured {configured} ({path})"),
            None => format!(
                "seat {seat_id}: unavailable: line {line_id} has no adopted version in {path}"
            ),
        });
        if !harness.is_empty() {
            lines.push(executable_line(
                "  executable",
                harness_executable(&harness),
                look,
            ));
        }
        lines.push(format!(
            "  {seat_id} live model and effort: unavailable: doctor cannot observe what a running session selected; observe it natively in the seat"
        ));
    }
    if lines.is_empty() {
        lines.push(format!(
            "seats: unavailable: {path} names no standing seats"
        ));
    }
    lines
}

/// The `default_permissions` value and the permission profile names a Codex
/// `config.toml` declares, or `None` when it is not valid TOML.
fn codex_profiles(text: &str) -> Option<(Option<String>, Vec<String>)> {
    let config = text.parse::<toml::Table>().ok()?;
    let default = config
        .get("default_permissions")
        .and_then(toml::Value::as_str)
        .map(str::to_string);
    let profiles = config
        .get("permissions")
        .and_then(toml::Value::as_table)
        .map(|table| table.keys().cloned().collect())
        .unwrap_or_default();
    Some((default, profiles))
}

/// The sandbox lines: what the project settings request, never what a
/// running session enforces.
fn sandbox_lines(root: &std::path::Path) -> Vec<String> {
    let mut lines = Vec::new();
    let claude = ".claude/settings.json";
    match std::fs::read_to_string(root.join(claude))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
    {
        Some(settings) => {
            let flag = |key: &str| match settings["sandbox"][key].as_bool() {
                Some(value) => value.to_string(),
                None => "unset".to_string(),
            };
            lines.push(format!(
                "sandbox claude: configured enabled {}, failIfUnavailable {} ({claude})",
                flag("enabled"),
                flag("failIfUnavailable")
            ));
        }
        None => lines.push(format!("sandbox claude: unavailable: no readable {claude}")),
    }
    let codex = ".codex/config.toml";
    match std::fs::read_to_string(root.join(codex)) {
        Ok(text) => match codex_profiles(&text) {
            Some((default, profiles)) => lines.push(format!(
                "sandbox codex: configured default profile {default}, profiles {profiles} ({codex})",
                default = default.as_deref().unwrap_or("unset"),
                profiles = if profiles.is_empty() {
                    "none".to_string()
                } else {
                    profiles.join("|")
                }
            )),
            None => lines.push(format!(
                "sandbox codex: unavailable: {codex} is not valid TOML"
            )),
        },
        Err(_) => lines.push(format!("sandbox codex: unavailable: no readable {codex}")),
    }
    lines.push(
        "  effective boundary: unavailable: doctor cannot observe the boundary a running session enforces; check it natively in each seat"
            .to_string(),
    );
    lines
}

/// What the installed Codex says about the builder launch overrides
/// (TSK-1003). This is the one preflight value that runs a binary:
/// `codex --version` and `codex features list` with the overrides, which
/// load config and start no model, session or sandbox. An exit code of 0
/// shows only that Codex accepted the command; whether the project config
/// loaded is read from the feature table, and is otherwise unverified.
fn codex_builder_line(
    root: &std::path::Path,
    look: &dyn Fn(&str) -> Option<std::path::PathBuf>,
) -> String {
    use codeflow_core::doctor::{CodexLoad, ProjectLayer};
    let label = "codex builder profile";
    let config = std::fs::read_to_string(root.join(".codex/config.toml"))
        .ok()
        .and_then(|text| text.parse::<toml::Table>().ok());
    let defines_builder = config
        .as_ref()
        .and_then(|config| config.get("permissions"))
        .and_then(toml::Value::as_table)
        .is_some_and(|profiles| profiles.contains_key("cf-builder"));
    if !defines_builder {
        return format!("{label}: unavailable: .codex/config.toml defines no cf-builder profile");
    }
    let Some(codex) = look("codex") else {
        return format!("{label}: unavailable: codex not found on PATH");
    };
    let probe = codeflow_core::doctor::probe_codex_builder_profile(root, &codex);
    let version = match &probe.version {
        Ok(version) => version.clone(),
        Err(reason) => return format!("{label}: unavailable: {}", shown(reason)),
    };
    let version = shown(&version);
    let stage = |load: &CodexLoad, name: &str| match load {
        CodexLoad::Accepted(_) => None,
        CodexLoad::Rejected(reason) => Some((true, format!("rejected {name}: {reason}"))),
        CodexLoad::NotRun(reason) => Some((false, format!("not checked {name}: {reason}"))),
    };
    let problems: Vec<(bool, String)> = [
        stage(&probe.startup, "without a project config"),
        stage(&probe.project, "in the project folder"),
    ]
    .into_iter()
    .flatten()
    .collect();
    if problems.iter().any(|(rejected, _)| *rejected) {
        let text: Vec<&str> = problems.iter().map(|(_, text)| text.as_str()).collect();
        return format!(
            "{label}: observed REJECTED by {version}: {}; a builder seat launched as transport.md says will not open, so qualify the profile on this Codex before building",
            shown(&text.join("; "))
        );
    }
    if !problems.is_empty() {
        let text: Vec<&str> = problems.iter().map(|(_, text)| text.as_str()).collect();
        return format!("{label}: unavailable: {}", shown(&text.join("; ")));
    }
    let configured: std::collections::BTreeMap<String, bool> = config
        .as_ref()
        .and_then(|config| config.get("features"))
        .and_then(toml::Value::as_table)
        .map(|features| {
            features
                .iter()
                .filter_map(|(name, value)| value.as_bool().map(|value| (name.clone(), value)))
                .collect()
        })
        .unwrap_or_default();
    let accepted = format!(
        "{label}: observed {version} accepts the builder launch overrides without a project config, as the TUI startup load reads them, and in the project folder"
    );
    let boundary = "doctor cannot observe the boundary a running seat enforces";
    match probe.project_layer(&configured) {
        Some(ProjectLayer::Loaded { feature, value }) => format!(
            "{accepted}; the project config loaded there (features.{feature} = {value} appears only with it); {boundary}",
            feature = shown(&feature)
        ),
        Some(ProjectLayer::NotLoaded {
            feature,
            shown: seen,
            configured,
        }) => format!(
            "{accepted}, but Codex did not load the project config there (features.{feature} shows {seen}, the project sets {configured}); trust the project in Codex, or a builder seat runs the restricted startup profile; {boundary}",
            feature = shown(&feature)
        ),
        Some(ProjectLayer::Unverified) | None => format!(
            "{accepted}; whether Codex loaded the project config is unverified, because no feature the project sets differs from the user config; {boundary}"
        ),
    }
}

/// The preflight evidence report (TSK-452): each value is marked
/// `configured` (what project files request), `observed` (what doctor saw on
/// this host) or `unavailable` (what doctor cannot establish).
fn preflight_report(
    root: &std::path::Path,
    look: &dyn Fn(&str) -> Option<std::path::PathBuf>,
) -> Vec<String> {
    let mut lines = vec![
        "preflight evidence for cf-model-orchestrator; read it, then make the live observations the preflight requires".to_string(),
    ];
    lines.extend(seat_lines(root, look));
    lines.extend(sandbox_lines(root));
    lines.push(codex_builder_line(root, look));
    for tool in PREFLIGHT_TOOLS {
        lines.push(executable_line("tool", tool, look));
    }
    lines
}

/// The testable core of [`run`]: explicit options and output sink, no
/// process-global state except deduplication of unsupported-policy notices.
fn run_with(args: &DoctorArgs, opts: &Options, out: &mut dyn Write) -> i32 {
    if args.list {
        for name in doctor::check_names() {
            let _ = term_writeln!(out, "{name}");
        }
        return 0;
    }
    for note in codeflow_core::hooks::policy_schema::deprecation_warnings(std::path::Path::new(
        &opts.project_dir,
    ))
    .into_iter()
    .filter(|note| codeflow_core::hooks::policy_schema::first_unsupported_warning(&note.text))
    {
        let _ = term_writeln!(out, "note  policy: {}", note.text);
        let _ = term_writeln!(out, "      clear it: {}", note.remedy);
    }
    let results = if let Some(name) = &args.check {
        match doctor::run_check(name, opts) {
            Ok(r) => vec![r],
            Err(e) => {
                eprintln!("codeflow doctor: {e}", e = crate::cmd::shown(&e));
                return 1;
            }
        }
    } else {
        let rt = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                eprintln!("codeflow doctor: runtime: {e}", e = crate::cmd::shown(&e));
                return 1;
            }
        };
        rt.block_on(doctor::run_all(opts))
    };
    let mut failed = false;
    for r in &results {
        let badge = match r.status {
            Status::Pass => "ok  ",
            Status::Warn(_) => "warn",
            Status::Note(_) => "note",
            Status::Fail => {
                failed = true;
                "FAIL"
            }
        };
        let _ = term_writeln!(
            out,
            "{badge}  {name}: {message}",
            name = r.name,
            // A check message can quote a policy value, a ref or a path;
            // the sink rule keeps each further line indented (TSK-388).
            message = r.message
        );
        match &r.status {
            Status::Warn(remedy) => {
                let _ = term_writeln!(out, "      clear it: {}", remedy);
            }
            Status::Note(remedy) => {
                let _ = term_writeln!(out, "      confirm: {}", remedy);
            }
            Status::Pass | Status::Fail => {}
        }
    }
    i32::from(failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(check: Option<&str>, list: bool) -> DoctorArgs {
        DoctorArgs {
            check: check.map(ToString::to_string),
            list,
            preflight: false,
        }
    }

    fn run_to_string(a: &DoctorArgs, opts: &Options) -> (i32, String) {
        let mut buf = Vec::new();
        let code = run_with(a, opts, &mut buf);
        (code, String::from_utf8(buf).unwrap())
    }

    /// A tempdir with a valid initialized `.codeflow/` directory.
    fn initialized_project() -> (tempfile::TempDir, Options) {
        let dir = tempfile::tempdir().unwrap();
        let cf = dir.path().join(".codeflow");
        std::fs::create_dir_all(&cf).unwrap();
        std::fs::write(cf.join("policy.json"), "{\"schema_version\": 1}\n").unwrap();
        // A complete installed-file record: doctor refuses one it cannot
        // parse instead of skipping the checks that read it.
        std::fs::write(
            cf.join("manifest.json"),
            "{\"schema_version\": 1, \"scaffold_version\": \"3.1.0\", \"files\": {}}\n",
        )
        .unwrap();
        std::fs::write(
            cf.join("project.toml"),
            "schema_version = 1\ntier = \"standard\"\nscaffold_version = \"3.1.0\"\nstack = \"rust\"\nareas = [\"engine\"]\npolicy_armed = false\ngit_hooks = \"unwired\"\npermission_preset = \"acceptEdits\"\n",
        )
        .unwrap();
        let opts = Options {
            project_dir: dir.path().to_string_lossy().into_owned(),
            look_path: Some(|name| Ok(format!("/stub/bin/{name}"))),
            exec_command: Some(|_, args| {
                Ok(if args == ["git-hook", "capabilities"] {
                    "hooks 3".into()
                } else {
                    String::new()
                })
            }),
            exec_command_stdin: Some(|_, _, _| Ok(String::new())),
            guard_canary: None,
            update_plan: None,
            codeflow_exe: None,
            codeflow_home: None,
            qualification_dir: None,
            harness_home: Some(dir.path().join("home")),
            env_var: Some(|_| None),
        };
        (dir, opts)
    }

    #[test]
    fn list_prints_every_check_name_and_exits_zero() {
        let (code, out) = run_to_string(&args(None, true), &Options::default());
        assert_eq!(code, 0);
        let listed: Vec<&str> = out.lines().collect();
        assert_eq!(listed, doctor::check_names());
    }

    #[test]
    fn check_config_passes_on_initialized_tempdir() {
        let (_dir, opts) = initialized_project();
        let (code, out) = run_to_string(&args(Some("config"), false), &opts);
        assert_eq!(code, 0, "valid config must be healthy: {out}");
        assert!(out.starts_with("ok  "), "got: {out}");
        assert!(out.contains("config:"), "got: {out}");
        assert_eq!(out.lines().count(), 1, "single check renders one line");
    }

    #[test]
    fn check_config_fails_on_invalid_json() {
        let (dir, opts) = initialized_project();
        std::fs::write(dir.path().join(".codeflow/manifest.json"), "{broken").unwrap();
        let (code, out) = run_to_string(&args(Some("config"), false), &opts);
        assert_eq!(code, 1);
        assert!(out.contains("FAIL"), "got: {out}");
        assert!(out.contains("manifest.json"), "names the offender: {out}");
    }

    #[test]
    fn a_warning_prints_the_step_that_clears_it() {
        // SPC-013 R-80: an uninitialized project warns and names `codeflow init`.
        let dir = tempfile::tempdir().unwrap();
        let opts = Options {
            project_dir: dir.path().to_string_lossy().into_owned(),
            ..Options::default()
        };
        let (code, out) = run_to_string(&args(Some("config"), false), &opts);
        assert_eq!(code, 0, "a warning never fails doctor: {out}");
        assert!(out.starts_with("warn  config:"), "got: {out}");
        assert!(
            out.contains("\n      clear it: run `codeflow init` in the project root"),
            "got: {out}"
        );
    }

    #[test]
    fn check_unknown_name_exits_one() {
        let (code, out) = run_to_string(&args(Some("bogus"), false), &Options::default());
        assert_eq!(code, 1);
        assert!(out.is_empty(), "errors go to stderr, not the report");
    }

    #[test]
    fn full_run_healthy_renders_all_checks_and_exits_zero() {
        let (_dir, opts) = initialized_project();
        let (code, out) = run_to_string(&args(None, false), &opts);
        assert_eq!(code, 0, "all stubbed checks pass: {out}");
        for name in doctor::check_names() {
            assert!(
                out.contains(&format!("{name}:")),
                "{name} must render: {out}"
            );
        }
        assert!(
            out.contains("config: all .codeflow/ JSON files are valid"),
            "got: {out}"
        );
        assert!(
            out.contains("permissions: file permissions correct"),
            "got: {out}"
        );
    }

    #[test]
    fn r21_incomplete_existing_project_refuses() {
        let (dir, opts) = initialized_project();
        std::fs::write(
            dir.path().join(".codeflow/project.toml"),
            "tier = \"standard\"\n",
        )
        .unwrap();
        let (code, out) = run_to_string(&args(None, false), &opts);
        assert_eq!(
            code, 1,
            "incomplete existing state cannot be healthy: {out}"
        );
        assert!(out.contains("cannot read existing CodeFlow state"), "{out}");
        assert!(out.contains("project.toml"), "{out}");
    }

    #[test]
    fn full_run_with_failing_hooks_exits_one() {
        let (_dir, mut opts) = initialized_project();
        // Binary found but every hook subcommand probe fails.
        opts.exec_command = Some(|_, _| Err("boom".into()));
        let (code, out) = run_to_string(&args(None, false), &opts);
        assert_eq!(code, 1);
        assert!(out.contains("FAIL  hooks:"), "got: {out}");
        // Failure of one check never hides the others.
        assert!(out.contains("config:"), "got: {out}");
        assert!(out.contains("permissions:"), "got: {out}");
    }
}
