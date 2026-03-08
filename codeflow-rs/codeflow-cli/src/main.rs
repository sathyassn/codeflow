use anyhow::Result;
use clap::{Parser, Subcommand};

mod cmd;

#[derive(Debug, Parser)]
#[command(name = "codeflow", version, about = "CodeFlow CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Show version information
    Version,
    /// Uninstall `CodeFlow`
    Uninstall,
    /// Database operations
    Db,
    /// Session management
    Session,
    /// Initialize `CodeFlow` project
    Init,
    /// Diagnose infrastructure issues
    Doctor,
    /// Configuration management
    Config,
    /// Update `CodeFlow`
    Update,
    /// Autorun batch execution
    Autorun,
    /// Ledger operations
    Ledger,
    /// Show welcome message
    Welcome,
    /// Internal operations
    Internal,
    /// `PathFlow` phase management
    Pathflow,
    /// State management
    State,
    /// Validate configuration and state
    Validate,
    /// Hook event handlers
    Hooks,
    /// Sentinel management
    Sentinel,
    /// Coordination operations
    Coordination,
    /// Settings management
    Settings,
    /// Git worktree management
    Worktree,
    /// Report generation
    Report,
    /// `WorkGraph` operations
    Workgraph,
    /// Git hooks management
    #[command(name = "git-hooks")]
    GitHooks,
    /// Shadow test runner
    #[command(name = "shadow-test")]
    ShadowTest,
    /// Normalize data
    Normalize,
    /// Run test suite
    Test,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Version => cmd::version::run(),
        Command::Uninstall => cmd::uninstall::run(),
        Command::Db => cmd::db::run(),
        Command::Session => cmd::session::run(),
        Command::Init => cmd::init::run(),
        Command::Doctor => cmd::doctor::run(),
        Command::Config => cmd::config::run(),
        Command::Update => cmd::update::run(),
        Command::Autorun => cmd::autorun::run(),
        Command::Ledger => cmd::ledger::run(),
        Command::Welcome => cmd::welcome::run(),
        Command::Internal => cmd::internal::run(),
        Command::Pathflow => cmd::pathflow::run(),
        Command::State => cmd::state::run(),
        Command::Validate => cmd::validate::run(),
        Command::Hooks => cmd::hooks::run(),
        Command::Sentinel => cmd::sentinel::run(),
        Command::Coordination => cmd::coordination::run(),
        Command::Settings => cmd::settings::run(),
        Command::Worktree => cmd::worktree::run(),
        Command::Report => cmd::report::run(),
        Command::Workgraph => cmd::workgraph::run(),
        Command::GitHooks => cmd::git_hooks::run(),
        Command::ShadowTest => cmd::shadow_test::run(),
        Command::Normalize => cmd::normalize::run(),
        Command::Test => cmd::test::run(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_parses_help() {
        // Verify the CLI struct can be constructed and the subcommand enum has all variants.
        // clap's derive macro validates the structure at compile time.
        let result = Cli::try_parse_from(["codeflow", "--help"]);
        // --help causes clap to return an error (it prints help and exits),
        // but the error kind should be DisplayHelp, not a parse failure.
        let err = result.expect_err("--help returns an error");
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayHelp);
    }

    #[test]
    fn test_subcommand_count() {
        // Verify all 26 subcommands are present by checking help output.
        let err = Cli::try_parse_from(["codeflow", "--help"]).expect_err("help error");
        let help_text = err.to_string();
        let expected_commands = [
            "version",
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
            "report",
            "workgraph",
            "git-hooks",
            "shadow-test",
            "normalize",
            "test",
        ];
        for cmd in &expected_commands {
            assert!(
                help_text.contains(cmd),
                "help output missing subcommand: {cmd}"
            );
        }
        assert_eq!(expected_commands.len(), 26);
    }

    #[test]
    fn test_each_subcommand_parses() {
        let commands = [
            "version",
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
            "report",
            "workgraph",
            "git-hooks",
            "shadow-test",
            "normalize",
            "test",
        ];
        for cmd in &commands {
            let result = Cli::try_parse_from(["codeflow", cmd]);
            assert!(result.is_ok(), "failed to parse subcommand: {cmd}");
        }
    }
}
