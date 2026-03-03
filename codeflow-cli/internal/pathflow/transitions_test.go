package pathflow

import (
	"bufio"
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"

	"github.com/codeflow/codeflow-cli/internal/ledger"
)

// newTestWriter creates a TransitionWriter in a temporary directory.
func newTestWriter(t *testing.T) (*TransitionWriter, string) {
	t.Helper()
	dir := t.TempDir()
	tw, err := NewTransitionWriter(context.TODO(), dir)
	if err != nil {
		t.Fatalf("NewTransitionWriter: %v", err)
	}
	return tw, dir
}

// readEvents reads all JSONL events from the pathflow events file.
func readEvents(t *testing.T, dir string) []map[string]any {
	t.Helper()
	path := filepath.Join(dir, ledger.FilePathflowEvents)
	f, err := os.Open(path)
	if err != nil {
		t.Fatalf("opening events file: %v", err)
	}
	defer f.Close()

	var events []map[string]any
	scanner := bufio.NewScanner(f)
	for scanner.Scan() {
		var m map[string]any
		if err := json.Unmarshal(scanner.Bytes(), &m); err != nil {
			t.Fatalf("parsing event line: %v", err)
		}
		events = append(events, m)
	}
	if err := scanner.Err(); err != nil {
		t.Fatalf("scanning events: %v", err)
	}
	return events
}

func TestPhaseTransitionValidation(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name    string
		params  PhaseTransitionParams
		wantErr string
	}{
		{
			name:    "missing session_id",
			params:  PhaseTransitionParams{Phase: "PF1-INIT", Status: "entered"},
			wantErr: "session_id is required",
		},
		{
			name:    "missing phase",
			params:  PhaseTransitionParams{SessionID: "ses-123", Status: "entered"},
			wantErr: "phase is required",
		},
		{
			name:    "invalid phase",
			params:  PhaseTransitionParams{SessionID: "ses-123", Phase: "PF8-UNKNOWN", Status: "entered"},
			wantErr: "invalid phase",
		},
		{
			name:    "missing status",
			params:  PhaseTransitionParams{SessionID: "ses-123", Phase: "PF1-INIT"},
			wantErr: "status is required",
		},
		{
			name:    "invalid status",
			params:  PhaseTransitionParams{SessionID: "ses-123", Phase: "PF1-INIT", Status: "running"},
			wantErr: "invalid phase status",
		},
		{
			name:   "valid params",
			params: PhaseTransitionParams{SessionID: "ses-123", Phase: "PF1-INIT", Status: "entered"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			err := tt.params.Validate()
			if tt.wantErr != "" {
				if err == nil {
					t.Fatalf("expected error containing %q, got nil", tt.wantErr)
				}
				if got := err.Error(); !contains(got, tt.wantErr) {
					t.Errorf("error = %q, want containing %q", got, tt.wantErr)
				}
			} else if err != nil {
				t.Fatalf("unexpected error: %v", err)
			}
		})
	}
}

func TestStageTransitionValidation(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name    string
		params  StageTransitionParams
		wantErr string
	}{
		{
			name:    "missing session_id",
			params:  StageTransitionParams{Stage: "WS-DEV", Status: "in_progress", Iteration: 1},
			wantErr: "session_id is required",
		},
		{
			name:    "missing stage",
			params:  StageTransitionParams{SessionID: "ses-123", Status: "in_progress", Iteration: 1},
			wantErr: "stage is required",
		},
		{
			name:    "invalid stage",
			params:  StageTransitionParams{SessionID: "ses-123", Stage: "WS-UNKNOWN", Status: "in_progress", Iteration: 1},
			wantErr: "invalid stage",
		},
		{
			name:    "missing status",
			params:  StageTransitionParams{SessionID: "ses-123", Stage: "WS-DEV", Iteration: 1},
			wantErr: "status is required",
		},
		{
			name:    "invalid status",
			params:  StageTransitionParams{SessionID: "ses-123", Stage: "WS-DEV", Status: "running", Iteration: 1},
			wantErr: "invalid stage status",
		},
		{
			name:    "invalid verdict",
			params:  StageTransitionParams{SessionID: "ses-123", Stage: "WS-DEV", Status: "complete", Iteration: 1, Verdict: "unknown"},
			wantErr: "invalid verdict",
		},
		{
			name:    "zero iteration",
			params:  StageTransitionParams{SessionID: "ses-123", Stage: "WS-DEV", Status: "in_progress", Iteration: 0},
			wantErr: "iteration must be >= 1",
		},
		{
			name:   "valid without verdict",
			params: StageTransitionParams{SessionID: "ses-123", Stage: "WS-DEV", Status: "in_progress", Iteration: 1},
		},
		{
			name:   "valid with verdict",
			params: StageTransitionParams{SessionID: "ses-123", Stage: "WS-REV", Status: "complete", Iteration: 1, Verdict: "approved"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			err := tt.params.Validate()
			if tt.wantErr != "" {
				if err == nil {
					t.Fatalf("expected error containing %q, got nil", tt.wantErr)
				}
				if got := err.Error(); !contains(got, tt.wantErr) {
					t.Errorf("error = %q, want containing %q", got, tt.wantErr)
				}
			} else if err != nil {
				t.Fatalf("unexpected error: %v", err)
			}
		})
	}
}

