package security

import (
	"testing"
)

func TestDangerousModule(t *testing.T) {
	t.Parallel()

	mod := &DangerousModule{}
	policy := DefaultPolicy()

	blocked := []struct {
		name string
		cmd  string
	}{
		{"dd zero", "dd if=/dev/zero of=disk.img"},
		{"dd random", "dd if=/dev/random of=out"},
		{"mkfs", "mkfs.ext4 /dev/sda1"},
		{"dev redirect", "> /dev/sda"},
		{"rm rf root", "rm -rf /"},
		{"rm rf root star", "rm -rf /*"},
		{"rm fr root", "rm -fr /"},
		{"rm rf home", "rm -rf ~"},
		{"rm rf etc", "rm -rf /etc"},
		{"rm rf var", "rm -rf /var"},
		{"dd to disk", "dd if=image.iso of=/dev/sda"},
		{"mke2fs", "mke2fs /dev/sda1"},
		{"mkswap", "mkswap /dev/sda2"},
		{"chmod 777 root", "chmod 777 /"},
		{"chmod 777 etc", "chmod 777 /etc"},
		{"chmod R root", "chmod -R 755 /"},
		{"chown R root", "chown -R root /"},
		{"fork bomb", ":(){ :|:& };:"},
	}

	for _, tt := range blocked {
		t.Run("block_"+tt.name, func(t *testing.T) {
			t.Parallel()
			ctx := &CheckContext{Command: tt.cmd, Policy: policy}
			v := mod.Check(ctx)
			if v == nil || v.Allow {
				t.Errorf("expected block for %q, got allow", tt.cmd)
			}
		})
	}

	allowed := []struct {
		name string
		cmd  string
	}{
		{"rm file", "rm some-file.txt"},
		{"rm rf dir", "rm -rf /tmp/build"},
		{"chmod normal", "chmod 644 file.txt"},
		{"dd normal", "dd if=input.bin of=output.bin"},
		{"ls", "ls -la"},
	}

	for _, tt := range allowed {
		t.Run("allow_"+tt.name, func(t *testing.T) {
			t.Parallel()
			ctx := &CheckContext{Command: tt.cmd, Policy: policy}
			v := mod.Check(ctx)
			if v != nil && !v.Allow {
				t.Errorf("expected allow for %q, got block: %s", tt.cmd, v.Reason)
			}
		})
	}
}
