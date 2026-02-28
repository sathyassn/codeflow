package security

import (
	"os"
	"regexp"
	"strings"
)

// TmpModule validates tmp file operations.
// Go equivalent of cf-tmp-protection.sh.
type TmpModule struct{}

// Name returns the module name.
func (m *TmpModule) Name() string { return "tmp-protection" }

// Check evaluates the command for managed tmp violations.
func (m *TmpModule) Check(ctx *CheckContext) *Verdict {
	cmd := ctx.Command

	projectRoot := os.Getenv("CF_PROJECT_ROOT")
	if projectRoot == "" {
		projectRoot = "codeflow"
	}

	var folders []string
	var stateFolder string

	if ctx.Policy != nil {
		folders = ctx.Policy.ManagedTmpFolders(projectRoot)
		stateFolder = ctx.Policy.StateFolderPath(projectRoot)
	} else {
		folders = defaultManagedTmpFolders(projectRoot)
		stateFolder = "/tmp/claude/" + projectRoot + "/managed/state"
	}

	// Block deletion/rename of managed folders
	for _, folder := range folders {
		qf := regexp.QuoteMeta(folder)
		// rm/rmdir/mv targeting the folder itself
		if rmFolder, err := regexp.Compile(`(rm|rmdir|mv)\s+((-[a-zA-Z]+\s+)*)` + qf + `($|\s)`); err == nil && rmFolder.MatchString(cmd) {
			return block("Managed Tmp Protection", "Cannot delete/rename managed folder", folder)
		}
		// rm -rf targeting the folder
		rmRF, err1 := regexp.Compile(`rm\s+-[a-zA-Z]*r[a-zA-Z]*f[a-zA-Z]*\s+` + qf + `($|\s)`)
		rmFR, err2 := regexp.Compile(`rm\s+-[a-zA-Z]*f[a-zA-Z]*r[a-zA-Z]*\s+` + qf + `($|\s)`)
		if err1 == nil && err2 == nil && (rmRF.MatchString(cmd) || rmFR.MatchString(cmd)) {
			return block("Managed Tmp Protection", "Cannot delete managed folder recursively", folder)
		}
	}

	// Block deletion of state files (but allow creation and editing)
	if stateFolder != "" && strings.Contains(cmd, stateFolder+"/") {
		if rmState, err := regexp.Compile(`(rm|unlink)\s+((-[a-zA-Z]+\s+)*)` + regexp.QuoteMeta(stateFolder) + `/`); err == nil && rmState.MatchString(cmd) {
			return block("State File Protection",
				"State files protected from deletion. User can rm manually if needed.",
				stateFolder+"/*")
		}
	}

	return nil
}
