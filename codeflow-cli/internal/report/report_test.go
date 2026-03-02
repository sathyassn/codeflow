package report

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// createEpicFixture creates an epic directory with an epic markdown file and optional tasks.
func createEpicFixture(t *testing.T, epicsDir, area, epicID string, epicYAML string, tasks map[string]string) string {
	t.Helper()
	epicDir := filepath.Join(epicsDir, area, epicID)
	if err := os.MkdirAll(epicDir, 0o755); err != nil {
		t.Fatal(err)
	}

	epicFile := filepath.Join(epicDir, epicID+"-epic.md")
	content := "---\n" + epicYAML + "\n---\n\n# " + epicID + "\n"
	if err := os.WriteFile(epicFile, []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}

	if len(tasks) > 0 {
		tasksDir := filepath.Join(epicDir, "tasks")
		if err := os.MkdirAll(tasksDir, 0o755); err != nil {
			t.Fatal(err)
		}
		for name, yaml := range tasks {
			taskContent := "---\n" + yaml + "\n---\n\n# " + name + "\n"
			if err := os.WriteFile(filepath.Join(tasksDir, name+".md"), []byte(taskContent), 0o644); err != nil {
				t.Fatal(err)
			}
		}
	}

	return epicDir
}

func TestGenerate_SingleEpic(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	epicsDir := filepath.Join(tmpDir, "epics")

	createEpicFixture(t, epicsDir, "INF", "INF-EPC-001", `title: Test Epic
status: active
priority: high
area_type: infrastructure
work_type: FEAT
domain: testing`, map[string]string{
		"INF-TSK-001-001": "status: todo",
		"INF-TSK-001-002": "status: in_progress",
		"INF-TSK-001-003": "status: complete",
	})

	tracker := &EpicTracker{EpicsDir: epicsDir}
	content, err := tracker.Generate("INF-EPC-001")
	if err != nil {
		t.Fatalf("Generate failed: %v", err)
	}

	// Verify key output fields.
	if !strings.Contains(content, "# Epic Report: INF-EPC-001") {
		t.Error("output missing epic ID header")
	}
	if !strings.Contains(content, "**Title:** Test Epic") {
		t.Error("output missing title")
	}
	if !strings.Contains(content, "**Status:** active") {
		t.Error("output missing status")
	}
	if !strings.Contains(content, "1 todo") {
		t.Error("output missing todo count")
	}
	if !strings.Contains(content, "1 in_progress") {
		t.Error("output missing in_progress count")
	}
	if !strings.Contains(content, "1 complete") {
		t.Error("output missing complete count")
	}
}

func TestGenerate_EpicNotFound(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	epicsDir := filepath.Join(tmpDir, "epics")
	if err := os.MkdirAll(filepath.Join(epicsDir, "INF"), 0o755); err != nil {
		t.Fatal(err)
	}

	tracker := &EpicTracker{EpicsDir: epicsDir}
	_, err := tracker.Generate("NONEXISTENT")
	if err == nil {
		t.Fatal("expected error for nonexistent epic")
	}
	if !strings.Contains(err.Error(), "not found") {
		t.Errorf("expected 'not found' error, got: %v", err)
	}
}

func TestGenerate_EmptyEpicsDir(t *testing.T) {
	t.Parallel()

	tracker := &EpicTracker{EpicsDir: ""}
	_, err := tracker.Generate("anything")
	if err == nil {
		t.Fatal("expected error for empty EpicsDir")
	}
	if !strings.Contains(err.Error(), "EpicsDir is required") {
		t.Errorf("expected 'EpicsDir is required' error, got: %v", err)
	}
}

