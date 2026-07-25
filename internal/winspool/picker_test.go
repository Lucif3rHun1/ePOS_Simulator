package winspool

import (
	"bytes"
	"strings"
	"testing"
)

func TestFilterEmpty_KeepsNamed(t *testing.T) {
	in := []PrinterInfo{
		{Name: "Epson TM-T20III"},
		{Name: "Microsoft Print to PDF"},
	}
	out := filterEmpty(in)
	if len(out) != 2 {
		t.Fatalf("got %d, want 2", len(out))
	}
	if out[0].Name != "Epson TM-T20III" || out[1].Name != "Microsoft Print to PDF" {
		t.Errorf("names wrong: %+v", out)
	}
}

func TestFilterEmpty_RemovesBlank(t *testing.T) {
	in := []PrinterInfo{
		{Name: "Real Printer", PortName: "USB001"},
		{Name: "", PortName: "USB002"}, // printer driver returned a bad pointer
		{Name: "Another Real", PortName: "USB003"},
	}
	out := filterEmpty(in)
	if len(out) != 2 {
		t.Fatalf("got %d, want 2 (blank row should be dropped)", len(out))
	}
	if out[0].Name != "Real Printer" || out[1].Name != "Another Real" {
		t.Errorf("names wrong: %+v", out)
	}
}

func TestFilterEmpty_AllBlank(t *testing.T) {
	in := []PrinterInfo{{Name: ""}, {Name: ""}, {Name: ""}}
	out := filterEmpty(in)
	if len(out) != 0 {
		t.Errorf("got %d, want 0", len(out))
	}
}

func TestFilterEmpty_EmptyInput(t *testing.T) {
	if out := filterEmpty(nil); len(out) != 0 {
		t.Errorf("nil input: got %d, want 0", len(out))
	}
	if out := filterEmpty([]PrinterInfo{}); len(out) != 0 {
		t.Errorf("empty input: got %d, want 0", len(out))
	}
}

func TestPickFromInfos_PickNumber(t *testing.T) {
	infos := []PrinterInfo{
		{Name: "Epson TM-T20III", PortName: "USB001"},
		{Name: "Microsoft Print to PDF", PortName: "PORTPROMPT:"},
	}
	in := strings.NewReader("2\n")
	out := &bytes.Buffer{}
	name, err := pickFromInfos(infos, in, out, "")
	if err != nil {
		t.Fatalf("Pick: %v", err)
	}
	if name != "Microsoft Print to PDF" {
		t.Errorf("got %q, want Microsoft Print to PDF", name)
	}
	if !strings.Contains(out.String(), "Available printers:") {
		t.Errorf("output should mention Available printers; got %q", out.String())
	}
}

func TestPickFromInfos_EmptyInputUsesDefault(t *testing.T) {
	infos := []PrinterInfo{
		{Name: "Default Printer", IsDefault: true},
		{Name: "Other Printer"},
	}
	in := strings.NewReader("\n")
	name, err := pickFromInfos(infos, in, nil, "")
	if err != nil {
		t.Fatalf("Pick: %v", err)
	}
	if name != "Default Printer" {
		t.Errorf("got %q, want Default Printer (default should be picked on empty input)", name)
	}
}

func TestPickFromInfos_EmptyInputNoDefaultFallsBack(t *testing.T) {
	infos := []PrinterInfo{
		{Name: "Printer A"},
		{Name: "Printer B"},
	}
	in := strings.NewReader("\n")
	name, err := pickFromInfos(infos, in, nil, "fallback-name")
	if err != nil {
		t.Fatalf("Pick: %v", err)
	}
	if name != "fallback-name" {
		t.Errorf("got %q, want fallback-name", name)
	}
}

func TestPickFromInfos_PickByName(t *testing.T) {
	infos := []PrinterInfo{
		{Name: "Epson TM-T20III"},
		{Name: "Microsoft Print to PDF"},
	}
	in := strings.NewReader("Microsoft Print to PDF\n")
	name, err := pickFromInfos(infos, in, nil, "")
	if err != nil {
		t.Fatalf("Pick: %v", err)
	}
	if name != "Microsoft Print to PDF" {
		t.Errorf("got %q, want Microsoft Print to PDF", name)
	}
}

func TestPickFromInfos_PickByNameCaseInsensitive(t *testing.T) {
	infos := []PrinterInfo{
		{Name: "Epson TM-T20III"},
	}
	in := strings.NewReader("epson tm-t20iii\n")
	name, err := pickFromInfos(infos, in, nil, "")
	if err != nil {
		t.Fatalf("Pick: %v", err)
	}
	if name != "Epson TM-T20III" {
		t.Errorf("got %q, want Epson TM-T20III (case-insensitive match)", name)
	}
}

func TestPickFromInfos_InvalidNumberFallsThroughToFreeform(t *testing.T) {
	infos := []PrinterInfo{{Name: "Real Printer"}}
	in := strings.NewReader("99\n")
	name, err := pickFromInfos(infos, in, nil, "")
	if err != nil {
		t.Fatalf("Pick: %v", err)
	}
	// Out-of-range number falls through to the free-form branch which
	// returns the literal line — the user might have a printer the
	// enumeration didn't pick up.
	if name != "99" {
		t.Errorf("got %q, want 99 (out-of-range number should be treated as free-form name)", name)
	}
}

func TestPickFromInfos_EmptyList(t *testing.T) {
	name, err := pickFromInfos(nil, strings.NewReader("ignored"), nil, "fb")
	if err != nil {
		t.Fatalf("Pick: %v", err)
	}
	if name != "fb" {
		t.Errorf("got %q, want fb (fallback when list empty)", name)
	}
}

func TestPickFromInfos_NoInputFallsBackOrCancelled(t *testing.T) {
	// Empty reader + no fallback -> ErrUserCancelled
	_, err := pickFromInfos([]PrinterInfo{{Name: "x"}}, strings.NewReader(""), nil, "")
	if err != ErrUserCancelled {
		t.Errorf("got err %v, want ErrUserCancelled", err)
	}
	// Empty reader + fallback -> fallback
	name, err := pickFromInfos([]PrinterInfo{{Name: "x"}}, strings.NewReader(""), nil, "fb")
	if err != nil {
		t.Fatalf("Pick: %v", err)
	}
	if name != "fb" {
		t.Errorf("got %q, want fb", name)
	}
}
