//! Shared outbound-URL guard (operator directive): https only, port 443,
//! host allowlisted to YouTube's CDN families, and every address the host
//! resolves to must be public. Loopback, private, link-local, CGNAT,
//! reserved, and documentation ranges are all rejected. Callers fetch the
//! returned [`Url`] with redirects disabled, so the guard covers every hop.

use std::io;
use std::net::{IpAddr, ToSocketAddrs};
use url::Url;

#[derive(Debug, thiserror::Error)]
pub enum GuardError {
    #[error("blocked scheme: {0}")]
    Scheme(String),
    #[error("blocked port: {0:?}")]
    Port(Option<u16>),
    #[error("blocked host: {0}")]
    Host(String),
    #[error("host did not resolve: {0}")]
    Unresolved(String),
    #[error("blocked resolved address: {0}")]
    PrivateAddress(IpAddr),
    #[error("invalid url: {0}")]
    Parse(#[from] url::ParseError),
    #[error("resolution failed: {0}")]
    Resolve(#[from] io::Error),
}

const ALLOWED_SUFFIXES: [&str; 4] = ["googlevideo.com", "youtube.com", "ytimg.com", "ggpht.com"];

/// Guard a URL using the system resolver. Blocking; call from async code via
/// `spawn_blocking`.
pub fn guard(raw: &str) -> Result<Url, GuardError> {
    guard_with(raw, |host| {
        (host, 443u16)
            .to_socket_addrs()
            .map(|it| it.map(|a| a.ip()).collect())
    })
}

/// Guard a URL with an injected resolver, so tests can exercise the
/// resolved-address checks without touching the network.
pub fn guard_with(
    raw: &str,
    resolve: impl Fn(&str) -> io::Result<Vec<IpAddr>>,
) -> Result<Url, GuardError> {
    let url = Url::parse(raw)?;
    if url.scheme() != "https" {
        return Err(GuardError::Scheme(url.scheme().to_string()));
    }
    if url.port_or_known_default() != Some(443) {
        return Err(GuardError::Port(url.port()));
    }
    let host = url
        .host_str()
        .ok_or_else(|| GuardError::Host(String::new()))?
        .to_ascii_lowercase();
    let is_ip_literal = host.parse::<IpAddr>().is_ok();
    let blocked_name = host == "localhost" || host.ends_with(".local");
    if blocked_name || is_ip_literal || !allowed(&host) {
        return Err(GuardError::Host(host));
    }
    let addrs = resolve(&host)?;
    if addrs.is_empty() {
        return Err(GuardError::Unresolved(host));
    }
    for ip in addrs.iter().copied() {
        if !ip_is_public(ip) {
            return Err(GuardError::PrivateAddress(ip));
        }
    }
    Ok(url)
}

fn allowed(host: &str) -> bool {
    ALLOWED_SUFFIXES
        .iter()
        .any(|s| host == *s || host.ends_with(&format!(".{s}")))
}

fn ip_is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            let reserved_band = o[0] & 240 == 240; // 240.0.0.0/4
            let cgnat = o[0] == 100 && (o[1] & 0xC0) == 64; // 100.64.0.0/10
            !(v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_unspecified()
                || cgnat
                || reserved_band)
        }
        IpAddr::V6(v6) => {
            let s = v6.segments();
            let link_local = (s[0] & 0xffc0) == 0xfe80; // fe80::/10
            let unique_local = (s[0] & 0xfe00) == 0xfc00; // fc00::/7
            !(v6.is_loopback() || v6.is_unspecified() || link_local || unique_local)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn public(_: &str) -> io::Result<Vec<IpAddr>> {
        Ok(vec![IpAddr::V4(Ipv4Addr::new(142, 250, 1, 1))])
    }

    #[test]
    fn rejects_plain_http() {
        let err = guard_with("http://rr3.googlevideo.com/x", public).unwrap_err();
        assert!(matches!(err, GuardError::Scheme(s) if s == "http"));
    }

    #[test]
    fn rejects_nonstandard_port() {
        let err = guard_with("https://rr3.googlevideo.com:8443/x", public).unwrap_err();
        assert!(matches!(err, GuardError::Port(Some(8443))));
    }

    #[test]
    fn rejects_localhost_and_local_suffix_and_ip_literals() {
        for raw in [
            "https://localhost/x",
            "https://foo.local/x",
            "https://127.0.0.1/x",
            "https://[::1]/x",
        ] {
            let err = guard_with(raw, public).unwrap_err();
            assert!(matches!(err, GuardError::Host(_)), "{raw}: {err}");
        }
    }

    #[test]
    fn rejects_hosts_outside_the_allowlist() {
        let err = guard_with("https://evil.example.com/x", public).unwrap_err();
        assert!(matches!(err, GuardError::Host(h) if h == "evil.example.com"));
        // Suffix games do not pass: the allowlist matches domain boundaries.
        let err = guard_with("https://googlevideo.com.evil.example.com/x", public).unwrap_err();
        assert!(matches!(err, GuardError::Host(_)));
    }

    type Resolver = fn(&str) -> io::Result<Vec<IpAddr>>;

    #[test]
    fn rejects_private_resolved_addresses() {
        // Non-capturing closures coerce to fn pointers, which keeps the
        // higher-ranked `for<'a> Fn(&'a str)` bound satisfiable.
        let resolvers: [Resolver; 6] = [
            |_| Ok(vec![IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))]),
            |_| Ok(vec![IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1))]),
            |_| Ok(vec![IpAddr::V4(Ipv4Addr::new(172, 16, 0, 1))]),
            |_| Ok(vec![IpAddr::V4(Ipv4Addr::new(100, 64, 0, 1))]),
            |_| Ok(vec![IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))]),
            |_| Ok(vec![IpAddr::V4(Ipv4Addr::new(169, 254, 1, 1))]),
        ];
        for r in resolvers {
            let err = guard_with("https://rr3.googlevideo.com/x", r).unwrap_err();
            assert!(matches!(err, GuardError::PrivateAddress(_)), "{err}");
        }
    }

    #[test]
    fn rejects_unresolved_and_empty_resolution() {
        let err = guard_with("https://rr3.googlevideo.com/x", |_| Ok(vec![])).unwrap_err();
        assert!(matches!(err, GuardError::Unresolved(_)));
    }

    #[test]
    fn accepts_allowlisted_host_with_public_resolution() {
        for raw in [
            "https://rr3---sn-jvhh4pcgx-ajts.googlevideo.com/videoplayback?itag=140",
            "https://music.youtube.com/youtubei/v1/search",
            "https://i.ytimg.com/vi/abc/hq720.jpg",
        ] {
            guard_with(raw, public).expect(raw);
        }
        // Exact domain match (no subdomain) is allowed for youtube.com.
        guard_with("https://youtube.com/x", public).unwrap();
    }
}