func TestSessionRegisterValidation(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name    string
		params  SessionRegisterParams
		wantErr string
	}{
		{
			name:    "missing session_id",
			params:  SessionRegisterParams{Mode: "interactive"},
			wantErr: "session_id is required",
		},
		{
			name:    "missing mode",
			params:  SessionRegisterParams{SessionID: "ses-123"},
			wantErr: "mode is required",
		},
		{
			name:    "invalid mode",
			params:  SessionRegisterParams{SessionID: "ses-123", Mode: "batch"},
			wantErr: "invalid mode",
		},
		{
			name:   "valid interactive",
			params: SessionRegisterParams{SessionID: "ses-123", Mode: "interactive"},
		},
		{
			name:   "valid autorun",
			params: SessionRegisterParams{SessionID: "ses-123", Mode: "autorun"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			err := tt.params.Validate()
			if tt.wantErr != "" {
				if err == nil {
					t.Fatalf("expected error containing %q, got nil", tt.wantErr)
				}
				if got := err.Error(); !contains(got, tt.wantErr) {
					t.Errorf("error = %q, want containing %q", got, tt.wantErr)
				}
			} else if err != nil {
				t.Fatalf("unexpected error: %v", err)
			}
		})
	}
}

func TestTaskUpdateValidation(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name    string
		params  TaskUpdateParams
		wantErr string
	}{
		{
			name:    "missing session_id",
			params:  TaskUpdateParams{TaskID: "PF3-TSK-01", TaskStatus: "completed"},
			wantErr: "session_id is required",
		},
		{
			name:    "missing task_id",
			params:  TaskUpdateParams{SessionID: "ses-123", TaskStatus: "completed"},
			wantErr: "task_id is required",
		},
		{
			name:    "invalid task_id format",
			params:  TaskUpdateParams{SessionID: "ses-123", TaskID: "TASK-001", TaskStatus: "completed"},
			wantErr: "invalid task_id format",
		},
		{
			name:    "task_id PF0 invalid",
			params:  TaskUpdateParams{SessionID: "ses-123", TaskID: "PF0-TSK-01", TaskStatus: "completed"},
			wantErr: "invalid task_id format",
		},
		{
			name:    "task_id PF8 invalid",
			params:  TaskUpdateParams{SessionID: "ses-123", TaskID: "PF8-TSK-01", TaskStatus: "completed"},
			wantErr: "invalid task_id format",
		},
		{
			name:    "missing task_status",
			params:  TaskUpdateParams{SessionID: "ses-123", TaskID: "PF3-TSK-01"},
			wantErr: "task_status is required",
		},
		{
			name:    "invalid task_status",
			params:  TaskUpdateParams{SessionID: "ses-123", TaskID: "PF3-TSK-01", TaskStatus: "running"},
			wantErr: "invalid task_status",
		},
		{
			name:   "valid params",
			params: TaskUpdateParams{SessionID: "ses-123", TaskID: "PF3-TSK-01", TaskStatus: "completed"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			err := tt.params.Validate()
			if tt.wantErr != "" {
				if err == nil {
					t.Fatalf("expected error containing %q, got nil", tt.wantErr)
				}
				if got := err.Error(); !contains(got, tt.wantErr) {
					t.Errorf("error = %q, want containing %q", got, tt.wantErr)
				}
			} else if err != nil {
				t.Fatalf("unexpected error: %v", err)
			}
		})
	}
}

