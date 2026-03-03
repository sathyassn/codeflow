package shadowtest

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/json"
	"fmt"
	"os/exec"
	"strings"
	"time"
)

// ExecResult captures the output of a single command execution.
type ExecResult struct {
	// ExitCode is the command exit code (0 = success, 2 = blocked, etc.).
	ExitCode int

	// Stdout is the captured standard output.
	Stdout string

	// Stderr is the captured standard error.
	Stderr string

	// Duration is how long the command ran.
	Duration time.Duration
}

// DivergenceKind categorizes the type of divergence found.
type DivergenceKind string

const (
	// DivergenceExitCode means the Go and shell exit codes differ.
	DivergenceExitCode DivergenceKind = "exit_code"

	// DivergenceStdout means the normalized stdout outputs differ.
	DivergenceStdout DivergenceKind = "stdout"

	// DivergenceStderr means the filtered stderr outputs differ.
	DivergenceStderr DivergenceKind = "stderr"
)

// Divergence captures a single output mismatch between Go and shell.
type Divergence struct {
	// Kind is the type of divergence (exit_code, stdout, stderr).
	Kind DivergenceKind

	// GoValue is the value from the Go implementation.
	GoValue string

	// ShellValue is the value from the shell implementation.
	ShellValue string

	// Diff is a human-readable description of the difference.
	Diff string
}

// DivergenceLog is a structured record of a shadow test run with divergences.
type DivergenceLog struct {
	// Timestamp is when the shadow test ran.
	Timestamp time.Time

	// Name is the shadow test name.
	Name string

	// StdinHash is the SHA-256 hash of the stdin payload.
	StdinHash string

	// GoResult captures the Go command output.
	GoResult *ExecResult

	// ShellResult captures the shell command output.
	ShellResult *ExecResult

	// Divergences lists all found divergences.
	Divergences []Divergence
}

// NormalizationRules defines transformations applied to both Go and shell
// outputs before comparison. This allows known schema evolution differences
// to be excluded from divergence reporting.
type NormalizationRules struct {
	// FieldRenames maps old field names to new field names in JSONL events.
	// Applied to both Go and shell outputs (shell output is renamed to match Go).
	// Example: {"type": "event", "ts": "timestamp"}
	FieldRenames map[string]string

	// OmitFields lists JSONL fields to strip from shell output before comparison.
	// Example: ["id", "event_id"]
	OmitFields []string

	// EventTypeMapping maps shell event type values to their Go equivalents.
	// Example: {"session_metadata": "session_register"}
	EventTypeMapping map[string]string

	// OmitStdoutFields lists fields to strip from shell stdout JSON before
	// comparison. These are fields that shell includes but Go omits.
	// Example: ["event_id", "verdict"]
	OmitStdoutFields []string

	// FlattenKeyValue indicates that shell JSONL events use a key/value
	// envelope ({key: "k", value: "v"}) that should be flattened to top-level
	// fields ({k: v}) to match Go's output format.
	FlattenKeyValue bool

	// StderrNoisePatterns lists substring prefixes to strip from stderr before
	// comparison. Lines containing any of these strings are dropped. Used to
	// filter shell verbosity (e.g. stale session warnings) that Go does not emit.
	// Example: ["WARNING:", "STALE SESSIONS"]
	StderrNoisePatterns []string

	// StripStdoutLinePrefixes lists line prefixes to strip from stdout before
	// comparison. Lines starting with any of these prefixes are dropped. Used
	// to filter shell-only status messages (e.g. "SessionEnd:", "[INFO] ").
	StripStdoutLinePrefixes []string

	// OmitStderrFields lists JSON fields to strip from structured stderr
	// lines before comparison. Used when Go and shell emit the same error
	// structure but with different message wording (e.g., "error" field).
	OmitStderrFields []string

	// OmitStdout skips stdout comparison entirely when true. Use for tests
	// where stdout content differs structurally between Go and shell but the
	// exit code is the meaningful signal (e.g., validation commands).
	OmitStdout bool
}

// DefaultNormalizationRules returns the standard rules for comparing Go and
// shell pathflow/hook outputs, handling all known schema evolution differences.
func DefaultNormalizationRules() NormalizationRules {
	return NormalizationRules{
		FieldRenames: map[string]string{
			"type": "event",
			"ts":   "timestamp",
		},
		OmitFields:       []string{"id"},
		OmitStdoutFields: []string{"event_id", "verdict"},
		EventTypeMapping: map[string]string{
			"session_metadata": "session_register",
		},
	}
}

