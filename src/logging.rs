//! Structured logging + rotating file writer.
//!
//! Wraps the `tracing` crate. A rotating file writer (`RotateWriter`) is
//! provided as a stand-alone `io::Write` implementation that delegates to
//! `tracing-subscriber`'s JSON layer or to a plain file.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

/// Runtime configuration for the logger.
#[derive(Debug, Clone)]
pub struct Config {
    pub verbose: bool,
    pub log_file: Option<PathBuf>,
    pub max_bytes: u64,
    pub max_backups: u32,
    pub include_pid: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            verbose: false,
            log_file: None,
            max_bytes: 10 * 1024 * 1024,
            max_backups: 3,
            include_pid: true,
        }
    }
}

/// Initialize the global tracing subscriber.
///
/// - `verbose = true` -> `tracing` debug-level
/// - `verbose = false` -> `tracing` info-level
/// - `log_file = Some(path)` -> tee to a rotating file
pub fn init(cfg: Config) -> io::Result<()> {
    let level = if cfg.verbose { tracing::Level::DEBUG } else { tracing::Level::INFO };

    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(match level {
            tracing::Level::DEBUG => "debug",
            tracing::Level::INFO => "info",
            tracing::Level::WARN => "warn",
            tracing::Level::ERROR => "error",
            _ => "info",
        })
    });

    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_target(false)
        .with_writer(std::io::stderr)
        .with_ansi(false);

    let registry = tracing_subscriber::registry().with(env_filter).with(fmt_layer);

    if let Some(path) = cfg.log_file {
        let writer = RotateWriter::new(&path, cfg.max_bytes, cfg.max_backups)?;
        let file_layer = tracing_subscriber::fmt::layer()
            .with_target(false)
            .with_ansi(false)
            .with_writer(move || writer.clone());
        registry.with(file_layer).try_init().map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
    } else {
        registry.try_init().map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
    }
    Ok(())
}

/// Close any open log file handles. `tracing-subscriber` does not own the file
/// directly; we let `Drop` handle it. Provided for symmetry with the Go API.
pub fn close() {}

/// A thread-safe rotating file writer.
///
/// When the file exceeds `max_bytes`, it is renamed to `path.1`, and existing
/// `.1` is renamed to `.2`, etc. Backups older than `max_backups` are deleted.
#[derive(Clone)]
pub struct RotateWriter {
    inner: std::sync::Arc<Mutex<RotateState>>,
}

struct RotateState {
    path: PathBuf,
    max_bytes: u64,
    max_backups: u32,
    file: Option<File>,
    bytes_written: u64,
}

impl RotateWriter {
    /// Open `path` for append. Creates the file if it does not exist.
    pub fn new(path: impl AsRef<Path>, max_bytes: u64, max_backups: u32) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file = open_append(&path)?;
        let bytes_written = file.metadata().map(|m| m.len()).unwrap_or(0);
        Ok(Self {
            inner: std::sync::Arc::new(Mutex::new(RotateState {
                path,
                max_bytes,
                max_backups,
                file: Some(file),
                bytes_written,
            })),
        })
    }

    fn rotate_if_needed(state: &mut RotateState) -> io::Result<()> {
        if state.bytes_written < state.max_bytes {
            return Ok(());
        }
        // Drop current file to release the handle before renaming.
        state.file.take();
        // Shift backups: .N-1 -> .N down to .1 -> .2; oldest dropped.
        for i in (1..state.max_backups).rev() {
            let from = backup_path(&state.path, i);
            let to = backup_path(&state.path, i + 1);
            if from.exists() {
                let _ = std::fs::rename(&from, &to);
            }
        }
        let first_backup = backup_path(&state.path, 1);
        std::fs::rename(&state.path, &first_backup)?;
        state.file = Some(open_append(&state.path)?);
        state.bytes_written = 0;
        Ok(())
    }
}

fn open_append(path: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

fn backup_path(path: &Path, n: u32) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(format!(".{n}"));
    PathBuf::from(s)
}

impl Write for RotateWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut state = self.inner.lock().unwrap();
        RotateWriter::rotate_if_needed(&mut state)?;
        let n = state.file.as_mut().unwrap().write(buf)?;
        state.bytes_written += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut state = self.inner.lock().unwrap();
        state.file.as_mut().unwrap().flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn rotate_creates_backup_when_full() {
        let dir = std::env::temp_dir().join("epos-emulator-logger-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rotate.log");
        let _ = std::fs::remove_file(&path);

        let mut w = RotateWriter::new(&path, 8, 2).unwrap();
        w.write_all(b"AAAAAAAA").unwrap();
        w.write_all(b"BBBBBBBB").unwrap(); // forces rotate
        drop(w);

        let mut buf = String::new();
        File::open(&path).unwrap().read_to_string(&mut buf).unwrap();
        assert_eq!(buf, "BBBBBBBB");

        let backup = backup_path(&path, 1);
        let mut buf2 = String::new();
        File::open(&backup).unwrap().read_to_string(&mut buf2).unwrap();
        assert_eq!(buf2, "AAAAAAAA");

        std::fs::remove_file(&path).ok();
        std::fs::remove_file(&backup).ok();
    }
}
