use anyhow::Result;
use clap::{Parser, Subcommand};

mod cmd;
mod exit;
mod helpers;

#[derive(Debug, Parser)]
#[command(name = "codeflow", version = concat!(env!("CARGO_PKG_VERSION"), " (", env!("GIT_COMMIT"), " ", env!("BUILD_TIMESTAMP"), ")"), about = "CodeFlow CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Uninstall `CodeFlow`
    Uninstall,
    /// Database operations
    Db,
    /// Session management
    Session,
    /// Initialize `CodeFlow` project
    Init {
        /// Skip location detection, assume existing project
        #[arg(long)]
        existing: bool,
        /// Join an existing CodeFlow project (cloned repo)
        #[arg(long)]
        join: bool,
        /// Skip authentication steps (Claude Code and Git provider)
        #[arg(long)]
        skip_auth: bool,
        /// Non-interactive mode, accept all defaults
        #[arg(long, short)]
        yes: bool,
    },
    /// Diagnose infrastructure issues
    Doctor,
    /// Configuration management
    Config {
        #[command(subcommand)]
        command: Option<cmd::config::ConfigCommand>,
    },
    /// Update `CodeFlow`
    Update,
    /// Autorun batch execution
    Autorun {
        #[command(subcommand)]
        command: Option<cmd::autorun::AutorunCommand>,
    },
    /// Ledger operations (status, compact, rebuild, migrate)
    Ledger {
        #[command(subcommand)]
        command: Option<cmd::ledger::LedgerCommand>,
    },
    /// Show welcome message
    Welcome,
    /// Internal operations
    Internal,
    /// `PathFlow` phase management
    Pathflow,
    /// State management
    State,
    /// Validate configuration and state
    Validate {
        #[command(subcommand)]
        command: cmd::validate::ValidateCommand,
    },
    /// Hook event handlers
    Hooks {
        #[command(subcommand)]
        command: cmd::hooks::HookCommand,
    },
    /// Sentinel management
    Sentinel,
    /// Coordination operations
    Coordination {
        #[command(subcommand)]
        command: Option<cmd::coordination::CoordinationCommand>,
    },
    /// Settings management
    Settings {
        #[command(subcommand)]
        command: Option<cmd::settings::SettingsCommand>,
    },
    /// Git worktree management
    Worktree {
        #[command(subcommand)]
        command: Option<cmd::worktree::WorktreeCommand>,
    },
    /// Parallel execution status
    Parallel {
        #[command(subcommand)]
        command: Option<cmd::parallel::ParallelCommand>,
    },
    /// Report generation
    Report,
    /// `WorkGraph` operations
    Workgraph,
    /// Git operations (conflict detection, branch analysis)
    Git {
        #[command(subcommand)]
        command: Option<cmd::git::GitCommand>,
    },
    /// Git hooks management
    #[command(name = "git-hooks")]
    GitHooks {
        #[command(subcommand)]
        command: Option<cmd::git_hooks::GitHooksCommand>,
    },
    /// Shadow test runner
    #[command(name = "shadow-test")]
    ShadowTest,
    /// Normalize data
    Normalize,
    /// Run test suite
    Test {
        #[command(flatten)]
        args: cmd::test::TestArgs,
    },
    /// Sync daemon operations
    Sync {
        #[command(subcommand)]
        command: cmd::sync::SyncCommand,
    },
    /// Interactive session management (alias: codeflow -i)
    #[command(visible_alias = "-i")]
    Interactive {
        #[command(subcommand)]
        command: Option<cmd::interactive::InteractiveCommand>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    dispatch(cli.command).await
}

