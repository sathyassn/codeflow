package workgraph

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"strings"

	"github.com/codeflow/codeflow-cli/internal/ledger"
)

// UpdateEpicResult is the JSON output of the update-epic command.
type UpdateEpicResult struct {
	ID            string   `json:"id"`
	FormatID      string   `json:"format_id"`
	UpdatedFields []string `json:"updated_fields"`
}

// epicFieldSpec describes how to validate and store an epic field update.
type epicFieldSpec struct {
	column   string
	validate func(string) error
	isString bool // true for string columns, false for bool/int
}

// taskFieldSpec describes how to validate and store a task field update.
type taskFieldSpec struct {
	column   string
	validate func(string) error
	isString bool
	isBool   bool
}

// UpdateEpic resolves an epic by id or format_id, updates the specified fields,
// and appends an epic_status_changed JSONL event if the status changed.
func (s *Service) UpdateEpic(ctx context.Context, data string, w io.Writer) error {
	input, err := parseInput(data)
	if err != nil {
		return err
	}

	epicID, epicFormatID, oldStatus, err := resolveEpic(ctx, s.DB, input)
	if err != nil {
		return err
	}

	// Build dynamic SET clause from provided fields.
	specs := map[string]epicFieldSpec{
		"title":        {column: "title", isString: true},
		"summary":      {column: "summary", isString: true},
		"status":       {column: "status", validate: func(v string) error { return validateEnum("status", v, ValidEpicStatuses) }, isString: true},
		"priority":     {column: "priority", validate: func(v string) error { return validateEnum("priority", v, ValidPriorities) }, isString: true},
		"is_ongoing":   {column: "is_ongoing"},
		"file_scope":   {column: "file_scope", isString: true},
		"pr_number":    {column: "pr_number"},
		"external_id":  {column: "external_id", isString: true},
		"external_url": {column: "external_url", isString: true},
	}

	var setClauses []string
	var args []any
	var updatedFields []string

	for field, spec := range specs {
		val, ok := input[field]
		if !ok {
			continue
		}

		if spec.validate != nil {
			if s, ok := val.(string); ok {
				if err := spec.validate(s); err != nil {
					return err
				}
			}
		}

		setClauses = append(setClauses, spec.column+" = ?")
		if spec.isString {
			if s, ok := val.(string); ok {
				args = append(args, nullString(s))
			} else {
				// Handle JSON arrays (file_scope).
				b, err := json.Marshal(val)
				if err != nil {
					return fmt.Errorf("workgraph: marshaling %s: %w", field, err)
				}
				args = append(args, string(b))
			}
		} else {
			args = append(args, val)
		}
		updatedFields = append(updatedFields, field)
	}

	if len(setClauses) == 0 {
		return fmt.Errorf("%w: no fields to update", ErrInvalidInput)
	}

	now := s.Now().Format("2006-01-02T15:04:05Z")
	setClauses = append(setClauses, "updated_at = ?")
	args = append(args, now)
	args = append(args, epicID)

	query := fmt.Sprintf("UPDATE epics SET %s WHERE id = ?", strings.Join(setClauses, ", "))
	if _, err := s.DB.Execute(ctx, query, args...); err != nil {
		return fmt.Errorf("workgraph: updating epic: %w", err)
	}

	// Append epic_status_changed event if status changed.
	newStatus := getString(input, "status")
	if newStatus != "" && newStatus != oldStatus {
		event := ledger.Event{
			EventType: "epic_status_changed",
			Data: map[string]any{
				"epic_id":    epicID,
				"old_status": oldStatus,
				"new_status": newStatus,
			},
		}
		if err := s.Writer.AppendEvent(event); err != nil {
			return fmt.Errorf("workgraph: appending epic_status_changed event: %w", err)
		}
	}

	result := UpdateEpicResult{
		ID:            epicID,
		FormatID:      epicFormatID,
		UpdatedFields: updatedFields,
	}
	out, err := json.MarshalIndent(result, "", "  ")
	if err != nil {
		return fmt.Errorf("workgraph: marshaling result: %w", err)
	}
	fmt.Fprintln(w, string(out))
	return nil
}

// UpdateTaskResult is the JSON output of the update-task command.
type UpdateTaskResult struct {
	ID            string   `json:"id"`
	FormatID      string   `json:"format_id"`
	UpdatedFields []string `json:"updated_fields"`
}

