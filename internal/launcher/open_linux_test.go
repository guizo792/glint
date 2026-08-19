//go:build linux

package launcher

import (
	"strings"
	"testing"
)

func TestAppCommand(t *testing.T) {
	cmd := appCommand("myeditor --new")
	if !strings.HasSuffix(cmd.Path, "sh") {
		t.Fatalf("expected sh, got %q", cmd.Path)
	}
	got := strings.Join(cmd.Args, " ")
	if got != "sh -c myeditor --new" {
		t.Fatalf("unexpected args: %q", got)
	}
}

func TestFileCommand(t *testing.T) {
	cmd := fileCommand("/home/user/docs/report.pdf")
	if cmd.Path != "xdg-open" {
		t.Fatalf("expected xdg-open, got %q", cmd.Path)
	}
	if len(cmd.Args) != 2 || cmd.Args[1] != "/home/user/docs/report.pdf" {
		t.Fatalf("unexpected args: %v", cmd.Args)
	}
}
