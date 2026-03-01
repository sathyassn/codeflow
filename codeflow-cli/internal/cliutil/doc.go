// Package cliutil provides shared CLI utilities for the codeflow CLI.
//
// It centralises common operations that multiple packages depend on:
//   - Structured logging to stderr (LogInfo, LogWarn, LogError, LogDebug)
//   - Exported exit codes and exit-helper functions (ExitBlock, ExitAllow)
//   - Typed loader for pathflow-config.json (LoadPathflowConfig)
//   - Input validation helpers (ValidateULID, ValidateFilePath)
package cliutil
