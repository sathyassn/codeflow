package main

import (
	"errors"
	"fmt"
	"os"

	"github.com/codeflow/codeflow-cli/internal/githooks"
	"github.com/spf13/cobra"
)

// newGitHooksCmd creates the top-level "git-hooks" command with subcommands
// for each git hook.
func newGitHooksCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "git-hooks",
		Short: "Git hook implementations",
		Long:  "Run git hook logic implemented in Go. Called by thin shell wrappers in .codeflow/scripts/git-hooks/.",
		RunE: func(cmd *cobra.Command, _ []string) error {
			return cmd.Help()
		},
	}

	cmd.AddCommand(newGitHooksCommitMsgCmd())
	cmd.AddCommand(newGitHooksPostCommitCmd())
	cmd.AddCommand(newGitHooksPrePushCmd())
	cmd.AddCommand(newGitHooksPrepareCommitMsgCmd())
	cmd.AddCommand(newGitHooksPreCommitValidateCmd())

	return cmd
}

// loadGitHooksPolicy loads the enforcement policy, falling back to defaults.
func loadGitHooksPolicy() *githooks.EnforcementPolicy {
	projectDir := detectProjectDir()
	policy, err := githooks.LoadEnforcementPolicy(projectDir)
	if err != nil {
		return githooks.DefaultPolicy()
	}
	return policy
}

func newGitHooksCommitMsgCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "commit-msg <file>",
		Short: "Validate commit message format",
		Long:  "Validate a commit message file against conventional commit format rules from enforcement-policy.json.",
		Args:  cobra.ExactArgs(1),
		RunE: func(_ *cobra.Command, args []string) error {
			policy := loadGitHooksPolicy()
			err := githooks.RunCommitMsg(args[0], policy)
			if err != nil {
				var cmErrs *githooks.CommitMsgErrors
				if errors.As(err, &cmErrs) {
					for _, e := range cmErrs.Errors {
						fmt.Fprintf(os.Stderr, "\033[0;31mERROR: %s\033[0m\n", e)
					}
					fmt.Fprintln(os.Stderr)
					fmt.Fprintf(os.Stderr, "\033[0;31mCommit message validation failed with %d error(s)\033[0m\n", len(cmErrs.Errors))
					return &exitError{code: ExitGeneralError, err: err}
				}
				fmt.Fprintf(os.Stderr, "error: %v\n", err)
				return &exitError{code: ExitGeneralError, err: err}
			}
			fmt.Fprintf(os.Stdout, "\033[0;32mCommit message format valid\033[0m\n")
			return nil
		},
	}
}

func newGitHooksPostCommitCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "post-commit",
		Short: "Log commit metadata and show confirmation",
		Long:  "Write commit metadata to .state/logs/git/commits-{date}.jsonl and display user-visible confirmation.",
		Args:  cobra.NoArgs,
		RunE: func(_ *cobra.Command, _ []string) error {
			projectDir := detectProjectDir()
			return githooks.RunPostCommit(os.Stdout, projectDir)
		},
	}
}

func newGitHooksPrePushCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "pre-push <remote> <url>",
		Short: "Validate push against protected branches",
		Long:  "Read pushed refs from stdin, block pushes to protected branches, detect force-pushes, and support TTY override.",
		Args:  cobra.ExactArgs(2),
		RunE: func(_ *cobra.Command, args []string) error {
			policy := loadGitHooksPolicy()
			projectDir := detectProjectDir()
			err := githooks.RunPrePush(os.Stdin, os.Stdout, os.Stderr, args[0], args[1], projectDir, policy)
			if err != nil {
				return &exitError{code: ExitGeneralError, err: err}
			}
			return nil
		},
	}
}

func newGitHooksPrepareCommitMsgCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "prepare-commit-msg <file> [source]",
		Short: "Generate commit message template from branch name",
		Long:  "Extract commit type and scope from branch name pattern and write a template to the commit message file.",
		Args:  cobra.RangeArgs(1, 2),
		RunE: func(_ *cobra.Command, args []string) error {
			source := ""
			if len(args) > 1 {
				source = args[1]
			}
			policy := loadGitHooksPolicy()
			return githooks.RunPrepareCommitMsg(args[0], source, policy)
		},
	}
}

func newGitHooksPreCommitValidateCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "pre-commit-validate",
		Short: "Run pure-logic pre-commit checks",
		Long:  "Branch protection, sensitive files, JSON validation, Go test conventions, and coverage enforcement.",
		Args:  cobra.NoArgs,
		RunE: func(_ *cobra.Command, _ []string) error {
			policy := loadGitHooksPolicy()
			projectDir := detectProjectDir()
			err := githooks.RunPreCommitValidate(os.Stdout, projectDir, policy)
			if err != nil {
				var pcErrs *githooks.PreCommitErrors
				if errors.As(err, &pcErrs) {
					for _, e := range pcErrs.Errors {
						fmt.Fprintf(os.Stderr, "\033[0;31m%s\033[0m\n", e)
					}
				}
				return &exitError{code: ExitGeneralError, err: err}
			}
			return nil
		},
	}
}
