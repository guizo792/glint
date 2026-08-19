package main

import (
	"context"
	"log"

	"fyne.io/fyne/v2"

	"spotlight/internal/hotkey"
	"spotlight/internal/indexer"
	"spotlight/internal/launcher"
	"spotlight/internal/search"
	"spotlight/internal/ui"
)

func main() {
	store := indexer.NewStore()
	ix := indexer.NewIndexer(store)
	go ix.Run(context.Background())

	onSearch := func(q string) []search.Entry {
		return search.Rank(q, store.Entries())
	}

	o := ui.NewOverlay(onSearch, func(e search.Entry) {
		if err := launcher.Open(e); err != nil {
			log.Printf("launcher: failed to open %q: %v", e.Name, err)
		}
	})

	hk, err := hotkey.New()
	if err != nil {
		log.Printf("hotkey: %v", err)
	} else {
		defer hk.Close()
		go func() {
			for range hk.Triggered() {
				fyne.Do(o.Toggle)
			}
		}()
	}

	o.Run()
}