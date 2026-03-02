package settings

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// setupTemplateDir creates a temp directory with settings template files and
// a minimal enforcement-policy.json. Returns the project root dir.
func setupTemplateDir(t *testing.T, templates map[string]any) string {
	t.Helper()

	dir := t.TempDir()

	// Create template directory.
	templateDir := filepath.Join(dir, ".claude", "settings-templates")
	if err := os.MkdirAll(templateDir, 0o755); err != nil {
		t.Fatal(err)
	}

	// Create hooks directory for wiring audit.
	hooksDir := filepath.Join(dir, ".claude", "hooks", "codeflow")
	if err := os.MkdirAll(hooksDir, 0o755); err != nil {
		t.Fatal(err)
	}

	// Write template files.
	for name, content := range templates {
		data, err := json.MarshalIndent(content, "", "  ")
		if err != nil {
			t.Fatalf("marshal template %s: %v", name, err)
		}
		if err := os.WriteFile(filepath.Join(templateDir, name), data, 0o644); err != nil {
			t.Fatalf("write template %s: %v", name, err)
		}
	}

	// Create enforcement-policy.json with copy_mappings.
	policyDir := filepath.Join(dir, ".codeflow", "config", "enforcement")
	if err := os.MkdirAll(policyDir, 0o755); err != nil {
		t.Fatal(err)
	}
	policy := map[string]any{
		"settings_templates": map[string]any{
			"directory": ".claude/settings-templates",
			"copy_mappings": []map[string]string{
				{"template": "autonomous.json", "destination": ".claude/settings.json", "purpose": "Project settings"},
				{"template": "autonomous.json", "destination": ".claude/settings.local.json", "purpose": "Local settings"},
			},
		},
	}
	policyData, _ := json.MarshalIndent(policy, "", "  ")
	if err := os.WriteFile(filepath.Join(policyDir, "enforcement-policy.json"), policyData, 0o644); err != nil {
		t.Fatal(err)
	}

	return dir
}

// consistentTemplate returns a template map with hooks and _version.
func consistentTemplate(version string) map[string]any {
	return map[string]any{
		"_version": version,
		"hooks": map[string]any{
			"pre-tool-use": []map[string]any{
				{"command": ".claude/hooks/codeflow/pre-tool-use/test-hook.sh"},
			},
		},
	}
}

func TestValidateSettingsTemplates_AllPass(t *testing.T) {
	t.Parallel()

	tmpl := consistentTemplate("1.0.0")
	dir := setupTemplateDir(t, map[string]any{
		"autonomous.json": tmpl,
		"standard.json":   tmpl,
	})

	// Create hook script referenced by templates.
	hooksDir := filepath.Join(dir, ".claude", "hooks", "codeflow", "pre-tool-use")
	if err := os.MkdirAll(hooksDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(hooksDir, "test-hook.sh"), []byte("#!/bin/bash"), 0o755); err != nil {
		t.Fatal(err)
	}

	// Create destination files matching the template.
	templatePath := filepath.Join(dir, ".claude", "settings-templates", "autonomous.json")
	templateData, _ := os.ReadFile(templatePath)
	settingsDir := filepath.Join(dir, ".claude")
	if err := os.WriteFile(filepath.Join(settingsDir, "settings.json"), templateData, 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(settingsDir, "settings.local.json"), templateData, 0o644); err != nil {
		t.Fatal(err)
	}

	results, err := ValidateSettingsTemplates(dir)
	if err != nil {
		t.Fatalf("ValidateSettingsTemplates() error: %v", err)
	}

	if HasFailures(results) {
		for _, r := range results {
			if !r.Passed {
				t.Errorf("unexpected failure: [%s] %s: %s", r.Check, r.Message, r.Details)
			}
		}
	}

	// Expect 5 checks: hooks SHA256, version, hook wiring, settings checksum, settings.local checksum.
	if len(results) != 5 {
		t.Errorf("expected 5 results, got %d", len(results))
	}
}

