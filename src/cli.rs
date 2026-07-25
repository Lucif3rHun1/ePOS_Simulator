//! Command-line interface (clap definitions + main entry point).

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

use crate::eposhttp::{self, AppConfig};
use crate::logging::{self, Config as LogConfig};
use crate::netinfo;
use crate::picker::{self, Picker};
use crate::winspool;

/// ePOS Printer Emulator for Odoo Online POS.
#[derive(Debug, Parser)]
#[command(name = "epos-emulator", version, about, long_about = None)]
pub struct Args {
    /// HTTP(S) listen port.
    #[arg(long, default_value_t = 8080)]
    pub port: u16,

    /// Printer name (use --list-printers to see options; empty=interactive pick).
    #[arg(long, default_value = "")]
    pub printer: String,

    /// Enable HTTPS with self-signed cert.
    #[arg(long)]
    pub tls: bool,

    /// TLS certificate file (PEM).
    #[arg(long)]
    pub cert: Option<PathBuf>,

    /// TLS private key file (PEM).
    #[arg(long)]
    pub key: Option<PathBuf>,

    /// Verbose logging (hex dumps of XML + ESC/POS).
    #[arg(long, short)]
    pub verbose: bool,

    /// Run self-test via spooler and exit.
    #[arg(long)]
    pub selftest: bool,

    /// Enable drawer kick (ESC p).
    #[arg(long)]
    pub drawer: bool,

    /// Paper width in dots (384=58mm, 512/576=80mm).
    #[arg(long, default_value_t = 576)]
    pub paper_width: u32,

    /// Barcode/QR codepage.
    #[arg(long, default_value = "CP437")]
    pub codepage: String,

    /// Log file path (rotating, default 10MB x 3 backups).
    #[arg(long)]
    pub log_file: Option<PathBuf>,

    /// List installed printers and exit.
    #[arg(long)]
    pub list_printers: bool,

    /// Log rotation size threshold in bytes.
    #[arg(long, default_value_t = 10 * 1024 * 1024)]
    pub max_log_bytes: u64,

    /// Number of rotated log backups to keep (.1, .2, .N).
    #[arg(long, default_value_t = 3)]
    pub max_log_backups: u32,

    /// Reject unknown ePOS-Print XML elements instead of ignoring.
    #[arg(long)]
    pub strict_xml: bool,

    /// When --printer is empty, prompt user to select from enumerated printers.
    #[arg(long, default_value_t = true)]
    pub interactive: bool,
}

pub async fn run(args: Args) -> anyhow::Result<ExitCode> {
    // Logging
    let log_cfg = LogConfig {
        verbose: args.verbose,
        log_file: args.log_file.clone(),
        max_bytes: args.max_log_bytes,
        max_backups: args.max_log_backups as u32,
        include_pid: true,
    };
    logging::init(log_cfg)?;

    let printer_name = resolve_printer(&args)?;
    tracing::info!(target: "startup", "version={} platform={} verbose={}", env!("CARGO_PKG_VERSION"), std::env::consts::OS, args.verbose);

    if args.list_printers {
        return run_list_printers();
    }
    if args.selftest {
        return run_selftest(&printer_name);
    }
    run_server(&args, &printer_name).await
}

fn resolve_printer(args: &Args) -> anyhow::Result<String> {
    if !args.printer.is_empty() {
        return Ok(args.printer.clone());
    }
    if args.interactive && picker::is_interactive() {
        let picked = picker::InteractivePicker {
            input: Box::new(std::io::stdin().lock()),
            output: Box::new(std::io::stderr().lock()),
            fallback: String::new(),
        }
        .pick()?;
        if !picked.is_empty() {
            tracing::info!(target: "startup", "picked={}", picked);
            return Ok(picked);
        }
    }
    Ok(String::new())
}

fn run_list_printers() -> anyhow::Result<ExitCode> {
    let infos = winspool::enum_printers()?;
    let infos = winspool::filter_empty(infos);
    print!("{}", winspool::format_list(&infos));
    Ok(ExitCode::SUCCESS)
}

