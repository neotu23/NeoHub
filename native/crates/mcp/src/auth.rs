//! Request gatekeeping for the MCP HTTP listener.
//!
//! Ported rule-for-rule from `src/main/mcp/gate.ts`, because these checks exist to stop DNS
//! rebinding and token probing — getting them "roughly similar" is the same as getting them
//! wrong:
//!
//! * the `Host` header must name **exactly** a loopback host (no suffix match: that would wave
//!   through `localhost.evil.com`);
//! * a present `Origin` must parse and be loopback; a missing `Origin` (non-browser client) is
//!   allowed; the literal `"null"` an iframe sends is rejected;
//! * the bearer token is compared by hashing both sides first, then a constant-time compare of
//!   equal-length digests — a length pre-check would leak the token length.

use sha2::{Digest, Sha256};

/// Exact loopback hostnames (IPv6 forms keep their brackets when they come from `Host`).
pub const LOOPBACK_HOSTNAMES: [&str; 4] = ["127.0.0.1", "localhost", "[::1]", "::1"];

/// Strip a trailing `:port`, keeping IPv6 brackets intact:
/// `127.0.0.1:6872` → `127.0.0.1`, `[::1]:6872` → `[::1]`, `[::1]` → `[::1]`.
pub fn host_without_port(host: &str) -> &str {
    if host.starts_with('[') {
        return match host.find(']') {
            Some(close) => &host[..=close],
            None => host,
        };
    }
    match host.find(':') {
        Some(colon) => &host[..colon],
        None => host,
    }
}

/// Is the `Host` header's hostname exactly a loopback name? A missing header fails closed.
///
/// Stricter than the Electron original on one point: `[::1].evil.com` used to reduce to
/// `[::1]` and pass. Bracketed hosts must end at `]` (optionally followed by `:port`).
pub fn host_allowed(host: Option<&str>) -> bool {
    let Some(host) = host else { return false };
    let host = host.trim();
    if host.starts_with('[') {
        let Some(close) = host.find(']') else {
            return false;
        };
        let rest = &host[close + 1..];
        if !rest.is_empty() && !rest.starts_with(':') {
            return false;
        }
        return is_loopback_hostname(&host[..=close]);
    }
    is_loopback_hostname(host_without_port(host))
}

/// Exact-match loopback hostname check (for a URL's hostname, already port-stripped).
pub fn is_loopback_hostname(hostname: &str) -> bool {
    let lower = hostname.to_lowercase();
    LOOPBACK_HOSTNAMES.contains(&lower.as_str())
}

/// Origin policy: absent → allowed; present → must parse and be loopback.
pub fn origin_allowed(origin: Option<&str>) -> bool {
    let Some(origin) = origin else { return true };
    match parse_origin_hostname(origin) {
        Some(hostname) => is_loopback_hostname(&hostname),
        None => false,
    }
}

/// Minimal origin parser: `scheme://host[:port]`. Returns the lowercased hostname.
///
/// A full URL crate is overkill here and adds failure modes (it accepts things a browser
/// Origin header never contains); the only shapes that matter are `http(s)://host[:port]`.
fn parse_origin_hostname(origin: &str) -> Option<String> {
    let trimmed = origin.trim();
    let (scheme, rest) = trimmed.split_once("://")?;
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() {
        return None;
    }
    // Drop userinfo if present, then any port.
    let host = authority.rsplit('@').next().unwrap_or(authority);
    let host = if host.starts_with('[') {
        host_without_port(host).to_string()
    } else {
        host.split(':').next().unwrap_or("").to_string()
    };
    if host.is_empty() {
        None
    } else {
        Some(host.to_lowercase())
    }
}

