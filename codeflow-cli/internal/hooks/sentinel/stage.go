package sentinel

import (
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"regexp"
	"strings"
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
	// Use strings.Fields + Join for simplicity -- splits on any whitespace
	// and joins with single space.
	return strings.Join(strings.Fields(s), " ")
}
