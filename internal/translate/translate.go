package translate

import (
	"encoding/base64"
	"encoding/xml"
	"fmt"
	"io"
	"strconv"
	"strings"

	"epos-emulator/internal/escpos"
)

// Translate parses ePOS-Print XML and produces ESC/POS byte stream.
func Translate(data []byte, verbose bool) ([]byte, error) {
	return TranslateWithOptions(data, verbose, false)
}

// TranslateWithOptions parses with drawer gating.
func TranslateWithOptions(data []byte, verbose, allowDrawer bool) ([]byte, error) {
	eposBody := extractEposPrintBody(data)
	if eposBody == "" {
		return nil, fmt.Errorf("no epos-print content found in SOAP envelope")
	}

	var out []byte
	out = append(out, escpos.Init()...)

	decoder := xml.NewDecoder(strings.NewReader("<root>" + eposBody + "</root>"))
	for {
		token, err := decoder.Token()
		if err == io.EOF {
			break
		}
		if err != nil {
			return nil, fmt.Errorf("XML parse error: %w", err)
		}

		switch t := token.(type) {
		case xml.StartElement:
			switch t.Name.Local {
			case "text":
				handleText(decoder, t, &out)

			case "feedline":
				out = append(out, escpos.Feed(1)...)

			case "cut":
				out = append(out, escpos.Cut(0)...)

			case "image":
				handleImage(t, &out)

			case "drawer":
				if allowDrawer {
					out = append(out, escpos.Drawer(100, 250)...)
				}

			case "barcode":
				handleBarcode(t, decoder, &out)

			case "symbol":
				handleSymbol(t, decoder, &out)
			}
		}
	}

	return out, nil
}

func handleText(decoder *xml.Decoder, t xml.StartElement, out *[]byte) {
	// Extract align attribute if present
	var align byte = 0
	for _, attr := range t.Attr {
		if attr.Name.Local == "align" {
			switch strings.ToLower(attr.Value) {
			case "center":
				align = 1
			case "right":
				align = 2
			}
		}
	}
	if align != 0 {
		*out = append(*out, escpos.Align(align)...)
	}

	var textElem struct {
		Chardata string       `xml:",chardata"`
		Line     []struct {
			Content string `xml:"content"`
		} `xml:"line"`
	}
	if err := decoder.DecodeElement(&textElem, &t); err == nil {
		if len(textElem.Line) > 0 {
			for _, l := range textElem.Line {
				if s := strings.TrimSpace(l.Content); s != "" {
					*out = append(*out, escpos.Text(s)...)
				}
			}
		} else if s := strings.TrimSpace(textElem.Chardata); s != "" {
			*out = append(*out, escpos.Text(s)...)
		}
	}

	// Reset alignment
	if align != 0 {
		*out = append(*out, escpos.Align(0)...)
	}
}

func handleImage(t xml.StartElement, out *[]byte) {
	var dataVal string
	var paperWidth, width, height int
	for _, attr := range t.Attr {
		switch attr.Name.Local {
		case "data":
			dataVal = attr.Value
		case "width":
			paperWidth, _ = strconv.Atoi(attr.Value)
		case "height":
			height, _ = strconv.Atoi(attr.Value)
		case "x":
			width, _ = strconv.Atoi(attr.Value)
		}
	}
	if dataVal == "" {
		return
	}
	if paperWidth == 0 {
		paperWidth = 576
	}
	if width == 0 {
		width = paperWidth
	}
	if height == 0 {
		height = 1
	}
	if imgBytes, err := base64.StdEncoding.DecodeString(dataVal); err == nil {
		*out = append(*out, escpos.RasterBanded(imgBytes, width, height, paperWidth)...)
	}
}

func handleBarcode(t xml.StartElement, decoder *xml.Decoder, out *[]byte) {
	var barcodeType byte = 0 // GS k type 0 = UPC-A
	var width byte = 3
	var height byte = 100
	var data string
	for _, attr := range t.Attr {
		switch attr.Name.Local {
		case "type":
			barcodeType = parseBarcodeType(attr.Value)
		case "width":
			w, _ := strconv.Atoi(attr.Value)
			if w >= 2 && w <= 6 {
				width = byte(w)
			}
		case "height":
			h, _ := strconv.Atoi(attr.Value)
			if h > 0 && h <= 255 {
				height = byte(h)
			}
		case "data":
			data = attr.Value
		}
	}
	// Also check chardata if no data attr
	if data == "" {
		var elem struct {
			Chardata string `xml:",chardata"`
		}
		decoder.DecodeElement(&elem, &t)
		data = strings.TrimSpace(elem.Chardata)
	}
	if data != "" {
		*out = append(*out, escpos.Barcode([]byte(data), barcodeType, width, height)...)
	}
}

func parseBarcodeType(s string) byte {
	switch strings.ToUpper(s) {
	case "UPCA":
		return 0
	case "UPCE":
		return 1
	case "EAN13":
		return 2
	case "EAN8":
		return 3
	case "CODE39":
		return 4
	case "ITF":
		return 5
	case "CODABAR":
		return 6
	case "CODE93":
		return 7
	case "CODE128":
		return 8
	default:
		return 8 // CODE128 as safe default
	}
}

func handleSymbol(t xml.StartElement, decoder *xml.Decoder, out *[]byte) {
	var eccLevel byte = 3 // Q
	var data string
	for _, attr := range t.Attr {
		switch attr.Name.Local {
		case "data":
			data = attr.Value
		case "eccLevel":
			eccLevel = byte(parseEccLevel(attr.Value))
		}
	}
	if data == "" {
		var elem struct {
			Chardata string `xml:",chardata"`
		}
		decoder.DecodeElement(&elem, &t)
		data = strings.TrimSpace(elem.Chardata)
	}
	if data != "" {
		*out = append(*out, escpos.QRCode([]byte(data), eccLevel, 4)...)
	}
}

func parseEccLevel(s string) int {
	switch strings.ToUpper(s) {
	case "L":
		return 1
	case "M":
		return 2
	case "Q":
		return 3
	case "H":
		return 4
	default:
		return 3
	}
}

func extractEposPrintBody(data []byte) string {
	s := string(data)
	start := strings.Index(s, "<epos-print")
	if start == -1 {
		return ""
	}
	tagEnd := strings.Index(s[start:], ">")
	if tagEnd == -1 {
		return ""
	}
	contentStart := start + tagEnd + 1
	content := s[contentStart:]
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