// SessionRegisterNormalizationRules returns rules for session-register commands,
// which have an additional key/value→flat-fields normalization requirement.
func SessionRegisterNormalizationRules() NormalizationRules {
	rules := DefaultNormalizationRules()
	rules.FlattenKeyValue = true
	return rules
}

// SessionStartNormalizationRules returns rules for session-start hooks, which
// may emit stale-session warnings to stderr that Go does not emit.
func SessionStartNormalizationRules() NormalizationRules {
	rules := DefaultNormalizationRules()
	rules.StderrNoisePatterns = []string{"WARNING:", "STALE SESSIONS", "stale session"}
	return rules
}

// SessionEndNormalizationRules returns rules for session-end hooks, which emit
// "SessionEnd:" status messages to stdout that Go does not emit.
func SessionEndNormalizationRules() NormalizationRules {
	rules := DefaultNormalizationRules()
	rules.StripStdoutLinePrefixes = []string{"SessionEnd:"}
	return rules
}

// ValidationNormalizationRules returns rules for validate-task and validate-epic
// commands, which emit "[INFO] " prefixed lines to stdout that Go does not emit,
// and Go emits a "Validation PASSED" / "Validation FAILED" summary line that
// shell does not emit. Both sides are stripped so the normalized stdout is empty
// and the exit code becomes the sole comparison signal.
func ValidationNormalizationRules() NormalizationRules {
	rules := DefaultNormalizationRules()
	rules.StripStdoutLinePrefixes = []string{"[INFO] ", "[INFO]", "Validation "}
	return rules
}

// PhaseTransitionErrorNormalizationRules returns rules for phase-transition
// commands when the expected output is an error. Both Go and shell output JSON
// to stderr on invalid phase, but with different wording in the "error" field.
// OmitStderrFields strips the "error" field value so only structure is compared.
func PhaseTransitionErrorNormalizationRules() NormalizationRules {
	rules := DefaultNormalizationRules()
	rules.OmitStderrFields = []string{"error"}
	return rules
}

// SessionRegisterErrorNormalizationRules returns rules for session-register
// commands when the expected output is an error. Both Go and shell output JSON
// to stderr on invalid mode, but with different wording in the "error" field.
// OmitStderrFields strips the "error" field value so only structure is compared.
func SessionRegisterErrorNormalizationRules() NormalizationRules {
	rules := DefaultNormalizationRules()
	rules.OmitStderrFields = []string{"error"}
	return rules
}

// TaskUpdateNormalizationRules returns rules for task-update commands, which
// have a "status"→"task_status" rename in the JSONL event.
func TaskUpdateNormalizationRules() NormalizationRules {
	rules := DefaultNormalizationRules()
	rules.FieldRenames["status"] = "task_status"
	return rules
}

// NormalizeJSONLEvent applies normalization rules to a single parsed JSONL
// event map. It renames fields, strips omitted fields, applies event type
// mappings, and optionally flattens key/value envelopes. The input map is
// modified in place and returned.
func NormalizeJSONLEvent(event map[string]interface{}, rules NormalizationRules) map[string]interface{} {
	if event == nil {
		return event
	}

	// Strip omitted fields.
	for _, field := range rules.OmitFields {
		delete(event, field)
	}

	// Apply field renames.
	for oldName, newName := range rules.FieldRenames {
		if v, ok := event[oldName]; ok {
			delete(event, oldName)
			event[newName] = v
		}
	}

	// Apply event type mapping.
	if mapping := rules.EventTypeMapping; len(mapping) > 0 {
		if evType, ok := event["event"].(string); ok {
			if mapped, ok := mapping[evType]; ok {
				event["event"] = mapped
			}
		}
	}

	// Flatten key/value envelopes: {"key":"tracking_level","value":"pending"}
	// becomes {"tracking_level":"pending"}.
	if rules.FlattenKeyValue {
		if k, ok := event["key"].(string); ok {
			if v, vOK := event["value"]; vOK && k != "" {
				delete(event, "key")
				delete(event, "value")
				event[k] = v
			}
		}
	}

	return event
}

// NormalizeJSONLOutput parses multi-line JSONL text, applies normalization
// rules to each line, and returns canonical JSON lines sorted for deterministic
// comparison. Non-JSON lines are silently skipped.
func NormalizeJSONLOutput(raw string, rules NormalizationRules) string {
	var normalized []string
	for _, line := range strings.Split(strings.TrimSpace(raw), "\n") {
		line = strings.TrimSpace(line)
		if line == "" {
			continue
		}
		var event map[string]interface{}
		if err := json.Unmarshal([]byte(line), &event); err != nil {
			// Not JSON — skip.
			continue
		}
		event = NormalizeJSONLEvent(event, rules)
		if data, err := json.Marshal(event); err == nil {
			normalized = append(normalized, string(data))
		}
	}
	return strings.Join(normalized, "\n")
}