func TestGenerateAll_MultipleEpics(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	epicsDir := filepath.Join(tmpDir, "epics")

	createEpicFixture(t, epicsDir, "INF", "INF-EPC-001", `title: Infrastructure Epic
status: active
priority: high
is_ongoing: true
area_type: infrastructure
work_type: FEAT
domain: infra`, map[string]string{
		"task1": "status: complete",
		"task2": "status: todo",
	})

	createEpicFixture(t, epicsDir, "APP", "APP-EPC-001", `title: App Epic
status: complete
priority: normal
area_type: application
work_type: FIX
domain: app`, map[string]string{
		"task1": "status: complete",
	})

	tracker := &EpicTracker{EpicsDir: epicsDir}
	content, err := tracker.GenerateAll()
	if err != nil {
		t.Fatalf("GenerateAll failed: %v", err)
	}

	// Verify summary counts.
	if !strings.Contains(content, "| Total Epics | 2 |") {
		t.Error("output missing total epics count of 2")
	}
	if !strings.Contains(content, "| Active Epics | 1 |") {
		t.Error("output missing active epics count of 1")
	}
	if !strings.Contains(content, "| Total Tasks | 3 |") {
		t.Error("output missing total tasks count of 3")
	}
	if !strings.Contains(content, "| Completed Tasks | 2 |") {
		t.Error("output missing completed tasks count of 2")
	}

	// Verify area section.
	if !strings.Contains(content, "## Epics by Area") {
		t.Error("output missing area section")
	}

	// Verify active section has INF epic.
	if !strings.Contains(content, "### INF-EPC-001: Infrastructure Epic") {
		t.Error("output missing active epic heading")
	}

	// Verify completed section has APP epic.
	if !strings.Contains(content, "**APP-EPC-001**: App Epic (complete)") {
		t.Error("output missing completed epic entry")
	}
}

func TestGenerateAll_NoEpicsDir(t *testing.T) {
	t.Parallel()

	tracker := &EpicTracker{EpicsDir: filepath.Join(t.TempDir(), "nonexistent")}
	content, err := tracker.GenerateAll()
	if err != nil {
		t.Fatalf("GenerateAll failed: %v", err)
	}

	// Should produce empty report.
	if !strings.Contains(content, "| Total Epics | 0 |") {
		t.Error("expected zero epics in empty report")
	}
}

func TestGenerateAll_EmptyEpicsDir(t *testing.T) {
	t.Parallel()

	tracker := &EpicTracker{EpicsDir: ""}
	_, err := tracker.GenerateAll()
	if err == nil {
		t.Fatal("expected error for empty EpicsDir")
	}
}

func TestWriteReport(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	epicsDir := filepath.Join(tmpDir, "epics")
	outputDir := filepath.Join(tmpDir, "output")

	createEpicFixture(t, epicsDir, "INF", "INF-EPC-001", `title: Write Test
status: active`, nil)

	tracker := &EpicTracker{
		EpicsDir:  epicsDir,
		OutputDir: outputDir,
	}

	outputPath, err := tracker.WriteReport()
	if err != nil {
		t.Fatalf("WriteReport failed: %v", err)
	}

	expected := filepath.Join(outputDir, "epic-tracker.md")
	if outputPath != expected {
		t.Errorf("expected output path %s, got %s", expected, outputPath)
	}

	data, err := os.ReadFile(outputPath)
	if err != nil {
		t.Fatalf("reading output: %v", err)
	}

	content := string(data)
	if !strings.Contains(content, "# Epic Tracker") {
		t.Error("written file missing Epic Tracker heading")
	}
	if !strings.Contains(content, "Write Test") {
		t.Error("written file missing epic title")
	}
}

func TestWriteReport_DefaultOutputPath(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	pmDir := filepath.Join(tmpDir, "project-management")
	epicsDir := filepath.Join(pmDir, "epics")

	createEpicFixture(t, epicsDir, "INF", "INF-EPC-001", `title: Default Path
status: active`, nil)

	tracker := &EpicTracker{EpicsDir: epicsDir}
	outputPath, err := tracker.WriteReport()
	if err != nil {
		t.Fatalf("WriteReport failed: %v", err)
	}

	expected := filepath.Join(pmDir, "tracking", "epic-tracker.md")
	if outputPath != expected {
		t.Errorf("expected default output path %s, got %s", expected, outputPath)
	}
}

