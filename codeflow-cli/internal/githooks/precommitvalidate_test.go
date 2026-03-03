package githooks

import (
	"bytes"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestCheckBranchProtection(t *testing.T) {
	t.Parallel()

	policy := testPolicy()
	tests := []struct {
		name     string
		branch   string
		wantErr  bool
		errMatch string
	}{
		{"protected main", "main", true, "BRANCH PROTECTION"},
		{"protected master", "master", true, "BRANCH PROTECTION"},
		{"protected production", "production", true, "BRANCH PROTECTION"},
		{"protected release", "release/1.0", true, "BRANCH PROTECTION"},
		{"unprotected feat", "feat/add-login", false, ""},
		{"unprotected develop", "develop", false, ""},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			errs := &PreCommitErrors{}
			checkBranchProtection(tt.branch, policy, errs)
			if tt.wantErr && len(errs.Errors) == 0 {
				t.Error("expected branch protection error")
			}
			if !tt.wantErr && len(errs.Errors) > 0 {
				t.Errorf("unexpected error: %v", errs.Errors)
			}
			if tt.wantErr && len(errs.Errors) > 0 && !strings.Contains(errs.Errors[0], tt.errMatch) {
				t.Errorf("expected error containing %q, got: %s", tt.errMatch, errs.Errors[0])
			}
		})
	}
}

func TestCheckBranchName(t *testing.T) {
	t.Parallel()

	policy := testPolicy()
	tests := []struct {
		name    string
		branch  string
		wantErr bool
	}{
		{"valid feat", "feat/add-login", false},
		{"valid fix", "fix/resolve-bug", false},
		{"valid docs", "docs/update-readme", false},
		{"invalid no slash", "main", true},
		{"invalid uppercase", "feat/Add-Login", true},
		{"invalid unknown type", "unknown/something", true},
		{"invalid spaces", "feat/has space", true},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			errs := &PreCommitErrors{}
			w := &bytes.Buffer{}
			checkBranchName(w, tt.branch, policy, errs)
			if tt.wantErr && len(errs.Errors) == 0 {
				t.Error("expected branch name error")
			}
			if !tt.wantErr && len(errs.Errors) > 0 {
				t.Errorf("unexpected error: %v", errs.Errors)
			}
		})
	}
}

func TestCheckBranchName_RegexCompileError(t *testing.T) {
	t.Parallel()

	// Branch types containing invalid regex characters.
	policy := &EnforcementPolicy{
		GitFormat: GitFormat{BranchTypes: []string{"[invalid"}},
	}
	errs := &PreCommitErrors{}
	w := &bytes.Buffer{}
	// The function should handle regex compile error gracefully.
	checkBranchName(w, "feat/test", policy, errs)
	// When regex compile fails, the function returns early — no error added.
	// (Actually, the regex is built from types which get joined with |, so
	// a malformed type might cause compile failure.)
}

func TestCheckBranchName_EmptyTypes(t *testing.T) {
	t.Parallel()

	policy := &EnforcementPolicy{
		GitFormat: GitFormat{BranchTypes: nil},
	}
	errs := &PreCommitErrors{}
	w := &bytes.Buffer{}
	checkBranchName(w, "anything", policy, errs)
	if len(errs.Errors) > 0 {
		t.Error("expected no error when branch types are empty")
	}
}

func TestCheckSensitiveFiles(t *testing.T) {
	t.Parallel()

	policy := testPolicy()
	tests := []struct {
		name    string
		files   []string
		wantErr bool
	}{
		{"no sensitive", []string{"src/main.go", "README.md"}, false},
		{"env file", []string{"src/main.go", ".env"}, true},
		{"env.local", []string{".env.local"}, true},
		{"pem file", []string{"cert.pem"}, true},
		{"key file", []string{"server.key"}, true},
		{"credentials", []string{"credentials.json"}, true},
		{"id_rsa", []string{"id_rsa"}, true},
		{"id_ed25519", []string{"id_ed25519"}, true},
		{"secret file", []string{"app.secret"}, true},
		{"password file", []string{"password.txt"}, true},
		{"empty list", []string{}, false},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			errs := &PreCommitErrors{}
			w := &bytes.Buffer{}
			checkSensitiveFiles(w, tt.files, policy, errs)
			if tt.wantErr && len(errs.Errors) == 0 {
				t.Error("expected sensitive file error")
			}
			if !tt.wantErr && len(errs.Errors) > 0 {
				t.Errorf("unexpected error: %v", errs.Errors)
			}
		})
	}
}

