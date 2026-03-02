package main

import (
	"bytes"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/codeflow/codeflow-cli/internal/settings"
	"github.com/spf13/cobra"
)

func TestNewSettingsCmd(t *testing.T) {
	t.Parallel()

	cmd := newSettingsCmd()
	if cmd.Use != "settings" {
		t.Errorf("newSettingsCmd().Use = %q, want %q", cmd.Use, "settings")
	}

	subs := cmd.Commands()
	names := make(map[string]bool)
	for _, sub := range subs {
		names[sub.Name()] = true
	}
	if !names["setup-managed"] {
		t.Error("missing subcommand 'setup-managed'")
	}
	if !names["validate"] {
		t.Error("missing subcommand 'validate'")
	}
}

func TestNewSetupManagedCmd(t *testing.T) {
	t.Parallel()

	cmd := newSetupManagedCmd()
	if cmd.Use != "setup-managed" {
		t.Errorf("Use = %q, want setup-managed", cmd.Use)
	}

	// Verify dry-run flag exists.
	flag := cmd.Flags().Lookup("dry-run")
	if flag == nil {
		t.Error("missing --dry-run flag")
	}
}

func TestNewSettingsValidateCmd(t *testing.T) {
	t.Parallel()

	cmd := newSettingsValidateCmd()
	if cmd.Use != "validate" {
		t.Errorf("Use = %q, want validate", cmd.Use)
	}
}

func TestIsSettingsTemplateEdit(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name string
		data string
		want bool
	}{
		{
			name: "Edit on settings template",
			data: `{"tool_name":"Edit","tool_input":{"file_path":"/project/.claude/settings-templates/autonomous.json"}}`,
			want: true,
		},
		{
			name: "Write on settings template",
			data: `{"tool_name":"Write","tool_input":{"file_path":"/project/.claude/settings-templates/standard.json"}}`,
			want: true,
		},
		{
			name: "Edit on non-template",
			data: `{"tool_name":"Edit","tool_input":{"file_path":"/project/src/main.go"}}`,
			want: false,
		},
		{
			name: "Bash tool (not Edit/Write)",
			data: `{"tool_name":"Bash","tool_input":{"command":"ls"}}`,
			want: false,
		},
		{
			name: "Edit on non-json settings template",
			data: `{"tool_name":"Edit","tool_input":{"file_path":"/project/.claude/settings-templates/readme.md"}}`,
			want: false,
		},
		{
			name: "empty data",
			data: "",
			want: false,
		},
		{
			name: "invalid JSON",
			data: "not json",
			want: false,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			got := isSettingsTemplateEdit([]byte(tt.data))
			if got != tt.want {
				t.Errorf("isSettingsTemplateEdit() = %v, want %v", got, tt.want)
			}
		})
	}
}

func TestRunSettingsValidateHook_NonTemplate(t *testing.T) {
	t.Parallel()

	// Non-template edit should produce no output.
	stdin := strings.NewReader(`{"tool_name":"Edit","tool_input":{"file_path":"/project/src/main.go"}}`)
	var stdout bytes.Buffer

	cmd := newSettingsValidateHookCmd()
	cmd.SetIn(stdin)
	cmd.SetOut(&stdout)

	err := cmd.Execute()
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if stdout.Len() > 0 {
		t.Errorf("expected no output for non-template; got: %s", stdout.String())
	}
}

