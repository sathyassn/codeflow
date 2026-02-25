package db

import (
	"database/sql"
	"encoding/json"
)

// ActiveWork represents a row in the active_work table.
type ActiveWork struct {
	ID           string          `json:"id"`
	TaskID       sql.NullString  `json:"task_id"`
	Topic        string          `json:"topic"`
	Status       string          `json:"status"`
	Branch       sql.NullString  `json:"branch"`
	Scope        json.RawMessage `json:"scope"`
	Deliverables json.RawMessage `json:"deliverables"`
	Agent        sql.NullString  `json:"agent"`
	SessionID    sql.NullString  `json:"session_id"`
	CurrentStage sql.NullString  `json:"current_stage"`
	TeamName     sql.NullString  `json:"team_name"`
	CreatedAt    string          `json:"created_at"`
	UpdatedAt    string          `json:"updated_at"`
}

// Session represents a row in the sessions table.
type Session struct {
	ID                string          `json:"id"`
	ProjectID         sql.NullString  `json:"project_id"`
	UserID            string          `json:"user_id"`
	UserHost          string          `json:"user_host"`
	MachineFingerprint sql.NullString `json:"machine_fingerprint"`
	StartedAt         string          `json:"started_at"`
	EndedAt           sql.NullString  `json:"ended_at"`
	DurationSeconds   sql.NullInt64   `json:"duration_seconds"`
	Status            string          `json:"status"`
	WorkIDs           json.RawMessage `json:"work_ids"`
	PreviousSessionID sql.NullString  `json:"previous_session_id"`
	ContextSummary    sql.NullString  `json:"context_summary"`
	ToolStats         json.RawMessage `json:"tool_stats"`
	Metadata          json.RawMessage `json:"metadata"`
}

// ProjectConfig represents a row in the project_config table.
type ProjectConfig struct {
	ID            string          `json:"id"`
	Name          string          `json:"name"`
	Description   sql.NullString  `json:"description"`
	GitRemoteURL  sql.NullString  `json:"git_remote_url"`
	DefaultBranch string          `json:"default_branch"`
	CreatedAt     string          `json:"created_at"`
	UpdatedAt     string          `json:"updated_at"`
	Metadata      json.RawMessage `json:"metadata"`
}

// Epic represents a row in the epics table.
type Epic struct {
	ID          string          `json:"id"`
	FormatID    string          `json:"format_id"`
	Title       string          `json:"title"`
	Summary     sql.NullString  `json:"summary"`
	Status      string          `json:"status"`
	AreaType    string          `json:"area_type"`
	WorkType    string          `json:"work_type"`
	Domain      string          `json:"domain"`
	IsOngoing   bool            `json:"is_ongoing"`
	FileScope   json.RawMessage `json:"file_scope"`
	Priority    string          `json:"priority"`
	PRNumber    sql.NullInt64   `json:"pr_number"`
	ExternalID  sql.NullString  `json:"external_id"`
	ExternalURL sql.NullString  `json:"external_url"`
	CreatedAt   string          `json:"created_at"`
	UpdatedAt   string          `json:"updated_at"`
}

// Task represents a row in the tasks table.
type Task struct {
	ID              string          `json:"id"`
	FormatID        string          `json:"format_id"`
	EpicID          string          `json:"epic_id"`
	Title           string          `json:"title"`
	Description     sql.NullString  `json:"description"`
	Status          string          `json:"status"`
	AreaType        string          `json:"area_type"`
	WorkType        string          `json:"work_type"`
	Domain          string          `json:"domain"`
	Origin          string          `json:"origin"`
	FileScope       json.RawMessage `json:"file_scope"`
	ScopePolicy     string          `json:"scope_policy"`
	ScopeRoot       sql.NullString  `json:"scope_root"`
	Estimate        sql.NullString  `json:"estimate"`
	Priority        string          `json:"priority"`
	AssigneeID      sql.NullString  `json:"assignee_id"`
	AutorunEligible bool            `json:"autorun_eligible"`
	AutoCommit      bool            `json:"auto_commit"`
	RaisePR         bool            `json:"raise_pr"`
	AutoMerge       bool            `json:"auto_merge"`
	TargetBranch    sql.NullString  `json:"target_branch"`
	Acceptance      json.RawMessage `json:"acceptance"`
	Tests           json.RawMessage `json:"tests"`
	Branch          sql.NullString  `json:"branch"`
	PRNumber        sql.NullInt64   `json:"pr_number"`
	ExternalID      sql.NullString  `json:"external_id"`
	ExternalURL     sql.NullString  `json:"external_url"`
	CreatedAt       string          `json:"created_at"`
	UpdatedAt       string          `json:"updated_at"`
	StartedAt       sql.NullString  `json:"started_at"`
	CompletedAt     sql.NullString  `json:"completed_at"`
	Stage           sql.NullString  `json:"stage"`
	StageStatus     sql.NullString  `json:"stage_status"`
	StageHistory    json.RawMessage `json:"stage_history"`
}

// User represents a row in the users table.
type User struct {
	ID                     string          `json:"id"`
	Email                  string          `json:"email"`
	DisplayName            sql.NullString  `json:"display_name"`
	GitUsername             sql.NullString  `json:"git_username"`
	Role                   string          `json:"role"`
	LastHost               sql.NullString  `json:"last_host"`
	LastMachineFingerprint sql.NullString  `json:"last_machine_fingerprint"`
	FirstSeenAt            string          `json:"first_seen_at"`
	LastActiveAt           sql.NullString  `json:"last_active_at"`
	Preferences            json.RawMessage `json:"preferences"`
	Metadata               json.RawMessage `json:"metadata"`
}

// MemoryEvent represents a row in the memory_events table.
type MemoryEvent struct {
	ID         string         `json:"id"`
	EventType  string         `json:"event_type"`
	Domain     string         `json:"domain"`
	WorkID     sql.NullString `json:"work_id"`
	Data       string         `json:"data"`
	MemoryType sql.NullString `json:"memory_type"`
	CreatedAt  string         `json:"created_at"`
}

// AutorunSession represents a row in the autorun_sessions table.
type AutorunSession struct {
	ID                string         `json:"id"`
	BatchFile         string         `json:"batch_file"`
	BatchName         sql.NullString `json:"batch_name"`
	Status            string         `json:"status"`
	MaxSessionWorkers int            `json:"max_session_workers"`
	TotalTasks        int            `json:"total_tasks"`
	CompletedTasks    int            `json:"completed_tasks"`
	FailedTasks       int            `json:"failed_tasks"`
	CreatedAt         string         `json:"created_at"`
	CompletedAt       sql.NullString `json:"completed_at"`
}

// AutorunWorker represents a row in the autorun_workers table.
type AutorunWorker struct {
	ID           string         `json:"id"`
	SessionID    string         `json:"session_id"`
	WorkerNum    int            `json:"worker_num"`
	TaskID       string         `json:"task_id"`
	Status       string         `json:"status"`
	TmuxSession  sql.NullString `json:"tmux_session"`
	WorktreePath sql.NullString `json:"worktree_path"`
	PRNumber     sql.NullInt64  `json:"pr_number"`
	StartedAt    sql.NullString `json:"started_at"`
	CompletedAt  sql.NullString `json:"completed_at"`
}
