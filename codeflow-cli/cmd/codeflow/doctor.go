package main

import (
	"bufio"
	"context"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"time"

	"github.com/codeflow/codeflow-cli/internal/doctor"
	"github.com/spf13/cobra"
)

// newDoctorCmd creates the "doctor" command for infrastructure diagnostics.
func newDoctorCmd() *cobra.Command {
	var (
		check  string
		asJSON bool
		reset  bool
		repair bool
		yes    bool
	)

	cmd := &cobra.Command{
		Use:   "doctor",
		Short: "Run infrastructure health checks",
		Long:  "Runs 16 health checks against the CodeFlow infrastructure and reports results. Supports repair actions for common issues.",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runDoctor(cmd, check, asJSON, reset, repair, yes)
		},
	}

	cmd.Flags().StringVar(&check, "check", "", "run a single named check (e.g., database, jsonl)")
	cmd.Flags().BoolVar(&asJSON, "json", false, "output results as machine-readable JSON")
	cmd.Flags().BoolVar(&reset, "reset", false, "reset .state/ directories (requires confirmation)")
	cmd.Flags().BoolVar(&repair, "repair", false, "run repair actions for failed checks")
	cmd.Flags().BoolVar(&yes, "yes", false, "skip confirmation prompts")

	return cmd
}

// runDoctor implements the doctor command logic.
func runDoctor(cmd *cobra.Command, check string, asJSON, reset, repair, yes bool) error {
	ctx := cmd.Context()
	if ctx == nil {
		ctx = context.Background()
	}

	opts := &doctor.Options{
		DBPath:     defaultDBPath(),
		LedgerDir:  defaultLedgerDir(),
		ProjectDir: ".",
		StateDir:   filepath.Join(".", ".state"),
		LookPath:   exec.LookPath,
		ExecCommand: func(name string, args ...string) ([]byte, error) {
			return exec.Command(name, args...).CombinedOutput()
		},
	}

	// Handle --reset flag.
	if reset {
		return runDoctorReset(cmd, opts, yes)
	}

	// Handle --repair flag.
	if repair {
		return runDoctorRepair(cmd, ctx, opts)
	}

	// Handle --check flag for single check.
	if check != "" {
		result, err := doctor.RunCheck(ctx, check, opts)
		if err != nil {
			return &exitError{code: ExitGeneralError, err: err}
		}

		if asJSON {
			return outputJSON(cmd, []doctor.Result{result})
		}

		printResultTable(cmd, []doctor.Result{result})

		if result.Status == doctor.StatusFail {
			return &exitError{
				code: classifyDoctorResult(result),
				err:  fmt.Errorf("check %q failed: %s", result.Name, result.Message),
			}
		}
		return nil
	}

	// Run all checks.
	results := doctor.RunAll(ctx, opts)

	if asJSON {
		return outputJSON(cmd, results)
	}

	printResultTable(cmd, results)

	// Return error if any critical checks failed.
	for _, r := range results {
		if r.Status == doctor.StatusFail {
			return &exitError{
				code: ExitRuntimeError,
				err:  fmt.Errorf("one or more health checks failed"),
			}
		}
	}

	return nil
}

// runDoctorReset handles the --reset flag.
func runDoctorReset(cmd *cobra.Command, opts *doctor.Options, yes bool) error {
	stateDir := opts.StateDir

	if !yes {
		fmt.Fprintf(cmd.OutOrStdout(), "Reset state directories in %s? This will remove all session data. [y/N] ", stateDir)
		scanner := bufio.NewScanner(cmd.InOrStdin())
		if !scanner.Scan() {
			return fmt.Errorf("reading confirmation: %w", scanner.Err())
		}
		answer := strings.TrimSpace(strings.ToLower(scanner.Text()))
		if answer != "y" && answer != "yes" {
			fmt.Fprintln(cmd.OutOrStdout(), "reset cancelled")
			return nil
		}
	}

	// Remove and recreate state subdirectories.
	subdirs := []string{"db", "ledger", "logs", "runtime", "sentinels"}
	for _, dir := range subdirs {
		path := filepath.Join(stateDir, dir)
		if err := os.RemoveAll(path); err != nil {
			return &exitError{
				code: ExitRuntimeError,
				err:  fmt.Errorf("removing %s: %w", path, err),
			}
		}
		if err := os.MkdirAll(path, 0o755); err != nil {
			return &exitError{
				code: ExitRuntimeError,
				err:  fmt.Errorf("recreating %s: %w", path, err),
			}
		}
	}

	fmt.Fprintln(cmd.OutOrStdout(), "state directories reset successfully")
	return nil
}

