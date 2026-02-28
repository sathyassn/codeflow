package main

import (
	"encoding/json"
	"fmt"
	"io"
	"os"

	"github.com/codeflow/codeflow-cli/internal/ledger"
	"github.com/spf13/cobra"
)

// newLedgerCmd creates the top-level "ledger" command with append/validate subcommands.
func newLedgerCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "ledger",
		Short: "JSONL ledger operations",
		Long:  "Manage the CodeFlow JSONL event ledger: append validated events and validate ledger file integrity.",
		RunE: func(cmd *cobra.Command, _ []string) error {
			return cmd.Help()
		},
	}

	cmd.AddCommand(newLedgerAppendCmd())
	cmd.AddCommand(newLedgerValidateCmd())

	return cmd
}

// newLedgerAppendCmd creates the "ledger append" subcommand.
func newLedgerAppendCmd() *cobra.Command {
	var (
		ledgerDir string
		eventJSON string
	)

	cmd := &cobra.Command{
		Use:   "append",
		Short: "Append a validated event to the ledger",
		Long: `Appends a single JSON event to the appropriate JSONL ledger file.
The event is schema-validated and routed to the correct file based on event type.
Event JSON can be provided via --event flag or piped via stdin.`,
		Args: cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return runLedgerAppend(cmd.OutOrStdout(), cmd.InOrStdin(), ledgerDir, eventJSON)
		},
	}

	cmd.Flags().StringVar(&ledgerDir, "ledger", defaultLedgerDir(), "path to JSONL ledger directory")
	cmd.Flags().StringVar(&eventJSON, "event", "", "event as JSON string (alternative to stdin)")

	return cmd
}

// runLedgerAppend implements the ledger append logic.
func runLedgerAppend(w io.Writer, stdin io.Reader, ledgerDir, eventJSON string) error {
	// Read event JSON from flag or stdin.
	var rawJSON []byte
	if eventJSON != "" {
		rawJSON = []byte(eventJSON)
	} else {
		data, err := io.ReadAll(stdin)
		if err != nil {
			return &exitError{code: ExitGeneralError, err: fmt.Errorf("reading stdin: %w", err)}
		}
		rawJSON = data
	}

	if len(rawJSON) == 0 {
		return &exitError{code: ExitConfigError, err: fmt.Errorf("no event data provided; use --event or pipe JSON to stdin")}
	}

	// Parse the JSON into an Event.
	var event ledger.Event
	if err := json.Unmarshal(rawJSON, &event); err != nil {
		return &exitError{code: ExitConfigError, err: fmt.Errorf("parsing event JSON: %w", err)}
	}

	// Create writer and append.
	writer, err := ledger.NewWriter(ledgerDir)
	if err != nil {
		return &exitError{code: ExitRuntimeError, err: fmt.Errorf("creating ledger writer: %w", err)}
	}

	if err := writer.AppendEvent(event); err != nil {
		return &exitError{code: ExitRuntimeError, err: fmt.Errorf("appending event: %w", err)}
	}

	fmt.Fprintln(w, "Event appended successfully")
	return nil
}

// newLedgerValidateCmd creates the "ledger validate" subcommand.
func newLedgerValidateCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "validate FILE",
		Short: "Validate JSONL file integrity",
		Long:  "Reads a JSONL file and validates that each line is well-formed JSON with a recognized event type.",
		Args:  cobra.ExactArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			return runLedgerValidate(cmd.OutOrStdout(), args[0])
		},
	}

	return cmd
}

// runLedgerValidate implements the ledger validate logic.
func runLedgerValidate(w io.Writer, filePath string) error {
	data, err := os.ReadFile(filePath)
	if err != nil {
		return &exitError{code: ExitRuntimeError, err: fmt.Errorf("reading file: %w", err)}
	}

	lines := splitLines(data)
	var errors []string
	valid := 0

	for i, line := range lines {
		if len(line) == 0 {
			continue
		}

		var m map[string]any
		if err := json.Unmarshal(line, &m); err != nil {
			errors = append(errors, fmt.Sprintf("line %d: invalid JSON: %s", i+1, err.Error()))
			continue
		}

		eventType, _ := m["event"].(string)
		if eventType == "" {
			errors = append(errors, fmt.Sprintf("line %d: missing 'event' field", i+1))
			continue
		}

		valid++
	}

	fmt.Fprintf(w, "Validated %d events", valid)
	if len(errors) > 0 {
		fmt.Fprintf(w, ", %d errors:\n", len(errors))
		for _, e := range errors {
			fmt.Fprintf(w, "  - %s\n", e)
		}
		return &exitError{code: ExitRuntimeError, err: fmt.Errorf("%d validation errors found", len(errors))}
	}

	fmt.Fprintln(w, ", 0 errors")
	return nil
}

// splitLines splits byte data into lines, handling both \n and \r\n.
func splitLines(data []byte) [][]byte {
	var lines [][]byte
	start := 0
	for i := range data {
		if data[i] == '\n' {
			end := i
			if end > start && data[end-1] == '\r' {
				end--
			}
			lines = append(lines, data[start:end])
			start = i + 1
		}
	}
	if start < len(data) {
		lines = append(lines, data[start:])
	}
	return lines
}
