package eposhttp

import (
	"encoding/json"
	"fmt"
	"io"
	"net/http"

	"epos-emulator/internal/logging"
	"epos-emulator/internal/soap"
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
			h.applyCORS(w, r)
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
	h.applyCORS(w, r)
	w.WriteHeader(http.StatusNoContent)
}

func (h *eposHandler) applyCORS(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Access-Control-Allow-Origin", "*")
	w.Header().Set("Access-Control-Allow-Methods", "POST, OPTIONS")
	w.Header().Set("Access-Control-Allow-Headers", "Content-Type, SOAPAction")
	if r.Header.Get("Access-Control-Request-Private-Network") == "true" {
		w.Header().Set("Access-Control-Allow-Private-Network", "true")
	}
}

func (h *eposHandler) handleEpos(w http.ResponseWriter, r *http.Request) {
	h.applyCORS(w, r)

	body, err := io.ReadAll(r.Body)
	if err != nil {
		http.Error(w, "Failed to read body", http.StatusBadRequest)
		return
	}
	defer r.Body.Close()

	logXML(r.Method, body)
	format := soap.DetectFormat(body)

	escposBytes, err := translate.TranslateWithOptions(body, h.verbose, h.allowDrawer)
	if err != nil {
		logging.Error("translate failed", "err", err.Error(), "body_size", len(body))
		h.sendError(w, format, err.Error())
		return
	}

	if len(escposBytes) > 0 {
		logging.LogESCPOS("ESCPOS TX", escposBytes)

		hh, err := winspool.OpenPrinter(h.printerName)
		if err != nil {
			logging.Error("open printer failed", "printer", h.printerName, "err", err.Error())
			h.sendError(w, format, fmt.Sprintf("Printer open error: %v", err))
			return
		}
		defer winspool.ClosePrinter(hh)

		if err := winspool.PrintRaw(hh, "ePOS Emulator", escposBytes); err != nil {
			logging.Error("print raw failed", "printer", h.printerName, "bytes", len(escposBytes), "err", err.Error())
			h.sendError(w, format, fmt.Sprintf("Printer error: %v", err))
			return
		}
		logging.Info("printed", "printer", h.printerName, "bytes", len(escposBytes), "format", formatName(format))
	}

	h.sendSuccess(w, format)
}

func (h *eposHandler) sendSuccess(w http.ResponseWriter, format soap.Format) {
	w.Header().Set("Content-Type", "text/xml; charset=utf-8")
	if format == soap.FormatRaw {
		w.Write(soap.RawSuccessResponseBytes())
		return
	}
	w.Write(soap.SuccessResponseBytes())
}

func (h *eposHandler) sendError(w http.ResponseWriter, format soap.Format, code string) {
	w.Header().Set("Content-Type", "text/xml; charset=utf-8")
	if format == soap.FormatRaw {
		w.Write(soap.RawErrorResponseBytes(code))
		return
	}
	w.Write(soap.ErrorResponseBytes(code))
}

func logXML(method string, body []byte) {
	logging.LogXML("XML RX", body)
	_ = method
}

func formatName(f soap.Format) string {
	switch f {
	case soap.FormatSOAP:
		return "soap"
	case soap.FormatRaw:
		return "raw"
	default:
		return "unknown"
	}
}
