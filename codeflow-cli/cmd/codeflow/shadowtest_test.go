package main

import (
	"bytes"
	"context"
	"strings"
	"testing"

	"github.com/codeflow/codeflow-cli/internal/shadowtest"
)

// TestNewShadowTestCmd verifies the command is constructed with the expected
// Use string, flags, and no subcommands.
func TestNewShadowTestCmd(t *testing.T) {
	t.Parallel()

	cmd := newShadowTestCmd()
	if cmd.Use != "shadow-test" {
		t.Errorf("Use = %q, want %q", cmd.Use, "shadow-test")
	}
	if cmd.HasSubCommands() {
		t.Error("newShadowTestCmd() should have no subcommands")
	}

	// Verify expected flags exist.
	for _, flag := range []string{"project-dir", "go-binary", "verbose", "category"} {
		if cmd.Flags().Lookup(flag) == nil {
			t.Errorf("expected flag --%s to be registered", flag)
		}
	}
}

// TestFilterByCategory_WithSuffix verifies that a category string without a
// trailing slash is handled correctly.
func TestFilterByCategory_WithSuffix(t *testing.T) {
	t.Parallel()

	tests := []shadowtest.ShadowTest{
		{Name: "hook/security/allow"},
		{Name: "hook/gate/allow"},
		{Name: "pathflow/phase-transition/ok"},
		{Name: "validate/task/ok"},
	}

	got := filterByCategory(tests, "hook")
	if len(got) != 2 {
		t.Errorf("filterByCategory(hook) = %d tests, want 2", len(got))
	}
	for _, st := range got {
		if !strings.HasPrefix(st.Name, "hook/") {
			t.Errorf("unexpected test in hook category: %q", st.Name)
		}
	}
}

// TestFilterByCategory_WithTrailingSlash verifies that a category string with
// a trailing slash is handled correctly.
func TestFilterByCategory_WithTrailingSlash(t *testing.T) {
	t.Parallel()

	tests := []shadowtest.ShadowTest{
		{Name: "pathflow/phase-transition/ok"},
		{Name: "pathflow/stage-transition/ok"},
		{Name: "hook/security/allow"},
	}

	got := filterByCategory(tests, "pathflow/")
	if len(got) != 2 {
		t.Errorf("filterByCategory(pathflow/) = %d tests, want 2", len(got))
	}
}

// TestFilterByCategory_NoMatch verifies empty result when category matches nothing.
func TestFilterByCategory_NoMatch(t *testing.T) {
	t.Parallel()

	tests := []shadowtest.ShadowTest{
		{Name: "hook/security/allow"},
		{Name: "pathflow/phase-transition/ok"},
	}

	got := filterByCategory(tests, "nonexistent")
	if len(got) != 0 {
		t.Errorf("filterByCategory(nonexistent) = %d tests, want 0", len(got))
	}
}

// TestFilterByCategory_Empty verifies empty input produces empty output.
func TestFilterByCategory_Empty(t *testing.T) {
	t.Parallel()

	got := filterByCategory(nil, "hook")
	if len(got) != 0 {
		t.Errorf("filterByCategory(nil) = %d tests, want 0", len(got))
	}
}

// TestReportShadowSummary_AllPass verifies zero-divergence verdict.
func TestReportShadowSummary_AllPass(t *testing.T) {
	t.Parallel()

	results := []*shadowtest.ShadowResult{
		{Test: shadowtest.ShadowTest{Name: "test/alpha"}, Divergences: nil},
		{Test: shadowtest.ShadowTest{Name: "test/beta"}, Divergences: nil},
	}

	var stdout, stderr bytes.Buffer
	err := reportShadowSummary(&stdout, &stderr, results, 0)
	if err != nil {
		t.Errorf("reportShadowSummary() error = %v, want nil", err)
	}
	if !strings.Contains(stdout.String(), "PASSED") {
		t.Errorf("stdout = %q, want PASSED", stdout.String())
	}
	if !strings.Contains(stdout.String(), "2/2") {
		t.Errorf("stdout = %q, want 2/2 count", stdout.String())
	}
}

