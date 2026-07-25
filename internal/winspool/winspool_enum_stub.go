//go:build !windows

package winspool

import (
	"fmt"
	"strings"
)

// PrinterInfo represents an enumerated printer (stub on non-Windows).
type PrinterInfo struct {
	Name        string
	ServerName  string
	ShareName   string
	PortName    string
	DriverName  string
	Comment     string
	Location    string
	IsDefault   bool
}

// EnumPrinters returns a single fake printer on non-Windows for dev/testing.
func EnumPrinters() ([]PrinterInfo, error) {
	return []PrinterInfo{
		{
			Name:      "FakePrinter (non-Windows stub)",
			PortName:  "FAKE",
			DriverName: "Generic / Text Only",
			IsDefault: true,
			Comment:   "Emulator is running on non-Windows; this is a stub. Real enumeration requires Windows.",
		},
	}, nil
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
