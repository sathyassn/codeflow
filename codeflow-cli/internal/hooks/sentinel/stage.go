package sentinel

import (
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"time"

	"github.com/codeflow/codeflow-cli/internal/hooks/session"
)

// stageCompleteRe matches "STAGE-COMPLETE: WS-{STAGE}" in normalized (uppercased) content.
var stageCompleteRe = regexp.MustCompile(`STAGE-COMPLETE:\s+WS-(DEV|REV|QA|TEST|PLAN|DOCS)`)

// primaryStages lists the primary work stages that must complete before WS-REV.
var primaryStages = []string{"dev", "plan", "docs", "test"}

// Verdict represents the result of a sentinel hook check.
type Verdict struct {
	// Allow is true if the operation is permitted.
	Allow bool

	// Reason is a human-readable explanation when the operation is blocked.
	Reason string
}

// postToolUseInput represents the JSON structure sent by Claude Code on stdin
// to PostToolUse hooks.
type postToolUseInput struct {
	ToolName  string          `json:"tool_name"`
	ToolInput json.RawMessage `json:"tool_input"`
}

// sendMessageInput represents the tool_input for SendMessage tool calls.
type sendMessageInput struct {
	Content string `json:"content"`
}

// CheckAndCreateStageSentinel parses PostToolUse stdin for SendMessage calls,
// pattern-matches "STAGE-COMPLETE: WS-{STAGE}" in the content, validates
// stage ordering, and creates the corresponding sentinel file.
//
// Returns a Verdict. When the verdict blocks (Allow=false), the caller should
// exit with code 2.
func CheckAndCreateStageSentinel(stdin io.Reader, sentinelDir string) *Verdict {
	data, err := io.ReadAll(stdin)
	if err != nil {
		// Read error -- allow through (graceful degradation).
		return &Verdict{Allow: true}
	}

	return CheckAndCreateStageSentinelFromData(data, sentinelDir)
}

// CheckAndCreateStageSentinelFromData is like CheckAndCreateStageSentinel but
// operates on pre-read data instead of an io.Reader. This allows the caller to
// read stdin once and dispatch to multiple handlers.
func CheckAndCreateStageSentinelFromData(data []byte, sentinelDir string) *Verdict {
	if len(data) == 0 {
		return &Verdict{Allow: true}
	}

	var input postToolUseInput
	if err := json.Unmarshal(data, &input); err != nil {
		// Parse error -- allow through.
		return &Verdict{Allow: true}
	}

	if input.ToolName != "SendMessage" {
		return &Verdict{Allow: true}
	}

	if len(input.ToolInput) == 0 {
		return &Verdict{Allow: true}
	}

	var msg sendMessageInput
	if err := json.Unmarshal(input.ToolInput, &msg); err != nil {
		return &Verdict{Allow: true}
	}

	if msg.Content == "" {
		return &Verdict{Allow: true}
	}

	// Normalize: uppercase + collapse whitespace.
	normalized := strings.ToUpper(msg.Content)
	normalized = collapseWhitespace(normalized)

	matches := stageCompleteRe.FindStringSubmatch(normalized)
	if matches == nil {
		// No stage completion pattern -- nothing to do.
		return &Verdict{Allow: true}
	}

	stageLower := strings.ToLower(matches[1])

	// Stage ordering validation.
	switch stageLower {
	case "rev":
		// WS-REV requires a prior primary stage sentinel.
		hasPrimary := false
		for _, ps := range primaryStages {
			if hasSentinelFile(sentinelDir, "ws-"+ps) {
				hasPrimary = true
				break
			}
		}
		if !hasPrimary {
			return &Verdict{
				Allow:  false,
				Reason: "BLOCKED: ws-rev requires prior primary stage (ws-dev/ws-plan/ws-docs/ws-test)",
			}
		}

	case "qa":
		// WS-QA requires prior WS-DEV or WS-TEST.
		if !hasSentinelFile(sentinelDir, "ws-dev") && !hasSentinelFile(sentinelDir, "ws-test") {
			return &Verdict{
				Allow:  false,
				Reason: "BLOCKED: ws-qa requires prior ws-dev or ws-test sentinel",
			}
		}
	}

	// Create sentinel file.
	if err := createSentinelFile(sentinelDir, "ws-"+stageLower); err != nil {
		return &Verdict{
			Allow:  true,
			Reason: fmt.Sprintf("sentinel creation failed: %v", err),
		}
	}

	return &Verdict{Allow: true}
}

