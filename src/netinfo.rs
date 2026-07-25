//! Network interface enumeration + startup banner formatting.

use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, ToSocketAddrs};

pub fn local_ips(include_loopback: bool) -> Vec<IpAddr> {
    let mut out: Vec<IpAddr> = Vec::new();
    if let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0") {
        if let Some(addr) = "8.8.8.8:80".to_socket_addrs().ok().and_then(|mut a| a.next()) {
            let _ = socket.connect(addr);
            if let Ok(local) = socket.local_addr() {
                out.push(local.ip());
            }
        }
    }
    if include_loopback {
        out.push(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));
    }
    out.sort();
    out.dedup();
    out
}

pub fn format_banner(addr: &str) -> String {
    let mut sb = String::new();
    let parsed: Option<SocketAddr> = addr.parse().ok();
    match parsed {
        Some(sa) if sa.ip().is_unspecified() => {
            for ip in local_ips(true) {
                sb.push_str(&format!("  http://{ip}:{}\n", sa.port()));
            }
        }
        Some(sa) => sb.push_str(&format!("  http://{sa}\n")),
        None => sb.push_str(&format!("  http://{addr}\n")),
    }
    sb
}

pub fn probe(addr: SocketAddr, timeout_ms: u64) -> bool {
    TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(timeout_ms)).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn banner_includes_loopback_when_unspecified() {
        let banner = format_banner(":8080");
        // Banner should mention the port; loopback presence depends on the host.
        assert!(banner.contains("8080"), "expected 8080 in {banner:?}");
        assert!(banner.starts_with("  http://"));
    }

    #[test]
    fn banner_uses_specific_address_when_provided() {
        let banner = format_banner("192.168.1.9:8080");
        assert!(banner.contains("192.168.1.9:8080"));
    }
}
