package pathflow

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"strconv"
	"strings"
	"syscall"
	"time"
)

// taskIDRe matches task IDs in the form PF{N}-TSK-{NN}.
var taskIDRe = regexp.MustCompile(`^(PF[0-9]+)-TSK-[0-9]+$`)

// PhaseCheckpoint represents the checkpoint state for a single phase.
type PhaseCheckpoint struct {
	Expected        []string          `json:"expected"`
	Conditions      map[string]string `json:"conditions"`
	Registered      map[string]string `json:"registered"`
	Completed       map[string]string `json:"completed"`
	Skipped         map[string]string `json:"skipped"`
	SentinelCreated bool              `json:"sentinel_created"`
}

// CheckpointFile represents the full checkpoint JSON file.
// It contains a "context" key for session metadata alongside PF1..PF7 phase keys.
type CheckpointFile struct {
	Context map[string]string           `json:"context,omitempty"`
	Phases  map[string]*PhaseCheckpoint `json:"-"`
}

// MarshalJSON produces a flat JSON object with "context" alongside phase keys.
func (cf *CheckpointFile) MarshalJSON() ([]byte, error) {
	m := make(map[string]any, len(cf.Phases)+1)
	if len(cf.Context) > 0 {
		m["context"] = cf.Context
	}
	for k, v := range cf.Phases {
		m[k] = v
	}
	return json.Marshal(m)
}

// UnmarshalJSON reads a flat JSON object, separating "context" from phase keys.
func (cf *CheckpointFile) UnmarshalJSON(data []byte) error {
	var raw map[string]json.RawMessage
	if err := json.Unmarshal(data, &raw); err != nil {
		return err
	}

	cf.Phases = make(map[string]*PhaseCheckpoint, len(raw))

	for k, v := range raw {
		if k == "context" {
			cf.Context = make(map[string]string)
			if err := json.Unmarshal(v, &cf.Context); err != nil {
				return fmt.Errorf("unmarshaling context: %w", err)
			}
			continue
		}
		var pc PhaseCheckpoint
		if err := json.Unmarshal(v, &pc); err != nil {
			return fmt.Errorf("unmarshaling phase %s: %w", k, err)
		}
		cf.Phases[k] = &pc
	}
	return nil
}

// Checkpoint manages PathFlow phase task tracking with file-locked access.
type Checkpoint struct {
	// Now returns the current time. Override in tests for deterministic output.
	Now func() time.Time
}

// NewCheckpoint creates a Checkpoint with default time source.
func NewCheckpoint() *Checkpoint {
	return &Checkpoint{
		Now: func() time.Time { return time.Now().UTC() },
	}
}

// ErrCrossPhaseBlock is returned when a task registration is blocked because
// the previous phase sentinel does not exist.
var ErrCrossPhaseBlock = fmt.Errorf("cross-phase registration blocked")

// InitAllPhases reads pathflow-config.json and initializes the checkpoint file
// with all 7 phases. Each phase gets its expected tasks and conditions from the
// config. Idempotent: existing phases are not overwritten.
func (c *Checkpoint) InitAllPhases(checkpointPath, configPath string) error {
	configData, err := os.ReadFile(configPath)
	if err != nil {
		return fmt.Errorf("checkpoint: reading config: %w", err)
	}

	var config struct {
		Phases map[string]struct {
			RequiredTasks []string `json:"required_tasks"`
			Tasks         []struct {
				ID        string `json:"id"`
				Condition string `json:"condition,omitempty"`
			} `json:"tasks"`
		} `json:"phases"`
	}
	if err := json.Unmarshal(configData, &config); err != nil {
		return fmt.Errorf("checkpoint: parsing config: %w", err)
	}

	return c.withLock(checkpointPath, func(cf *CheckpointFile) error {
		for configKey, phase := range config.Phases {
			// Extract phase prefix: PF1-INIT -> PF1
			phaseID := strings.SplitN(configKey, "-", 2)[0]

			// Skip if already initialized (idempotent).
			if _, exists := cf.Phases[phaseID]; exists {
				continue
			}

			conditions := make(map[string]string)
			for _, t := range phase.Tasks {
				if t.Condition != "" {
					conditions[t.ID] = t.Condition
				}
			}

			cf.Phases[phaseID] = &PhaseCheckpoint{
				Expected:   phase.RequiredTasks,
				Conditions: conditions,
				Registered: make(map[string]string),
				Completed:  make(map[string]string),
				Skipped:    make(map[string]string),
			}
		}
		return nil
	})
}