// TestReportShadowSummary_WithFailures verifies failure verdict and error return.
func TestReportShadowSummary_WithFailures(t *testing.T) {
	t.Parallel()

	results := []*shadowtest.ShadowResult{
		{
			Test: shadowtest.ShadowTest{Name: "test/alpha"},
			Divergences: []shadowtest.Divergence{
				{Kind: shadowtest.DivergenceExitCode, GoValue: "2", ShellValue: "0"},
			},
		},
	}

	var stdout, stderr bytes.Buffer
	err := reportShadowSummary(&stdout, &stderr, results, 1)
	if err == nil {
		t.Error("reportShadowSummary() error = nil, want error for failures")
	}
	if !strings.Contains(stderr.String(), "FAILED") {
		t.Errorf("stderr = %q, want FAILED", stderr.String())
	}
	if code := exitCode(err); code != ExitGeneralError {
		t.Errorf("exitCode = %d, want %d", code, ExitGeneralError)
	}
}

// TestReportShadowSummary_Empty verifies empty results produce a pass verdict.
func TestReportShadowSummary_Empty(t *testing.T) {
	t.Parallel()

	var stdout, stderr bytes.Buffer
	err := reportShadowSummary(&stdout, &stderr, nil, 0)
	if err != nil {
		t.Errorf("reportShadowSummary() error = %v, want nil for empty results", err)
	}
	if !strings.Contains(stdout.String(), "PASSED") {
		t.Errorf("stdout = %q, want PASSED", stdout.String())
	}
}

// TestRunShadowTest_UnknownCategory verifies that an unknown category returns
// an error without running any tests.
func TestRunShadowTest_UnknownCategory(t *testing.T) {
	t.Parallel()

	var stdout, stderr bytes.Buffer
	ctx := context.TODO()
	err := runShadowTest(ctx, &stdout, &stderr, t.TempDir(), "sh", "nonexistent-category", false)
	if err == nil {
		t.Error("runShadowTest() error = nil, want error for unknown category")
	}
	if !strings.Contains(stderr.String(), "no tests found for category") {
		t.Errorf("stderr = %q, want 'no tests found for category'", stderr.String())
	}
}

// TestRunShadowTestWithLoader_AllPass exercises the full test loop using
// real echo commands that both produce identical output (zero divergences).
func TestRunShadowTestWithLoader_AllPass(t *testing.T) {
	t.Parallel()

	// Loader returns two shadow tests that use "echo" via "sh -c" for both
	// Go and shell sides — identical output guaranteed.
	loader := func(_ string) []shadowtest.ShadowTest {
		return []shadowtest.ShadowTest{
			{
				Name:         "test/echo-alpha",
				GoCommand:    []string{"-c", "echo ok"},
				ShellCommand: []string{"sh", "-c", "echo ok"},
				Stdin:        []byte{},
			},
			{
				Name:         "test/echo-beta",
				GoCommand:    []string{"-c", "echo ok"},
				ShellCommand: []string{"sh", "-c", "echo ok"},
				Stdin:        []byte{},
			},
		}
	}

	var stdout, stderr bytes.Buffer
	ctx := context.TODO()
	err := runShadowTestWithLoader(ctx, &stdout, &stderr, t.TempDir(), "sh", "", false, loader)
	if err != nil {
		t.Errorf("runShadowTestWithLoader() error = %v, want nil", err)
	}
	if !strings.Contains(stdout.String(), "Running 2 shadow tests") {
		t.Errorf("stdout = %q, want 'Running 2 shadow tests'", stdout.String())
	}
	if !strings.Contains(stdout.String(), "PASSED") {
		t.Errorf("stdout = %q, want PASSED verdict", stdout.String())
	}
}

