# ePOS Printer Emulator for Odoo Online POS

A single binary that impersonates an Epson ePOS network printer over HTTP,
translates Odoo POS receipts from ePOS-Print XML into raw ESC/POS, and
forwards them to a locally-installed Windows printer via the spooler RAW path.

Runs on **Windows** in production, on **macOS / Linux** for dev. Same binary
(`epos-emulator`) for both — the Windows code path is gated on `cfg(windows)`.

---

## TL;DR — start the server in 10 seconds

```bash
# From a release you downloaded:
epos-emulator.exe                                    # interactive picker, port 8080
epos-emulator.exe --printer "Epson TM-T20III"        # hardcoded, skip picker
epos-emulator.exe --printer "Epson TM-T20III" --log-file "C:\\ProgramData\\epos\\epos.log"
```

The first run will:
1. Enumerate every installed Windows printer.
2. Prompt you to pick one (arrow keys / number / Enter).
3. Bind `0.0.0.0:8080` and print a banner listing every local IPv4 — paste
   one into Odoo POS → Settings → Printer → **Printer IP Address**.
4. Forward `<epos-print>` XML → ESC/POS → spooler RAW → printer.

Stop with `Ctrl-C`. SIGTERM works on Unix.

---

## Table of contents

- [Quick start](#quick-start)
- [CLI flags](#cli-flags)
- [Endpoints](#endpoints)
- [Odoo POS configuration](#odoo-pos-configuration)
- [TLS via reverse proxy](#tls-via-reverse-proxy)
- [Building from source](#building-from-source)
- [Windows build (release binary)](#windows-build-release-binary)
- [Running as a Windows service](#running-as-a-windows-service)
- [Testing](#testing)
- [Architecture](#architecture)
- [What's new in v2.x](#whats-new-in-v2x)

---

## Quick start

### Dev (macOS / Linux)

```bash
# Clone + build
git clone https://github.com/Lucif3rHun1/ePOS_Simulator.git
cd ePOS_Simulator
cargo build --release

# Run (on Linux/macOS the winspool module is a no-op stub — useful for
# verifying the HTTP/XML/translate path even without a real printer)
./target/release/epos-emulator --port 8080

# Dry-run a test print against the running server (no printer needed)
curl -X POST http://127.0.0.1:8080/cgi-bin/epos/service.cgi?devid=local_printer \
     -H 'Content-Type: text/xml; charset=utf-8' \
     --data-binary @testdata/odoo-pos-test-print.xml
```

### Production (Windows)

1. Download `epos-emulator.exe` from the [latest release](../../releases).
2. Place it somewhere stable (e.g. `C:\\Program Files\\ePOS\\`).
3. Open **PowerShell as Administrator** (only needed for service install).
4. Run it once interactively to confirm it finds your printer:

   ```powershell
   .\\epos-emulator.exe --list-printers
   .\\epos-emulator.exe --printer "Epson TM-T20III"
   ```

5. Leave it running, or install it as a service (see below).

---

## CLI flags

```
epos-emulator [OPTIONS]

Options:
      --port <PORT>              HTTP listen port [default: 8080]
      --printer <PRINTER>        Printer name; empty=interactive pick [default: ""]
      --tls                      Enable HTTPS (TODO; use reverse proxy for now)
      --cert <CERT>              TLS certificate (PEM, when --tls is implemented)
      --key <KEY>                TLS private key (PEM, when --tls is implemented)
  -v, --verbose                  Verbose logging (hex dumps of XML + ESC/POS)
      --selftest                 Send a self-test page to the printer and exit
      --drawer                   Allow drawer kick (ESC p)
      --paper-width <DOTS>       Paper width in dots [384=58mm, 512/576=80mm] [default: 576]
      --codepage <CODEPAGE>      Barcode/QR codepage [default: CP437]
      --log-file <PATH>          Log file path (rotating, default 10 MB x 3 backups)
      --list-printers            List installed printers and exit
      --max-log-bytes <BYTES>    Log rotation size threshold [default: 10485760]
      --max-log-backups <N>      Number of rotated log files to keep [default: 3]
      --strict-xml               Reject unknown ePOS-Print elements (default: ignore)
      --interactive              Prompt when --printer is empty and stdin is a TTY [default: true]
      --help                     Print help
      --version                  Print version
```

### Common recipes

```bash
# Verbose log to file, hardcoded printer, drawer kick enabled, 58mm paper
epos-emulator --printer "Epson TM-T20III" --drawer --paper-width 384 \
              --log-file /var/log/epos.log --verbose

# Production: persist logs, no picker
epos-emulator.exe --printer "EPSON TM-T20III" \
                  --log-file "C:\\ProgramData\\ePOS\\epos.log"

# Strict XML mode (reject unknown ePOS-Print elements)
epos-emulator --strict-xml

# List printers and exit (useful for picking the right name)
epos-emulator --list-printers

# Send a self-test receipt to the configured printer and exit
epos-emulator --printer "Epson TM-T20III" --selftest
```

---

## Endpoints

| Method | Path                            | Purpose                                        |
| ------ | ------------------------------- | ---------------------------------------------- |
| GET    | `/`                              | Health check — `{"status":"ok"}`                   |
| OPTIONS | `/`                              | CORS preflight (PNA-aware)                     |
| POST   | `/cgi-bin/epos/service.cgi?devid=<id>` | Receive ePOS-Print XML (raw or SOAP-wrapped)   |
| OPTIONS | `/cgi-bin/epos/service.cgi`     | CORS preflight (PNA-aware)                      |

All responses include:
- `Access-Control-Allow-Origin: *`
- `Access-Control-Allow-Private-Network: true` *(when the preflight asked)*
- `Access-Control-Max-Age: 86400`
- `X-Request-ID: <hex>` for correlating with logs
- `X-Idempotency-Replay: true` *(only on dedup-replayed responses)*

### What a request looks like

```http
POST /cgi-bin/epos/service.cgi?devid=local_printer HTTP/1.1
Host: <printer-ip>:8080
Content-Type: text/xml; charset=utf-8
SOAPAction: ""

<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
    <s:Body>
        <epos-print xmlns="http://www.epson-pos.com/schemas/2011/03/epos-print">
            <feed line="1" />
            <text align="center">Test print for printer Printer&#10;</text>
            <feed line="3" />
            <cut type="feed" />
        </epos-print>
    </s:Body>
</s:Envelope>
```

The `?devid=` query parameter is the Odoo-side device ID and is logged but
otherwise ignored.

---

## Odoo POS configuration

In Odoo (Online or On-Premise) → **Point of Sale → Configuration → Printers**:

| Field                  | Value                                                  |
| ---------------------- | ------------------------------------------------------ |
| **Printer Name**       | Anything you want (e.g. `Receipt Printer`)                |
| **Type**               | `Receipt` (or `Preparation` for kitchen tickets)         |
| **Printer IP Address** | `<your-machine-ip>:8080` (or `127.0.0.1:8080` for local) |
| **Use Local Network Access** | ON (required for remote HTTP; allows PNA preflight) |
| **Paper Size**         | `Standard 80mm` (matches `--paper-width 576`)              |
| **Printed Product Category** | Optional — route specific categories to this printer |

Click **Test** in Odoo to send the payload above. You should see:
- Banner log line: `epos printed | printer="Epson TM-T20III" bytes=N format=Soap`
- `200 OK` from Odoo
- A receipt on the printer with the test text, three blank lines, partial cut

### Local vs Remote

| Origin                | Protocol                | Why                                                                 |
| --------------------- | ----------------------- | ------------------------------------------------------------------- |
| `127.0.0.1` / `localhost` | HTTP works directly | Same-origin — no PNA, no mixed-content checks                            |
| Remote LAN IP             | HTTP + PNA preflight needed | Browser blocks by default; our `OPTIONS` echoes `Access-Control-Allow-Private-Network: true` |
| Remote, Odoo on HTTPS      | HTTP is blocked         | Mixed content — must use HTTPS (drop a reverse proxy in front, see below) |

---

## TLS via reverse proxy

`--tls` is not wired yet (the rustls / aws-lc-sys chain doesn't cross-compile
cleanly). For HTTPS, drop a reverse proxy in front.

### Caddy (easiest)

```bash
# /etc/caddy/Caddyfile
:8443 {
    reverse_proxy 127.0.0.1:8080
    tls internal    # Caddy signs its own cert; export the root CA from the
                   # Caddy admin UI and trust it on every terminal
}
```

### nginx

```nginx
server {
    listen 8443 ssl;
    server_name epos.local;

    ssl_certificate     /etc/nginx/ssl/epos.crt;
    ssl_certificate_key /etc/nginx/ssl/epos.key;

    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_set_header Host              $host;
        proxy_set_header X-Forwarded-For   $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

Then in Odoo POS use `https://<your-host>:8443/cgi-bin/epos/service.cgi`.

---

## Building from source

### macOS / Linux (dev)

```bash
git clone https://github.com/Lucif3rHun1/ePOS_Simulator.git
cd ePOS_Simulator

# Stable toolchain (1.75+)
rustup toolchain install stable
rustup default stable

# Build
cargo build --release

# Output: ./target/release/epos-emulator (~1.2 MB)

# Run with hot-reload during dev:
cargo run --release -- --printer "Epson TM-T20III" --verbose
```

### Cross-compile to Windows from macOS / Linux

```bash
rustup target add x86_64-pc-windows-msvc
cargo build --release --target x86_64-pc-windows-msvc

# Output: target/x86_64-pc-windows-msvc/release/epos-emulator.exe
```

> **Note**: pure Rust, no native deps at link time on the Rust side, but
> `windows-sys` ultimately talks to `winspool.drv` at *runtime* (not link time)
> — that's the whole point. Cross-compile works without Visual Studio Build Tools.

---

## Windows build (release binary)

The CI in `.github/workflows/build-windows.yml` produces a signed-ready
`epos-emulator.exe` artifact on every `v*` tag push.

### Trigger a release build

```bash
git tag v2.0.3
git push origin v2.0.3
# → GitHub Actions builds epos-emulator.exe on ubuntu-latest
# → Attaches it to the GitHub release
# → Generates release notes
```

Or trigger manually: **Actions → build-windows → Run workflow**.

### Build locally on Windows

```cmd
:: Install Rust
winget install Rustlang.Rustup
rustup default stable

:: Clone + build
git clone https://github.com/Lucif3rHun1/ePOS_Simulator.git
cd ePOS_Simulator
cargo build --release

:: Output: target\release\epos-emulator.exe
```

---

## Running as a Windows service

The binary doesn't ship an installer yet, but you can wire it to
`sc.exe` or NSSM in 2 minutes.

### Option A — `sc.exe` (no extra tools)

```cmd
sc.exe create ePOS-Emulator \
    binPath= ""C:\Program Files\ePOS\epos-emulator.exe" --printer "Epson TM-T20III" --log-file "C:\ProgramData\ePOS\epos.log"" \
    start= auto \
    DisplayName= "ePOS Printer Emulator"

sc.exe start ePOS-Emulator
sc.exe stop  ePOS-Emulator
sc.exe delete ePOS-Emulator
```

> The `binPath=` value needs the escaped quotes exactly as above — `sc.exe`
> is famously picky about that space after `=`.

### Option B — NSSM (more robust)

```cmd
winget install NSSM
nssm install ePOS-Emulator "C:\Program Files\ePOS\epos-emulator.exe" \
    --printer "Epson TM-T20III" --log-file "C:\ProgramData\ePOS\epos.log"
nssm set ePOS-Emulator AppStdout "C:\ProgramData\ePOS\stdout.log"
nssm set ePOS-Emulator AppStderr "C:\ProgramData\ePOS\stderr.log"
nssm set ePOS-Emulator AppRotateFiles 1
nssm set ePOS-Emulator AppRotateBytes 10485760
nssm start ePOS-Emulator
```

---

## Testing

```bash
# Unit tests (46 of them, runs in <1s)
cargo test --release --lib

# Run a specific test
cargo test --release --lib translates_odoo_pos_test_print

# Verify the Odoo POS payload round-trips through your running server
curl -X POST http://127.0.0.1:8080/cgi-bin/epos/service.cgi?devid=local_printer \
     -H 'Content-Type: text/xml; charset=utf-8' \
     --data-binary @testdata/odoo-pos-test-print.xml
# Expect 200 OK with a <soap:Envelope>...success...</soap:Envelope> response
```

### Bulk / concurrency

The HTTP server is hardened for Odoo's burst pattern:
- **Body cap**: 1 MB (request rejected with SOAP error if larger)
- **Per-request timeout**: 30 s
- **In-flight spooler cap**: 16 (configurable via `max_inflight_prints`)
- **Idempotency**: FNV-1a body fingerprint + LRU(1024) dedup; replays return the
  cached response with `X-Idempotency-Replay: true` instead of double-printing
- **Spooler retries**: 2 attempts, 50 ms / 100 ms backoff

These are not configurable from the CLI yet — edit `AppConfig::default()` in
`src/eposhttp.rs` if you need different limits.

---

## Architecture

```
                ┌──────────────────────────────────────────────────────────┐
                │ Odoo POS (browser)                                       │
                │   Epson ePOS SDK for JS → POST /cgi-bin/epos/service.cgi │
                └──────────────────────────────────────────────────────────┘
                                       │ HTTP
                                       ▼
┌──────────────────────────────────────────────────────────────────────────────┐
│ epos-emulator (axum / tokio)                                                  │
│  ┌────────────────┐  ┌────────────────┐  ┌────────────────┐                  │
│  │ HTTP hardening  │→ │ SOAP unwrap     │→ │ translate (XML │                  │
│  │ • 1 MB body cap │  │ (raw or SOAP)   │  │ → ESC/POS)    │                  │
│  │ • 30 s timeout  │  └────────────────┘  └────────────────┘                  │
│  │ • 16-slot queue │                                                          │
│  │ • idempotency   │                                                          │
│  └────────────────┘  ┌────────────────┐  ┌────────────────┐                  │
│        │              │ CORS / PNA       │  │ tracing → log  │                  │
│        ▼              │ headers          │  │ file (rotating)│                  │
│  ┌────────────────┐                                                          │
│  │ spawn_blocking │                                                          │
│  │ + 2x retry     │                                                          │
│  └────────────────┘                                                          │
└──────────────┬───────────────────────────────────────────────────────────────┘
               │ RAW bytes via winspool.drv (Windows only)
               ▼
       ┌──────────────────┐
       │ Windows spooler  │
       └──────────────────┘
               │ driver-dependent
               ▼
       ┌──────────────────┐
       │ USB receipt      │
       │ printer          │
       └──────────────────┘
```

```
src/
  main.rs            binary entry point
  lib.rs             module declarations
  cli.rs             clap + main run loop + graceful shutdown
  eposhttp.rs        axum router, CORS/PNA, hardening, dedup
  escpos.rs          ESC/POS byte builders (init/cut/feed/text/barcode/QR/raster)
  soap.rs            request format detection + response envelopes
  translate.rs       ePOS-Print XML → ESC/POS bytes (state machine)
  logging.rs         tracing-subscriber + rotating file writer
  netinfo.rs         local IPv4 enumeration for the startup banner
  picker.rs          interactive printer picker
  winspool.rs        Windows print spooler wrapper (RAW) + non-Windows stub

testdata/
  odoo-soap-request.xml         generic SOAP receipt
  text-only-soap.xml            minimal text-only receipt
  odoo-pos-test-print.xml       exact payload Odoo sends from the Printer > Test button
```

---

## What's new in v2.x

v2.x is a clean **Rust port** of the original Go project (`v1.x`).

### v2.0.3 (current)

- **`<cut type="feed"/>`** now correctly emits `ESC d 3 + GS V 1` (was `GS V 0` — full cut, jammed receipts).
- Default `<cut/>` is partial cut (`GS V 1`) — what every receipt printer actually wants.
- New variants: `<cut type="no-feed"/>` (partial, no feed), `<cut type="full"/>` (full cut).
- Trailing `&#10;` in `<text>...</text>` is preserved (was stripped by `.trim()` — Odoo uses `&#10;` as a line separator).
- Fixture `testdata/odoo-pos-test-print.xml` = exact payload Odoo sends from the Printer > Test button.
- 46/46 lib tests pass.

### v2.0.2

- Body size cap (1 MB), per-request timeout (30 s), bounded concurrency (16), FNV-1a idempotency dedup with `X-Idempotency-Replay`, 2× spooler retry with backoff, OPTIONS `/` preflight with PNA, graceful SIGINT/SIGTERM shutdown.

### v2.0.0 / v2.0.1

- Pure Rust, no OpenSSL/winspool link-time deps.
- Cross-platform build (`cargo build` on any host).
- Go predecessor's wire protocol preserved byte-for-byte (CORS headers, SOAP envelopes, ESC/POS output).
- Go code removed.

---

## License

MIT — same as the Go predecessor.
