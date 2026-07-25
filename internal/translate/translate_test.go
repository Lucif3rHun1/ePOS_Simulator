package translate

import (
	"os"
	"strings"
	"testing"
)

func TestTranslate_TextOnly(t *testing.T) {
	data := []byte(`<epos-print><text>Hello World</text></epos-print>`)
	out, err := Translate(data, false)
	if err != nil {
		t.Fatal(err)
	}
	if len(out) == 0 {
		t.Fatal("expected non-empty output")
	}
	if !strings.Contains(string(out), "Hello World") {
		t.Fatal("output does not contain expected text")
	}
}

func TestTranslate_Feedline(t *testing.T) {
	data := []byte(`<epos-print><text>Line1</text><feedline/><text>Line2</text></epos-print>`)
	out, err := Translate(data, false)
	if err != nil {
		t.Fatal(err)
	}
	// Feed = ESC d 1 = 0x1B 0x64 0x01
	if len(out) < 5 {
		t.Fatal("output too short")
	}
}

func TestTranslate_Cut(t *testing.T) {
	data := []byte(`<epos-print><cut/></epos-print>`)
	out, err := Translate(data, false)
	if err != nil {
		t.Fatal(err)
	}
	// Cut = GS V 0 = 0x1D 0x56 0x00
	if len(out) < 3 {
		t.Fatal("output too short for cut")
	}
	if out[len(out)-3] != 0x1D || out[len(out)-2] != 0x56 {
		t.Fatal("expected GS V cut command")
	}
}

func TestTranslate_Drawer(t *testing.T) {
	data := []byte(`<epos-print><drawer/></epos-print>`)
	// Without allowDrawer, drawer should be ignored
	out, err := TranslateWithOptions(data, false, false)
	if err != nil {
		t.Fatal(err)
	}
	// Should only be Init() bytes (ESC @)
	if len(out) > 2 {
		t.Fatalf("drawer should be ignored without allowDrawer, got %d bytes", len(out))
	}

	// With allowDrawer, drawer should be present
	out2, err := TranslateWithOptions(data, false, true)
	if err != nil {
		t.Fatal(err)
	}
	if len(out2) <= 2 {
		t.Fatal("drawer should produce bytes with allowDrawer=true")
	}
}

func TestTranslate_Barcode(t *testing.T) {
	data := []byte(`<epos-print><barcode type="CODE128" data="TEST123" width="3" height="100"/></epos-print>`)
	out, err := Translate(data, false)
	if err != nil {
		t.Fatal(err)
	}
	if len(out) < 10 {
		t.Fatal("barcode output too short")
	}
}

func TestTranslate_QRCode(t *testing.T) {
	data := []byte(`<epos-print><symbol type="QRCODE" data="https://example.com" eccLevel="M"/></epos-print>`)
	out, err := Translate(data, false)
	if err != nil {
		t.Fatal(err)
	}
	if len(out) < 10 {
		t.Fatal("QR code output too short")
	}
}

func TestTranslate_GoldenOdooRequest(t *testing.T) {
	data, err := os.ReadFile("../../testdata/odoo-soap-request.xml")
	if err != nil {
		t.Fatalf("cannot read golden fixture: %v", err)
	}
	out, err := Translate(data, false)
	if err != nil {
		t.Fatalf("Translate failed on golden fixture: %v", err)
	}
	if len(out) == 0 {
		t.Fatal("golden fixture produced empty output")
	}
	// Should contain "Test Receipt" text
	if !strings.Contains(string(out), "Test Receipt") {
		t.Fatal("golden fixture output missing expected text")
	}
}

func TestTranslate_TextAlignment(t *testing.T) {
	data := []byte(`<epos-print><text align="center">Centered</text></epos-print>`)
	out, err := Translate(data, false)
	if err != nil {
		t.Fatal(err)
	}
	// Should contain ESC a 1 (center align)
	found := false
	for i := 0; i < len(out)-2; i++ {
		if out[i] == 0x1B && out[i+1] == 0x61 && out[i+2] == 1 {
			found = true
			break
		}
	}
	if !found {
		t.Fatal("expected ESC a 1 center alignment command")
	}
}