func TestRunSettingsValidateHook_TemplateEdit(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	templateDir := filepath.Join(dir, ".claude", "settings-templates")
	if err := os.MkdirAll(templateDir, 0o755); err != nil {
		t.Fatal(err)
	}
	hooksDir := filepath.Join(dir, ".claude", "hooks", "codeflow", "pre-tool-use")
	if err := os.MkdirAll(hooksDir, 0o755); err != nil {
		t.Fatal(err)
	}

	tmpl := map[string]any{
		"_version": "1.0.0",
		"hooks": map[string]any{
			"pre-tool-use": []map[string]any{
				{"command": ".claude/hooks/codeflow/pre-tool-use/test.sh"},
			},
		},
	}
	data, _ := json.MarshalIndent(tmpl, "", "  ")
	for _, name := range []string{"autonomous.json", "standard.json"} {
		if err := os.WriteFile(filepath.Join(templateDir, name), data, 0o644); err != nil {
			t.Fatal(err)
		}
	}

	// Create referenced hook script.
	if err := os.WriteFile(filepath.Join(hooksDir, "test.sh"), []byte("#!/bin/bash"), 0o755); err != nil {
		t.Fatal(err)
	}

	// Create settings.json and settings.local.json matching template.
	if err := os.WriteFile(filepath.Join(dir, ".claude", "settings.json"), data, 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dir, ".claude", "settings.local.json"), data, 0o644); err != nil {
		t.Fatal(err)
	}

	// Call runSettingsValidateHook through the command layer with injected project dir.
	stdin := strings.NewReader(`{"tool_name":"Edit","tool_input":{"file_path":"/project/.claude/settings-templates/autonomous.json"}}`)
	cmd := newSettingsValidateHookCmd()
	cmd.SetIn(stdin)
	var stdout bytes.Buffer
	cmd.SetOut(&stdout)

	// Override RunE to inject test project dir.
	cmd.RunE = func(c *cobra.Command, _ []string) error {
		return runSettingsValidateHook(c, func() string { return dir })
	}

	err := cmd.Execute()
	if err != nil {
		t.Fatalf("runSettingsValidateHook() unexpected error: %v", err)
	}

	output := stdout.String()
	if output == "" {
		t.Error("expected non-empty hook output for template validation")
	}
	if strings.Contains(output, "hookSpecificOutput") {
		var parsed map[string]any
		if err := json.Unmarshal([]byte(strings.TrimSpace(output)), &parsed); err != nil {
			t.Errorf("output should be valid JSON; got: %s", output)
		}
	}
}

func TestCountFailures(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name    string
		results []settings.ValidationResult
		want    int
	}{
		{
			name:    "no results",
			results: nil,
			want:    0,
		},
		{
			name: "all pass",
			results: []settings.ValidationResult{
				{Passed: true},
				{Passed: true},
			},
			want: 0,
		},
		{
			name: "one failure",
			results: []settings.ValidationResult{
				{Passed: true},
				{Passed: false},
			},
			want: 1,
		},
		{
			name: "all fail",
			results: []settings.ValidationResult{
				{Passed: false},
				{Passed: false},
				{Passed: false},
			},
			want: 3,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			got := countFailures(tt.results)
			if got != tt.want {
				t.Errorf("countFailures() = %d, want %d", got, tt.want)
			}
		})
	}
}

func TestRunSettingsValidate(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	templateDir := filepath.Join(dir, ".claude", "settings-templates")
	if err := os.MkdirAll(templateDir, 0o755); err != nil {
		t.Fatal(err)
	}
	hooksDir := filepath.Join(dir, ".claude", "hooks", "codeflow", "pre-tool-use")
	if err := os.MkdirAll(hooksDir, 0o755); err != nil {
		t.Fatal(err)
	}

	tmpl := map[string]any{
		"_version": "1.0.0",
		"hooks": map[string]any{
			"pre-tool-use": []map[string]any{
				{"command": ".claude/hooks/codeflow/pre-tool-use/test.sh"},
			},
		},
	}
	data, _ := json.MarshalIndent(tmpl, "", "  ")
	for _, name := range []string{"autonomous.json", "standard.json"} {
		if err := os.WriteFile(filepath.Join(templateDir, name), data, 0o644); err != nil {
			t.Fatal(err)
		}
	}
	if err := os.WriteFile(filepath.Join(hooksDir, "test.sh"), []byte("#!/bin/bash"), 0o755); err != nil {
		t.Fatal(err)
	}
	// Matching settings files.
	if err := os.WriteFile(filepath.Join(dir, ".claude", "settings.json"), data, 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dir, ".claude", "settings.local.json"), data, 0o644); err != nil {
		t.Fatal(err)
	}

	// Call runSettingsValidate through the command layer with injected project dir.
	cmd := newSettingsValidateCmd()
	// Override RunE to inject test project dir.
	cmd.RunE = func(c *cobra.Command, _ []string) error {
		return runSettingsValidate(c, dir)
	}
	var stdout bytes.Buffer
	cmd.SetOut(&stdout)

	err := cmd.Execute()
	if err != nil {
		t.Fatalf("runSettingsValidate() unexpected error: %v", err)
	}

	output := stdout.String()
	if !strings.Contains(output, "Settings Template Validation") {
		t.Errorf("FormatResults output missing header; got: %s", output)
	}
}

