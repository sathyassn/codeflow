package main

import (
	"strings"
	"testing"

	"github.com/spf13/cobra"
)

func TestReadStdinBytes(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name    string
		input   string
		want    string
		wantErr bool
	}{
		{
			name:  "reads stdin content",
			input: `{"tool_name":"Edit"}`,
			want:  `{"tool_name":"Edit"}`,
		},
		{
			name:  "empty stdin",
			input: "",
			want:  "",
		},
		{
			name:  "multiline content",
			input: "line1\nline2\nline3",
			want:  "line1\nline2\nline3",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()

			cmd := &cobra.Command{}
			cmd.SetIn(strings.NewReader(tt.input))

			got, err := readStdinBytes(cmd)
			if (err != nil) != tt.wantErr {
				t.Fatalf("readStdinBytes() error = %v, wantErr %v", err, tt.wantErr)
			}
			if string(got) != tt.want {
				t.Errorf("readStdinBytes() = %q, want %q", string(got), tt.want)
			}
		})
	}
}

func TestIsSettingsTemplateEdit_EdgeCases(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name string
		data string
		want bool
	}{
		{
			name: "nested settings-templates path",
			data: `{"tool_name":"Edit","tool_input":{"file_path":"/deep/nested/.claude/settings-templates/custom.json"}}`,
			want: true,
		},
		{
			name: "missing file_path field",
			data: `{"tool_name":"Edit","tool_input":{}}`,
			want: false,
		},
		{
			name: "null data",
			data: "null",
			want: false,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			got := isSettingsTemplateEdit([]byte(tt.data))
			if got != tt.want {
				t.Errorf("isSettingsTemplateEdit() = %v, want %v", got, tt.want)
			}
		})
	}
}
