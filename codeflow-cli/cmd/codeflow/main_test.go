package main

import (
	"bytes"
	"context"
	"fmt"
	"io"
	"os"
	"testing"
)

func TestVersionVariable(t *testing.T) {
	if version == "" {
		t.Fatal("version should have a default value")
	}
	if version != "dev" {
		t.Errorf("expected default version to be %q, got %q", "dev", version)
	}
}

func TestRun_VersionCommand(t *testing.T) {
	ctx := t.Context()
	output := captureStdout(t, func() {
		err := run(ctx, []string{"version"})
		if err != nil {
			t.Fatalf("run(version) returned error: %v", err)
		}
	})

	expected := fmt.Sprintf("codeflow %s\n", version)
	if output != expected {
		t.Errorf("expected output %q, got %q", expected, output)
	}
}

func TestRun_NoArgs(t *testing.T) {
	ctx := t.Context()
	output := captureStdout(t, func() {
		err := run(ctx, nil)
		if err != nil {
			t.Fatalf("run(nil) returned error: %v", err)
		}
	})

	if output != "codeflow CLI\n" {
		t.Errorf("expected output %q, got %q", "codeflow CLI\n", output)
	}
}

func TestRun_ReturnsNoError(t *testing.T) {
	t.Run("no args", func(t *testing.T) {
		ctx := t.Context()
		output := captureStdout(t, func() {
			err := run(ctx, nil)
			if err != nil {
				t.Errorf("expected nil error, got: %v", err)
			}
		})
		if output == "" {
			t.Error("expected non-empty output from run(nil)")
		}
		if ctx.Err() != nil {
			t.Errorf("test context cancelled unexpectedly: %v", ctx.Err())
		}
	})

	t.Run("version", func(t *testing.T) {
		ctx := t.Context()
		output := captureStdout(t, func() {
			err := run(ctx, []string{"version"})
			if err != nil {
				t.Errorf("expected nil error, got: %v", err)
			}
		})
		if output == "" {
			t.Error("expected non-empty output from run(version)")
		}
	})

	t.Run("cancelled context", func(t *testing.T) {
		ctx, cancel := context.WithCancel(t.Context())
		cancel()
		// run should still succeed for simple commands even with cancelled context,
		// since current commands don't perform I/O that respects context.
		output := captureStdout(t, func() {
			err := run(ctx, nil)
			if err != nil {
				t.Errorf("expected nil error even with cancelled context, got: %v", err)
			}
		})
		if output == "" {
			t.Error("expected non-empty output from run with cancelled context")
		}
	})
}

// captureStdout redirects os.Stdout to capture output from fn.
// The pipe read end is closed via t.Cleanup to prevent resource leaks.
// Not safe for use with t.Parallel() — mutates the global os.Stdout.
func captureStdout(t *testing.T, fn func()) string {
	t.Helper()

	old := os.Stdout
	r, w, err := os.Pipe()
	if err != nil {
		t.Fatalf("failed to create pipe: %v", err)
	}
	t.Cleanup(func() { r.Close() })

	os.Stdout = w

	fn()

	w.Close()
	os.Stdout = old

	var buf bytes.Buffer
	if _, err := io.Copy(&buf, r); err != nil {
		t.Fatalf("failed to read captured output: %v", err)
	}
	return buf.String()
}
