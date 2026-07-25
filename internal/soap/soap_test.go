package soap

import (
	"strings"
	"testing"
)

func TestParse(t *testing.T) {
	// Simulate Odoo SOAP envelope
	soap := `<?xml version="1.0" encoding="UTF-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
  <s:Body>
    <epos-print xmlns="http://www.epson-pos.com/schemas/2011/03/epos-print">
      <text>Hello World</text>
      <feedline/>
      <cut/>
    </epos-print>
  </s:Body>
</s:Envelope>`

	body, err := Parse([]byte(soap))
	if err != nil {
		t.Fatalf("Parse failed: %v", err)
	}
	if !strings.Contains(body, "Hello World") {
		t.Errorf("expected body to contain 'Hello World', got: %s", body)
	}
	if !strings.Contains(body, "<feedline") {
		t.Errorf("expected body to contain '<feedline', got: %s", body)
	}
	if !strings.Contains(body, "<cut") {
		t.Errorf("expected body to contain '<cut', got: %s", body)
	}
}

func TestParse_SelfClosing(t *testing.T) {
	soap := `<?xml version="1.0" encoding="UTF-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
  <s:Body>
    <epos-print xmlns="http://www.epson-pos.com/schemas/2011/03/epos-print">
      <text>Test</text><cut/>
    </epos-print>
  </s:Body>
</s:Envelope>`

	body, err := Parse([]byte(soap))
	if err != nil {
		t.Fatalf("Parse failed: %v", err)
	}
	if !strings.Contains(body, "<cut") {
		t.Errorf("expected body to contain self-closing '<cut', got: %s", body)
	}
}

func TestSuccessResponse(t *testing.T) {
	resp := SuccessResponse
	if !strings.Contains(resp, `success="true"`) {
		t.Error("SuccessResponse missing success=\"true\"")
	}
	if !strings.Contains(resp, "status=\"252\"") {
		t.Error("SuccessResponse missing status=\"252\"")
	}
	if !strings.Contains(resp, "battery=\"0\"") {
		t.Error("SuccessResponse missing battery=\"0\"")
	}
	if !strings.Contains(resp, "s:Envelope") {
		t.Error("SuccessResponse missing SOAP envelope")
	}
	if !strings.Contains(resp, "xmlns=\"http://www.epson-pos.com/schemas/2011/03/epos-print\"") {
		t.Error("SuccessResponse missing epos-print namespace")
	}
}

func TestErrorResponse(t *testing.T) {
	resp := ErrorResponse
	if !strings.Contains(resp, `success="false"`) {
		t.Error("ErrorResponse missing success=\"false\"")
	}
	if !strings.Contains(resp, "s:Envelope") {
		t.Error("ErrorResponse missing SOAP envelope")
	}
}

func TestSuccessResponseBytes(t *testing.T) {
	b := SuccessResponseBytes()
	if len(b) == 0 {
		t.Fatal("SuccessResponseBytes returned empty")
	}
	if !strings.Contains(string(b), `success="true"`) {
		t.Error("SuccessResponseBytes missing success=true")
	}
}

func TestErrorResponseBytes(t *testing.T) {
	b := ErrorResponseBytes("TEST_ERR")
	if !strings.Contains(string(b), "TEST_ERR") {
		t.Error("ErrorResponseBytes missing custom code")
	}
	if !strings.Contains(string(b), `success="false"`) {
		t.Error("ErrorResponseBytes missing success=false")
	}
}

func TestParse_InvalidXML(t *testing.T) {
	_, err := Parse([]byte("not xml at all"))
	if err == nil {
		t.Error("expected error for invalid XML")
	}
}

func TestParse_MissingEposPrint(t *testing.T) {
	soap := `<?xml version="1.0" encoding="UTF-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
  <s:Body>
    <other-element>test</other-element>
  </s:Body>
</s:Envelope>`
	_, err := Parse([]byte(soap))
	if err == nil {
		t.Error("expected error for missing epos-print element")
	}
}
