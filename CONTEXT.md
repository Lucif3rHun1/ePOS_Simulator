# Project Context: ePOS Printer Emulator for Odoo Online POS

## Goal
Provide a small Windows daemon that masquerades as an Epson TM-T20 (or compatible)
ESC/POS network printer, so Odoo Online POS can print to a local USB Epson
thermal receipt printer via the browser's ePOS-Print XML protocol — without
requiring an IoT Box or a printer with built-in Wi-Fi.

## Domain Glossary
- **ePOS-Print**: Epson's XML-based print protocol. POSTed to the printer's
  `/cgi-bin/epos/service.cgi?devid=...` endpoint. Browser generates the XML.
- **ePOS-Print XML elements**: `<epos-print>` root, child `<text>`/`<feedline>`/`<cut>`/
  `<image>`/`<drawer>`/`<barcode>`/`<symbol>`.
- **SOAP envelope**: optional outer wrapper (`<s:Envelope><s:Body><epos-print>`...).
  Odoo POS sends raw XML without it; some clients send SOAP-wrapped.
- **Winspool / RAW print**: Microsoft's Windows print spooler API. OpenPrinterW →
  StartDocPrinterW(datatype=RAW) → WritePrinter → EndDocPrinter → ClosePrinter.
  Send bytes directly to the driver without any driver-side rendering.
- **libusb / WinUSB**: alternative USB transport (skipped — see ADR 0001).
- **CORS preflight**: OPTIONS request a browser sends before a cross-origin POST.
  Needs `Access-Control-Allow-Origin/Methods/Headers`.
- **Private Network Access (PNA)**: Chrome's restriction on public→private
  network requests. Needs `Access-Control-Allow-Private-Network: true` in preflight.
- **self-signed cert**: TLS cert generated locally, pre-imported to Windows
  Trusted Root. Required because Odoo Online's browser cannot prompt to accept
  untrusted certs.

## Architecture
```
   Odoo POS Browser  ─POST XML─▶  ePOS_Simulator (this project)
                                       │
                                       ├─▶ translate (XML → ESC/POS bytes)
                                       ├─▶ winspool (RAW bytes → printer driver)
                                       └─▶ USB ESC/POS thermal printer
```

## Build Targets
- Primary: `GOOS=windows GOARCH=amd64` static exe (10-11 MB, no CGO).
- Dev: darwin/arm64. Windows-only files behind `//go:build windows`.

## Key Constraints
- **No console**, double-click run. So: silent startup banner on stderr, no
  interactive prompts by default; `--list-printers` for diagnostics.
- **Browser cannot accept untrusted cert**. TLS cert must be self-signed AND
  pre-imported to the user's Windows Trusted Root store (manual one-time step).
- **RAW datatype**: must declare `pDatatype: "RAW"` in `PRINTER_DEFAULTS`
  when calling `OpenPrinterW`, otherwise `StartDocPrinter` returns
  ERROR_INVALID_LEVEL (1242).
- **Drawers**: default OFF. `--drawer` opt-in to fire the cash drawer pulse.
