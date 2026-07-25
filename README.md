# ePOS Printer Emulator for Odoo Online

A lightweight Windows executable that impersonates an Epson ePOS network printer over HTTP/HTTPS, allowing Odoo Online POS to print receipts to a locally connected USB receipt printer via the Windows spooler RAW path.

## Why This Exists

Odoo Online POS sends receipts directly to the printer's IP address via ePOS-Print XML (HTTP POST). It bypasses the OS print queue entirely. This emulator intercepts those HTTP requests, translates ePOS-Print XML into ESC/POS commands, and sends them to a locally installed printer driver via the Windows spooler.

## Requirements

- Windows 10/11 (amd64)
- A receipt printer installed with a Windows driver (Epson TM-T20III or compatible)
- Odoo POS configured with the printer's IP pointing to this emulator (e.g., `127.0.0.1` or `localhost`)

## Installation

### Download pre-built binary

Latest release: <https://github.com/Lucif3rHun1/ePOS_Simulator/releases/latest>

> **Browser warning:** Chrome and Edge flag unfamiliar `.exe` downloads as "not commonly downloaded" or "can harm your computer". This is a reputation check, not a malware detection. Click **Keep** / **Keep anyway** to proceed. To bypass the browser entirely, use PowerShell or `curl`:

```powershell
# Windows PowerShell — downloads without browser scan
Invoke-WebRequest -Uri "https://github.com/Lucif3rHun1/ePOS_Simulator/releases/latest/download/epos-emulator.exe" -OutFile "epos-emulator.exe"
Unblock-File .\epos-emulator.exe       # removes Zone.Identifier (alternative to "Keep anyway")
.\epos-emulator.exe --selftest
```

```bash
# macOS / Linux — same binary, useful for testing the HTTP server
curl -L -o epos-emulator.exe https://github.com/Lucif3rHun1/ePOS_Simulator/releases/latest/download/epos-emulator.exe
```

If Windows Defender quarantines the file (rare): open **Windows Security → Virus & threat protection → Protection history** and check **Allow on device**, then submit the file at <https://www.microsoft.com/en-us/wdsi/filesubmission> as a false positive.

## Building from source

Requires Go 1.21+ on any platform. Cross-compile for Windows from any host:

```bash
GOOS=windows GOARCH=amd64 go build -o epos-emulator.exe ./cmd/epos-emulator
```

Cross-compile is fully supported — the `winspool` package uses build tags (`//go:build windows`) and the matching stub (`winspool_stub.go` with `//go:build !windows`) lets you build and run on macOS/Linux for HTTP-layer testing. The HTTP server, SOAP/RAW XML translation, CORS/PNA handling, and all unit tests work cross-platform.

## Usage

```bash
# Basic (HTTP on port 8080) — interactive printer picker if --printer omitted
epos-emulator.exe --printer "Epson TM-T20III" --port 8080

# Pick a printer interactively from a numbered list (default when --printer omitted)
epos-emulator.exe --port 8080

# HTTPS with auto-generated self-signed cert
epos-emulator.exe --printer "Epson TM-T20III" --port 443 --tls

# HTTPS with your own cert
epos-emulator.exe --printer "Epson TM-T20III" --port 443 --tls --cert cert.pem --cert-key key.pem

# Allow drawer open commands
epos-emulator.exe --printer "Epson TM-T20III" --drawer

# Verbose logging (hex dumps of XML and ESC/POS bytes)
epos-emulator.exe --printer "Epson TM-T20III" --verbose --log-file emulator.log

# Self-test (prints test page, verifies spooler connectivity)
epos-emulator.exe --printer "Epson TM-T20III" --selftest

# List installed printers (Windows only)
epos-emulator.exe --list-printers

# Strict XML mode (rejects unknown elements, matches odoo/epos-proxy behavior)
epos-emulator.exe --printer "Epson TM-T20III" --strict-xml
```

### CLI Flags

