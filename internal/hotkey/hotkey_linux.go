//go:build linux

package hotkey

import (
	"errors"
	"log"
	"sync"

	"github.com/jezek/xgb"
	"github.com/jezek/xgb/xproto"
)

var (
	ErrRegistrationFailed = errors.New("hotkey: registration failed")
	ErrNoDisplay          = errors.New("hotkey: no X11 display available")
)

type manager struct {
	conn      *xgb.Conn
	keycode   xproto.Keycode
	triggered chan struct{}
	closeOnce sync.Once
	done      chan struct{}
}

const (
	keysymSpace = 0x20
	ctrlMask    = xproto.KeyButMaskControl
	lockBits    = xproto.KeyButMaskLock | xproto.KeyButMaskMod2 | xproto.KeyButMaskMod5
)

func New() (Manager, error) {
	conn, err := xgb.NewConn()
	if err != nil {
		return nil, ErrNoDisplay
	}
	keycode := findKeycode(conn, keysymSpace)
	if keycode == 0 {
		conn.Close()
		return nil, ErrRegistrationFailed
	}

	m := &manager{
		conn:      conn,
		keycode:   keycode,
		triggered: make(chan struct{}),
		done:      make(chan struct{}),
	}

	root := xproto.Setup(conn).DefaultScreen(conn).Root
	for _, mask := range grabMasks() {
		if err := xproto.GrabKeyChecked(conn, true, root, mask, keycode, xproto.GrabModeAsync, xproto.GrabModeAsync).Check(); err != nil {
			conn.Close()
			return nil, ErrRegistrationFailed
		}
	}

	go m.listen()
	return m, nil
}

func grabMasks() []uint16 {
	// The exact modifier mask, plus common lock-key variants
	// (CapsLock=Lock, NumLock=Mod2, ScrollLock=Mod5), so the hotkey
	// still fires when lock keys are active.
	base := uint16(ctrlMask)
	return []uint16{
		base,
		base | uint16(lockBits),
	}
}

func findKeycode(conn *xgb.Conn, keysym xproto.Keysym) xproto.Keycode {
	setup := xproto.Setup(conn)
	first := setup.MinKeycode
	count := setup.MaxKeycode - setup.MinKeycode + 1
	reply, err := xproto.GetKeyboardMapping(conn, first, byte(count)).Reply()
	if err != nil {
		return 0
	}
	per := int(reply.KeysymsPerKeycode)
	syms := reply.Keysyms
	for i := 0; i+per <= len(syms); i += per {
		for j := 0; j < per; j++ {
			if syms[i+j] == keysym {
				return first + xproto.Keycode(i/per)
			}
		}
	}
	return 0
}

func (m *manager) Triggered() <-chan struct{} { return m.triggered }

func (m *manager) Close() error {
	m.closeOnce.Do(func() {
		close(m.done)
		m.conn.Close()
	})
	return nil
}

func (m *manager) listen() {
	defer close(m.triggered)
	for {
		select {
		case <-m.done:
			return
		default:
		}
		ev, xerr := m.conn.WaitForEvent()
		if ev == nil && xerr == nil {
			return
		}
		if xerr != nil {
			log.Printf("hotkey: X error: %v", xerr)
			continue
		}
		if press, ok := ev.(xproto.KeyPressEvent); ok && press.Detail == m.keycode {
			select {
			case m.triggered <- struct{}{}:
			case <-m.done:
				return
			}
		}
	}
}
