package shadowtest

import (
	"context"
	"encoding/json"
	"strings"
	"testing"
)

// TestDefaultNormalizationRules verifies the default rules include all expected
// schema evolution mappings.
func TestDefaultNormalizationRules(t *testing.T) {
	t.Parallel()

	rules := DefaultNormalizationRules()

	if rules.FieldRenames["type"] != "event" {
		t.Errorf("expected FieldRenames[type]=event, got %q", rules.FieldRenames["type"])
	}
	if rules.FieldRenames["ts"] != "timestamp" {
		t.Errorf("expected FieldRenames[ts]=timestamp, got %q", rules.FieldRenames["ts"])
	}
	if !containsString(rules.OmitFields, "id") {
		t.Error("expected OmitFields to include 'id'")
	}
	if !containsString(rules.OmitStdoutFields, "event_id") {
		t.Error("expected OmitStdoutFields to include 'event_id'")
	}
	if !containsString(rules.OmitStdoutFields, "verdict") {
		t.Error("expected OmitStdoutFields to include 'verdict'")
	}
	if rules.EventTypeMapping["session_metadata"] != "session_register" {
		t.Errorf("expected EventTypeMapping[session_metadata]=session_register, got %q",
			rules.EventTypeMapping["session_metadata"])
	}
	if rules.FlattenKeyValue {
		t.Error("expected DefaultNormalizationRules FlattenKeyValue=false")
	}
}

// TestSessionRegisterNormalizationRules verifies that session-register rules
// include FlattenKeyValue on top of default rules.
func TestSessionRegisterNormalizationRules(t *testing.T) {
	t.Parallel()

	rules := SessionRegisterNormalizationRules()

	if !rules.FlattenKeyValue {
		t.Error("expected SessionRegisterNormalizationRules FlattenKeyValue=true")
	}
	// Must still include all default mappings.
	if rules.FieldRenames["type"] != "event" {
		t.Errorf("expected FieldRenames[type]=event, got %q", rules.FieldRenames["type"])
	}
}

// TestTaskUpdateNormalizationRules verifies that task-update rules include the
// status→task_status rename on top of default rules.
func TestTaskUpdateNormalizationRules(t *testing.T) {
	t.Parallel()

	rules := TaskUpdateNormalizationRules()

	if rules.FieldRenames["status"] != "task_status" {
		t.Errorf("expected FieldRenames[status]=task_status, got %q", rules.FieldRenames["status"])
	}
	// Must still include all default mappings.
	if rules.FieldRenames["ts"] != "timestamp" {
		t.Errorf("expected FieldRenames[ts]=timestamp, got %q", rules.FieldRenames["ts"])
	}
}

// TestNormalizeJSONLEvent_FieldRenames verifies that type→event and ts→timestamp
// renames are applied correctly.
func TestNormalizeJSONLEvent_FieldRenames(t *testing.T) {
	t.Parallel()

	event := map[string]interface{}{
		"type":       "phase_transition",
		"ts":         "2026-01-01T00:00:00Z",
		"session_id": "ses-001",
	}
	rules := DefaultNormalizationRules()
	result := NormalizeJSONLEvent(event, rules)

	if _, ok := result["type"]; ok {
		t.Error("expected 'type' field to be renamed, still present")
	}
	if result["event"] != "phase_transition" {
		t.Errorf("expected result[event]=phase_transition, got %v", result["event"])
	}
	if _, ok := result["ts"]; ok {
		t.Error("expected 'ts' field to be renamed, still present")
	}
	if result["timestamp"] != "2026-01-01T00:00:00Z" {
		t.Errorf("expected result[timestamp]=2026-01-01T00:00:00Z, got %v", result["timestamp"])
	}
}

// TestNormalizeJSONLEvent_OmitFields verifies that the 'id' field is stripped.
func TestNormalizeJSONLEvent_OmitFields(t *testing.T) {
	t.Parallel()

	event := map[string]interface{}{
		"type": "phase_transition",
		"ts":   "2026-01-01T00:00:00Z",
		"id":   "EVT-01ABC123",
	}
	rules := DefaultNormalizationRules()
	result := NormalizeJSONLEvent(event, rules)

	if _, ok := result["id"]; ok {
		t.Error("expected 'id' field to be omitted, still present")
	}
}

// TestNormalizeJSONLEvent_EventTypeMapping verifies that session_metadata is
// mapped to session_register.
func TestNormalizeJSONLEvent_EventTypeMapping(t *testing.T) {
	t.Parallel()

	event := map[string]interface{}{
		"type":       "session_metadata",
		"session_id": "ses-001",
	}
	rules := DefaultNormalizationRules()
	result := NormalizeJSONLEvent(event, rules)

	// After rename: type→event, then mapping: session_metadata→session_register.
	if result["event"] != "session_register" {
		t.Errorf("expected event=session_register after mapping, got %v", result["event"])
	}
}