// HandleStageSentinelStatus updates pathflow-session-status.json with the
// last completed stage when a STAGE-COMPLETE message is detected.
func HandleStageSentinelStatus(data []byte, sessionDir string) {
	if sessionDir == "" {
		return
	}

	var input postToolUseInput
	if err := json.Unmarshal(data, &input); err != nil {
		return
	}

	if input.ToolName != "SendMessage" || len(input.ToolInput) == 0 {
		return
	}

	var msg sendMessageInput
	if err := json.Unmarshal(input.ToolInput, &msg); err != nil || msg.Content == "" {
		return
	}

	normalized := strings.ToUpper(msg.Content)
	normalized = collapseWhitespace(normalized)
	matches := stageCompleteRe.FindStringSubmatch(normalized)
	if matches == nil {
		return
	}

	stageLower := strings.ToLower(matches[1])
	_ = session.UpdatePathflowSessionStatus(sessionDir, func() time.Time { return time.Now().UTC() }, func(s *session.PathflowSessionStatus) {
		s.LastCompletedStage = "ws-" + stageLower
	})
}

// hasSentinelFile checks whether a sentinel file exists in the sentinel directory.
func hasSentinelFile(sentinelDir, name string) bool {
	path := filepath.Join(sentinelDir, "pathflow-"+name)
	_, err := os.Stat(path)
	return err == nil
}

// createSentinelFile creates a sentinel file in the sentinel directory.
// It creates the directory if it does not exist.
func createSentinelFile(sentinelDir, name string) error {
	if err := os.MkdirAll(sentinelDir, 0o755); err != nil {
		return fmt.Errorf("create sentinel dir: %w", err)
	}

	path := filepath.Join(sentinelDir, "pathflow-"+name)
	return os.WriteFile(path, []byte("1"), 0o644)
}

// collapseWhitespace replaces runs of whitespace with a single space.
func collapseWhitespace(s string) string {
	return strings.Join(strings.Fields(s), " ")
}

// teamCreateInput represents the tool_input for TeamCreate tool calls.
type teamCreateInput struct {
	TeamName string `json:"team_name"`
}

// taskSpawnInput represents the tool_input for Task (teammate spawn) tool calls.
type taskSpawnInput struct {
	Name string `json:"name"`
}

// PathflowTeam represents the pathflow-team.json schema.
type PathflowTeam struct {
	TeamName          string  `json:"team_name"`
	LeadPID           int     `json:"lead_pid"`
	CodeflowSessionID string  `json:"codeflow_session_id"`
	TeammateSpawned   bool    `json:"teammate_spawned"`
	CreatedAt         string  `json:"created_at"`
	LastSpawnName     *string `json:"last_spawn_name"`
}

