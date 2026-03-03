package pathflow

import (
	"context"
	"fmt"
	"regexp"

	"github.com/codeflow/codeflow-cli/internal/ledger"
)

// Valid values for pathflow transition commands.
var (
	validPhases = map[string]bool{
		"PF1-INIT": true, "PF2-CONTEXT": true, "PF3-CLASSIFY": true,
		"PF4-EXECUTE": true, "PF5-VERIFY": true, "PF6-COMPLETE": true,
		"PF7-END": true,
	}

	validPhaseStatuses = map[string]bool{
		"entered": true, "completed": true, "skipped": true,
	}

	validStages = map[string]bool{
		"WS-DEV": true, "WS-PLAN": true, "WS-DOCS": true,
		"WS-TEST": true, "WS-REV": true, "WS-QA": true,
	}

	validStageStatuses = map[string]bool{
		"pending": true, "in_progress": true, "complete": true, "failed": true,
	}

	validStageVerdicts = map[string]bool{
		"pass": true, "fail": true, "approved": true, "changes_requested": true,
	}

	validInteractionModes = map[string]bool{
		"interactive": true, "autorun": true,
	}

	validTaskStatuses = map[string]bool{
		"pending": true, "in_progress": true, "completed": true,
		"skipped": true, "blocked": true,
	}

	// pathflowTaskIDRe matches task IDs in the form PF{1-7}-TSK-{00-99}.
	pathflowTaskIDRe = regexp.MustCompile(`^PF[1-7]-TSK-[0-9]{2}$`)
)

// PhaseTransitionParams holds the input for a phase transition event.
type PhaseTransitionParams struct {
	SessionID string
	Phase     string
	Status    string
}

// Validate checks that all required fields are present and valid.
func (p PhaseTransitionParams) Validate() error {
	if p.SessionID == "" {
		return fmt.Errorf("pathflow: session_id is required")
	}
	if p.Phase == "" {
		return fmt.Errorf("pathflow: phase is required")
	}
	if !validPhases[p.Phase] {
		return fmt.Errorf("pathflow: invalid phase %q", p.Phase)
	}
	if p.Status == "" {
		return fmt.Errorf("pathflow: status is required")
	}
	if !validPhaseStatuses[p.Status] {
		return fmt.Errorf("pathflow: invalid phase status %q", p.Status)
	}
	return nil
}

// StageTransitionParams holds the input for a stage transition event.
type StageTransitionParams struct {
	SessionID string
	Stage     string
	Status    string
	Iteration int
	Verdict   string // Empty string means omit from output.
}

// Validate checks that all required fields are present and valid.
func (p StageTransitionParams) Validate() error {
	if p.SessionID == "" {
		return fmt.Errorf("pathflow: session_id is required")
	}
	if p.Stage == "" {
		return fmt.Errorf("pathflow: stage is required")
	}
	if !validStages[p.Stage] {
		return fmt.Errorf("pathflow: invalid stage %q", p.Stage)
	}
	if p.Status == "" {
		return fmt.Errorf("pathflow: status is required")
	}
	if !validStageStatuses[p.Status] {
		return fmt.Errorf("pathflow: invalid stage status %q", p.Status)
	}
	if p.Verdict != "" && !validStageVerdicts[p.Verdict] {
		return fmt.Errorf("pathflow: invalid verdict %q", p.Verdict)
	}
	if p.Iteration < 1 {
		return fmt.Errorf("pathflow: iteration must be >= 1, got %d", p.Iteration)
	}
	return nil
}

// SessionRegisterParams holds the input for a session register event.
type SessionRegisterParams struct {
	SessionID string
	Mode      string // "interactive" or "autorun"
}

// Validate checks that all required fields are present and valid.
func (p SessionRegisterParams) Validate() error {
	if p.SessionID == "" {
		return fmt.Errorf("pathflow: session_id is required")
	}
	if p.Mode == "" {
		return fmt.Errorf("pathflow: mode is required")
	}
	if !validInteractionModes[p.Mode] {
		return fmt.Errorf("pathflow: invalid mode %q (must be interactive or autorun)", p.Mode)
	}
	return nil
}

// TaskUpdateParams holds the input for a pathflow task update event.
type TaskUpdateParams struct {
	SessionID  string
	TaskID     string
	TaskStatus string
}

// Validate checks that all required fields are present and valid.
func (p TaskUpdateParams) Validate() error {
	if p.SessionID == "" {
		return fmt.Errorf("pathflow: session_id is required")
	}
	if p.TaskID == "" {
		return fmt.Errorf("pathflow: task_id is required")
	}
	if !pathflowTaskIDRe.MatchString(p.TaskID) {
		return fmt.Errorf("pathflow: invalid task_id format %q (expected PF[1-7]-TSK-[0-9]{2})", p.TaskID)
	}
	if p.TaskStatus == "" {
		return fmt.Errorf("pathflow: task_status is required")
	}
	if !validTaskStatuses[p.TaskStatus] {
		return fmt.Errorf("pathflow: invalid task_status %q", p.TaskStatus)
	}
	return nil
}

