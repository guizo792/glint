//go:build linux

package indexer

import (
	"os"
	"path/filepath"
	"testing"

	"spotlight/internal/search"
)

func TestDiscoverApps(t *testing.T) {
	dir := t.TempDir()
	writeFile(t, filepath.Join(dir, "good.desktop"), `[Desktop Entry]
Type=Application
Name=My Editor
Exec=myeditor %U
Icon=editor-icon
`)
	writeFile(t, filepath.Join(dir, "hidden.desktop"), `[Desktop Entry]
Type=Application
Name=Secret Tool
Exec=secret
Hidden=true
`)
	writeFile(t, filepath.Join(dir, "nodisplay.desktop"), `[Desktop Entry]
Type=Application
Name=Internal
Exec=internal
NoDisplay=true
`)
	writeFile(t, filepath.Join(dir, "notapp.desktop"), `[Desktop Entry]
Type=Link
Name=Docs
URL=https://example.com
`)
	writeFile(t, filepath.Join(dir, "noexec.desktop"), `[Desktop Entry]
Type=Application
Name=Broken
`)
	writeFile(t, filepath.Join(dir, "notes.txt"), "not a desktop file")

	apps := discoverAppsFrom([]string{dir})
	if len(apps) != 1 {
		t.Fatalf("expected exactly 1 app, got %d: %+v", len(apps), apps)
	}
	app := apps[0]
	if app.Name != "My Editor" || app.Path != "myeditor %U" || app.Icon != "editor-icon" || app.Kind != search.KindApp {
		t.Fatalf("unexpected app entry: %+v", app)
	}
}

func TestDiscoverAppsDedupeByName(t *testing.T) {
	dirA := t.TempDir()
	dirB := t.TempDir()
	writeFile(t, filepath.Join(dirA, "same.desktop"), "[Desktop Entry]\nType=Application\nName=First\nExec=first\n")
	writeFile(t, filepath.Join(dirB, "same.desktop"), "[Desktop Entry]\nType=Application\nName=Second\nExec=second\n")

	apps := discoverAppsFrom([]string{dirA, dirB})
	if len(apps) != 1 || apps[0].Name != "First" {
		t.Fatalf("expected first dir to win, got %+v", apps)
	}
}

func TestStripExecFieldCodes(t *testing.T) {
	cases := map[string]string{
		"myeditor %U":          "myeditor",
		"app --flag %f %u":     "app --flag",
		`terminal -e "vim %F"`: `terminal -e "vim "`,
		"app %i %c %k":         "app",
		"no codes here":        "no codes here",
		"env VAR=1 app":        "env VAR=1 app",
	}
	for in, want := range cases {
		if got := StripExecFieldCodes(in); got != want {
			t.Errorf("StripExecFieldCodes(%q) = %q, want %q", in, got, want)
		}
	}
}

func writeFile(t *testing.T, path, content string) {
	t.Helper()
	if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}
}
