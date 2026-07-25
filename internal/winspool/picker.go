package winspool

import (
	"bufio"
	"errors"
	"fmt"
	"io"
	"os"
	"strings"
)

// Picker resolves a printer name from a flag value plus an interactive fallback.
type Picker interface {
	// Pick returns the printer name to use. Returns ("", ErrUserCancelled)
	// if the user cancelled interactive selection.
	Pick() (string, error)
}

// StaticPicker returns a fixed name (or "" for system default).
type StaticPicker struct {
	Name string
}

func (s StaticPicker) Pick() (string, error) { return s.Name, nil }

// InteractivePicker enumerates printers, lists them, and reads user input
// from in. If user picks nothing and fallback is non-empty, fallback is used.
// If user picks nothing and fallback is empty, returns ("", ErrUserCancelled).
type InteractivePicker struct {
	In       io.Reader
	Out      io.Writer
	Fallback string
}

func (p InteractivePicker) Pick() (string, error) {
	infos, err := EnumPrinters()
	if err != nil {
		return "", fmt.Errorf("enumerate printers: %w", err)
	}
	if len(infos) == 0 {
		return p.Fallback, nil
	}
	if p.Out != nil {
		fmt.Fprintln(p.Out, "")
		fmt.Fprintln(p.Out, "Available printers:")
		fmt.Fprint(p.Out, FormatList(infos))
		fmt.Fprintln(p.Out, "")
		fmt.Fprintln(p.Out, "No --printer flag given. Enter the number to select,")
		fmt.Fprintln(p.Out, "or press Enter to use the default (marked with *).")
		fmt.Fprint(p.Out, "> ")
	}
	scanner := bufio.NewScanner(p.In)
	if !scanner.Scan() {
		if p.Fallback != "" {
			return p.Fallback, nil
		}
		return "", ErrUserCancelled
	}
	line := strings.TrimSpace(scanner.Text())
	if line == "" {
		// Use default
		for _, inf := range infos {
			if inf.IsDefault {
				return inf.Name, nil
			}
		}
		return p.Fallback, nil
	}
	// Parse number or name
	n := 0
	if _, err := fmt.Sscanf(line, "%d", &n); err == nil {
		if n >= 1 && n <= len(infos) {
			return infos[n-1].Name, nil
		}
	}
	// Match by name (case-insensitive prefix)
	for _, inf := range infos {
		if strings.EqualFold(inf.Name, line) {
			return inf.Name, nil
		}
	}
	return line, nil // allow free-form names in case user has a printer not enumerated
}

// DefaultWithFallback returns the flag value if non-empty, else falls back to
// InteractivePicker.Fallback. Does not prompt. Useful when stdin is not a TTY.
func DefaultWithFallback(name, fallback string) string {
	if name != "" {
		return name
	}
	return fallback
}

// ErrUserCancelled is returned when the user aborts an interactive pick.
var ErrUserCancelled = errors.New("winspool: printer selection cancelled")

// IsInteractive returns true if stdin appears to be a terminal (not piped).
// Used by main to decide whether to show InteractivePicker.
func IsInteractive() bool {
	fi, err := os.Stdin.Stat()
	if err != nil {
		return false
	}
	return (fi.Mode() & os.ModeCharDevice) != 0
}
