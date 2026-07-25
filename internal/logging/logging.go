// Package logging provides cross-platform structured logging with rotation
// and per-request context. Replaces the stdlib `log` calls in this app.
package logging

import (
	"fmt"
	"io"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"time"
)

// Level represents log severity.
type Level int

const (
	LevelDebug Level = iota
	LevelInfo
	LevelWarn
	LevelError
)

func (l Level) String() string {
	switch l {
	case LevelDebug:
		return "DEBUG"
	case LevelInfo:
		return "INFO"
	case LevelWarn:
		return "WARN"
	case LevelError:
		return "ERROR"
	default:
		return "?"
	}
}

// Config controls logging behavior.
type Config struct {
	Verbose     bool   // enable Debug level
	LogFile     string // rotating log file path (empty = stderr only)
	MaxBytes    int64  // rotate when file exceeds this size (default 10MB)
	MaxBackups  int    // keep N rotated backups (.1, .2, ..., .N) (default 3)
	IncludePID  bool   // include process ID in log lines (default true)
}

var (
	mu          sync.Mutex
	cfg         Config
	logFile     *os.File
	logFileSize int64
	currentVerb bool // cached Verbose
)

// Init configures logging. Safe to call once at startup.
func Init(c Config) {
	mu.Lock()
	defer mu.Unlock()

	cfg = c
	currentVerb = c.Verbose

	if c.MaxBytes == 0 {
		cfg.MaxBytes = 10 * 1024 * 1024
	}
	if c.MaxBackups == 0 {
		cfg.MaxBackups = 3
	}

	if c.LogFile != "" {
		// Open (or create) the log file in append mode.
		f, err := os.OpenFile(c.LogFile, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0644)
		if err != nil {
			fmt.Fprintf(os.Stderr, "[LOG] Cannot open %s: %v\n", c.LogFile, err)
			return
		}
		logFile = f
		fi, err := f.Stat()
		if err == nil {
			logFileSize = fi.Size()
		}
	}
}

// Close flushes and closes the log file.
func Close() {
	mu.Lock()
	defer mu.Unlock()
	if logFile != nil {
		logFile.Close()
		logFile = nil
	}
}

// IsVerbose returns true if Debug level is enabled.
func IsVerbose() bool {
	mu.Lock()
	defer mu.Unlock()
	return currentVerb
}

// SetVerbose toggles Debug level at runtime.
func SetVerbose(v bool) {
	mu.Lock()
	defer mu.Unlock()
	currentVerb = v
	cfg.Verbose = v
}

// Log emits a structured log entry.
// fields is a key-value list (key1, val1, key2, val2, ...).
func Log(level Level, msg string, fields ...any) {
	if !shouldLog(level) {
		return
	}
	line := formatLine(level, msg, fields)
	writeLine(line)
}

func shouldLog(level Level) bool {
	mu.Lock()
	defer mu.Unlock()
	if level == LevelDebug && !currentVerb {
		return false
	}
	return true
}

// Convenience wrappers.
func Debug(msg string, fields ...any) { Log(LevelDebug, msg, fields...) }
func Info(msg string, fields ...any)  { Log(LevelInfo, msg, fields...) }
func Warn(msg string, fields ...any)  { Log(LevelWarn, msg, fields...) }
func Error(msg string, fields ...any) { Log(LevelError, msg, fields...) }

func formatLine(level Level, msg string, fields []any) string {
	ts := time.Now().UTC().Format("2006-01-02T15:04:05.000Z")
	var sb strings.Builder
	sb.WriteString(ts)
	sb.WriteByte(' ')
	sb.WriteString(level.String())
	sb.WriteByte(' ')
	if cfg.IncludePID {
		fmt.Fprintf(&sb, "pid=%d ", os.Getpid())
	}
	sb.WriteString(strings.TrimSpace(msg))
	if len(fields) > 0 {
		sb.WriteString(" |")
		for i := 0; i+1 < len(fields); i += 2 {
			fmt.Fprintf(&sb, " %v=%v", fields[i], fields[i+1])
		}
		if len(fields)%2 == 1 {
			fmt.Fprintf(&sb, " extra=%v", fields[len(fields)-1])
		}
	}
	sb.WriteByte('\n')
	return sb.String()
}

func writeLine(line string) {
	mu.Lock()
	defer mu.Unlock()

	// Always write to stderr.
	os.Stderr.WriteString(line)

	if logFile != nil {
		logFileSize += int64(len(line))
		if logFileSize >= cfg.MaxBytes {
			rotateLocked()
		}
		logFile.WriteString(line)
	}
}

func rotateLocked() {
	logFile.Close()
	logFile = nil

	// Shift backups: .N -> deleted, .(N-1) -> .N, ..., .1 -> .2
	for i := cfg.MaxBackups; i >= 1; i-- {
		old := cfg.LogFile + "." + fmt.Sprintf("%d", i)
		var src string
		if i == 1 {
			src = cfg.LogFile
		} else {
			src = cfg.LogFile + "." + fmt.Sprintf("%d", i-1)
		}
		if _, err := os.Stat(src); err == nil {
			os.Rename(src, old)
		}
	}

	f, err := os.OpenFile(cfg.LogFile, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0644)
	if err != nil {
		fmt.Fprintf(os.Stderr, "[LOG] rotate reopen failed: %v\n", err)
		return
	}
	logFile = f
	logFileSize = 0
}

// HexDump returns a formatted hex dump of data (cross-platform).
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
		fmt.Fprintf(&sb, "  %04x: ", i)
		for j := i; j < end; j++ {
			fmt.Fprintf(&sb, "%02x ", data[j])
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
		sb.WriteByte('\n')
	}
	return sb.String()
}

// LogXML logs raw XML body if verbose.
func LogXML(label string, data []byte) {
	if !IsVerbose() {
		return
	}
	Info("hex dump", "label", label, "size", len(data))
	Debug(HexDump(label, data))
}

// LogESCPOS logs ESC/POS bytes if verbose.
func LogESCPOS(label string, data []byte) {
	if !IsVerbose() {
		return
	}
	Info("hex dump", "label", label, "size", len(data))
	Debug(HexDump(label, data))
}

// Platform returns a short platform identifier for log lines.
func Platform() string {
	return runtime.GOOS + "/" + runtime.GOARCH
}

// Ensure the unused io import is referenced (kept for future io.Writer sinks).
var _ io.Writer = (*os.File)(nil)

// filepath imported for callers that need log directory helpers.
var _ = filepath.Separator
