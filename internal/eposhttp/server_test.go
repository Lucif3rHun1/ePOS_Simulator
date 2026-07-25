package eposhttp

import (
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestHandler_HealthEndpoint(t *testing.T) {
	handler := Handler("", false, false)
	req := httptest.NewRequest(http.MethodGet, "/", nil)
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, req)
	if w.Code != http.StatusOK {
		t.Fatalf("expected 200, got %d", w.Code)
	}
	if !strings.Contains(w.Body.String(), "ok") {
		t.Fatal("expected status ok")
	}
}

func TestHandler_OptionsCorsHeaders(t *testing.T) {
	handler := Handler("", false, false)
	req := httptest.NewRequest(http.MethodOptions, "/cgi-bin/epos/service.cgi", nil)
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, req)
	if w.Code != http.StatusNoContent {
		t.Fatalf("expected 204, got %d", w.Code)
	}
	if w.Header().Get("Access-Control-Allow-Origin") != "*" {
		t.Fatal("missing Allow-Origin")
	}
	if w.Header().Get("Access-Control-Allow-Methods") != "POST, OPTIONS" {
		t.Fatal("missing Allow-Methods")
	}
	if w.Header().Get("Access-Control-Allow-Headers") != "Content-Type, SOAPAction" {
		t.Fatal("missing Allow-Headers")
	}
}

func TestHandler_PnaHeaders(t *testing.T) {
	handler := Handler("", false, false)
	req := httptest.NewRequest(http.MethodOptions, "/cgi-bin/epos/service.cgi", nil)
	req.Header.Set("Access-Control-Request-Private-Network", "true")
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, req)
	if w.Header().Get("Access-Control-Allow-Private-Network") != "true" {
		t.Fatal("missing PNA header")
	}
}

func TestHandler_PnaHeaders_NotSetWhenNotRequested(t *testing.T) {
	handler := Handler("", false, false)
	req := httptest.NewRequest(http.MethodOptions, "/cgi-bin/epos/service.cgi", nil)
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, req)
	if w.Header().Get("Access-Control-Allow-Private-Network") != "" {
		t.Fatal("PNA header should not be set when not requested")
	}
}

func TestHandler_PostEndpoint(t *testing.T) {
	handler := Handler("", false, false)
	body := `<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
  <s:Body>
    <epos-print xmlns="http://www.epson-pos.com/schemas/2011/03/epos-print">
      <text>Hello</text>
    </epos-print>
  </s:Body>
</s:Envelope>`
	req := httptest.NewRequest(http.MethodPost, "/cgi-bin/epos/service.cgi", strings.NewReader(body))
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, req)
	// Winspool stub fails on macOS, but we still get a response
	if w.Code != http.StatusOK && w.Code != http.StatusInternalServerError {
		t.Fatalf("expected 200 or 500, got %d", w.Code)
	}
}

func TestHandler_NotFound(t *testing.T) {
	handler := Handler("", false, false)
	req := httptest.NewRequest(http.MethodGet, "/nonexistent", nil)
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, req)
	if w.Code != http.StatusNotFound {
		t.Fatalf("expected 404, got %d", w.Code)
	}
}

func TestHandler_MethodNotAllowed(t *testing.T) {
	handler := Handler("", false, false)
	req := httptest.NewRequest(http.MethodGet, "/cgi-bin/epos/service.cgi", nil)
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, req)
	if w.Code != http.StatusMethodNotAllowed {
		t.Fatalf("expected 405, got %d", w.Code)
	}
}
