package ledger_test

import (
	"testing"

	"github.com/codeflow/codeflow-cli/internal/db"
	"github.com/codeflow/codeflow-cli/internal/ledger"
)

// TestFileConstantsMatchDB verifies that the canonical JSONL file name
// constants in the ledger package (write-side authority) match the
// identical constants in the db package (read/sync side). This prevents
// the two packages from silently diverging.
func TestFileConstantsMatchDB(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name     string
		ledgerFn string
		dbFn     string
	}{
		{"FileWorkGraph", ledger.FileWorkGraph, db.FileWorkGraph},
		{"FileMemoryEvents", ledger.FileMemoryEvents, db.FileMemoryEvents},
		{"FileSessions", ledger.FileSessions, db.FileSessions},
		{"FileConfig", ledger.FileConfig, db.FileConfig},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			if tt.ledgerFn != tt.dbFn {
				t.Errorf("ledger.%s = %q, db.%s = %q — constants have diverged",
					tt.name, tt.ledgerFn, tt.name, tt.dbFn)
			}
		})
	}
}

// TestCanonicalFilesMatchDB verifies that ledger.CanonicalFiles() and
// db.CanonicalFiles() return identical sets.
func TestCanonicalFilesMatchDB(t *testing.T) {
	t.Parallel()

	ledgerFiles := ledger.CanonicalFiles()
	dbFiles := db.CanonicalFiles()

	if len(ledgerFiles) != len(dbFiles) {
		t.Fatalf("ledger.CanonicalFiles() has %d entries, db.CanonicalFiles() has %d",
			len(ledgerFiles), len(dbFiles))
	}

	dbSet := make(map[string]bool, len(dbFiles))
	for _, f := range dbFiles {
		dbSet[f] = true
	}

	for _, f := range ledgerFiles {
		if !dbSet[f] {
			t.Errorf("ledger.CanonicalFiles() includes %q but db.CanonicalFiles() does not", f)
		}
	}
}

// TestPathflowEventRoutesExist verifies that all 5 pathflow event types are
// routed to FilePathflowEvents. This prevents silent removal of pathflow
// routes from the routing table.
func TestPathflowEventRoutesExist(t *testing.T) {
	t.Parallel()

	pathflowEvents := []string{
		"phase_transition",
		"stage_transition",
		"session_register",
		"session_metadata",
		"pathflow_task_update",
	}

	for _, eventType := range pathflowEvents {
		t.Run(eventType, func(t *testing.T) {
			t.Parallel()

			file, err := ledger.RouteEvent(eventType)
			if err != nil {
				t.Fatalf("RouteEvent(%q) returned error: %v", eventType, err)
			}
			if file != ledger.FilePathflowEvents {
				t.Errorf("RouteEvent(%q) = %q, want %q", eventType, file, ledger.FilePathflowEvents)
			}
		})
	}
}

// TestFilePathflowEventsNotInCanonicalFiles verifies that FilePathflowEvents
// is excluded from CanonicalFiles (pathflow events live in .state/logs/, not
// .state/ledger/, and are not synced to SQLite).
func TestFilePathflowEventsNotInCanonicalFiles(t *testing.T) {
	t.Parallel()

	for _, f := range ledger.CanonicalFiles() {
		if f == ledger.FilePathflowEvents {
			t.Errorf("CanonicalFiles() should not include FilePathflowEvents (%q)", f)
		}
	}
}
