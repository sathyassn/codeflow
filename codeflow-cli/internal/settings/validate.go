package settings

import (
	"crypto/sha256"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"
)

// ValidationCheck identifies a specific check type.
type ValidationCheck string

const (
	// CheckHooksSHA256 checks hooks section SHA256 consistency across templates.
	CheckHooksSHA256 ValidationCheck = "hooks_sha256"
	// CheckVersionConsistency checks _version consistency across templates.
	CheckVersionConsistency ValidationCheck = "version_consistency"
	// CheckHookWiring checks for orphaned or broken hook script references.
	CheckHookWiring ValidationCheck = "hook_wiring"
	// CheckSettingsChecksum checks full file SHA256 of settings.json vs template.
	CheckSettingsChecksum ValidationCheck = "settings_checksum"
	// CheckSettingsLocalChecksum checks full file SHA256 of settings.local.json vs template.
	CheckSettingsLocalChecksum ValidationCheck = "settings_local_checksum"
)

// ValidationResult holds the outcome of a single validation check.
type ValidationResult struct {
	Check   ValidationCheck `json:"check"`
	Passed  bool            `json:"passed"`
	Message string          `json:"message"`
	Details string          `json:"details,omitempty"`
}

// CopyMapping represents a template-to-destination mapping from enforcement-policy.json.
type CopyMapping struct {
	Template    string `json:"template"`
	Destination string `json:"destination"`
	Purpose     string `json:"purpose"`
}

// SettingsTemplatesConfig represents the settings_templates section
// of enforcement-policy.json.
type SettingsTemplatesConfig struct {
	Directory    string        `json:"directory"`
	CopyMappings []CopyMapping `json:"copy_mappings"`
}

// ValidateSettingsTemplates runs all 5 validation checks against the settings
// templates in the given project directory.
//
// Checks:
//  1. Hooks section SHA256 consistency across all templates
//  2. _version consistency across all templates
//  3. Hook wiring audit (orphaned/broken scripts)
//  4. Full file SHA256 of settings.json vs template source
//  5. Full file SHA256 of settings.local.json vs template source
func ValidateSettingsTemplates(projectDir string) ([]ValidationResult, error) {
	config, err := loadSettingsTemplatesConfig(projectDir)
	if err != nil {
		return nil, fmt.Errorf("load settings templates config: %w", err)
	}

	templateDir := filepath.Join(projectDir, config.Directory)

	templates, err := discoverTemplates(templateDir)
	if err != nil {
		return nil, fmt.Errorf("discover templates: %w", err)
	}

	if len(templates) < 2 {
		return nil, fmt.Errorf("need at least 2 templates, found %d in %s", len(templates), templateDir)
	}

	var results []ValidationResult

	// Check 1: Hooks SHA256 consistency.
	results = append(results, checkHooksConsistency(templateDir, templates))

	// Check 2: Version consistency.
	results = append(results, checkVersionConsistency(templateDir, templates))

	// Check 3: Hook wiring audit.
	results = append(results, checkHookWiring(projectDir, filepath.Join(templateDir, templates[0])))

	// Check 4 & 5: File SHA256 checksums for copy mappings.
	for _, mapping := range config.CopyMappings {
		results = append(results, checkFileChecksum(projectDir, templateDir, mapping))
	}

	return results, nil
}

// HasFailures returns true if any validation result failed.
func HasFailures(results []ValidationResult) bool {
	for _, r := range results {
		if !r.Passed {
			return true
		}
	}
	return false
}

// FormatResults returns a human-readable summary of all validation results.
func FormatResults(results []ValidationResult) string {
	var b strings.Builder
	passed := 0
	failed := 0

	b.WriteString("Settings Template Validation\n")
	b.WriteString(strings.Repeat("=", 50) + "\n\n")

	for _, r := range results {
		status := "PASS"
		if !r.Passed {
			status = "FAIL"
			failed++
		} else {
			passed++
		}
		b.WriteString(fmt.Sprintf("[%s] %s: %s\n", status, r.Check, r.Message))
		if r.Details != "" {
			b.WriteString(fmt.Sprintf("       %s\n", r.Details))
		}
	}

	b.WriteString(fmt.Sprintf("\nSummary: %d passed, %d failed\n", passed, failed))
	return b.String()
}