// SessionMetadataParams holds the input for a session metadata event.
type SessionMetadataParams struct {
	SessionID string
	Key       string
	Value     string
}

// Validate checks that all required fields are present and valid.
func (p SessionMetadataParams) Validate() error {
	if p.SessionID == "" {
		return fmt.Errorf("pathflow: session_id is required")
	}
	if p.Key == "" {
		return fmt.Errorf("pathflow: key is required")
	}
	if p.Value == "" {
		return fmt.Errorf("pathflow: value is required")
	}
	return nil
}

// TransitionWriter writes pathflow transition events to the JSONL ledger.
type TransitionWriter struct {
	writer *ledger.Writer
}

// NewTransitionWriter creates a TransitionWriter that writes to the given directory.
// The context parameter is reserved for future use (e.g., cancellation).
func NewTransitionWriter(_ context.Context, logsDir string) (*TransitionWriter, error) {
	w, err := ledger.NewWriter(logsDir)
	if err != nil {
		return nil, fmt.Errorf("pathflow: creating ledger writer: %w", err)
	}
	return &TransitionWriter{writer: w}, nil
}

// RecordPhaseTransition writes a phase_transition event.
func (tw *TransitionWriter) RecordPhaseTransition(p PhaseTransitionParams) error {
	if err := p.Validate(); err != nil {
		return err
	}

	event := ledger.Event{
		EventType: "phase_transition",
		SessionID: p.SessionID,
		Data: map[string]any{
			"phase":  p.Phase,
			"status": p.Status,
		},
	}

	return tw.writer.AppendEventToFile(ledger.FilePathflowEvents, event)
}

// RecordStageTransition writes a stage_transition event.
// When Verdict is empty, the verdict field is omitted from the output entirely.
func (tw *TransitionWriter) RecordStageTransition(p StageTransitionParams) error {
	if err := p.Validate(); err != nil {
		return err
	}

	data := map[string]any{
		"stage":     p.Stage,
		"status":    p.Status,
		"iteration": p.Iteration,
	}
	if p.Verdict != "" {
		data["verdict"] = p.Verdict
	}

	event := ledger.Event{
		EventType: "stage_transition",
		SessionID: p.SessionID,
		Data:      data,
	}

	return tw.writer.AppendEventToFile(ledger.FilePathflowEvents, event)
}

// RegisterSession writes two session_register events: one for tracking_level
// and one for interaction_mode. Returns the event ID concept (the first event's
// canonical identification is via session_id + timestamp).
func (tw *TransitionWriter) RegisterSession(p SessionRegisterParams) error {
	if err := p.Validate(); err != nil {
		return err
	}

	// Event 1: tracking_level = pending
	trackingEvent := ledger.Event{
		EventType: "session_register",
		SessionID: p.SessionID,
		Data: map[string]any{
			"tracking_level": "pending",
		},
	}
	if err := tw.writer.AppendEventToFile(ledger.FilePathflowEvents, trackingEvent); err != nil {
		return fmt.Errorf("pathflow: writing tracking_level event: %w", err)
	}

	// Event 2: interaction_mode
	modeEvent := ledger.Event{
		EventType: "session_register",
		SessionID: p.SessionID,
		Data: map[string]any{
			"interaction_mode": p.Mode,
		},
	}
	if err := tw.writer.AppendEventToFile(ledger.FilePathflowEvents, modeEvent); err != nil {
		return fmt.Errorf("pathflow: writing interaction_mode event: %w", err)
	}

	return nil
}

// RecordTaskUpdate writes a pathflow_task_update event.
func (tw *TransitionWriter) RecordTaskUpdate(p TaskUpdateParams) error {
	if err := p.Validate(); err != nil {
		return err
	}

	event := ledger.Event{
		EventType: "pathflow_task_update",
		SessionID: p.SessionID,
		Data: map[string]any{
			"task_id":     p.TaskID,
			"task_status": p.TaskStatus,
		},
	}

	return tw.writer.AppendEventToFile(ledger.FilePathflowEvents, event)
}

// RecordSessionMetadata writes a session_metadata event.
func (tw *TransitionWriter) RecordSessionMetadata(p SessionMetadataParams) error {
	if err := p.Validate(); err != nil {
		return err
	}

	event := ledger.Event{
		EventType: "session_metadata",
		SessionID: p.SessionID,
		Data: map[string]any{
			"key":   p.Key,
			"value": p.Value,
		},
	}

	return tw.writer.AppendEventToFile(ledger.FilePathflowEvents, event)
}
