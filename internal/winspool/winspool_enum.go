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
//
// Note: we previously tried to bounds-check the string pointers EnumPrintersW
// returns against the buffer we passed in. That was too strict: Windows
// printer drivers can hand back pointers to memory they manage themselves
// (not into our buffer), and those pointers are still valid UTF-16 strings.
// Bounds-checking dropped every printer on some real machines, leaving the
// picker empty. Now we trust the pointer and rely on recover() to catch the
// rare case where a driver returns a truly unreadable address.
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

	// safeStr reads a NUL-terminated UTF-16 string from p. UTF16PtrToString
	// will fault if p points to unreadable memory; recover() turns that into
	// an empty string so one misbehaving printer driver doesn't kill the
	// whole enumeration. The recovered printer shows up with an empty Name
	// (and gets dropped by filterEmpty in the picker).
	safeStr := func(p *uint16) (out string) {
		if p == nil {
			return ""
		}
		defer func() {
			if recover() != nil {
				out = ""
			}
		}()
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
	const nameCol = 55
	var sb strings.Builder
	sb.WriteString(fmt.Sprintf("%-4s %-9s %-55s %-20s\n", "#", "Default", "Name", "Port"))
	sb.WriteString(strings.Repeat("-", 95) + "\n")
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
		if len(name) > nameCol {
			name = name[:nameCol-3] + "..."
		}
		sb.WriteString(fmt.Sprintf("%-4d %-9s %-55s %-20s\n", i+1, marker, name, port))
	}
	return sb.String()
}
