package main

import (
	"context"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"

	"github.com/codeflow/codeflow-cli/internal/shadowtest"
	"github.com/spf13/cobra"
)

// newShadowTestCmd creates the "shadow-test" command that runs all shadow tests
// comparing Go and shell implementations for behavioral parity.
//
// Named "shadow-test" (not "test shadow") to avoid conflict with the
// ./codeflow test shell wrapper that handles the unified test suite.
func newShadowTestCmd() *cobra.Command {
	var (
		projectDir string
		goBinary   string
		verbose    bool
		category   string
	)

	cmd := &cobra.Command{
		Use:   "shadow-test",
		Short: "Run shadow tests comparing Go and shell implementation parity",
		Long: `Run shadow tests that execute each Go subcommand alongside its shell script
equivalent with identical stdin/env, compare outputs using schema-aware
normalization, and report divergences.

Shadow tests validate behavioral parity before Phase E cutover that replaces
shell invocations with Go binary calls. All expected schema evolution differences
(type->event, ts->timestamp, id field absence, etc.) are excluded via normalization.

Exit code: 0 if all tests pass or only known divergences found.
           1 if any unexpected divergences detected.`,
		Args: cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runShadowTest(cmd.Context(), cmd.OutOrStdout(), cmd.ErrOrStderr(), projectDir, goBinary, category, verbose)
		},
	}

	cmd.Flags().StringVar(&projectDir, "project-dir", "", "Project root directory (defaults to CF_PROJECT_ROOT or current directory)")
	cmd.Flags().StringVar(&goBinary, "go-binary", "", "Path to codeflow binary (defaults to self-resolved path)")
	cmd.Flags().BoolVar(&verbose, "verbose", false, "Print results for passing tests as well as failing ones")
	cmd.Flags().StringVar(&category, "category", "", "Run only tests in this category (hook, pathflow, validate)")

	return cmd
}

// runShadowTest executes all shadow tests and reports divergences.
func runShadowTest(ctx context.Context, w, errW io.Writer, projectDir, goBinary, category string, verbose bool) error {
	if projectDir == "" {
		projectDir = os.Getenv("CF_PROJECT_ROOT")
	}
	if projectDir == "" {
		projectDir = os.Getenv("REPO_ROOT")
	}
	if projectDir == "" {
		var err error
		projectDir, err = os.Getwd()
		if err != nil {
			return fmt.Errorf("shadow-test: cannot determine project directory: %w", err)
		}
	}
	projectDir, _ = filepath.Abs(projectDir)

	if goBinary == "" {
		var err error
		goBinary, err = os.Executable()
		if err != nil {
			return fmt.Errorf("shadow-test: cannot determine self path: %w", err)
		}
	}

	return runShadowTestWithLoader(ctx, w, errW, projectDir, goBinary, category, verbose, shadowtest.AllShadowTests)
}

// runShadowTestWithLoader runs shadow tests using the provided test loader
// function. Separated from runShadowTest to allow injection of a custom loader
// in tests without spawning real shell processes.
func runShadowTestWithLoader(ctx context.Context, w, errW io.Writer, projectDir, goBinary, category string, verbose bool, loader func(string) []shadowtest.ShadowTest) error {
	allTests := loader(projectDir)

	// Filter by category if requested.
	if category != "" {
		allTests = filterByCategory(allTests, category)
		if len(allTests) == 0 {
			msg := fmt.Sprintf("no tests found for category %q", category)
			fmt.Fprintf(errW, "shadow-test: %s\n", msg)
			return &exitError{code: ExitGeneralError, err: fmt.Errorf("%s", msg)}
		}
	}

	fmt.Fprintf(w, "Running %d shadow tests...\n", len(allTests))

	var results []*shadowtest.ShadowResult
	unexpectedCount := 0

	for _, st := range allTests {
		stCopy := st
		result, err := stCopy.Run(ctx, goBinary, projectDir)
		if err != nil {
			fmt.Fprintf(errW, "  ERROR %s: %v\n", st.Name, err)
			continue
		}

		results = append(results, result)
		if result.HasUnexpectedDivergences() {
			unexpectedCount++
			fmt.Fprintf(errW, "  FAIL %s: %d divergence(s)\n", st.Name, len(result.Divergences))
		} else if verbose {
			fmt.Fprintf(w, "  PASS %s\n", st.Name)
		}
	}

	// Print divergence report.
	report := shadowtest.FormatDivergenceReport(results)
	fmt.Fprint(w, report)

	return reportShadowSummary(w, errW, results, unexpectedCount)
}

// reportShadowSummary prints the final verdict and returns an error if any
// unexpected divergences were found.
func reportShadowSummary(w, errW io.Writer, results []*shadowtest.ShadowResult, unexpectedCount int) error {
	total := len(results)
	passed := total - unexpectedCount

	if unexpectedCount > 0 {
		msg := fmt.Sprintf("shadow-test FAILED: %d unexpected divergence(s) in %d/%d tests",
			unexpectedCount, unexpectedCount, total)
		fmt.Fprintf(errW, "%s\n", msg)
		return &exitError{code: ExitGeneralError, err: fmt.Errorf("%s", msg)}
	}

	fmt.Fprintf(w, "shadow-test PASSED: %d/%d tests passed (zero unexpected divergences)\n", passed, total)
	return nil
}

// filterByCategory returns only shadow tests whose Name starts with the given
// category prefix (e.g., "hook/", "pathflow/", "validate/").
func filterByCategory(tests []shadowtest.ShadowTest, category string) []shadowtest.ShadowTest {
	prefix := category
	if !strings.HasSuffix(prefix, "/") {
		prefix += "/"
	}
	var filtered []shadowtest.ShadowTest
	for _, t := range tests {
		if strings.HasPrefix(t.Name, prefix) {
			filtered = append(filtered, t)
		}
	}
	return filtered
}