func TestValidateSettingsTemplates_HooksMismatch(t *testing.T) {
	t.Parallel()

	dir := setupTemplateDir(t, map[string]any{
		"autonomous.json": map[string]any{
			"_version": "1.0.0",
			"hooks": map[string]any{
				"pre-tool-use": []map[string]any{
					{"command": ".claude/hooks/codeflow/pre-tool-use/hook-a.sh"},
				},
			},
		},
		"standard.json": map[string]any{
			"_version": "1.0.0",
			"hooks": map[string]any{
				"pre-tool-use": []map[string]any{
					{"command": ".claude/hooks/codeflow/pre-tool-use/hook-b.sh"},
				},
			},
		},
	})

	results, err := ValidateSettingsTemplates(dir)
	if err != nil {
		t.Fatalf("ValidateSettingsTemplates() error: %v", err)
	}

	// Find the hooks SHA256 check.
	var hooksResult *ValidationResult
	for i := range results {
		if results[i].Check == CheckHooksSHA256 {
			hooksResult = &results[i]
			break
		}
	}

	if hooksResult == nil {
		t.Fatal("expected hooks_sha256 check result")
	}
	if hooksResult.Passed {
		t.Error("hooks_sha256 should fail when templates have different hooks")
	}
	if !strings.Contains(hooksResult.Details, "standard.json") {
		t.Errorf("Details should mention mismatched template; got: %s", hooksResult.Details)
	}
}

func TestValidateSettingsTemplates_VersionMismatch(t *testing.T) {
	t.Parallel()

	dir := setupTemplateDir(t, map[string]any{
		"autonomous.json": consistentTemplate("1.0.0"),
		"standard.json":   consistentTemplate("2.0.0"),
	})

	results, err := ValidateSettingsTemplates(dir)
	if err != nil {
		t.Fatalf("ValidateSettingsTemplates() error: %v", err)
	}

	var versionResult *ValidationResult
	for i := range results {
		if results[i].Check == CheckVersionConsistency {
			versionResult = &results[i]
			break
		}
	}

	if versionResult == nil {
		t.Fatal("expected version_consistency check result")
	}
	if versionResult.Passed {
		t.Error("version_consistency should fail when versions differ")
	}
}

func TestValidateSettingsTemplates_TooFewTemplates(t *testing.T) {
	t.Parallel()

	dir := setupTemplateDir(t, map[string]any{
		"single.json": consistentTemplate("1.0.0"),
	})

	_, err := ValidateSettingsTemplates(dir)
	if err == nil {
		t.Error("expected error with fewer than 2 templates")
	}
	if !strings.Contains(err.Error(), "at least 2") {
		t.Errorf("error should mention minimum template count; got: %v", err)
	}
}

func TestValidateSettingsTemplates_NoConfig(t *testing.T) {
	t.Parallel()

	// Create directory with templates but no enforcement-policy.json.
	dir := t.TempDir()
	templateDir := filepath.Join(dir, ".claude", "settings-templates")
	if err := os.MkdirAll(templateDir, 0o755); err != nil {
		t.Fatal(err)
	}
	hooksDir := filepath.Join(dir, ".claude", "hooks", "codeflow")
	if err := os.MkdirAll(hooksDir, 0o755); err != nil {
		t.Fatal(err)
	}

	tmpl := consistentTemplate("1.0.0")
	for _, name := range []string{"autonomous.json", "standard.json"} {
		data, _ := json.MarshalIndent(tmpl, "", "  ")
		if err := os.WriteFile(filepath.Join(templateDir, name), data, 0o644); err != nil {
			t.Fatal(err)
		}
	}

	// Should use defaults (no error).
	results, err := ValidateSettingsTemplates(dir)
	if err != nil {
		t.Fatalf("ValidateSettingsTemplates() should work with defaults; error: %v", err)
	}
	if len(results) == 0 {
		t.Error("expected at least some results")
	}
}

