//! Hook command dispatch: `codeflow hooks <event-group> <handler>`.
//!
//! 3-level Clap hierarchy matching the Go CLI's hook subcommand tree.

mod post_tool_use;
mod pre_tool_use;
mod session_end;
mod session_start;
mod stop;
mod task_completed;
mod user_prompt_submit;

use anyhow::Result;
use clap::Subcommand;

/// Hook event group subcommands.
#[derive(Debug, Clone, Copy, Subcommand)]
pub enum HookCommand {
    /// Session start handlers
    #[command(name = "session-start")]
    SessionStart {
        #[command(subcommand)]
        handler: session_start::SessionStartHandler,
    },
    /// Session end handlers
    #[command(name = "session-end")]
    SessionEnd {
        #[command(subcommand)]
        handler: session_end::SessionEndHandler,
    },
    /// Pre-tool-use handlers
    #[command(name = "pre-tool-use")]
    PreToolUse {
        #[command(subcommand)]
        handler: pre_tool_use::PreToolUseHandler,
    },
    /// Post-tool-use handlers
    #[command(name = "post-tool-use")]
    PostToolUse {
        #[command(subcommand)]
        handler: post_tool_use::PostToolUseHandler,
    },
    /// Task completed handlers
    #[command(name = "task-completed")]
    TaskCompleted {
        #[command(subcommand)]
        handler: task_completed::TaskCompletedHandler,
    },
    /// Stop handlers
    Stop {
        #[command(subcommand)]
        handler: stop::StopHandler,
    },
    /// User prompt submit handlers
    #[command(name = "user-prompt-submit")]
    UserPromptSubmit {
        #[command(subcommand)]
        handler: user_prompt_submit::UserPromptSubmitHandler,
    },
}

/// Dispatch hook subcommand to the appropriate handler.
pub fn run(cmd: HookCommand) -> Result<()> {
    match cmd {
        HookCommand::SessionStart { handler } => session_start::run(handler),
        HookCommand::SessionEnd { handler } => session_end::run(handler),
        HookCommand::PreToolUse { handler } => pre_tool_use::run(handler),
        HookCommand::PostToolUse { handler } => post_tool_use::run(handler),
        HookCommand::TaskCompleted { handler } => task_completed::run(handler),
        HookCommand::Stop { handler } => stop::run(handler),
        HookCommand::UserPromptSubmit { handler } => user_prompt_submit::run(handler),
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(subcommand)]
        cmd: super::HookCommand,
    }

    #[test]
    fn test_hook_command_parses_session_start_init() {
        let cli = TestCli::try_parse_from(["test", "session-start", "init"]);
        assert!(cli.is_ok(), "should parse session-start init");
    }

    #[test]
    fn test_hook_command_parses_pre_tool_use_gate_check() {
        let cli = TestCli::try_parse_from(["test", "pre-tool-use", "gate-check"]);
        assert!(cli.is_ok(), "should parse pre-tool-use gate-check");
    }

    #[test]
    fn test_hook_command_parses_post_tool_use_sentinel_write() {
        let cli = TestCli::try_parse_from(["test", "post-tool-use", "sentinel-write"]);
        assert!(cli.is_ok(), "should parse post-tool-use sentinel-write");
    }

    #[test]
    fn test_hook_command_parses_task_completed() {
        let cli = TestCli::try_parse_from(["test", "task-completed", "checkpoint-complete"]);
        assert!(
            cli.is_ok(),
            "should parse task-completed checkpoint-complete"
        );
    }

    #[test]
    fn test_hook_command_parses_stop_logging() {
        let cli = TestCli::try_parse_from(["test", "stop", "logging"]);
        assert!(cli.is_ok(), "should parse stop logging");
    }

    #[test]
    fn test_hook_command_parses_user_prompt_submit_validate() {
        let cli = TestCli::try_parse_from(["test", "user-prompt-submit", "validate"]);
        assert!(cli.is_ok(), "should parse user-prompt-submit validate");
    }

    #[test]
    fn test_hook_command_parses_session_end_cleanup() {
        let cli = TestCli::try_parse_from(["test", "session-end", "cleanup"]);
        assert!(cli.is_ok(), "should parse session-end cleanup");
    }

    #[test]
    fn test_all_pre_tool_use_handlers_parse() {
        let handlers = [
            "gate-check",
            "team-guard",
            "edit-write-guard",
            "gh-pr-guard",
            "protection-guard",
            "security",
            "webfetch-guard",
        ];
        for h in &handlers {
            let cli = TestCli::try_parse_from(["test", "pre-tool-use", h]);
            assert!(cli.is_ok(), "should parse pre-tool-use {h}");
        }
    }

    #[test]
    fn test_all_post_tool_use_handlers_parse() {
        let handlers = [
            "sentinel-write",
            "settings-validate",
            "checkpoint-register",
            "logging",
        ];
        for h in &handlers {
            let cli = TestCli::try_parse_from(["test", "post-tool-use", h]);
            assert!(cli.is_ok(), "should parse post-tool-use {h}");
        }
    }

    #[test]
    fn test_all_session_end_handlers_parse() {
        let handlers = ["cleanup", "logging"];
        for h in &handlers {
            let cli = TestCli::try_parse_from(["test", "session-end", h]);
            assert!(cli.is_ok(), "should parse session-end {h}");
        }
    }

    #[test]
    fn test_all_session_start_handlers_parse() {
        let handlers = ["init", "instructions", "logging"];
        for h in &handlers {
            let cli = TestCli::try_parse_from(["test", "session-start", h]);
            assert!(cli.is_ok(), "should parse session-start {h}");
        }
    }

    #[test]
    fn test_all_user_prompt_submit_handlers_parse() {
        let handlers = ["validate", "logging"];
        for h in &handlers {
            let cli = TestCli::try_parse_from(["test", "user-prompt-submit", h]);
            assert!(cli.is_ok(), "should parse user-prompt-submit {h}");
        }
    }

    #[test]
    fn test_all_task_completed_handlers_parse() {
        let cli = TestCli::try_parse_from(["test", "task-completed", "checkpoint-complete"]);
        assert!(
            cli.is_ok(),
            "should parse task-completed checkpoint-complete"
        );
    }

    #[test]
    fn test_all_stop_handlers_parse() {
        let cli = TestCli::try_parse_from(["test", "stop", "logging"]);
        assert!(cli.is_ok(), "should parse stop logging");
    }

    #[test]
    fn test_invalid_handler_name_rejected() {
        let cli = TestCli::try_parse_from(["test", "pre-tool-use", "nonexistent-handler"]);
        assert!(
            cli.is_err(),
            "nonexistent handler name should fail to parse"
        );
    }
}
