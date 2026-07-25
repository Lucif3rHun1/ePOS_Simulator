//go:build !windows

package winspool

import "fmt"

type Handle uintptr

func OpenPrinter(name string) (Handle, error) {
	return 0, fmt.Errorf("winspool: not available on this platform")
}

func ClosePrinter(h Handle) error {
	return fmt.Errorf("winspool: not available on this platform")
}

func PrintRaw(h Handle, docName string, data []byte) error {
	return fmt.Errorf("winspool: not available on this platform")
}
