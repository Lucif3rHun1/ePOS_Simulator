package eposhttp

import (
	"crypto/rand"
	"encoding/hex"
	"net/http"
	"time"

	"epos-emulator/internal/logging"
)

// responseRecorder wraps http.ResponseWriter to capture status code and bytes written.
type responseRecorder struct {
	http.ResponseWriter
	status int
	bytes  int
}

func (r *responseRecorder) WriteHeader(code int) {
	r.status = code
	r.ResponseWriter.WriteHeader(code)
}

func (r *responseRecorder) Write(b []byte) (int, error) {
	if r.status == 0 {
		r.status = http.StatusOK
	}
	n, err := r.ResponseWriter.Write(b)
	r.bytes += n
	return n, err
}

// requestID generates a short unique ID for correlating logs.
func requestID() string {
	var b [8]byte
	rand.Read(b[:])
	return hex.EncodeToString(b[:])
}

// LoggingMiddleware wraps h with per-request logging: timestamp, method, path,
// remote, status, duration, body size, request id. Every request emits one log
// entry at INFO; verbose mode also logs request body for POST to /cgi-bin/epos.
func LoggingMiddleware(h http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		start := time.Now()
		rid := requestID()

		// Expose request ID in response headers for client-side correlation.
		w.Header().Set("X-Request-ID", rid)

		rec := &responseRecorder{ResponseWriter: w}
		h.ServeHTTP(rec, r)

		dur := time.Since(start)
		logging.Info("http request",
			"id", rid,
			"method", r.Method,
			"path", r.URL.Path,
			"query", r.URL.RawQuery,
			"remote", r.RemoteAddr,
			"status", rec.status,
			"bytes", rec.bytes,
			"dur_ms", dur.Milliseconds(),
			"ua", r.UserAgent(),
		)
	})
}

// LoggingMiddlewareVerbose wraps LoggingMiddleware to also log full request/response
// bodies for the ePOS POST endpoint. Use only for debugging; large bodies.
func LoggingMiddlewareVerbose(h http.Handler) http.Handler {
	return LoggingMiddleware(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method == http.MethodPost && r.URL.Path == "/cgi-bin/epos/service.cgi" {
			logging.Debug("post body received",
				"path", r.URL.Path,
				"content_type", r.Header.Get("Content-Type"),
				"content_length", r.ContentLength,
			)
		}
		h.ServeHTTP(w, r)
	}))
}