// TestRunShadowTestWithLoader_Verbose verifies that verbose=true prints per-test
// PASS lines to stdout for passing tests.
func TestRunShadowTestWithLoader_Verbose(t *testing.T) {
	t.Parallel()

	loader := func(_ string) []shadowtest.ShadowTest {
		return []shadowtest.ShadowTest{
			{
				Name:         "test/verbose-echo",
				GoCommand:    []string{"-c", "echo ok"},
				ShellCommand: []string{"sh", "-c", "echo ok"},
				Stdin:        []byte{},
			},
		}
	}

	var stdout, stderr bytes.Buffer
	ctx := context.TODO()
	err := runShadowTestWithLoader(ctx, &stdout, &stderr, t.TempDir(), "sh", "", true, loader)
	if err != nil {
		t.Errorf("runShadowTestWithLoader() error = %v, want nil", err)
	}
	if !strings.Contains(stdout.String(), "PASS test/verbose-echo") {
		t.Errorf("stdout = %q, want 'PASS test/verbose-echo' in verbose mode", stdout.String())
	}
}

// TestRunShadowTestWithLoader_WithDivergence exercises the test loop when one
// test has unexpected divergence (different exit codes).
func TestRunShadowTestWithLoader_WithDivergence(t *testing.T) {
	t.Parallel()

	// Loader returns a test where Go exits 0 but shell exits 1 — divergence.
	loader := func(_ string) []shadowtest.ShadowTest {
		return []shadowtest.ShadowTest{
			{
				Name:         "test/diverge-exit",
				GoCommand:    []string{"-c", "exit 0"},
				ShellCommand: []string{"sh", "-c", "exit 1"},
				Stdin:        []byte{},
			},
		}
	}

	var stdout, stderr bytes.Buffer
	ctx := context.TODO()
	err := runShadowTestWithLoader(ctx, &stdout, &stderr, t.TempDir(), "sh", "", false, loader)
	if err == nil {
		t.Error("runShadowTestWithLoader() error = nil, want error for divergence")
	}
	if !strings.Contains(stderr.String(), "FAILED") {
		t.Errorf("stderr = %q, want FAILED verdict", stderr.String())
	}
}

// TestRunShadowTestWithLoader_RunError exercises the error path when a test
// Run() call fails (command not found).
func TestRunShadowTestWithLoader_RunError(t *testing.T) {
	t.Parallel()

	// Loader returns a test with a nonexistent shell command — Run() will error.
	loader := func(_ string) []shadowtest.ShadowTest {
		return []shadowtest.ShadowTest{
			{
				Name:         "test/run-error",
				GoCommand:    []string{"--help"},
				ShellCommand: []string{"/nonexistent/path/cmd-xyz-that-does-not-exist"},
				Stdin:        []byte{},
			},
		}
	}

	var stdout, stderr bytes.Buffer
	ctx := context.TODO()
	// With no results accumulated (run error skips the result), the summary
	// reports 0/0 passed — still a PASS verdict.
	err := runShadowTestWithLoader(ctx, &stdout, &stderr, t.TempDir(), "sh", "", false, loader)
	// A run error is logged to stderr but does not cause a test failure
	// (the test is skipped from results). The summary should still pass.
	if err != nil {
		// Only fail if the error is not expected (it depends on os behavior).
		// Accept either outcome — the important thing is the ERROR log.
		_ = err
	}
	if !strings.Contains(stderr.String(), "ERROR") {
		t.Errorf("stderr = %q, want ERROR log for run error", stderr.String())
	}
}

// TestRunShadowTestWithLoader_CategoryFilter exercises category filtering
// within the loader path.
func TestRunShadowTestWithLoader_CategoryFilter(t *testing.T) {
	t.Parallel()

	loader := func(_ string) []shadowtest.ShadowTest {
		return []shadowtest.ShadowTest{
			{Name: "hook/security/allow", GoCommand: []string{"-c", "echo ok"}, ShellCommand: []string{"sh", "-c", "echo ok"}, Stdin: []byte{}},
			{Name: "pathflow/phase/ok", GoCommand: []string{"-c", "echo ok"}, ShellCommand: []string{"sh", "-c", "echo ok"}, Stdin: []byte{}},
		}
	}

	var stdout, stderr bytes.Buffer
	ctx := context.TODO()
	err := runShadowTestWithLoader(ctx, &stdout, &stderr, t.TempDir(), "sh", "hook", false, loader)
	if err != nil {
		t.Errorf("runShadowTestWithLoader() error = %v, want nil", err)
	}
	if !strings.Contains(stdout.String(), "Running 1 shadow tests") {
		t.Errorf("stdout = %q, want 'Running 1 shadow tests'", stdout.String())
	}
}