func TestSessionMetadataValidation(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name    string
		params  SessionMetadataParams
		wantErr string
	}{
		{
			name:    "missing session_id",
			params:  SessionMetadataParams{Key: "work_type", Value: "FEAT"},
			wantErr: "session_id is required",
		},
		{
			name:    "missing key",
			params:  SessionMetadataParams{SessionID: "ses-123", Value: "FEAT"},
			wantErr: "key is required",
		},
		{
			name:    "missing value",
			params:  SessionMetadataParams{SessionID: "ses-123", Key: "work_type"},
			wantErr: "value is required",
		},
		{
			name:   "valid params",
			params: SessionMetadataParams{SessionID: "ses-123", Key: "work_type", Value: "FEAT"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			err := tt.params.Validate()
			if tt.wantErr != "" {
				if err == nil {
					t.Fatalf("expected error containing %q, got nil", tt.wantErr)
				}
				if got := err.Error(); !contains(got, tt.wantErr) {
					t.Errorf("error = %q, want containing %q", got, tt.wantErr)
				}
			} else if err != nil {
				t.Fatalf("unexpected error: %v", err)
			}
		})
	}
}

func TestRecordPhaseTransition(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name   string
		params PhaseTransitionParams
	}{
		{
			name:   "entered",
			params: PhaseTransitionParams{SessionID: "ses-test-001", Phase: "PF1-INIT", Status: "entered"},
		},
		{
			name:   "completed",
			params: PhaseTransitionParams{SessionID: "ses-test-002", Phase: "PF3-CLASSIFY", Status: "completed"},
		},
		{
			name:   "skipped",
			params: PhaseTransitionParams{SessionID: "ses-test-003", Phase: "PF4-EXECUTE", Status: "skipped"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			tw, dir := newTestWriter(t)

			if err := tw.RecordPhaseTransition(tt.params); err != nil {
				t.Fatalf("RecordPhaseTransition: %v", err)
			}

			events := readEvents(t, dir)
			if len(events) != 1 {
				t.Fatalf("expected 1 event, got %d", len(events))
			}

			evt := events[0]
			assertField(t, evt, "event", "phase_transition")
			assertField(t, evt, "session_id", tt.params.SessionID)
			assertField(t, evt, "phase", tt.params.Phase)
			assertField(t, evt, "status", tt.params.Status)
			assertHasField(t, evt, "timestamp")
		})
	}
}

func TestRecordPhaseTransitionAllPhases(t *testing.T) {
	t.Parallel()

	phases := []string{
		"PF1-INIT", "PF2-CONTEXT", "PF3-CLASSIFY", "PF4-EXECUTE",
		"PF5-VERIFY", "PF6-COMPLETE", "PF7-END",
	}

	for _, phase := range phases {
		t.Run(phase, func(t *testing.T) {
			t.Parallel()
			tw, dir := newTestWriter(t)

			err := tw.RecordPhaseTransition(PhaseTransitionParams{
				SessionID: "ses-all-phases",
				Phase:     phase,
				Status:    "entered",
			})
			if err != nil {
				t.Fatalf("RecordPhaseTransition(%s): %v", phase, err)
			}

			events := readEvents(t, dir)
			if len(events) != 1 {
				t.Fatalf("expected 1 event, got %d", len(events))
			}
			assertField(t, events[0], "phase", phase)
		})
	}
}

func TestRecordPhaseTransitionInvalidParams(t *testing.T) {
	t.Parallel()

	tw, _ := newTestWriter(t)
	err := tw.RecordPhaseTransition(PhaseTransitionParams{})
	if err == nil {
		t.Fatal("expected error for empty params")
	}
}