func TestParseFrontmatter_Valid(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	f := filepath.Join(tmpDir, "test.md")
	if err := os.WriteFile(f, []byte("---\ntitle: Hello\nstatus: active\n---\n\n# Content\n"), 0o644); err != nil {
		t.Fatal(err)
	}

	meta, err := parseFrontmatter[epicMeta](f)
	if err != nil {
		t.Fatalf("parseFrontmatter failed: %v", err)
	}
	if meta.Title != "Hello" {
		t.Errorf("expected title Hello, got %s", meta.Title)
	}
	if meta.Status != "active" {
		t.Errorf("expected status active, got %s", meta.Status)
	}
}

func TestParseFrontmatter_MissingOpening(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	f := filepath.Join(tmpDir, "test.md")
	if err := os.WriteFile(f, []byte("no frontmatter here"), 0o644); err != nil {
		t.Fatal(err)
	}

	_, err := parseFrontmatter[epicMeta](f)
	if err == nil {
		t.Fatal("expected error for missing opening delimiter")
	}
	if !strings.Contains(err.Error(), "missing opening delimiter") {
		t.Errorf("unexpected error: %v", err)
	}
}

func TestParseFrontmatter_MissingClosing(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	f := filepath.Join(tmpDir, "test.md")
	if err := os.WriteFile(f, []byte("---\ntitle: Hello\nstatus: active\n"), 0o644); err != nil {
		t.Fatal(err)
	}

	_, err := parseFrontmatter[epicMeta](f)
	if err == nil {
		t.Fatal("expected error for missing closing delimiter")
	}
	if !strings.Contains(err.Error(), "missing closing delimiter") {
		t.Errorf("unexpected error: %v", err)
	}
}

func TestParseFrontmatter_BOM(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	f := filepath.Join(tmpDir, "test.md")
	// Write with BOM prefix.
	content := "\xEF\xBB\xBF---\ntitle: BOM Test\n---\n\n# Content\n"
	if err := os.WriteFile(f, []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}

	meta, err := parseFrontmatter[epicMeta](f)
	if err != nil {
		t.Fatalf("parseFrontmatter with BOM failed: %v", err)
	}
	if meta.Title != "BOM Test" {
		t.Errorf("expected title 'BOM Test', got %s", meta.Title)
	}
}

func TestParseFrontmatter_FileNotFound(t *testing.T) {
	t.Parallel()

	_, err := parseFrontmatter[epicMeta](filepath.Join(t.TempDir(), "nonexistent.md"))
	if err == nil {
		t.Fatal("expected error for nonexistent file")
	}
}

func TestCountTasksByStatus(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	tasksDir := filepath.Join(tmpDir, "tasks")
	if err := os.MkdirAll(tasksDir, 0o755); err != nil {
		t.Fatal(err)
	}

	taskFiles := map[string]string{
		"task-todo.md":        "---\nstatus: todo\n---\n",
		"task-empty.md":       "---\nstatus: \"\"\n---\n",
		"task-inprogress.md":  "---\nstatus: in_progress\n---\n",
		"task-complete.md":    "---\nstatus: complete\n---\n",
		"task-completed.md":   "---\nstatus: completed\n---\n",
		"task-done.md":        "---\nstatus: done\n---\n",
		"task-blocked.md":     "---\nstatus: blocked\n---\n",
		"not-a-task.txt":      "not markdown",
	}
	for name, content := range taskFiles {
		if err := os.WriteFile(filepath.Join(tasksDir, name), []byte(content), 0o644); err != nil {
			t.Fatal(err)
		}
	}

	counts := countTasksByStatus(tasksDir)

	if counts.Todo != 2 { // "todo" + empty status
		t.Errorf("expected 2 todo, got %d", counts.Todo)
	}
	if counts.InProgress != 1 {
		t.Errorf("expected 1 in_progress, got %d", counts.InProgress)
	}
	if counts.Complete != 3 { // "complete" + "completed" + "done"
		t.Errorf("expected 3 complete, got %d", counts.Complete)
	}
	if counts.Blocked != 1 {
		t.Errorf("expected 1 blocked, got %d", counts.Blocked)
	}
	if counts.total() != 7 {
		t.Errorf("expected total 7, got %d", counts.total())
	}
}

