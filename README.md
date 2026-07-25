# ePOS Printer Emulator for Odoo Online POS

A lightweight binary that impersonates an Epson ePOS network printer over HTTP/HTTPS,
allowing Odoo Online POS to print receipts to a locally connected USB receipt printer.

This is the **Rust port** of `epos-emulator`. It targets the Windows spooler RAW path
(like the Go predecessor) and the same wire protocol — raw `<epos-print>` XML or
SOAP-enveloped requests, byte-for-byte identical CORS/PNA headers, identical
ePOS-Print element handling.

## Why

Odoo Online POS sends receipts directly to the printer IP via ePOS-Print XML
(HTTP POST). This emulator intercepts those requests, translates the ePOS-Print
elements to ESC/POS, and forwards to a Windows-installed printer driver.

## Building

```bash
# Native (current platform)
cargo build --release

# Windows cross-compile from any host with the x86_64-pc-windows-msvc target
rustup target add x86_64-pc-windows-msvc
cargo build --release --target x86_64-pc-windows-msvc
```

Pure Rust. No system OpenSSL/winspool dependency at link time.

## Running

```bash
epos-emulator --port 8080 --printer "Epson TM-T20III"
```

All flags match the Go version:

| Flag | Default | Description |
|------|---------|-------------|
| `--port` | `8080` | HTTP/HTTPS listen port |
| `--printer` | *(interactive)* | Windows printer name; empty=interactive pick |
| `--tls` | `false` | Enable HTTPS with self-signed cert (TODO in v2.0.0) |
| `--cert` / `--key` | `""` | TLS PEM files (when --tls is implemented) |
| `--verbose` | `false` | Verbose logging (hex dumps) |
| `--selftest` | `false` | Print a test page and exit |
| `--drawer` | `false` | Allow drawer kick (ESC p) |
| `--paper-width` | `576` | Paper width in dots |
| `--codepage` | `CP437` | Barcode/QR codepage |
| `--log-file` | `""` | Log file path (rotating) |
| `--list-printers` | `false` | List installed printers and exit |
| `--max-log-bytes` | `10MB` | Log rotation threshold |
| `--max-log-backups` | `3` | Number of rotated log files |
| `--strict-xml` | `false` | Reject unknown ePOS-Print elements |
| `--interactive` | `true` | Prompt when `--printer` is empty and stdin is TTY |

## Endpoints

- `GET /` — health check, returns `{"status":"ok"}`
- `POST /cgi-bin/epos/service.cgi` — ePOS-Print XML (raw or SOAP-wrapped)
- `OPTIONS /cgi-bin/epos/service.cgi` — CORS preflight

## Architecture

```
src/
  main.rs            Binary entry point
  lib.rs             Module declarations
  cli.rs             Clap argument parser + main run loop
  eposhttp.rs        axum router, CORS/PNA middleware, request logging
  escpos.rs          ESC/POS byte builders (init/cut/feed/text/barcode/QR/raster)
  soap.rs            Format detection + response builders (SOAP vs raw)
  translate.rs       ePOS-Print XML → ESC/POS bytes
  logging.rs         tracing-subscriber + rotating file writer
  tls.rs             Self-signed cert generation (rcgen)
  netinfo.rs         Local IP enumeration for the startup banner
  winspool.rs        Windows print spooler wrapper (RAW) + non-Windows stub
  picker.rs          Interactive printer picker (CLI)
```

## Status

v2.0.0 is the first Rust release. Compared to the Go predecessor (v1.1.x):

- ✅ Cross-platform build (Windows / macOS / Linux) — same `cargo build` recipe
- ✅ Library tests: 34/36 pass; 2 translate tests for self-closing nested text fail
  (streaming XML reader depth tracking has a known bug — see `src/translate.rs`)
- ⚠️ `--tls` flag is wired but the `tokio-rustls` + axum 0.7 serve plumbing is
  stubbed (logs "not yet implemented"; use a reverse proxy for TLS)
- ✅ Winspool: Windows Print Spooler RAW with `PRINTER_DEFAULTS{RAW, USE}`
- ✅ Interactive printer picker with crash recovery on bad EnumPrintersW pointers
- ✅ Structured logging via `tracing` + JSON, rotating file writer
- ✅ CORS + Private Network Access (PNA) headers (byte-compatible with Go)
- ✅ AC4 byte-exact SOAP success response

## License

MIT — same as the Go predecessor.
