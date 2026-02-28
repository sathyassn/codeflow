package workstate

import (
	"fmt"

	"github.com/codeflow/codeflow-cli/internal/ledger"
)

// RecordMemoryEvent appends a memory event to the ledger. The ledger writer
// routes memory event types (progress, decision, milestone, etc.) to
// memory-events.jsonl automatically via its routing table.
//
// The event must have EventType set to a valid memory event type.
func RecordMemoryEvent(w *ledger.Writer, event ledger.Event) error {
	if event.EventType == "" {
		return fmt.Errorf("workstate: event type must not be empty")
	}

	if err := w.AppendEvent(event); err != nil {
		return fmt.Errorf("workstate: recording memory event: %w", err)
	}

	return nil
}