func TestCountTasksByStatus_NoDir(t *testing.T) {
	t.Parallel()

	counts := countTasksByStatus(filepath.Join(t.TempDir(), "nonexistent"))
	if counts.total() != 0 {
		t.Errorf("expected total 0 for nonexistent dir, got %d", counts.total())
	}
}

func TestScanEpic_NoEpicFile(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	epicDir := filepath.Join(tmpDir, "epics", "INF", "INF-EPC-999")
	if err := os.MkdirAll(epicDir, 0o755); err != nil {
		t.Fatal(err)
	}
	// Create a non-epic file.
	if err := os.WriteFile(filepath.Join(epicDir, "readme.md"), []byte("# README"), 0o644); err != nil {
		t.Fatal(err)
	}

	tracker := &EpicTracker{EpicsDir: filepath.Join(tmpDir, "epics")}
	rec, err := tracker.scanEpic("INF", epicDir)
	if err != nil {
		t.Fatalf("scanEpic failed: %v", err)
	}
	if rec != nil {
		t.Error("expected nil record for directory without epic file")
	}
}

func TestOrDefault(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name     string
		val      string
		def      string
		expected string
	}{
		{"empty value", "", "fallback", "fallback"},
		{"non-empty value", "actual", "fallback", "actual"},
		{"both empty", "", "", ""},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			got := orDefault(tc.val, tc.def)
			if got != tc.expected {
				t.Errorf("orDefault(%q, %q) = %q, want %q", tc.val, tc.def, got, tc.expected)
			}
		})
	}
}

func TestFormatFull_Structure(t *testing.T) {
	t.Parallel()

	tracker := &EpicTracker{EpicsDir: "/fake"}
	content := tracker.formatFull(2, 1, 5, 3,
		[]epicRecord{
			{
				Area: "INF", ID: "INF-EPC-001",
				Meta: epicMeta{Title: "Active One", Status: "active", Priority: "high"},
				Tasks: taskCounts{Todo: 1, Complete: 1},
			},
			{
				Area: "APP", ID: "APP-EPC-001",
				Meta: epicMeta{Title: "Done One", Status: "complete"},
				Tasks: taskCounts{Complete: 3},
			},
		},
		[]areaRecord{
			{Folder: "INF", Total: 1, Active: 1, Ongoing: 0},
			{Folder: "APP", Total: 1, Active: 0, Ongoing: 0},
		},
	)

	// Verify frontmatter.
	if !strings.Contains(content, "---") {
		t.Error("missing frontmatter delimiters")
	}
	if !strings.Contains(content, "total_epics: 2") {
		t.Error("missing total_epics in frontmatter")
	}

	// Verify summary table.
	if !strings.Contains(content, "| Total Epics | 2 |") {
		t.Error("missing summary total epics")
	}

	// Verify area table.
	if !strings.Contains(content, "| INF | 1 | 1 | 0 |") {
		t.Error("missing INF area row")
	}

	// Verify active section.
	if !strings.Contains(content, "### INF-EPC-001: Active One") {
		t.Error("missing active epic heading")
	}

	// Verify completed section.
	if !strings.Contains(content, "**APP-EPC-001**: Done One (complete)") {
		t.Error("missing completed epic entry")
	}
}

func TestFormatFull_Empty(t *testing.T) {
	t.Parallel()

	tracker := &EpicTracker{EpicsDir: "/fake"}
	content := tracker.formatFull(0, 0, 0, 0, nil, nil)

	if !strings.Contains(content, "| Total Epics | 0 |") {
		t.Error("empty report missing zero counts")
	}
	if !strings.Contains(content, "(none)") {
		t.Error("empty report missing (none) markers")
	}
}

func TestGenerateAll_OngoingEpic(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	epicsDir := filepath.Join(tmpDir, "epics")

	createEpicFixture(t, epicsDir, "INF", "INF-EPC-001", `title: Ongoing Epic
status: active
is_ongoing: true`, nil)

	tracker := &EpicTracker{EpicsDir: epicsDir}
	content, err := tracker.GenerateAll()
	if err != nil {
		t.Fatalf("GenerateAll failed: %v", err)
	}

	if !strings.Contains(content, "**Ongoing:** true") {
		t.Error("output missing ongoing flag")
	}
}
