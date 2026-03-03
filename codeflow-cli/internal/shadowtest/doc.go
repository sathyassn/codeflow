// Package shadowtest provides a harness for running shadow tests that execute
// each Go subcommand alongside its shell script equivalent with identical
// stdin and environment, compare outputs using schema-aware normalization, and
// report divergences to validate behavioral parity before Phase E cutover.
package shadowtest