// FormatHookOutput returns the hookSpecificOutput JSON for PostToolUse hooks.
func FormatHookOutput(results []ValidationResult) (string, error) {
	var messages []string
	for _, r := range results {
		status := "OK"
		if !r.Passed {
			status = "ISSUE"
		}
		messages = append(messages, fmt.Sprintf("[%s] %s: %s", status, r.Check, r.Message))
	}

	output := map[string]any{
		"hookSpecificOutput": map[string]any{
			"hookEventName":     "PostToolUse",
			"additionalContext": strings.Join(messages, "\\n"),
		},
	}

	data, err := json.MarshalIndent(output, "", "  ")
	if err != nil {
		return "", fmt.Errorf("marshal hook output: %w", err)
	}
	return string(data), nil
}

// loadSettingsTemplatesConfig reads the settings_templates section from
// enforcement-policy.json.
func loadSettingsTemplatesConfig(projectDir string) (*SettingsTemplatesConfig, error) {
	configPath := filepath.Join(projectDir, ".codeflow", "config", "enforcement", "enforcement-policy.json")
	data, err := os.ReadFile(configPath)
	if err != nil {
		// Return defaults if config not found.
		return &SettingsTemplatesConfig{
			Directory: ".claude/settings-templates",
			CopyMappings: []CopyMapping{
				{Template: "autonomous.json", Destination: ".claude/settings.json", Purpose: "Project settings"},
				{Template: "autonomous.json", Destination: ".claude/settings.local.json", Purpose: "Local settings"},
			},
		}, nil
	}

	var raw struct {
		SettingsTemplates SettingsTemplatesConfig `json:"settings_templates"`
	}
	if err := json.Unmarshal(data, &raw); err != nil {
		return nil, fmt.Errorf("parse enforcement policy: %w", err)
	}

	config := &raw.SettingsTemplates
	if config.Directory == "" {
		config.Directory = ".claude/settings-templates"
	}
	if len(config.CopyMappings) == 0 {
		config.CopyMappings = []CopyMapping{
			{Template: "autonomous.json", Destination: ".claude/settings.json", Purpose: "Project settings"},
			{Template: "autonomous.json", Destination: ".claude/settings.local.json", Purpose: "Local settings"},
		}
	}
	return config, nil
}

// discoverTemplates finds all .json files in the template directory,
// sorted alphabetically.
func discoverTemplates(dir string) ([]string, error) {
	entries, err := os.ReadDir(dir)
	if err != nil {
		return nil, fmt.Errorf("read template directory %s: %w", dir, err)
	}

	var templates []string
	for _, e := range entries {
		if e.IsDir() {
			continue
		}
		if filepath.Ext(e.Name()) == ".json" {
			templates = append(templates, e.Name())
		}
	}

	sort.Strings(templates)
	return templates, nil
}

// hooksHash computes the SHA256 hash of the sorted "hooks" section in a
// settings template JSON file.
func hooksHash(filePath string) (string, error) {
	data, err := os.ReadFile(filePath)
	if err != nil {
		return "", err
	}

	var parsed map[string]any
	if err := json.Unmarshal(data, &parsed); err != nil {
		return "", err
	}

	hooksSection, ok := parsed["hooks"]
	if !ok {
		return "", fmt.Errorf("no hooks section in %s", filePath)
	}

	// Marshal with sorted keys for deterministic comparison.
	sorted, err := json.Marshal(hooksSection)
	if err != nil {
		return "", err
	}

	h := sha256.Sum256(sorted)
	return fmt.Sprintf("%x", h), nil
}

// templateVersion extracts the _version field from a settings template.
func templateVersion(filePath string) (string, error) {
	data, err := os.ReadFile(filePath)
	if err != nil {
		return "", err
	}

	var parsed map[string]any
	if err := json.Unmarshal(data, &parsed); err != nil {
		return "", err
	}

	ver, ok := parsed["_version"]
	if !ok {
		return "missing", nil
	}

	return fmt.Sprintf("%v", ver), nil
}

