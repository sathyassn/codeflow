package main

import (
	"fmt"
	"io"

	"github.com/codeflow/codeflow-cli/internal/hooks/edit"
	"github.com/codeflow/codeflow-cli/internal/hooks/security"
	"github.com/spf13/cobra"
)

// newEditWriteGuardCmd creates the "edit-write-guard" subcommand that enforces
// Edit/Write file path scope from enforcement-policy.json.
func newEditWriteGuardCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "edit-write-guard",
		Short: "Validate Edit/Write file paths against scope rules",
		Long: `Validate Edit/Write tool file paths against enforcement-policy.json scope rules.

Reads Claude Code hook JSON from stdin, extracts the file path, and validates
it against blocked directories, allowed temp prefixes, protected branch
restrictions, and dangerous extension checks.

Exits 0 if allowed, 2 if blocked. Warnings for dangerous extensions are
written to stderr but still exit 0.

Stdin format:
  {"tool_name":"Edit","tool_input":{"file_path":"/abs/path/to/file.go"}}`,
		Args: cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runEditWriteGuard(cmd.InOrStdin(), cmd.OutOrStdout(), cmd.ErrOrStderr())
		},
	}
}

// runEditWriteGuard implements the edit-write-guard enforcement logic.
func runEditWriteGuard(stdin io.Reader, _ io.Writer, errW io.Writer) error {
	projectDir := detectProjectDir()
	checker := buildScopeChecker(projectDir)

	verdict, err := checker.Check(stdin)
	if err != nil {
		// Errors are not security violations — allow through.
		return nil
	}

	if !verdict.Allow {
		fmt.Fprint(errW, verdict.Message)
		return &exitError{code: ExitHookBlock, err: fmt.Errorf("edit-write-guard: blocked")}
	}

	// Print warnings (dangerous extensions) to stderr.
	if verdict.Message != "" {
		fmt.Fprintln(errW, verdict.Message)
	}

	return nil
}

// buildScopeChecker creates a ScopeChecker from enforcement-policy.json.
func buildScopeChecker(projectDir string) *edit.ScopeChecker {
	checker := &edit.ScopeChecker{
		ProjectDir:    projectDir,
		CurrentBranch: detectCurrentBranch(),
	}

	policy, err := security.ReadEnforcementPolicy(projectDir)
	if err != nil {
		// Defaults when policy unavailable.
		checker.BlockedDirs = edit.DefaultBlockedDirs()
		checker.AllowedTmpPrefixes = edit.DefaultAllowedTmpPrefixes()
		checker.DangerousExts = edit.DefaultDangerousExtensions()
		checker.WarnOnDangerous = true
		checker.ProtectedBranches = edit.DefaultProtectedBranches()
		return checker
	}

	// Load from policy.
	if len(policy.EditWrite.BlockedDirectories) > 0 {
		checker.BlockedDirs = policy.EditWrite.BlockedDirectories
	} else {
		checker.BlockedDirs = edit.DefaultBlockedDirs()
	}

	if len(policy.EditWrite.AllowedTmpPrefixes) > 0 {
		checker.AllowedTmpPrefixes = policy.EditWrite.AllowedTmpPrefixes
	} else {
		checker.AllowedTmpPrefixes = edit.DefaultAllowedTmpPrefixes()
	}

	checker.DangerousExts = edit.DangerousExtensions{
		Binary:     policy.EditWrite.DangerousExtensions.Binary,
		Credential: policy.EditWrite.DangerousExtensions.Credential,
		Archive:    policy.EditWrite.DangerousExtensions.Archive,
	}
	if len(checker.DangerousExts.Binary) == 0 {
		checker.DangerousExts = edit.DefaultDangerousExtensions()
	}

	checker.WarnOnDangerous = policy.EditWrite.WarnOnDangerous

	if len(policy.ProtectedBranches) > 0 {
		checker.ProtectedBranches = policy.ProtectedBranches
	} else {
		checker.ProtectedBranches = edit.DefaultProtectedBranches()
	}

	return checker
}
