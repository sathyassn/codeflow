package security

import (
	"strings"
	"testing"
)

func TestParseHookInput(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name        string
		input       string
		wantTool    string
		wantCmd     string
		wantSandbox bool
		wantErr     bool
	}{
		{
			name:     "valid bash command",
			input:    `{"tool_name":"Bash","tool_input":{"command":"ls -la"}}`,
			wantTool: "Bash",
			wantCmd:  "ls -la",
		},
		{
			name:     "non-bash tool",
			input:    `{"tool_name":"Read","tool_input":{"file_path":"/tmp/x"}}`,
			wantTool: "Read",
			wantCmd:  "",
		},
		{
			name:        "sandbox bypass",
			input:       `{"tool_name":"Bash","tool_input":{"command":"git push","dangerouslyDisableSandbox":true}}`,
			wantTool:    "Bash",
			wantCmd:     "git push",
			wantSandbox: true,
		},
		{
			name:     "empty input",
			input:    "",
			wantTool: "",
			wantCmd:  "",
		},
		{
			name:    "invalid json",
			input:   "not json",
			wantErr: true,
		},
		{
			name:     "empty tool input",
			input:    `{"tool_name":"Bash"}`,
			wantTool: "Bash",
			wantCmd:  "",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			tool, cmd, sandbox, err := ParseHookInput(strings.NewReader(tt.input))
			if tt.wantErr {
				if err == nil {
					t.Error("expected error, got nil")
				}
				return
			}
			if err != nil {
				t.Fatalf("unexpected error: %v", err)
			}
			if tool != tt.wantTool {
				t.Errorf("tool = %q, want %q", tool, tt.wantTool)
			}
			if cmd != tt.wantCmd {
				t.Errorf("cmd = %q, want %q", cmd, tt.wantCmd)
			}
			if sandbox != tt.wantSandbox {
				t.Errorf("sandbox = %v, want %v", sandbox, tt.wantSandbox)
			}
		})
	}
}
