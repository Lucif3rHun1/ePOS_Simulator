package eposhttp

import (
	"encoding/json"
	"fmt"
	"io"
	"net/http"

	"epos-emulator/internal/logging"
	"epos-emulator/internal/translate"
	"epos-emulator/internal/winspool"
)

// Handler returns an http.Handler implementing the ePOS HTTP server.
func Handler(printerName string, verbose bool, allowDrawer bool) http.Handler {
	h := &eposHandler{
		printerName: printerName,
		verbose:     verbose,
		allowDrawer: allowDrawer,
	}
	// Wrap with logging middleware. Verbose flag also enables body hex dumps.
	if verbose {
		return LoggingMiddlewareVerbose(h)
	}
	return LoggingMiddleware(h)
}

type eposHandler struct {
	printerName string
	verbose     bool
	allowDrawer bool
}

func (h *eposHandler) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	if r.URL.Path == "/" && r.Method == http.MethodGet {
		h.handleHealth(w, r)
		return
	}

	if r.URL.Path == "/cgi-bin/epos/service.cgi" {
		switch r.Method {
		case http.MethodOptions:
			h.handlePreflight(w, r)
		case http.MethodPost:
			h.handleEpos(w, r)
		default:
			http.Error(w, "Method not allowed", http.StatusMethodNotAllowed)
		}
		return
	}

	http.NotFound(w, r)
}

func (h *eposHandler) handleHealth(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(map[string]string{"status": "ok"})
}

func (h *eposHandler) handlePreflight(w http.ResponseWriter, r *http.Request) {
	// AC2: CORS preflight headers
	w.Header().Set("Access-Control-Allow-Origin", "*")
	w.Header().Set("Access-Control-Allow-Methods", "POST, OPTIONS")
	w.Header().Set("Access-Control-Allow-Headers", "Content-Type, SOAPAction")

	// AC3: Private Network Access
	if r.Header.Get("Access-Control-Request-Private-Network") == "true" {
		w.Header().Set("Access-Control-Allow-Private-Network", "true")
	}

	w.WriteHeader(http.StatusNoContent)
}

func (h *eposHandler) handleEpos(w http.ResponseWriter, r *http.Request) {
	// CORS response headers (AC2)
	w.Header().Set("Access-Control-Allow-Origin", "*")

	body, err := io.ReadAll(r.Body)
	if err != nil {
		http.Error(w, "Failed to read body", http.StatusBadRequest)
		return
	}
	defer r.Body.Close()

	logging.LogXML("XML RX", body)

	// Parse SOAP and translate to ESC/POS
	escposBytes, err := translate.TranslateWithOptions(body, h.verbose, h.allowDrawer)
	if err != nil {
		logging.Error("translate failed", "err", err.Error(), "body_size", len(body))
		h.sendSoapError(w, err.Error())
		return
	}

	// Send to printer via spooler
	if len(escposBytes) > 0 {
		logging.LogESCPOS("ESCPOS TX", escposBytes)

		hh, err := winspool.OpenPrinter(h.printerName)
		if err != nil {
			logging.Error("open printer failed", "printer", h.printerName, "err", err.Error())
			h.sendSoapError(w, fmt.Sprintf("Printer open error: %v", err))
			return
		}
		defer winspool.ClosePrinter(hh)

		if err := winspool.PrintRaw(hh, "ePOS Emulator", escposBytes); err != nil {
			logging.Error("print raw failed", "printer", h.printerName, "bytes", len(escposBytes), "err", err.Error())
			h.sendSoapError(w, fmt.Sprintf("Printer error: %v", err))
			return
		}
		logging.Info("printed", "printer", h.printerName, "bytes", len(escposBytes))
	}

	// AC4: Send success response
	h.sendSoapSuccess(w)
}

func (h *eposHandler) sendSoapSuccess(w http.ResponseWriter) {
	w.Header().Set("Content-Type", "text/xml; charset=utf-8")
	w.Write([]byte(`<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
  <s:Body>
    <response success="true" code="" status="252" battery="0" xmlns="http://www.epson-pos.com/schemas/2011/03/epos-print"/>
  </s:Body>
</s:Envelope>`))
}

func (h *eposHandler) sendSoapError(w http.ResponseWriter, msg string) {
	w.Header().Set("Content-Type", "text/xml; charset=utf-8")
	w.Write([]byte(fmt.Sprintf(`<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
  <s:Body>
    <response success="false" code="EPSON_ERR_FAILURE" status="0" battery="0" xmlns="http://www.epson-pos.com/schemas/2011/03/epos-print"/>
  </s:Body>
</s:Envelope>`)))
}