// NormalizeStdout strips shell-only fields from stdout and filters lines with
// known shell-only prefixes. If the content is JSON, shell-only fields are
// removed. Lines starting with any StripStdoutLinePrefixes are dropped.
// If OmitStdout is true, returns "" unconditionally (stdout comparison skipped).
func NormalizeStdout(raw string, rules NormalizationRules) string {
	if rules.OmitStdout {
		return ""
	}

	raw = strings.TrimSpace(raw)
	if raw == "" {
		return ""
	}

	// Filter lines with shell-only prefixes (e.g. "SessionEnd:", "[INFO] ").
	if len(rules.StripStdoutLinePrefixes) > 0 {
		var kept []string
		for _, line := range strings.Split(raw, "\n") {
			stripped := false
			for _, prefix := range rules.StripStdoutLinePrefixes {
				if strings.HasPrefix(strings.TrimSpace(line), prefix) {
					stripped = true
					break
				}
			}
			if !stripped {
				kept = append(kept, line)
			}
		}
		raw = strings.TrimSpace(strings.Join(kept, "\n"))
	}

	if raw == "" {
		return ""
	}

	var obj map[string]interface{}
	if err := json.Unmarshal([]byte(raw), &obj); err != nil {
		// Not JSON — return filtered text as-is.
		return raw
	}
	for _, field := range rules.OmitStdoutFields {
		delete(obj, field)
	}
	if data, err := json.Marshal(obj); err == nil {
		return string(data)
	}
	return raw
}

// FilterStderr removes debug/logging lines from stderr that differ between
// implementations due to shell library sourcing and verbose output. It keeps
// only structured error lines (JSON) and lines starting with known error
// prefixes (BLOCKED, ERROR, WARNING). Per-test StderrNoisePatterns are also
// stripped. OmitStderrFields strips named fields from JSON lines before
// comparison, handling wording differences in error messages.
func FilterStderr(raw string, rules NormalizationRules) string {
	var kept []string
	for _, line := range strings.Split(raw, "\n") {
		line = strings.TrimSpace(line)
		if line == "" {
			continue
		}
		// Drop lines matching per-test noise patterns.
		noisy := false
		for _, pattern := range rules.StderrNoisePatterns {
			if strings.Contains(line, pattern) {
				noisy = true
				break
			}
		}
		if noisy {
			continue
		}
		// Keep structured error output.
		if strings.HasPrefix(line, "{") || strings.HasPrefix(line, "BLOCKED") ||
			strings.HasPrefix(line, "ERROR") || strings.HasPrefix(line, "WARNING") {
			// Apply OmitStderrFields to JSON lines.
			if strings.HasPrefix(line, "{") && len(rules.OmitStderrFields) > 0 {
				var obj map[string]interface{}
				if err := json.Unmarshal([]byte(line), &obj); err == nil {
					for _, field := range rules.OmitStderrFields {
						delete(obj, field)
					}
					if data, err := json.Marshal(obj); err == nil {
						line = string(data)
					}
				}
			}
			kept = append(kept, line)
		}
		// Discard shell library debug, source tracing, timing lines, etc.
	}
	return strings.Join(kept, "\n")
}

// ShadowTest defines a single comparison between a Go subcommand and its shell
// script equivalent.
type ShadowTest struct {
	// Name is a human-readable identifier for this shadow test.
	Name string

	// GoCommand is the Go CLI command arguments (without the binary name).
	// Example: ["hooks", "pre-tool-use", "security"]
	GoCommand []string

	// ShellCommand is the shell script path with arguments.
	// Example: ".codeflow/scripts/pathflow/cf-pathflow-phase-transition.sh"
	ShellCommand []string

	// Stdin is the payload sent to both commands on standard input.
	Stdin []byte

	// Env holds environment variables set for both commands. These are
	// merged with the test process environment.
	Env map[string]string

	// Rules are the normalization rules applied before output comparison.
	// Defaults to DefaultNormalizationRules() if nil.
	Rules *NormalizationRules

	// ExpectedExitCode is the exit code both implementations should return.
	// 0 = success, 2 = blocked/error.
	ExpectedExitCode int

	// KnownDivergence describes a documented behavioral difference that
	// should NOT cause a test failure. Empty means no known divergences.
	KnownDivergence string
}