func TestHasFailures(t *testing.T) {
	t.Parallel()

	t.Run("no failures", func(t *testing.T) {
		t.Parallel()
		results := []ValidationResult{
			{Passed: true},
			{Passed: true},
		}
		if HasFailures(results) {
			t.Error("HasFailures() = true, want false")
		}
	})

	t.Run("with failure", func(t *testing.T) {
		t.Parallel()
		results := []ValidationResult{
			{Passed: true},
			{Passed: false},
		}
		if !HasFailures(results) {
			t.Error("HasFailures() = false, want true")
		}
	})

	t.Run("empty", func(t *testing.T) {
		t.Parallel()
		if HasFailures(nil) {
			t.Error("HasFailures(nil) = true, want false")
		}
	})
}

func TestFormatResults(t *testing.T) {
	t.Parallel()

	results := []ValidationResult{
		{Check: CheckHooksSHA256, Passed: true, Message: "All hooks match"},
		{Check: CheckVersionConsistency, Passed: false, Message: "Version mismatch", Details: "standard.json=2.0"},
	}

	output := FormatResults(results)

	if !strings.Contains(output, "[PASS]") {
		t.Error("output should contain [PASS]")
	}
	if !strings.Contains(output, "[FAIL]") {
		t.Error("output should contain [FAIL]")
	}
	if !strings.Contains(output, "1 passed, 1 failed") {
		t.Errorf("summary incorrect; got: %s", output)
	}
	if !strings.Contains(output, "standard.json=2.0") {
		t.Error("output should include details")
	}
}

func TestFormatHookOutput(t *testing.T) {
	t.Parallel()

	results := []ValidationResult{
		{Check: CheckHooksSHA256, Passed: true, Message: "All hooks match"},
		{Check: CheckVersionConsistency, Passed: false, Message: "Version mismatch"},
	}

	output, err := FormatHookOutput(results)
	if err != nil {
		t.Fatalf("FormatHookOutput() error: %v", err)
	}

	// Verify it's valid JSON.
	var parsed map[string]any
	if err := json.Unmarshal([]byte(output), &parsed); err != nil {
		t.Fatalf("FormatHookOutput() produced invalid JSON: %v; output: %s", err, output)
	}

	// Verify structure.
	hso, ok := parsed["hookSpecificOutput"]
	if !ok {
		t.Fatal("output missing hookSpecificOutput key")
	}
	hsoMap, ok := hso.(map[string]any)
	if !ok {
		t.Fatal("hookSpecificOutput is not a map")
	}
	if hsoMap["hookEventName"] != "PostToolUse" {
		t.Errorf("hookEventName = %v, want PostToolUse", hsoMap["hookEventName"])
	}
	ctx, ok := hsoMap["additionalContext"].(string)
	if !ok {
		t.Fatal("additionalContext is not a string")
	}
	if !strings.Contains(ctx, "[OK]") {
		t.Error("additionalContext should contain [OK] for passed check")
	}
	if !strings.Contains(ctx, "[ISSUE]") {
		t.Error("additionalContext should contain [ISSUE] for failed check")
	}
}

func TestHooksHash(t *testing.T) {
	t.Parallel()

	t.Run("consistent hashes", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		tmpl := `{"hooks":{"pre":["a","b"]}}`
		for _, name := range []string{"a.json", "b.json"} {
			if err := os.WriteFile(filepath.Join(dir, name), []byte(tmpl), 0o644); err != nil {
				t.Fatal(err)
			}
		}

		h1, err := hooksHash(filepath.Join(dir, "a.json"))
		if err != nil {
			t.Fatalf("hooksHash() error: %v", err)
		}
		h2, err := hooksHash(filepath.Join(dir, "b.json"))
		if err != nil {
			t.Fatalf("hooksHash() error: %v", err)
		}
		if h1 != h2 {
			t.Errorf("identical hooks should produce same hash; got %s vs %s", h1, h2)
		}
	})

	t.Run("missing hooks section", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		if err := os.WriteFile(filepath.Join(dir, "no-hooks.json"), []byte(`{"_version":"1"}`), 0o644); err != nil {
			t.Fatal(err)
		}

		_, err := hooksHash(filepath.Join(dir, "no-hooks.json"))
		if err == nil {
			t.Error("expected error for missing hooks section")
		}
	})

	t.Run("file not found", func(t *testing.T) {
		t.Parallel()
		_, err := hooksHash("/nonexistent/file.json")
		if err == nil {
			t.Error("expected error for missing file")
		}
	})
}