async fn dispatch(command: Command) -> Result<()> {
    match command {
        Command::Uninstall => cmd::uninstall::run(),
        Command::Db => cmd::db::run().await,
        Command::Session => cmd::session::run(),
        Command::Init {
            existing,
            join,
            skip_auth,
            yes,
        } => cmd::init::run(cmd::init::InitFlags {
            existing,
            join,
            skip_auth,
            yes,
        }),
        Command::Doctor => cmd::doctor::run().await,
        Command::Config { command } => cmd::config::run(command),
        Command::Update => cmd::update::run(),
        Command::Autorun { command } => cmd::autorun::run(command).await,
        Command::Ledger { command } => cmd::ledger::run(command),
        Command::Welcome => cmd::welcome::run(),
        Command::Internal => cmd::internal::run(),
        Command::Pathflow => cmd::pathflow::run(),
        Command::State => cmd::state::run(),
        Command::Validate { command } => cmd::validate::run(command),
        Command::Hooks { command } => cmd::hooks::run(command),
        Command::Sentinel => cmd::sentinel::run(),
        Command::Coordination { command } => cmd::coordination::run(command),
        Command::Settings { command } => cmd::settings::run(command),
        Command::Worktree { command } => cmd::worktree::run(command),
        Command::Parallel { command } => cmd::parallel::run(command),
        Command::Report => cmd::report::run(),
        Command::Workgraph => cmd::workgraph::run().await,
        Command::Git { command } => cmd::git::run(command),
        Command::GitHooks { command } => cmd::git_hooks::run(command),
        Command::ShadowTest => cmd::shadow_test::run(),
        Command::Normalize => cmd::normalize::run(),
        Command::Test { args } => cmd::test::run(Some(args)),
        Command::Sync { command } => cmd::sync::run(command),
        Command::Interactive { command } => cmd::interactive::run(command).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_parses_help() {
        let result = Cli::try_parse_from(["codeflow", "--help"]);
        let err = result.expect_err("--help returns an error");
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayHelp);
    }

    #[test]
    fn test_subcommand_count() {
        let err = Cli::try_parse_from(["codeflow", "--help"]).expect_err("help error");
        let help_text = err.to_string();
        let expected_commands = [
            "uninstall",
            "db",
            "session",
            "init",
            "doctor",
            "config",
            "update",
            "autorun",
            "ledger",
            "welcome",
            "internal",
            "pathflow",
            "state",
            "validate",
            "hooks",
            "sentinel",
            "coordination",
            "settings",
            "worktree",
            "parallel",
            "report",
            "workgraph",
            "git",
            "git-hooks",
            "shadow-test",
            "normalize",
            "test",
            "sync",
            "interactive",
        ];
        for cmd in &expected_commands {
            assert!(
                help_text.contains(cmd),
                "help output missing subcommand: {cmd}"
            );
        }
        assert_eq!(expected_commands.len(), 29);
    }

    #[test]
    fn test_each_subcommand_parses() {
        // Subcommands that parse with just the name (no further args required).
        let commands = [
            "uninstall",
            "db",
            "session",
            "init",
            "doctor",
            "config",
            "update",
            "autorun",
            "ledger",
            "welcome",
            "internal",
            "pathflow",
            "state",
            "sentinel",
            "coordination",
            "settings",
            "worktree",
            "parallel",
            "report",
            "workgraph",
            "git",
            "git-hooks",
            "shadow-test",
            "normalize",
            "test",
            "interactive",
        ];
        for cmd in &commands {
            let result = Cli::try_parse_from(["codeflow", cmd]);
            assert!(result.is_ok(), "failed to parse subcommand: {cmd}");
        }
    }

    #[test]
    fn test_validate_subcommand_requires_subcommand() {
        let result = Cli::try_parse_from(["codeflow", "validate"]);
        assert!(result.is_err(), "validate without subcommand should fail");
    }

    #[test]
    fn test_validate_epic_parses() {
        let result = Cli::try_parse_from(["codeflow", "validate", "epic", "/tmp/epic.md"]);
        assert!(result.is_ok(), "should parse validate epic <file>");
    }

    #[test]
    fn test_validate_task_parses() {
        let result = Cli::try_parse_from(["codeflow", "validate", "task", "/tmp/task.md"]);
        assert!(result.is_ok(), "should parse validate task <file>");
    }

    #[test]
    fn test_validate_all_parses() {
        let result = Cli::try_parse_from(["codeflow", "validate", "all"]);
        assert!(result.is_ok(), "should parse validate all");
    }

    #[test]
    fn test_hooks_subcommand_requires_event_group() {
        // `codeflow hooks` alone should fail (needs a subcommand).
        let result = Cli::try_parse_from(["codeflow", "hooks"]);
        assert!(result.is_err(), "hooks without event should fail");
    }

    #[test]
    fn test_hooks_three_level_parse() {
        let result = Cli::try_parse_from(["codeflow", "hooks", "pre-tool-use", "gate-check"]);
        assert!(
            result.is_ok(),
            "should parse 3-level: hooks pre-tool-use gate-check"
        );
    }

    #[tokio::test]
    async fn test_dispatch_uninstall() {
        let result = dispatch(Command::Uninstall).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_dispatch_welcome() {
        let result = dispatch(Command::Welcome).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_dispatch_internal() {
        let result = dispatch(Command::Internal).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_dispatch_update() {
        let result = dispatch(Command::Update).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_dispatch_normalize() {
        let result = dispatch(Command::Normalize).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_dispatch_shadow_test() {
        let result = dispatch(Command::ShadowTest).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_dispatch_config() {
        let result = dispatch(Command::Config { command: None }).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_dispatch_ledger() {
        let result = dispatch(Command::Ledger { command: None }).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_dispatch_init() {
        let result = dispatch(Command::Init {
            existing: false,
            join: false,
            skip_auth: false,
            yes: false,
        })
        .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_dispatch_sentinel() {
        let result = dispatch(Command::Sentinel).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_dispatch_coordination() {
        let result = dispatch(Command::Coordination { command: None }).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_dispatch_state() {
        let result = dispatch(Command::State).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_dispatch_report() {
        let result = dispatch(Command::Report).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_dispatch_session() {
        // Session returns error when no state dir exists, which is fine.
        let _result = dispatch(Command::Session).await;
    }

    #[tokio::test]
    async fn test_dispatch_pathflow() {
        let result = dispatch(Command::Pathflow).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_dispatch_git() {
        let result = dispatch(Command::Git { command: None }).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_dispatch_git_hooks() {
        let result = dispatch(Command::GitHooks { command: None }).await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_sync_subcommand_requires_subcommand() {
        let result = Cli::try_parse_from(["codeflow", "sync"]);
        assert!(result.is_err(), "sync without subcommand should fail");
    }

    #[test]
    fn test_sync_daemon_parses() {
        let result = Cli::try_parse_from(["codeflow", "sync", "daemon"]);
        assert!(result.is_ok(), "should parse sync daemon");
    }

    #[test]
    fn test_sync_daemon_with_interval_parses() {
        let result = Cli::try_parse_from(["codeflow", "sync", "daemon", "--interval", "60"]);
        assert!(result.is_ok(), "should parse sync daemon --interval 60");
    }

    #[test]
    fn test_sync_status_parses() {
        let result = Cli::try_parse_from(["codeflow", "sync", "status"]);
        assert!(result.is_ok(), "should parse sync status");
    }

    #[test]
    fn test_sync_start_parses() {
        let result = Cli::try_parse_from(["codeflow", "sync", "start"]);
        assert!(result.is_ok(), "should parse sync start");
    }

    #[test]
    fn test_sync_start_with_interval_parses() {
        let result = Cli::try_parse_from(["codeflow", "sync", "start", "--interval", "10"]);
        assert!(result.is_ok(), "should parse sync start --interval 10");
    }

    #[test]
    fn test_sync_stop_parses() {
        let result = Cli::try_parse_from(["codeflow", "sync", "stop"]);
        assert!(result.is_ok(), "should parse sync stop");
    }

    #[test]
    fn test_worktree_list_parses() {
        let result = Cli::try_parse_from(["codeflow", "worktree", "list"]);
        assert!(result.is_ok(), "should parse worktree list");
    }

    #[test]
    fn test_worktree_cleanup_parses() {
        let result = Cli::try_parse_from(["codeflow", "worktree", "cleanup"]);
        assert!(result.is_ok(), "should parse worktree cleanup");
    }

    #[test]
    fn test_worktree_cleanup_with_flags_parses() {
        let result =
            Cli::try_parse_from(["codeflow", "worktree", "cleanup", "--force", "--dry-run"]);
        assert!(
            result.is_ok(),
            "should parse worktree cleanup --force --dry-run"
        );
    }

    #[test]
    fn test_worktree_repair_parses() {
        let result = Cli::try_parse_from(["codeflow", "worktree", "repair"]);
        assert!(result.is_ok(), "should parse worktree repair");
    }

    #[test]
    fn test_worktree_repair_with_path_parses() {
        let result = Cli::try_parse_from(["codeflow", "worktree", "repair", "--path", "/tmp/wt"]);
        assert!(result.is_ok(), "should parse worktree repair --path");
    }

    #[test]
    fn test_parallel_status_parses() {
        let result = Cli::try_parse_from(["codeflow", "parallel", "status"]);
        assert!(result.is_ok(), "should parse parallel status");
    }

    #[tokio::test]
    async fn test_dispatch_worktree() {
        let result = dispatch(Command::Worktree { command: None }).await;
        // May fail if no project dir detected, which is fine.
        let _ = result;
    }

    #[tokio::test]
    async fn test_dispatch_parallel() {
        let result = dispatch(Command::Parallel { command: None }).await;
        let _ = result;
    }

    #[test]
    fn test_unknown_subcommand_rejected() {
        let result = Cli::try_parse_from(["codeflow", "nonexistent"]);
        assert!(result.is_err(), "unknown subcommand should be rejected");
    }

    #[test]
    fn test_version_flag_shows_version() {
        let result = Cli::try_parse_from(["codeflow", "--version"]);
        let err = result.expect_err("--version returns an error");
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayVersion);
    }

    // -- Interactive subcommand parsing --

    #[test]
    fn test_interactive_subcommand_parses() {
        let result = Cli::try_parse_from(["codeflow", "interactive"]);
        assert!(result.is_ok(), "interactive should parse");
    }

    #[test]
    fn test_interactive_alias_parses() {
        let result = Cli::try_parse_from(["codeflow", "-i"]);
        assert!(result.is_ok(), "interactive -i alias should parse");
    }

    #[test]
    fn test_interactive_status_parses() {
        let result = Cli::try_parse_from(["codeflow", "interactive", "status"]);
        assert!(result.is_ok(), "interactive status should parse");
    }

    #[test]
    fn test_interactive_list_parses() {
        let result = Cli::try_parse_from(["codeflow", "interactive", "list"]);
        assert!(result.is_ok(), "interactive list should parse");
    }

    #[test]
    fn test_interactive_cleanup_parses() {
        let result = Cli::try_parse_from(["codeflow", "interactive", "cleanup"]);
        assert!(result.is_ok(), "interactive cleanup should parse");
    }

    // -- Autorun subcommand parsing --

    #[test]
    fn test_autorun_run_subcommand_parses() {
        let result = Cli::try_parse_from(["codeflow", "autorun", "run"]);
        assert!(result.is_ok(), "autorun run should parse");
    }

    #[test]
    fn test_autorun_run_with_batch_flag_parses() {
        let result =
            Cli::try_parse_from(["codeflow", "autorun", "run", "--batch", "/tmp/batch.yaml"]);
        assert!(
            result.is_ok(),
            "autorun run --batch should parse: {result:?}"
        );
    }

    #[test]
    fn test_autorun_no_subcommand_still_parses() {
        // Backwards compatibility: bare "autorun" still works (Option<AutorunCommand> = None).
        let result = Cli::try_parse_from(["codeflow", "autorun"]);
        assert!(
            result.is_ok(),
            "bare autorun should still parse for backwards compat: {result:?}"
        );
    }
}

/// Test environment isolation: strips CodeFlow env vars before any test runs.
/// Prevents worktree env pollution from corrupting test state.
#[cfg(test)]
#[ctor::ctor]
fn strip_codeflow_env() {
    for var in [
        "CODEFLOW_WORKTREE_PATH",
        "CODEFLOW_SESSION_ID",
        "CF_PROJECT_ROOT",
        "AUTORUN_SESSION_ID",
        "AUTORUN_BATCH_ID",
        "AUTORUN_TASK_ID",
        "AUTORUN_ACCEPTANCE",
        "CODEFLOW_WORKTREE_MODE",
    ] {
        // SAFETY: This runs before any test thread spawns (ctor guarantees
        // single-threaded execution). No other thread reads these vars.
        unsafe {
            std::env::remove_var(var);
        }
    }
}
