package indexer

import (
	"context"
	"os"
	"path/filepath"
	"testing"

	"spotlight/internal/search"
)

func TestWalkRoots(t *testing.T) {
	root := t.TempDir()
	os.MkdirAll(filepath.Join(root, "Documents", "sub"), 0o755)
	os.MkdirAll(filepath.Join(root, ".hidden"), 0o755)
	os.MkdirAll(filepath.Join(root, "project", "node_modules"), 0o755)

	writeFile(t, filepath.Join(root, "readme.md"), "x")
	writeFile(t, filepath.Join(root, "Documents", "report.pdf"), "x")
	writeFile(t, filepath.Join(root, "Documents", "sub", "deep.txt"), "x")
	writeFile(t, filepath.Join(root, ".hidden", "secret.txt"), "x")
	writeFile(t, filepath.Join(root, "project", "main.go"), "x")
	writeFile(t, filepath.Join(root, "project", "node_modules", "pkg.js"), "x")
	writeFile(t, filepath.Join(root, ".gitignore"), "x")

	store := NewStore()
	ix := NewIndexer(store)
	ix.walkRoots(context.Background(), []string{root})

	entries := store.Entries()
	got := make(map[string]bool)
	for _, e := range entries {
		if e.Kind != search.KindFile {
			t.Fatalf("unexpected kind %q for %s", e.Kind, e.Path)
		}
		got[e.Name] = true
	}

	for _, want := range []string{"readme.md", "report.pdf", "deep.txt", "main.go"} {
		if !got[want] {
			t.Errorf("expected %q to be indexed, got %v", want, got)
		}
	}
	for _, not := range []string{"secret.txt", "pkg.js", ".gitignore"} {
		if got[not] {
			t.Errorf("expected %q to be excluded", not)
		}
	}
}

func TestWalkRootsDedupesPaths(t *testing.T) {
	root := t.TempDir()
	writeFile(t, filepath.Join(root, "shared.txt"), "x")

	store := NewStore()
	ix := NewIndexer(store)
	ix.walkRoots(context.Background(), []string{root, root, filepath.Join(root, "..", filepath.Base(root))})

	count := 0
	for _, e := range store.Entries() {
		if e.Name == "shared.txt" {
			count++
		}
	}
	if count != 1 {
		t.Fatalf("expected shared.txt indexed once, got %d", count)
	}
}

func TestWalkRootsDepthCap(t *testing.T) {
	root := t.TempDir()
	deep := filepath.Join(root, "a", "b", "c", "d", "e", "f", "g", "h")
	os.MkdirAll(deep, 0o755)
	writeFile(t, filepath.Join(root, "top.txt"), "x")
	writeFile(t, filepath.Join(deep, "too-deep.txt"), "x")

	store := NewStore()
	ix := NewIndexer(store)
	ix.MaxDepth = 3
	ix.walkRoots(context.Background(), []string{root})

	foundDeep := false
	for _, e := range store.Entries() {
		if e.Name == "too-deep.txt" {
			foundDeep = true
		}
	}
	if foundDeep {
		t.Fatal("expected file beyond depth cap to be excluded")
	}
}

func TestWalkRootsEntryCap(t *testing.T) {
	root := t.TempDir()
	for i := 0; i < 10; i++ {
		writeFile(t, filepath.Join(root, "file"+string(rune('0'+i))+".txt"), "x")
	}

	store := NewStore()
	ix := NewIndexer(store)
	ix.MaxEntries = 5
	ix.walkRoots(context.Background(), []string{root})

	if n := len(store.Entries()); n != 5 {
		t.Fatalf("expected entry cap of 5, got %d", n)
	}
}

func TestWalkRootsSkipsUnreadableRoot(t *testing.T) {
	root := t.TempDir()
	writeFile(t, filepath.Join(root, "ok.txt"), "x")

	store := NewStore()
	ix := NewIndexer(store)
	ix.walkRoots(context.Background(), []string{filepath.Join(root, "missing"), root})

	found := false
	for _, e := range store.Entries() {
		if e.Name == "ok.txt" {
			found = true
		}
	}
	if !found {
		t.Fatal("expected walker to continue past a missing root")
	}
}