// TestNormalizeJSONLEvent_FlattenKeyValue verifies that key/value envelopes
// are flattened to top-level fields.
func TestNormalizeJSONLEvent_FlattenKeyValue(t *testing.T) {
	t.Parallel()

	event := map[string]interface{}{
		"type":       "session_metadata",
		"session_id": "ses-001",
		"key":        "tracking_level",
		"value":      "interactive",
	}
	rules := SessionRegisterNormalizationRules()
	result := NormalizeJSONLEvent(event, rules)

	if _, ok := result["key"]; ok {
		t.Error("expected 'key' field to be removed after flatten, still present")
	}
	if _, ok := result["value"]; ok {
		t.Error("expected 'value' field to be removed after flatten, still present")
	}
	if result["tracking_level"] != "interactive" {
		t.Errorf("expected result[tracking_level]=interactive, got %v", result["tracking_level"])
	}
}

// TestNormalizeJSONLEvent_NilInput verifies that nil input is handled safely.
func TestNormalizeJSONLEvent_NilInput(t *testing.T) {
	t.Parallel()

	rules := DefaultNormalizationRules()
	result := NormalizeJSONLEvent(nil, rules)
	if result != nil {
		t.Errorf("expected nil result for nil input, got %v", result)
	}
}

// TestNormalizeJSONLOutput_MultipleLines verifies that multi-line JSONL is
// normalized with all rules applied to each line.
func TestNormalizeJSONLOutput_MultipleLines(t *testing.T) {
	t.Parallel()

	raw := `{"type":"phase_transition","ts":"2026-01-01T00:00:00Z","id":"EVT-01"}
{"type":"stage_transition","ts":"2026-01-01T00:01:00Z","id":"EVT-02"}`

	rules := DefaultNormalizationRules()
	result := NormalizeJSONLOutput(raw, rules)

	lines := strings.Split(strings.TrimSpace(result), "\n")
	if len(lines) != 2 {
		t.Fatalf("expected 2 output lines, got %d: %q", len(lines), result)
	}

	// Each line should be valid JSON with 'event' not 'type'.
	for i, line := range lines {
		var m map[string]interface{}
		if err := json.Unmarshal([]byte(line), &m); err != nil {
			t.Errorf("line %d: not valid JSON: %v", i, err)
			continue
		}
		if _, ok := m["type"]; ok {
			t.Errorf("line %d: 'type' field still present after normalization", i)
		}
		if _, ok := m["id"]; ok {
			t.Errorf("line %d: 'id' field still present after normalization", i)
		}
		if _, ok := m["event"]; !ok {
			t.Errorf("line %d: 'event' field missing after normalization", i)
		}
	}
}

// TestNormalizeJSONLOutput_SkipsNonJSON verifies that non-JSON lines are
// silently skipped.
func TestNormalizeJSONLOutput_SkipsNonJSON(t *testing.T) {
	t.Parallel()

	raw := `not-json-at-all
{"type":"phase_transition","ts":"2026-01-01T00:00:00Z"}
another non-json line`

	rules := DefaultNormalizationRules()
	result := NormalizeJSONLOutput(raw, rules)

	lines := strings.Split(strings.TrimSpace(result), "\n")
	if len(lines) != 1 {
		t.Errorf("expected 1 output line (skipping non-JSON), got %d: %q", len(lines), result)
	}
}

// TestNormalizeStdout_StripStdoutFields verifies that event_id and verdict
// are stripped from stdout JSON.
func TestNormalizeStdout_StripStdoutFields(t *testing.T) {
	t.Parallel()

	raw := `{"status":"recorded","event_id":"EVT-01ABC","verdict":"changes_requested","phase":"PF1-INIT"}`
	rules := DefaultNormalizationRules()
	result := NormalizeStdout(raw, rules)

	var m map[string]interface{}
	if err := json.Unmarshal([]byte(result), &m); err != nil {
		t.Fatalf("result is not valid JSON: %v", err)
	}
	if _, ok := m["event_id"]; ok {
		t.Error("expected 'event_id' to be stripped from stdout, still present")
	}
	if _, ok := m["verdict"]; ok {
		t.Error("expected 'verdict' to be stripped from stdout, still present")
	}
	if m["status"] != "recorded" {
		t.Errorf("expected status=recorded preserved, got %v", m["status"])
	}
}

// TestNormalizeStdout_NonJSONPassthrough verifies that non-JSON stdout is
// returned unchanged.
func TestNormalizeStdout_NonJSONPassthrough(t *testing.T) {
	t.Parallel()

	raw := "BLOCKED: dangerous command"
	rules := DefaultNormalizationRules()
	result := NormalizeStdout(raw, rules)

	if result != raw {
		t.Errorf("expected non-JSON stdout unchanged, got %q", result)
	}
}

// TestNormalizeStdout_EmptyInput verifies empty input returns empty string.
func TestNormalizeStdout_EmptyInput(t *testing.T) {
	t.Parallel()

	rules := DefaultNormalizationRules()
	result := NormalizeStdout("", rules)
	if result != "" {
		t.Errorf("expected empty result for empty input, got %q", result)
	}
}

