package githooks

import (
	"encoding/json"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"time"
)

// CommitLogEntry is a single commit event written to the JSONL log.
type CommitLogEntry struct {
	Timestamp string `json:"ts"`
	Event     string `json:"event"`
	Hash      string `json:"hash"`
	ShortHash string `json:"short_hash"`
	Message   string `json:"message"`
	Author    string `json:"author"`
	Branch    string `json:"branch"`
}

// runCommand executes an external command. Overridden in tests to avoid
// real subprocess calls.
var runCommand = func(name string, args ...string) *exec.Cmd {
	return exec.Command(name, args...)
}

// RunPostCommit collects commit metadata, writes a JSON log entry to
// .state/logs/git/commits-{date}.jsonl, prints the user-visible
// confirmation message to w, and auto-rebuilds the Go CLI binary when
// Go source files changed in the commit.
func RunPostCommit(w io.Writer, projectDir string) error {
	hash, err := gitOutput("rev-parse", "HEAD")
	if err != nil {
		return fmt.Errorf("get commit hash: %w", err)
	}

	shortHash, err := gitOutput("rev-parse", "--short", "HEAD")
	if err != nil {
		return fmt.Errorf("get short hash: %w", err)
	}

	message, err := gitOutput("log", "-1", "--pretty=%s")
	if err != nil {
		return fmt.Errorf("get commit message: %w", err)
	}

	author, err := gitOutput("log", "-1", "--pretty=%an")
	if err != nil {
		return fmt.Errorf("get commit author: %w", err)
	}

	branch, err := gitOutput("branch", "--show-current")
	if err != nil {
		// Detached HEAD — not fatal.
		branch = ""
	}

	// Write JSON log entry.
	logDir := filepath.Join(projectDir, ".state", "logs", "git")
	if mkErr := os.MkdirAll(logDir, 0o755); mkErr != nil {
		// Non-fatal — logging should not block commits.
		fmt.Fprintf(os.Stderr, "warning: cannot create log directory: %v\n", mkErr)
	} else {
		entry := CommitLogEntry{
			Timestamp: time.Now().UTC().Format("2006-01-02T15:04:05.000Z"),
			Event:     "commit",
			Hash:      hash,
			ShortHash: shortHash,
			Message:   message,
			Author:    author,
			Branch:    branch,
		}

		logFile := filepath.Join(logDir, fmt.Sprintf("commits-%s.jsonl", time.Now().Format("2006-01-02")))
		if writeErr := appendJSONL(logFile, entry); writeErr != nil {
			fmt.Fprintf(os.Stderr, "warning: cannot write commit log: %v\n", writeErr)
		}
	}

	// User-visible output — must match the original shell hook format exactly.
	fmt.Fprintln(w)
	fmt.Fprintf(w, "\033[0;32m✓ Commit created: %s\033[0m\n", shortHash)
	fmt.Fprintf(w, "  Branch: %s\n", branch)
	fmt.Fprintf(w, "  Message: %s\n", message)
	fmt.Fprintln(w)

	// Auto-rebuild: if Go CLI source files changed, rebuild the binary so
	// the local `codeflow` command always reflects the latest committed code.
	// Non-fatal — rebuild failures are warnings, never block the commit.
	autoRebuildCLI(projectDir)

	return nil
}

// goFilesChanged reports whether the latest commit touched Go source files
// in the codeflow-cli directory (*.go, go.mod, go.sum).
func goFilesChanged() bool {
	cmd := runCommand("git", "diff", "--name-only", "HEAD~1", "HEAD", "--",
		"codeflow-cli/*.go",
		"codeflow-cli/**/*.go",
		"codeflow-cli/go.mod",
		"codeflow-cli/go.sum",
	)
	out, err := cmd.Output()
	if err != nil {
		return false
	}
	return len(strings.TrimSpace(string(out))) > 0
}

// autoRebuildCLI runs `go install ./cmd/codeflow/` from the codeflow-cli
// subdirectory when Go source files changed. Errors are printed as warnings
// to stderr and never propagated — post-commit must remain non-blocking.
func autoRebuildCLI(projectDir string) {
	// Only rebuild if this repo contains the CodeFlow CLI source.
	modFile := filepath.Join(projectDir, "codeflow-cli", "go.mod")
	if _, err := os.Stat(modFile); os.IsNotExist(err) {
		return
	}

	if !goFilesChanged() {
		return
	}

	fmt.Fprintln(os.Stderr, "[auto-rebuild] Go CLI source changed, rebuilding...")

	cliDir := filepath.Join(projectDir, "codeflow-cli")
	cmd := runCommand("go", "install", "./cmd/codeflow/")
	cmd.Dir = cliDir
	cmd.Stdout = os.Stderr
	cmd.Stderr = os.Stderr

	if err := cmd.Run(); err != nil {
		fmt.Fprintf(os.Stderr, "[auto-rebuild] warning: rebuild failed: %v\n", err)
		return
	}

	fmt.Fprintln(os.Stderr, "[auto-rebuild] rebuild complete.")
}

// appendJSONL marshals v as JSON and appends a newline-terminated line to path.
func appendJSONL(path string, v any) error {
	data, err := json.Marshal(v)
	if err != nil {
		return fmt.Errorf("marshal log entry: %w", err)
	}
	data = append(data, '\n')

	f, err := os.OpenFile(path, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0o644)
	if err != nil {
		return fmt.Errorf("open log file: %w", err)
	}
	defer f.Close()

	if _, err := f.Write(data); err != nil {
		return fmt.Errorf("write log entry: %w", err)
	}
	return nil
}

// gitOutput runs a git command and returns trimmed stdout.
func gitOutput(args ...string) (string, error) {
	cmd := exec.Command("git", args...)
	out, err := cmd.Output()
	if err != nil {
		return "", fmt.Errorf("git %s: %w", strings.Join(args, " "), err)
	}
	return strings.TrimSpace(string(out)), nil
}
