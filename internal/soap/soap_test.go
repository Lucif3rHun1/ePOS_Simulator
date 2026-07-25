package soap

import (
	"strings"
	"testing"
)

func TestParse(t *testing.T) {
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

func TestParse_RawXML(t *testing.T) {
	raw := `<?xml version="1.0" encoding="UTF-8"?>
<epos-print xmlns="http://www.epson-pos.com/schemas/2011/03/epos-print">
  <text>Raw Hello</text>
  <feedline/>
  <cut/>
</epos-print>`

	body, err := Parse([]byte(raw))
	if err != nil {
		t.Fatalf("Parse failed on raw XML: %v", err)
	}
	if !strings.Contains(body, "Raw Hello") {
		t.Errorf("expected body to contain 'Raw Hello', got: %s", body)
	}
}

func TestSuccessResponse(t *testing.T) {
	resp := SuccessResponse
	if !strings.Contains(resp, `success="true"`) {
		t.Error("SuccessResponse missing success=\"true\"")
	}
	if !strings.Contains(resp, `status="252"`) {
		t.Error("SuccessResponse missing status=\"252\"")
	}
	if !strings.Contains(resp, `battery="0"`) {
		t.Error("SuccessResponse missing battery=\"0\"")
	}
	if !strings.Contains(resp, "s:Envelope") {
		t.Error("SuccessResponse missing SOAP envelope")
	}
	if !strings.Contains(resp, `xmlns="http://www.epson-pos.com/schemas/2011/03/epos-print"`) {
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

func TestDetectFormat_SOAP(t *testing.T) {
	body := []byte(`<?xml version="1.0"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
  <s:Body><epos-print><text>x</text></epos-print></s:Body>
</s:Envelope>`)
	if got := DetectFormat(body); got != FormatSOAP {
		t.Errorf("expected FormatSOAP, got %v", got)
	}
}

func TestDetectFormat_Raw(t *testing.T) {
	body := []byte(`<?xml version="1.0"?>
<epos-print xmlns="http://www.epson-pos.com/schemas/2011/03/epos-print">
  <text>Hi</text>
</epos-print>`)
	if got := DetectFormat(body); got != FormatRaw {
		t.Errorf("expected FormatRaw, got %v", got)
	}
}

func TestDetectFormat_Unknown(t *testing.T) {
	body := []byte(`plain text no xml here`)
	if got := DetectFormat(body); got != FormatUnknown {
		t.Errorf("expected FormatUnknown, got %v", got)
	}
}

func TestRawSuccessResponse(t *testing.T) {
	resp := RawSuccessResponse
	if !strings.Contains(resp, `success="true"`) {
		t.Error("RawSuccessResponse missing success=\"true\"")
	}
	if strings.Contains(resp, "s:Envelope") {
		t.Error("RawSuccessResponse must not include SOAP envelope")
	}
	if !strings.Contains(resp, `xmlns="http://www.epson-pos.com/schemas/2011/03/epos-print"`) {
		t.Error("RawSuccessResponse missing epos-print namespace")
	}
}

func TestRawSuccessResponseBytes(t *testing.T) {
	b := RawSuccessResponseBytes()
	if len(b) == 0 {
		t.Fatal("RawSuccessResponseBytes returned empty")
	}
	if strings.Contains(string(b), "s:Envelope") {
		t.Error("RawSuccessResponseBytes must be bare XML, no SOAP envelope")
	}
}

func TestRawErrorResponseBytes(t *testing.T) {
	b := RawErrorResponseBytes("SchemaError")
	s := string(b)
	if !strings.Contains(s, "SchemaError") {
		t.Error("RawErrorResponseBytes missing custom code")
	}
	if !strings.Contains(s, `success="false"`) {
		t.Error("RawErrorResponseBytes missing success=\"false\"")
	}
	if strings.Contains(s, "s:Envelope") {
		t.Error("RawErrorResponseBytes must be bare XML")
	}
}