// RegisterTask registers a task in the checkpoint. It extracts the phase from
// the task ID (e.g., PF1-TSK-01 -> PF1) and verifies that the previous phase
// sentinel exists (cross-phase gate). Returns ErrCrossPhaseBlock if the gate
// check fails. Idempotent: registering an already-registered task is a no-op.
func (c *Checkpoint) RegisterTask(checkpointPath, sentinelDir, taskID string) error {
	phaseID, err := extractPhase(taskID)
	if err != nil {
		return err
	}

	// Cross-phase gate: PF{N} requires PF{N-1} sentinel for N > 1.
	if err := c.checkCrossPhaseGate(sentinelDir, phaseID); err != nil {
		return err
	}

	return c.withLock(checkpointPath, func(cf *CheckpointFile) error {
		pc, ok := cf.Phases[phaseID]
		if !ok {
			return fmt.Errorf("checkpoint: phase %s not initialized", phaseID)
		}

		// Idempotent check.
		if _, already := pc.Registered[taskID]; already {
			return nil
		}

		pc.Registered[taskID] = c.timestamp()
		return nil
	})
}

// CompleteTask marks a task as completed and checks if the phase is now
// complete. If all expected tasks are done, it creates the phase sentinel
// automatically. Idempotent: completing an already-completed task is a no-op
// (but still checks phase completion).
func (c *Checkpoint) CompleteTask(checkpointPath, sentinelDir, taskID string) error {
	phaseID, err := extractPhase(taskID)
	if err != nil {
		return err
	}

	return c.withLock(checkpointPath, func(cf *CheckpointFile) error {
		pc, ok := cf.Phases[phaseID]
		if !ok {
			return fmt.Errorf("checkpoint: phase %s not initialized", phaseID)
		}

		// If sentinel already created, nothing to do.
		if pc.SentinelCreated {
			return nil
		}

		// Mark completed (idempotent -- overwrites with same timestamp concept).
		if _, already := pc.Completed[taskID]; !already {
			pc.Completed[taskID] = c.timestamp()
		}

		// Check phase completion and create sentinel if done.
		if IsPhaseComplete(pc, cf.Context) {
			sentinelName := phaseSentinelName(phaseID)
			if err := Create(sentinelDir, sentinelName); err != nil {
				return fmt.Errorf("checkpoint: creating sentinel: %w", err)
			}
			pc.SentinelCreated = true
		}

		return nil
	})
}

// SkipTask marks a task as skipped and checks if the phase is now complete.
// Idempotent: skipping an already-skipped task is a no-op.
func (c *Checkpoint) SkipTask(checkpointPath, sentinelDir, taskID string) error {
	phaseID, err := extractPhase(taskID)
	if err != nil {
		return err
	}

	return c.withLock(checkpointPath, func(cf *CheckpointFile) error {
		pc, ok := cf.Phases[phaseID]
		if !ok {
			return fmt.Errorf("checkpoint: phase %s not initialized", phaseID)
		}

		if pc.SentinelCreated {
			return nil
		}

		if _, already := pc.Skipped[taskID]; !already {
			pc.Skipped[taskID] = c.timestamp()
		}

		if IsPhaseComplete(pc, cf.Context) {
			sentinelName := phaseSentinelName(phaseID)
			if err := Create(sentinelDir, sentinelName); err != nil {
				return fmt.Errorf("checkpoint: creating sentinel: %w", err)
			}
			pc.SentinelCreated = true
		}

		return nil
	})
}

