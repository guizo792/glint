package hotkey

type Manager interface {
	Triggered() <-chan struct{}
	Close() error
}
