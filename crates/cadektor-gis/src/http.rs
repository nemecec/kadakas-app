//! A small HTTP client for map services.
//!
//! Every request has timeouts and a limit on the size of the answer. Redirects are followed by
//! hand (at most [`MAX_REDIRECTS`]), and each address is checked against the [`UrlPolicy`] again,
//! so an allowed server cannot send the client to a server that is not allowed.

use std::time::Duration;

use crate::{GisError, Result};

/// Redirects that the client follows.
pub const MAX_REDIRECTS: usize = 4;
/// Largest answer in bytes (64 MB).
pub const MAX_BODY: usize = 64 * 1024 * 1024;

/// An address split into its parts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedUrl {
    pub scheme: String,
    /// Lower case.
    pub host: String,
    pub port: u16,
    /// With the leading slash. Never empty.
    pub path: String,
    pub query: Option<String>,
}

impl ParsedUrl {
    /// `scheme://host:port`, without a default port.
    pub fn origin(&self) -> String {
        let default = if self.scheme == "https" { 443 } else { 80 };
        if self.port == default {
            format!("{}://{}", self.scheme, self.host)
        } else {
            format!("{}://{}:{}", self.scheme, self.host, self.port)
        }
    }
}

/// Splits an http or https address. Addresses with a user name or password are refused, because
/// documents and reports could show them.
pub fn parse_url(url: &str) -> Result<ParsedUrl> {
    let url = url.trim();
    let (scheme, rest) = url
        .split_once("://")
        .ok_or_else(|| GisError::Url(format!("not a web address: {url}")))?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err(GisError::Url(format!(
            "only http and https addresses are allowed, not {scheme}"
        )));
    }
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(end);
    if authority.contains('@') {
        return Err(GisError::Url(
            "an address with a user name or password is not accepted".into(),
        ));
    }
    if authority.contains('[') {
        return Err(GisError::Url(
            "addresses with an IPv6 number are not supported".into(),
        ));
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => {
            let port = p
                .parse::<u16>()
                .map_err(|_| GisError::Url(format!("bad port in {}", shorten_text(url))))?;
            (h, port)
        }
        None => (authority, if scheme == "https" { 443 } else { 80 }),
    };
    if host.is_empty() {
        return Err(GisError::Url(format!("no host name in {url}")));
    }
    let tail = tail.split('#').next().unwrap_or("");
    let (path, query) = match tail.split_once('?') {
        Some((p, q)) => (p, Some(q.to_string())),
        None => (tail, None),
    };
    Ok(ParsedUrl {
        scheme,
        host: host.to_ascii_lowercase(),
        port,
        path: if path.is_empty() { "/".into() } else { path.into() },
        query,
    })
}

/// Which addresses may be fetched.
///
/// An agent may only fetch addresses on the list that the owner of the app sets (`--allow-url`).
/// The default list is empty, so nothing is allowed. The user of the app is trusted with the
/// addresses they type in dialogs: the app uses [`UrlPolicy::unrestricted`] for those.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UrlPolicy {
    /// `None`: everything is allowed.
    list: Option<Vec<String>>,
}

impl UrlPolicy {
    pub fn unrestricted() -> Self {
        UrlPolicy { list: None }
    }

    /// Nothing is allowed.
    pub fn deny_all() -> Self {
        UrlPolicy {
            list: Some(Vec::new()),
        }
    }

    /// Entries are a host name (`maps.example.com`), a sub domain pattern (`*.example.com`) or an
    /// address prefix (`https://maps.example.com/wms/`). A prefix ends at a whole path part.
    pub fn allow_list<I: IntoIterator<Item = String>>(entries: I) -> Self {
        UrlPolicy {
            list: Some(
                entries
                    .into_iter()
                    .map(|e| e.trim().to_string())
                    .filter(|e| !e.is_empty())
                    .collect(),
            ),
        }
    }

    pub fn is_restricted(&self) -> bool {
        self.list.is_some()
    }

    /// The list, empty when everything is allowed or nothing.
    pub fn entries(&self) -> &[String] {
        self.list.as_deref().unwrap_or(&[])
    }

    /// Adds an entry (does nothing for an unrestricted policy).
    pub fn add(&mut self, entry: &str) {
        if let Some(l) = &mut self.list
            && !entry.trim().is_empty()
            && !l.iter().any(|e| e == entry.trim())
        {
            l.push(entry.trim().to_string());
        }
    }

