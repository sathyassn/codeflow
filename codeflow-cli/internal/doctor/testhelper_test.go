package doctor

import (
	"github.com/codeflow/codeflow-cli/internal/db"
)

// newTestDBImpl creates a proper SQLite database using the db package.
func newTestDBImpl(path string) (interface{ Close() error }, error) {
	return db.NewDB(path)
}
