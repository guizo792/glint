package ui

import (
	"strings"
	"sync"

	"fyne.io/fyne/v2"
	"fyne.io/fyne/v2/app"
	"fyne.io/fyne/v2/canvas"
	"fyne.io/fyne/v2/container"
	"fyne.io/fyne/v2/theme"
	"fyne.io/fyne/v2/widget"

	"spotlight/internal/search"
)

const overlayTitle = "Spotlight Launcher"

type Overlay struct {
	app      fyne.App
	win      fyne.Window
	entry    *keyEntry
	list     *widget.List
	items    []search.Entry
	selected int
	visible  bool
	onSearch func(string) []search.Entry
	onOpen   func(search.Entry)

	x11Once sync.Once
}

func NewOverlay(onSearch func(string) []search.Entry, onOpen func(search.Entry)) *Overlay {
	a := app.NewWithID("dev.spotlight.launcher")
	win := a.NewWindow(overlayTitle)
	o := &Overlay{app: a, win: win, onSearch: onSearch, onOpen: onOpen}
	o.build()
	return o
}

func (o *Overlay) build() {
	o.entry = &keyEntry{}
	o.entry.PlaceHolder = "Search apps and files…"
	o.entry.OnChanged = o.refresh

	o.list = widget.NewList(
		func() int { return len(o.items) },
		func() fyne.CanvasObject { return newResultRow().obj },
		func(id int, obj fyne.CanvasObject) {
			row := rows[obj]
			row.update(o.items[id], o.entry.Text)
			row.setSelected(id == o.selected)
		},
	)
	o.list.OnSelected = func(id int) {
		if id < len(o.items) {
			o.open(id)
		}
	}

	title := canvas.NewText("Spotlight", theme.Color(theme.ColorNamePrimary))
	title.TextStyle = fyne.TextStyle{Bold: true}
	top := container.NewVBox(title, o.entry)

	o.win.SetContent(container.NewBorder(container.NewPadded(top), nil, nil, nil, o.list))
	o.win.SetFixedSize(true)
	o.win.Resize(fyne.NewSize(640, 480))
	o.win.CenterOnScreen()
	o.win.SetCloseIntercept(func() { o.Hide() })
	o.win.SetMaster()

	o.entry.onUp = o.moveSelection(-1)
	o.entry.onDown = o.moveSelection(1)
	o.entry.onEnter = func() { o.open(o.selected) }
	o.entry.onEscape = o.Hide
}

func (o *Overlay) Run() {
	o.app.Run()
}

func (o *Overlay) Toggle() {
	if o.visible {
		o.Hide()
	} else {
		o.Show()
	}
}

func (o *Overlay) Show() {
	o.visible = true
	o.win.Show()
	o.win.RequestFocus()
	o.entry.SetText("")
	o.win.Canvas().Focus(o.entry)
	o.x11Once.Do(func() { applyX11Overlay(overlayTitle, func() { o.Hide() }) })
}

func (o *Overlay) Hide() {
	o.visible = false
	o.win.Hide()
}

func (o *Overlay) refresh(_ string) {
	o.items = o.onSearch(o.entry.Text)
	o.selected = 0
	o.list.Refresh()
}

func (o *Overlay) moveSelection(delta int) func() {
	return func() {
		n := len(o.items)
		if n == 0 {
			return
		}
		o.selected = (o.selected + delta + n) % n
		o.list.Refresh()
	}
}

func (o *Overlay) open(id int) {
	if id < 0 || id >= len(o.items) {
		return
	}
	o.onOpen(o.items[id])
	o.Hide()
}

type keyEntry struct {
	widget.Entry
	onUp     func()
	onDown   func()
	onEnter  func()
	onEscape func()
}

func (k *keyEntry) TypedKey(ev *fyne.KeyEvent) {
	switch ev.Name {
	case fyne.KeyUp:
		if k.onUp != nil {
			k.onUp()
		}
	case fyne.KeyDown:
		if k.onDown != nil {
			k.onDown()
		}
	case fyne.KeyReturn, fyne.KeyEnter:
		if k.onEnter != nil {
			k.onEnter()
		}
	case fyne.KeyEscape:
		if k.onEscape != nil {
			k.onEscape()
		}
	default:
		k.Entry.TypedKey(ev)
	}
}

type resultRow struct {
	bg       *canvas.Rectangle
	icon     *widget.Icon
	title    *widget.RichText
	subtitle *canvas.Text
	obj      fyne.CanvasObject
}

var rows = make(map[fyne.CanvasObject]*resultRow)

func newResultRow() *resultRow {
	bg := canvas.NewRectangle(theme.Color(theme.ColorNameHover))
	bg.Hide()
	icon := widget.NewIcon(theme.FileIcon())
	title := widget.NewRichText()
	title.Truncation = fyne.TextTruncateEllipsis
	sub := canvas.NewText("", theme.Color(theme.ColorNameDisabled))
	sub.TextSize = 12

	inner := container.NewHBox(icon, container.NewPadded(container.NewVBox(title, sub)))
	obj := container.NewStack(bg, container.NewPadded(inner))
	row := &resultRow{bg: bg, icon: icon, title: title, subtitle: sub, obj: obj}
	rows[obj] = row
	return row
}

func (r *resultRow) update(e search.Entry, query string) {
	if e.Kind == search.KindApp {
		r.icon.SetResource(theme.ComputerIcon())
	} else {
		r.icon.SetResource(theme.FileIcon())
	}
	r.subtitle.Text = e.Path
	r.title.Segments = highlightSegments(e.Name, query)
	r.title.Refresh()
	r.subtitle.Refresh()
}

func (r *resultRow) setSelected(sel bool) {
	r.bg.Hidden = !sel
	r.bg.Refresh()
}

func highlightSegments(name, query string) []widget.RichTextSegment {
	normal := widget.RichTextStyle{ColorName: theme.ColorNameForeground}
	if query == "" {
		return []widget.RichTextSegment{&widget.TextSegment{Text: name, Style: normal}}
	}
	start, end := matchRange(query, name)
	if start < 0 {
		return []widget.RichTextSegment{&widget.TextSegment{Text: name, Style: normal}}
	}
	hl := widget.RichTextStyle{
		ColorName: theme.ColorNamePrimary,
		TextStyle: fyne.TextStyle{Bold: true},
	}
	segs := []widget.RichTextSegment{}
	if start > 0 {
		segs = append(segs, &widget.TextSegment{Text: name[:start], Style: normal})
	}
	segs = append(segs, &widget.TextSegment{Text: name[start:end], Style: hl})
	if end < len(name) {
		segs = append(segs, &widget.TextSegment{Text: name[end:], Style: normal})
	}
	return segs
}

func matchRange(query, name string) (int, int) {
	q := strings.ToLower(query)
	n := strings.ToLower(name)
	if len(q) > len(n) {
		return -1, -1
	}
	if n == q {
		return 0, len(q)
	}
	if strings.HasPrefix(n, q) {
		return 0, len(q)
	}
	if idx := strings.Index(n, q); idx >= 0 {
		return idx, idx + len(q)
	}
	return -1, -1
}
