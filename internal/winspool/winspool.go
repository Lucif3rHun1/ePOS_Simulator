//go:build windows

package winspool

import (
	"fmt"
	"unsafe"

	"golang.org/x/sys/windows"
)

var (
	modwinspool      = windows.NewLazySystemDLL("winspool.drv")
	procOpenPrinterW = modwinspool.NewProc("OpenPrinterW")
	procClosePrinter = modwinspool.NewProc("ClosePrinter")
	procStartDoc     = modwinspool.NewProc("StartDocPrinterW")
	procEndDoc       = modwinspool.NewProc("EndDocPrinter")
	procWritePrinter = modwinspool.NewProc("WritePrinter")
)

const (
	datatypeRAW = "RAW"
	// PRINTER_ACCESS_USE: needed to send jobs to the printer.
	// https://learn.microsoft.com/en-us/windows/win32/printdocs/printer-access-use
	printerAccessUse = 0x00000008
)

// PRINTER_DEFAULTSW: passed as pDefault to OpenPrinterW. Declaring
// pDatatype = "RAW" up front is what makes the spooler treat the handle
// as a raw-byte stream. Without it, drivers default to text mode and
// reject subsequent StartDocPrinterW(RAW) with ERROR_INVALID_LEVEL (1242).
// https://learn.microsoft.com/en-us/windows/win32/printdocs/openprinter
type printerDefaults struct {
	pDatatype     *uint16
	pDevMode      uintptr // NULL — driver uses its default
	DesiredAccess uint32
}

var (
	hRawDefaults printerDefaults
	rawDefaultsInit bool
)

func initRawDefaults() {
	if rawDefaultsInit {
		return
	}
	p, _ := windows.UTF16PtrFromString(datatypeRAW)
	hRawDefaults = printerDefaults{
		pDatatype:     p,
		pDevMode:      0,
		DesiredAccess: printerAccessUse,
	}
	rawDefaultsInit = true
}

// OpenPrinter opens a handle to the named printer with RAW datatype pre-declared.
// If name is empty, uses the system default printer.
func OpenPrinter(name string) (windows.Handle, error) {
	initRawDefaults()

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
		uintptr(unsafe.Pointer(&hRawDefaults)),
	)
	if r == 0 {
		return 0, fmt.Errorf("OpenPrinter failed: %w (does the printer exist? check --list-printers)", err)
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

// DOC_INFO_1W: three pointer fields, in order. Pass NULL for OutputFile
// to send the output to the printer (vs. to a file on disk).
// https://learn.microsoft.com/en-us/windows/win32/printdocs/doc-info-1
type docInfo1 struct {
	DocName    *uint16
	DataType   *uint16
	OutputFile *uint16
}

// PrintRaw sends raw byte data to the printer via the spooler.
// The handle must have been opened with PRINTER_DEFAULTS specifying RAW
// datatype (see OpenPrinter above).
func PrintRaw(h windows.Handle, docName string, data []byte) error {
	if len(data) == 0 {
		return fmt.Errorf("PrintRaw: empty data")
	}

	docNamePtr, err := windows.UTF16PtrFromString(docName)
	if err != nil {
		return fmt.Errorf("invalid doc name: %w", err)
	}
	dataTypePtr, err := windows.UTF16PtrFromString(datatypeRAW)
	if err != nil {
		return fmt.Errorf("invalid datatype: %w", err)
	}
	di := docInfo1{
		DocName:    docNamePtr,
		DataType:   dataTypePtr,
		OutputFile: nil,
	}

	r, _, e := procStartDoc.Call(
		uintptr(h),
		uintptr(unsafe.Pointer(&di)),
	)
	if r == 0 {
		return fmt.Errorf("StartDocPrinter failed: %w (handle may not have been opened for RAW datatype, or printer driver rejected RAW)", e)
	}

	var written uint32
	r, _, e = procWritePrinter.Call(
		uintptr(h),
		uintptr(unsafe.Pointer(&data[0])),
		uintptr(len(data)),
		uintptr(unsafe.Pointer(&written)),
	)
	if r == 0 {
		// best-effort cleanup
		procEndDoc.Call(uintptr(h))
		return fmt.Errorf("WritePrinter failed: %w (wrote %d of %d bytes)", e, written, len(data))
	}

	if written != uint32(len(data)) {
		procEndDoc.Call(uintptr(h))
		return fmt.Errorf("WritePrinter short write: %d of %d bytes", written, len(data))
	}

	r, _, e = procEndDoc.Call(uintptr(h))
	if r == 0 {
		return fmt.Errorf("EndDocPrinter failed: %w", e)
	}
	return nil
}
