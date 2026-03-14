package sentinel

import (
	"encoding/json"
	"fmt"
	"io"
	"log/slog"
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

// pfTaskNumRe extracts the task number from a PF task ID.
var pfTaskNumRe = regexp.MustCompile(`^PF\d+-TSK-(\d+)$`)

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
// extracts PF{N}-TSK-{NN} from the subject, validates cross-phase dependencies
// (cumulative: ALL prior phases must have sentinels), and registers the task.
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
		return &Verdict{Allow: true}
	}

	taskID := matches[1]

	phaseNum, err := extractPhaseNum(taskID)
	if err != nil {
		return &Verdict{Allow: true}
	}

	// CUMULATIVE cross-phase gate: require ALL prior phase sentinels.
	if phaseNum > 1 {
		ok, missing := VerifyCumulativePhaseSentinels(sentinelDir, phaseNum-1)
		if !ok {
			phaseID := fmt.Sprintf("PF%d", phaseNum)
			return &Verdict{
				Allow: false,
				Reason: fmt.Sprintf(
					"CHECKPOINT BLOCK: Cannot register task '%s' for phase %s.\n"+
						"Phase sentinel '%s' does not exist.\n"+
						"All prior phase sentinels (pf-1 through pf-%d) must exist before %s tasks can be created.\n"+
						"Action: Complete all prior phases first, then retry.",
					taskID, phaseID, missing, phaseNum-1, phaseID,
				),
			}
		}
	}

	// Read checkpoint, register task, write back.
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

	if _, exists := phase.Registered[taskID]; exists {
		return &Verdict{Allow: true}
	}

	phase.Registered[taskID] = time.Now().UTC().Format(time.RFC3339)

	if err := writeCheckpoint(checkpointFile, checkpoint); err != nil {
		return &Verdict{Allow: true}
	}

	return &Verdict{Allow: true}
}

