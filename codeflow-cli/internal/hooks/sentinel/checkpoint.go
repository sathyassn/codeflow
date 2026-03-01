package sentinel

import (
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"regexp"
	"strconv"
	"strings"
	"time"
)

// pfTaskRe matches PF{N}-TSK-{NN} task identifiers.
var pfTaskRe = regexp.MustCompile(`(PF\d+-TSK-\d+)`)

// pfPhaseNumRe extracts the phase number from a PF task ID.
var pfPhaseNumRe = regexp.MustCompile(`^PF(\d+)-TSK-\d+$`)

// CheckpointPhase represents a single phase entry in the checkpoint JSON.
type CheckpointPhase struct {
	Expected        []string          `json:"expected"`
	Conditions      map[string]string `json:"conditions"`
	Registered      map[string]string `json:"registered"`
	Completed       map[string]string `json:"completed"`
	Skipped         map[string]string `json:"skipped"`
	SentinelCreated bool              `json:"sentinel_created"`
}

// CheckpointContext stores session context used for conditional task evaluation.
type CheckpointContext struct {
	Origin   string `json:"origin,omitempty"`
	WorkType string `json:"work_type,omitempty"`
}

// CheckpointData represents the full checkpoint JSON structure.
// Phase keys are "PF1", "PF2", etc. The "context" key stores session metadata.
type CheckpointData struct {
	Phases  map[string]*CheckpointPhase `json:"-"`
	Context *CheckpointContext          `json:"context,omitempty"`
}

// taskCreateInput represents the tool_input for TaskCreate calls.
type taskCreateInput struct {
	Subject string `json:"subject"`
}

// taskCompletedInput represents the JSON from TaskCompleted events.
type taskCompletedInput struct {
	TaskSubject string `json:"task_subject"`
}

// RegisterCheckpointTask parses PostToolUse stdin for TaskCreate calls,
// extracts PF{N}-TSK-{NN} from the subject, validates cross-phase dependencies,
// and registers the task in the checkpoint file.
//
// The sessionDir should be: {projectDir}/.state/session/{sessionID}/pathflow/
// The sentinelDir should be: {projectDir}/.state/sentinels/pathflow/{sessionID}/
func RegisterCheckpointTask(stdin io.Reader, sessionDir, sentinelDir string) *Verdict {
	data, err := io.ReadAll(stdin)
	if err != nil {
		return &Verdict{Allow: true}
	}

	if len(data) == 0 {
		return &Verdict{Allow: true}
	}

	var input postToolUseInput
	if err := json.Unmarshal(data, &input); err != nil {
		return &Verdict{Allow: true}
	}

	if input.ToolName != "TaskCreate" {
		return &Verdict{Allow: true}
	}

	if len(input.ToolInput) == 0 {
		return &Verdict{Allow: true}
	}

	var tc taskCreateInput
	if err := json.Unmarshal(input.ToolInput, &tc); err != nil {
		return &Verdict{Allow: true}
	}

	if tc.Subject == "" {
		return &Verdict{Allow: true}
	}

	// Extract PF task ID from subject.
	matches := pfTaskRe.FindStringSubmatch(tc.Subject)
	if matches == nil {
		// Not a PathFlow task -- ignore silently.
		return &Verdict{Allow: true}
	}

	taskID := matches[1]

	// Extract phase number.
	phaseNum, err := extractPhaseNum(taskID)
	if err != nil {
		return &Verdict{Allow: true}
	}

	// Cross-phase gate: if phase > 1, require previous phase sentinel.
	if phaseNum > 1 {
		prevSentinel := fmt.Sprintf("pf-%d", phaseNum-1)
		if !hasSentinelFile(sentinelDir, prevSentinel) {
			phaseID := fmt.Sprintf("PF%d", phaseNum)
			prevPhaseID := fmt.Sprintf("PF%d", phaseNum-1)
			return &Verdict{
				Allow: false,
				Reason: fmt.Sprintf(
					"CHECKPOINT BLOCK: Cannot register task '%s' for phase %s.\n"+
						"Phase %s is not yet complete -- its sentinel (pathflow-%s) does not exist.\n"+
						"All %s tasks must be registered and completed before %s tasks can be created.\n"+
						"Action: Complete all %s tasks first, then retry.",
					taskID, phaseID, prevPhaseID, prevSentinel, prevPhaseID, phaseID, prevPhaseID,
				),
			}
		}
	}

	// Read checkpoint, register task, write back.
	checkpointFile := filepath.Join(sessionDir, "pathflow-phase-tasks.json")
	checkpoint, err := readCheckpoint(checkpointFile)
	if err != nil {
		// Cannot read checkpoint -- allow through but do not register.
		return &Verdict{Allow: true}
	}

	phaseKey := fmt.Sprintf("PF%d", phaseNum)
	phase, ok := checkpoint.Phases[phaseKey]
	if !ok {
		// Phase not initialized in checkpoint -- allow through.
		return &Verdict{Allow: true}
	}

	// Check if already registered (idempotent).
	if _, exists := phase.Registered[taskID]; exists {
		return &Verdict{Allow: true}
	}

	// Register with timestamp.
	phase.Registered[taskID] = time.Now().UTC().Format(time.RFC3339)

	if err := writeCheckpoint(checkpointFile, checkpoint); err != nil {
		// Write error -- allow through.
		return &Verdict{Allow: true}
	}

	return &Verdict{Allow: true}
}

