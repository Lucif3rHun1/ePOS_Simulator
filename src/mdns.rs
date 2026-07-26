//! LAN discovery via mDNS/DNS-SD (Bonjour/Avahi-compatible).
//!
//! Advertises this printer as `_epos._tcp.local.` so phones and other apps
//! on the same WiFi network can find it (IP + port + the ePOS endpoint
//! path) without the IP being typed in manually. Purely additive: the HTTP
//! server already accepts requests from any device that reaches it, this
//! just makes it discoverable.

use std::collections::HashMap;
use std::net::IpAddr;

use mdns_sd::{ServiceDaemon, ServiceInfo};

use crate::netinfo;

/// DNS-SD service type this daemon advertises under.
pub const SERVICE_TYPE: &str = "_epos._tcp.local.";

/// Holds the running mDNS daemon + the fullname of our registered service,
/// so it can be unregistered and shut down cleanly.
pub struct MdnsHandle {
    daemon: ServiceDaemon,
    fullname: String,
}

impl MdnsHandle {
    /// Unregister the service and stop the mDNS daemon. Best-effort: log
    /// and move on if either step fails, since this only runs on shutdown.
    pub fn stop(self) {
        if let Ok(recv) = self.daemon.unregister(&self.fullname) {
            let _ = recv.recv_timeout(std::time::Duration::from_millis(500));
        }
        if let Ok(recv) = self.daemon.shutdown() {
            let _ = recv.recv_timeout(std::time::Duration::from_millis(500));
        }
    }
}

/// Start advertising the printer on the local network. Returns `None`
/// (after logging a warning) if mDNS can't start — e.g. no usable network
/// interface, or multicast blocked — since discovery is a convenience, not
/// a requirement for the HTTP server to work.
pub fn advertise(port: u16, printer_name: &str) -> Option<MdnsHandle> {
    match try_advertise(port, printer_name) {
        Ok(handle) => Some(handle),
        Err(e) => {
            tracing::warn!(target: "mdns", "could not start LAN discovery | err={}", e);
            None
        }
    }
}

fn try_advertise(port: u16, printer_name: &str) -> anyhow::Result<MdnsHandle> {
    let daemon = ServiceDaemon::new()?;

    let ips: Vec<IpAddr> = netinfo::local_ips(false);
    if ips.is_empty() {
        anyhow::bail!("no local network address found to advertise");
    }

    let host_name = format!("{}.local.", hostname_label());
    let instance_name = format!("ePOS Emulator on {}", hostname_label());

    let mut properties: HashMap<String, String> = HashMap::new();
    properties.insert("path".to_string(), "/cgi-bin/epos/service.cgi".to_string());
    properties.insert(
        "printer".to_string(),
        if printer_name.is_empty() { "(system default)".to_string() } else { printer_name.to_string() },
    );
    properties.insert("version".to_string(), env!("CARGO_PKG_VERSION").to_string());

    let service = ServiceInfo::new(
        SERVICE_TYPE,
        &instance_name,
        &host_name,
        ips.as_slice(),
        port,
        Some(properties),
    )?;

    let fullname = service.get_fullname().to_string();
    daemon.register(service)?;

    tracing::info!(
        target: "mdns",
        "advertising | service={} name={} port={}",
        SERVICE_TYPE, instance_name, port
    );

    Ok(MdnsHandle { daemon, fullname })
}

/// Best-effort machine name to use as the mDNS hostname label.
fn hostname_label() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "epos-emulator".to_string())
}