// runDoctorRepair handles the --repair flag.
func runDoctorRepair(cmd *cobra.Command, ctx context.Context, opts *doctor.Options) error {
	w := cmd.OutOrStdout()

	fmt.Fprintln(w, "Running repairs...")

	fmt.Fprint(w, "  Repairing database... ")
	if err := doctor.RepairDatabase(ctx, opts); err != nil {
		fmt.Fprintf(w, "FAILED: %v\n", err)
	} else {
		fmt.Fprintln(w, "OK")
	}

	fmt.Fprint(w, "  Repairing permissions... ")
	if err := doctor.RepairPermissions(ctx, opts); err != nil {
		fmt.Fprintf(w, "FAILED: %v\n", err)
	} else {
		fmt.Fprintln(w, "OK")
	}

	fmt.Fprint(w, "  Repairing config... ")
	if err := doctor.RepairConfig(ctx, opts); err != nil {
		fmt.Fprintf(w, "FAILED: %v\n", err)
	} else {
		fmt.Fprintln(w, "OK")
	}

	fmt.Fprintln(w, "\nRepair complete. Run 'codeflow doctor' to verify.")
	return nil
}

// outputJSON writes results as JSON to the command output.
func outputJSON(cmd *cobra.Command, results []doctor.Result) error {
	// Convert Duration to milliseconds for JSON output.
	type jsonResult struct {
		Name       string `json:"name"`
		Status     string `json:"status"`
		Message    string `json:"message"`
		DurationMs int64  `json:"duration_ms"`
	}

	jResults := make([]jsonResult, len(results))
	for i, r := range results {
		jResults[i] = jsonResult{
			Name:       r.Name,
			Status:     string(r.Status),
			Message:    r.Message,
			DurationMs: r.Duration.Milliseconds(),
		}
	}

	data, err := json.MarshalIndent(jResults, "", "  ")
	if err != nil {
		return &exitError{code: ExitInternalError, err: fmt.Errorf("marshaling JSON: %w", err)}
	}

	fmt.Fprintln(cmd.OutOrStdout(), string(data))
	return nil
}

// printResultTable prints check results in a formatted table.
func printResultTable(cmd *cobra.Command, results []doctor.Result) {
	w := cmd.OutOrStdout()

	// Find the maximum name length for alignment.
	maxName := 0
	for _, r := range results {
		if len(r.Name) > maxName {
			maxName = len(r.Name)
		}
	}

	fmt.Fprintln(w, "CodeFlow Doctor")
	fmt.Fprintln(w, strings.Repeat("-", 60))

	for _, r := range results {
		var statusIcon string
		switch r.Status {
		case doctor.StatusPass:
			statusIcon = "PASS"
		case doctor.StatusFail:
			statusIcon = "FAIL"
		case doctor.StatusWarn:
			statusIcon = "WARN"
		}

		padding := strings.Repeat(" ", maxName-len(r.Name))
		fmt.Fprintf(w, "  %-4s  %s%s  %s  (%s)\n",
			statusIcon, r.Name, padding, r.Message, r.Duration.Truncate(time.Millisecond))
	}

	fmt.Fprintln(w, strings.Repeat("-", 60))

	// Summary line.
	var pass, fail, warn int
	for _, r := range results {
		switch r.Status {
		case doctor.StatusPass:
			pass++
		case doctor.StatusFail:
			fail++
		case doctor.StatusWarn:
			warn++
		}
	}

	fmt.Fprintf(w, "Results: %d passed, %d failed, %d warnings\n", pass, fail, warn)
}

// classifyDoctorResult maps a failed result to an exit code.
func classifyDoctorResult(r doctor.Result) int {
	switch r.Name {
	case "database", "jsonl", "crdt", "config":
		return ExitRuntimeError
	case "claude", "auth", "python", "network":
		return ExitExternalError
	default:
		return ExitRuntimeError
	}
}
