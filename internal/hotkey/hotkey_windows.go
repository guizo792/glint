//go:build windows

package hotkey

import (
	"errors"

	ghk "golang.design/x/hotkey"
)

var ErrRegistrationFailed = errors.New("hotkey: registration failed")

type manager struct {
	hk        *ghk.Hotkey
	triggered chan struct{}
	done      chan struct{}
}

func New() (Manager, error) {
	hk := ghk.New([]ghk.Modifier{ghk.ModCtrl}, ghk.KeySpace)
	if err := hk.Register(); err != nil {
		return nil, ErrRegistrationFailed
	}
	m := &manager{
		hk:        hk,
		triggered: make(chan struct{}),
		done:      make(chan struct{}),
	}
	go m.listen()
	return m, nil
}

func (m *manager) Triggered() <-chan struct{} { return m.triggered }

func (m *manager) Close() error {
	select {
	case <-m.done:
		return nil
	default:
	}
	close(m.done)
	m.hk.Unregister()
	return nil
}

func (m *manager) listen() {
	defer close(m.triggered)
	for {
		select {
		case <-m.done:
			return
		case <-m.hk.Keydown():
			select {
			case m.triggered <- struct{}{}:
			case <-m.done:
				return
			}
		}
	}
}
