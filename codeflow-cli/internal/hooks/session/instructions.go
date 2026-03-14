package session

import (
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"

	"github.com/codeflow/codeflow-cli/internal/pathflow"
	"github.com/codeflow/codeflow-cli/internal/workstate"
)

// instructionsConfig represents the structure of instructions-config.json.
type instructionsConfig struct {
	Hooks struct {
		SessionStart map[string]instructionEntry `json:"SessionStart"`
	} `json:"hooks"`
}

// instructionEntry represents a single instruction entry in the config.
type instructionEntry struct {
	File    string `json:"file"`
	Enabled bool   `json:"enabled"`
}

// fallbackInstructions is the hardcoded fallback when config is unavailable.
const fallbackInstructions = `SESSION START - EXECUTE CLAUDE.md SECTION 2
You MUST execute the Session Start procedure from CLAUDE.md Section 2.
Check for active work (grep Status: active), present options to user, wait for choice.`

// RunInstructions implements the session-start instructions hook.
// It always returns nil (exit 0) — SessionStart hooks must never block.
func RunInstructions(stdin io.Reader, stdout io.Writer, projectDir string) error {
	// Parse stdin JSON (consume to prevent pipe blocking).
	_ = parseStdin(stdin)

	// Section 1: Config-driven instruction loading.
	outputInstructions(stdout, projectDir)

	// Section 2: Active task context.
	outputActiveTask(stdout, projectDir)

	// Section 3: PathFlow context.
	outputPathFlowContext(stdout, projectDir)

	return nil
}

// outputInstructions reads enabled SessionStart instructions from config and
// writes their content to stdout. Falls back to a hardcoded string if the
// config file is absent or cannot be parsed.
func outputInstructions(w io.Writer, projectDir string) {
	instructionsDir := filepath.Join(projectDir, ".codeflow", "config", "instructions")
	configPath := filepath.Join(instructionsDir, "instructions-config.json")

	data, err := os.ReadFile(configPath)
	if err != nil {
		fmt.Fprintln(w, fallbackInstructions)
		return
	}

	var cfg instructionsConfig
	if err := json.Unmarshal(data, &cfg); err != nil {
		fmt.Fprintln(w, fallbackInstructions)
		return
	}

	if len(cfg.Hooks.SessionStart) == 0 {
		fmt.Fprintln(w, fallbackInstructions)
		return
	}

	wrote := false
	for _, entry := range cfg.Hooks.SessionStart {
		if !entry.Enabled || entry.File == "" {
			continue
		}
		filePath := filepath.Join(instructionsDir, entry.File)
		content, err := os.ReadFile(filePath)
		if err != nil {
			continue
		}
		fmt.Fprint(w, string(content))
		fmt.Fprintln(w) // Blank line between instructions.
		wrote = true
	}

	if !wrote {
		fmt.Fprintln(w, fallbackInstructions)
	}
}

// outputActiveTask reads the active task from .state/runtime/active-task.json
// and outputs context to stdout. If no active task exists, outputs the
// "None" message with a register-work reminder.
func outputActiveTask(w io.Writer, projectDir string) {
	runtimeDir := filepath.Join(projectDir, ".state", "runtime")
	task, err := workstate.GetActiveTask(runtimeDir)
	if err != nil {
		// ErrNoActiveTask or any other read error — show "None" with reminder.
		fmt.Fprintln(w)
		fmt.Fprintln(w, "ACTIVE TASKS DETECTED: None")
		fmt.Fprintln(w)
		fmt.Fprintln(w, "IMPORTANT: Register work before making modifications.")
		fmt.Fprintln(w, `Delegate to cf-knowledge-layer teammate: SendMessage(recipient="cf-knowledge-layer", content="ensure-work-registered")`)
		return
	}

	taskID := task.FormatID
	if taskID == "" {
		taskID = task.TaskID
	}

	fmt.Fprintln(w)
	fmt.Fprintln(w, "ACTIVE TASKS DETECTED")
	fmt.Fprintln(w, "=====================")
	fmt.Fprintln(w)
	fmt.Fprintln(w, "Incomplete tasks found:")
	fmt.Fprintf(w, "  - %s (%s)\n", taskID, task.Status)
	if task.Title != "" {
		fmt.Fprintf(w, "    \"%s\"\n", task.Title)
	}
	fmt.Fprintln(w)
	fmt.Fprintln(w, "Options:")
	fmt.Fprintln(w, "  1. Resume task")
	fmt.Fprintln(w, "  2. Start new work")
	fmt.Fprintln(w, "  3. Review tasks")
}

