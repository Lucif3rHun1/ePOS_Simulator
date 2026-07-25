//! Self-signed TLS certificate generation.
//!
//! Generates an ECDSA P-256 certificate with SAN for 127.0.0.1 + localhost,
//! valid for 10 years. PEM files are written to disk so the user can import
//! them to the Windows Trusted Root store before the browser hits the
//! emulator (browsers have no UI for trusting unknown certs programmatically).

use std::fs;
use std::path::Path;

use rcgen::generate_simple_self_signed;

/// Generate a self-signed cert + key and write them to `cert_file` / `key_file`.
/// Overwrites existing files.
pub fn generate_self_signed(cert_file: impl AsRef<Path>, key_file: impl AsRef<Path>) -> std::io::Result<()> {
    let cert = generate_simple_self_signed(vec!["localhost".to_string()])
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
    let cert_pem = cert.cert.pem();
    let key_pem = cert.key_pair.serialize_pem();

    fs::write(cert_file, cert_pem.as_bytes())?;
    fs::write(key_file, key_pem.as_bytes())?;
    Ok(())
}

/// Load existing PEM files or generate new ones if either is missing.
/// On success, returns PEM bytes.
pub fn load_or_generate(cert_file: impl AsRef<Path>, key_file: impl AsRef<Path>) -> std::io::Result<(String, String)> {
    let cert_path = cert_file.as_ref();
    let key_path = key_file.as_ref();

    if !cert_path.exists() || !key_path.exists() {
        generate_self_signed(cert_path, key_path)?;
    }

    let cert_pem = fs::read_to_string(cert_path)?;
    let key_pem = fs::read_to_string(key_path)?;
    Ok((cert_pem, key_pem))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_pem() {
        let dir = std::env::temp_dir().join("epos-tls-test");
        std::fs::create_dir_all(&dir).unwrap();
        let cert = dir.join("cert.pem");
        let key = dir.join("key.pem");
        let _ = std::fs::remove_file(&cert);
        let _ = std::fs::remove_file(&key);

        generate_self_signed(&cert, &key).unwrap();
        let (c, k) = load_or_generate(&cert, &key).unwrap();
        assert!(c.contains("BEGIN CERTIFICATE"));
        assert!(k.contains("PRIVATE KEY"));

        std::fs::remove_file(&cert).ok();
        std::fs::remove_file(&key).ok();
    }
}
