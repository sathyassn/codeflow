package main

import (
	"fmt"
	"io"

	"github.com/codeflow/codeflow-cli/internal/hooks/prompt"
	"github.com/codeflow/codeflow-cli/internal/hooks/security"
	"github.com/spf13/cobra"
)

// newPromptValidateCmd creates the "validate" subcommand under user-prompt-submit
// that outputs context reminders (git status, branch protection, active task,
// PathFlow mode).
func newPromptValidateCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "validate",
		Short: "Output context reminders on user prompt submission",
		Long: `Output context reminders when a user submits a prompt.

Checks git status, protected branch, active task, and PathFlow mode.
Each reminder is wrapped in <user-prompt-submit-hook> tags.

Always exits 0 (validation never blocks).

Stdin format:
  {"session_id":"uuid-from-claude"}`,
		Args: cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runPromptValidate(cmd.InOrStdin(), cmd.OutOrStdout(), cmd.ErrOrStderr())
		},
	}
}

// runPromptValidate implements the user-prompt-submit validate logic.
func runPromptValidate(stdin io.Reader, outW io.Writer, errW io.Writer) error {
	projectDir := detectProjectDir()
	validator := buildPromptValidator(projectDir)

	if err := validator.Validate(stdin, outW); err != nil {
		fmt.Fprintf(errW, "user-prompt-submit validate: %v\n", err)
	}
	// Never block.
	return nil
}

// buildPromptValidator creates a PromptValidator from enforcement-policy.json.
func buildPromptValidator(projectDir string) *prompt.PromptValidator {
	v := &prompt.PromptValidator{
		ProjectDir:        projectDir,
		ProtectedBranches: prompt.DefaultProtectedBranches(),
	}

	policy, err := security.ReadEnforcementPolicy(projectDir)
	if err != nil {
		return v
	}

	if len(policy.ProtectedBranches) > 0 {
		v.ProtectedBranches = policy.ProtectedBranches
	}

	return v
}
