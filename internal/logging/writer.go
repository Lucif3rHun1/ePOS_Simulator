package logging

import (
	"fmt"
	"io"
	"os"
	"sync"
)

// RotateWriter is an io.Writer that rotates the underlying file when it
// exceeds a size threshold. Composes with the structured logger above.
// Used as a sink for log output when --log-file is set; not required by
// callers using the package-level Debug/Info/Warn/Error helpers (those
// manage rotation internally via state in logging.go).
type RotateWriter struct {
	mu         sync.Mutex
	path       string
	maxBytes   int64
	maxBackups int
	f          *os.File
	size       int64
}

// NewRotateWriter opens (or creates) path in append mode and returns a writer
// that rotates when the file exceeds maxBytes, keeping up to maxBackups
// backups (path.1, path.2, ..., path.N).
func NewRotateWriter(path string, maxBytes int64, maxBackups int) (*RotateWriter, error) {
	if path == "" {
		return nil, fmt.Errorf("RotateWriter: empty path")
	}
	if maxBytes <= 0 {
		maxBytes = 10 * 1024 * 1024
	}
	if maxBackups <= 0 {
		maxBackups = 3
	}
	f, err := os.OpenFile(path, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0644)
	if err != nil {
		return nil, err
	}
	fi, err := f.Stat()
	if err != nil {
		f.Close()
		return nil, err
	}
	return &RotateWriter{
		path:       path,
		maxBytes:   maxBytes,
		maxBackups: maxBackups,
		f:          f,
		size:       fi.Size(),
	}, nil
}

// Write implements io.Writer. Lines exceeding maxBytes trigger a rotate.
func (r *RotateWriter) Write(p []byte) (int, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.f == nil {
		return 0, fmt.Errorf("RotateWriter: closed")
	}
	if r.size+int64(len(p)) > r.maxBytes {
		if err := r.rotateLocked(); err != nil {
			return 0, err
		}
	}
	n, err := r.f.Write(p)
	r.size += int64(n)
	return n, err
}

// Close flushes and closes the underlying file. Safe to call once.
func (r *RotateWriter) Close() error {
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.f == nil {
		return nil
	}
	err := r.f.Close()
	r.f = nil
	return err
}

func (r *RotateWriter) rotateLocked() error {
	if err := r.f.Close(); err != nil {
		return err
	}
	r.f = nil
	for i := r.maxBackups; i >= 1; i-- {
		var src string
		if i == 1 {
			src = r.path
		} else {
			src = fmt.Sprintf("%s.%d", r.path, i-1)
		}
		dst := fmt.Sprintf("%s.%d", r.path, i)
		if _, err := os.Stat(src); err == nil {
			if err := os.Rename(src, dst); err != nil {
				return err
			}
		}
	}
	f, err := os.OpenFile(r.path, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0644)
	if err != nil {
		return err
	}
	r.f = f
	r.size = 0
	return nil
}

// Ensure compile-time conformance with io.Writer.
var _ io.Writer = (*RotateWriter)(nil)