// TestFilterStderr_KeepsStructuredOutput verifies that JSON and error-prefixed
// lines are kept.
func TestFilterStderr_KeepsStructuredOutput(t *testing.T) {
	t.Parallel()

	raw := `sourcing lib.sh...
DEBUG: loading configuration
{"error":"validation failed"}
BLOCKED: operation denied
ERROR: unexpected exit
WARNING: deprecated flag
`
	result := FilterStderr(raw, NormalizationRules{})
	lines := strings.Split(strings.TrimSpace(result), "\n")

	if len(lines) != 4 {
		t.Errorf("expected 4 kept lines, got %d: %q", len(lines), result)
	}
	if !strings.Contains(result, `{"error":"validation failed"}`) {
		t.Error("expected JSON line to be kept")
	}
	if !strings.Contains(result, "BLOCKED:") {
		t.Error("expected BLOCKED line to be kept")
	}
	if !strings.Contains(result, "ERROR:") {
		t.Error("expected ERROR line to be kept")
	}
	if !strings.Contains(result, "WARNING:") {
		t.Error("expected WARNING line to be kept")
	}
}

// TestFilterStderr_StripsDebugLines verifies that shell debug/trace lines are
// filtered out.
func TestFilterStderr_StripsDebugLines(t *testing.T) {
	t.Parallel()

	raw := `sourcing lib.sh...
+ echo hello
:: trace line`
	result := FilterStderr(raw, NormalizationRules{})

	if result != "" {
		t.Errorf("expected empty result after filtering debug lines, got %q", result)
	}
}

// TestFilterStderr_NoisePatterns verifies that StderrNoisePatterns drops
// matching lines from stderr.
func TestFilterStderr_NoisePatterns(t *testing.T) {
	t.Parallel()

	rules := NormalizationRules{
		StderrNoisePatterns: []string{"WARNING:", "STALE SESSIONS"},
	}
	raw := `WARNING: stale session detected
STALE SESSIONS found in directory
{"error":"real error"}
ERROR: actual problem`
	result := FilterStderr(raw, rules)

	if strings.Contains(result, "WARNING:") {
		t.Error("expected WARNING: line to be stripped by noise pattern")
	}
	if strings.Contains(result, "STALE SESSIONS") {
		t.Error("expected STALE SESSIONS line to be stripped by noise pattern")
	}
	if !strings.Contains(result, `{"error":"real error"}`) {
		t.Error("expected JSON error line to be kept")
	}
	if !strings.Contains(result, "ERROR: actual problem") {
		t.Error("expected ERROR line to be kept")
	}
}

// TestFilterStderr_OmitStderrFields verifies that OmitStderrFields strips
// named fields from JSON stderr lines before comparison, allowing Go and shell
// to have different error message wording while still comparing structure.
func TestFilterStderr_OmitStderrFields(t *testing.T) {
	t.Parallel()

	rules := NormalizationRules{
		OmitStderrFields: []string{"error"},
	}
	// Go outputs: {"error":"pathflow: invalid phase \"PF9-INVALID\""}
	// Shell outputs: {"error":"invalid phase: PF9-INVALID. Valid: PF1-INIT..."}
	// After stripping "error" field both become: {}
	goStderr := `{"error":"pathflow: invalid phase \"PF9-INVALID\""}`
	shellStderr := `{"error":"invalid phase: PF9-INVALID. Valid: PF1-INIT, PF2-CONTEXT"}`

	goFiltered := FilterStderr(goStderr, rules)
	shellFiltered := FilterStderr(shellStderr, rules)

	if goFiltered != shellFiltered {
		t.Errorf("expected OmitStderrFields to normalize both sides to equal, got:\n  Go:    %q\n  Shell: %q", goFiltered, shellFiltered)
	}

	// Verify the filtered result is valid JSON with no "error" field.
	var m map[string]interface{}
	if err := json.Unmarshal([]byte(goFiltered), &m); err != nil {
		t.Fatalf("filtered result is not valid JSON: %v", err)
	}
	if _, ok := m["error"]; ok {
		t.Error("expected 'error' field to be stripped from filtered stderr, still present")
	}
}

// TestFilterStderr_OmitStderrFields_NonJSONUnchanged verifies that non-JSON
// lines are not affected by OmitStderrFields (they are kept as-is).
func TestFilterStderr_OmitStderrFields_NonJSONUnchanged(t *testing.T) {
	t.Parallel()

	rules := NormalizationRules{
		OmitStderrFields: []string{"error"},
	}
	// BLOCKED lines are not JSON — they should pass through unchanged.
	raw := "BLOCKED: dangerous operation denied"
	result := FilterStderr(raw, rules)

	if result != raw {
		t.Errorf("expected non-JSON BLOCKED line unchanged, got %q", result)
	}
}

// TestPhaseTransitionErrorNormalizationRules verifies that the constructor
// returns rules with OmitStderrFields set to ["error"].
func TestPhaseTransitionErrorNormalizationRules(t *testing.T) {
	t.Parallel()

	r := PhaseTransitionErrorNormalizationRules()
	if !containsString(r.OmitStderrFields, "error") {
		t.Errorf("expected OmitStderrFields to contain 'error', got %v", r.OmitStderrFields)
	}
	// Must still include default field renames.
	if r.FieldRenames["type"] != "event" {
		t.Errorf("expected FieldRenames[type]=event, got %q", r.FieldRenames["type"])
	}
}

