package workgraph

import (
	"context"
	"fmt"
	"io"
	"strings"
)

// Valid query modes.
var validModes = map[string]bool{
	"get-epic":   true,
	"get-task":   true,
	"list-epics": true,
	"list-tasks": true,
}

// QueryParams holds the parsed flags for the query command.
type QueryParams struct {
	Mode     string
	ID       string
	FormatID string
	EpicID   string
	Area     string
	Status   string
	Domain   string
	WorkType string
	Limit    int
}

// Query executes a workgraph query based on the mode and filters.
func (s *Service) Query(ctx context.Context, params QueryParams, w io.Writer) error {
	if !validModes[params.Mode] {
		return fmt.Errorf("%w: invalid mode %q (valid: get-epic, get-task, list-epics, list-tasks)", ErrInvalidInput, params.Mode)
	}

	switch params.Mode {
	case "get-epic":
		return s.getEpic(ctx, params, w)
	case "get-task":
		return s.getTask(ctx, params, w)
	case "list-epics":
		return s.listEpics(ctx, params, w)
	case "list-tasks":
		return s.listTasks(ctx, params, w)
	default:
		return fmt.Errorf("%w: unknown mode %q", ErrInvalidInput, params.Mode)
	}
}

func (s *Service) getEpic(ctx context.Context, params QueryParams, w io.Writer) error {
	if params.ID == "" && params.FormatID == "" {
		return fmt.Errorf("%w: get-epic requires --id or --format-id", ErrInvalidInput)
	}

	var query string
	var arg string
	if params.ID != "" {
		query = "SELECT * FROM epics WHERE id = ?"
		arg = params.ID
	} else {
		query = "SELECT * FROM epics WHERE format_id = ?"
		arg = params.FormatID
	}

	data, err := s.DB.QueryToJSON(ctx, query, arg)
	if err != nil {
		return fmt.Errorf("workgraph: querying epic: %w", err)
	}

	// QueryToJSON returns a JSON array; for get mode, unwrap to single object.
	result, err := unwrapSingle(data, "epic", params.ID, params.FormatID)
	if err != nil {
		return err
	}
	fmt.Fprintln(w, string(result))
	return nil
}

func (s *Service) getTask(ctx context.Context, params QueryParams, w io.Writer) error {
	if params.ID == "" && params.FormatID == "" {
		return fmt.Errorf("%w: get-task requires --id or --format-id", ErrInvalidInput)
	}

	var query string
	var arg string
	if params.ID != "" {
		query = "SELECT * FROM tasks WHERE id = ?"
		arg = params.ID
	} else {
		query = "SELECT * FROM tasks WHERE format_id = ?"
		arg = params.FormatID
	}

	data, err := s.DB.QueryToJSON(ctx, query, arg)
	if err != nil {
		return fmt.Errorf("workgraph: querying task: %w", err)
	}

	result, err := unwrapSingle(data, "task", params.ID, params.FormatID)
	if err != nil {
		return err
	}
	fmt.Fprintln(w, string(result))
	return nil
}

func (s *Service) listEpics(ctx context.Context, params QueryParams, w io.Writer) error {
	query := "SELECT * FROM epics"
	var conditions []string
	var args []any

	if params.Area != "" {
		conditions = append(conditions, "area_type = ?")
		args = append(args, params.Area)
	}
	if params.Status != "" {
		conditions = append(conditions, "status = ?")
		args = append(args, params.Status)
	}
	if params.Domain != "" {
		conditions = append(conditions, "domain = ?")
		args = append(args, params.Domain)
	}
	if params.WorkType != "" {
		conditions = append(conditions, "work_type = ?")
		args = append(args, params.WorkType)
	}
	if len(conditions) > 0 {
		query += " WHERE " + strings.Join(conditions, " AND ")
	}
	query += " ORDER BY created_at DESC"
	if params.Limit > 0 {
		query += fmt.Sprintf(" LIMIT %d", params.Limit)
	}

	data, err := s.DB.QueryToJSON(ctx, query, args...)
	if err != nil {
		return fmt.Errorf("workgraph: listing epics: %w", err)
	}
	fmt.Fprintln(w, string(data))
	return nil
}

func (s *Service) listTasks(ctx context.Context, params QueryParams, w io.Writer) error {
	query := "SELECT * FROM tasks"
	var conditions []string
	var args []any

	if params.EpicID != "" {
		conditions = append(conditions, "epic_id = ?")
		args = append(args, params.EpicID)
	}
	if params.Area != "" {
		conditions = append(conditions, "area_type = ?")
		args = append(args, params.Area)
	}
	if params.Status != "" {
		conditions = append(conditions, "status = ?")
		args = append(args, params.Status)
	}
	if params.Domain != "" {
		conditions = append(conditions, "domain = ?")
		args = append(args, params.Domain)
	}
	if params.WorkType != "" {
		conditions = append(conditions, "work_type = ?")
		args = append(args, params.WorkType)
	}
	if len(conditions) > 0 {
		query += " WHERE " + strings.Join(conditions, " AND ")
	}
	query += " ORDER BY created_at DESC"
	if params.Limit > 0 {
		query += fmt.Sprintf(" LIMIT %d", params.Limit)
	}

	data, err := s.DB.QueryToJSON(ctx, query, args...)
	if err != nil {
		return fmt.Errorf("workgraph: listing tasks: %w", err)
	}
	fmt.Fprintln(w, string(data))
	return nil
}

// unwrapSingle extracts a single JSON object from a JSON array result.
// Returns ErrNotFound if the array is empty.
func unwrapSingle(data []byte, entity, id, formatID string) ([]byte, error) {
	// QueryToJSON returns "[\n  {...}\n]" or "[]".
	// Quick check: if it starts with "[]" it's empty.
	trimmed := strings.TrimSpace(string(data))
	if trimmed == "[]" {
		label := "id"
		value := id
		if id == "" {
			label = "format_id"
			value = formatID
		}
		return nil, fmt.Errorf("%w: %s with %s=%q", ErrNotFound, entity, label, value)
	}

	// Strip the outer array brackets to return a single object.
	// Find the first '{' and last '}'.
	start := strings.Index(trimmed, "{")
	end := strings.LastIndex(trimmed, "}")
	if start == -1 || end == -1 || start >= end {
		return data, nil // fallback: return as-is
	}
	return []byte(trimmed[start : end+1]), nil
}
