package githooks

import (
	"os"
	"path/filepath"
	"testing"
)

func TestLoadEnforcementPolicy_ValidFile(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	cfgDir := filepath.Join(dir, ".codeflow", "config", "enforcement")
	if err := os.MkdirAll(cfgDir, 0o755); err != nil {
		t.Fatal(err)
	}

	content := `{
		"protected_branches": ["main", "master"],
		"sensitive_file_patterns": ["\\.env$", "credentials"],
		"git_format": {
			"commit_types": ["feat", "fix"],
			"branch_types": ["feat", "fix"],
			"branch_prefixes": ["feat/", "fix/"],
			"ai_attribution_patterns": ["Claude"],
			"subject": {
				"max_length": 50,
				"require_lowercase_type": true,
				"forbid_trailing_period": true
			},
			"body": {
				"format": "bullets_only",
				"max_bullets": 3,
				"line_max_length": 72
			}
		}
	}`
	if err := os.WriteFile(filepath.Join(cfgDir, "enforcement-policy.json"), []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}

	policy, err := LoadEnforcementPolicy(dir)
	if err != nil {
		t.Fatalf("expected no error, got: %v", err)
	}
	if len(policy.ProtectedBranches) != 2 {
		t.Errorf("expected 2 protected branches, got %d", len(policy.ProtectedBranches))
	}
	if len(policy.SensitiveFilePatterns) != 2 {
		t.Errorf("expected 2 sensitive file patterns, got %d", len(policy.SensitiveFilePatterns))
	}
	if len(policy.GitFormat.CommitTypes) != 2 {
		t.Errorf("expected 2 commit types, got %d", len(policy.GitFormat.CommitTypes))
	}
	if policy.GitFormat.Subject.MaxLength != 50 {
		t.Errorf("expected max_length 50, got %d", policy.GitFormat.Subject.MaxLength)
	}
}

func TestLoadEnforcementPolicy_NonexistentFile(t *testing.T) {
	t.Parallel()

	_, err := LoadEnforcementPolicy("/nonexistent/project")
	if err == nil {
		t.Fatal("expected error for nonexistent file")
	}
}

func TestLoadEnforcementPolicy_InvalidJSON(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	cfgDir := filepath.Join(dir, ".codeflow", "config", "enforcement")
	if err := os.MkdirAll(cfgDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(cfgDir, "enforcement-policy.json"), []byte("{invalid"), 0o644); err != nil {
		t.Fatal(err)
	}

	_, err := LoadEnforcementPolicy(dir)
	if err == nil {
		t.Fatal("expected error for invalid JSON")
	}
}

func TestDefaultPolicy(t *testing.T) {
	t.Parallel()

	policy := DefaultPolicy()
	if policy == nil {
		t.Fatal("expected non-nil policy")
	}
	if len(policy.ProtectedBranches) == 0 {
		t.Error("expected protected branches in default policy")
	}
	if len(policy.GitFormat.CommitTypes) == 0 {
		t.Error("expected commit types in default policy")
	}
	if len(policy.GitFormat.BranchTypes) == 0 {
		t.Error("expected branch types in default policy")
	}
	if len(policy.GitFormat.AIAttributionPatterns) == 0 {
		t.Error("expected AI attribution patterns in default policy")
	}
	if len(policy.SensitiveFilePatterns) == 0 {
		t.Error("expected sensitive file patterns in default policy")
	}
	if len(policy.SensitiveFilePatterns) != 9 {
		t.Errorf("expected 9 sensitive file patterns, got %d", len(policy.SensitiveFilePatterns))
	}
	if policy.GitFormat.Subject.MaxLength != 50 {
		t.Errorf("expected max_length 50, got %d", policy.GitFormat.Subject.MaxLength)
	}
	if policy.GitFormat.Body.MaxBullets != 3 {
		t.Errorf("expected max_bullets 3, got %d", policy.GitFormat.Body.MaxBullets)
	}
}
