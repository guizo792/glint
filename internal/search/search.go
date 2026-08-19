package search

import "sort"

const MaxResults = 20

type Kind string

const (
	KindApp  Kind = "app"
	KindFile Kind = "file"
)

type Entry struct {
	Name string
	Path string
	Kind Kind
	Icon string
}

type scored struct {
	entry Entry
	score int
}

func Rank(query string, entries []Entry) []Entry {
	q := normalize(query)
	if q == "" {
		return quickEntries(entries)
	}

	matched := make([]scored, 0, len(entries))
	for _, e := range entries {
		if s := score(q, normalize(e.Name)); s > 0 {
			matched = append(matched, scored{entry: e, score: s})
		}
	}

	sort.SliceStable(matched, func(i, j int) bool {
		a, b := matched[i], matched[j]
		if a.score != b.score {
			return a.score > b.score
		}
		if len(a.entry.Name) != len(b.entry.Name) {
			return len(a.entry.Name) < len(b.entry.Name)
		}
		return a.entry.Name < b.entry.Name
	})

	if len(matched) > MaxResults {
		matched = matched[:MaxResults]
	}

	out := make([]Entry, len(matched))
	for i, s := range matched {
		out[i] = s.entry
	}
	return out
}

func quickEntries(entries []Entry) []Entry {
	if len(entries) > MaxResults {
		entries = entries[:MaxResults]
	}
	return entries
}

func score(q, name string) int {
	if name == q {
		return 4
	}
	if hasPrefix(name, q) {
		return 3
	}
	if hasSubstring(name, q) {
		return 2
	}
	if hasFuzzy(name, q) {
		return 1
	}
	return 0
}

func normalize(s string) string {
	b := make([]rune, 0, len(s))
	for _, r := range s {
		if r >= 'A' && r <= 'Z' {
			r += 'a' - 'A'
		}
		b = append(b, r)
	}
	return string(b)
}

func hasPrefix(name, q string) bool {
	return len(q) <= len(name) && name[:len(q)] == q
}

func hasSubstring(name, q string) bool {
	if len(q) > len(name) {
		return false
	}
	for i := 0; i+len(q) <= len(name); i++ {
		if name[i:i+len(q)] == q {
			return true
		}
	}
	return false
}

func hasFuzzy(name, q string) bool {
	qi := 0
	for i := 0; i < len(name) && qi < len(q); i++ {
		if name[i] == q[qi] {
			qi++
		}
	}
	return qi == len(q)
}