func TestCheckSensitiveFiles_MultipleSensitive(t *testing.T) {
	t.Parallel()

	policy := testPolicy()
	// First file is sensitive — should return immediately after first match.
	files := []string{".env", "credentials.json", "server.key"}
	errs := &PreCommitErrors{}
	w := &bytes.Buffer{}
	checkSensitiveFiles(w, files, policy, errs)
	// Should have exactly 1 error (returns on first match).
	if len(errs.Errors) != 1 {
		t.Errorf("expected 1 error (first match), got %d: %v", len(errs.Errors), errs.Errors)
	}
}

func TestCheckSensitiveFiles_EmptyPatterns(t *testing.T) {
	t.Parallel()

	// Policy with no sensitive file patterns — should return early with no errors.
	policy := &EnforcementPolicy{SensitiveFilePatterns: nil}
	errs := &PreCommitErrors{}
	w := &bytes.Buffer{}
	checkSensitiveFiles(w, []string{".env", "credentials.json"}, policy, errs)
	if len(errs.Errors) > 0 {
		t.Errorf("expected no errors with empty patterns, got: %v", errs.Errors)
	}
}

func TestCheckSensitiveFiles_InvalidRegex(t *testing.T) {
	t.Parallel()

	// Policy with an invalid regex pattern — should be skipped gracefully.
	policy := &EnforcementPolicy{
		SensitiveFilePatterns: []string{"[invalid", `\.env$`},
	}
	errs := &PreCommitErrors{}
	w := &bytes.Buffer{}
	// The invalid pattern "[invalid" should be skipped, but ".env$" should still match.
	checkSensitiveFiles(w, []string{".env"}, policy, errs)
	if len(errs.Errors) == 0 {
		t.Error("expected error for .env matching valid pattern after invalid one")
	}
}

func TestCheckSensitiveFiles_InvalidRegexOnly(t *testing.T) {
	t.Parallel()

	// Policy with only invalid regex patterns — should skip all without error.
	policy := &EnforcementPolicy{
		SensitiveFilePatterns: []string{"[invalid", "[also-bad"},
	}
	errs := &PreCommitErrors{}
	w := &bytes.Buffer{}
	checkSensitiveFiles(w, []string{".env"}, policy, errs)
	if len(errs.Errors) > 0 {
		t.Errorf("expected no errors with only invalid patterns, got: %v", errs.Errors)
	}
}

func TestCheckJSONFiles_Valid(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	jsonFile := "test.json"
	if err := os.WriteFile(filepath.Join(dir, jsonFile), []byte(`{"key": "value"}`), 0o644); err != nil {
		t.Fatal(err)
	}

	errs := &PreCommitErrors{}
	w := &bytes.Buffer{}
	checkJSONFiles(w, dir, []string{jsonFile}, errs)
	if len(errs.Errors) > 0 {
		t.Errorf("expected valid JSON, got errors: %v", errs.Errors)
	}
	if !strings.Contains(w.String(), "JSON files are valid") {
		t.Error("expected success message")
	}
}

func TestCheckJSONFiles_Invalid(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	jsonFile := "bad.json"
	if err := os.WriteFile(filepath.Join(dir, jsonFile), []byte(`{invalid json`), 0o644); err != nil {
		t.Fatal(err)
	}

	errs := &PreCommitErrors{}
	w := &bytes.Buffer{}
	checkJSONFiles(w, dir, []string{jsonFile}, errs)
	if len(errs.Errors) == 0 {
		t.Error("expected error for invalid JSON")
	}
}

func TestCheckJSONFiles_NoJSON(t *testing.T) {
	t.Parallel()

	errs := &PreCommitErrors{}
	w := &bytes.Buffer{}
	checkJSONFiles(w, t.TempDir(), []string{"file.go", "file.md"}, errs)
	if len(errs.Errors) > 0 {
		t.Error("expected no errors for non-JSON files")
	}
	if w.Len() > 0 {
		t.Error("expected no output for non-JSON files")
	}
}

