package launcher

import (
	"fmt"

	"spotlight/internal/search"
)

func Open(e search.Entry) error {
	switch e.Kind {
	case search.KindApp:
		return launchApp(e.Path)
	case search.KindFile:
		return openFile(e.Path)
	default:
		return fmt.Errorf("launcher: unknown entry kind %q", e.Kind)
	}
}
