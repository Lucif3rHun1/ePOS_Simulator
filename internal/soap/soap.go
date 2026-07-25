package soap

import (
	"encoding/xml"
	"fmt"
	"strings"
)

// Format indicates whether the request body uses SOAP envelope wrapping or raw XML.
type Format int

const (
	FormatUnknown Format = iota
	FormatSOAP
	FormatRaw
)

// DetectFormat inspects a request body and reports whether it looks like a
// full SOAP envelope (FormatSOAP) or a bare <epos-print> document (FormatRaw).
// Odoo POS sends raw XML, while some Epson SDK clients send SOAP envelopes.
func DetectFormat(data []byte) Format {
	s := string(data)
	if strings.Contains(s, "<s:Envelope") || strings.Contains(s, "<SOAP-ENV:Envelope") ||
		strings.Contains(s, ":Envelope ") || strings.Contains(s, "<Envelope ") {
		return FormatSOAP
	}
	if strings.Contains(s, "<epos-print") {
		return FormatRaw
	}
	return FormatUnknown
}

// SOAP envelope structure
type Envelope struct {
	XMLName xml.Name `xml:"Envelope"`
	Body    Body     `xml:"Body"`
}

type Body struct {
	EposPrint EposPrint `xml:"epos-print"`
}

type EposPrint struct {
	XMLName xml.Name `xml:"epos-print"`
	Text    string   `xml:",chardata"`
	// Elements will be processed as raw XML
}

// SuccessResponse is the full SOAP envelope golden response used when the
// client sent a SOAP envelope. Matches AC4 byte-for-byte.
const SuccessResponse = `<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
  <s:Body>
    <response success="true" code="" status="252" battery="0" xmlns="http://www.epson-pos.com/schemas/2011/03/epos-print"/>
  </s:Body>
</s:Envelope>`

// ErrorResponse is the full SOAP envelope error template.
const ErrorResponse = `<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
  <s:Body>
    <response success="false" code="EPSON_ERR_FAILURE" status="0" battery="0" xmlns="http://www.epson-pos.com/schemas/2011/03/epos-print"/>
  </s:Body>
</s:Envelope>`

// RawSuccessResponse is the bare XML response used when the client sent raw
// <epos-print>... (what real Odoo POS sends). Matches the format used by the
// official odoo/epos-proxy server.
const RawSuccessResponse = `<?xml version="1.0" encoding="utf-8"?>
<response success="true" code="" status="" xmlns="http://www.epson-pos.com/schemas/2011/03/epos-print"/>`

// RawErrorResponse builds a bare XML error response.
func RawErrorResponse(code string) string {
	return fmt.Sprintf(`<?xml version="1.0" encoding="utf-8"?>
<response success="false" code="%s" status="0" xmlns="http://www.epson-pos.com/schemas/2011/03/epos-print"/>`, code)
}

// Parse extracts the epos-print body content from a SOAP envelope or raw XML.
// Returns the inner XML between <epos-print ...> and </epos-print>.
func Parse(data []byte) (string, error) {
	content := extractEposPrint(data)
	if content == "" {
		return "", fmt.Errorf("no epos-print content found")
	}
	return content, nil
}

func extractEposPrint(data []byte) string {
	start := strings.Index(string(data), "<epos-print")
	if start == -1 {
		return ""
	}

	tagEnd := strings.Index(string(data)[start:], ">")
	if tagEnd == -1 {
		return ""
	}

	contentStart := start + tagEnd + 1
	content := string(data)[contentStart:]

	endIdx := strings.Index(content, "</epos-print>")
	if endIdx == -1 {
		endIdx = strings.Index(content, "/>")
		if endIdx == -1 {
			return content
		}
		return content[:endIdx]
	}

	return content[:endIdx]
}

// SuccessResponseBytes returns the SOAP success response as bytes.
func SuccessResponseBytes() []byte {
	return []byte(SuccessResponse)
}

// RawSuccessResponseBytes returns the raw XML success response as bytes.
func RawSuccessResponseBytes() []byte {
	return []byte(RawSuccessResponse)
}

// ErrorResponseBytes returns the SOAP error response as bytes.
func ErrorResponseBytes(code string) []byte {
	return []byte(fmt.Sprintf(`<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
  <s:Body>
    <response success="false" code="%s" status="0" battery="0" xmlns="http://www.epson-pos.com/schemas/2011/03/epos-print"/>
  </s:Body>
</s:Envelope>`, code))
}

// RawErrorResponseBytes returns the raw XML error response as bytes.
func RawErrorResponseBytes(code string) []byte {
	return []byte(RawErrorResponse(code))
}