func TestTemplateVersion(t *testing.T) {
	t.Parallel()

	t.Run("version present", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		if err := os.WriteFile(filepath.Join(dir, "v.json"), []byte(`{"_version":"2.5"}`), 0o644); err != nil {
			t.Fatal(err)
		}

		ver, err := templateVersion(filepath.Join(dir, "v.json"))
		if err != nil {
			t.Fatalf("templateVersion() error: %v", err)
		}
		if ver != "2.5" {
			t.Errorf("templateVersion() = %q, want %q", ver, "2.5")
		}
	})

	t.Run("version missing", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		if err := os.WriteFile(filepath.Join(dir, "nover.json"), []byte(`{"hooks":{}}`), 0o644); err != nil {
			t.Fatal(err)
		}

		ver, err := templateVersion(filepath.Join(dir, "nover.json"))
		if err != nil {
			t.Fatalf("templateVersion() error: %v", err)
		}
		if ver != "missing" {
			t.Errorf("templateVersion() = %q, want %q", ver, "missing")
		}
	})
}

func TestFileHash(t *testing.T) {
	t.Parallel()

	t.Run("identical files same hash", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		content := []byte("test content")
		for _, name := range []string{"a.txt", "b.txt"} {
			if err := os.WriteFile(filepath.Join(dir, name), content, 0o644); err != nil {
				t.Fatal(err)
			}
		}

		h1, err := fileHash(filepath.Join(dir, "a.txt"))
		if err != nil {
			t.Fatal(err)
		}
		h2, err := fileHash(filepath.Join(dir, "b.txt"))
		if err != nil {
			t.Fatal(err)
		}
		if h1 != h2 {
			t.Errorf("same content should produce same hash; got %s vs %s", h1, h2)
		}
	})

	t.Run("different files different hash", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		if err := os.WriteFile(filepath.Join(dir, "a.txt"), []byte("aaa"), 0o644); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(dir, "b.txt"), []byte("bbb"), 0o644); err != nil {
			t.Fatal(err)
		}

		h1, _ := fileHash(filepath.Join(dir, "a.txt"))
		h2, _ := fileHash(filepath.Join(dir, "b.txt"))
		if h1 == h2 {
			t.Error("different content should produce different hash")
		}
	})

	t.Run("missing file", func(t *testing.T) {
		t.Parallel()
		_, err := fileHash("/nonexistent/file.txt")
		if err == nil {
			t.Error("expected error for missing file")
		}
	})
}

func TestExtractReferencedScripts(t *testing.T) {
	t.Parallel()

	t.Run("finds hook commands", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		template := `{
			"hooks": {
				"pre-tool-use": [
					{"command": ".claude/hooks/codeflow/pre-tool-use/hook-a.sh"},
					{"command": ".claude/hooks/codeflow/pre-tool-use/hook-b.sh"}
				],
				"post-tool-use": [
					{"command": ".claude/hooks/codeflow/post-tool-use/hook-c.sh"}
				]
			}
		}`
		path := filepath.Join(dir, "template.json")
		if err := os.WriteFile(path, []byte(template), 0o644); err != nil {
			t.Fatal(err)
		}

		scripts, err := extractReferencedScripts(path)
		if err != nil {
			t.Fatalf("extractReferencedScripts() error: %v", err)
		}
		if len(scripts) != 3 {
			t.Errorf("expected 3 scripts, got %d: %v", len(scripts), scripts)
		}
	})

	t.Run("deduplicates", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		template := `{
			"hooks": {
				"a": [{"command": ".claude/hooks/codeflow/pre/hook.sh"}],
				"b": [{"command": ".claude/hooks/codeflow/post/hook.sh"}]
			}
		}`
		path := filepath.Join(dir, "template.json")
		if err := os.WriteFile(path, []byte(template), 0o644); err != nil {
			t.Fatal(err)
		}

		scripts, err := extractReferencedScripts(path)
		if err != nil {
			t.Fatal(err)
		}
		// Same basename "hook.sh" should be deduplicated.
		if len(scripts) != 1 {
			t.Errorf("expected 1 unique script, got %d: %v", len(scripts), scripts)
		}
	})

	t.Run("no hooks", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		path := filepath.Join(dir, "empty.json")
		if err := os.WriteFile(path, []byte(`{"_version":"1"}`), 0o644); err != nil {
			t.Fatal(err)
		}

		scripts, err := extractReferencedScripts(path)
		if err != nil {
			t.Fatal(err)
		}
		if len(scripts) != 0 {
			t.Errorf("expected 0 scripts, got %d", len(scripts))
		}
	})
}

