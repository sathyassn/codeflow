package ledger

import (
	"fmt"
	"os"
	"path/filepath"
	"syscall"
	"time"
)

// Writer appends validated events to JSONL ledger files with file locking.
type Writer struct {
	// dir is the ledger directory containing the JSONL files.
	dir string

	// Now returns the current time. Override in tests for deterministic output.
	// Defaults to time.Now().UTC().
	Now func() time.Time
}

// NewWriter creates a Writer that writes to the specified ledger directory.
// The directory is created if it does not exist.
func NewWriter(dir string) (*Writer, error) {
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return nil, fmt.Errorf("ledger: creating directory: %w", err)
	}
	return &Writer{
		dir: dir,
		Now: func() time.Time { return time.Now().UTC() },
	}, nil
}

// AppendEvent validates the event, routes it to the correct file, and appends
// it atomically with file locking. The event is marshaled to a single JSON
// line followed by a newline.
//
// If the event has no timestamp, one is auto-generated using the Writer's Now
// function. If the target file does not exist, it is created. File locking via
// flock ensures that concurrent writers do not produce partial or interleaved
// writes.
func (w *Writer) AppendEvent(event Event) error {
	// Validate schema.
	if err := ValidateEvent(event); err != nil {
		return err
	}

	// Route to the correct file.
	targetFile, err := RouteEvent(event.EventType)
	if err != nil {
		return err
	}

	return w.appendToFile(targetFile, event)
}

// AppendEventToFile validates the event schema and routing, then appends it
// to the specified file. Returns ErrMisroutedEvent if the event type does not
// belong to the target file.
func (w *Writer) AppendEventToFile(targetFile string, event Event) error {
	// Validate schema.
	if err := ValidateEvent(event); err != nil {
		return err
	}

	// Validate routing.
	if err := ValidateRoute(event.EventType, targetFile); err != nil {
		return err
	}

	return w.appendToFile(targetFile, event)
}

// appendToFile performs the atomic append: fill timestamp, marshal, lock, write, unlock.
func (w *Writer) appendToFile(filename string, event Event) error {
	// Auto-generate timestamp if missing.
	if event.Timestamp == "" {
		event.Timestamp = w.Now().Format(time.RFC3339)
	}

	data, err := event.marshalFlat()
	if err != nil {
		return fmt.Errorf("ledger: marshaling event: %w", err)
	}

	// Append newline to form a complete JSONL line.
	data = append(data, '\n')

	filePath := filepath.Join(w.dir, filename)
	lockPath := filePath + ".lock"

	// Open the lock file (created if needed).
	lockFile, err := os.OpenFile(lockPath, os.O_CREATE|os.O_WRONLY, 0o644)
	if err != nil {
		return fmt.Errorf("ledger: opening lock file: %w", err)
	}
	defer lockFile.Close()

	// Acquire exclusive lock.
	if err := syscall.Flock(int(lockFile.Fd()), syscall.LOCK_EX); err != nil {
		return fmt.Errorf("ledger: acquiring lock: %w", err)
	}
	defer func() {
		_ = syscall.Flock(int(lockFile.Fd()), syscall.LOCK_UN)
	}()

	// Open the data file for appending (created if needed).
	f, err := os.OpenFile(filePath, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0o644)
	if err != nil {
		return fmt.Errorf("ledger: opening file: %w", err)
	}
	defer f.Close()

	// Write the full line in a single call to minimize partial-write risk.
	if _, err := f.Write(data); err != nil {
		return fmt.Errorf("ledger: writing event: %w", err)
	}

	return nil
}

// Dir returns the ledger directory path.
func (w *Writer) Dir() string {
	return w.dir
}