| Flag | Default | Description |
|------|---------|-------------|
| `--port` | `8080` | HTTP/HTTPS listen port |
| `--printer` | *(interactive pick)* | Windows printer name (exact match from Control Panel). When empty and stdin is a TTY, prompts an interactive picker. |
| `--tls` | `false` | Enable HTTPS (auto-generates self-signed cert if `--cert` not provided) |
| `--cert` | `""` | PEM certificate file path (for custom cert) |
| `--cert-key` | `""` | PEM private key file path (for custom cert) |
| `--verbose` | `false` | Enable verbose logging (hex dumps of XML and ESC/POS bytes) |
| `--log-file` | `""` | Rotating log file path. Set to a path to persist logs. |
| `--max-log-bytes` | `10485760` | (10MB) Log file rotation threshold |
| `--max-log-backups` | `3` | Number of rotated backup files to retain (`.1`, `.2`, `.3`) |
| `--selftest` | `false` | Print a test page and exit (non-zero on spooler failure) |
| `--drawer` | `false` | Allow drawer open (`ESC p 0`) commands |
| `--paper-width` | `576` | Paper width in dots (576=80mm, 384=58mm) |
| `--codepage` | `CP437` | Default codepage for text encoding |
| `--list-printers` | `false` | Print a numbered table of installed printers and exit (Windows only) |
| `--strict-xml` | `false` | Reject unknown ePOS-Print elements with an error response (default: silently skip unknown elements) |
| `--interactive` | `true` | When `--printer` is empty and stdin is a TTY, prompt the user to pick from the enumerated printers |

## Printer Selection

The emulator needs to know which Windows printer to send jobs to. There are three ways to specify it:

1. **`--printer "Exact Name"`** — pass the printer name as it appears in Control Panel → Devices and Printers. Right-click the printer → Printer properties → General tab to copy the exact name.

2. **Interactive picker** (default) — omit `--printer` and the emulator lists all installed printers and prompts:

   ```
   Available printers:
     1. Epson TM-T20III                 [default]
     2. Microsoft Print to PDF
     3. OneNote for Windows 10
   
   Select printer [1]: _
   ```

   Accepts the number, the printer name, or just Enter to take the default. Press Ctrl+C to cancel.

3. **`--list-printers`** — print the table and exit, useful for scripting or first-time setup. Disable with `--interactive=false` in scripts/CI where no TTY is available; the emulator falls back to the system default printer.

## Odoo POS Configuration

