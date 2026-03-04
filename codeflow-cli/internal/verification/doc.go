// Package verification provides post-cutover verification tests that ensure
// the Go CLI implementation is complete, correct, and has no lingering
// dependencies on removed shell scripts.
//
// Tests in this package verify:
//   - Full PathFlow lifecycle via Go hook implementations
//   - Doctor infrastructure health checks pass cleanly
//   - No python3 references remain in production code
//   - Settings.json contains no .sh hook references
package verification
