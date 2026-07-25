//go:build !windows

package winspool

type Handle uintptr

func OpenPrinter(name string) (Handle, error) {
	return 0, ErrUnsupported
}

func ClosePrinter(h Handle) error {
	return ErrUnsupported
}

func PrintRaw(h Handle, docName string, data []byte) error {
	return ErrUnsupported
}