func TestRecordStageTransitionWithoutVerdict(t *testing.T) {
	t.Parallel()

	tw, dir := newTestWriter(t)

	err := tw.RecordStageTransition(StageTransitionParams{
		SessionID: "ses-stage-001",
		Stage:     "WS-DEV",
		Status:    "in_progress",
		Iteration: 1,
	})
	if err != nil {
		t.Fatalf("RecordStageTransition: %v", err)
	}

	events := readEvents(t, dir)
	if len(events) != 1 {
		t.Fatalf("expected 1 event, got %d", len(events))
	}

	evt := events[0]
	assertField(t, evt, "event", "stage_transition")
	assertField(t, evt, "session_id", "ses-stage-001")
	assertField(t, evt, "stage", "WS-DEV")
	assertField(t, evt, "status", "in_progress")
	assertHasField(t, evt, "timestamp")

	// Iteration should be a number (float64 from JSON unmarshaling).
	if iter, ok := evt["iteration"].(float64); !ok || iter != 1 {
		t.Errorf("iteration = %v, want 1", evt["iteration"])
	}

	// Verdict must be absent (not null, not empty string).
	if _, exists := evt["verdict"]; exists {
		t.Errorf("verdict field should be absent when not provided, got %v", evt["verdict"])
	}
}

func TestRecordStageTransitionWithVerdict(t *testing.T) {
	t.Parallel()

	tw, dir := newTestWriter(t)

	err := tw.RecordStageTransition(StageTransitionParams{
		SessionID: "ses-stage-002",
		Stage:     "WS-REV",
		Status:    "complete",
		Iteration: 2,
		Verdict:   "approved",
	})
	if err != nil {
		t.Fatalf("RecordStageTransition: %v", err)
	}

	events := readEvents(t, dir)
	if len(events) != 1 {
		t.Fatalf("expected 1 event, got %d", len(events))
	}

	evt := events[0]
	assertField(t, evt, "event", "stage_transition")
	assertField(t, evt, "verdict", "approved")

	if iter, ok := evt["iteration"].(float64); !ok || iter != 2 {
		t.Errorf("iteration = %v, want 2", evt["iteration"])
	}
}

func TestRecordStageTransitionAllStages(t *testing.T) {
	t.Parallel()

	stages := []string{"WS-DEV", "WS-PLAN", "WS-DOCS", "WS-TEST", "WS-REV", "WS-QA"}

	for _, stage := range stages {
		t.Run(stage, func(t *testing.T) {
			t.Parallel()
			tw, dir := newTestWriter(t)

			err := tw.RecordStageTransition(StageTransitionParams{
				SessionID: "ses-all-stages",
				Stage:     stage,
				Status:    "pending",
				Iteration: 1,
			})
			if err != nil {
				t.Fatalf("RecordStageTransition(%s): %v", stage, err)
			}

			events := readEvents(t, dir)
			if len(events) != 1 {
				t.Fatalf("expected 1 event, got %d", len(events))
			}
			assertField(t, events[0], "stage", stage)
		})
	}
}

func TestRecordStageTransitionAllVerdicts(t *testing.T) {
	t.Parallel()

	verdicts := []string{"pass", "fail", "approved", "changes_requested"}

	for _, verdict := range verdicts {
		t.Run(verdict, func(t *testing.T) {
			t.Parallel()
			tw, dir := newTestWriter(t)

			err := tw.RecordStageTransition(StageTransitionParams{
				SessionID: "ses-verdicts",
				Stage:     "WS-REV",
				Status:    "complete",
				Iteration: 1,
				Verdict:   verdict,
			})
			if err != nil {
				t.Fatalf("RecordStageTransition(verdict=%s): %v", verdict, err)
			}

			events := readEvents(t, dir)
			assertField(t, events[0], "verdict", verdict)
		})
	}
}

func TestRegisterSession(t *testing.T) {
	t.Parallel()

	tw, dir := newTestWriter(t)

	err := tw.RegisterSession(SessionRegisterParams{
		SessionID: "ses-register-001",
		Mode:      "interactive",
	})
	if err != nil {
		t.Fatalf("RegisterSession: %v", err)
	}

	events := readEvents(t, dir)
	if len(events) != 2 {
		t.Fatalf("expected 2 events, got %d", len(events))
	}

	// Event 1: tracking_level
	evt1 := events[0]
	assertField(t, evt1, "event", "session_register")
	assertField(t, evt1, "session_id", "ses-register-001")
	assertField(t, evt1, "tracking_level", "pending")
	assertHasField(t, evt1, "timestamp")

	// Event 2: interaction_mode
	evt2 := events[1]
	assertField(t, evt2, "event", "session_register")
	assertField(t, evt2, "session_id", "ses-register-001")
	assertField(t, evt2, "interaction_mode", "interactive")
	assertHasField(t, evt2, "timestamp")
}

