// Package winspool provides Windows print spooler access for RAW ESC/POS bytes.
// The stub files (winspool_stub.go, winspool_enum_stub.go) return ErrUnsupported
// on non-Windows so callers can use errors.Is to detect platform absence.
package winspool

import "errors"

// ErrUnsupported is returned by stub implementations on non-Windows platforms.
// Callers should use errors.Is(err, winspool.ErrUnsupported) to detect that
// the failure is environmental (no winspool on this OS) rather than a real
// error (printer not found, access denied, etc.).
var ErrUnsupported = errors.New("winspool: not available on this platform")
