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
