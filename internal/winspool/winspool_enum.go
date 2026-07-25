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

// printerInfo2W matches Windows SDK winspool.h PRINTER_INFO_2W on amd64.
// 13 pointers (8 bytes each) + 6 DWORDs (4 bytes each) = 128 bytes total.
// Used only as a typed overlay over the raw EnumPrintersW buffer; we never
// dereference DevMode/SecurityDescriptor (uintptr placeholders).
type printerInfo2W struct {
	ServerName         *uint16
	PrinterName        *uint16
	ShareName          *uint16
	PortName           *uint16
	DriverName         *uint16
	Comment            *uint16
	Location           *uint16
	DevMode            uintptr
	SepFile            *uint16
	PrintProcessor     *uint16
	Datatype           *uint16
	Parameters         *uint16
	SecurityDescriptor uintptr
	Attributes         uint32
	Priority           uint32
	DefaultPriority    uint32
	StartTime          uint32
	UntilTime          uint32
	Status             uint32
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

	// Capture buffer bounds so we can validate the pointers EnumPrintersW
	// hands us. If a printer driver returns a pointer that does not point
	// inside this buffer, the call to UTF16PtrToString will fault. We saw
	// this on a real Windows machine: pPrinterName was 0x100001a40 (a
	// non-nil, non-readable address) and crashed the process at startup.
	bufStart := uintptr(unsafe.Pointer(&buf[0]))
	bufEnd := bufStart + uintptr(len(buf))

	safeStr := func(p *uint16) string {
		if p == nil {
			return ""
		}
		addr := uintptr(unsafe.Pointer(p))
		if addr < bufStart || addr >= bufEnd {
			return ""
		}
		return windows.UTF16PtrToString(p)
	}

	recordSize := unsafe.Sizeof(printerInfo2W{})
	for i := uint32(0); i < returned; i++ {
		base := uintptr(i) * recordSize
		if base+recordSize > uintptr(len(buf)) {
			break
		}
		info := (*printerInfo2W)(unsafe.Pointer(&buf[base]))

		name := safeStr(info.PrinterName)
		infos = append(infos, PrinterInfo{
			Name:       name,
			ServerName: safeStr(info.ServerName),
			ShareName:  safeStr(info.ShareName),
			PortName:   safeStr(info.PortName),
			DriverName: safeStr(info.DriverName),
			Comment:    safeStr(info.Comment),
			Location:   safeStr(info.Location),
			IsDefault:  name != "" && name == defaultName,
		})
	}
	return filterEmpty(infos), nil
}

func getDefaultPrinterName() string {
	mod := windows.NewLazySystemDLL("winspool.drv")
	proc := mod.NewProc("GetDefaultPrinterW")
	var needed uint32
	// First pass: get required buffer size.
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