func TestRegisterSessionAutorun(t *testing.T) {
	t.Parallel()

	tw, dir := newTestWriter(t)

	err := tw.RegisterSession(SessionRegisterParams{
		SessionID: "ses-register-002",
		Mode:      "autorun",
	})
	if err != nil {
		t.Fatalf("RegisterSession: %v", err)
	}

	events := readEvents(t, dir)
	if len(events) != 2 {
		t.Fatalf("expected 2 events, got %d", len(events))
	}

	assertField(t, events[1], "interaction_mode", "autorun")
}

func TestRegisterSessionInvalidParams(t *testing.T) {
	t.Parallel()

	tw, _ := newTestWriter(t)
	err := tw.RegisterSession(SessionRegisterParams{})
	if err == nil {
		t.Fatal("expected error for empty params")
	}
}

func TestRecordTaskUpdate(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name       string
		taskID     string
		taskStatus string
	}{
		{name: "completed", taskID: "PF3-TSK-01", taskStatus: "completed"},
		{name: "in_progress", taskID: "PF4-TSK-05", taskStatus: "in_progress"},
		{name: "skipped", taskID: "PF1-TSK-02", taskStatus: "skipped"},
		{name: "blocked", taskID: "PF7-TSK-03", taskStatus: "blocked"},
		{name: "pending", taskID: "PF2-TSK-04", taskStatus: "pending"},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			tw, dir := newTestWriter(t)

			err := tw.RecordTaskUpdate(TaskUpdateParams{
				SessionID:  "ses-task-001",
				TaskID:     tt.taskID,
				TaskStatus: tt.taskStatus,
			})
			if err != nil {
				t.Fatalf("RecordTaskUpdate: %v", err)
			}

			events := readEvents(t, dir)
			if len(events) != 1 {
				t.Fatalf("expected 1 event, got %d", len(events))
			}

			evt := events[0]
			assertField(t, evt, "event", "pathflow_task_update")
			assertField(t, evt, "session_id", "ses-task-001")
			assertField(t, evt, "task_id", tt.taskID)
			assertField(t, evt, "task_status", tt.taskStatus)
			assertHasField(t, evt, "timestamp")
		})
	}
}

func TestRecordTaskUpdateInvalidParams(t *testing.T) {
	t.Parallel()

	tw, _ := newTestWriter(t)
	err := tw.RecordTaskUpdate(TaskUpdateParams{})
	if err == nil {
		t.Fatal("expected error for empty params")
	}
}

func TestRecordSessionMetadata(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name  string
		key   string
		value string
	}{
		{name: "work_type", key: "work_type", value: "FEAT"},
		{name: "area_type", key: "area_type", value: "INF"},
		{name: "tracking_level", key: "tracking_level", value: "tracked"},
		{name: "branch", key: "branch", value: "feat/my-feature"},
		{name: "custom_key", key: "custom_key", value: "custom_value"},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			tw, dir := newTestWriter(t)

			err := tw.RecordSessionMetadata(SessionMetadataParams{
				SessionID: "ses-meta-001",
				Key:       tt.key,
				Value:     tt.value,
			})
			if err != nil {
				t.Fatalf("RecordSessionMetadata: %v", err)
			}

			events := readEvents(t, dir)
			if len(events) != 1 {
				t.Fatalf("expected 1 event, got %d", len(events))
			}

			evt := events[0]
			assertField(t, evt, "event", "session_metadata")
			assertField(t, evt, "session_id", "ses-meta-001")
			assertField(t, evt, "key", tt.key)
			assertField(t, evt, "value", tt.value)
			assertHasField(t, evt, "timestamp")
		})
	}
}

func TestRecordSessionMetadataInvalidParams(t *testing.T) {
	t.Parallel()

	tw, _ := newTestWriter(t)
	err := tw.RecordSessionMetadata(SessionMetadataParams{})
	if err == nil {
		t.Fatal("expected error for empty params")
	}
}

