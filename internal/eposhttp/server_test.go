package eposhttp

import (
	"bytes"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestHandler_HealthEndpoint(t *testing.T) {
	h := Handler("", false, false)
	req := httptest.NewRequest(http.MethodGet, "/", nil)
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	if w.Code != http.StatusOK {
		t.Errorf("expected 200, got %d", w.Code)
	}
	if !strings.Contains(w.Body.String(), `"status":"ok"`) {
		t.Errorf("expected status:ok JSON, got: %s", w.Body.String())
	}
}

func TestHandler_OptionsCorsHeaders(t *testing.T) {
	h := Handler("", false, false)
	req := httptest.NewRequest(http.MethodOptions, "/cgi-bin/epos/service.cgi", nil)
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	if w.Code != http.StatusNoContent {
		t.Errorf("expected 204, got %d", w.Code)
	}
	if got := w.Header().Get("Access-Control-Allow-Origin"); got != "*" {
		t.Errorf("expected Allow-Origin=*, got %q", got)
	}
	if got := w.Header().Get("Access-Control-Allow-Methods"); got != "POST, OPTIONS" {
		t.Errorf("expected Allow-Methods=POST, OPTIONS, got %q", got)
	}
	if got := w.Header().Get("Access-Control-Allow-Headers"); got != "Content-Type, SOAPAction" {
		t.Errorf("expected Allow-Headers=Content-Type, SOAPAction, got %q", got)
	}
}

func TestHandler_PnaHeaders(t *testing.T) {
	h := Handler("", false, false)
	req := httptest.NewRequest(http.MethodOptions, "/cgi-bin/epos/service.cgi", nil)
	req.Header.Set("Access-Control-Request-Private-Network", "true")
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	if got := w.Header().Get("Access-Control-Allow-Private-Network"); got != "true" {
		t.Errorf("expected PNA header=true, got %q", got)
	}
}

func TestHandler_PnaHeaders_NotSetWhenNotRequested(t *testing.T) {
	h := Handler("", false, false)
	req := httptest.NewRequest(http.MethodOptions, "/cgi-bin/epos/service.cgi", nil)
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	if got := w.Header().Get("Access-Control-Allow-Private-Network"); got != "" {
		t.Errorf("expected PNA header empty, got %q", got)
	}
}

func TestHandler_PostEndpoint_SOAPEnvelope(t *testing.T) {
	h := Handler("", false, false)
	body := `<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
  <s:Body>
    <epos-print xmlns="http://www.epson-pos.com/schemas/2011/03/epos-print">
      <text>Hi from SOAP</text>
    </epos-print>
  </s:Body>
</s:Envelope>`
	req := httptest.NewRequest(http.MethodPost, "/cgi-bin/epos/service.cgi", bytes.NewBufferString(body))
	req.Header.Set("Content-Type", "text/xml")
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	// Winspool fails on macOS, expect error response in SOAP envelope
	if !strings.Contains(w.Body.String(), "s:Envelope") {
		t.Errorf("SOAP request should respond with SOAP envelope, got: %s", w.Body.String())
	}
	if !strings.Contains(w.Body.String(), `success="false"`) {
		t.Errorf("expected failure response, got: %s", w.Body.String())
	}
}

func TestHandler_PostEndpoint_RawXML(t *testing.T) {
	h := Handler("", false, false)
	body := `<?xml version="1.0" encoding="utf-8"?>
<epos-print xmlns="http://www.epson-pos.com/schemas/2011/03/epos-print">
  <text>Hi from raw</text>
</epos-print>`
	req := httptest.NewRequest(http.MethodPost, "/cgi-bin/epos/service.cgi", bytes.NewBufferString(body))
	req.Header.Set("Content-Type", "text/xml")
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	// Winspool fails on macOS, expect error response as RAW XML
	if strings.Contains(w.Body.String(), "s:Envelope") {
		t.Errorf("raw request should NOT respond with SOAP envelope, got: %s", w.Body.String())
	}
	if !strings.Contains(w.Body.String(), `<response`) {
		t.Errorf("expected bare response element, got: %s", w.Body.String())
	}
	if !strings.Contains(w.Body.String(), `success="false"`) {
		t.Errorf("expected failure response, got: %s", w.Body.String())
	}
}

func TestHandler_NotFound(t *testing.T) {
	h := Handler("", false, false)
	req := httptest.NewRequest(http.MethodGet, "/nonexistent", nil)
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	if w.Code != http.StatusNotFound {
		t.Errorf("expected 404, got %d", w.Code)
	}
}

func TestHandler_MethodNotAllowed(t *testing.T) {
	h := Handler("", false, false)
	req := httptest.NewRequest(http.MethodPut, "/cgi-bin/epos/service.cgi", nil)
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	if w.Code != http.StatusMethodNotAllowed {
		t.Errorf("expected 405, got %d", w.Code)
	}
}

func TestHandler_InvalidRequestBody(t *testing.T) {
	h := Handler("", false, false)
	req := httptest.NewRequest(http.MethodPost, "/cgi-bin/epos/service.cgi", bytes.NewBufferString("not xml"))
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	// Should send error response
	body, _ := io.ReadAll(w.Body)
	if !strings.Contains(string(body), "success") {
		t.Errorf("expected error response, got: %s", string(body))
	}
}