// CompleteCheckpointTask parses TaskCompleted stdin, extracts PF{N}-TSK-{NN}
// from task_subject, validates cross-phase dependencies, marks the task
// complete, and creates a phase sentinel if all tasks in the phase are done.
//
// The sessionDir should be: {projectDir}/.state/session/{sessionID}/pathflow/
// The sentinelDir should be: {projectDir}/.state/sentinels/pathflow/{sessionID}/
func CompleteCheckpointTask(stdin io.Reader, sessionDir, sentinelDir string) *Verdict {
	data, err := io.ReadAll(stdin)
	if err != nil {
		return &Verdict{Allow: true}
	}

	if len(data) == 0 {
		return &Verdict{Allow: true}
	}

	var input taskCompletedInput
	if err := json.Unmarshal(data, &input); err != nil {
		return &Verdict{Allow: true}
	}

	if input.TaskSubject == "" {
		return &Verdict{Allow: true}
	}

	// Extract PF task ID from subject.
	matches := pfTaskRe.FindStringSubmatch(input.TaskSubject)
	if matches == nil {
		return &Verdict{Allow: true}
	}

	taskID := matches[1]

	// Extract phase number.
	phaseNum, err := extractPhaseNum(taskID)
	if err != nil {
		return &Verdict{Allow: true}
	}

	// Cross-phase gate: if phase > 1, require previous phase sentinel.
	if phaseNum > 1 {
		prevSentinel := fmt.Sprintf("pf-%d", phaseNum-1)
		if !hasSentinelFile(sentinelDir, prevSentinel) {
			phaseID := fmt.Sprintf("PF%d", phaseNum)
			prevPhaseID := fmt.Sprintf("PF%d", phaseNum-1)
			return &Verdict{
				Allow: false,
				Reason: fmt.Sprintf(
					"CHECKPOINT BLOCK: Cannot complete task '%s' for phase %s.\n"+
						"Phase %s is not yet complete -- its sentinel (pathflow-%s) does not exist.\n"+
						"All %s tasks must be registered and completed before %s tasks can finish.\n"+
						"Action: Complete all %s tasks first, then retry.",
					taskID, phaseID, prevPhaseID, prevSentinel, prevPhaseID, phaseID, prevPhaseID,
				),
			}
		}
	}

	// Read checkpoint, mark complete, write back.
	checkpointFile := filepath.Join(sessionDir, "pathflow-phase-tasks.json")
	checkpoint, err := readCheckpoint(checkpointFile)
	if err != nil {
		return &Verdict{Allow: true}
	}

	phaseKey := fmt.Sprintf("PF%d", phaseNum)
	phase, ok := checkpoint.Phases[phaseKey]
	if !ok {
		return &Verdict{Allow: true}
	}

	// Check if sentinel already created (phase already done).
	if phase.SentinelCreated {
		return &Verdict{Allow: true}
	}

	// Mark completed with timestamp (idempotent).
	if _, exists := phase.Completed[taskID]; !exists {
		phase.Completed[taskID] = time.Now().UTC().Format(time.RFC3339)

		if err := writeCheckpoint(checkpointFile, checkpoint); err != nil {
			return &Verdict{Allow: true}
		}
	}

	// Check if phase is now complete.
	if isPhaseComplete(phase, checkpoint.Context) {
		sentinelName := fmt.Sprintf("pf-%d", phaseNum)
		if err := createSentinelFile(sentinelDir, sentinelName); err == nil {
			phase.SentinelCreated = true
			// Best-effort write of sentinel_created flag.
			_ = writeCheckpoint(checkpointFile, checkpoint)
		}
	}

	return &Verdict{Allow: true}
}

