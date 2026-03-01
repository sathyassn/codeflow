// Package sentinel provides PathFlow sentinel CRUD operations for CodeFlow.
//
// Sentinels are empty marker files that track PathFlow phase and stage
// progression. They are stored in .state/sentinels/pathflow/{sessionID}/
// and named "pathflow-{name}" (e.g., "pathflow-pf-3", "pathflow-ws-dev").
//
// The [Manager] type provides Create, Check, List, and Delete operations
// for sentinel files. It is the single source of truth for sentinel
// operations, shared by both hook implementations (internal/hooks/sentinel)
// and CLI subcommands (codeflow sentinel).
//
// This package handles PathFlow sentinels only. Skill sentinels were
// removed as dead infrastructure.
package sentinel