func TestCheckJSONFiles_DeletedFile(t *testing.T) {
	t.Parallel()

	// Staged file that doesn't exist on disk (deleted after staging).
	errs := &PreCommitErrors{}
	w := &bytes.Buffer{}
	checkJSONFiles(w, t.TempDir(), []string{"nonexistent.json"}, errs)
	// Should not error — deleted files are skipped.
	if len(errs.Errors) > 0 {
		t.Errorf("expected no errors for deleted file, got: %v", errs.Errors)
	}
}

func TestCheckGoTestConventions_ContextBackground(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	testFile := "example_test.go"
	// Build fixture via concatenation to avoid triggering the pre-commit
	// convention scanner on this test file itself.
	ctxCall := "context." + "Background()"
	content := "package example\n\nimport (\n\t\"context\"\n\t\"testing\"\n)\n\n" +
		"func TestExample(t *testing.T) {\n\tt.Parallel()\n\tctx := " + ctxCall + "\n\t_ = ctx\n}\n"
	if err := os.WriteFile(filepath.Join(dir, testFile), []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}

	errs := &PreCommitErrors{}
	w := &bytes.Buffer{}
	checkGoTestConventions(w, dir, []string{testFile}, errs)
	if len(errs.Errors) == 0 {
		t.Error("expected error for " + ctxCall)
	}
	found := false
	for _, e := range errs.Errors {
		if strings.Contains(e, ctxCall) {
			found = true
			break
		}
	}
	if !found {
		t.Errorf("expected %s error, got: %v", ctxCall, errs.Errors)
	}
}

func TestCheckGoTestConventions_ContextBackgroundNocheck(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	testFile := "example_test.go"
	// Build fixture via concatenation to avoid triggering the pre-commit
	// convention scanner on this test file itself.
	ctxCall := "context." + "Background()"
	nocheck := "// NOCHECK: context." + "Background"
	content := "package example\n\nimport (\n\t\"context\"\n\t\"testing\"\n)\n\n" +
		"func TestExample(t *testing.T) {\n\tt.Parallel()\n\tctx := " + ctxCall + " " + nocheck + "\n\t_ = ctx\n}\n"
	if err := os.WriteFile(filepath.Join(dir, testFile), []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}

	errs := &PreCommitErrors{}
	w := &bytes.Buffer{}
	checkGoTestConventions(w, dir, []string{testFile}, errs)
	if len(errs.Errors) > 0 {
		t.Errorf("expected no errors with NOCHECK escape, got: %v", errs.Errors)
	}
}

func TestCheckGoTestConventions_MissingParallel(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	testFile := "example_test.go"
	// Build fixture via concatenation to avoid triggering the pre-commit
	// convention scanner on this test file itself. The func signature
	// "func TestExample" without t.Parallel() in the next 5 lines would
	// otherwise be flagged when scanning this file.
	content := "package example\n\nimport \"testing\"\n\n" +
		"func Test" + "Example(t *testing.T) {\n\tif true {\n\t\tt.Log(\"no parallel\")\n\t}\n}\n"
	if err := os.WriteFile(filepath.Join(dir, testFile), []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}

	errs := &PreCommitErrors{}
	w := &bytes.Buffer{}
	checkGoTestConventions(w, dir, []string{testFile}, errs)
	if len(errs.Errors) == 0 {
		t.Error("expected error for missing t.Parallel()")
	}
}

func TestCheckGoTestConventions_ParallelEscapeHatch(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	testFile := "example_test.go"
	// Build fixture via concatenation to avoid triggering the pre-commit
	// convention scanner on this test file itself.
	escapeComment := "// NOTE: no t." + "Parallel"
	content := "package example\n\nimport \"testing\"\n\n" +
		"func Test" + "Example(t *testing.T) {\n\t" + escapeComment + "\n\tt.Log(\"sequential test\")\n}\n"
	if err := os.WriteFile(filepath.Join(dir, testFile), []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}

	errs := &PreCommitErrors{}
	w := &bytes.Buffer{}
	checkGoTestConventions(w, dir, []string{testFile}, errs)
	if len(errs.Errors) > 0 {
		t.Errorf("expected no errors with escape hatch, got: %v", errs.Errors)
	}
}

