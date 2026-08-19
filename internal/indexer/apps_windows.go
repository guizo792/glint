//go:build windows

package indexer

import (
	"os"
	"path/filepath"
	"strings"

	"spotlight/internal/search"
)

func DiscoverApps() []search.Entry {
	programData, _ := os.LookupEnv("ProgramData")
	appData, _ := os.LookupEnv("APPDATA")
	programFiles, _ := os.LookupEnv("ProgramFiles")
	programFilesX86, _ := os.LookupEnv("ProgramFiles(x86)")

	dirs := []string{
		filepath.Join(programData, "Microsoft", "Windows", "Start Menu", "Programs"),
		filepath.Join(appData, "Microsoft", "Windows", "Start Menu", "Programs"),
		filepath.Join(programFiles, "Microsoft", "Windows", "Start Menu", "Programs"),
	}

	seen := make(map[string]bool)
	var apps []search.Entry
	for _, dir := range dirs {
		if dir == "" {
			continue
		}
		filepath.WalkDir(dir, func(path string, d os.DirEntry, err error) error {
			if err != nil || d.IsDir() {
				return nil
			}
			ext := strings.ToLower(filepath.Ext(d.Name()))
			if ext != ".lnk" && ext != ".exe" {
				return nil
			}
			name := strings.TrimSuffix(d.Name(), filepath.Ext(d.Name()))
			if seen[name] {
				return nil
			}
			seen[name] = true
			apps = append(apps, search.Entry{Name: name, Path: path, Kind: search.KindApp})
			return nil
		})
	}
	_ = programFilesX86
	return apps
}