// TestSessionRegisterErrorNormalizationRules verifies that the constructor
// returns rules with OmitStderrFields set to ["error"].
func TestSessionRegisterErrorNormalizationRules(t *testing.T) {
	t.Parallel()

	r := SessionRegisterErrorNormalizationRules()
	if !containsString(r.OmitStderrFields, "error") {
		t.Errorf("expected OmitStderrFields to contain 'error', got %v", r.OmitStderrFields)
	}
	// Must still include default field renames.
	if r.FieldRenames["ts"] != "timestamp" {
		t.Errorf("expected FieldRenames[ts]=timestamp, got %q", r.FieldRenames["ts"])
	}
}

// TestCompareOutputs_OmitStderrFieldsNormalizesWording verifies that stderr
// divergence is NOT reported when both sides have the same JSON structure but
// different "error" field wording.
func TestCompareOutputs_OmitStderrFieldsNormalizesWording(t *testing.T) {
	t.Parallel()

	goResult := &ExecResult{
		ExitCode: 2,
		Stdout:   "",
		Stderr:   `{"error":"pathflow: invalid phase \"PF9-INVALID\""}`,
	}
	shellResult := &ExecResult{
		ExitCode: 2,
		Stdout:   "",
		Stderr:   `{"error":"invalid phase: PF9-INVALID. Valid: PF1-INIT, PF2-CONTEXT"}`,
	}

	r := PhaseTransitionErrorNormalizationRules()
	divergences := CompareOutputs(goResult, shellResult, r)

	for _, d := range divergences {
		if d.Kind == DivergenceStderr {
			t.Errorf("unexpected stderr divergence — error wording should be normalized away: %v", d)
		}
	}
}

// TestNormalizeStdout_StripLinePrefixes verifies that StripStdoutLinePrefixes
// drops matching lines from stdout before JSON field stripping.
func TestNormalizeStdout_StripLinePrefixes(t *testing.T) {
	t.Parallel()

	rules := NormalizationRules{
		StripStdoutLinePrefixes: []string{"SessionEnd:", "[INFO] "},
	}
	raw := `SessionEnd: No expired sentinels to clean
[INFO] Validating task file
PASS`
	result := NormalizeStdout(raw, rules)

	if strings.Contains(result, "SessionEnd:") {
		t.Error("expected SessionEnd: line to be stripped")
	}
	if strings.Contains(result, "[INFO]") {
		t.Error("expected [INFO] line to be stripped")
	}
	if result != "PASS" {
		t.Errorf("expected 'PASS' after stripping prefixes, got %q", result)
	}
}

// TestSessionStartNormalizationRules verifies the session-start rules include
// stderr noise patterns for stale session warnings and a StripStdoutLinePrefixes
// entry to drop the Go init {"env":{...}} JSON line.
func TestSessionStartNormalizationRules(t *testing.T) {
	t.Parallel()

	r := SessionStartNormalizationRules()
	if len(r.StderrNoisePatterns) == 0 {
		t.Error("expected StderrNoisePatterns to be set for session-start rules")
	}
	found := false
	for _, p := range r.StderrNoisePatterns {
		if strings.Contains(p, "WARNING") || strings.Contains(p, "STALE") {
			found = true
			break
		}
	}
	if !found {
		t.Errorf("expected WARNING or STALE pattern in StderrNoisePatterns, got %v", r.StderrNoisePatterns)
	}

	// Verify that the Go init {"env":{...}} JSON line is stripped so it does not
	// diverge from the shell's empty stdout.
	foundEnvPrefix := false
	for _, p := range r.StripStdoutLinePrefixes {
		if strings.Contains(p, `{"env":`) {
			foundEnvPrefix = true
			break
		}
	}
	if !foundEnvPrefix {
		t.Errorf(`expected {"env": prefix in StripStdoutLinePrefixes, got %v`, r.StripStdoutLinePrefixes)
	}
}

// TestSessionEndNormalizationRules verifies the session-end rules strip
// "SessionEnd:" stdout prefix lines.
func TestSessionEndNormalizationRules(t *testing.T) {
	t.Parallel()

	r := SessionEndNormalizationRules()
	if len(r.StripStdoutLinePrefixes) == 0 {
		t.Error("expected StripStdoutLinePrefixes to be set for session-end rules")
	}
	found := false
	for _, p := range r.StripStdoutLinePrefixes {
		if strings.HasPrefix(p, "SessionEnd") {
			found = true
			break
		}
	}
	if !found {
		t.Errorf("expected SessionEnd prefix in StripStdoutLinePrefixes, got %v", r.StripStdoutLinePrefixes)
	}
}

