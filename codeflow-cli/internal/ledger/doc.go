// Package ledger provides atomic, schema-validated JSONL append operations
// for the CodeFlow event ledger. It replaces the shell-based ledger.sh with
// Go-native file locking, JSON marshaling, and event-type routing.
package ledger