func TestFindExistingScripts(t *testing.T) {
	t.Parallel()

	t.Run("finds sh files recursively", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		sub := filepath.Join(dir, "pre-tool-use")
		if err := os.MkdirAll(sub, 0o755); err != nil {
			t.Fatal(err)
		}
		for _, name := range []string{"hook-a.sh", "hook-b.sh"} {
			if err := os.WriteFile(filepath.Join(sub, name), []byte("#!/bin/bash"), 0o755); err != nil {
				t.Fatal(err)
			}
		}
		// Non-sh file should be ignored.
		if err := os.WriteFile(filepath.Join(sub, "readme.md"), []byte("docs"), 0o644); err != nil {
			t.Fatal(err)
		}

		scripts, err := findExistingScripts(dir)
		if err != nil {
			t.Fatalf("findExistingScripts() error: %v", err)
		}
		if len(scripts) != 2 {
			t.Errorf("expected 2 scripts, got %d: %v", len(scripts), scripts)
		}
	})

	t.Run("nonexistent dir returns empty", func(t *testing.T) {
		t.Parallel()
		scripts, err := findExistingScripts("/nonexistent/hooks/dir")
		// WalkDir returns an error for nonexistent root.
		// If it doesn't error, it should return empty.
		if err != nil {
			return // error is acceptable
		}
		if len(scripts) != 0 {
			t.Errorf("expected 0 scripts for nonexistent dir, got %d", len(scripts))
		}
	})
}

func TestCheckFileChecksum(t *testing.T) {
	t.Parallel()

	t.Run("matching checksum", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		templateDir := filepath.Join(dir, "templates")
		if err := os.MkdirAll(templateDir, 0o755); err != nil {
			t.Fatal(err)
		}

		content := []byte(`{"test": true}`)
		if err := os.WriteFile(filepath.Join(templateDir, "source.json"), content, 0o644); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(dir, "dest.json"), content, 0o644); err != nil {
			t.Fatal(err)
		}

		mapping := CopyMapping{Template: "source.json", Destination: "dest.json"}
		result := checkFileChecksum(dir, templateDir, mapping)
		if !result.Passed {
			t.Errorf("expected pass for matching files; got: %s", result.Message)
		}
	})

	t.Run("mismatched checksum", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		templateDir := filepath.Join(dir, "templates")
		if err := os.MkdirAll(templateDir, 0o755); err != nil {
			t.Fatal(err)
		}

		if err := os.WriteFile(filepath.Join(templateDir, "source.json"), []byte("original"), 0o644); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(dir, "dest.json"), []byte("modified"), 0o644); err != nil {
			t.Fatal(err)
		}

		mapping := CopyMapping{Template: "source.json", Destination: "dest.json"}
		result := checkFileChecksum(dir, templateDir, mapping)
		if result.Passed {
			t.Error("expected fail for mismatched files")
		}
		if !strings.Contains(result.Details, "Fix: cp") {
			t.Errorf("Details should contain fix command; got: %s", result.Details)
		}
	})

	t.Run("settings.local.json uses correct check type", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		templateDir := filepath.Join(dir, "templates")
		if err := os.MkdirAll(templateDir, 0o755); err != nil {
			t.Fatal(err)
		}

		content := []byte("same")
		if err := os.WriteFile(filepath.Join(templateDir, "src.json"), content, 0o644); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(dir, "settings.local.json"), content, 0o644); err != nil {
			t.Fatal(err)
		}

		mapping := CopyMapping{Template: "src.json", Destination: "settings.local.json"}
		result := checkFileChecksum(dir, templateDir, mapping)
		if result.Check != CheckSettingsLocalChecksum {
			t.Errorf("Check = %q, want %q", result.Check, CheckSettingsLocalChecksum)
		}
	})
}