// fileHash computes the SHA256 hash of a file's contents.
func fileHash(path string) (string, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return "", err
	}
	h := sha256.Sum256(data)
	return fmt.Sprintf("%x", h), nil
}

func checkHooksConsistency(templateDir string, templates []string) ValidationResult {
	refHash, err := hooksHash(filepath.Join(templateDir, templates[0]))
	if err != nil {
		return ValidationResult{
			Check:   CheckHooksSHA256,
			Passed:  false,
			Message: "Failed to read reference hooks section",
			Details: err.Error(),
		}
	}

	var mismatched []string
	for _, t := range templates[1:] {
		h, err := hooksHash(filepath.Join(templateDir, t))
		if err != nil {
			mismatched = append(mismatched, t+" (read error)")
			continue
		}
		if h != refHash {
			mismatched = append(mismatched, t)
		}
	}

	if len(mismatched) > 0 {
		return ValidationResult{
			Check:   CheckHooksSHA256,
			Passed:  false,
			Message: fmt.Sprintf("Hooks section mismatch across templates (ref: %s)", templates[0]),
			Details: fmt.Sprintf("Differing: %s", strings.Join(mismatched, ", ")),
		}
	}

	return ValidationResult{
		Check:   CheckHooksSHA256,
		Passed:  true,
		Message: fmt.Sprintf("Hooks sections identical across all %d templates", len(templates)),
	}
}

func checkVersionConsistency(templateDir string, templates []string) ValidationResult {
	refVersion, err := templateVersion(filepath.Join(templateDir, templates[0]))
	if err != nil {
		return ValidationResult{
			Check:   CheckVersionConsistency,
			Passed:  false,
			Message: "Failed to read reference version",
			Details: err.Error(),
		}
	}

	var mismatched []string
	for _, t := range templates[1:] {
		ver, err := templateVersion(filepath.Join(templateDir, t))
		if err != nil {
			mismatched = append(mismatched, t+" (read error)")
			continue
		}
		if ver != refVersion {
			mismatched = append(mismatched, fmt.Sprintf("%s=%s", t, ver))
		}
	}

	if len(mismatched) > 0 {
		return ValidationResult{
			Check:   CheckVersionConsistency,
			Passed:  false,
			Message: fmt.Sprintf("Version mismatch (ref %s: %s)", templates[0], refVersion),
			Details: fmt.Sprintf("Differing: %s", strings.Join(mismatched, ", ")),
		}
	}

	return ValidationResult{
		Check:   CheckVersionConsistency,
		Passed:  true,
		Message: fmt.Sprintf("Version %s consistent across all %d templates", refVersion, len(templates)),
	}
}

func checkHookWiring(projectDir, templateFile string) ValidationResult {
	hooksDir := filepath.Join(projectDir, ".claude", "hooks", "codeflow")

	// Get referenced scripts from template.
	referenced, err := extractReferencedScripts(templateFile)
	if err != nil {
		return ValidationResult{
			Check:   CheckHookWiring,
			Passed:  false,
			Message: "Failed to extract referenced scripts from template",
			Details: err.Error(),
		}
	}

	// Get existing scripts on disk.
	existing, err := findExistingScripts(hooksDir)
	if err != nil {
		return ValidationResult{
			Check:   CheckHookWiring,
			Passed:  false,
			Message: "Failed to scan hooks directory",
			Details: err.Error(),
		}
	}

	// Find orphaned (exist but not referenced).
	existSet := make(map[string]bool)
	for _, s := range existing {
		existSet[s] = true
	}
	refSet := make(map[string]bool)
	for _, s := range referenced {
		refSet[s] = true
	}

	var orphaned, broken []string
	for _, s := range existing {
		if !refSet[s] {
			orphaned = append(orphaned, s)
		}
	}
	for _, s := range referenced {
		if !existSet[s] {
			broken = append(broken, s)
		}
	}

	if len(orphaned) > 0 || len(broken) > 0 {
		var details []string
		if len(orphaned) > 0 {
			details = append(details, fmt.Sprintf("Orphaned: %s", strings.Join(orphaned, ", ")))
		}
		if len(broken) > 0 {
			details = append(details, fmt.Sprintf("Broken: %s", strings.Join(broken, ", ")))
		}
		return ValidationResult{
			Check:   CheckHookWiring,
			Passed:  false,
			Message: fmt.Sprintf("Hook wiring issues: %d orphaned, %d broken", len(orphaned), len(broken)),
			Details: strings.Join(details, "; "),
		}
	}

	return ValidationResult{
		Check:   CheckHookWiring,
		Passed:  true,
		Message: fmt.Sprintf("Hook wiring OK: %d referenced, %d exist", len(referenced), len(existing)),
	}
}