// ShadowResult captures the complete output of a shadow test run.
type ShadowResult struct {
	// Test is the shadow test that was run.
	Test ShadowTest

	// GoResult is the output from the Go implementation.
	GoResult *ExecResult

	// ShellResult is the output from the shell implementation.
	ShellResult *ExecResult

	// Divergences lists all found divergences after normalization.
	// Empty means the implementations are functionally equivalent.
	Divergences []Divergence

	// Log is the full divergence log entry for reporting.
	Log DivergenceLog
}

// HasUnexpectedDivergences returns true if there are divergences that are not
// accounted for by KnownDivergence.
func (r *ShadowResult) HasUnexpectedDivergences() bool {
	if len(r.Divergences) == 0 {
		return false
	}
	if r.Test.KnownDivergence != "" {
		// If a known divergence is documented, we expect some divergences.
		// This is a coarse check — in production, divergences would be
		// validated against the specific known difference.
		return false
	}
	return true
}

// Run executes both the Go and shell implementations with identical stdin and
// environment, compares their outputs, and returns a ShadowResult.
func (st *ShadowTest) Run(ctx context.Context, goBinary, projectDir string) (*ShadowResult, error) {
	rules := DefaultNormalizationRules()
	if st.Rules != nil {
		rules = *st.Rules
	}

	// Build environment.
	env := buildEnv(st.Env, projectDir)

	// Execute Go implementation.
	goStart := time.Now()
	goResult, err := runCommand(ctx, goBinary, st.GoCommand, st.Stdin, env, projectDir)
	if err != nil {
		return nil, fmt.Errorf("shadowtest: running Go command %v: %w", st.GoCommand, err)
	}
	goResult.Duration = time.Since(goStart)

	// Execute shell implementation.
	shellStart := time.Now()
	shellResult, err := runCommand(ctx, "", st.ShellCommand, st.Stdin, env, projectDir)
	if err != nil {
		return nil, fmt.Errorf("shadowtest: running shell command %v: %w", st.ShellCommand, err)
	}
	shellResult.Duration = time.Since(shellStart)

	// Compare outputs.
	divergences := CompareOutputs(goResult, shellResult, rules)

	// Build divergence log.
	h := sha256.Sum256(st.Stdin)
	log := DivergenceLog{
		Timestamp:   time.Now().UTC(),
		Name:        st.Name,
		StdinHash:   fmt.Sprintf("%x", h[:8]),
		GoResult:    goResult,
		ShellResult: shellResult,
		Divergences: divergences,
	}

	return &ShadowResult{
		Test:        *st,
		GoResult:    goResult,
		ShellResult: shellResult,
		Divergences: divergences,
		Log:         log,
	}, nil
}

// CompareOutputs compares Go and shell ExecResults, applying normalization
// rules to JSONL and stdout before comparison. It returns a slice of
// Divergence entries for any detected mismatches.
func CompareOutputs(goResult, shellResult *ExecResult, rules NormalizationRules) []Divergence {
	var divergences []Divergence

	// Compare exit codes.
	if goResult.ExitCode != shellResult.ExitCode {
		divergences = append(divergences, Divergence{
			Kind:       DivergenceExitCode,
			GoValue:    fmt.Sprintf("%d", goResult.ExitCode),
			ShellValue: fmt.Sprintf("%d", shellResult.ExitCode),
			Diff:       fmt.Sprintf("exit code: Go=%d, shell=%d", goResult.ExitCode, shellResult.ExitCode),
		})
	}

	// Compare normalized stdout.
	goStdout := NormalizeStdout(goResult.Stdout, rules)
	shellStdout := NormalizeStdout(shellResult.Stdout, rules)
	if goStdout != shellStdout {
		divergences = append(divergences, Divergence{
			Kind:       DivergenceStdout,
			GoValue:    goStdout,
			ShellValue: shellStdout,
			Diff:       diffStrings(goStdout, shellStdout),
		})
	}

	// Compare filtered stderr (lenient — only structured error output).
	goStderr := FilterStderr(goResult.Stderr, rules)
	shellStderr := FilterStderr(shellResult.Stderr, rules)
	if goStderr != shellStderr {
		divergences = append(divergences, Divergence{
			Kind:       DivergenceStderr,
			GoValue:    goStderr,
			ShellValue: shellStderr,
			Diff:       diffStrings(goStderr, shellStderr),
		})
	}

	return divergences
}

// CompareJSONLOutputs compares normalized JSONL outputs from Go and shell,
// applying the given rules before comparison. It returns a Divergence if
// the normalized outputs differ.
func CompareJSONLOutputs(goOutput, shellOutput string, rules NormalizationRules) *Divergence {
	goNorm := NormalizeJSONLOutput(goOutput, rules)
	shellNorm := NormalizeJSONLOutput(shellOutput, rules)
	if goNorm == shellNorm {
		return nil
	}
	return &Divergence{
		Kind:       DivergenceStdout,
		GoValue:    goNorm,
		ShellValue: shellNorm,
		Diff:       diffStrings(goNorm, shellNorm),
	}
}

