package report

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"

	"gopkg.in/yaml.v3"
)

// EpicTracker generates progress tracking reports from filesystem data.
type EpicTracker struct {
	// EpicsDir is the path to the epics directory (e.g., project-management/epics).
	EpicsDir string

	// OutputDir is the path to the tracking output directory.
	// If empty, defaults to the sibling "tracking" directory of EpicsDir's parent.
	OutputDir string
}

// epicMeta holds parsed YAML frontmatter from an epic markdown file.
type epicMeta struct {
	Title     string `yaml:"title"`
	Status    string `yaml:"status"`
	Priority  string `yaml:"priority"`
	IsOngoing bool   `yaml:"is_ongoing"`
	AreaType  string `yaml:"area_type"`
	WorkType  string `yaml:"work_type"`
	Domain    string `yaml:"domain"`
}

// taskMeta holds parsed YAML frontmatter from a task markdown file.
type taskMeta struct {
	Status string `yaml:"status"`
}

// taskCounts holds aggregated counts by status.
type taskCounts struct {
	Todo       int
	InProgress int
	Complete   int
	Blocked    int
}

func (c taskCounts) total() int {
	return c.Todo + c.InProgress + c.Complete + c.Blocked
}

// epicRecord holds all data for one epic, used during report generation.
type epicRecord struct {
	Area     string
	ID       string
	Meta     epicMeta
	Tasks    taskCounts
	FilePath string
}

// areaRecord holds aggregate counts for an area directory.
type areaRecord struct {
	Folder  string
	Total   int
	Active  int
	Ongoing int
}

// Generate produces a tracking report for a specific epic.
// It returns the markdown content as a string.
func (t *EpicTracker) Generate(epicID string) (string, error) {
	if t.EpicsDir == "" {
		return "", fmt.Errorf("report: EpicsDir is required")
	}

	// Find the epic directory across all area folders.
	entries, err := os.ReadDir(t.EpicsDir)
	if err != nil {
		return "", fmt.Errorf("reading epics dir: %w", err)
	}

	for _, areaEntry := range entries {
		if !areaEntry.IsDir() {
			continue
		}
		epicDir := filepath.Join(t.EpicsDir, areaEntry.Name(), epicID)
		if _, err := os.Stat(epicDir); err != nil {
			continue
		}

		rec, err := t.scanEpic(areaEntry.Name(), epicDir)
		if err != nil {
			return "", err
		}
		if rec == nil {
			continue
		}

		return t.formatSingleEpic(rec), nil
	}

	return "", fmt.Errorf("report: epic %q not found", epicID)
}

// GenerateAll produces a full tracking report for all epics.
// It returns the markdown content as a string.
func (t *EpicTracker) GenerateAll() (string, error) {
	if t.EpicsDir == "" {
		return "", fmt.Errorf("report: EpicsDir is required")
	}

	if _, err := os.Stat(t.EpicsDir); err != nil {
		return t.formatEmpty(), nil
	}

	var (
		totalEpics     int
		activeEpics    int
		totalTasks     int
		completedTasks int
		epicRecords    []epicRecord
		areaRecords    []areaRecord
	)

	areaEntries, err := os.ReadDir(t.EpicsDir)
	if err != nil {
		return "", fmt.Errorf("reading epics dir: %w", err)
	}

	for _, areaEntry := range areaEntries {
		if !areaEntry.IsDir() {
			continue
		}
		areaFolder := areaEntry.Name()
		areaDir := filepath.Join(t.EpicsDir, areaFolder)

		var localCount, localActive, localOngoing int

		epicEntries, err := os.ReadDir(areaDir)
		if err != nil {
			continue
		}

		for _, epicEntry := range epicEntries {
			if !epicEntry.IsDir() {
				continue
			}
			epicDir := filepath.Join(areaDir, epicEntry.Name())

			rec, err := t.scanEpic(areaFolder, epicDir)
			if err != nil || rec == nil {
				continue
			}

			totalEpics++
			localCount++

			if rec.Meta.Status != "complete" && rec.Meta.Status != "archived" {
				activeEpics++
				localActive++
			}
			if rec.Meta.IsOngoing {
				localOngoing++
			}

			totalTasks += rec.Tasks.total()
			completedTasks += rec.Tasks.Complete

			epicRecords = append(epicRecords, *rec)
		}

		if localCount > 0 {
			areaRecords = append(areaRecords, areaRecord{
				Folder:  areaFolder,
				Total:   localCount,
				Active:  localActive,
				Ongoing: localOngoing,
			})
		}
	}

	return t.formatFull(totalEpics, activeEpics, totalTasks, completedTasks, epicRecords, areaRecords), nil
}

