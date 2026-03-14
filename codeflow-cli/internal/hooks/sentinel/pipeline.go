package sentinel

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
)

// pathflowConfig represents the relevant parts of pathflow-config.json.
type pathflowConfig struct {
	Phases    map[string]phaseConfig `json:"phases"`
	Pipelines map[string][]string   `json:"pipelines"`
}

// phaseConfig represents a single phase entry in pathflow-config.json.
type phaseConfig struct {
	PhaseOrder int `json:"phase_order"`
}

// LoadPipelines reads pathflow-config.json and returns all pipeline definitions.
// Returns map[workType][]stageNames, e.g., {"FEAT": ["WS-DEV", "WS-REV", "WS-QA"]}.
func LoadPipelines(configDir string) (map[string][]string, error) {
	configPath := filepath.Join(configDir, "pathflow-config.json")
	data, err := os.ReadFile(configPath)
	if err != nil {
		return nil, fmt.Errorf("load pipelines: %w", err)
	}

	var config pathflowConfig
	if err := json.Unmarshal(data, &config); err != nil {
		return nil, fmt.Errorf("parse pipelines: %w", err)
	}

	if config.Pipelines == nil {
		return nil, fmt.Errorf("load pipelines: no pipelines section in config")
	}

	return config.Pipelines, nil
}

// LoadPhaseOrder reads pathflow-config.json and returns phase keys in order.
// Returns ["PF1", "PF2", "PF3", "PF4", "PF5", "PF6", "PF7"].
func LoadPhaseOrder(configDir string) ([]string, error) {
	configPath := filepath.Join(configDir, "pathflow-config.json")
	data, err := os.ReadFile(configPath)
	if err != nil {
		return nil, fmt.Errorf("load phase order: %w", err)
	}

	var config pathflowConfig
	if err := json.Unmarshal(data, &config); err != nil {
		return nil, fmt.Errorf("parse phase order: %w", err)
	}

	if config.Phases == nil {
		return nil, fmt.Errorf("load phase order: no phases section in config")
	}

	// Build ordered slice using phase_order field.
	ordered := make([]string, len(config.Phases))
	for key, phase := range config.Phases {
		// Extract "PF1" from "PF1-INIT".
		phaseID := strings.SplitN(key, "-", 2)[0]
		idx := phase.PhaseOrder - 1 // phase_order is 1-based
		if idx >= 0 && idx < len(ordered) {
			ordered[idx] = phaseID
		}
	}

	// Filter out any empty slots (shouldn't happen with valid config).
	var result []string
	for _, p := range ordered {
		if p != "" {
			result = append(result, p)
		}
	}

	return result, nil
}

// VerifyCumulativeStageSentinels checks that ALL stage sentinels from pipeline[0]
// through pipeline[upToIndex] exist in sentinelDir.
// Returns (allPresent, firstMissingSentinel).
func VerifyCumulativeStageSentinels(sentinelDir string, pipeline []string, upToIndex int) (bool, string) {
	for i := 0; i <= upToIndex && i < len(pipeline); i++ {
		name := StageSentinelKey(pipeline[i])
		if !hasSentinelFile(sentinelDir, name) {
			return false, name
		}
	}
	return true, ""
}

// VerifyCumulativePhaseSentinels checks that ALL pf-1 through pf-{upToPhase}
// sentinels exist in sentinelDir.
// Returns (allPresent, firstMissingSentinel).
func VerifyCumulativePhaseSentinels(sentinelDir string, upToPhase int) (bool, string) {
	for i := 1; i <= upToPhase; i++ {
		name := fmt.Sprintf("pf-%d", i)
		if !hasSentinelFile(sentinelDir, name) {
			return false, name
		}
	}
	return true, ""
}

// InferWorkTypeFromBranch maps git branch prefix to work type.
// feat/ -> FEAT, fix/ -> FIX, refactor/ -> RFCT, docs/ -> DOCS, test/ -> TEST,
// chore/ -> CHOR, cicd/ -> CICD, plan/ -> PLAN, spike/ -> SPKE, hotfix/ -> HTFX.
// Returns empty string if the branch prefix is not recognized.
func InferWorkTypeFromBranch(branchName string) string {
	prefixMap := map[string]string{
		"feat/":     "FEAT",
		"fix/":      "FIX",
		"refactor/": "RFCT",
		"docs/":     "DOCS",
		"test/":     "TEST",
		"chore/":    "CHOR",
		"cicd/":     "CICD",
		"plan/":     "PLAN",
		"spike/":    "SPKE",
		"hotfix/":   "HTFX",
	}

	for prefix, workType := range prefixMap {
		if strings.HasPrefix(branchName, prefix) {
			return workType
		}
	}
	return ""
}

// StageSentinelKey converts a pipeline entry to its sentinel name.
// "WS-DEV" -> "ws-dev", "WS-REV" -> "ws-rev".
func StageSentinelKey(pipelineEntry string) string {
	return strings.ToLower(pipelineEntry)
}

// ReadWorkTypeFromSessionStatus reads the work_type field from
// pathflow-session-status.json in the given session pathflow directory.
func ReadWorkTypeFromSessionStatus(sessionDir string) string {
	statusPath := filepath.Join(sessionDir, "pathflow-session-status.json")
	data, err := os.ReadFile(statusPath)
	if err != nil {
		return ""
	}

	var status struct {
		WorkType string `json:"work_type"`
	}
	if err := json.Unmarshal(data, &status); err != nil {
		return ""
	}
	return status.WorkType
}

// deriveProjectDir derives the project root directory from a sentinel directory path.
// sentinelDir is {projectDir}/.state/sentinels/pathflow/{SID}/
// So projectDir is 4 levels up.
func deriveProjectDir(sentinelDir string) string {
	return filepath.Join(sentinelDir, "..", "..", "..", "..")
}

// deriveSessionDir derives the session pathflow directory from a sentinel directory path.
// sentinelDir is {projectDir}/.state/sentinels/pathflow/{SID}/
// sessionDir is {projectDir}/.state/session/{SID}/pathflow/
func deriveSessionDir(sentinelDir string) string {
	projectDir := deriveProjectDir(sentinelDir)
	sessionID := filepath.Base(sentinelDir)
	return filepath.Join(projectDir, ".state", "session", sessionID, "pathflow")
}

// deriveConfigDir derives the pathflow config directory from a sentinel directory path.
func deriveConfigDir(sentinelDir string) string {
	projectDir := deriveProjectDir(sentinelDir)
	return filepath.Join(projectDir, ".codeflow", "config", "pathflow")
}
