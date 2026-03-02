// Package settings provides managed settings setup and settings template
// validation for the CodeFlow CLI.
//
// It includes:
//   - SetupManagedSettings: creates or updates the system-level managed-settings.json
//     file that enforces enterprise policies across all Claude Code usage.
//   - ValidateSettingsTemplates: shared core function that validates consistency
//     across settings template files (hooks SHA256, version, wiring, copy mapping checksums).
package settings
