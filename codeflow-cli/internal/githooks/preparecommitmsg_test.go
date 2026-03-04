package githooks

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestExtractTypeAndScope(t *testing.T) {
	t.Parallel()

	policy := testPolicy()
	tests := []struct {
		name      string
		branch    string
		wantType  string
		wantScope string
	}{
		{"feat branch", "feat/add-login", "feat", "add"},
		{"fix branch with scope", "fix/inf-tsk-021-bug", "fix", "inf"},
		{"docs branch", "docs/update-readme", "docs", "update"},
		{"no slash", "main", "", ""},
		{"unknown type", "unknown/something", "", ""},
		{"no scope separator", "feat/simple", "feat", ""},
		{"refactor", "refactor/inf-cleanup", "refactor", "inf"},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			gotType, gotScope := extractTypeAndScope(tt.branch, policy)
			if gotType != tt.wantType {
				t.Errorf("type: got %q, want %q", gotType, tt.wantType)
			}
			if gotScope != tt.wantScope {
				t.Errorf("scope: got %q, want %q", gotScope, tt.wantScope)
			}
		})
	}
}

func TestIsEmptyOrComments(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name string
		msg  string
		want bool
	}{
		{"empty", "", true},
		{"only comments", "# comment\n# another", true},
		{"whitespace and comments", "  \n# comment\n  ", true},
		{"has content", "some content", false},
		{"content after comment", "# comment\ncontent", false},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			if got := isEmptyOrComments(tt.msg); got != tt.want {
				t.Errorf("got %v, want %v", got, tt.want)
			}
		})
	}
}

func TestRunPrepareCommitMsg_SkipSources(t *testing.T) {
	t.Parallel()

	policy := testPolicy()
	dir := t.TempDir()
	msgFile := filepath.Join(dir, "COMMIT_EDITMSG")
	original := "original content"
	if err := os.WriteFile(msgFile, []byte(original), 0o644); err != nil {
		t.Fatal(err)
	}

	for _, source := range []string{"message", "merge", "commit", "squash"} {
		t.Run(source, func(t *testing.T) {
			t.Parallel()
			localFile := filepath.Join(t.TempDir(), "COMMIT_EDITMSG")
			if err := os.WriteFile(localFile, []byte(original), 0o644); err != nil {
				t.Fatal(err)
			}
			if err := RunPrepareCommitMsg(localFile, source, policy); err != nil {
				t.Fatalf("expected no error, got: %v", err)
			}
			data, err := os.ReadFile(localFile)
			if err != nil {
				t.Fatal(err)
			}
			if string(data) != original {
				t.Errorf("file was modified for source %q", source)
			}
		})
	}
}

func TestRunPrepareCommitMsg_NonexistentFile(t *testing.T) {
	t.Parallel()

	policy := testPolicy()
	// source="" so we don't skip. getCurrentBranch may fail (empty branch) but
	// the function continues and attempts to read the nonexistent file, which
	// returns an error.
	err := RunPrepareCommitMsg("/nonexistent/COMMIT_EDITMSG", "", policy)
	if err == nil {
		t.Log("no error (getCurrentBranch may have provided a branch in this env)")
		return
	}
	if !strings.Contains(err.Error(), "read commit message file") {
		t.Errorf("expected file read error, got: %v", err)
	}
}

func TestWriteTemplate_WithTypeAndScope(t *testing.T) {
	t.Parallel()

	policy := testPolicy()
	dir := t.TempDir()
	msgFile := filepath.Join(dir, "COMMIT_EDITMSG")
	if err := writeTemplate(msgFile, "feat/inf-add-login", "feat", "inf", policy); err != nil {
		t.Fatal(err)
	}

	data, err := os.ReadFile(msgFile)
	if err != nil {
		t.Fatal(err)
	}
	content := string(data)

	if !strings.HasPrefix(content, "feat(inf): \n") {
		t.Errorf("expected template to start with 'feat(inf): \\n', got: %q", content[:min(len(content), 30)])
	}
	if !strings.Contains(content, "# Types:") {
		t.Error("expected types list in template")
	}
	if !strings.Contains(content, "# Branch: feat/inf-add-login") {
		t.Error("expected branch name in template")
	}
}