// CompleteCheckpointTask parses TaskCompleted stdin, extracts PF{N}-TSK-{NN},
// validates cumulative cross-phase dependencies, marks the task complete,
// checks PF4 stage sentinel gates, and creates a phase sentinel if all tasks
// in the phase are done.
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

	matches := pfTaskRe.FindStringSubmatch(input.TaskSubject)
	if matches == nil {
		return &Verdict{Allow: true}
	}

	taskID := matches[1]

	phaseNum, err := extractPhaseNum(taskID)
	if err != nil {
		return &Verdict{Allow: true}
	}

	// CUMULATIVE cross-phase gate: require ALL prior phase sentinels.
	if phaseNum > 1 {
		ok, missing := VerifyCumulativePhaseSentinels(sentinelDir, phaseNum-1)
		if !ok {
			phaseID := fmt.Sprintf("PF%d", phaseNum)
			return &Verdict{
				Allow: false,
				Reason: fmt.Sprintf(
					"CHECKPOINT BLOCK: Cannot complete task '%s' for phase %s.\n"+
						"Phase sentinel '%s' does not exist.\n"+
						"All prior phase sentinels (pf-1 through pf-%d) must exist before %s tasks can finish.\n"+
						"Action: Complete all prior phases first, then retry.",
					taskID, phaseID, missing, phaseNum-1, phaseID,
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

	if phase.SentinelCreated {
		return &Verdict{Allow: true}
	}

	if _, exists := phase.Completed[taskID]; !exists {
		phase.Completed[taskID] = time.Now().UTC().Format(time.RFC3339)

		if err := writeCheckpoint(checkpointFile, checkpoint); err != nil {
			return &Verdict{Allow: true}
		}
	}

	// PF4 stage task completion gate: verify cumulative stage sentinels.
	if phaseNum == 4 {
		if v := checkPF4StageSentinels(taskID, sentinelDir, checkpoint, phase, checkpointFile); v != nil {
			return v
		}
	}

	// Check if phase is now complete.
	if isPhaseComplete(phase, checkpoint.Context, sentinelDir, phaseNum) {
		sentinelName := fmt.Sprintf("pf-%d", phaseNum)
		if err := createSentinelFile(sentinelDir, sentinelName); err == nil {
			phase.SentinelCreated = true
			_ = writeCheckpoint(checkpointFile, checkpoint)
		}
	}

	return &Verdict{Allow: true}
}

// checkPF4StageSentinels verifies cumulative stage sentinels for PF4 stage tasks
// (PF4-TSK-05 through PF4-TSK-07). If the corresponding pipeline stage sentinel
// is missing, the completion is undone and a blocking verdict is returned.
func checkPF4StageSentinels(taskID, sentinelDir string, checkpoint *CheckpointData, phase *CheckpointPhase, checkpointFile string) *Verdict {
	taskNum, err := extractTaskNum(taskID)
	if err != nil {
		return nil
	}

	if taskNum < 5 {
		return nil
	}

	pipelineIndex := taskNum - 5

	configDir := deriveConfigDir(sentinelDir)
	pipelines, err := LoadPipelines(configDir)
	if err != nil {
		slog.Warn("checkpoint: cannot load pipelines for PF4 stage check", "error", err)
		return nil
	}

	workType := ""
	if checkpoint.Context != nil {
		workType = checkpoint.Context.WorkType
	}
	if workType == "" {
		return nil
	}

	pipeline, exists := pipelines[workType]
	if !exists {
		return nil
	}

	if pipelineIndex >= len(pipeline) {
		return nil
	}

	ok, missing := VerifyCumulativeStageSentinels(sentinelDir, pipeline, pipelineIndex)
	if !ok {
		delete(phase.Completed, taskID)
		_ = writeCheckpoint(checkpointFile, checkpoint)

		return &Verdict{
			Allow: false,
			Reason: fmt.Sprintf(
				"CHECKPOINT BLOCK: Cannot complete %s. Stage sentinel '%s' missing.\n"+
					"All prior pipeline stages must complete before this task can be marked done.\n"+
					"Pipeline for %s: %v",
				taskID, missing, workType, pipeline,
			),
		}
	}

	return nil
}

// extractTaskNum extracts the task number from a PF task ID.
// "PF4-TSK-06" -> 6
func extractTaskNum(taskID string) (int, error) {
	matches := pfTaskNumRe.FindStringSubmatch(taskID)
	if matches == nil {
		return 0, fmt.Errorf("invalid PF task ID: %s", taskID)
	}
	return strconv.Atoi(matches[1])
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
// For PF4, additionally verifies ALL pipeline stage sentinels exist.
func isPhaseComplete(phase *CheckpointPhase, ctx *CheckpointContext, sentinelDir string, phaseNum int) bool {
	if len(phase.Expected) == 0 {
		return false
	}

	for _, taskID := range phase.Expected {
		if _, ok := phase.Completed[taskID]; ok {
			continue
		}
		if _, ok := phase.Skipped[taskID]; ok {
			continue
		}
		if autoSkipped(taskID, phase.Conditions, ctx) {
			continue
		}
		return false
	}

	// For PF4: additionally require ALL pipeline stage sentinels.
	if phaseNum == 4 && sentinelDir != "" && ctx != nil && ctx.WorkType != "" {
		configDir := deriveConfigDir(sentinelDir)
		pipelines, err := LoadPipelines(configDir)
		if err != nil {
			slog.Warn("checkpoint: cannot load pipelines for PF4 completion check", "error", err)
			return true
		}

		pipeline, exists := pipelines[ctx.WorkType]
		if !exists {
			return true
		}

		ok, _ := VerifyCumulativeStageSentinels(sentinelDir, pipeline, len(pipeline)-1)
		if !ok {
			return false
		}
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
		return ctx.Origin == "planned"
	case "if_pipeline_includes_qa":
		wt := strings.ToUpper(ctx.WorkType)
		return wt == "DOCS" || wt == "PLAN" || wt == "SPKE"
	}

	return false
}

// SetCheckpointContext updates the checkpoint context with a key-value pair.
func SetCheckpointContext(checkpointFile, key, value string) error {
	checkpoint, err := readCheckpoint(checkpointFile)
	if err != nil {
		return fmt.Errorf("set checkpoint context: %w", err)
	}

	if checkpoint.Context == nil {
		checkpoint.Context = &CheckpointContext{}
	}

	switch key {
	case "work_type":
		checkpoint.Context.WorkType = value
	case "origin":
		checkpoint.Context.Origin = value
	default:
		return fmt.Errorf("set checkpoint context: unknown key %q", key)
	}

	return writeCheckpoint(checkpointFile, checkpoint)
}

// readCheckpoint reads and parses the checkpoint JSON file.
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

		if strings.HasPrefix(key, "PF") {
			var phase CheckpointPhase
			if err := json.Unmarshal(val, &phase); err == nil {
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

// writeCheckpoint writes the checkpoint data to a JSON file atomically.
func writeCheckpoint(path string, data *CheckpointData) error {
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		return fmt.Errorf("create checkpoint dir: %w", err)
	}

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
