package logging

import (
	"fmt"
	"log"
	"os"
	"strings"
	"sync"
)

var (
	mu      sync.Mutex
	logFile *os.File
	verbose bool
)

// Init sets up rotating file logging.
func Init(v bool, logFilePath string) {
	verbose = v
	if logFilePath == "" {
		return
	}
	rotateIfNeeded(logFilePath)
	var err error
	logFile, err = os.OpenFile(logFilePath, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0644)
	if err != nil {
		log.Printf("[LOG] Cannot open %s: %v", logFilePath, err)
		return
	}
	log.SetOutput(logFile)
	log.SetFlags(log.Ldate | log.Ltime | log.Lmicroseconds)
}

func rotateIfNeeded(path string) {
	fi, err := os.Stat(path)
	if err != nil || fi.Size() < 10*1024*1024 {
		return
	}
	backup := path + ".1"
	os.Remove(backup)
	os.Rename(path, backup)
}

// Close closes the log file.
func Close() {
	mu.Lock()
	defer mu.Unlock()
	if logFile != nil {
		logFile.Close()
	}
}

// IsVerbose returns whether verbose logging is enabled.
func IsVerbose() bool { return verbose }

// HexDump returns a formatted hex dump of data.
func HexDump(label string, data []byte) string {
	if len(data) == 0 {
		return fmt.Sprintf("%s: (empty)", label)
	}
	var sb strings.Builder
	sb.WriteString(fmt.Sprintf("%s (%d bytes):\n", label, len(data)))
	for i := 0; i < len(data); i += 16 {
		end := i + 16
		if end > len(data) {
			end = len(data)
		}
		sb.WriteString(fmt.Sprintf("  %04x: ", i))
		for j := i; j < end; j++ {
			sb.WriteString(fmt.Sprintf("%02x ", data[j]))
		}
		for j := end; j < i+16; j++ {
			sb.WriteString("   ")
		}
		sb.WriteString(" ")
		for j := i; j < end; j++ {
			c := data[j]
			if c >= 32 && c < 127 {
				sb.WriteByte(c)
			} else {
				sb.WriteByte('.')
			}
		}
		sb.WriteByte(0x0A) // newline
	}
	return sb.String()
}

// LogXML logs raw XML body if verbose.
func LogXML(data []byte) {
	if verbose {
		log.Print(HexDump("[XML RX]", data))
	}
}

// LogESCPOS logs ESC/POS bytes if verbose.
func LogESCPOS(data []byte) {
	if verbose {
		log.Print(HexDump("[ESCPOS TX]", data))
	}
}