func TestCheckGoTestConventions_ReadError(t *testing.T) {
	t.Parallel()

	// File listed as staged but doesn't exist on disk.
	errs := &PreCommitErrors{}
	w := &bytes.Buffer{}
	checkGoTestConventions(w, t.TempDir(), []string{"nonexistent_test.go"}, errs)
	// ReadFile error should be silently skipped (continue).
	if len(errs.Errors) > 0 {
		t.Errorf("expected no errors for missing file, got: %v", errs.Errors)
	}
}

func TestCheckGoTestConventions_NoTestFiles(t *testing.T) {
	t.Parallel()

	errs := &PreCommitErrors{}
	w := &bytes.Buffer{}
	checkGoTestConventions(w, t.TempDir(), []string{"main.go", "util.go"}, errs)
	if len(errs.Errors) > 0 {
		t.Error("expected no errors for non-test files")
	}
}

func TestCheckGoTestConventions_ValidTests(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	testFile := "example_test.go"
	// Build fixture via concatenation to avoid triggering the pre-commit
	// convention scanner on this test file itself.
	content := "package example\n\nimport \"testing\"\n\n" +
		"func Test" + "Example(t *testing.T) {\n\tt.Parallel()\n\tt.Log(\"valid\")\n}\n"
	if err := os.WriteFile(filepath.Join(dir, testFile), []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}

	errs := &PreCommitErrors{}
	w := &bytes.Buffer{}
	checkGoTestConventions(w, dir, []string{testFile}, errs)
	if len(errs.Errors) > 0 {
		t.Errorf("expected no errors, got: %v", errs.Errors)
	}
	if !strings.Contains(w.String(), "Go test convention check passed") {
		t.Error("expected success message")
	}
}

func TestCheckGoTestFilePairing_PairedFile(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	// Create the source file and its test file.
	cliDir := filepath.Join(dir, "codeflow-cli", "internal", "example")
	if err := os.MkdirAll(cliDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(cliDir, "util.go"), []byte("package example"), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(cliDir, "util_test.go"), []byte("package example"), 0o644); err != nil {
		t.Fatal(err)
	}

	errs := &PreCommitErrors{}
	w := &bytes.Buffer{}
	checkGoTestFilePairing(w, dir, []string{"codeflow-cli/internal/example/util.go"}, errs)
	if len(errs.Errors) > 0 {
		t.Errorf("expected no errors for paired file, got: %v", errs.Errors)
	}
}

func TestCheckGoTestFilePairing_MissingTestFile(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	// Create only the source file.
	cliDir := filepath.Join(dir, "codeflow-cli", "internal", "example")
	if err := os.MkdirAll(cliDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(cliDir, "util.go"), []byte("package example"), 0o644); err != nil {
		t.Fatal(err)
	}

	errs := &PreCommitErrors{}
	w := &bytes.Buffer{}
	checkGoTestFilePairing(w, dir, []string{"codeflow-cli/internal/example/util.go"}, errs)
	if len(errs.Errors) == 0 {
		t.Error("expected error for missing test file")
	}
	if len(errs.Errors) > 0 && !strings.Contains(errs.Errors[0], "GO TEST FILE MISSING") {
		t.Errorf("expected 'GO TEST FILE MISSING' error, got: %s", errs.Errors[0])
	}
}

func TestCheckGoTestFilePairing_DocGoExcluded(t *testing.T) {
	t.Parallel()

	errs := &PreCommitErrors{}
	w := &bytes.Buffer{}
	checkGoTestFilePairing(w, t.TempDir(), []string{"codeflow-cli/internal/example/doc.go"}, errs)
	if len(errs.Errors) > 0 {
		t.Error("expected doc.go to be excluded")
	}
}