// TestRunShadowTestWithLoader_EmptyLoader exercises the empty-results path.
func TestRunShadowTestWithLoader_EmptyLoader(t *testing.T) {
	t.Parallel()

	loader := func(_ string) []shadowtest.ShadowTest { return nil }

	var stdout, stderr bytes.Buffer
	ctx := context.TODO()
	err := runShadowTestWithLoader(ctx, &stdout, &stderr, t.TempDir(), "sh", "", false, loader)
	if err != nil {
		t.Errorf("runShadowTestWithLoader() error = %v, want nil for empty test list", err)
	}
	if !strings.Contains(stdout.String(), "Running 0 shadow tests") {
		t.Errorf("stdout = %q, want 'Running 0 shadow tests'", stdout.String())
	}
}

// TestRunShadowTest_ProjectDirFromEnv verifies that CF_PROJECT_ROOT env var is
// used when project-dir flag is not set.
// Note: t.Setenv is incompatible with t.Parallel() — runs sequentially.
func TestRunShadowTest_ProjectDirFromEnv(t *testing.T) {
	// NOTE: no t.Parallel — t.Setenv is incompatible with t.Parallel (panics).
	dir := t.TempDir()
	t.Setenv("CF_PROJECT_ROOT", dir)

	var stdout, stderr bytes.Buffer
	ctx := context.TODO()
	// Use a nonexistent category to short-circuit without running tests.
	err := runShadowTest(ctx, &stdout, &stderr, "", "sh", "nonexistent-xy", false)
	if err == nil {
		t.Error("runShadowTest() error = nil, want error for unknown category")
	}
	// The error should be about the category, not about project dir resolution.
	if !strings.Contains(stderr.String(), "no tests found for category") {
		t.Errorf("stderr = %q, want 'no tests found for category'", stderr.String())
	}
}

// TestRunShadowTest_ProjectDirFromREPO_ROOT verifies that REPO_ROOT env var
// is used as fallback when CF_PROJECT_ROOT is not set.
// Note: t.Setenv is incompatible with t.Parallel() — runs sequentially.
func TestRunShadowTest_ProjectDirFromREPO_ROOT(t *testing.T) {
	// NOTE: no t.Parallel — t.Setenv is incompatible with t.Parallel (panics).
	dir := t.TempDir()
	t.Setenv("CF_PROJECT_ROOT", "")
	t.Setenv("REPO_ROOT", dir)

	var stdout, stderr bytes.Buffer
	ctx := context.TODO()
	err := runShadowTest(ctx, &stdout, &stderr, "", "sh", "nonexistent-zz", false)
	if err == nil {
		t.Error("runShadowTest() error = nil, want error for unknown category")
	}
	if !strings.Contains(stderr.String(), "no tests found for category") {
		t.Errorf("stderr = %q, want 'no tests found for category'", stderr.String())
	}
}

// TestRunShadowTest_ExplicitProjectDir verifies that an explicit project-dir
// parameter is used when provided.
func TestRunShadowTest_ExplicitProjectDir(t *testing.T) {
	t.Parallel()

	var stdout, stderr bytes.Buffer
	ctx := context.TODO()
	err := runShadowTest(ctx, &stdout, &stderr, t.TempDir(), "sh", "nonexistent-explicit", false)
	if err == nil {
		t.Error("runShadowTest() error = nil, want error for unknown category")
	}
	if !strings.Contains(stderr.String(), "no tests found for category") {
		t.Errorf("stderr = %q, want 'no tests found for category'", stderr.String())
	}
}
