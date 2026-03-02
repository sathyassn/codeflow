package settings

import "os/exec"

// defaultReadWithSudo reads a file that may require elevated privileges.
func defaultReadWithSudo(path string) ([]byte, error) {
	out, err := exec.Command("sudo", "cat", path).Output()
	if err != nil {
		return nil, err
	}
	return out, nil
}

// defaultSudoCopy copies a file using sudo.
func defaultSudoCopy(src, dst string) error {
	return exec.Command("sudo", "cp", src, dst).Run()
}

// defaultSudoMkdir creates a directory with sudo.
func defaultSudoMkdir(dir string) error {
	return exec.Command("sudo", "mkdir", "-p", dir).Run()
}

// defaultSudoChmod sets file permissions with sudo.
func defaultSudoChmod(path, mode string) error {
	return exec.Command("sudo", "chmod", mode, path).Run()
}