// TestValidationNormalizationRules verifies the validation rules strip
// "[INFO] " stdout prefix lines and Go's "Validation PASSED/FAILED" summary lines.
func TestValidationNormalizationRules(t *testing.T) {
	t.Parallel()

	r := ValidationNormalizationRules()
	if len(r.StripStdoutLinePrefixes) == 0 {
		t.Error("expected StripStdoutLinePrefixes to be set for validation rules")
	}
	foundInfo := false
	foundValidation := false
	for _, p := range r.StripStdoutLinePrefixes {
		if strings.Contains(p, "[INFO]") {
			foundInfo = true
		}
		if strings.HasPrefix(p, "Validation") {
			foundValidation = true
		}
	}
	if !foundInfo {
		t.Errorf("expected [INFO] prefix in StripStdoutLinePrefixes, got %v", r.StripStdoutLinePrefixes)
	}
	if !foundValidation {
		t.Errorf("expected Validation prefix in StripStdoutLinePrefixes, got %v", r.StripStdoutLinePrefixes)
	}
}

// TestValidationNormalizationRules_StripsGoSummaryLine verifies that Go's
// "Validation PASSED" summary line is stripped, normalizing to empty stdout
// so the exit code is the sole comparison signal.
func TestValidationNormalizationRules_StripsGoSummaryLine(t *testing.T) {
	t.Parallel()

	r := ValidationNormalizationRules()

	// Go outputs "Validation PASSED" — should be stripped.
	goResult := NormalizeStdout("Validation PASSED", r)
	if goResult != "" {
		t.Errorf("expected Go 'Validation PASSED' to normalize to empty, got %q", goResult)
	}

	// Shell outputs "[INFO] Validating: ..." lines — already stripped by existing rule.
	shellResult := NormalizeStdout("[INFO] Validating task file: /some/path/task.md", r)
	if shellResult != "" {
		t.Errorf("expected shell [INFO] line to normalize to empty, got %q", shellResult)
	}
}

// TestCompareOutputs_MatchingOutputs verifies zero divergences for identical
// Go and shell results.
func TestCompareOutputs_MatchingOutputs(t *testing.T) {
	t.Parallel()

	goResult := &ExecResult{ExitCode: 0, Stdout: `{"status":"ok"}`, Stderr: ""}
	shellResult := &ExecResult{ExitCode: 0, Stdout: `{"status":"ok"}`, Stderr: ""}

	r := DefaultNormalizationRules()
	divergences := CompareOutputs(goResult, shellResult, r)

	if len(divergences) != 0 {
		t.Errorf("expected 0 divergences for matching outputs, got %d: %v", len(divergences), divergences)
	}
}

// TestCompareOutputs_ExitCodeDivergence verifies that exit code differences
// are reported.
func TestCompareOutputs_ExitCodeDivergence(t *testing.T) {
	t.Parallel()

	goResult := &ExecResult{ExitCode: 2, Stdout: "", Stderr: "BLOCKED: denied"}
	shellResult := &ExecResult{ExitCode: 0, Stdout: "", Stderr: ""}

	r := DefaultNormalizationRules()
	divergences := CompareOutputs(goResult, shellResult, r)

	if len(divergences) == 0 {
		t.Fatal("expected divergences for exit code mismatch, got 0")
	}
	found := false
	for _, d := range divergences {
		if d.Kind == DivergenceExitCode {
			found = true
			if d.GoValue != "2" {
				t.Errorf("expected GoValue=2, got %q", d.GoValue)
			}
			if d.ShellValue != "0" {
				t.Errorf("expected ShellValue=0, got %q", d.ShellValue)
			}
		}
	}
	if !found {
		t.Error("expected DivergenceExitCode in divergences")
	}
}

// TestCompareOutputs_StdoutDivergence verifies that stdout differences
// (after normalization) are reported.
func TestCompareOutputs_StdoutDivergence(t *testing.T) {
	t.Parallel()

	goResult := &ExecResult{ExitCode: 0, Stdout: `{"status":"recorded"}`, Stderr: ""}
	shellResult := &ExecResult{ExitCode: 0, Stdout: `{"status":"written"}`, Stderr: ""}

	r := DefaultNormalizationRules()
	divergences := CompareOutputs(goResult, shellResult, r)

	if len(divergences) == 0 {
		t.Fatal("expected divergences for stdout mismatch, got 0")
	}
	found := false
	for _, d := range divergences {
		if d.Kind == DivergenceStdout {
			found = true
		}
	}
	if !found {
		t.Error("expected DivergenceStdout in divergences")
	}
}

// TestCompareOutputs_StdoutNormalizationExcludesEventID verifies that event_id
// differences are NOT reported (normalized away).
func TestCompareOutputs_StdoutNormalizationExcludesEventID(t *testing.T) {
	t.Parallel()

	goResult := &ExecResult{ExitCode: 0, Stdout: `{"status":"recorded","phase":"PF1-INIT"}`, Stderr: ""}
	shellResult := &ExecResult{ExitCode: 0, Stdout: `{"status":"recorded","phase":"PF1-INIT","event_id":"EVT-01ABC"}`, Stderr: ""}

	r := DefaultNormalizationRules()
	divergences := CompareOutputs(goResult, shellResult, r)

	for _, d := range divergences {
		if d.Kind == DivergenceStdout {
			t.Errorf("unexpected stdout divergence — event_id should be normalized away: %v", d)
		}
	}
}