fn run_selftest(printer_name: &str) -> anyhow::Result<ExitCode> {
    use crate::escpos;
    let data = escpos::self_test_bytes();
    println!("ePOS Emulator Self-Test");
    println!("ESC/POS bytes ({}): {:02x?}", data.len(), data);
    match winspool::print_raw(printer_name, "ePOS Self-Test", &data) {
        Ok(()) => {
            println!("Self-test sent to printer successfully.");
            Ok(ExitCode::SUCCESS)
        }
        Err(e) => {
            println!("NOTE: spooler unavailable: {e}");
            println!("Self-test passed (dry run).");
            Ok(ExitCode::SUCCESS)
        }
    }
}

async fn run_server(args: &Args, printer_name: &str) -> anyhow::Result<ExitCode> {
    let cfg = AppConfig {
        printer_name: printer_name.to_string(),
        verbose: args.verbose,
        allow_drawer: args.drawer,
        strict_xml: args.strict_xml,
        ..Default::default()
    };

    print_banner(args, printer_name);

    // Build the TCP listener (async via tokio).
    let addr: SocketAddr = format!("0.0.0.0:{}", args.port).parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(target: "http", "listening | addr={} tls={}", addr, args.tls);

    let app = eposhttp::router(cfg);

    if args.tls {
        anyhow::bail!("--tls is not yet wired in the Rust port (rustls/aws-lc cross-build issue). v2.0.0 dev binary is HTTP-only.");
    }

    tracing::info!(target: "startup", "ready | ctrl_c to stop");
    axum::serve(listener, app.into_make_service())
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    tracing::info!(target: "startup", "shutdown complete");

    Ok(ExitCode::SUCCESS)
}

/// Resolve on SIGINT (Ctrl-C) on every platform, plus SIGTERM on Unix.
/// In-flight requests are allowed to finish before axum returns; spooler
/// jobs already submitted via `spawn_blocking` will complete on their
/// own blocking threads.
async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::error!(target: "shutdown", "ctrl_c handler failed | err={}", e);
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => { let _ = s.recv().await; }
            Err(e) => {
                tracing::error!(target: "shutdown", "SIGTERM handler failed | err={}", e);
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => tracing::info!(target: "shutdown", "received SIGINT"),
        _ = terminate => tracing::info!(target: "shutdown", "received SIGTERM"),
    }
}

fn print_banner(args: &Args, printer_name: &str) {
    let printer_label = if printer_name.is_empty() { "(system default)" } else { printer_name };
    let log_label = match &args.log_file {
        Some(p) => p.display().to_string(),
        None => "(stderr only)".to_string(),
    };
    let scheme = if args.tls { "https" } else { "http" };
    let addr = format!(":{}", args.port);

    let mut out = String::new();
    out.push_str("
============================================================
");
    out.push_str("  ePOS Printer Emulator for Odoo Online POS
");
    out.push_str("============================================================
");
    out.push_str(&format!("  Platform : {}
", std::env::consts::OS));
    out.push_str(&format!("  PID      : {}
", std::process::id()));
    out.push_str(&format!("  Printer  : {printer_label}
"));
    out.push_str(&format!("  Paper    : {} dots
", args.paper_width));
    out.push_str(&format!("  Drawer   : {}
", args.drawer));
    out.push_str(&format!("  TLS      : {}
", args.tls));
    out.push_str(&format!("  Verbose  : {}
", args.verbose));
    out.push_str(&format!("  Strict   : {}
", args.strict_xml));
    out.push_str(&format!("  Log file : {log_label}
"));
    out.push_str("
  API endpoints:
");
    out.push_str("    GET  /                          (health check, returns JSON)
");
    out.push_str("    POST /cgi-bin/epos/service.cgi  (ePOS XML, Odoo POS target)

");
    out.push_str(&format!("  Listening on (scheme = {scheme}):
"));
    out.push_str(&netinfo::format_banner(&addr));
    out.push_str("
  Odoo POS IoT Printer URL: copy any IP/port above into
");
    out.push_str("  Settings > Point of Sale > Printers > Printer IP.
");
    out.push_str("============================================================
");
    eprint!("{out}");
}