func TestRunSettingsValidateHook_EmptyStdin(t *testing.T) {
	t.Parallel()

	stdin := strings.NewReader("")
	var stdout bytes.Buffer

	cmd := newSettingsValidateHookCmd()
	cmd.SetIn(stdin)
	cmd.SetOut(&stdout)

	err := cmd.Execute()
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if stdout.Len() > 0 {
		t.Errorf("expected no output for empty stdin; got: %s", stdout.String())
	}
}

func TestRunSettingsValidateHook_ValidationError(t *testing.T) {
	t.Parallel()

	// Template edit input with a project dir that has no templates directory.
	// This triggers the ValidateSettingsTemplates error path (line 132-136).
	stdin := strings.NewReader(`{"tool_name":"Edit","tool_input":{"file_path":"/project/.claude/settings-templates/autonomous.json"}}`)
	cmd := newSettingsValidateHookCmd()
	cmd.SetIn(stdin)
	var stdout, stderr bytes.Buffer
	cmd.SetOut(&stdout)
	cmd.SetErr(&stderr)

	// Override RunE to inject a nonexistent project dir.
	cmd.RunE = func(c *cobra.Command, _ []string) error {
		return runSettingsValidateHook(c, func() string { return "/nonexistent/project" })
	}

	err := cmd.Execute()
	// Advisory hook — always exits 0.
	if err != nil {
		t.Errorf("expected nil error (advisory hook); got: %v", err)
	}
	if stdout.Len() > 0 {
		t.Errorf("expected no stdout for validation error; got: %s", stdout.String())
	}
}

func TestRunSettingsValidate_ProjectDirError(t *testing.T) {
	t.Parallel()

	// Nonexistent project dir triggers ValidateSettingsTemplates error path.
	cmd := newSettingsValidateCmd()
	cmd.RunE = func(c *cobra.Command, _ []string) error {
		return runSettingsValidate(c, "/nonexistent/project")
	}
	var stdout bytes.Buffer
	cmd.SetOut(&stdout)
	cmd.SilenceUsage = true

	err := cmd.Execute()
	if err == nil {
		t.Error("expected error for nonexistent project dir")
	}
	if !strings.Contains(err.Error(), "settings validation") {
		t.Errorf("error = %v, want 'settings validation' prefix", err)
	}
}

func TestRunSetupManaged_DryRun(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()

	// Create the source file at the path that SourceFilePath(dir) will resolve to.
	sourceDir := filepath.Join(dir, ".codeflow", "docs", "security", "claude-enterprise")
	if err := os.MkdirAll(sourceDir, 0o755); err != nil {
		t.Fatal(err)
	}
	sourceFile := filepath.Join(sourceDir, "managed-settings.json")
	if err := os.WriteFile(sourceFile, []byte(`{"test": true}`), 0o644); err != nil {
		t.Fatal(err)
	}

	// Call runSetupManaged through the command layer with injected project dir.
	cmd := newSetupManagedCmd()
	cmd.SetArgs([]string{"--dry-run"})
	// Override RunE to inject test project dir.
	cmd.RunE = func(c *cobra.Command, _ []string) error {
		return runSetupManaged(c, dir, true)
	}
	var stdout bytes.Buffer
	cmd.SetOut(&stdout)

	err := cmd.Execute()
	if err != nil {
		t.Fatalf("runSetupManaged(dryRun=true) error: %v", err)
	}

	output := stdout.String()
	if !strings.Contains(output, "Dry run complete") {
		t.Errorf("expected 'Dry run complete' in output; got: %s", output)
	}
}