// TestCompareOutputs_StderrDivergence verifies that stderr differences
// (after filtering) are reported.
func TestCompareOutputs_StderrDivergence(t *testing.T) {
	t.Parallel()

	goResult := &ExecResult{ExitCode: 2, Stdout: "", Stderr: "BLOCKED: go says no"}
	shellResult := &ExecResult{ExitCode: 2, Stdout: "", Stderr: "BLOCKED: shell says no"}

	r := DefaultNormalizationRules()
	divergences := CompareOutputs(goResult, shellResult, r)

	found := false
	for _, d := range divergences {
		if d.Kind == DivergenceStderr {
			found = true
		}
	}
	if !found {
		t.Error("expected DivergenceStderr in divergences")
	}
}

// TestCompareOutputs_StderrNormalizationExcludesDebug verifies that debug lines
// on stderr do NOT produce a divergence.
func TestCompareOutputs_StderrNormalizationExcludesDebug(t *testing.T) {
	t.Parallel()

	goResult := &ExecResult{ExitCode: 0, Stdout: `{"status":"ok"}`, Stderr: ""}
	shellResult := &ExecResult{ExitCode: 0, Stdout: `{"status":"ok"}`, Stderr: "sourcing lib.sh...\nDEBUG: loaded"}

	r := DefaultNormalizationRules()
	divergences := CompareOutputs(goResult, shellResult, r)

	for _, d := range divergences {
		if d.Kind == DivergenceStderr {
			t.Errorf("unexpected stderr divergence — debug lines should be filtered: %v", d)
		}
	}
}

// TestCompareJSONLOutputs_MatchingAfterNormalization verifies that schema
// differences (type→event, ts→timestamp, id absent in Go) do not cause
// divergence after normalization.
func TestCompareJSONLOutputs_MatchingAfterNormalization(t *testing.T) {
	t.Parallel()

	goOutput := `{"event":"phase_transition","timestamp":"2026-01-01T00:00:00Z","session_id":"ses-001","phase":"PF1-INIT","status":"entered"}`
	shellOutput := `{"type":"phase_transition","ts":"2026-01-01T00:00:00Z","session_id":"ses-001","phase":"PF1-INIT","status":"entered","id":"EVT-01ABC"}`

	r := DefaultNormalizationRules()
	div := CompareJSONLOutputs(goOutput, shellOutput, r)
	if div != nil {
		t.Errorf("expected nil divergence after normalization, got: %v", div)
	}
}

// TestCompareJSONLOutputs_ActualDivergence verifies that genuine content
// differences are detected.
func TestCompareJSONLOutputs_ActualDivergence(t *testing.T) {
	t.Parallel()

	goOutput := `{"event":"phase_transition","timestamp":"2026-01-01T00:00:00Z","phase":"PF1-INIT","status":"entered"}`
	shellOutput := `{"type":"phase_transition","ts":"2026-01-01T00:00:00Z","phase":"PF2-CONTEXT","status":"entered"}`

	r := DefaultNormalizationRules()
	div := CompareJSONLOutputs(goOutput, shellOutput, r)
	if div == nil {
		t.Error("expected divergence for different phase values, got nil")
	}
}

// TestShadowResult_HasUnexpectedDivergences_NoDivergences verifies false when
// no divergences are found.
func TestShadowResult_HasUnexpectedDivergences_NoDivergences(t *testing.T) {
	t.Parallel()

	result := &ShadowResult{
		Test:        ShadowTest{Name: "test/no-divergences"},
		Divergences: nil,
	}
	if result.HasUnexpectedDivergences() {
		t.Error("expected HasUnexpectedDivergences()=false for empty divergences")
	}
}

// TestShadowResult_HasUnexpectedDivergences_WithDivergences verifies true when
// unexpected divergences exist.
func TestShadowResult_HasUnexpectedDivergences_WithDivergences(t *testing.T) {
	t.Parallel()

	result := &ShadowResult{
		Test: ShadowTest{Name: "test/unexpected"},
		Divergences: []Divergence{
			{Kind: DivergenceExitCode, GoValue: "2", ShellValue: "0"},
		},
	}
	if !result.HasUnexpectedDivergences() {
		t.Error("expected HasUnexpectedDivergences()=true for unexpected divergences")
	}
}

// TestShadowResult_HasUnexpectedDivergences_KnownDivergence verifies false when
// all divergences are covered by KnownDivergence.
func TestShadowResult_HasUnexpectedDivergences_KnownDivergence(t *testing.T) {
	t.Parallel()

	result := &ShadowResult{
		Test: ShadowTest{
			Name:            "test/known-divergence",
			KnownDivergence: "Shell accepts iteration=0; Go requires iteration>=1",
		},
		Divergences: []Divergence{
			{Kind: DivergenceExitCode, GoValue: "2", ShellValue: "0"},
		},
	}
	if result.HasUnexpectedDivergences() {
		t.Error("expected HasUnexpectedDivergences()=false when KnownDivergence is set")
	}
}