// outputPathFlowContext checks if PathFlow is active and outputs completed
// phase sentinels and a compact recovery checklist. Always silent on failure.
func outputPathFlowContext(w io.Writer, projectDir string) {
	sessionID := resolveCodeflowSessionID(projectDir)
	if sessionID == "" {
		return
	}

	// Check if PathFlow is active using session status file.
	pathflowDir := filepath.Join(projectDir, ".state", "session", sessionID, "pathflow")
	if !IsPathflowActive(pathflowDir) {
		return
	}

	fmt.Fprintln(w)
	fmt.Fprintln(w, "PATHFLOW SESSION ACTIVE")
	fmt.Fprintln(w, "======================")

	// List completed phase sentinels.
	sentinelDir := filepath.Join(projectDir, ".state", "sentinels", "pathflow", sessionID)
	names, err := pathflow.List(sentinelDir)
	if err == nil && len(names) > 0 {
		// Filter to only phase sentinels (pf-*).
		var phases []string
		for _, n := range names {
			if strings.HasPrefix(n, "pf-") {
				phases = append(phases, n)
			}
		}
		if len(phases) > 0 {
			fmt.Fprintln(w, "Completed phases:")
			for _, p := range phases {
				fmt.Fprintf(w, "  - pathflow-%s\n", p)
			}
		} else {
			fmt.Fprintln(w, "No completed phases found")
		}
	} else {
		fmt.Fprintln(w, "No completed phases found")
	}

	fmt.Fprintln(w, "Mode: pathflow")
	fmt.Fprintln(w, "PCV: bypassed (WS-REV provides quality assurance)")
	fmt.Fprintln(w)

	// Compact recovery checklist (only if phase sentinels exist).
	if err == nil && len(names) > 0 {
		hasPhase := false
		for _, n := range names {
			if strings.HasPrefix(n, "pf-") {
				hasPhase = true
				break
			}
		}
		if hasPhase {
			fmt.Fprintln(w, "COMPACT RECOVERY: Task tracker registration check required.")
			fmt.Fprintln(w, "Phase sentinels exist from prior context. Task tracker may be out of sync.")
			fmt.Fprintln(w)
			fmt.Fprintln(w, "MANDATORY: Resume task tracker registration after context overflow.")
			fmt.Fprintln(w, "  Step 1: Read checkpoint state at .state/session/{SID}/pathflow/pathflow-phase-tasks.json")
			fmt.Fprintln(w, "  Step 2: Identify current phase from sentinel files at .state/sentinels/pathflow/{SID}/")
			fmt.Fprintln(w, "  Step 3: Backfill completed phases: TaskCreate then TaskUpdate to completed for each missing task")
			fmt.Fprintln(w, "  Step 4: Register current phase tasks: TaskCreate for EVERY PF{N}-TSK-{NN}")
			fmt.Fprintln(w, "  Step 5: Verify sentinel pipeline resumes creating sentinels")
			fmt.Fprintln(w)
			fmt.Fprintln(w, "FORBIDDEN: Skipping task tracker registration after context overflow.")
			fmt.Fprintln(w, "FORBIDDEN: Clubbing multiple PF{N}-TSK-{NN} entries into a single TaskCreate.")
			fmt.Fprintln(w, "FORBIDDEN: Proceeding past a phase gate without verifying its sentinel exists.")
			fmt.Fprintln(w)
		}
	}
}

// resolveCodeflowSessionID determines the CODEFLOW_SESSION_ID.
// Priority: CODEFLOW_SESSION_ID env var > codeflow-env.sh file.
func resolveCodeflowSessionID(projectDir string) string {
	// Priority 1: environment variable.
	if sid := os.Getenv("CODEFLOW_SESSION_ID"); sid != "" {
		return strings.TrimSpace(sid)
	}

	// Priority 2: codeflow-env.sh file.
	envFilePath := filepath.Join(projectDir, ".state", "runtime", "codeflow-env.sh")
	data, err := os.ReadFile(envFilePath)
	if err != nil {
		return ""
	}
	return parseEnvFileSessionID(string(data))
}