func TestCheckGoTestFilePairing_NonCliPrefix(t *testing.T) {
	t.Parallel()

	errs := &PreCommitErrors{}
	w := &bytes.Buffer{}
	// Files outside codeflow-cli/ are excluded.
	checkGoTestFilePairing(w, t.TempDir(), []string{"other/pkg/util.go"}, errs)
	if len(errs.Errors) > 0 {
		t.Error("expected non-CLI files to be excluded")
	}
}

func TestCheckGoTestFilePairing_TestFileExcluded(t *testing.T) {
	t.Parallel()

	errs := &PreCommitErrors{}
	w := &bytes.Buffer{}
	checkGoTestFilePairing(w, t.TempDir(), []string{"codeflow-cli/internal/example/util_test.go"}, errs)
	if len(errs.Errors) > 0 {
		t.Error("expected test files to be excluded from pairing check")
	}
}

func TestCheckGoTestFilePairing_ExceptionFile(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	// Create test config with an exception.
	cfgDir := filepath.Join(dir, "codeflow-cli", "config", "testing")
	if err := os.MkdirAll(cfgDir, 0o755); err != nil {
		t.Fatal(err)
	}
	cfg := map[string]interface{}{
		"conventions": map[string]interface{}{
			"exceptions": []map[string]string{
				{"file": "internal/example/special.go", "reason": "test exception"},
			},
		},
	}
	data, err := json.Marshal(cfg)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(cfgDir, "test-config.json"), data, 0o644); err != nil {
		t.Fatal(err)
	}

	errs := &PreCommitErrors{}
	w := &bytes.Buffer{}
	checkGoTestFilePairing(w, dir, []string{"codeflow-cli/internal/example/special.go"}, errs)
	if len(errs.Errors) > 0 {
		t.Errorf("expected exception to suppress error, got: %v", errs.Errors)
	}
}

func TestLoadGoTestExceptions(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	cfgDir := filepath.Join(dir, "codeflow-cli", "config", "testing")
	if err := os.MkdirAll(cfgDir, 0o755); err != nil {
		t.Fatal(err)
	}

	cfg := map[string]interface{}{
		"conventions": map[string]interface{}{
			"exceptions": []map[string]string{
				{"file": "internal/a.go"},
				{"file": "internal/b.go"},
			},
		},
	}
	data, err := json.Marshal(cfg)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(cfgDir, "test-config.json"), data, 0o644); err != nil {
		t.Fatal(err)
	}

	exceptions := loadGoTestExceptions(dir)
	if len(exceptions) != 2 {
		t.Errorf("expected 2 exceptions, got %d", len(exceptions))
	}
}

func TestLoadGoTestExceptions_InvalidJSON(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	cfgDir := filepath.Join(dir, "codeflow-cli", "config", "testing")
	if err := os.MkdirAll(cfgDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(cfgDir, "test-config.json"), []byte("{invalid"), 0o644); err != nil {
		t.Fatal(err)
	}

	exceptions := loadGoTestExceptions(dir)
	if exceptions != nil {
		t.Errorf("expected nil for invalid JSON, got: %v", exceptions)
	}
}

func TestLoadGoTestExceptions_NoFile(t *testing.T) {
	t.Parallel()

	exceptions := loadGoTestExceptions(t.TempDir())
	if exceptions != nil {
		t.Errorf("expected nil for nonexistent config, got: %v", exceptions)
	}
}

func TestIsException(t *testing.T) {
	t.Parallel()

	exceptions := []string{"internal/a.go", "internal/b.go"}
	tests := []struct {
		name    string
		relPath string
		want    bool
	}{
		{"match a", "internal/a.go", true},
		{"match b", "internal/b.go", true},
		{"no match", "internal/c.go", false},
		{"partial match", "internal/a.go/sub", true}, // Contains match.
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			if got := isException(tt.relPath, exceptions); got != tt.want {
				t.Errorf("isException(%q) = %v, want %v", tt.relPath, got, tt.want)
			}
		})
	}
}

func TestPreCommitErrors_Error(t *testing.T) {
	t.Parallel()

	errs := &PreCommitErrors{}
	errs.add("error 1")
	errs.add("error 2")
	msg := errs.Error()
	if !strings.Contains(msg, "2 error(s)") {
		t.Errorf("expected '2 error(s)' in message, got: %s", msg)
	}
}