// TestFormatDivergenceReport_AllPass verifies the report format when all tests
// pass.
func TestFormatDivergenceReport_AllPass(t *testing.T) {
	t.Parallel()

	results := []*ShadowResult{
		{
			Test:        ShadowTest{Name: "test/alpha"},
			GoResult:    &ExecResult{ExitCode: 0, Stdout: `{"status":"ok"}`},
			ShellResult: &ExecResult{ExitCode: 0, Stdout: `{"status":"ok"}`},
			Divergences: nil,
		},
		{
			Test:        ShadowTest{Name: "test/beta"},
			GoResult:    &ExecResult{ExitCode: 0, Stdout: `{"status":"ok"}`},
			ShellResult: &ExecResult{ExitCode: 0, Stdout: `{"status":"ok"}`},
			Divergences: nil,
		},
	}

	report := FormatDivergenceReport(results)
	if !strings.Contains(report, "2/2 passed") {
		t.Errorf("expected '2/2 passed' in report, got: %q", report)
	}
	if !strings.Contains(report, "[PASS]") {
		t.Error("expected [PASS] entries in report")
	}
	if strings.Contains(report, "[FAIL]") {
		t.Error("unexpected [FAIL] entry in all-pass report")
	}
}

// TestFormatDivergenceReport_WithFail verifies the report format when a test
// fails.
func TestFormatDivergenceReport_WithFail(t *testing.T) {
	t.Parallel()

	results := []*ShadowResult{
		{
			Test:        ShadowTest{Name: "test/alpha"},
			GoResult:    &ExecResult{ExitCode: 2, Stdout: ""},
			ShellResult: &ExecResult{ExitCode: 0, Stdout: ""},
			Divergences: []Divergence{
				{Kind: DivergenceExitCode, GoValue: "2", ShellValue: "0", Diff: "exit code: Go=2, shell=0"},
			},
		},
	}

	report := FormatDivergenceReport(results)
	if !strings.Contains(report, "0/1 passed") {
		t.Errorf("expected '0/1 passed' in report, got: %q", report)
	}
	if !strings.Contains(report, "[FAIL]") {
		t.Error("expected [FAIL] entry in report")
	}
	if !strings.Contains(report, "exit_code") {
		t.Error("expected divergence kind in report")
	}
}

// TestFormatDivergenceReport_KnownDivergence verifies that known divergences
// show as EXPECTED, not FAIL.
func TestFormatDivergenceReport_KnownDivergence(t *testing.T) {
	t.Parallel()

	results := []*ShadowResult{
		{
			Test: ShadowTest{
				Name:            "test/known",
				KnownDivergence: "shell accepts iteration=0, Go requires >=1",
			},
			GoResult:    &ExecResult{ExitCode: 2},
			ShellResult: &ExecResult{ExitCode: 0},
			Divergences: []Divergence{
				{Kind: DivergenceExitCode, GoValue: "2", ShellValue: "0"},
			},
		},
	}

	report := FormatDivergenceReport(results)
	if !strings.Contains(report, "1/1 passed") {
		t.Errorf("expected '1/1 passed' in report (known divergence counts as pass), got: %q", report)
	}
	if !strings.Contains(report, "[EXPECTED]") {
		t.Error("expected [EXPECTED] entry in report for known divergence")
	}
	if strings.Contains(report, "[FAIL]") {
		t.Error("unexpected [FAIL] for known divergence")
	}
}

// TestBuildEnv_ContainsRequiredVars verifies that buildEnv includes the
// project-specific CF_PROJECT_ROOT variable and test overrides.
func TestBuildEnv_ContainsRequiredVars(t *testing.T) {
	t.Parallel()

	testEnv := map[string]string{
		"MY_TEST_VAR": "test-value",
	}
	result := buildEnv(testEnv, "/tmp/test-project")

	hasRoot := false
	hasMyVar := false
	for _, entry := range result {
		if entry == "CF_PROJECT_ROOT=/tmp/test-project" {
			hasRoot = true
		}
		if entry == "MY_TEST_VAR=test-value" {
			hasMyVar = true
		}
	}

	if !hasRoot {
		t.Error("expected CF_PROJECT_ROOT in env")
	}
	if !hasMyVar {
		t.Error("expected MY_TEST_VAR override in env")
	}
}

// TestBuildEnv_OverrideDefault verifies that test-specific values override
// defaults.
func TestBuildEnv_OverrideDefault(t *testing.T) {
	t.Parallel()

	testEnv := map[string]string{
		"HOME": "/override/home",
	}
	result := buildEnv(testEnv, "/tmp/project")

	hasOverride := false
	for _, entry := range result {
		if entry == "HOME=/override/home" {
			hasOverride = true
		}
	}
	if !hasOverride {
		t.Error("expected HOME override in env")
	}
}

// TestRunCommand_SuccessfulEcho verifies that runCommand captures stdout and
// exit code from a real process.
func TestRunCommand_SuccessfulEcho(t *testing.T) {
	t.Parallel()

	ctx := context.TODO()
	env := buildEnv(nil, "/tmp")
	result, err := runCommand(ctx, "", []string{"sh", "-c", "echo hello"}, nil, env, "/tmp")
	if err != nil {
		t.Fatalf("runCommand() unexpected error: %v", err)
	}
	if result.ExitCode != 0 {
		t.Errorf("expected exit code 0, got %d", result.ExitCode)
	}
	if !strings.Contains(result.Stdout, "hello") {
		t.Errorf("expected stdout to contain 'hello', got %q", result.Stdout)
	}
}

