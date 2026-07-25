//go:build windows

package winspool

import (
	"fmt"
	"unsafe"

	"golang.org/x/sys/windows"
)

var (
	modwinspool       = windows.NewLazySystemDLL("winspool.drv")
	procOpenPrinterW  = modwinspool.NewProc("OpenPrinterW")
	procClosePrinter  = modwinspool.NewProc("ClosePrinter")
	procStartDoc      = modwinspool.NewProc("StartDocPrinterW")
	procEndDoc        = modwinspool.NewProc("EndDocPrinter")
	procWritePrinter  = modwinspool.NewProc("WritePrinter")
)

const (
	datatypeRAW = "RAW"
)

// OpenPrinter opens a handle to the named printer.
// If name is empty, uses the system default printer.
func OpenPrinter(name string) (windows.Handle, error) {
	var pName *uint16
	if name != "" {
		var err error
		pName, err = windows.UTF16PtrFromString(name)
		if err != nil {
			return 0, fmt.Errorf("invalid printer name: %w", err)
		}
	}
	var h windows.Handle
	r, _, err := procOpenPrinterW.Call(
		uintptr(unsafe.Pointer(pName)),
		uintptr(unsafe.Pointer(&h)),
		0,
	)
	if r == 0 {
		return 0, fmt.Errorf("OpenPrinter failed: %w", err)
	}
	return h, nil
}

// ClosePrinter releases the printer handle.
func ClosePrinter(h windows.Handle) error {
	r, _, err := procClosePrinter.Call(uintptr(h))
	if r == 0 {
		return fmt.Errorf("ClosePrinter failed: %w", err)
	}
	return nil
}

// docInfo1 corresponds to DOC_INFO_1W.
type docInfo1 struct {
	DocName *uint16
	DataType *uint16
	OutputFile *uint16
}

// PrintRaw sends raw byte data to the printer via the spooler.
func PrintRaw(h windows.Handle, docName string, data []byte) error {
	docNamePtr, _ := windows.UTF16PtrFromString(docName)
	dataTypePtr, _ := windows.UTF16PtrFromString(datatypeRAW)
	di := docInfo1{
		DocName:   docNamePtr,
		DataType:  dataTypePtr,
		OutputFile: nil,
	}

	r, _, err := procStartDoc.Call(
		uintptr(h),
		uintptr(unsafe.Pointer(&di)),
	)
	if r == 0 {
		return fmt.Errorf("StartDocPrinter failed: %w", err)
	}

	var written uint32
	r, _, err = procWritePrinter.Call(
		uintptr(h),
		uintptr(unsafe.Pointer(&data[0])),
		uintptr(len(data)),
		uintptr(unsafe.Pointer(&written)),
	)
	if r == 0 {
		procEndDoc.Call(uintptr(h))
		return fmt.Errorf("WritePrinter failed: %w", err)
	}

	r, _, err = procEndDoc.Call(uintptr(h))
	if r == 0 {
		return fmt.Errorf("EndDocPrinter failed: %w", err)
	}
	return nil
}
