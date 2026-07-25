# ADR 0001: Spooler RAW over libusb/WinUSB

## Status
Accepted. 2026-07-25.

## Context
The ePOS Simulator needs to deliver ESC/POS bytes to a USB thermal printer
on Windows. Two transports exist:

1. **Windows print spooler (RAW datatype)**: send bytes through the user's
   installed Epson driver via `OpenPrinterW`/`StartDocPrinterW`/`WritePrinter`.
2. **Direct USB via libusb/WinUSB**: open the USB device, claim the bulk-OUT
   endpoint, write bytes directly.

The official Odoo `epos-proxy` project uses option 2 (libusb on darwin,
WinUSB on windows).

## Decision
Use **option 1 (spooler RAW)**.

## Consequences
**Positive**:
- Works with whatever driver the user already installed (Epson APD, generic
  text driver, etc.).
- No driver-signing / WinUSB driver install required (the user would need
  Zadig or a vendor INF to set up WinUSB on Windows).
- Compatible with virtual printers (`Microsoft Print to PDF`, OneNote) for
  dry-run testing on machines without a real Epson.

**Negative**:
- Requires the user to have already installed an Epson-compatible printer
  driver in Windows. If not, print spooler rejects RAW.
- Cannot bypass the driver. If the driver adds unwanted transformations,
  we cannot fix them.
- Cannot auto-detect the printer by USB VID/PID without going through the
  spooler.

## Rationale
- Odoo's user base is non-technical; requiring a WinUSB driver install is
  a blocker.
- Most users installing an ESC/POS thermal printer on Windows already
  install the vendor's driver anyway.
- The "system default printer" fallback gives dry-run testing on any
  Windows machine.

## When to revisit
- If we need bidirectional status (out-of-paper, cover-open), the spooler
  does not expose these easily. Direct USB or vendor SDK would be required.
- If targeting macOS/Linux v2, libusb becomes the right choice on those
  platforms (no spooler).