// extractPhaseNum extracts the phase number from a PF task ID.
// "PF3-TSK-01" -> 3
func extractPhaseNum(taskID string) (int, error) {
	matches := pfPhaseNumRe.FindStringSubmatch(taskID)
	if matches == nil {
		return 0, fmt.Errorf("invalid PF task ID: %s", taskID)
	}
	return strconv.Atoi(matches[1])
}

// isPhaseComplete checks whether all expected tasks in a phase are completed,
// skipped, or auto-skipped by condition.
func isPhaseComplete(phase *CheckpointPhase, ctx *CheckpointContext) bool {
	if len(phase.Expected) == 0 {
		return false
	}

	for _, taskID := range phase.Expected {
		// Check if completed.
		if _, ok := phase.Completed[taskID]; ok {
			continue
		}
		// Check if skipped.
		if _, ok := phase.Skipped[taskID]; ok {
			continue
		}
		// Check auto-skip via conditions.
		if autoSkipped(taskID, phase.Conditions, ctx) {
			continue
		}
		// Task is not done.
		return false
	}

	return true
}

// autoSkipped evaluates whether a task should be auto-skipped based on its
// condition and the current session context.
func autoSkipped(taskID string, conditions map[string]string, ctx *CheckpointContext) bool {
	cond, ok := conditions[taskID]
	if !ok || cond == "" {
		return false
	}

	if ctx == nil {
		return false
	}

	switch cond {
	case "adhoc_only":
		// Skip if origin is "planned".
		return ctx.Origin == "planned"

	case "if_pipeline_includes_qa":
		// Skip if work type is DOCS, PLAN, or SPKE (no QA stage).
		wt := strings.ToUpper(ctx.WorkType)
		return wt == "DOCS" || wt == "PLAN" || wt == "SPKE"
	}

	return false
}

// readCheckpoint reads and parses the checkpoint JSON file.
// Returns an empty checkpoint structure if the file does not exist.
func readCheckpoint(path string) (*CheckpointData, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		if os.IsNotExist(err) {
			return &CheckpointData{
				Phases: make(map[string]*CheckpointPhase),
			}, nil
		}
		return nil, fmt.Errorf("read checkpoint: %w", err)
	}

	return parseCheckpoint(data)
}

// parseCheckpoint parses checkpoint JSON bytes into a CheckpointData struct.
// The checkpoint JSON has dynamic keys (PF1, PF2, ..., context), so we
// unmarshal into a raw map first and then extract typed fields.
func parseCheckpoint(data []byte) (*CheckpointData, error) {
	var raw map[string]json.RawMessage
	if err := json.Unmarshal(data, &raw); err != nil {
		return nil, fmt.Errorf("parse checkpoint JSON: %w", err)
	}

	result := &CheckpointData{
		Phases: make(map[string]*CheckpointPhase),
	}

	for key, val := range raw {
		if key == "context" {
			var ctx CheckpointContext
			if err := json.Unmarshal(val, &ctx); err == nil {
				result.Context = &ctx
			}
			continue
		}

		// Phase keys start with "PF".
		if strings.HasPrefix(key, "PF") {
			var phase CheckpointPhase
			if err := json.Unmarshal(val, &phase); err == nil {
				// Ensure maps are initialized.
				if phase.Registered == nil {
					phase.Registered = make(map[string]string)
				}
				if phase.Completed == nil {
					phase.Completed = make(map[string]string)
				}
				if phase.Skipped == nil {
					phase.Skipped = make(map[string]string)
				}
				if phase.Conditions == nil {
					phase.Conditions = make(map[string]string)
				}
				result.Phases[key] = &phase
			}
		}
	}

	return result, nil
}

// writeCheckpoint writes the checkpoint data to a JSON file atomically
// (write to tmp file, then rename).
func writeCheckpoint(path string, data *CheckpointData) error {
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		return fmt.Errorf("create checkpoint dir: %w", err)
	}

	// Build the output map preserving the expected structure.
	out := make(map[string]any)
	for key, phase := range data.Phases {
		out[key] = phase
	}
	if data.Context != nil {
		out["context"] = data.Context
	}

	b, err := json.MarshalIndent(out, "", "  ")
	if err != nil {
		return fmt.Errorf("marshal checkpoint: %w", err)
	}

	// Atomic write: tmp + rename.
	tmpPath := path + ".tmp"
	if err := os.WriteFile(tmpPath, b, 0o644); err != nil {
		return fmt.Errorf("write checkpoint tmp: %w", err)
	}

	if err := os.Rename(tmpPath, path); err != nil {
		_ = os.Remove(tmpPath)
		return fmt.Errorf("rename checkpoint: %w", err)
	}

	return nil
}
