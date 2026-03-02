package prompt

import "os/exec"

// newExecCmd creates an *exec.Cmd. Replaceable in tests via init().
var newExecCmd = exec.Command