// TestRunCommand_NonZeroExit verifies that non-zero exit codes are captured
// without returning a Go error.
func TestRunCommand_NonZeroExit(t *testing.T) {
	t.Parallel()

	ctx := context.TODO()
	env := buildEnv(nil, "/tmp")
	result, err := runCommand(ctx, "", []string{"sh", "-c", "exit 2"}, nil, env, "/tmp")
	if err != nil {
		t.Fatalf("runCommand() unexpected error: %v", err)
	}
	if result.ExitCode != 2 {
		t.Errorf("expected exit code 2, got %d", result.ExitCode)
	}
}

// TestRunCommand_StdinPassed verifies that stdin is passed to the command.
func TestRunCommand_StdinPassed(t *testing.T) {
	t.Parallel()

	ctx := context.TODO()
	env := buildEnv(nil, "/tmp")
	stdin := []byte("hello from stdin")
	result, err := runCommand(ctx, "", []string{"cat"}, stdin, env, "/tmp")
	if err != nil {
		t.Fatalf("runCommand() unexpected error: %v", err)
	}
	if !strings.Contains(result.Stdout, "hello from stdin") {
		t.Errorf("expected stdout to contain stdin content, got %q", result.Stdout)
	}
}

// TestRunCommand_EmptyArgsError verifies that an empty shell command returns
// an error.
func TestRunCommand_EmptyArgsError(t *testing.T) {
	t.Parallel()

	ctx := context.TODO()
	env := buildEnv(nil, "/tmp")
	_, err := runCommand(ctx, "", []string{}, nil, env, "/tmp")
	if err == nil {
		t.Error("expected error for empty shell command, got nil")
	}
}

// TestShadowTest_Run_MatchingCommands verifies that ShadowTest.Run produces
// zero divergences when both implementations return identical output.
func TestShadowTest_Run_MatchingCommands(t *testing.T) {
	t.Parallel()

	st := ShadowTest{
		Name:             "test/matching-echo",
		GoCommand:        []string{"echo", "parity"},
		ShellCommand:     []string{"sh", "-c", "echo parity"},
		Stdin:            []byte{},
		Env:              map[string]string{},
		Rules:            rules(DefaultNormalizationRules()),
		ExpectedExitCode: 0,
	}

	ctx := context.TODO()
	// Use "sh" as the Go binary for unit test parity.
	result, err := st.Run(ctx, "sh", "/tmp")
	if err != nil {
		t.Fatalf("ShadowTest.Run() unexpected error: %v", err)
	}
	if result.GoResult == nil {
		t.Fatal("expected non-nil GoResult")
	}
	if result.ShellResult == nil {
		t.Fatal("expected non-nil ShellResult")
	}
	if result.Log.Name != "test/matching-echo" {
		t.Errorf("expected Log.Name=test/matching-echo, got %q", result.Log.Name)
	}
	if result.Log.StdinHash == "" {
		t.Error("expected non-empty StdinHash in DivergenceLog")
	}
}

// TestShadowTest_Run_DifferentExitCodes verifies that ShadowTest.Run detects
// exit code divergences when one implementation succeeds and one fails.
func TestShadowTest_Run_DifferentExitCodes(t *testing.T) {
	t.Parallel()

	st := ShadowTest{
		Name:         "test/exit-divergence",
		GoCommand:    []string{"-c", "exit 0"},
		ShellCommand: []string{"sh", "-c", "exit 1"},
		Stdin:        []byte{},
		Env:          map[string]string{},
		Rules:        rules(DefaultNormalizationRules()),
	}

	ctx := context.TODO()
	result, err := st.Run(ctx, "sh", "/tmp")
	if err != nil {
		t.Fatalf("ShadowTest.Run() unexpected error: %v", err)
	}

	found := false
	for _, d := range result.Divergences {
		if d.Kind == DivergenceExitCode {
			found = true
		}
	}
	if !found {
		t.Error("expected DivergenceExitCode for different exit codes")
	}
}

// TestTruncate verifies that long strings are truncated with ellipsis.
func TestTruncate(t *testing.T) {
	t.Parallel()

	short := truncate("hello", 10)
	if short != "hello" {
		t.Errorf("expected short string unchanged, got %q", short)
	}

	long := truncate("hello world this is a long string", 10)
	if !strings.HasSuffix(long, "...") {
		t.Errorf("expected truncated string to end with '...', got %q", long)
	}
	if len(long) > 13 { // 10 chars + "..."
		t.Errorf("expected truncated string len <= 13, got %d: %q", len(long), long)
	}
}

// containsString is a helper that checks if a string slice contains a value.
func containsString(slice []string, target string) bool {
	for _, s := range slice {
		if s == target {
			return true
		}
	}
	return false
}