// HandleTeamCreate processes a TeamCreate PostToolUse event, creates
// pathflow-team.json in the session pathflow directory, and updates the
// session status to "pf-started" with the team name.
//
// The sessionDir should be: {projectDir}/.state/session/{sessionID}/pathflow/
func HandleTeamCreate(data []byte, sessionDir, sessionID string) *Verdict {
	if sessionDir == "" || sessionID == "" {
		return &Verdict{Allow: true}
	}

	var input postToolUseInput
	if err := json.Unmarshal(data, &input); err != nil {
		return &Verdict{Allow: true}
	}

	if input.ToolName != "TeamCreate" {
		return &Verdict{Allow: true}
	}

	var tc teamCreateInput
	if err := json.Unmarshal(input.ToolInput, &tc); err != nil {
		return &Verdict{Allow: true}
	}

	team := PathflowTeam{
		TeamName:          tc.TeamName,
		LeadPID:           0, // PID tracking removed — status file is the authority
		CodeflowSessionID: sessionID,
		TeammateSpawned:   false,
		CreatedAt:         time.Now().UTC().Format(time.RFC3339),
		LastSpawnName:     nil,
	}

	teamJSON, err := json.Marshal(team)
	if err != nil {
		return &Verdict{
			Allow:  true,
			Reason: fmt.Sprintf("pathflow-team.json marshal failed: %v", err),
		}
	}

	if err := os.MkdirAll(sessionDir, 0o755); err != nil {
		return &Verdict{
			Allow:  true,
			Reason: fmt.Sprintf("create session dir: %v", err),
		}
	}

	teamFilePath := filepath.Join(sessionDir, "pathflow-team.json")
	if err := atomicWriteFile(teamFilePath, teamJSON, 0o644); err != nil {
		return &Verdict{
			Allow:  true,
			Reason: fmt.Sprintf("write pathflow-team.json: %v", err),
		}
	}

	// Update session status to "pf-started" with team name.
	_ = session.UpdatePathflowSessionStatus(sessionDir, func() time.Time { return time.Now().UTC() }, func(s *session.PathflowSessionStatus) {
		s.Status = "pf-started"
		s.TeamName = tc.TeamName
	})

	return &Verdict{Allow: true}
}

// HandleTeammateSpawn processes a Task PostToolUse event (teammate spawn) and
// updates pathflow-team.json with teammate_spawned=true and last_spawn_name.
//
// The sessionDir should be: {projectDir}/.state/session/{sessionID}/pathflow/
func HandleTeammateSpawn(data []byte, sessionDir string) *Verdict {
	if sessionDir == "" {
		return &Verdict{Allow: true}
	}

	var input postToolUseInput
	if err := json.Unmarshal(data, &input); err != nil {
		return &Verdict{Allow: true}
	}

	if input.ToolName != "Task" {
		return &Verdict{Allow: true}
	}

	var ts taskSpawnInput
	if err := json.Unmarshal(input.ToolInput, &ts); err != nil {
		return &Verdict{Allow: true}
	}

	teamFilePath := filepath.Join(sessionDir, "pathflow-team.json")
	existing, err := os.ReadFile(teamFilePath)
	if err != nil {
		// pathflow-team.json doesn't exist yet -- skip silently.
		return &Verdict{Allow: true}
	}

	var team PathflowTeam
	if err := json.Unmarshal(existing, &team); err != nil {
		return &Verdict{Allow: true}
	}

	team.TeammateSpawned = true
	if ts.Name != "" {
		team.LastSpawnName = &ts.Name
	}

	updated, err := json.Marshal(team)
	if err != nil {
		return &Verdict{
			Allow:  true,
			Reason: fmt.Sprintf("pathflow-team.json marshal failed: %v", err),
		}
	}

	if err := atomicWriteFile(teamFilePath, updated, 0o644); err != nil {
		return &Verdict{
			Allow:  true,
			Reason: fmt.Sprintf("update pathflow-team.json: %v", err),
		}
	}

	return &Verdict{Allow: true}
}

// atomicWriteFile writes data to a file atomically via tmp+rename.
// Uses os.CreateTemp in the same directory for concurrent safety.
func atomicWriteFile(path string, data []byte, perm os.FileMode) error {
	dir := filepath.Dir(path)
	tmp, err := os.CreateTemp(dir, ".pathflow-*.tmp")
	if err != nil {
		return fmt.Errorf("create temp file: %w", err)
	}
	tmpPath := tmp.Name()

	if _, err := tmp.Write(data); err != nil {
		tmp.Close()
		_ = os.Remove(tmpPath)
		return fmt.Errorf("write temp file: %w", err)
	}
	if err := tmp.Chmod(perm); err != nil {
		tmp.Close()
		_ = os.Remove(tmpPath)
		return fmt.Errorf("chmod temp file: %w", err)
	}
	if err := tmp.Close(); err != nil {
		_ = os.Remove(tmpPath)
		return fmt.Errorf("close temp file: %w", err)
	}
	if err := os.Rename(tmpPath, path); err != nil {
		_ = os.Remove(tmpPath)
		return fmt.Errorf("rename tmp to target: %w", err)
	}
	return nil
}
