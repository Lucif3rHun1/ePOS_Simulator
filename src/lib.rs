//! ePOS Printer Emulator library: ESC/POS builder, ePOS-Print XML translator,
//! SOAP envelope helper, winspool wrapper, HTTP handler, and CLI entry point.
//!
//! The crate is split into modules that mirror the original Go project:
//!
//! - [`escpos`]  — raw ESC/POS byte sequences
//! - [`soap`]    — request body format detection + response envelope builders
//! - [`translate`] — XML walker: ePOS-Print elements → ESC/POS bytes
//! - [`winspool`] — Windows spooler RAW print + interactive picker
//! - [`http`]    — axum HTTP server with CORS/PNA/health/print endpoints
//! - [`logging`] — structured tracing-based logger with rotation
//! - [`tls`]     — self-signed certificate generation (rustls + rcgen)
//! - [`netinfo`] — enumerate local IPv4 addresses for the startup banner
//! - [`cli`]     — `clap` definitions + main entry point

#![deny(rust_2018_idioms)]
#![warn(missing_debug_implementations)]

pub mod cli;
pub mod eposhttp;
pub mod escpos;
pub mod logging;
pub mod netinfo;
pub mod picker;
pub mod soap;
pub mod tls;
pub mod translate;
pub mod winspool;
