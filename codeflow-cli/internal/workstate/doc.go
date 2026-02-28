// Package workstate provides active task bridge file management and memory
// event recording for the CodeFlow CLI. It replaces the shell-based
// cf-work-state.sh and memory.sh with Go-native JSON marshaling, atomic
// file writes, and ledger-backed event persistence.
package workstate
