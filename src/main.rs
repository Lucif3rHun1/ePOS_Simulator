//! ePOS Printer Emulator binary entry point.

use std::process::ExitCode;

use clap::Parser;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> ExitCode {
    let args = epos_emulator::cli::Args::parse();
    match epos_emulator::cli::run(args).await {
        Ok(code) => code,
        Err(e) => {
            eprintln!("epos-emulator: fatal: {e}");
            ExitCode::FAILURE
        }
    }
}