// ResetAllPhases force-resets the checkpoint file to a fresh state by deleting
// the existing file and re-initializing all phases from pathflow-config.json.
// This provides an atomic reset with proper file locking, used by
// HandlePostTeamDelete to prepare the checkpoint for the next session.
func (c *Checkpoint) ResetAllPhases(checkpointPath, configPath string) error {
	dir := filepath.Dir(checkpointPath)
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return fmt.Errorf("checkpoint: creating directory: %w", err)
	}

	lockPath := checkpointPath + ".lock"
	lockFile, err := os.OpenFile(lockPath, os.O_CREATE|os.O_WRONLY, 0o644)
	if err != nil {
		return fmt.Errorf("checkpoint: opening lock file: %w", err)
	}
	defer lockFile.Close()

	if err := syscall.Flock(int(lockFile.Fd()), syscall.LOCK_EX); err != nil {
		return fmt.Errorf("checkpoint: acquiring lock: %w", err)
	}
	defer func() {
		_ = syscall.Flock(int(lockFile.Fd()), syscall.LOCK_UN)
	}()

	// Delete existing checkpoint file.
	if err := os.Remove(checkpointPath); err != nil && !os.IsNotExist(err) {
		return fmt.Errorf("checkpoint: removing old file: %w", err)
	}

	// Re-initialize from config (InitAllPhases uses its own lock, so we call
	// the unlocked version directly to avoid deadlock).
	configData, err := os.ReadFile(configPath)
	if err != nil {
		return fmt.Errorf("checkpoint: reading config: %w", err)
	}

	var config struct {
		Phases map[string]struct {
			RequiredTasks []string `json:"required_tasks"`
			Tasks         []struct {
				ID        string `json:"id"`
				Condition string `json:"condition,omitempty"`
			} `json:"tasks"`
		} `json:"phases"`
	}
	if err := json.Unmarshal(configData, &config); err != nil {
		return fmt.Errorf("checkpoint: parsing config: %w", err)
	}

	cf := &CheckpointFile{
		Phases: make(map[string]*PhaseCheckpoint, len(config.Phases)),
	}

	for configKey, phase := range config.Phases {
		phaseID := strings.SplitN(configKey, "-", 2)[0]

		conditions := make(map[string]string)
		for _, t := range phase.Tasks {
			if t.Condition != "" {
				conditions[t.ID] = t.Condition
			}
		}

		cf.Phases[phaseID] = &PhaseCheckpoint{
			Expected:   phase.RequiredTasks,
			Conditions: conditions,
			Registered: make(map[string]string),
			Completed:  make(map[string]string),
			Skipped:    make(map[string]string),
		}
	}

	return writeCheckpointFileUnlocked(checkpointPath, cf)
}

// GetStatus returns the checkpoint state for a specific phase.
func (c *Checkpoint) GetStatus(checkpointPath, phase string) (*PhaseCheckpoint, error) {
	cf, err := readCheckpointFile(checkpointPath)
	if err != nil {
		return nil, err
	}

	pc, ok := cf.Phases[phase]
	if !ok {
		return nil, fmt.Errorf("checkpoint: phase %s not found", phase)
	}
	return pc, nil
}

// IsPhaseComplete checks if all expected tasks in a phase are completed,
// skipped, or auto-skipped via conditions. An empty expected list means
// the phase is NOT complete (safety: uninitialized phases should not pass).
func IsPhaseComplete(pc *PhaseCheckpoint, ctx map[string]string) bool {
	if len(pc.Expected) == 0 {
		return false
	}

	for _, taskID := range pc.Expected {
		// Explicitly completed.
		if _, ok := pc.Completed[taskID]; ok {
			continue
		}
		// Explicitly skipped.
		if _, ok := pc.Skipped[taskID]; ok {
			continue
		}
		// Auto-skip via condition.
		cond, hasCond := pc.Conditions[taskID]
		if hasCond && isConditionAutoSkipped(cond, ctx) {
			continue
		}
		// Task is neither completed, skipped, nor auto-skippable.
		return false
	}
	return true
}

// isConditionAutoSkipped evaluates whether a conditional task should be
// auto-skipped based on the session context.
func isConditionAutoSkipped(condition string, ctx map[string]string) bool {
	switch condition {
	case "adhoc_only":
		// Auto-skip when origin is "planned" (planned tasks don't need adhoc registration).
		return ctx["origin"] == "planned"
	case "if_pipeline_includes_qa":
		// Auto-skip when work type doesn't include QA (DOCS, PLAN, SPKE).
		wt := ctx["work_type"]
		return wt == "DOCS" || wt == "PLAN" || wt == "SPKE"
	default:
		return false
	}
}

// extractPhase extracts the phase ID from a task ID (e.g., PF1-TSK-01 -> PF1).
func extractPhase(taskID string) (string, error) {
	m := taskIDRe.FindStringSubmatch(taskID)
	if m == nil {
		return "", fmt.Errorf("checkpoint: invalid task ID format: %s (expected PF{N}-TSK-{NN})", taskID)
	}
	return m[1], nil
}