1. Start the emulator on the same machine running Odoo (or accessible from it)
2. In Odoo POS settings, set the **Receipt Printer IP** to `127.0.0.1` (or the machine's IP)
3. Set the **Receipt Printer Port** to match `--port`
4. If using HTTPS (`--tls`), import the self-signed cert first (see below)

### Importing the Self-Signed Certificate

When `--tls` is used without `--cert`, the emulator generates a self-signed certificate saved as `cert.pem` in the working directory. To trust it:

```powershell
# PowerShell (Run as Administrator)
Import-Certificate -FilePath .\cert.pem -CertStoreLocation Cert:\LocalMachine\Root
```

Or double-click `cert.pem` → Install Certificate → Local Machine → Trusted Root Certification Authorities.

**Important**: Browsers and `fetch()`/`XHR` have no certificate prompt for programmatic requests. The cert MUST be pre-trusted or Odoo POS will fail with a network error.

## How It Works

1. Odoo POS sends an HTTP POST with ePOS-Print XML to the emulator
2. The emulator auto-detects the body format (raw `<epos-print>...</epos-print>` or full SOAP envelope) and extracts the epos-print subtree
3. ESC/POS commands are extracted: text (alignment/emphasis/underline), images (banded raster), cut, feed, drawer, barcode, QR code
4. Commands are sent to the configured printer via the Windows spooler (RAW datatype, declared in `PRINTER_DEFAULTS` at open time)
5. A success response is returned in the same format the request used (bare XML if request was raw, SOAP envelope if request was SOAP)

### Supported ePOS-Print Elements

| Element | ESC/POS Command | Notes |
|---------|-----------------|-------|
| `<text>` | Raw text bytes | Supports `align` (left/center/right), `em`, `ul`, `dw`, `dh` attrs. Flat chardata or nested `<line><content>` |
| `<feedline>` | `ESC d 1` | Feed one line |
| `<feed line="N">` | `ESC d N` | Feed N lines (odoo/epos-proxy style) |
| `<cut/>` | `GS V 0` (or `GS V A LF`) | Full cut |
| `<image>` | `GS v 0` (raster, banded, m=0x00, mode=0x30) | Base64 data, width/height attributes; max 255 rows per band |
| `<barcode>` | `GS k` | UPC-A/UPC-E/EAN13/EAN8/Code39/ITF/Codabar/Code93/Code128 |
| `<symbol>` | `GS ( k` | QR code with configurable ECC level (L/M/Q/H) |
| `<drawer>` | `ESC p 0 t1 t2` | Single pin pulse (only when `--drawer` flag is set) |
| `<pulse>` | `ESC = 0x01` + `ESC p 0 25 25` + `ESC p 1 25 25` | odoo/epos-proxy drawer: pulses BOTH pin 2 and pin 5 |

Unknown elements are silently skipped by default. Pass `--strict-xml` to make the emulator reject them with an error response — this matches odoo/epos-proxy behavior and helps catch unsupported features early.

## CORS & Private Network Access

The emulator responds to browser preflight requests with:

- `Access-Control-Allow-Origin: *`
- `Access-Control-Allow-Methods: POST, OPTIONS`
- `Access-Control-Allow-Headers: Content-Type, SOAPAction`
- `Access-Control-Allow-Private-Network: true` (when the browser sends `Access-Control-Request-Private-Network: true`)

These headers are unconditional — no origin validation. This is by design: the emulator runs on localhost and is not exposed to the internet.

## Compatibility with odoo/epos-proxy

This emulator is wire-compatible with Odoo's official [odoo/epos-proxy](https://github.com/odoo/epos-proxy) reference implementation. Both accept the same body format (raw `<epos-print>` XML or SOAP envelope), return success/error responses in the same shape, and translate the same element set to ESC/POS. The main differences:

| Aspect | ePOS_Simulator (this repo) | odoo/epos-proxy |
|--------|----------------------------|-----------------|
| USB transport | Windows spooler RAW (uses installed driver) | Direct USB via libusb/winusb |
| Routing | Single printer, `--printer` selects target | Multi-device via `/p/{device_id}/` URL path |
| Drawer | `<drawer>` (single pin, gated by `--drawer`) and `<pulse>` (both pins, always on) | `<pulse/>` (both pins, always on) |
| TLS | `--tls` flag, auto-generates self-signed cert | Webview UI for cert management |
| Multi-printer | One printer per process | Multiple printers per process with auto-discovery |
| Frontend | None (CLI-only) | System tray webview (Wails) |

If you only need to print from Odoo POS to a single local printer and you don't want to install a Tauri/Wails app, this emulator is the lighter-weight option.

## Troubleshooting

### "No such printer" error
The `--printer` name must exactly match the printer name in Windows Control Panel → Devices and Printers. Right-click the printer → Properties → General tab to copy the exact name. Run `epos-emulator.exe --list-printers` to see all available names.

### Odoo POS shows network error
1. Check the emulator is running and listening on the expected port (look for the `http server listening` log line)
2. Verify the printer IP in Odoo POS matches `127.0.0.1:<port>`
3. If using HTTPS, ensure the self-signed cert is imported to Windows Trusted Root store
4. Check Windows Firewall isn't blocking the port
5. Enable `--verbose` to see the raw request and response

### `StartDocPrinter failed: ERROR_INVALID_LEVEL (1242)`
The printer handle was not opened with the RAW datatype pre-declared. This emulator opens printers with `PRINTER_DEFAULTS{pDatatype: "RAW", DesiredAccess: PRINTER_ACCESS_USE}`, which is the supported Windows pattern. If you still see this error, the installed driver does not accept RAW data — install the Epson APD or use a driver that supports `RAW` datatype. Run `--list-printers` to see the driver name.

### Blank/missing receipts
1. Enable `--verbose` to see hex dumps of incoming XML and outgoing ESC/POS
2. Check the printer has paper and is online
3. Verify the printer driver supports RAW mode (most Epson drivers do)
4. Run `--selftest` to verify the spooler chain works without Odoo in the picture

### Logs
Use `--log-file emulator.log` for persistent rotating logs. Default rotation threshold is 10MB with 3 backup files (`emulator.log.1`, `.2`, `.3`). Adjust with `--max-log-bytes` and `--max-log-backups`. Verbose mode (`--verbose`) adds hex dumps of raw XML payloads and ESC/POS byte streams to every request.

## Architecture

See [CONTEXT.md](CONTEXT.md) for the domain glossary and [docs/adr/](docs/adr/) for architecture decisions. The accepted ADR ([0001](docs/adr/0001-spooler-raw-over-libusb.md)) explains why this emulator uses the Windows spooler RAW path instead of direct USB via libusb/winusb like odoo/epos-proxy.
