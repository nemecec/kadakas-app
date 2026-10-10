//! The address of a collaboration document.

use std::fmt;

/// Why an address is not good.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UrlError(pub String);

impl fmt::Display for UrlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for UrlError {}

/// A checked WebSocket address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CollabUrl {
    /// `ws` or `wss`.
    pub scheme: String,
    /// Lower case; an IPv6 address is in brackets.
    pub host: String,
    pub port: u16,
    /// With the leading `/`, without the query.
    pub path: String,
}

impl CollabUrl {
    /// Reads `ws://`, `wss://` and (as the same) `http://` and `https://`. An address with a user
    /// name or password, a query or a fragment is refused: a secret must not travel in an address.
    pub fn parse(url: &str) -> Result<CollabUrl, UrlError> {
        let url = url.trim();
        let (scheme, rest) = url
            .split_once("://")
            .ok_or_else(|| UrlError("the address needs a scheme: ws:// or wss://".into()))?;
        let scheme = match scheme.to_ascii_lowercase().as_str() {
            "ws" | "http" => "ws",
            "wss" | "https" => "wss",
            other => {
                return Err(UrlError(format!(
                    "the scheme {other}:// is not supported: use ws:// or wss://"
                )));
            }
        };
        if rest.contains('?') || rest.contains('#') {
            return Err(UrlError(
                "the address must not have a query or a fragment: give the token as the token"
                    .into(),
            ));
        }
        let (authority, path) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, "/"),
        };
        if authority.contains('@') {
            return Err(UrlError(
                "the address must not have a user name or password".into(),
            ));
        }
        let (host, port) = split_host_port(authority)?;
        if host.is_empty() {
            return Err(UrlError("the address has no host".into()));
        }
        let default_port = if scheme == "wss" { 443 } else { 80 };
        Ok(CollabUrl {
            scheme: scheme.to_string(),
            host: host.to_ascii_lowercase(),
            port: port.unwrap_or(default_port),
            path: path.to_string(),
        })
    }

    /// The host without brackets.
    pub fn bare_host(&self) -> &str {
        self.host.trim_start_matches('[').trim_end_matches(']')
    }

    /// True for `localhost`, `*.localhost`, `127.0.0.0/8` and `::1`.
    pub fn is_loopback(&self) -> bool {
        let h = self.bare_host();
        h == "localhost"
            || h.ends_with(".localhost")
            || h == "::1"
            || h.parse::<std::net::Ipv4Addr>()
                .is_ok_and(|a| a.is_loopback())
    }

    /// The address as text, without anything secret: `scheme://host:port/path`.
    pub fn display(&self) -> String {
        let default = if self.scheme == "wss" { 443 } else { 80 };
        if self.port == default {
            format!("{}://{}{}", self.scheme, self.host, self.path)
        } else {
            format!("{}://{}:{}{}", self.scheme, self.host, self.port, self.path)
        }
    }

    /// True if an entry of an allow list matches this address. An entry is a host name
    /// (`collab.example.org`), a pattern for sub domains (`*.example.org`) or an address prefix
    /// (`wss://collab.example.org/ws/`); a prefix ends at a whole path part. This is the same
    /// form as the entries of `--allow-url`.
    pub fn matches_entry(&self, entry: &str) -> bool {
        let entry = entry.trim();
        if entry.contains("://") {
            let Ok(e) = CollabUrl::parse(entry) else {
                return false;
            };
            if e.scheme != self.scheme || e.host != self.host || e.port != self.port {
                return false;
            }
            let prefix = e.path.as_str();
            return self.path == prefix
                || (self.path.starts_with(prefix)
                    && (prefix.ends_with('/') || self.path[prefix.len()..].starts_with('/')));
        }
        let entry = entry.to_ascii_lowercase();
        match entry.strip_prefix("*.") {
            Some(domain) => self.bare_host().ends_with(&format!(".{domain}")),
            None => self.bare_host() == entry,
        }
    }

    /// Adds `/ws/<doc>` to an address that has only a base path.
    pub fn with_document(mut self, doc: &str) -> CollabUrl {
        let base = self.path.trim_end_matches('/');
        self.path = format!("{base}/ws/{doc}");
        self
    }
}

fn split_host_port(authority: &str) -> Result<(&str, Option<u16>), UrlError> {
    let bad = || UrlError("the port is not a number".into());
    if authority.starts_with('[') {
        let end = authority
            .find(']')
            .ok_or_else(|| UrlError("bad IPv6 address".into()))?;
        let host = &authority[..=end];
        return match authority[end + 1..].strip_prefix(':') {
            Some(p) => Ok((host, Some(p.parse().map_err(|_| bad())?))),
            None if authority.len() == end + 1 => Ok((host, None)),
            None => Err(UrlError("bad IPv6 address".into())),
        };
    }
    match authority.rsplit_once(':') {
        Some((h, p)) => Ok((h, Some(p.parse().map_err(|_| bad())?))),
        None => Ok((authority, None)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_checks() {
        let u = CollabUrl::parse("ws://127.0.0.1:8787/ws/abc").unwrap();
        assert_eq!(
            (u.scheme.as_str(), u.port, u.path.as_str()),
            ("ws", 8787, "/ws/abc")
        );
        assert!(u.is_loopback());
        let u = CollabUrl::parse("WSS://Collab.Example.org").unwrap();
        assert_eq!(
            (u.host.as_str(), u.port, u.path.as_str()),
            ("collab.example.org", 443, "/")
        );
        assert!(!u.is_loopback());
        assert_eq!(u.display(), "wss://collab.example.org/");
        assert!(CollabUrl::parse("ws://[::1]:9/ws/x").unwrap().is_loopback());
        assert_eq!(CollabUrl::parse("https://a.b/x").unwrap().scheme, "wss");
    }

    #[test]
    fn refuses_secrets_and_bad_schemes() {
        for bad in [
            "ws://user:pw@host/ws/x",
            "ws://host/ws/x?token=abc",
            "ws://host/ws/x#a",
            "ftp://host/",
            "host/ws/x",
            "ws://host:port/",
            "ws:///x",
        ] {
            assert!(CollabUrl::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn allow_entries() {
        let u = CollabUrl::parse("wss://collab.example.org/ws/abc").unwrap();
        for yes in [
            "collab.example.org",
            "COLLAB.example.org",
            "*.example.org",
            "wss://collab.example.org/",
            "wss://collab.example.org/ws",
            "https://collab.example.org/ws/abc",
        ] {
            assert!(u.matches_entry(yes), "{yes}");
        }
        for no in [
            "example.org",
            "*.other.org",
            "ws://collab.example.org/",
            "wss://collab.example.org:444/",
            "wss://collab.example.org/w",
            "",
        ] {
            assert!(!u.matches_entry(no), "{no}");
        }
    }

    #[test]
    fn adds_the_document() {
        let u = CollabUrl::parse("ws://h:1/").unwrap().with_document("D");
        assert_eq!(u.path, "/ws/D");
        let u = CollabUrl::parse("wss://h/base/")
            .unwrap()
            .with_document("D");
        assert_eq!(u.path, "/base/ws/D");
    }
}