func TestLoadSettingsTemplatesConfig(t *testing.T) {
	t.Parallel()

	t.Run("valid config", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		policyDir := filepath.Join(dir, ".codeflow", "config", "enforcement")
		if err := os.MkdirAll(policyDir, 0o755); err != nil {
			t.Fatal(err)
		}
		policy := `{
			"settings_templates": {
				"directory": "custom/templates",
				"copy_mappings": [
					{"template": "a.json", "destination": "b.json", "purpose": "test"}
				]
			}
		}`
		if err := os.WriteFile(filepath.Join(policyDir, "enforcement-policy.json"), []byte(policy), 0o644); err != nil {
			t.Fatal(err)
		}

		config, err := loadSettingsTemplatesConfig(dir)
		if err != nil {
			t.Fatalf("loadSettingsTemplatesConfig() error: %v", err)
		}
		if config.Directory != "custom/templates" {
			t.Errorf("Directory = %q, want custom/templates", config.Directory)
		}
		if len(config.CopyMappings) != 1 {
			t.Errorf("CopyMappings len = %d, want 1", len(config.CopyMappings))
		}
	})

	t.Run("missing config uses defaults", func(t *testing.T) {
		t.Parallel()
		config, err := loadSettingsTemplatesConfig(t.TempDir())
		if err != nil {
			t.Fatalf("loadSettingsTemplatesConfig() error: %v", err)
		}
		if config.Directory != ".claude/settings-templates" {
			t.Errorf("Directory = %q, want default", config.Directory)
		}
		if len(config.CopyMappings) != 2 {
			t.Errorf("CopyMappings len = %d, want 2 defaults", len(config.CopyMappings))
		}
	})

	t.Run("empty directory uses default", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		policyDir := filepath.Join(dir, ".codeflow", "config", "enforcement")
		if err := os.MkdirAll(policyDir, 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(policyDir, "enforcement-policy.json"), []byte(`{"settings_templates":{}}`), 0o644); err != nil {
			t.Fatal(err)
		}

		config, err := loadSettingsTemplatesConfig(dir)
		if err != nil {
			t.Fatal(err)
		}
		if config.Directory != ".claude/settings-templates" {
			t.Errorf("Directory = %q, want default", config.Directory)
		}
	})
}

func TestCheckHooksConsistency_ReadError(t *testing.T) {
	t.Parallel()

	// Create only one template with an unreadable hooks section path.
	dir := t.TempDir()
	templateDir := filepath.Join(dir, "templates")
	if err := os.MkdirAll(templateDir, 0o755); err != nil {
		t.Fatal(err)
	}

	// Template with hooks.
	good := `{"hooks":{"pre":["a"]}}`
	// Template with invalid JSON (will cause read error on hooks hash).
	bad := `not json`
	if err := os.WriteFile(filepath.Join(templateDir, "good.json"), []byte(good), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(templateDir, "bad.json"), []byte(bad), 0o644); err != nil {
		t.Fatal(err)
	}

	result := checkHooksConsistency(templateDir, []string{"good.json", "bad.json"})
	if result.Passed {
		t.Error("expected failure when second template can't be parsed")
	}
	if !strings.Contains(result.Details, "bad.json") {
		t.Errorf("Details should mention bad template; got: %s", result.Details)
	}
}

