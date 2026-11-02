//! `codeflow models limits`, `record-limits` and `launch-env`: the account
//! profile and the limit readings of each family (TSK-431). Read-only apart
//! from `record-limits`, which writes one small file under the project's
//! untracked local folder. Nothing here reads, copies or stores a token.

use std::io::Read;
use std::path::Path;

use clap::{Args, ValueEnum};
use codeflow_core::registry;
use codeflow_core::seats::accounts::{self, Profile};
use codeflow_core::seats::limits::{self, FamilyLimits, WindowView};
use codeflow_core::seats::{self, Family};
use serde_json::json;

/// A family a seat can run.
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum FamilyArg {
    /// Claude Code.
    Claude,
    /// Codex.
    Codex,
    /// Grok Build.
    Grok,
}

impl FamilyArg {
    fn family(self) -> Family {
        match self {
            FamilyArg::Claude => Family::Claude,
            FamilyArg::Codex => Family::Codex,
            FamilyArg::Grok => Family::Grok,
        }
    }
}

#[derive(Debug, Args)]
pub struct LimitsArgs {
    /// Print the readings and accounts as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
pub struct RecordArgs {
    /// Family the payload on standard input comes from.
    #[arg(long, value_enum)]
    family: FamilyArg,
    /// Grok only: record that Grok reported a usage limit, which is its only
    /// signal. Reads nothing from standard input.
    #[arg(long)]
    limit_hit: bool,
    /// With --limit-hit: Unix time (seconds) the error says the limit resets.
    #[arg(long, value_name = "EPOCH", requires = "limit_hit")]
    resets_at: Option<u64>,
    /// Config home of the seat that produced the payload. Defaults to the
    /// family's own config-home variable in this process, or the CLI's
    /// default home when that is unset. A reading is shown only for the
    /// config home the current profile names.
    #[arg(long, value_name = "DIR")]
    config_home: Option<std::path::PathBuf>,
}

#[derive(Debug, Args)]
pub struct LaunchEnvArgs {
    /// Family whose seat is about to launch.
    #[arg(long, value_enum)]
    family: FamilyArg,
    /// Print the variable and folder as JSON instead of a shell line.
    #[arg(long, conflicts_with = "check")]
    json: bool,
    /// Do not print a launch line: succeed only when this process's
    /// environment already matches the profile (the mapped variable is the
    /// mapped folder; for an unmapped family its variable is unset).
    #[arg(long)]
    check: bool,
}

fn load_profile(root: &Path) -> Result<Option<Profile>, String> {
    let home = registry::codeflow_home();
    accounts::load(
        root,
        home.as_deref(),
        codeflow_core::portable_path::user_home().as_deref(),
    )
}

pub fn launch_env(args: &LaunchEnvArgs) -> i32 {
    let family = args.family.family();
    let variable = family.config_home_var();
    let result = super::repo_root()
        .and_then(|root| load_profile(&root))
        .map(|profile| profile.and_then(|p| p.launch_env(family)));
    let env = match result {
        Ok(env) => env,
        Err(reason) => {
            eprintln!(
                "models launch-env: {reason}",
                reason = super::shown(&reason)
            );
            return 2;
        }
    };
    if args.check {
        let inherited = std::env::var_os(variable);
        return match (&env, inherited) {
            (Some(env), Some(value)) if std::path::Path::new(&value) == env.config_home => {
                println!("{variable} is the mapped config home");
                0
            }
            (None, None) => {
                println!(
                    "{variable} is unset; the {} seat uses the CLI's default home",
                    family.id()
                );
                0
            }
            (Some(env), _) => {
                eprintln!(
                    "models launch-env: {variable} is not the mapped config home {}; stop the launch",
                    codeflow_core::git::display_path(&env.config_home)
                );
                2
            }
            (None, Some(_)) => {
                eprintln!(
                    "models launch-env: {variable} is set but the profile maps no config home for {}; \
                     stop the launch (run the unset line from `launch-env`)",
                    family.id()
                );
                2
            }
        };
    }
    let Some(env) = env else {
        if args.json {
            println!(
                "{}",
                json!({"family": family.id(), "variable": variable, "config_home": null})
            );
        } else {
            println!("unset {variable}");
        }
        return 0;
    };
    if args.json {
        let value = json!({
            "family": family.id(),
            "variable": env.variable,
            "config_home": codeflow_core::git::display_path(&env.config_home),
        });
        println!("{value}");
        return 0;
    }
    match env.shell_export() {
        Ok(line) => {
            println!("{line}");
            0
        }
        Err(reason) => {
            eprintln!(
                "models launch-env: {reason}",
                reason = super::shown(&reason)
            );
            2
        }
    }
}

pub fn record(args: &RecordArgs) -> i32 {
    let family = args.family.family();
    let outcome = (|| -> Result<Option<String>, String> {
        let (source, reading) = match family {
            Family::Grok => {
                if !args.limit_hit {
                    return Err(
                        "Grok exposes no usage reading; record its limit-hit error with \
                                --limit-hit [--resets-at EPOCH]"
                            .into(),
                    );
                }
                (
                    "grok limit-hit error",
                    limits::grok_limit_hit(args.resets_at)?,
                )
            }
            Family::Claude | Family::Codex => {
                if args.limit_hit {
                    return Err("--limit-hit is for Grok; Claude and Codex give readings".into());
                }
                let mut text = String::new();
                std::io::stdin()
                    .take(256 * 1024 + 1)
                    .read_to_string(&mut text)
                    .map_err(|error| format!("cannot read standard input: {error}"))?;
                if family == Family::Claude {
                    (
                        "claude statusline rate_limits",
                        limits::parse_claude_statusline(&text)?,
                    )
                } else {
                    (
                        "codex account/rateLimits",
                        limits::parse_codex_rate_limits(&text)?,
                    )
                }
            }
        };
        let project = seats::shared_root(&super::repo_root()?)?;
        let inherited = std::env::var_os(family.config_home_var());
        let home = args
            .config_home
            .clone()
            .or_else(|| inherited.map(std::path::PathBuf::from))
            .map(|home| codeflow_core::git::display_path(&home));
        let now = limits::now_secs();
        let snapshot = limits::record(&project, family, home.as_deref(), source, &reading, now)?;
        Ok(snapshot.map(|_| {
            format!(
                "recorded {} limits from {source} for {}",
                family.id(),
                home.as_deref().unwrap_or("the default config home")
            )
        }))
    })();
    match outcome {
        Ok(Some(line)) => {
            println!("{line}");
            0
        }
        Ok(None) => {
            println!(
                "nothing recorded for {}: the payload carried no rate_limits",
                family.id()
            );
            0
        }
        Err(reason) => {
            eprintln!(
                "models record-limits: {reason}",
                reason = super::shown(&reason)
            );
            2
        }
    }
}

pub fn show_limits(args: &LimitsArgs) -> i32 {
    let outcome = (|| -> Result<(Vec<FamilyLimits>, Option<Profile>), String> {
        let root = super::repo_root()?;
        let profile = load_profile(&root)?;
        let project = seats::shared_root(&root)?;
        let now = limits::now_secs();
        let states = Family::ALL
            .into_iter()
            .map(|family| {
                let home = profile
                    .as_ref()
                    .and_then(|p| p.config_home(family))
                    .map(codeflow_core::git::display_path);
                limits::state(&project, family, home.as_deref(), now)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok((states, profile))
    })();
    let (states, profile) = match outcome {
        Ok(pair) => pair,
        Err(reason) => {
            eprintln!("models limits: {reason}", reason = super::shown(&reason));
            return 2;
        }
    };
    if args.json {
        let families: Vec<_> = states
            .iter()
            .map(|state| {
                let family = Family::from_id(state.family);
                let account = family
                    .and_then(|f| profile.as_ref().and_then(|p| p.families.get(&f)))
                    .map(|a| {
                        json!({
                            "config_home": a.config_home.as_deref().map(codeflow_core::git::display_path),
                            "api_fallback": a.api_fallback.as_ref().map(|f| json!({
                                "key_env": f.key_env,
                                "spend_cap_usd": f.spend_cap_usd,
                            })),
                        })
                    });
                let mut value = serde_json::to_value(state).unwrap_or_default();
                value["account"] = account.unwrap_or(serde_json::Value::Null);
                value
            })
            .collect();
        let out = json!({
            "profile": profile.as_ref().map(|p| codeflow_core::git::display_path(&p.source)),
            "families": families,
        });
        match serde_json::to_string_pretty(&out) {
            Ok(text) => println_block!(codeflow_core::git::TextBlock::json(&text)),
            Err(error) => {
                eprintln!("models limits: {error}", error = super::shown(&error));
                return 2;
            }
        }
        return 0;
    }
    for state in &states {
        render(state, profile.as_ref());
    }
    0
}

fn when(secs: u64) -> String {
    registry::rfc3339_from_epoch_secs(secs)
}

fn window_line(name: &str, view: &WindowView) -> String {
    match view {
        WindowView::NotExposed => format!("  {name}: not exposed"),
        WindowView::Used {
            used_percent,
            resets_at: Some(reset),
        } => format!("  {name}: {used_percent}% used, resets {}", when(*reset)),
        WindowView::Used {
            used_percent,
            resets_at: None,
        } => format!("  {name}: {used_percent}% used, reset time not given"),
        WindowView::ResetSinceReading {
            was_percent,
            reset_at,
        } => format!(
            "  {name}: reset {} since the reading (it read {was_percent}%); wait for a new reading",
            when(*reset_at)
        ),
    }
}

fn render(state: &FamilyLimits, profile: Option<&Profile>) {
    println!("{}:", state.family);
    if let Some(account) = Family::from_id(state.family)
        .and_then(|family| profile.and_then(|p| p.families.get(&family)))
    {
        match &account.config_home {
            Some(home) => println!(
                "  account: config home {} (via {})",
                codeflow_core::git::display_path(home),
                Family::from_id(state.family).map_or("", Family::config_home_var)
            ),
            None => println!("  account: the CLI's default config home"),
        }
        if let Some(fallback) = &account.api_fallback {
            println!(
                "  api fallback: key in ${}, spend cap ${}",
                fallback.key_env, fallback.spend_cap_usd
            );
        }
    } else {
        println!("  account: the CLI's default config home");
    }
    println!("{}", window_line("5-hour", &state.five_hour));
    println!("{}", window_line("weekly", &state.weekly));
    if let Some(at) = state.limit_hit_at {
        match state.limit_hit_resets_at {
            Some(reset) => println!("  limit hit reported {}, resets {}", when(at), when(reset)),
            None => println!("  limit hit reported {}, reset time not given", when(at)),
        }
    }
    if state.legacy_unbound {
        println!(
            "  an earlier-format reading is stored; it names no account, so it is ignored and \
             the next `record-limits` replaces it"
        );
    }
    if !state.other_homes.is_empty() {
        println!(
            "  unknown for this account: readings exist only for {}",
            state.other_homes.join(", ")
        );
    }
    match (&state.source, state.recorded_at) {
        (Some(source), Some(at)) => println!("  read {} from {source}", when(at)),
        _ => {
            if state.family == "grok" {
                println!(
                    "  nothing recorded; Grok Build gives no usage reading, so record its limit-hit \
                     error with `codeflow models record-limits --family grok --limit-hit`"
                );
            } else {
                println!(
                    "  nothing recorded; pipe the CLI's payload to `codeflow models record-limits \
                     --family {}`",
                    state.family
                );
            }
        }
    }
}