// checkCrossPhaseGate verifies that the previous phase sentinel exists.
// For PF1, no gate check is needed (it has no predecessor).
func (c *Checkpoint) checkCrossPhaseGate(sentinelDir, phaseID string) error {
	// Extract the phase number.
	numStr := strings.TrimPrefix(phaseID, "PF")
	num, err := strconv.Atoi(numStr)
	if err != nil {
		return fmt.Errorf("checkpoint: invalid phase ID: %s", phaseID)
	}

	// PF1 has no predecessor.
	if num <= 1 {
		return nil
	}

	prevPhase := fmt.Sprintf("PF%d", num-1)
	prevSentinel := phaseSentinelName(prevPhase)

	if !Exists(sentinelDir, prevSentinel) {
		return fmt.Errorf("%w: phase %s requires sentinel %s (phase %s not complete)",
			ErrCrossPhaseBlock, phaseID, prevSentinel, prevPhase)
	}
	return nil
}

// phaseSentinelName converts a phase ID to its sentinel name (e.g., PF1 -> pf-1).
func phaseSentinelName(phaseID string) string {
	numStr := strings.TrimPrefix(phaseID, "PF")
	return "pf-" + numStr
}

// timestamp returns the current time as an RFC3339 string.
func (c *Checkpoint) timestamp() string {
	return c.Now().Format(time.RFC3339)
}

// withLock performs an atomic read-modify-write on the checkpoint file with
// exclusive file locking. The callback receives the parsed checkpoint data
// and may modify it in place. On success the modified data is written back
// atomically via a temp file + os.Rename.
func (c *Checkpoint) withLock(checkpointPath string, fn func(*CheckpointFile) error) error {
	dir := filepath.Dir(checkpointPath)
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return fmt.Errorf("checkpoint: creating directory: %w", err)
	}

	lockPath := checkpointPath + ".lock"
	lockFile, err := os.OpenFile(lockPath, os.O_CREATE|os.O_WRONLY, 0o644)
	if err != nil {
		return fmt.Errorf("checkpoint: opening lock file: %w", err)
	}
	defer lockFile.Close()

	if err := syscall.Flock(int(lockFile.Fd()), syscall.LOCK_EX); err != nil {
		return fmt.Errorf("checkpoint: acquiring lock: %w", err)
	}
	defer func() {
		_ = syscall.Flock(int(lockFile.Fd()), syscall.LOCK_UN)
	}()

	// Read existing checkpoint (or create empty).
	cf, err := readCheckpointFileUnlocked(checkpointPath)
	if err != nil {
		return err
	}

	// Apply mutation.
	if err := fn(cf); err != nil {
		return err
	}

	// Write back atomically.
	return writeCheckpointFileUnlocked(checkpointPath, cf)
}

// readCheckpointFile reads and parses the checkpoint file without locking.
// Used for read-only operations (GetStatus).
func readCheckpointFile(path string) (*CheckpointFile, error) {
	return readCheckpointFileUnlocked(path)
}

// readCheckpointFileUnlocked reads the checkpoint file. Returns an empty
// CheckpointFile if the file does not exist.
func readCheckpointFileUnlocked(path string) (*CheckpointFile, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		if os.IsNotExist(err) {
			return &CheckpointFile{
				Phases: make(map[string]*PhaseCheckpoint),
			}, nil
		}
		return nil, fmt.Errorf("checkpoint: reading file: %w", err)
	}

	var cf CheckpointFile
	if err := json.Unmarshal(data, &cf); err != nil {
		return nil, fmt.Errorf("checkpoint: parsing file: %w", err)
	}
	if cf.Phases == nil {
		cf.Phases = make(map[string]*PhaseCheckpoint)
	}
	return &cf, nil
}

// writeCheckpointFileUnlocked writes the checkpoint file atomically using
// a temp file + os.Rename.
func writeCheckpointFileUnlocked(path string, cf *CheckpointFile) error {
	data, err := json.MarshalIndent(cf, "", "  ")
	if err != nil {
		return fmt.Errorf("checkpoint: marshaling: %w", err)
	}
	data = append(data, '\n')

	tmpPath := path + ".tmp"
	if err := os.WriteFile(tmpPath, data, 0o644); err != nil {
		return fmt.Errorf("checkpoint: writing temp file: %w", err)
	}

	if err := os.Rename(tmpPath, path); err != nil {
		_ = os.Remove(tmpPath)
		return fmt.Errorf("checkpoint: renaming temp file: %w", err)
	}
	return nil
}
