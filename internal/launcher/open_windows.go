//go:build windows

package launcher

import (
	"os/exec"
)

func launchApp(execLine string) error {
	cmd := appCommand(execLine)
	return cmd.Start()
}

func openFile(path string) error {
	cmd := fileCommand(path)
	return cmd.Start()
}

func appCommand(execLine string) *exec.Cmd {
	return exec.Command("cmd", "/c", "start", "", execLine)
}

func fileCommand(path string) *exec.Cmd {
	return exec.Command("cmd", "/c", "start", "", path)
}
