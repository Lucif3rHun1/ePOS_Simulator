// Package netinfo resolves local network interfaces and IPs for the startup banner.
package netinfo

import (
	"fmt"
	"net"
	"sort"
	"strings"
)

// LocalIPs returns all non-loopback IPv4 addresses on this machine, sorted.
// Includes 127.0.0.1 if includeLoopback is true.
func LocalIPs(includeLoopback bool) []string {
	addrs, err := net.InterfaceAddrs()
	if err != nil {
		return nil
	}
	var ips []string
	for _, a := range addrs {
		var ip net.IP
		switch v := a.(type) {
		case *net.IPNet:
			ip = v.IP
		case *net.IPAddr:
			ip = v.IP
		}
		if ip == nil || ip.To4() == nil {
			continue
		}
		isLoopback := ip.IsLoopback()
		if isLoopback && !includeLoopback {
			continue
		}
		ips = append(ips, ip.String())
	}
	sort.Strings(ips)
	return ips
}

// FormatBanner returns a multi-line banner showing every reachable address for addr
// (e.g. ":8080"). IPv4 and IPv6 variants are emitted separately so the user can
// copy any of them into Odoo POS IoT / Printer settings.
func FormatBanner(addr string) string {
	host, port := splitHostPort(addr)
	if host == "" || host == "0.0.0.0" || host == "::" {
		var sb strings.Builder
		fmt.Fprintf(&sb, "Listening on port %s\n", port)
		ips := LocalIPs(true)
		if len(ips) == 0 {
			fmt.Fprintf(&sb, "  (no network interfaces found)\n")
			return sb.String()
		}
		for _, ip := range ips {
			fmt.Fprintf(&sb, "  http://%s:%s\n", ip, port)
		}
		return sb.String()
	}
	return fmt.Sprintf("Listening on http://%s\n", addr)
}

func splitHostPort(addr string) (host, port string) {
	if !strings.Contains(addr, ":") {
		return "", addr
	}
	if strings.HasPrefix(addr, "[") {
		end := strings.Index(addr, "]")
		if end < 0 {
			return "", strings.TrimPrefix(addr, "[")
		}
		return addr[1:end], addr[end+2:]
	}
	idx := strings.LastIndex(addr, ":")
	if idx < 0 {
		return "", addr
	}
	return addr[:idx], addr[idx+1:]
}
