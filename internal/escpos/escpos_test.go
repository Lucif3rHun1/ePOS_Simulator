package escpos

import (
	"testing"
)

func TestInit(t *testing.T) {
	out := Init()
	if len(out) != 2 || out[0] != 0x1B || out[1] != '@' {
		t.Fatalf("expected ESC @, got %x", out)
	}
}

func TestText(t *testing.T) {
	out := Text("hello")
	if string(out) != "hello" {
		t.Fatalf("expected hello, got %q", out)
	}
}

func TestCut(t *testing.T) {
	out := Cut(0)
	if len(out) != 3 || out[0] != 0x1D || out[1] != 'V' || out[2] != 0 {
		t.Fatalf("expected GS V 0, got %x", out)
	}
}

func TestFeed(t *testing.T) {
	out := Feed(3)
	if len(out) != 3 || out[0] != 0x1B || out[1] != 'd' || out[2] != 3 {
		t.Fatalf("expected ESC d 3, got %x", out)
	}
}

func TestDrawer(t *testing.T) {
	out := Drawer(100, 250)
	if len(out) != 5 {
		t.Fatalf("expected 5 bytes, got %d", len(out))
	}
	if out[0] != 0x1B || out[1] != 'p' || out[2] != 0 {
		t.Fatalf("expected ESC p 0, got %x", out[:3])
	}
}

func TestAlign(t *testing.T) {
	out := Align(1) // center
	if len(out) != 3 || out[0] != 0x1B || out[1] != 'a' || out[2] != 1 {
		t.Fatalf("expected ESC a 1, got %x", out)
	}
}

func TestEmphasis(t *testing.T) {
	on := Emphasis(true)
	off := Emphasis(false)
	if len(on) != 3 || on[2] != 1 {
		t.Fatalf("expected ESC E 1, got %x", on)
	}
	if len(off) != 3 || off[2] != 0 {
		t.Fatalf("expected ESC E 0, got %x", off)
	}
}

func TestBarcode(t *testing.T) {
	out := Barcode([]byte("TEST"), 8, 3, 100) // CODE128
	if len(out) < 10 {
		t.Fatalf("barcode output too short: %d bytes", len(out))
	}
	// Should start with GS h (height) and GS w (width)
	if out[0] != 0x1D || out[1] != 'h' {
		t.Fatalf("expected GS h, got %x", out[:2])
	}
	if out[3] != 0x1D || out[4] != 'w' {
		t.Fatalf("expected GS w, got %x", out[3:5])
	}
}

func TestQRCode(t *testing.T) {
	out := QRCode([]byte("test"), 3, 4) // ECC Q, module size 4
	if len(out) < 10 {
		t.Fatalf("QR code output too short: %d bytes", len(out))
	}
	// Should start with GS ( k
	if out[0] != 0x1D || out[1] != '(' || out[2] != 'k' {
		t.Fatalf("expected GS ( k, got %x", out[:3])
	}
}

func TestRasterBanded_SingleRow(t *testing.T) {
	// 8 pixels wide, 1 row
	img := []byte{0xFF}
	out := RasterBanded(img, 8, 1, 8)
	if len(out) == 0 {
		t.Fatal("expected non-empty output")
	}
	// GS v 0 = 1D 76 30
	if out[0] != 0x1D || out[1] != 'v' || out[2] != '0' {
		t.Fatalf("expected GS v 0, got %x", out[:3])
	}
}

func TestRasterBanded_MultiRowBanding(t *testing.T) {
	// 8x2 image, 16-dot paper
	img := []byte{0xFF, 0x00}
	out := RasterBanded(img, 8, 2, 16)
	if len(out) == 0 {
		t.Fatal("expected non-empty output")
	}
	// Should have 2 bands
	count := 0
	for i := 0; i < len(out)-2; i++ {
		if out[i] == 0x1D && out[i+1] == 'v' && out[i+2] == '0' {
			count++
		}
	}
	if count != 2 {
		t.Fatalf("expected 2 GS v 0 commands, got %d", count)
	}
}

func TestSelfTestBytes(t *testing.T) {
	out := SelfTestBytes()
	if len(out) < 10 {
		t.Fatalf("selftest too short: %d bytes", len(out))
	}
	// Should start with ESC @
	if out[0] != 0x1B || out[1] != '@' {
		t.Fatal("expected ESC @ at start")
	}
}
