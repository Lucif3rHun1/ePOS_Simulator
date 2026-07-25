package escpos

// ESC/POS command bytes
var (
	ESC  = byte(0x1B)
	GS   = byte(0x1D)
	CRLF = []byte{0x0D, 0x0A}
)

// Init sends ESC @ (initialize printer).
func Init() []byte { return []byte{ESC, '@'} }

// Cut sends GS V m (cut paper). m=0 full cut, m=1 partial.
func Cut(m byte) []byte { return []byte{GS, 'V', m} }

// Feed sends ESC d n (feed n lines).
func Feed(n byte) []byte { return []byte{ESC, 'd', n} }

// Text returns raw text bytes.
func Text(s string) []byte { return []byte(s) }

// Align sends ESC a n (n=0 left, 1 center, 2 right).
func Align(n byte) []byte { return []byte{ESC, 'a', n} }

// Emphasis sends ESC E n (n=0 off, 1 on).
func Emphasis(on bool) []byte {
	if on {
		return []byte{ESC, 'E', 1}
	}
	return []byte{ESC, 'E', 0}
}

// Underline sends ESC - n (n=0 off, 1 thin, 2 thick).
func Underline(n byte) []byte { return []byte{ESC, '-', n} }

// DoubleHeight sends GS ! n with bit 4 set for double height.
func DoubleHeight(on bool) []byte {
	if on {
		return []byte{GS, '!', 0x10}
	}
	return []byte{GS, '!', 0x00}
}

// RasterBanded encodes a 1-bit monochrome bitmap as GS v 0 banded.
func RasterBanded(img []byte, width, height, paperWidth int) []byte {
	if len(img) == 0 || width <= 0 || height <= 0 {
		return nil
	}

	bitsPerRow := paperWidth
	bytesPerRow := (bitsPerRow + 7) / 8

	var out []byte
	for y := 0; y < height; y++ {
		band := make([]byte, bytesPerRow)
		srcOff := y * ((width + 7) / 8)
		for x := 0; x < width && x < bitsPerRow; x++ {
			byteIdx := srcOff + x/8
			bitIdx := 7 - uint(x%8)
			if byteIdx < len(img) && (img[byteIdx]>>(bitIdx))&1 == 1 {
				band[x/8] |= 1 << bitIdx
			}
		}
		cmd := []byte{GS, 'v', byte('0'), byte(bytesPerRow % 256), byte(bytesPerRow / 256), 1, 0}
		cmd = append(cmd, band...)
		out = append(out, cmd...)
	}
	return out
}

// Drawer sends ESC p 0 t1 t2 (pulse drawer port 0).
func Drawer(t1, t2 byte) []byte { return []byte{ESC, 'p', 0, t1, t2} }

// Barcode sends GS k barcode.
// funcCode: 0-6 for GS k, barcodeType: barcode type byte
// data: barcode data bytes, width: module width (2-6), height: barcode height (0-255)
func Barcode(data []byte, barcodeType byte, width byte, height byte) []byte {
	var out []byte
	// Set barcode height: GS h n
	out = append(out, GS, 'h', height)
	// Set barcode width: GS w n
	out = append(out, GS, 'w', width)
	// HRI text position: GS H n (n=0 none, 1 above, 2 below, 3 both)
	out = append(out, GS, 'H', 1)
	// Print barcode: GS k m d1..dk NUL
	out = append(out, GS, 'k', barcodeType)
	out = append(out, data...)
	out = append(out, 0) // NUL terminator
	return out
}

// QRCode sends GS ( k for QR code.
// eccLevel: 1=L, 2=M, 3=Q, 4=H; moduleSize: 1-16
func QRCode(data []byte, eccLevel byte, moduleSize byte) []byte {
	var out []byte
	// Set module size: GS ( k 16 0 p1 p2
	out = append(out, GS, '(', 'k', 4, 0, 16, moduleSize)
	// Set ECC level: GS ( k 17 0 n
	out = append(out, GS, '(', 'k', 3, 0, 49, eccLevel)
	// Store data: GS ( k pL pH fn data
	dataLen := len(data)
	out = append(out, GS, '(', 'k', byte(dataLen%256), byte(dataLen/256), 49)
	out = append(out, data...)
	// Print: GS ( k 0
	out = append(out, GS, '(', 'k', 2, 0, 48)
	return out
}

// SelfTestBytes returns a minimal ESC/POS sequence for selftest.
func SelfTestBytes() []byte {
	var out []byte
	out = append(out, Init()...)
	out = append(out, Text("ePOS Emulator Self-Test\n")...)
	out = append(out, Feed(2)...)
	out = append(out, Cut(0)...)
	return out
}
