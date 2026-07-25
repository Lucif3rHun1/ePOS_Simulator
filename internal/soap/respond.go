package soap

import (
	"net/http"
)

// Kind enumerates success/error response shapes.
type Kind int

const (
	KindSuccess Kind = iota
	KindError
)

// Write writes a response in the format matching the request shape.
// On FormatRaw it returns a bare <response/> element; on FormatSOAP it wraps
// in the full s:Envelope/s:Body envelope. Unknown format falls back to SOAP.
func Write(w http.ResponseWriter, format Format, kind Kind, code string) {
	w.Header().Set("Content-Type", "text/xml; charset=utf-8")

	var body []byte
	if format == FormatRaw {
		if kind == KindSuccess {
			body = RawSuccessResponseBytes()
		} else {
			body = RawErrorResponseBytes(code)
		}
	} else {
		if kind == KindSuccess {
			body = SuccessResponseBytes()
		} else {
			body = ErrorResponseBytes(code)
		}
	}
	_, _ = w.Write(body)
}