// WriteReport generates the full report and writes it to the output file.
func (t *EpicTracker) WriteReport() (string, error) {
	content, err := t.GenerateAll()
	if err != nil {
		return "", err
	}

	outputPath := t.outputPath()
	if err := os.MkdirAll(filepath.Dir(outputPath), 0o755); err != nil {
		return "", fmt.Errorf("creating output dir: %w", err)
	}

	if err := os.WriteFile(outputPath, []byte(content), 0o644); err != nil {
		return "", fmt.Errorf("writing report: %w", err)
	}

	return outputPath, nil
}

func (t *EpicTracker) outputPath() string {
	if t.OutputDir != "" {
		return filepath.Join(t.OutputDir, "epic-tracker.md")
	}
	// Default: sibling tracking/ directory.
	return filepath.Join(filepath.Dir(t.EpicsDir), "tracking", "epic-tracker.md")
}

// scanEpic reads an epic directory and returns a record, or nil if no epic file found.
func (t *EpicTracker) scanEpic(area, epicDir string) (*epicRecord, error) {
	epicID := filepath.Base(epicDir)

	// Find epic file ({EPIC-ID}-epic.md pattern).
	var epicFilePath string
	entries, err := os.ReadDir(epicDir)
	if err != nil {
		return nil, nil
	}
	for _, e := range entries {
		if !e.IsDir() && strings.HasSuffix(e.Name(), "-epic.md") {
			epicFilePath = filepath.Join(epicDir, e.Name())
			break
		}
	}
	if epicFilePath == "" {
		return nil, nil
	}

	// Parse epic frontmatter.
	meta, err := parseFrontmatter[epicMeta](epicFilePath)
	if err != nil {
		return nil, fmt.Errorf("parsing epic %s: %w", epicFilePath, err)
	}

	// Count tasks.
	tasksDir := filepath.Join(epicDir, "tasks")
	counts := countTasksByStatus(tasksDir)

	return &epicRecord{
		Area:     area,
		ID:       epicID,
		Meta:     meta,
		Tasks:    counts,
		FilePath: epicFilePath,
	}, nil
}

// countTasksByStatus counts tasks in a directory by their frontmatter status.
func countTasksByStatus(tasksDir string) taskCounts {
	var counts taskCounts

	entries, err := os.ReadDir(tasksDir)
	if err != nil {
		return counts
	}

	for _, e := range entries {
		if e.IsDir() || !strings.HasSuffix(e.Name(), ".md") {
			continue
		}
		meta, err := parseFrontmatter[taskMeta](filepath.Join(tasksDir, e.Name()))
		if err != nil {
			continue
		}
		switch meta.Status {
		case "todo", "":
			counts.Todo++
		case "in_progress":
			counts.InProgress++
		case "complete", "completed", "done":
			counts.Complete++
		case "blocked":
			counts.Blocked++
		}
	}

	return counts
}

// parseFrontmatter reads a markdown file and parses its YAML frontmatter into T.
func parseFrontmatter[T any](path string) (T, error) {
	var zero T

	data, err := os.ReadFile(path)
	if err != nil {
		return zero, err
	}

	content := string(data)

	// Skip BOM.
	content = strings.TrimPrefix(content, "\xEF\xBB\xBF")

	// Must start with ---.
	if !strings.HasPrefix(content, "---") {
		return zero, fmt.Errorf("missing opening delimiter")
	}

	// Find closing ---.
	rest := content[strings.Index(content, "\n")+1:]
	closingIdx := -1
	pos := 0
	for pos < len(rest) {
		lineEnd := strings.Index(rest[pos:], "\n")
		var line string
		if lineEnd < 0 {
			line = rest[pos:]
		} else {
			line = rest[pos : pos+lineEnd]
		}
		if strings.TrimRight(line, " \t\r") == "---" {
			closingIdx = pos
			break
		}
		if lineEnd < 0 {
			break
		}
		pos += lineEnd + 1
	}

	if closingIdx < 0 {
		return zero, fmt.Errorf("missing closing delimiter")
	}

	yamlStr := rest[:closingIdx]

	var result T
	if err := yaml.Unmarshal([]byte(yamlStr), &result); err != nil {
		return zero, fmt.Errorf("parsing YAML: %w", err)
	}

	return result, nil
}

// formatEmpty returns a minimal report when no epics exist.
func (t *EpicTracker) formatEmpty() string {
	return t.formatFull(0, 0, 0, 0, nil, nil)
}

// formatSingleEpic formats a report for a single epic.
func (t *EpicTracker) formatSingleEpic(rec *epicRecord) string {
	var b strings.Builder

	fmt.Fprintf(&b, "# Epic Report: %s\n\n", rec.ID)
	fmt.Fprintf(&b, "**Title:** %s\n", rec.Meta.Title)
	fmt.Fprintf(&b, "**Status:** %s\n", rec.Meta.Status)
	fmt.Fprintf(&b, "**Area:** %s | **Type:** %s | **Domain:** %s\n",
		orDefault(rec.Meta.AreaType, "?"),
		orDefault(rec.Meta.WorkType, "?"),
		orDefault(rec.Meta.Domain, "?"))
	fmt.Fprintf(&b, "**Priority:** %s\n", orDefault(rec.Meta.Priority, "normal"))
	fmt.Fprintf(&b, "**Tasks:** %d todo, %d in_progress, %d complete, %d blocked\n",
		rec.Tasks.Todo, rec.Tasks.InProgress, rec.Tasks.Complete, rec.Tasks.Blocked)
	fmt.Fprintf(&b, "**Path:** `project-management/epics/%s/%s/`\n", rec.Area, rec.ID)

	return b.String()
}