    /// Fails with a message that tells the owner what to do.
    pub fn check(&self, url: &str) -> Result<()> {
        let u = parse_url(url)?;
        let Some(list) = &self.list else {
            return Ok(());
        };
        if list.iter().any(|e| entry_matches(e, &u)) {
            return Ok(());
        }
        Err(GisError::Url(format!(
            "{} is not on the list of allowed addresses. The owner of the app can allow it with --allow-url {}",
            u.origin(),
            u.host
        )))
    }
}

fn entry_matches(entry: &str, u: &ParsedUrl) -> bool {
    if entry.contains("://") {
        let Ok(e) = parse_url(entry) else {
            return false;
        };
        if e.scheme != u.scheme || e.host != u.host || e.port != u.port {
            return false;
        }
        let prefix = e.path.as_str();
        return u.path == prefix
            || (u.path.starts_with(prefix) && (prefix.ends_with('/') || u.path[prefix.len()..].starts_with('/')));
    }
    let host = entry.to_ascii_lowercase();
    match host.strip_prefix("*.") {
        Some(domain) => u.host.ends_with(&format!(".{domain}")),
        None => u.host == host,
    }
}

/// The answer of a server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    /// Lower case, without parameters (`image/png`).
    pub content_type: String,
    pub body: Vec<u8>,
}

/// Anything that can fetch an address. Tests use it to stand in for a server.
pub trait Fetch: Send + Sync {
    fn get(&self, url: &str) -> Result<Response>;
}

/// The HTTP client.
pub struct HttpClient {
    agent: ureq::Agent,
    policy: UrlPolicy,
    max_body: usize,
}

impl HttpClient {
    /// `timeout` limits the whole request, including the body.
    pub fn new(policy: UrlPolicy, timeout: Duration) -> HttpClient {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .timeout_connect(Some(timeout.min(Duration::from_secs(10))))
            .max_redirects(0)
            .http_status_as_error(false)
            .user_agent("Cadektor/0.1 (+map client)")
            .build()
            .into();
        HttpClient {
            agent,
            policy,
            max_body: MAX_BODY,
        }
    }

    /// Sets the largest answer in bytes.
    pub fn with_max_body(mut self, bytes: usize) -> Self {
        self.max_body = bytes;
        self
    }

    pub fn policy(&self) -> &UrlPolicy {
        &self.policy
    }
}

/// The address a `Location` header points to.
fn resolve(base: &ParsedUrl, location: &str) -> String {
    if location.contains("://") {
        location.to_string()
    } else if location.starts_with('/') {
        format!("{}{}", base.origin(), location)
    } else {
        let dir = base.path.rsplit_once('/').map_or("", |(d, _)| d);
        format!("{}{}/{}", base.origin(), dir, location)
    }
}

impl Fetch for HttpClient {
    fn get(&self, url: &str) -> Result<Response> {
        let mut url = url.trim().to_string();
        for _ in 0..=MAX_REDIRECTS {
            self.policy.check(&url)?;
            let mut resp = self
                .agent
                .get(&url)
                .call()
                .map_err(|e| GisError::Http(format!("{}: {}", shorten(&url), e)))?;
            let status = resp.status().as_u16();
            if matches!(status, 301 | 302 | 303 | 307 | 308) {
                let loc = resp
                    .headers()
                    .get("location")
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_string)
                    .ok_or_else(|| GisError::Http("redirect without a location".into()))?;
                url = resolve(&parse_url(&url)?, &loc);
                continue;
            }
            let content_type = resp
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .map(|v| {
                    v.split(';')
                        .next()
                        .unwrap_or("")
                        .trim()
                        .to_ascii_lowercase()
                })
                .unwrap_or_default();
            let body = resp
                .body_mut()
                .with_config()
                .limit(self.max_body as u64)
                .read_to_vec()
                .map_err(|e| GisError::Http(format!("{}: cannot read the answer: {e}", shorten(&url))))?;
            if !(200..300).contains(&status) {
                let hint: String = String::from_utf8_lossy(&body[..body.len().min(200)])
                    .chars()
                    .filter(|c| !c.is_control())
                    .collect();
                return Err(GisError::Http(format!(
                    "{}: HTTP {status} {hint}",
                    shorten(&url)
                )));
            }
            return Ok(Response {
                status,
                content_type,
                body,
            });
        }
        Err(GisError::Http(format!(
            "{}: too many redirects",
            shorten(&url)
        )))
    }
}