func TestNewTransitionWriterCreatesDirectory(t *testing.T) {
	t.Parallel()

	dir := filepath.Join(t.TempDir(), "nested", "logs")
	tw, err := NewTransitionWriter(context.TODO(), dir)
	if err != nil {
		t.Fatalf("NewTransitionWriter: %v", err)
	}

	// Verify the directory was created.
	info, err := os.Stat(dir)
	if err != nil {
		t.Fatalf("stat dir: %v", err)
	}
	if !info.IsDir() {
		t.Errorf("expected %s to be a directory", dir)
	}

	// Verify writing works.
	err = tw.RecordSessionMetadata(SessionMetadataParams{
		SessionID: "ses-dir-test",
		Key:       "test",
		Value:     "ok",
	})
	if err != nil {
		t.Fatalf("RecordSessionMetadata after dir creation: %v", err)
	}
}

func TestMultipleEventsAppend(t *testing.T) {
	t.Parallel()

	tw, dir := newTestWriter(t)

	// Write 3 different event types to the same file.
	if err := tw.RecordPhaseTransition(PhaseTransitionParams{
		SessionID: "ses-multi", Phase: "PF1-INIT", Status: "entered",
	}); err != nil {
		t.Fatalf("phase transition: %v", err)
	}

	if err := tw.RecordStageTransition(StageTransitionParams{
		SessionID: "ses-multi", Stage: "WS-DEV", Status: "in_progress", Iteration: 1,
	}); err != nil {
		t.Fatalf("stage transition: %v", err)
	}

	if err := tw.RecordSessionMetadata(SessionMetadataParams{
		SessionID: "ses-multi", Key: "work_type", Value: "FEAT",
	}); err != nil {
		t.Fatalf("session metadata: %v", err)
	}

	events := readEvents(t, dir)
	if len(events) != 3 {
		t.Fatalf("expected 3 events, got %d", len(events))
	}

	assertField(t, events[0], "event", "phase_transition")
	assertField(t, events[1], "event", "stage_transition")
	assertField(t, events[2], "event", "session_metadata")
}

func TestCanonicalSchemaFieldNames(t *testing.T) {
	t.Parallel()

	tw, dir := newTestWriter(t)

	if err := tw.RecordPhaseTransition(PhaseTransitionParams{
		SessionID: "ses-schema", Phase: "PF1-INIT", Status: "entered",
	}); err != nil {
		t.Fatalf("RecordPhaseTransition: %v", err)
	}

	events := readEvents(t, dir)
	evt := events[0]

	// Canonical Go schema uses "event" (not "type") and "timestamp" (not "ts").
	if _, ok := evt["event"]; !ok {
		t.Error("expected 'event' field in output")
	}
	if _, ok := evt["timestamp"]; !ok {
		t.Error("expected 'timestamp' field in output")
	}

	// Shell schema fields must NOT be present.
	if _, ok := evt["type"]; ok {
		t.Error("shell-schema 'type' field should not be present in Go output")
	}
	if _, ok := evt["ts"]; ok {
		t.Error("shell-schema 'ts' field should not be present in Go output")
	}
}

// assertField checks that a map has a string field with the expected value.
func assertField(t *testing.T, m map[string]any, key, want string) {
	t.Helper()
	got, ok := m[key].(string)
	if !ok {
		t.Errorf("field %q: not a string (value: %v)", key, m[key])
		return
	}
	if got != want {
		t.Errorf("field %q = %q, want %q", key, got, want)
	}
}

// assertHasField checks that a map has a non-empty field.
func assertHasField(t *testing.T, m map[string]any, key string) {
	t.Helper()
	v, ok := m[key]
	if !ok {
		t.Errorf("field %q: missing", key)
		return
	}
	if s, isStr := v.(string); isStr && s == "" {
		t.Errorf("field %q: empty string", key)
	}
}

// contains checks if s contains substr.
func contains(s, substr string) bool {
	return len(s) >= len(substr) && searchString(s, substr)
}

func searchString(s, substr string) bool {
	for i := 0; i <= len(s)-len(substr); i++ {
		if s[i:i+len(substr)] == substr {
			return true
		}
	}
	return false
}
