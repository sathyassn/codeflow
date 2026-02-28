// Package idgen provides ID generation utilities for the CodeFlow CLI.
//
// It offers two categories of ID generation:
//
//   - ULID generation: Universally Unique Lexicographically Sortable Identifiers
//     using the oklog/ulid library with crypto/rand entropy.
//   - Format ID generation: Sequential, human-readable IDs (e.g., INF-EPC-021,
//     INF-TSK-021-001) derived from SQLite queries against the CodeFlow database.
package idgen