// runCommand executes a command with the given arguments, stdin, and
// environment. If binary is empty, the command is run directly via the shell.
func runCommand(ctx context.Context, binary string, args []string, stdin []byte, env []string, workDir string) (*ExecResult, error) {
	var cmd *exec.Cmd
	if binary == "" {
		// Shell command: args[0] is the script path, args[1:] are arguments.
		if len(args) == 0 {
			return nil, fmt.Errorf("shadowtest: empty shell command")
		}
		cmd = exec.CommandContext(ctx, args[0], args[1:]...)
	} else {
		cmd = exec.CommandContext(ctx, binary, args...)
	}

	cmd.Dir = workDir
	cmd.Env = env

	var stdoutBuf, stderrBuf bytes.Buffer
	cmd.Stdin = bytes.NewReader(stdin)
	cmd.Stdout = &stdoutBuf
	cmd.Stderr = &stderrBuf

	err := cmd.Run()
	exitCode := 0
	if err != nil {
		if exitErr, ok := err.(*exec.ExitError); ok {
			exitCode = exitErr.ExitCode()
			err = nil // not a fatal error — we capture the exit code
		} else {
			return nil, err
		}
	}

	return &ExecResult{
		ExitCode: exitCode,
		Stdout:   stdoutBuf.String(),
		Stderr:   stderrBuf.String(),
	}, nil
}

// buildEnv constructs the environment for both commands. It includes the
// current process environment, project-specific variables, and test-specific
// overrides from st.Env.
func buildEnv(testEnv map[string]string, projectDir string) []string {
	// Start with minimal environment to ensure reproducibility.
	base := map[string]string{
		"PATH":            "/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin",
		"HOME":            "/tmp",
		"CF_PROJECT_ROOT": projectDir,
	}

	// Apply test-specific overrides.
	for k, v := range testEnv {
		base[k] = v
	}

	var env []string
	for k, v := range base {
		env = append(env, k+"="+v)
	}
	return env
}

// diffStrings produces a compact human-readable diff of two strings.
func diffStrings(a, b string) string {
	if a == b {
		return ""
	}
	aLines := strings.Split(a, "\n")
	bLines := strings.Split(b, "\n")
	var diff []string
	diff = append(diff, fmt.Sprintf("Go (%d lines):", len(aLines)))
	for _, l := range aLines {
		diff = append(diff, "  + "+l)
	}
	diff = append(diff, fmt.Sprintf("Shell (%d lines):", len(bLines)))
	for _, l := range bLines {
		diff = append(diff, "  - "+l)
	}
	return strings.Join(diff, "\n")
}

// FormatDivergenceReport produces a human-readable report of shadow test
// results for logging and debugging.
func FormatDivergenceReport(results []*ShadowResult) string {
	var sb strings.Builder
	totalTests := len(results)
	var failedTests int

	for _, r := range results {
		if r.HasUnexpectedDivergences() {
			failedTests++
		}
	}

	fmt.Fprintf(&sb, "Shadow Test Report: %d/%d passed\n", totalTests-failedTests, totalTests)
	fmt.Fprintln(&sb, strings.Repeat("=", 60))

	for _, r := range results {
		status := "PASS"
		if r.HasUnexpectedDivergences() {
			status = "FAIL"
		} else if r.Test.KnownDivergence != "" && len(r.Divergences) > 0 {
			status = "EXPECTED"
		}

		fmt.Fprintf(&sb, "[%s] %s\n", status, r.Test.Name)

		if status != "PASS" {
			fmt.Fprintf(&sb, "  Go:    exit=%d stdout=%q\n", r.GoResult.ExitCode, truncate(r.GoResult.Stdout, 100))
			fmt.Fprintf(&sb, "  Shell: exit=%d stdout=%q\n", r.ShellResult.ExitCode, truncate(r.ShellResult.Stdout, 100))
			for _, d := range r.Divergences {
				fmt.Fprintf(&sb, "  Divergence[%s]: %s\n", d.Kind, d.Diff)
			}
		}

		if r.Test.KnownDivergence != "" {
			fmt.Fprintf(&sb, "  KnownDivergence: %s\n", r.Test.KnownDivergence)
		}
	}

	return sb.String()
}

func truncate(s string, n int) string {
	s = strings.TrimSpace(s)
	if len(s) <= n {
		return s
	}
	return s[:n] + "..."
}
