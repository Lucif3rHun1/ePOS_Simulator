package soap

import (
	"encoding/xml"
	"fmt"
	"strings"
)

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

// Response builder - AC4 golden response
const SuccessResponse = `<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
  <s:Body>
    <response success="true" code="" status="252" battery="0" xmlns="http://www.epson-pos.com/schemas/2011/03/epos-print"/>
  </s:Body>
</s:Envelope>`

const ErrorResponse = `<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
  <s:Body>
    <response success="false" code="EPSON_ERR_FAILURE" status="0" battery="0" xmlns="http://www.epson-pos.com/schemas/2011/03/epos-print"/>
  </s:Body>
</s:Envelope>`

// Parse parses SOAP envelope and extracts ePOS-print body
func Parse(data []byte) (string, error) {
	// Try to find epos-print content directly
	content := extractEposPrint(data)
	if content == "" {
		return "", fmt.Errorf("no epos-print content found")
	}
	return content, nil
}

func extractEposPrint(data []byte) string {
	// Find <epos-print> or <epos-print ...> tag
	start := strings.Index(string(data), "<epos-print")
	if start == -1 {
		return ""
	}
	
	// Find closing > of opening tag
	tagEnd := strings.Index(string(data)[start:], ">")
	if tagEnd == -1 {
		return ""
	}
	
	contentStart := start + tagEnd + 1
	
	// Find closing </epos-print> or self-closing />
	content := string(data)[contentStart:]
	
	endIdx := strings.Index(content, "</epos-print>")
	if endIdx == -1 {
		// Try self-closing
		endIdx = strings.Index(content, "/>")
		if endIdx == -1 {
			return content
		}
		return content[:endIdx]
	}
	
	return content[:endIdx]
}

// SuccessResponse returns the golden success response
func SuccessResponseBytes() []byte {
	return []byte(SuccessResponse)
}

// ErrorResponseBytes returns an error response
func ErrorResponseBytes(code string) []byte {
	return []byte(fmt.Sprintf(`<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
  <s:Body>
    <response success="false" code="%s" status="0" battery="0" xmlns="http://www.epson-pos.com/schemas/2011/03/epos-print"/>
  </s:Body>
</s:Envelope>`, code))
}