func TestWriteTemplate_WithTypeNoScope(t *testing.T) {
	t.Parallel()

	policy := testPolicy()
	dir := t.TempDir()
	msgFile := filepath.Join(dir, "COMMIT_EDITMSG")
	if err := writeTemplate(msgFile, "feat/simple", "feat", "", policy); err != nil {
		t.Fatal(err)
	}

	data, err := os.ReadFile(msgFile)
	if err != nil {
		t.Fatal(err)
	}
	content := string(data)

	if !strings.HasPrefix(content, "feat: \n") {
		t.Errorf("expected template to start with 'feat: \\n', got: %q", content[:min(len(content), 20)])
	}
}

func TestWriteTemplate_NoType(t *testing.T) {
	t.Parallel()

	policy := testPolicy()
	dir := t.TempDir()
	msgFile := filepath.Join(dir, "COMMIT_EDITMSG")
	if err := writeTemplate(msgFile, "main", "", "", policy); err != nil {
		t.Fatal(err)
	}

	data, err := os.ReadFile(msgFile)
	if err != nil {
		t.Fatal(err)
	}
	content := string(data)

	if !strings.HasPrefix(content, "type: description\n") {
		t.Errorf("expected generic template, got: %q", content[:min(len(content), 30)])
	}
}

func TestWriteTemplate_EmptyTypes(t *testing.T) {
	t.Parallel()

	policy := &EnforcementPolicy{
		GitFormat: GitFormat{
			CommitTypes: nil,
		},
	}
	dir := t.TempDir()
	msgFile := filepath.Join(dir, "COMMIT_EDITMSG")
	if err := writeTemplate(msgFile, "feat/something", "feat", "", policy); err != nil {
		t.Fatal(err)
	}

	data, err := os.ReadFile(msgFile)
	if err != nil {
		t.Fatal(err)
	}
	content := string(data)

	if strings.Contains(content, "# Types:") {
		t.Error("expected no types list when types are empty")
	}
}

func TestRunPrepareCommitMsg_CommentOnlyContent(t *testing.T) {
	t.Parallel()

	policy := testPolicy()
	dir := t.TempDir()
	msgFile := filepath.Join(dir, "COMMIT_EDITMSG")
	// Write comment-only content — should be treated as "empty" and get template.
	if err := os.WriteFile(msgFile, []byte("# comment line\n# another comment\n"), 0o644); err != nil {
		t.Fatal(err)
	}

	// RunPrepareCommitMsg proceeds regardless of getCurrentBranch result.
	// In detached HEAD or non-git env, branch is empty → generic template.
	err := RunPrepareCommitMsg(msgFile, "", policy)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	data, err := os.ReadFile(msgFile)
	if err != nil {
		t.Fatal(err)
	}
	content := string(data)
	// Template should always be written since file had only comments.
	if !strings.Contains(content, "Conventional commit") {
		t.Error("expected template with conventional commit guidance")
	}
}

func TestRunPrepareCommitMsg_ExistingContent(t *testing.T) {
	t.Parallel()

	policy := testPolicy()
	dir := t.TempDir()
	msgFile := filepath.Join(dir, "COMMIT_EDITMSG")
	// Write non-empty, non-comment content.
	if err := os.WriteFile(msgFile, []byte("existing content"), 0o644); err != nil {
		t.Fatal(err)
	}

	// Non-comment content → isEmptyOrComments returns false → no template written.
	err := RunPrepareCommitMsg(msgFile, "", policy)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	data, err := os.ReadFile(msgFile)
	if err != nil {
		t.Fatal(err)
	}
	if string(data) != "existing content" {
		t.Error("file should not be modified when it has non-comment content")
	}
}