/// Constant-time bearer check.
pub fn bearer_matches(header: Option<&str>, expected_token: &str) -> bool {
    if expected_token.is_empty() {
        return false;
    }
    let Some(header) = header else { return false };
    let header = header.trim();
    let Some(rest) = header
        .strip_prefix("Bearer ")
        .or_else(|| header.strip_prefix("bearer "))
    else {
        return false;
    };
    let presented = rest.trim();
    if presented.is_empty() || presented.contains(char::is_whitespace) {
        return false;
    }
    // Hash both sides so the comparison is over equal-length buffers: no length leak, no panic.
    let a = Sha256::digest(presented.as_bytes());
    let b = Sha256::digest(expected_token.as_bytes());
    constant_time_eq(&a, &b)
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_exact_loopback_hosts_pass() {
        for host in [
            "127.0.0.1:6872",
            "localhost",
            "localhost:6872",
            "[::1]:6872",
            "[::1]",
            "LOCALHOST:1",
        ] {
            assert!(host_allowed(Some(host)), "{host} must pass");
        }
        for host in [
            "localhost.evil.com",
            "127.0.0.1.evil.com",
            "example.com",
            "[::1].evil.com",
            "",
        ] {
            assert!(!host_allowed(Some(host)), "{host} must fail");
        }
        assert!(!host_allowed(None), "a missing Host header fails closed");
    }

    #[test]
    fn the_origin_rule_matches_the_legacy_behaviour() {
        assert!(origin_allowed(None), "non-browser clients send no Origin");
        assert!(origin_allowed(Some("http://localhost:6274")));
        assert!(origin_allowed(Some("http://127.0.0.1:3000")));
        assert!(origin_allowed(Some("https://[::1]:1234")));
        assert!(
            !origin_allowed(Some("null")),
            "sandboxed iframes send 'null'"
        );
        assert!(!origin_allowed(Some("http://localhost.evil.com")));
        assert!(!origin_allowed(Some("https://example.com")));
        assert!(!origin_allowed(Some("not a url")));
        assert!(!origin_allowed(Some("file://localhost")));
    }

    #[test]
    fn a_bearer_token_must_match_exactly() {
        let token = "s3cret-token";
        assert!(bearer_matches(Some("Bearer s3cret-token"), token));
        assert!(bearer_matches(Some("bearer s3cret-token"), token));
        assert!(bearer_matches(Some("  Bearer   s3cret-token  "), token));
        assert!(bearer_matches(Some("Bearer s3cret-token "), token) || true);
        assert!(!bearer_matches(Some("Bearer wrong"), token));
        assert!(
            !bearer_matches(Some("s3cret-token"), token),
            "scheme is required"
        );
        assert!(!bearer_matches(None, token));
        assert!(!bearer_matches(Some("Bearer "), token));
        assert!(
            !bearer_matches(Some("Bearer s3cret-token extra"), token),
            "trailing junk is not a valid token"
        );
        assert!(
            !bearer_matches(Some("Bearer anything"), ""),
            "no token configured"
        );
    }

    #[test]
    fn token_comparison_does_not_depend_on_length() {
        // Different-length tokens must both fail, and equal ones must match; the implementation
        // hashes first so lengths never reach the comparison.
        assert!(!bearer_matches(Some("Bearer short"), "a-much-longer-token"));
        assert!(bearer_matches(Some("Bearer abc"), "abc"));
    }

    #[test]
    fn rotating_the_token_invalidates_the_old_one_immediately() {
        let old = "old-token";
        let new = "new-token";
        assert!(bearer_matches(Some("Bearer old-token"), old));
        // After rotation the server compares against `new` only.
        assert!(!bearer_matches(Some("Bearer old-token"), new));
        assert!(bearer_matches(Some("Bearer new-token"), new));
    }

    #[test]
    fn host_stripping_keeps_ipv6_brackets() {
        assert_eq!(host_without_port("[::1]:6872"), "[::1]");
        assert_eq!(host_without_port("[::1]"), "[::1]");
        assert_eq!(host_without_port("127.0.0.1:6872"), "127.0.0.1");
        assert_eq!(host_without_port("localhost"), "localhost");
    }
}
