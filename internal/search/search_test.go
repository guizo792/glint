package search

import (
	"reflect"
	"testing"
)

func TestRankTiers(t *testing.T) {
	entries := []Entry{
		{Name: "Firefox", Path: "/a", Kind: KindApp},
		{Name: "Firefox Developer", Path: "/b", Kind: KindApp},
		{Name: "MyFirefoxData", Path: "/c", Kind: KindFile},
		{Name: "XFoxY", Path: "/d", Kind: KindFile},
	}

	got := Rank("firefox", entries)
	want := []string{"Firefox", "Firefox Developer", "MyFirefoxData", "XFoxY"}
	for i, g := range got {
		if g.Name != want[i] {
			t.Fatalf("rank %d: got %q, want %q (all: %v)", i, g.Name, want[i], names(got))
		}
	}
}

func TestRankCaseInsensitive(t *testing.T) {
	entries := []Entry{{Name: "Spotlight", Path: "/s", Kind: KindApp}}
	got := Rank("SPOT", entries)
	if len(got) != 1 || got[0].Name != "Spotlight" {
		t.Fatalf("expected case-insensitive prefix match, got %v", names(got))
	}
}

func TestRankNoMatch(t *testing.T) {
	entries := []Entry{{Name: "Firefox", Path: "/f", Kind: KindApp}}
	if got := Rank("zzz", entries); len(got) != 0 {
		t.Fatalf("expected no results, got %v", names(got))
	}
}

func TestRankTieBreakShorterName(t *testing.T) {
	entries := []Entry{
		{Name: "LongerName", Path: "/l", Kind: KindApp},
		{Name: "Short", Path: "/s", Kind: KindApp},
	}
	got := Rank("t", entries)
	if got[0].Name != "Short" {
		t.Fatalf("expected shorter name first, got %v", names(got))
	}
}

func TestRankTieBreakLexicographic(t *testing.T) {
	entries := []Entry{
		{Name: "beta", Path: "/b", Kind: KindApp},
		{Name: "alpha", Path: "/a", Kind: KindApp},
	}
	got := Rank("a", entries)
	if got[0].Name != "alpha" {
		t.Fatalf("expected lexicographic order, got %v", names(got))
	}
}

func TestRankCap(t *testing.T) {
	entries := make([]Entry, 50)
	for i := range entries {
		entries[i] = Entry{Name: "App" + string(rune('a'+i)), Path: "/x", Kind: KindApp}
	}
	got := Rank("app", entries)
	if len(got) != MaxResults {
		t.Fatalf("expected cap of %d, got %d", MaxResults, len(got))
	}
}

func TestRankEmptyQuery(t *testing.T) {
	entries := make([]Entry, 30)
	for i := range entries {
		entries[i] = Entry{Name: "E", Path: "/e", Kind: KindFile}
	}
	got := Rank("", entries)
	if len(got) != MaxResults {
		t.Fatalf("expected %d quick entries, got %d", MaxResults, len(got))
	}
	if !reflect.DeepEqual(got, entries[:MaxResults]) {
		t.Fatal("quick entries must preserve input order")
	}
}

func names(entries []Entry) []string {
	out := make([]string, len(entries))
	for i, e := range entries {
		out[i] = e.Name
	}
	return out
}