func checkFileChecksum(projectDir, templateDir string, mapping CopyMapping) ValidationResult {
	check := CheckSettingsChecksum
	if strings.HasSuffix(mapping.Destination, "settings.local.json") {
		check = CheckSettingsLocalChecksum
	}

	templatePath := filepath.Join(templateDir, mapping.Template)
	destPath := filepath.Join(projectDir, mapping.Destination)

	templateHash, err := fileHash(templatePath)
	if err != nil {
		return ValidationResult{
			Check:   check,
			Passed:  false,
			Message: fmt.Sprintf("Cannot read template %s", mapping.Template),
			Details: err.Error(),
		}
	}

	destHash, err := fileHash(destPath)
	if err != nil {
		return ValidationResult{
			Check:   check,
			Passed:  false,
			Message: fmt.Sprintf("Cannot read %s", mapping.Destination),
			Details: err.Error(),
		}
	}

	if templateHash != destHash {
		return ValidationResult{
			Check:   check,
			Passed:  false,
			Message: fmt.Sprintf("%s does not match template %s", mapping.Destination, mapping.Template),
			Details: fmt.Sprintf("Fix: cp %s %s", filepath.Join(templateDir, mapping.Template), mapping.Destination),
		}
	}

	return ValidationResult{
		Check:   check,
		Passed:  true,
		Message: fmt.Sprintf("%s matches template %s", mapping.Destination, mapping.Template),
	}
}

// extractReferencedScripts parses a settings template JSON file and extracts
// all .sh script basenames referenced in hook command fields.
func extractReferencedScripts(templateFile string) ([]string, error) {
	data, err := os.ReadFile(templateFile)
	if err != nil {
		return nil, err
	}

	// Walk the JSON looking for "command" fields referencing .claude/hooks/.
	var scripts []string
	seen := make(map[string]bool)

	var walk func(v any)
	walk = func(v any) {
		switch val := v.(type) {
		case map[string]any:
			if cmd, ok := val["command"]; ok {
				if s, ok := cmd.(string); ok && strings.Contains(s, ".claude/hooks/") {
					base := filepath.Base(s)
					if !seen[base] {
						seen[base] = true
						scripts = append(scripts, base)
					}
				}
			}
			for _, child := range val {
				walk(child)
			}
		case []any:
			for _, child := range val {
				walk(child)
			}
		}
	}

	var parsed any
	if err := json.Unmarshal(data, &parsed); err != nil {
		return nil, err
	}
	walk(parsed)

	sort.Strings(scripts)
	return scripts, nil
}

// findExistingScripts finds all .sh files in the hooks directory tree.
func findExistingScripts(hooksDir string) ([]string, error) {
	var scripts []string
	seen := make(map[string]bool)

	err := filepath.WalkDir(hooksDir, func(path string, d os.DirEntry, err error) error {
		if err != nil {
			return nil // skip inaccessible directories
		}
		if d.IsDir() {
			return nil
		}
		if filepath.Ext(path) == ".sh" {
			base := filepath.Base(path)
			if !seen[base] {
				seen[base] = true
				scripts = append(scripts, base)
			}
		}
		return nil
	})
	if err != nil {
		return nil, err
	}

	sort.Strings(scripts)
	return scripts, nil
}