func TestRunSettingsValidate_WithFailures(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	templateDir := filepath.Join(dir, ".claude", "settings-templates")
	if err := os.MkdirAll(templateDir, 0o755); err != nil {
		t.Fatal(err)
	}

	// Create two templates with DIFFERENT versions to trigger version mismatch.
	tmpl1 := map[string]any{
		"_version": "1.0.0",
		"hooks":    map[string]any{},
	}
	tmpl2 := map[string]any{
		"_version": "2.0.0",
		"hooks":    map[string]any{},
	}
	data1, _ := json.MarshalIndent(tmpl1, "", "  ")
	data2, _ := json.MarshalIndent(tmpl2, "", "  ")
	if err := os.WriteFile(filepath.Join(templateDir, "autonomous.json"), data1, 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(templateDir, "standard.json"), data2, 0o644); err != nil {
		t.Fatal(err)
	}

	// Create .claude dir with settings that won't match.
	if err := os.WriteFile(filepath.Join(dir, ".claude", "settings.json"), []byte(`{}`), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dir, ".claude", "settings.local.json"), []byte(`{}`), 0o644); err != nil {
		t.Fatal(err)
	}

	// Call runSettingsValidate through the command layer with injected project dir.
	cmd := newSettingsValidateCmd()
	cmd.RunE = func(c *cobra.Command, _ []string) error {
		return runSettingsValidate(c, dir)
	}
	var stdout bytes.Buffer
	cmd.SetOut(&stdout)
	cmd.SilenceUsage = true

	err := cmd.Execute()
	// Should return an error (exitError) due to validation failures.
	if err == nil {
		t.Error("expected error from mismatched template versions")
	}

	output := stdout.String()
	if !strings.Contains(output, "Settings Template Validation") {
		t.Errorf("expected validation output; got: %s", output)
	}
}

func TestNewSettingsCmd_Execute(t *testing.T) {
	t.Parallel()

	// Executing settings with no subcommand should show help (not error).
	cmd := newSettingsCmd()
	var stdout bytes.Buffer
	cmd.SetOut(&stdout)
	cmd.SetArgs([]string{})

	err := cmd.Execute()
	if err != nil {
		t.Errorf("settings command with no args should not error; got: %v", err)
	}
}

func TestNewSetupManagedCmd_Execute_NoSource(t *testing.T) {
	t.Parallel()

	// Call runSetupManaged with a nonexistent project dir (source file won't exist).
	cmd := newSetupManagedCmd()
	cmd.RunE = func(c *cobra.Command, _ []string) error {
		return runSetupManaged(c, "/nonexistent/project", true)
	}
	var stdout bytes.Buffer
	cmd.SetOut(&stdout)
	cmd.SilenceUsage = true

	err := cmd.Execute()
	if err == nil {
		t.Error("expected error for missing source file")
	}
	if !strings.Contains(err.Error(), "source file not found") {
		t.Errorf("error = %v, want 'source file not found'", err)
	}
}

func TestSetupManagedCmd_RunE_Coverage(t *testing.T) {
	t.Parallel()

	// Exercise the native RunE closure (which calls detectProjectDir)
	// to cover the wiring code in newSetupManagedCmd.
	cmd := newSetupManagedCmd()
	cmd.SetArgs([]string{"--dry-run"})
	var stdout bytes.Buffer
	cmd.SetOut(&stdout)
	cmd.SilenceUsage = true
	cmd.SilenceErrors = true

	// This will likely error because the real project dir may not have the
	// managed-settings.json source file, but the RunE closure is exercised.
	_ = cmd.Execute()
}

func TestSettingsValidateCmd_RunE_Coverage(t *testing.T) {
	t.Parallel()

	// Exercise the native RunE closure (which calls detectProjectDir)
	// to cover the wiring code in newSettingsValidateCmd.
	cmd := newSettingsValidateCmd()
	var stdout bytes.Buffer
	cmd.SetOut(&stdout)
	cmd.SilenceUsage = true
	cmd.SilenceErrors = true

	// This will likely error because the real project dir may not match
	// expected template structure, but the RunE closure is exercised.
	_ = cmd.Execute()
}
