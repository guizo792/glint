//go:build linux

package indexer

import (
	"bufio"
	"os"
	"path/filepath"
	"strings"

	"spotlight/internal/search"
)

func DiscoverApps() []search.Entry {
	home, _ := os.UserHomeDir()
	dirs := []string{
		"/usr/share/applications",
		"/usr/local/share/applications",
		filepath.Join(home, ".local/share/applications"),
	}
	return discoverAppsFrom(dirs)
}

func discoverAppsFrom(dirs []string) []search.Entry {
	seen := make(map[string]bool)
	var apps []search.Entry
	for _, dir := range dirs {
		entries, err := os.ReadDir(dir)
		if err != nil {
			continue
		}
		for _, e := range entries {
			name := e.Name()
			if e.IsDir() || !strings.HasSuffix(name, ".desktop") {
				continue
			}
			base := strings.TrimSuffix(name, ".desktop")
			if seen[base] {
				continue
			}
			app, ok := parseDesktopFile(filepath.Join(dir, name))
			if !ok {
				continue
			}
			seen[base] = true
			apps = append(apps, app)
		}
	}
	return apps
}

func parseDesktopFile(path string) (search.Entry, bool) {
	f, err := os.Open(path)
	if err != nil {
		return search.Entry{}, false
	}
	defer f.Close()

	var name, exec, icon, typ string
	sc := bufio.NewScanner(f)
	for sc.Scan() {
		line := strings.TrimSpace(sc.Text())
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		key, value, ok := strings.Cut(line, "=")
		if !ok {
			continue
		}
		key = strings.TrimSpace(key)
		value = strings.TrimSpace(value)
		switch key {
		case "Type":
			typ = value
		case "Name":
			name = value
		case "Exec":
			exec = value
		case "Icon":
			icon = value
		case "NoDisplay":
			if strings.EqualFold(value, "true") {
				return search.Entry{}, false
			}
		case "Hidden":
			if strings.EqualFold(value, "true") {
				return search.Entry{}, false
			}
		}
	}

	if typ != "Application" || name == "" || exec == "" {
		return search.Entry{}, false
	}
	return search.Entry{Name: name, Path: exec, Kind: search.KindApp, Icon: icon}, true
}

func StripExecFieldCodes(exec string) string {
	var b strings.Builder
	for i := 0; i < len(exec); i++ {
		if exec[i] == '%' && i+1 < len(exec) {
			i++
			continue
		}
		b.WriteByte(exec[i])
	}
	return strings.TrimSpace(b.String())
}
