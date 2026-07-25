//go:build windows

package winspool

import (
	"fmt"
	"strings"
	"syscall"
	"unsafe"

	"golang.org/x/sys/windows"
)

// PrinterInfo represents an enumerated printer.
type PrinterInfo struct {
	Name       string
	ServerName string
	ShareName  string
	PortName   string
	DriverName string
	Comment    string
	Location   string
	IsDefault  bool
}

// EnumPrinters lists all printers visible to the local spooler.
// Uses EnumPrintersW with PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS.
func EnumPrinters() ([]PrinterInfo, error) {
	const (
		PRINTER_ENUM_LOCAL       = 0x00000002
		PRINTER_ENUM_CONNECTIONS = 0x00000004
		PRINTER_INFO_LEVEL       = 2
	)

	mod := windows.NewLazySystemDLL("winspool.drv")
	enumProc := mod.NewProc("EnumPrintersW")

	var needed, returned uint32

	// First call: query required buffer size.
	r1, _, _ := enumProc.Call(
		uintptr(PRINTER_ENUM_LOCAL|PRINTER_ENUM_CONNECTIONS),
		0,
		uintptr(PRINTER_INFO_LEVEL),
		0,
		0,
		uintptr(unsafe.Pointer(&needed)),
		uintptr(unsafe.Pointer(&returned)),
	)
	if needed == 0 {
		if r1 == 0 {
			return nil, fmt.Errorf("EnumPrinters initial call failed: %v", syscall.GetLastError())
		}
		return nil, nil
	}

	buf := make([]byte, needed)
	r1, _, _ = enumProc.Call(
		uintptr(PRINTER_ENUM_LOCAL|PRINTER_ENUM_CONNECTIONS),
		0,
		uintptr(PRINTER_INFO_LEVEL),
		uintptr(unsafe.Pointer(&buf[0])),
		uintptr(needed),
		uintptr(unsafe.Pointer(&needed)),
		uintptr(unsafe.Pointer(&returned)),
	)
	if r1 == 0 {
		return nil, fmt.Errorf("EnumPrinters second call failed: %v", syscall.GetLastError())
	}

	defaultName := getDefaultPrinterName()
	infos := make([]PrinterInfo, 0, returned)

	// PRINTER_INFO_2W on Windows amd64: 12 pointers (96 bytes) + 5 DWORDs (20 bytes) = 116 bytes, padded to 120.
	// We extract only the first 7 string pointers (Server, Printer, Share, Port, Driver, Comment, Location)
	// using fixed offsets — these are stable across Windows versions.
	const (
		OFF_PPRINTERNAME = 8  // 2nd pointer
		OFF_PSERVERNAME  = 0  // 1st pointer
		OFF_PSHARENAME   = 16 // 3rd pointer
		OFF_PPORTNAME    = 24 // 4th pointer
		OFF_PDRIVERNAME  = 32 // 5th pointer
		OFF_PCOMMENT     = 40 // 6th pointer
		OFF_PLOCATION    = 48 // 7th pointer
		RECORD_SIZE      = 120
	)

	for i := uint32(0); i < returned; i++ {
		base := uintptr(i) * RECORD_SIZE
		getStr := func(off uintptr) string {
			ptr := *(**uint16)(unsafe.Pointer(&buf[base+off]))
			if ptr == nil {
				return ""
			}
			return windows.UTF16PtrToString(ptr)
		}
		name := getStr(OFF_PPRINTERNAME)
		infos = append(infos, PrinterInfo{
			Name:       name,
			ServerName: getStr(OFF_PSERVERNAME),
			ShareName:  getStr(OFF_PSHARENAME),
			PortName:   getStr(OFF_PPORTNAME),
			DriverName: getStr(OFF_PDRIVERNAME),
			Comment:    getStr(OFF_PCOMMENT),
			Location:   getStr(OFF_PLOCATION),
			IsDefault:  name == defaultName,
		})
	}
	return infos, nil
}

func getDefaultPrinterName() string {
	mod := windows.NewLazySystemDLL("winspool.drv")
	proc := mod.NewProc("GetDefaultPrinterW")
	var needed uint32
	// First pass: get required buffer size (returns needed size even on success-with-buffer-too-small).
	proc.Call(0, uintptr(unsafe.Pointer(&needed)))
	if needed == 0 {
		return ""
	}
	name := make([]uint16, needed)
	r1, _, _ := proc.Call(uintptr(unsafe.Pointer(&name[0])), uintptr(unsafe.Pointer(&needed)))
	if r1 == 0 {
		return ""
	}
	return windows.UTF16ToString(name)
}

// FormatList returns a human-readable listing for CLI output.
func FormatList(infos []PrinterInfo) string {
	if len(infos) == 0 {
		return "No printers found.\n"
	}
	var sb strings.Builder
	sb.WriteString(fmt.Sprintf("%-4s %-9s %-40s %-20s\n", "#", "Default", "Name", "Port"))
	sb.WriteString(strings.Repeat("-", 80) + "\n")
	for i, p := range infos {
		marker := ""
		if p.IsDefault {
			marker = "*"
		}
		port := p.PortName
		if len(port) > 20 {
			port = port[:17] + "..."
		}
		name := p.Name
		if len(name) > 40 {
			name = name[:37] + "..."
		}
		sb.WriteString(fmt.Sprintf("%-4d %-9s %-40s %-20s\n", i+1, marker, name, port))
	}
	return sb.String()
}