func TestCheckVersionConsistency_ReadError(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	templateDir := filepath.Join(dir, "templates")
	if err := os.MkdirAll(templateDir, 0o755); err != nil {
		t.Fatal(err)
	}

	if err := os.WriteFile(filepath.Join(templateDir, "good.json"), []byte(`{"_version":"1.0"}`), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(templateDir, "bad.json"), []byte(`not json`), 0o644); err != nil {
		t.Fatal(err)
	}

	result := checkVersionConsistency(templateDir, []string{"good.json", "bad.json"})
	if result.Passed {
		t.Error("expected failure when second template can't be parsed")
	}
}

func TestCheckHooksConsistency_RefReadError(t *testing.T) {
	t.Parallel()

	// Reference template itself is invalid.
	dir := t.TempDir()
	templateDir := filepath.Join(dir, "templates")
	if err := os.MkdirAll(templateDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(templateDir, "ref.json"), []byte("not json"), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(templateDir, "other.json"), []byte(`{"hooks":{}}`), 0o644); err != nil {
		t.Fatal(err)
	}

	result := checkHooksConsistency(templateDir, []string{"ref.json", "other.json"})
	if result.Passed {
		t.Error("expected failure when reference template can't be parsed")
	}
}

func TestCheckVersionConsistency_RefReadError(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	templateDir := filepath.Join(dir, "templates")
	if err := os.MkdirAll(templateDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(templateDir, "ref.json"), []byte("not json"), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(templateDir, "other.json"), []byte(`{"_version":"1.0"}`), 0o644); err != nil {
		t.Fatal(err)
	}

	result := checkVersionConsistency(templateDir, []string{"ref.json", "other.json"})
	if result.Passed {
		t.Error("expected failure when reference template can't be parsed")
	}
}

func TestCheckHookWiring_HooksDirError(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	// Template exists but hooks dir does not.
	templateDir := filepath.Join(dir, ".claude", "settings-templates")
	if err := os.MkdirAll(templateDir, 0o755); err != nil {
		t.Fatal(err)
	}
	tmpl := `{"hooks":{"pre":[{"command":".claude/hooks/codeflow/pre/test.sh"}]}}`
	templateFile := filepath.Join(templateDir, "autonomous.json")
	if err := os.WriteFile(templateFile, []byte(tmpl), 0o644); err != nil {
		t.Fatal(err)
	}

	result := checkHookWiring(dir, templateFile)
	if result.Passed {
		t.Error("expected failure when hooks dir doesn't exist")
	}
}

func TestDiscoverTemplates(t *testing.T) {
	t.Parallel()

	t.Run("finds json files sorted", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		for _, name := range []string{"c.json", "a.json", "b.json"} {
			if err := os.WriteFile(filepath.Join(dir, name), []byte("{}"), 0o644); err != nil {
				t.Fatal(err)
			}
		}
		// Non-json should be excluded.
		if err := os.WriteFile(filepath.Join(dir, "readme.md"), []byte(""), 0o644); err != nil {
			t.Fatal(err)
		}

		templates, err := discoverTemplates(dir)
		if err != nil {
			t.Fatal(err)
		}
		if len(templates) != 3 {
			t.Errorf("expected 3 templates, got %d", len(templates))
		}
		if templates[0] != "a.json" {
			t.Errorf("first template = %q, want a.json (sorted)", templates[0])
		}
	})

	t.Run("skips directories", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		if err := os.MkdirAll(filepath.Join(dir, "subdir.json"), 0o755); err != nil {
			t.Fatal(err)
		}

		templates, err := discoverTemplates(dir)
		if err != nil {
			t.Fatal(err)
		}
		if len(templates) != 0 {
			t.Errorf("directories should be excluded; got %v", templates)
		}
	})

	t.Run("nonexistent dir", func(t *testing.T) {
		t.Parallel()
		_, err := discoverTemplates("/nonexistent/dir")
		if err == nil {
			t.Error("expected error for nonexistent directory")
		}
	})
}
