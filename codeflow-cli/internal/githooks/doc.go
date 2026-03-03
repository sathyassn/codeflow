// Package githooks implements Git hook logic as Go functions.
//
// Each hook reads configuration from enforcement-policy.json and
// provides the same validation that the original shell scripts
// performed, but without shell/jq dependencies.
//
// The package is consumed by the "codeflow git-hooks" CLI subcommand
// group and by thin shell wrappers in .codeflow/scripts/git-hooks/.
package githooks
