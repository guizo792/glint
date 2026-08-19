//go:build linux

package ui

import (
	"encoding/binary"
	"errors"
	"log"

	"fyne.io/fyne/v2"
	"github.com/jezek/xgb"
	"github.com/jezek/xgb/xproto"
)

// applyX11Overlay makes the window frameless on X11 and hides the overlay
// when it loses keyboard focus (covers "click outside closes").
func applyX11Overlay(title string, onFocusLost func()) {
	conn, err := xgb.NewConn()
	if err != nil {
		log.Printf("ui: x11 overlay setup skipped: %v", err)
		return
	}
	win, err := findWindowByTitle(conn, title)
	if err != nil || win == 0 {
		conn.Close()
		return
	}
	removeDecorations(conn, win)
	go watchFocus(conn, win, onFocusLost)
}

func findWindowByTitle(conn *xgb.Conn, title string) (xproto.Window, error) {
	root := xproto.Setup(conn).DefaultScreen(conn).Root
	return findInTree(conn, root, title, 0)
}

func findInTree(conn *xgb.Conn, win xproto.Window, title string, depth int) (xproto.Window, error) {
	if depth > 8 {
		return 0, nil
	}
	name, err := windowName(conn, win)
	if err == nil && name == title {
		return win, nil
	}
	tree, err := xproto.QueryTree(conn, win).Reply()
	if err != nil {
		return 0, err
	}
	for _, child := range tree.Children {
		found, err := findInTree(conn, child, title, depth+1)
		if err != nil {
			return 0, err
		}
		if found != 0 {
			return found, nil
		}
	}
	return 0, nil
}

func windowName(conn *xgb.Conn, win xproto.Window) (string, error) {
	for _, atomName := range []string{"_NET_WM_NAME", "WM_NAME"} {
		atom, err := internAtom(conn, atomName)
		if err != nil {
			continue
		}
		prop, err := xproto.GetProperty(conn, false, win, atom, 0, 0, 4096).Reply()
		if err != nil || prop == nil || len(prop.Value) == 0 {
			continue
		}
		return string(prop.Value), nil
	}
	return "", errors.New("no window name property")
}

func internAtom(conn *xgb.Conn, name string) (xproto.Atom, error) {
	reply, err := xproto.InternAtom(conn, false, uint16(len(name)), name).Reply()
	if err != nil {
		return 0, err
	}
	return reply.Atom, nil
}

// removeDecorations sets _MOTIF_WM_HINTS with decorations disabled,
// the standard X11 way to make a window frameless.
func removeDecorations(conn *xgb.Conn, win xproto.Window) {
	atom, err := internAtom(conn, "_MOTIF_WM_HINTS")
	if err != nil {
		return
	}
	// flags=decorations(2), functions=0, decorations=0, inputMode=0, status=0
	hints := []uint32{2, 0, 0, 0, 0}
	data := make([]byte, 0, len(hints)*4)
	for _, v := range hints {
		data = binary.LittleEndian.AppendUint32(data, v)
	}
	err = xproto.ChangePropertyChecked(conn, xproto.PropModeReplace, win,
		atom, xproto.AtomCardinal, 32, uint32(len(hints)), data).Check()
	if err != nil {
		log.Printf("ui: remove decorations failed: %v", err)
	}
}

func watchFocus(conn *xgb.Conn, win xproto.Window, onFocusLost func()) {
	defer conn.Close()
	for {
		ev, xerr := conn.WaitForEvent()
		if ev == nil && xerr == nil {
			return
		}
		if xerr != nil {
			continue
		}
		if fe, ok := ev.(xproto.FocusOutEvent); ok && fe.Event == win {
			fyne.Do(onFocusLost)
		}
	}
}