// UpdateTask resolves a task by id or format_id, updates the specified fields,
// sets started_at/completed_at on status transitions, and appends a
// task_status_changed JSONL event if the status changed.
func (s *Service) UpdateTask(ctx context.Context, data string, w io.Writer) error {
	input, err := parseInput(data)
	if err != nil {
		return err
	}

	taskID, taskFormatID, oldStatus, err := resolveTask(ctx, s.DB, input)
	if err != nil {
		return err
	}

	specs := map[string]taskFieldSpec{
		"title":            {column: "title", isString: true},
		"description":      {column: "description", isString: true},
		"status":           {column: "status", validate: func(v string) error { return validateEnum("status", v, ValidTaskStatuses) }, isString: true},
		"priority":         {column: "priority", validate: func(v string) error { return validateEnum("priority", v, ValidPriorities) }, isString: true},
		"file_scope":       {column: "file_scope", isString: true},
		"scope_policy":     {column: "scope_policy", validate: func(v string) error { return validateEnum("scope_policy", v, ValidScopePolicies) }, isString: true},
		"scope_root":       {column: "scope_root", isString: true},
		"estimate":         {column: "estimate", isString: true},
		"assignee_id":      {column: "assignee_id", isString: true},
		"autorun_eligible": {column: "autorun_eligible", isBool: true},
		"raise_pr":         {column: "raise_pr", isBool: true},
		"auto_merge":       {column: "auto_merge", isBool: true},
		"target_branch":    {column: "target_branch", isString: true},
		"acceptance":       {column: "acceptance", isString: true},
		"tests":            {column: "tests", isString: true},
		"branch":           {column: "branch", isString: true},
		"pr_number":        {column: "pr_number"},
		"external_id":      {column: "external_id", isString: true},
		"external_url":     {column: "external_url", isString: true},
		"stage":            {column: "stage", validate: func(v string) error { return validateEnum("stage", v, ValidStages) }, isString: true},
		"stage_status":     {column: "stage_status", validate: func(v string) error { return validateEnum("stage_status", v, ValidStageStatuses) }, isString: true},
	}

	var setClauses []string
	var args []any
	var updatedFields []string

	for field, spec := range specs {
		val, ok := input[field]
		if !ok {
			continue
		}

		if spec.validate != nil {
			if s, ok := val.(string); ok {
				if err := spec.validate(s); err != nil {
					return err
				}
			}
		}

		setClauses = append(setClauses, spec.column+" = ?")
		switch {
		case spec.isBool:
			args = append(args, getBool(input, field))
		case spec.isString:
			if s, ok := val.(string); ok {
				args = append(args, nullString(s))
			} else {
				b, err := json.Marshal(val)
				if err != nil {
					return fmt.Errorf("workgraph: marshaling %s: %w", field, err)
				}
				args = append(args, string(b))
			}
		default:
			args = append(args, val)
		}
		updatedFields = append(updatedFields, field)
	}

	// Validate estimate FK if provided.
	if estimate := getString(input, "estimate"); estimate != "" {
		if err := validateRefTable(ctx, s.DB, "estimate_types", "estimate", estimate); err != nil {
			return err
		}
	}

	if len(setClauses) == 0 {
		return fmt.Errorf("%w: no fields to update", ErrInvalidInput)
	}

	now := s.Now().Format("2006-01-02T15:04:05Z")
	setClauses = append(setClauses, "updated_at = ?")
	args = append(args, now)

	// Set started_at when transitioning to in_progress.
	newStatus := getString(input, "status")
	if newStatus == "in_progress" && oldStatus != "in_progress" {
		setClauses = append(setClauses, "started_at = ?")
		args = append(args, now)
	}
	// Set completed_at when transitioning to complete.
	if newStatus == "complete" && oldStatus != "complete" {
		setClauses = append(setClauses, "completed_at = ?")
		args = append(args, now)
	}

	args = append(args, taskID)

	query := fmt.Sprintf("UPDATE tasks SET %s WHERE id = ?", strings.Join(setClauses, ", "))
	if _, err := s.DB.Execute(ctx, query, args...); err != nil {
		return fmt.Errorf("workgraph: updating task: %w", err)
	}

	// Append task_status_changed event if status changed.
	// Uses "task_id" key per sync.go compatibility (not "id").
	if newStatus != "" && newStatus != oldStatus {
		event := ledger.Event{
			EventType: "task_status_changed",
			Data: map[string]any{
				"task_id":    taskID,
				"format_id":  taskFormatID,
				"old_status": oldStatus,
				"new_status": newStatus,
			},
		}
		if err := s.Writer.AppendEvent(event); err != nil {
			return fmt.Errorf("workgraph: appending task_status_changed event: %w", err)
		}
	}

	result := UpdateTaskResult{
		ID:            taskID,
		FormatID:      taskFormatID,
		UpdatedFields: updatedFields,
	}
	out, err := json.MarshalIndent(result, "", "  ")
	if err != nil {
		return fmt.Errorf("workgraph: marshaling result: %w", err)
	}
	fmt.Fprintln(w, string(out))
	return nil
}
