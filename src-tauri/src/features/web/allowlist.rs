//! Host allowlist for the `agentero-web` proxy.
//!
//! The proxy must never become an open relay (the stance the 广场 site proxies
//! take): only hosts explicitly allowed here are served. The frontend adds a
//! paper's host right before loading its viewer frame. Subresources on other
//! hosts never touch the proxy — the injected `<base>` makes them load
//! directly from the site.

use std::collections::HashSet;
use std::sync::RwLock;

static ALLOWED: RwLock<Option<HashSet<String>>> = RwLock::new(None);

/// Lowercase-trim a caller-supplied host.
pub fn normalize_host(host: &str) -> String {
    host.trim().trim_end_matches('.').to_ascii_lowercase()
}

/// Hosts the allowlist accepts: public DNS names only.
///
/// IP literals (v4/v6), loopback names, dotless intranet labels and mDNS /
/// internal-search suffixes are refused — the proxy must not be aimable at
/// link-local metadata endpoints or intranet hosts (SSRF guard).
pub fn is_public_dns_host(host: &str) -> bool {
    if host.is_empty() || !host.contains('.') {
        return false;
    }
    if host == "localhost" || host.ends_with(".localhost") {
        return false;
    }
    if host.ends_with(".local") || host.ends_with(".internal") || host.ends_with(".arpa") {
        return false;
    }
    if host.parse::<std::net::IpAddr>().is_ok() {
        return false;
    }
    host.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.'))
}

/// Whether the proxy may serve `host`.
pub fn is_allowed(host: &str) -> bool {
    let host = normalize_host(host);
    match ALLOWED.read() {
        Ok(set) => set.as_ref().is_some_and(|s| s.contains(&host)),
        Err(_) => false,
    }
}

/// Add `host` to the allowlist (idempotent; invalid hosts are dropped).
pub fn allow_host(host: &str) {
    let host = normalize_host(host);
    if !is_public_dns_host(&host) {
        log::warn!(target: "agentero::web_proxy", "refused to proxy non-public host: {host}");
        return;
    }
    if let Ok(mut set) = ALLOWED.write() {
        set.get_or_insert_with(HashSet::new).insert(host);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_public_dns_hosts() {
        assert!(is_public_dns_host("arxiv.org"));
        assert!(is_public_dns_host("blog.nature.com"));
        assert!(is_public_dns_host("xn--e1afmkfd.xn--p1ai"));
    }

    #[test]
    fn refuses_non_public_hosts() {
        // Loopback / dotless / internal suffixes / IP literals.
        assert!(!is_public_dns_host("localhost"));
        assert!(!is_public_dns_host("intranet"));
        assert!(!is_public_dns_host("printer.internal"));
        assert!(!is_public_dns_host("raspberrypi.local"));
        assert!(!is_public_dns_host("169.254.169.254"));
        assert!(!is_public_dns_host("127.0.0.1"));
        assert!(!is_public_dns_host("::1"));
        assert!(!is_public_dns_host("10.0.0.1"));
        assert!(!is_public_dns_host(""));
        // Not a hostname at all.
        assert!(!is_public_dns_host("arxiv.org:8080/path"));
    }

    #[test]
    fn allow_then_serve() {
        assert!(!is_allowed("Example.COM"));
        allow_host("Example.COM");
        assert!(is_allowed("example.com"));
        assert!(is_allowed("EXAMPLE.com"));
    }

    #[test]
    fn allow_drops_invalid_hosts() {
        allow_host("169.254.169.254");
        assert!(!is_allowed("169.254.169.254"));
    }
}