fn shorten_text(url: &str) -> &str {
    url.split(['?', '#']).next().unwrap_or("")
}

/// The address without its query, for messages (a query can hold a key).
pub fn shorten(url: &str) -> String {
    match parse_url(url) {
        Ok(u) => format!("{}{}", u.origin(), u.path),
        Err(_) => "address".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_addresses() {
        let u = parse_url("HTTPS://Maps.Example.com/wms/x?a=1&b=2#frag").unwrap();
        assert_eq!(u.scheme, "https");
        assert_eq!(u.host, "maps.example.com");
        assert_eq!(u.port, 443);
        assert_eq!(u.path, "/wms/x");
        assert_eq!(u.query.as_deref(), Some("a=1&b=2"));
        assert_eq!(u.origin(), "https://maps.example.com");
        let u = parse_url("http://127.0.0.1:8080").unwrap();
        assert_eq!((u.port, u.path.as_str()), (8080, "/"));
        assert_eq!(u.origin(), "http://127.0.0.1:8080");
        assert!(parse_url("ftp://x/").is_err());
        assert!(parse_url("file:///etc/passwd").is_err());
        assert!(parse_url("maps.example.com").is_err());
        assert!(parse_url("http://user:pw@x.com/").is_err());
        assert!(parse_url("http:///path").is_err());
        assert!(parse_url("http://x.com:99999/").is_err());
        assert!(parse_url("http://[::1]:80/").is_err());
    }

    #[test]
    fn policy_matches_hosts_patterns_and_prefixes() {
        let none = UrlPolicy::deny_all();
        assert!(none.is_restricted());
        assert!(none.check("https://a.com/").is_err());
        assert!(UrlPolicy::default().check("https://a.com/").is_ok());
        let free = UrlPolicy::unrestricted();
        assert!(!free.is_restricted() && free.check("http://a.com").is_ok());
        assert!(free.check("gopher://a.com").is_err());
        let mut p = UrlPolicy::allow_list([
            "maps.example.com".to_string(),
            "*.gov.fi".to_string(),
            "https://other.org/wms".to_string(),
            "".to_string(),
        ]);
        assert_eq!(p.entries().len(), 3);
        assert!(p.check("https://maps.example.com/any?x=1").is_ok());
        assert!(p.check("https://MAPS.example.com/").is_ok());
        assert!(p.check("https://evil.com/maps.example.com").is_err());
        assert!(p.check("https://www.gov.fi/x").is_ok());
        assert!(p.check("https://gov.fi/x").is_err());
        assert!(p.check("https://other.org/wms").is_ok());
        assert!(p.check("https://other.org/wms/sub").is_ok());
        assert!(p.check("https://other.org/wmsx").is_err());
        assert!(p.check("http://other.org/wms").is_err());
        assert!(p.check("https://other.org:444/wms").is_err());
        let e = p.check("https://nope.com/a").unwrap_err().to_string();
        assert!(e.contains("--allow-url nope.com"), "{e}");
        p.add("nope.com");
        p.add("nope.com");
        assert!(p.check("https://nope.com/a").is_ok());
        assert_eq!(p.entries().len(), 4);
        let mut free = UrlPolicy::unrestricted();
        free.add("x.com");
        assert!(!free.is_restricted());
        assert!(free.entries().is_empty());
        // A bad entry never matches.
        assert!(UrlPolicy::allow_list(["ftp://x.com/".into()]).check("https://x.com/").is_err());
    }

    #[test]
    fn resolves_redirects() {
        let base = parse_url("http://h.com:81/a/b/c?x=1").unwrap();
        assert_eq!(resolve(&base, "https://z.com/q"), "https://z.com/q");
        assert_eq!(resolve(&base, "/root"), "http://h.com:81/root");
        assert_eq!(resolve(&base, "d"), "http://h.com:81/a/b/d");
        assert_eq!(shorten("http://h.com/a?key=secret"), "http://h.com/a");
        assert_eq!(shorten("nonsense"), "address");
    }
}
