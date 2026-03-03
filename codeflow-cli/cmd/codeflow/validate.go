package main

import (
	"fmt"
	"io"

	"github.com/codeflow/codeflow-cli/internal/validate"
	"github.com/spf13/cobra"
)

// newValidateCmd creates the validate command group.
func newValidateCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "validate",
		Short: "Validate task and epic markdown files",
		Long:  "Validate YAML frontmatter and structural integrity of task and epic markdown files.",
		RunE: func(cmd *cobra.Command, _ []string) error {
			return cmd.Help()
		},
	}

	cmd.AddCommand(newValidateTaskCmd())
	cmd.AddCommand(newValidateEpicCmd())

	return cmd
}

// newValidateTaskCmd creates the "validate task" subcommand.
func newValidateTaskCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "task <file>",
		Short: "Validate a task markdown file",
		Long:  "Validate YAML frontmatter fields, enum values, patterns, cross-field constraints, and required sections in a task markdown file.",
		Args:  cobra.ExactArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			return runValidateTask(cmd.OutOrStdout(), cmd.ErrOrStderr(), args[0])
		},
	}
}

// newValidateEpicCmd creates the "validate epic" subcommand.
func newValidateEpicCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "epic <file>",
		Short: "Validate an epic markdown file",
		Long:  "Validate YAML frontmatter fields, enum values, patterns, and required sections in an epic markdown file.",
		Args:  cobra.ExactArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			return runValidateEpic(cmd.OutOrStdout(), cmd.ErrOrStderr(), args[0])
		},
	}
}

// runValidateTask implements the task validation command logic.
func runValidateTask(w, errW io.Writer, path string) error {
	errs, warns, err := validate.ValidateTask(path)
	if err != nil {
		fmt.Fprintf(errW, "[ERROR] %v\n", err)
		return &exitError{code: ExitGeneralError, err: err}
	}

	return reportValidationResults(w, errW, errs, warns)
}

// runValidateEpic implements the epic validation command logic.
func runValidateEpic(w, errW io.Writer, path string) error {
	errs, warns, err := validate.ValidateEpic(path)
	if err != nil {
		fmt.Fprintf(errW, "[ERROR] %v\n", err)
		return &exitError{code: ExitGeneralError, err: err}
	}

	return reportValidationResults(w, errW, errs, warns)
}

// reportValidationResults formats and outputs validation results.
func reportValidationResults(w, errW io.Writer, errs []validate.ValidationError, warns []validate.ValidationWarning) error {
	for _, e := range errs {
		fmt.Fprintf(errW, "[ERROR] %v\n", e)
	}
	for _, wrn := range warns {
		fmt.Fprintf(errW, "[WARN] %v\n", wrn)
	}

	if len(errs) > 0 {
		msg := fmt.Sprintf("Validation FAILED with %d error(s)", len(errs))
		fmt.Fprintf(errW, "%s\n", msg)
		return &exitError{code: ExitGeneralError, err: fmt.Errorf("%s", msg)}
	}

	if len(warns) > 0 {
		fmt.Fprintf(w, "[INFO] Validation PASSED with %d warning(s)\n", len(warns))
	} else {
		fmt.Fprintf(w, "[INFO] Validation PASSED\n")
	}

	return nil
}
