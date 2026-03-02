package main

import (
	"fmt"
	"io"

	"github.com/codeflow/codeflow-cli/internal/hooks/logging"
	"github.com/spf13/cobra"
)

// newSessionStartLogCmd creates the "logging" subcommand under session-start.
func newSessionStartLogCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "logging",
		Short: "Log session-start event",
		Long: `Log a session_start event to session-{date}.jsonl.

Reads Claude Code SessionStart hook JSON from stdin. Resolves session ID
from codeflow-env.sh / CODEFLOW_SESSION_ID (never uses stdin session_id).
Always exits 0 (logging never blocks).`,
		Args: cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runSessionStartLog(cmd.InOrStdin(), cmd.ErrOrStderr(), detectProjectDir())
		},
	}
}

// runSessionStartLog implements the session-start logging logic.
func runSessionStartLog(stdin io.Reader, errW io.Writer, projectDir string) error {
	cfg := logging.ReadConfig(projectDir)

	w, err := logging.NewActivityWriter(projectDir, cfg.SessionStart.LogDirectory)
	if err != nil {
		fmt.Fprintf(errW, "session-start logging: %v\n", err)
		return nil // Never block.
	}

	if err := logging.LogSessionStart(w, stdin, projectDir, cfg); err != nil {
		fmt.Fprintf(errW, "session-start logging: %v\n", err)
	}
	return nil
}

// newSessionEndLogCmd creates the "logging" subcommand under session-end.
func newSessionEndLogCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "logging",
		Short: "Log session-end event",
		Long: `Log a session_end event to session-{date}.jsonl.

Reads Claude Code SessionEnd hook JSON from stdin. Calculates session
duration from the session meta file. Always exits 0.`,
		Args: cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runSessionEndLog(cmd.InOrStdin(), cmd.ErrOrStderr(), detectProjectDir())
		},
	}
}

// runSessionEndLog implements the session-end logging logic.
func runSessionEndLog(stdin io.Reader, errW io.Writer, projectDir string) error {
	cfg := logging.ReadConfig(projectDir)

	w, err := logging.NewActivityWriter(projectDir, cfg.SessionEnd.LogDirectory)
	if err != nil {
		fmt.Fprintf(errW, "session-end logging: %v\n", err)
		return nil
	}

	if err := logging.LogSessionEnd(w, stdin, projectDir, cfg); err != nil {
		fmt.Fprintf(errW, "session-end logging: %v\n", err)
	}
	return nil
}

// newStopLogCmd creates the "logging" subcommand under stop.
func newStopLogCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "logging",
		Short: "Log stop event",
		Long: `Log a stop event to session-{date}.jsonl.

Reads Claude Code Stop hook JSON from stdin. Captures stop_reason,
task context (optional), and git branch state. Always exits 0.`,
		Args: cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runStopLog(cmd.InOrStdin(), cmd.ErrOrStderr(), detectProjectDir())
		},
	}
}

// runStopLog implements the stop logging logic.
func runStopLog(stdin io.Reader, errW io.Writer, projectDir string) error {
	cfg := logging.ReadConfig(projectDir)

	w, err := logging.NewActivityWriter(projectDir, cfg.Stop.LogDirectory)
	if err != nil {
		fmt.Fprintf(errW, "stop logging: %v\n", err)
		return nil
	}

	if err := logging.LogStop(w, stdin, projectDir, cfg); err != nil {
		fmt.Fprintf(errW, "stop logging: %v\n", err)
	}
	return nil
}

// newPostToolUseLogCmd creates the "logging" subcommand under post-tool-use.
func newPostToolUseLogCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "logging",
		Short: "Log post-tool-use event",
		Long: `Log a tool_completed event to tool-use-{date}.jsonl.

Reads Claude Code PostToolUse hook JSON from stdin. Filters by tool name,
truncates large results, and redacts sensitive data. Always exits 0.`,
		Args: cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runPostToolUseLog(cmd.InOrStdin(), cmd.ErrOrStderr(), detectProjectDir())
		},
	}
}

// runPostToolUseLog implements the post-tool-use logging logic.
func runPostToolUseLog(stdin io.Reader, errW io.Writer, projectDir string) error {
	cfg := logging.ReadConfig(projectDir)

	w, err := logging.NewActivityWriter(projectDir, cfg.PostToolUse.LogDirectory)
	if err != nil {
		fmt.Fprintf(errW, "post-tool-use logging: %v\n", err)
		return nil
	}

	if err := logging.LogToolUse(w, stdin, projectDir, cfg); err != nil {
		fmt.Fprintf(errW, "post-tool-use logging: %v\n", err)
	}
	return nil
}

// newUserPromptSubmitLogCmd creates the "logging" subcommand under user-prompt-submit.
func newUserPromptSubmitLogCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "logging",
		Short: "Log user-prompt-submit event",
		Long: `Log a prompt_submitted event to prompts-{date}.jsonl.

Reads Claude Code UserPromptSubmit hook JSON from stdin. Computes prompt
hash, detects intent type, and optionally captures full text. Always exits 0.`,
		Args: cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runUserPromptSubmitLog(cmd.InOrStdin(), cmd.ErrOrStderr(), detectProjectDir())
		},
	}
}

// runUserPromptSubmitLog implements the user-prompt-submit logging logic.
func runUserPromptSubmitLog(stdin io.Reader, errW io.Writer, projectDir string) error {
	cfg := logging.ReadConfig(projectDir)

	w, err := logging.NewActivityWriter(projectDir, cfg.UserPrompt.LogDirectory)
	if err != nil {
		fmt.Fprintf(errW, "user-prompt-submit logging: %v\n", err)
		return nil
	}

	if err := logging.LogPrompt(w, stdin, projectDir, cfg); err != nil {
		fmt.Fprintf(errW, "user-prompt-submit logging: %v\n", err)
	}
	return nil
}