// formatFull generates the complete tracking markdown.
func (t *EpicTracker) formatFull(totalEpics, activeEpics, totalTasks, completedTasks int, epics []epicRecord, areas []areaRecord) string {
	var b strings.Builder

	// Frontmatter.
	now := time.Now().UTC().Format("2006-01-02T15:04:05Z")
	fmt.Fprintln(&b, "---")
	fmt.Fprintln(&b, "# GENERATED — DO NOT EDIT")
	fmt.Fprintln(&b, "# Regenerate with: codeflow report epic-tracker")
	fmt.Fprintf(&b, "generated_at: %s\n", now)
	fmt.Fprintf(&b, "total_epics: %d\n", totalEpics)
	fmt.Fprintf(&b, "active_epics: %d\n", activeEpics)
	fmt.Fprintf(&b, "total_tasks: %d\n", totalTasks)
	fmt.Fprintf(&b, "completed_tasks: %d\n", completedTasks)
	fmt.Fprintln(&b, "source: filesystem scan")
	fmt.Fprintln(&b, "---")
	fmt.Fprintln(&b)

	// Title and summary.
	fmt.Fprintln(&b, "# Epic Tracker")
	fmt.Fprintln(&b)
	fmt.Fprintln(&b, "## Summary")
	fmt.Fprintln(&b)
	fmt.Fprintln(&b, "| Metric | Count |")
	fmt.Fprintln(&b, "|--------|-------|")
	fmt.Fprintf(&b, "| Total Epics | %d |\n", totalEpics)
	fmt.Fprintf(&b, "| Active Epics | %d |\n", activeEpics)
	fmt.Fprintf(&b, "| Total Tasks | %d |\n", totalTasks)
	fmt.Fprintf(&b, "| Completed Tasks | %d |\n", completedTasks)
	fmt.Fprintln(&b)

	// Epics by area.
	fmt.Fprintln(&b, "## Epics by Area")
	fmt.Fprintln(&b)
	fmt.Fprintln(&b, "| Area | Total | Active | Ongoing |")
	fmt.Fprintln(&b, "|------|-------|--------|---------|")
	for _, a := range areas {
		fmt.Fprintf(&b, "| %s | %d | %d | %d |\n", a.Folder, a.Total, a.Active, a.Ongoing)
	}
	fmt.Fprintln(&b)

	// Active epics.
	fmt.Fprintln(&b, "## Active Epics")
	fmt.Fprintln(&b)
	hasActive := false
	for _, rec := range epics {
		if rec.Meta.Status == "complete" || rec.Meta.Status == "archived" {
			continue
		}
		hasActive = true
		fmt.Fprintf(&b, "### %s: %s\n\n", rec.ID, rec.Meta.Title)
		fmt.Fprintf(&b, "- **Status:** %s\n", rec.Meta.Status)
		fmt.Fprintf(&b, "- **Area:** %s | **Type:** %s | **Domain:** %s\n",
			orDefault(rec.Meta.AreaType, "?"),
			orDefault(rec.Meta.WorkType, "?"),
			orDefault(rec.Meta.Domain, "?"))
		fmt.Fprintf(&b, "- **Priority:** %s | **Ongoing:** %t\n", orDefault(rec.Meta.Priority, "normal"), rec.Meta.IsOngoing)
		fmt.Fprintf(&b, "- **Tasks:** %d todo, %d in_progress, %d complete, %d blocked\n",
			rec.Tasks.Todo, rec.Tasks.InProgress, rec.Tasks.Complete, rec.Tasks.Blocked)
		fmt.Fprintf(&b, "- **Path:** `project-management/epics/%s/%s/`\n\n", rec.Area, rec.ID)
	}
	if !hasActive {
		fmt.Fprintln(&b, "(none)")
		fmt.Fprintln(&b)
	}

	// Completed epics.
	fmt.Fprintln(&b, "## Completed Epics")
	fmt.Fprintln(&b)
	hasCompleted := false
	for _, rec := range epics {
		if rec.Meta.Status != "complete" && rec.Meta.Status != "archived" {
			continue
		}
		hasCompleted = true
		fmt.Fprintf(&b, "- **%s**: %s (%s)\n", rec.ID, rec.Meta.Title, rec.Meta.Status)
	}
	if !hasCompleted {
		fmt.Fprintln(&b, "(none)")
	}

	return b.String()
}

func orDefault(val, def string) string {
	if val == "" {
		return def
	}
	return val
}
