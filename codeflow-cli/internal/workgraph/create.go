package workgraph

import (
	"context"
	"encoding/json"
	"fmt"
	"io"

	"github.com/codeflow/codeflow-cli/internal/idgen"
	"github.com/codeflow/codeflow-cli/internal/ledger"
)

// CreateEpicResult is the JSON output of the create-epic command.
type CreateEpicResult struct {
	ID       string `json:"id"`
	FormatID string `json:"format_id"`
	Title    string `json:"title"`
}

// CreateEpic generates dual IDs, inserts into the epics table, appends an
// epic_created JSONL event, and returns the result. The DB insert and JSONL
// append are NOT wrapped in a single DB transaction because the ledger is
// an independent file — instead, the DB write happens first, and if the JSONL
// write fails, the DB row remains (acceptable: db sync can rebuild from JSONL,
// and having extra DB rows is harmless).
func (s *Service) CreateEpic(ctx context.Context, data string, w io.Writer) error {
	input, err := parseInput(data)
	if err != nil {
		return err
	}

	// Required fields.
	title, err := requireString(input, "title")
	if err != nil {
		return err
	}
	areaType, err := requireString(input, "area_type")
	if err != nil {
		return err
	}
	workType, err := requireString(input, "work_type")
	if err != nil {
		return err
	}
	domain, err := requireString(input, "domain")
	if err != nil {
		return err
	}

	// Optional fields with defaults.
	status := getStringDefault(input, "status", "draft")
	if err := validateEnum("status", status, ValidEpicStatuses); err != nil {
		return err
	}
	priority := getStringDefault(input, "priority", "normal")
	if err := validateEnum("priority", priority, ValidPriorities); err != nil {
		return err
	}

	summary := getString(input, "summary")
	isOngoing := getBoolDefault(input, "is_ongoing", false)
	fileScope := getJSONArray(input, "file_scope")

	// Validate FK references.
	if err := validateRefs(ctx, s.DB, areaType, workType, domain); err != nil {
		return err
	}

	// Generate dual IDs.
	id := idgen.NewPrefixedULID("epic")
	querier := newQuerier(s.DB)
	seq, err := idgen.NextFormatID(ctx, querier, "epics", areaType)
	if err != nil {
		return fmt.Errorf("workgraph: generating format ID: %w", err)
	}
	formatID := idgen.FormatEpicID(areaType, seq)

	now := s.Now().Format("2006-01-02T15:04:05Z")

	// INSERT into epics table.
	_, err = s.DB.Execute(ctx,
		`INSERT INTO epics (id, format_id, title, summary, status, area_type, work_type, domain,
		 is_ongoing, file_scope, priority, created_at, updated_at)
		 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
		id, formatID, title, nullString(summary), status, areaType, workType, domain,
		isOngoing, nullString(fileScope), priority, now, now,
	)
	if err != nil {
		return fmt.Errorf("workgraph: inserting epic: %w", err)
	}

	// Append epic_created JSONL event.
	event := ledger.Event{
		EventType: "epic_created",
		Data: map[string]any{
			"id":        id,
			"format_id": formatID,
			"title":     title,
			"status":    status,
			"area_type": areaType,
			"work_type": workType,
			"domain":    domain,
			"is_ongoing": isOngoing,
			"file_scope": fileScope,
			"priority":  priority,
		},
	}
	if err := s.Writer.AppendEvent(event); err != nil {
		return fmt.Errorf("workgraph: appending epic_created event: %w", err)
	}

	// Output result.
	result := CreateEpicResult{
		ID:       id,
		FormatID: formatID,
		Title:    title,
	}
	out, err := json.MarshalIndent(result, "", "  ")
	if err != nil {
		return fmt.Errorf("workgraph: marshaling result: %w", err)
	}
	fmt.Fprintln(w, string(out))
	return nil
}

// CreateTaskResult is the JSON output of the create-task command.
type CreateTaskResult struct {
	ID       string `json:"id"`
	FormatID string `json:"format_id"`
	EpicID   string `json:"epic_id"`
	Title    string `json:"title"`
}

// CreateTask generates dual IDs, validates the parent epic exists, inserts into
// the tasks table, appends a task_created JSONL event, and returns the result.
func (s *Service) CreateTask(ctx context.Context, data string, w io.Writer) error {
	input, err := parseInput(data)
	if err != nil {
		return err
	}

	// Required fields.
	epicID, err := requireString(input, "epic_id")
	if err != nil {
		return err
	}
	title, err := requireString(input, "title")
	if err != nil {
		return err
	}
	areaType, err := requireString(input, "area_type")
	if err != nil {
		return err
	}
	workType, err := requireString(input, "work_type")
	if err != nil {
		return err
	}
	domain, err := requireString(input, "domain")
	if err != nil {
		return err
	}

	// Optional fields with defaults.
	status := getStringDefault(input, "status", "todo")
	if err := validateEnum("status", status, ValidTaskStatuses); err != nil {
		return err
	}
	priority := getStringDefault(input, "priority", "normal")
	if err := validateEnum("priority", priority, ValidPriorities); err != nil {
		return err
	}
	origin := getStringDefault(input, "origin", "planned")
	if err := validateEnum("origin", origin, ValidOrigins); err != nil {
		return err
	}
	scopePolicy := getStringDefault(input, "scope_policy", "soft")
	if err := validateEnum("scope_policy", scopePolicy, ValidScopePolicies); err != nil {
		return err
	}

	// Optional stage fields.
	if stage, present, err := validateOptionalEnum(input, "stage", ValidStages); err != nil {
		return err
	} else if present {
		_ = stage // used below
	}
	if stageStatus, present, err := validateOptionalEnum(input, "stage_status", ValidStageStatuses); err != nil {
		return err
	} else if present {
		_ = stageStatus
	}

	description := getString(input, "description")
	fileScope := getJSONArray(input, "file_scope")
	scopeRoot := getString(input, "scope_root")
	estimate := getString(input, "estimate")
	acceptance := getJSONArray(input, "acceptance")
	tests := getJSONArray(input, "tests")
	autorunEligible := getBoolDefault(input, "autorun_eligible", false)
	raisePR := getBoolDefault(input, "raise_pr", true)
	autoMerge := getBoolDefault(input, "auto_merge", false)
	targetBranch := getString(input, "target_branch")

	// Validate FK references.
	if err := validateRefs(ctx, s.DB, areaType, workType, domain); err != nil {
		return err
	}

	// Validate estimate FK if provided.
	if estimate != "" {
		if err := validateRefTable(ctx, s.DB, "estimate_types", "estimate", estimate); err != nil {
			return err
		}
	}

	// Verify epic_id exists and get its format_id.
	var epicFormatID string
	err = s.DB.QueryRow(ctx, "SELECT format_id FROM epics WHERE id = ?", epicID).Scan(&epicFormatID)
	if err != nil {
		return fmt.Errorf("%w: epic with id=%q", ErrNotFound, epicID)
	}

	// Parse epic sequence from format_id.
	epicSeq, err := idgen.ParseEpicSeq(epicFormatID)
	if err != nil {
		return fmt.Errorf("workgraph: parsing epic sequence: %w", err)
	}

	// Generate dual IDs.
	id := idgen.NewPrefixedULID("task")
	querier := newQuerier(s.DB)
	taskSeq, err := idgen.NextFormatID(ctx, querier, "tasks", areaType)
	if err != nil {
		return fmt.Errorf("workgraph: generating task format ID: %w", err)
	}
	formatID := idgen.FormatTaskID(areaType, epicSeq, taskSeq)

	now := s.Now().Format("2006-01-02T15:04:05Z")

	// INSERT into tasks table.
	_, err = s.DB.Execute(ctx,
		`INSERT INTO tasks (id, format_id, epic_id, title, description, status, area_type,
		 work_type, domain, origin, file_scope, scope_policy, scope_root, estimate, priority,
		 autorun_eligible, raise_pr, auto_merge, target_branch, acceptance, tests,
		 created_at, updated_at, stage, stage_status)
		 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
		id, formatID, epicID, title, nullString(description), status, areaType,
		workType, domain, origin, nullString(fileScope), scopePolicy,
		nullString(scopeRoot), nullString(estimate), priority,
		autorunEligible, raisePR, autoMerge, nullString(targetBranch),
		nullString(acceptance), nullString(tests),
		now, now,
		nullString(getString(input, "stage")), nullString(getString(input, "stage_status")),
	)
	if err != nil {
		return fmt.Errorf("workgraph: inserting task: %w", err)
	}

	// Append task_created JSONL event.
	event := ledger.Event{
		EventType: "task_created",
		Data: map[string]any{
			"id":              id,
			"format_id":       formatID,
			"epic_id":         epicID,
			"epic_format_id":  epicFormatID,
			"title":           title,
			"status":          status,
			"area_type":       areaType,
			"work_type":       workType,
			"domain":          domain,
			"origin":          origin,
			"priority":        priority,
		},
	}
	if err := s.Writer.AppendEvent(event); err != nil {
		return fmt.Errorf("workgraph: appending task_created event: %w", err)
	}

	// Output result.
	result := CreateTaskResult{
		ID:       id,
		FormatID: formatID,
		EpicID:   epicID,
		Title:    title,
	}
	out, err := json.MarshalIndent(result, "", "  ")
	if err != nil {
		return fmt.Errorf("workgraph: marshaling result: %w", err)
	}
	fmt.Fprintln(w, string(out))
	return nil
}

// nullString returns nil for empty strings (stored as SQL NULL).
func nullString(s string) any {
	if s == "" {
		return nil
	}
	return s
}
