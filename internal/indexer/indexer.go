package indexer

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"sync"

	"spotlight/internal/search"
)

const (
	defaultMaxDepth   = 8
	defaultMaxEntries = 50000
)

var skipDirNames = map[string]bool{
	".git":         true,
	".svn":         true,
	".hg":          true,
	".cache":       true,
	"node_modules": true,
	"vendor":       true,
	"__pycache__":  true,
	"target":       true,
	"dist":         true,
	"build":        true,
}

type Store struct {
	mu      sync.RWMutex
	entries []search.Entry
}

func NewStore() *Store { return &Store{} }

func (s *Store) Entries() []search.Entry {
	s.mu.RLock()
	defer s.mu.RUnlock()
	out := make([]search.Entry, len(s.entries))
	copy(out, s.entries)
	return out
}

func (s *Store) Add(entries ...search.Entry) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.entries = append(s.entries, entries...)
}

type Indexer struct {
	Store      *Store
	MaxDepth   int
	MaxEntries int
}

func NewIndexer(store *Store) *Indexer {
	return &Indexer{Store: store, MaxDepth: defaultMaxDepth, MaxEntries: defaultMaxEntries}
}

func (ix *Indexer) Run(ctx context.Context) {
	ix.Store.Add(DiscoverApps()...)
	ix.walkUserFiles(ctx)
}

func (ix *Indexer) walkUserFiles(ctx context.Context) {
	ix.walkRoots(ctx, fileRoots())
}

func (ix *Indexer) walkRoots(ctx context.Context, roots []string) {
	seen := make(map[string]bool)
	seenFiles := make(map[string]bool)
	count := 0
	for _, root := range roots {
		if ctx.Err() != nil {
			return
		}
		if seen[root] {
			continue
		}
		seen[root] = true
		err := filepath.WalkDir(root, func(path string, d os.DirEntry, err error) error {
			if ctx.Err() != nil {
				return ctx.Err()
			}
			if err != nil {
				return nil
			}
			if path == root {
				return nil
			}
			if d.IsDir() {
				if skipDirNames[d.Name()] || strings.HasPrefix(d.Name(), ".") {
					return filepath.SkipDir
				}
				return nil
			}
			if strings.HasPrefix(d.Name(), ".") {
				return nil
			}
			if ix.depth(root, path) > ix.MaxDepth {
				return filepath.SkipDir
			}
			if count >= ix.MaxEntries || seenFiles[path] {
				return nil
			}
			seenFiles[path] = true
			count++
			ix.Store.Add(search.Entry{Name: d.Name(), Path: path, Kind: search.KindFile})
			return nil
		})
		_ = err
	}
}

func (ix *Indexer) depth(root, path string) int {
	rel, err := filepath.Rel(root, path)
	if err != nil {
		return 0
	}
	return len(strings.Split(rel, string(filepath.Separator))) - 1
}

func fileRoots() []string {
	home, err := os.UserHomeDir()
	if err != nil {
		return nil
	}
	roots := []string{home}
	for _, sub := range []string{"Desktop", "Documents", "Downloads"} {
		p := filepath.Join(home, sub)
		if st, err := os.Stat(p); err == nil && st.IsDir() {
			roots = append(roots, p)
		}
	}
	return roots
}
