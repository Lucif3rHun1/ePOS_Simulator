# ePOS Printer Emulator for Odoo Online

A lightweight Windows executable that impersonates an Epson ePOS network printer over HTTP/HTTPS, allowing Odoo Online POS to print receipts to a locally connected USB receipt printer via the Windows spooler RAW path.

## Why This Exists

Odoo Online POS sends receipts directly to the printer's IP address via ePOS-Print XML (HTTP POST). It bypasses the OS print queue entirely. This emulator intercepts those HTTP requests, translates ePOS-Print XML into ESC/POS commands, and sends them to a locally installed printer driver via the Windows spooler.

## Requirements

- Windows 10/11 (amd64)
- A receipt printer installed with a Windows driver (Epson TM-T20III or compatible)
- Odoo POS configured with the printer's IP pointing to this emulator (e.g., `127.0.0.1` or `localhost`)

## Building

Requires Go 1.21+ on any platform. Cross-compile for Windows:

```bash
GOOS=windows GOARCH=amd64 go build -o epos-emulator.exe ./cmd/epos-emulator
```

## Usage

```bash
# Basic (HTTP on port 8080)
epos-emulator.exe --printer "Epson TM-T20III" --port 8080

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
```

### CLI Flags

| Flag | Default | Description |
|------|---------|-------------|
| `--port` | `8080` | HTTP/HTTPS listen port |
| `--printer` | *(required)* | Windows printer name (exact match from Control Panel) |
| `--tls` | `false` | Enable HTTPS (auto-generates self-signed cert if `--cert` not provided) |
| `--cert` | `""` | PEM certificate file path (for custom cert) |
| `--cert-key` | `""` | PEM private key file path (for custom cert) |
| `--verbose` | `false` | Enable verbose logging (hex dumps) |
| `--log-file` | `""` | Rotating log file path (max 10MB, auto-rotates to `.1`) |
| `--selftest` | `false` | Print a test page and exit |
| `--drawer` | `false` | Allow drawer open (ESC p 0) commands |
| `--paper-width` | `576` | Paper width in dots (576=80mm, 384=58mm) |
| `--codepage` | `CP437` | Default codepage for text encoding |

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
2. The emulator parses the XML (SOAP envelope → epos-print body)
3. ESC/POS commands are extracted: text, images (raster), cut, feed, drawer, barcode, QR code
4. Commands are sent to the configured printer via the Windows spooler (RAW mode)
5. A success SOAP response is returned to the browser

### Supported ePOS-Print Elements

| Element | ESC/POS Command | Notes |
|---------|-----------------|-------|
| `<text>` | Raw text bytes | Supports alignment, emphasis, underline, double-height |
| `<feedline>` | `ESC d 1` | Feed one line |
| `<cut>` | `GS V 0` | Full cut |
| `<image>` | `GS v 0` (raster, banded) | Base64 data, width/height attributes |
| `<barcode>` | `GS k` | UPC-A/UPC-E/EAN13/EAN8/Code39/ITF/Codabar/Code93/Code128 |
| `<symbol>` | `GS ( k` | QR code with configurable ECC level |
| `<drawer>` | `ESC p 0` | Only when `--drawer` flag is set |

## CORS & Private Network Access

The emulator responds to browser preflight requests with:

- `Access-Control-Allow-Origin: *`
- `Access-Control-Allow-Methods: POST, OPTIONS`
- `Access-Control-Allow-Headers: Content-Type, SOAPAction`
- `Access-Control-Allow-Private-Network: true` (when requested by the browser)

These headers are unconditional — no origin validation. This is by design: the emulator runs on localhost and is not exposed to the internet.

## Troubleshooting

### "No such printer" error
The `--printer` name must exactly match the printer name in Windows Control Panel → Devices and Printers. Right-click the printer → Properties → General tab to copy the exact name.

### Odoo POS shows network error
1. Check the emulator is running and listening on the expected port
2. Verify the printer IP in Odoo POS matches `127.0.0.1:<port>`
3. If using HTTPS, ensure the self-signed cert is imported to Windows Trusted Root store
4. Check Windows Firewall isn't blocking the port

### Blank/missing receipts
1. Enable `--verbose` to see hex dumps of incoming XML and outgoing ESC/POS
2. Check the printer has paper and is online
3. Verify the printer driver supports RAW mode (most Epson drivers do)

### Logs
Use `--log-file emulator.log` for persistent rotating logs. The log rotates at 10MB with a `.1` backup file.
