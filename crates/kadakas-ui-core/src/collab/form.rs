//! The Connect dialog: its state, its checks, the call it makes, and the browser fragment.

use kadakas_core::uid::Uid;
use serde_json::{Value, json};

/// What the program knows before the person types: the default server, whether a default
/// token exists, and a name for the person.
///
/// The default token itself is never copied here. It stays in the API session, which uses it
/// only for the default URL. `user` is the user ID that the default token names (see
/// [`token_user`]); the server wants `hello.user` to be that ID.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Defaults {
    pub url: Option<String>,
    pub has_token: bool,
    pub user: Option<String>,
    pub name: String,
}

/// Decodes base64url text (with or without `=`), as a JWT uses.
fn base64url(text: &str) -> Option<Vec<u8>> {
    let (mut out, mut acc, mut bits) = (Vec::new(), 0u32, 0u32);
    for c in text.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' | b'+' => 62,
            b'_' | b'/' => 63,
            b'=' => break,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// The claims of a token (a JWT). This reads them and does not check the signature: the server
/// does that.
fn token_claims(token: &str) -> Option<Value> {
    let payload = token.trim().split('.').nth(1)?;
    serde_json::from_slice(&base64url(payload)?).ok()
}

/// The user ID in the `sub` claim of a token (a JWT), if the token has one. The server signs in
/// a client only if `hello.user` is that ID, so the window passes it as `user`.
pub fn token_user(token: &str) -> Option<String> {
    let claims = token_claims(token)?;
    let sub = claims.get("sub")?.as_str()?;
    Uid::parse(sub).map(|_| sub.to_string())
}

/// True if the token (a JWT) has an `exp` claim in the past. A browser cannot see why a
/// WebSocket was refused (an old token looks like a lost network), so the window checks the expiry
/// itself before it connects. A token without a readable `exp` is not called expired.
pub fn token_expired(token: &str, now_secs: u64) -> bool {
    token_claims(token)
        .and_then(|c| c.get("exp").and_then(Value::as_u64))
        .is_some_and(|exp| exp <= now_secs)
}

/// Why the form cannot make a call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormError {
    UrlMissing,
    /// A secret in the address: a user name, a password, a query or a fragment.
    SecretInUrl,
    DocMissing,
    DocInvalid,
}

impl FormError {
    /// The message id of the text for the person.
    pub fn message_id(self) -> &'static str {
        match self {
            FormError::UrlMissing => kadakas_l10n::msg!("uic-collab-form-url-missing"),
            FormError::SecretInUrl => kadakas_l10n::msg!("uic-collab-form-secret-in-url"),
            FormError::DocMissing => kadakas_l10n::msg!("uic-collab-form-doc-missing"),
            FormError::DocInvalid => kadakas_l10n::msg!("uic-collab-form-doc-invalid"),
        }
    }

    pub fn text(self) -> String {
        kadakas_l10n::translate(self.message_id(), &[])
    }
}

/// The call to `collab.connect` that a valid form makes. The token is in memory only; the
/// `Debug` text hides it.
#[derive(Clone, PartialEq, Eq)]
pub struct ConnectRequest {
    pub url: Option<String>,
    pub doc: Option<String>,
    pub token: Option<String>,
    /// The user ID of the token.
    pub user: Option<String>,
    pub name: Option<String>,
}

impl std::fmt::Debug for ConnectRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectRequest")
            .field("url", &self.url)
            .field("doc", &self.doc)
            .field("token", &self.token.as_ref().map(|_| "<hidden>"))
            .field("user", &self.user)
            .field("name", &self.name)
            .finish()
    }
}

impl ConnectRequest {
    /// The parameters of `collab.connect`. `replace` and `discard` are true only after the
    /// person confirmed.
    pub fn params(&self, now_ms: u64, replace: bool, discard: bool) -> Value {
        let mut p = json!({"now_ms": now_ms});
        let m = p.as_object_mut().expect("an object");
        for (key, v) in [
            ("url", &self.url),
            ("doc", &self.doc),
            ("token", &self.token),
            ("user", &self.user),
            ("name", &self.name),
        ] {
            if let Some(v) = v {
                m.insert(key.into(), json!(v));
            }
        }
        if replace {
            m.insert("replace".into(), json!(true));
        }
        if discard {
            m.insert("discard".into(), json!(true));
        }
        p
    }
}

/// The state of the Connect dialog.
#[derive(Clone, PartialEq, Eq)]
pub struct ConnectForm {
    pub url: String,
    pub doc: String,
    /// What the person typed. Empty: use the default token (it applies to the default URL only).
    pub token: String,
    pub name: String,
    defaults: Defaults,
}

impl std::fmt::Debug for ConnectForm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectForm")
            .field("url", &self.url)
            .field("doc", &self.doc)
            .field("token", &(!self.token.is_empty()).then_some("<hidden>"))
            .field("name", &self.name)
            .finish()
    }
}

/// `scheme://host:port` of an address, in lower case, with `http` read as `ws`.
fn endpoint_of(url: &str) -> Option<String> {
    let (scheme, rest) = url.trim().split_once("://")?;
    let scheme = match scheme.to_ascii_lowercase().as_str() {
        "ws" | "http" => "ws",
        "wss" | "https" => "wss",
        _ => return None,
    };
    let authority = rest.split(['/', '?', '#']).next()?;
    (!authority.is_empty()).then(|| format!("{scheme}://{}", authority.to_ascii_lowercase()))
}

/// The document ID in a full address `.../ws/<UUID>`, if there is one.
fn doc_in_url(url: &str) -> Option<&str> {
    let path = url.split(['?', '#']).next()?;
    let (_, tail) = path.rsplit_once("/ws/")?;
    let tail = tail.trim_end_matches('/');
    Uid::parse(tail).map(|_| tail)
}

impl ConnectForm {
    pub fn new(defaults: Defaults) -> ConnectForm {
        let mut f = ConnectForm {
            url: String::new(),
            doc: String::new(),
            token: String::new(),
            name: defaults.name.clone(),
            defaults,
        };
        if let Some(u) = f.defaults.url.clone() {
            f.set_url(&u);
        }
        f
    }

    /// A form filled from the address fragment of the browser. The fragment wins over the
    /// defaults.
    pub fn from_fragment(defaults: Defaults, fragment: &Fragment) -> ConnectForm {
        let mut f = ConnectForm::new(defaults);
        f.set_url(&fragment.url);
        if let Some(d) = &fragment.doc {
            f.doc = d.clone();
        }
        if let Some(t) = &fragment.token {
            f.token = t.clone();
        }
        if let Some(n) = &fragment.name {
            f.name = n.clone();
        }
        f
    }

    /// Sets the address. A full address with the document ID fills the document field too.
    fn set_url(&mut self, url: &str) {
        if let Some(d) = doc_in_url(url) {
            self.doc = d.to_string();
        }
        self.url = url.trim().to_string();
    }

    pub fn defaults(&self) -> &Defaults {
        &self.defaults
    }

    /// True if the default token will be used: there is one, the field is empty, and the
    /// address is the default one (or empty, which means the default). The API makes the final
    /// decision; this is for a hint.
    pub fn default_token_applies(&self) -> bool {
        if !self.defaults.has_token || !self.token.is_empty() {
            return false;
        }
        let url = self.url.trim();
        if url.is_empty() {
            return true;
        }
        match (
            endpoint_of(url),
            self.defaults.url.as_deref().and_then(endpoint_of),
        ) {
            (Some(a), Some(b)) => a == b,
            _ => false,
        }
    }

    /// Wipes the token field (after a call, so the secret does not stay in the dialog).
    pub fn clear_secret(&mut self) {
        // Overwrite first, so the old text does not stay in the freed memory as long.
        self.token.replace_range(.., &"\0".repeat(self.token.len()));
        self.token.clear();
    }

    /// Checks the form and makes the call.
    pub fn request(&self) -> Result<ConnectRequest, FormError> {
        let url = self.url.trim();
        let default_url = self
            .defaults
            .url
            .as_deref()
            .filter(|u| !u.trim().is_empty());
        if url.is_empty() && default_url.is_none() {
            return Err(FormError::UrlMissing);
        }
        if url.contains(['?', '#']) || has_userinfo(url) {
            return Err(FormError::SecretInUrl);
        }
        let doc = self.doc.trim();
        let doc_known = doc_in_url(url).is_some()
            || (url.is_empty() && default_url.and_then(doc_in_url).is_some());
        let doc = if doc.is_empty() {
            if !doc_known {
                return Err(FormError::DocMissing);
            }
            None
        } else {
            if Uid::parse(doc).is_none() {
                return Err(FormError::DocInvalid);
            }
            Some(doc.to_string())
        };
        let some = |t: &str| (!t.trim().is_empty()).then(|| t.trim().to_string());
        // The user of the token the person typed, else of the default token (if it applies).
        let user = token_user(&self.token).or_else(|| {
            self.default_token_applies()
                .then(|| self.defaults.user.clone())
                .flatten()
        });
        Ok(ConnectRequest {
            url: some(url),
            doc,
            token: some(&self.token),
            user,
            name: some(&self.name),
        })
    }
}

/// True if the address has a user name or a password (`scheme://user:pw@host`).
fn has_userinfo(url: &str) -> bool {
    url.split_once("://")
        .map(|(_, rest)| rest.split('/').next().unwrap_or(""))
        .is_some_and(|authority| authority.contains('@'))
}

/// How the app answers an error of `collab.connect`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConnectIssue {
    /// The open document has changes that nobody saved. Ask the person, then pass `replace`.
    NeedsReplace,
    /// Edits wait for another document or user. Ask the person, then pass `discard`.
    NeedsDiscard,
    /// The session is connected already.
    AlreadyConnected,
    /// Any other error; the text is the English text of the API.
    Failed(String),
}

/// Reads an error of the API by its stable `reason`, never by its text.
pub fn classify_connect_error(reason: Option<&str>, message: &str) -> ConnectIssue {
    match reason {
        Some("unsaved_changes") => ConnectIssue::NeedsReplace,
        Some("edits_wait") => ConnectIssue::NeedsDiscard,
        Some("already_connected") => ConnectIssue::AlreadyConnected,
        _ => ConnectIssue::Failed(message.to_string()),
    }
}

/// What the address fragment of the browser holds: `#collab=<ws url>&doc=<id>&token=<t>`,
/// and optionally `&name=<n>`. A fragment never goes to a server.
#[derive(Clone, PartialEq, Eq)]
pub struct Fragment {
    pub url: String,
    pub doc: Option<String>,
    pub token: Option<String>,
    pub name: Option<String>,
}

impl std::fmt::Debug for Fragment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Fragment")
            .field("url", &self.url)
            .field("doc", &self.doc)
            .field("token", &self.token.as_ref().map(|_| "<hidden>"))
            .field("name", &self.name)
            .finish()
    }
}

/// The longest fragment that is read.
const MAX_FRAGMENT: usize = 16 * 1024;

fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

impl Fragment {
    /// Reads the fragment (with or without the leading `#`). `None` if there is no `collab`
    /// key or the text is not valid.
    pub fn parse(fragment: &str) -> Option<Fragment> {
        let text = fragment.strip_prefix('#').unwrap_or(fragment);
        if text.len() > MAX_FRAGMENT {
            return None;
        }
        let (mut url, mut doc, mut token, mut name) = (None, None, None, None);
        for pair in text.split('&') {
            let Some((key, value)) = pair.split_once('=') else {
                continue;
            };
            let Some(value) = percent_decode(value).filter(|v| !v.trim().is_empty()) else {
                continue;
            };
            match key {
                "collab" => url = Some(value),
                "doc" => doc = Some(value),
                "token" => token = Some(value),
                "name" => name = Some(value),
                _ => {}
            }
        }
        Some(Fragment {
            url: url?,
            doc,
            token,
            name,
        })
    }

    /// The fragment text without the token pair, to put back in the address bar. Empty if
    /// nothing else is left. Other keys stay as they were.
    pub fn without_token(fragment: &str) -> String {
        let text = fragment.strip_prefix('#').unwrap_or(fragment);
        let rest: Vec<&str> = text
            .split('&')
            .filter(|pair| !pair.is_empty() && pair.split('=').next() != Some("token"))
            .collect();
        if rest.is_empty() {
            String::new()
        } else {
            format!("#{}", rest.join("&"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "01939a2e-7c1d-7a2b-9d3e-0123456789ab";

    const JWT: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIwMTkwZjFjMi03YTNiLTdjMWQtOGUyZi0xMjM0NTY3ODlhYmMiLCJkb2MiOiJ4In0.c2ln";
    const JWT_USER: &str = "0190f1c2-7a3b-7c1d-8e2f-123456789abc";
    const JWT_WITHOUT_SUB: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJuYW1lIjoieCJ9.c2ln";

    fn defaults(url: Option<&str>, has_token: bool) -> Defaults {
        Defaults {
            url: url.map(str::to_string),
            has_token,
            user: None,
            name: "Ann".into(),
        }
    }

    #[test]
    fn a_new_form_takes_the_defaults() {
        let f = ConnectForm::new(defaults(Some("ws://127.0.0.1:8787"), true));
        assert_eq!(f.url, "ws://127.0.0.1:8787");
        assert_eq!(f.name, "Ann");
        assert!(f.doc.is_empty() && f.token.is_empty());
        assert!(f.default_token_applies());
        assert_eq!(f.defaults().url.as_deref(), Some("ws://127.0.0.1:8787"));
        // A default address with the document in it fills the document field.
        let f = ConnectForm::new(defaults(Some(&format!("ws://h:1/ws/{DOC}")), false));
        assert_eq!(f.doc, DOC);
    }

    #[test]
    fn the_default_token_applies_to_the_default_address_only() {
        let mut f = ConnectForm::new(defaults(Some("ws://127.0.0.1:8787"), true));
        f.url = "WS://127.0.0.1:8787/ws/x".into();
        assert!(f.default_token_applies());
        f.url = "http://127.0.0.1:8787".into();
        assert!(f.default_token_applies(), "http reads as ws");
        f.url = "ws://127.0.0.1:9999".into();
        assert!(!f.default_token_applies());
        f.url = "ftp://127.0.0.1:8787".into();
        assert!(!f.default_token_applies());
        f.url = String::new();
        assert!(f.default_token_applies());
        f.token = "mine".into();
        assert!(!f.default_token_applies());
        let f = ConnectForm::new(defaults(None, true));
        assert!(
            f.default_token_applies(),
            "an empty address means the default"
        );
        let f = ConnectForm::new(defaults(Some("ws://h"), false));
        assert!(!f.default_token_applies());
        let mut f = ConnectForm::new(defaults(None, true));
        f.url = "ws://h:1".into();
        assert!(!f.default_token_applies(), "no default address to compare");
    }

    #[test]
    fn the_checks_find_each_mistake() {
        let mut f = ConnectForm::new(defaults(None, false));
        assert_eq!(f.request().unwrap_err(), FormError::UrlMissing);
        f.url = "ws://127.0.0.1:8787".into();
        assert_eq!(f.request().unwrap_err(), FormError::DocMissing);
        f.doc = "not-a-uuid".into();
        assert_eq!(f.request().unwrap_err(), FormError::DocInvalid);
        f.doc = DOC.into();
        assert!(f.request().is_ok());
        for bad in [
            "ws://u:pw@host/",
            "ws://u@host/",
            "ws://host/?token=1",
            "ws://host/#x",
        ] {
            f.url = bad.into();
            assert_eq!(f.request().unwrap_err(), FormError::SecretInUrl, "{bad}");
        }
        for e in [
            FormError::UrlMissing,
            FormError::SecretInUrl,
            FormError::DocMissing,
            FormError::DocInvalid,
        ] {
            assert!(!e.text().is_empty());
            assert_ne!(e.text(), e.message_id(), "the message exists");
        }
    }

    #[test]
    fn a_full_address_needs_no_document_field() {
        let mut f = ConnectForm::new(defaults(None, false));
        f.url = format!("wss://collab.example.org/ws/{DOC}");
        assert_eq!(f.request().unwrap().doc, None);
        // An address with a path that is not a document is not enough.
        f.url = "wss://collab.example.org/ws/abc".into();
        assert_eq!(f.request().unwrap_err(), FormError::DocMissing);
        // An empty address and a default one with the document.
        let mut f = ConnectForm::new(defaults(Some(&format!("ws://h:1/ws/{DOC}")), false));
        f.url.clear();
        f.doc.clear();
        let r = f.request().unwrap();
        assert_eq!((r.url, r.doc), (None, None));
    }

    #[test]
    fn the_request_makes_the_parameters() {
        let mut f = ConnectForm::new(defaults(Some("ws://127.0.0.1:8787"), true));
        f.doc = format!(" {DOC} ");
        f.name = "  ".into();
        let r = f.request().unwrap();
        assert_eq!(r.token, None, "the default token is not copied");
        assert_eq!(r.name, None);
        assert_eq!(
            r.params(7, false, false),
            json!({"now_ms": 7, "url": "ws://127.0.0.1:8787", "doc": DOC})
        );
        f.token = " tok ".into();
        f.name = "Ann".into();
        let r = f.request().unwrap();
        let p = r.params(7, true, true);
        assert_eq!(p["token"], "tok");
        assert_eq!(p["name"], "Ann");
        assert_eq!(p["replace"], true);
        assert_eq!(p["discard"], true);
        assert!(r.params(7, false, false).get("replace").is_none());
    }

    #[test]
    fn a_token_never_shows_in_a_debug_text() {
        let mut f = ConnectForm::new(defaults(Some("ws://h:1"), false));
        f.doc = DOC.into();
        f.token = "secret-token".into();
        let r = f.request().unwrap();
        let frag = Fragment::parse("#collab=ws%3A%2F%2Fh&token=secret-token").unwrap();
        for text in [format!("{f:?}"), format!("{r:?}"), format!("{frag:?}")] {
            assert!(!text.contains("secret-token"), "{text}");
            assert!(text.contains("hidden"), "{text}");
        }
        f.token.clear();
        assert!(!format!("{f:?}").contains("hidden"));
    }

    #[test]
    fn the_user_of_a_token_is_read_from_its_claims() {
        assert_eq!(token_user(JWT).as_deref(), Some(JWT_USER));
        assert_eq!(token_user(&format!("  {JWT}\n")).as_deref(), Some(JWT_USER));
        for bad in [
            "",
            "abc",
            "a.b",
            "a.!!!.c",
            JWT_WITHOUT_SUB,
            // `sub` is not a UUID: {"sub":"x"}
            "e30.eyJzdWIiOiJ4In0.c2ln",
            // not JSON: "hello"
            "e30.aGVsbG8.c2ln",
        ] {
            assert_eq!(token_user(bad), None, "{bad}");
        }
        // Padding and the plain alphabet are fine.
        assert_eq!(base64url("aGVsbG8=").unwrap(), b"hello");
        assert_eq!(base64url("+/+/").unwrap(), [0xfb, 0xff, 0xbf]);
        assert_eq!(base64url("-_-_").unwrap(), [0xfb, 0xff, 0xbf]);
    }

    #[test]
    fn an_old_token_is_called_expired() {
        // {"exp":1000}
        let token = "e30.eyJleHAiOjEwMDB9.c2ln";
        assert!(!token_expired(token, 999));
        assert!(token_expired(token, 1000));
        assert!(token_expired(&format!(" {token} "), 5000));
        // No exp claim, no JWT, empty: not expired.
        for t in [JWT, JWT_WITHOUT_SUB, "opaque", "", "a.b.c"] {
            assert!(!token_expired(t, u64::MAX), "{t}");
        }
    }

    #[test]
    fn the_request_names_the_user_of_the_token() {
        let mut f = ConnectForm::new(defaults(Some("ws://127.0.0.1:8787"), false));
        f.doc = DOC.into();
        // A token that the person typed.
        f.token = JWT.into();
        assert_eq!(f.request().unwrap().user.as_deref(), Some(JWT_USER));
        assert_eq!(
            f.request().unwrap().params(1, false, false)["user"],
            JWT_USER
        );
        // A token without a user claim gives none.
        f.token = "opaque".into();
        assert_eq!(f.request().unwrap().user, None);
        // The default token: its user comes with the defaults.
        let d = Defaults {
            url: Some("ws://127.0.0.1:8787".into()),
            has_token: true,
            user: Some(JWT_USER.into()),
            name: String::new(),
        };
        let mut f = ConnectForm::new(d);
        f.doc = DOC.into();
        assert_eq!(f.request().unwrap().user.as_deref(), Some(JWT_USER));
        // ... only for the default address.
        f.url = "ws://127.0.0.1:9999".into();
        assert_eq!(f.request().unwrap().user, None);
    }

    #[test]
    fn the_secret_is_wiped() {
        let mut f = ConnectForm::new(defaults(None, false));
        f.token = "abc".into();
        f.clear_secret();
        assert!(f.token.is_empty());
        f.clear_secret();
    }

    #[test]
    fn connect_errors_are_read_by_reason() {
        use ConnectIssue::*;
        assert_eq!(
            classify_connect_error(Some("unsaved_changes"), "x"),
            NeedsReplace
        );
        assert_eq!(
            classify_connect_error(Some("edits_wait"), "x"),
            NeedsDiscard
        );
        assert_eq!(
            classify_connect_error(Some("already_connected"), "x"),
            AlreadyConnected
        );
        assert_eq!(
            classify_connect_error(None, "no way"),
            Failed("no way".into())
        );
        assert_eq!(
            classify_connect_error(Some("other"), "m"),
            Failed("m".into())
        );
    }

    #[test]
    fn the_fragment_is_read() {
        let f = Fragment::parse(&format!(
            "#collab=ws%3A%2F%2F127.0.0.1%3A8787&doc={DOC}&token=a.b-c_d&name=Ann%20Lee"
        ))
        .unwrap();
        assert_eq!(f.url, "ws://127.0.0.1:8787");
        assert_eq!(f.doc.as_deref(), Some(DOC));
        assert_eq!(f.token.as_deref(), Some("a.b-c_d"));
        assert_eq!(f.name.as_deref(), Some("Ann Lee"));
        // Plain characters, no leading hash, an unknown key.
        let f = Fragment::parse("collab=ws://h:1&x=1&junk").unwrap();
        assert_eq!(f.url, "ws://h:1");
        assert!(f.doc.is_none() && f.token.is_none() && f.name.is_none());
    }

    #[test]
    fn a_bad_fragment_gives_nothing() {
        for bad in [
            "",
            "#",
            "#doc=1",
            "#collab=",
            "#collab=%zz",
            "#collab=%f0%28",
            "#collab=%4",
        ] {
            assert_eq!(Fragment::parse(bad), None, "{bad:?}");
        }
        let long = format!("#collab={}", "a".repeat(MAX_FRAGMENT));
        assert_eq!(Fragment::parse(&long), None);
        // A value that is not valid is skipped, the rest is kept.
        let f = Fragment::parse("#collab=ws://h&doc=%zz&token=").unwrap();
        assert!(f.doc.is_none() && f.token.is_none());
    }

    #[test]
    fn the_fragment_fills_the_form() {
        let frag = Fragment::parse(&format!("#collab=ws://h:1&doc={DOC}&token=t&name=Bo")).unwrap();
        let f = ConnectForm::from_fragment(defaults(Some("ws://other:2"), true), &frag);
        assert_eq!((f.url.as_str(), f.doc.as_str()), ("ws://h:1", DOC));
        assert_eq!((f.token.as_str(), f.name.as_str()), ("t", "Bo"));
        let frag = Fragment::parse(&format!("#collab=ws://h:1/ws/{DOC}")).unwrap();
        let f = ConnectForm::from_fragment(defaults(None, false), &frag);
        assert_eq!(f.doc, DOC);
        assert_eq!(f.name, "Ann");
    }

    #[test]
    fn the_token_leaves_the_address_bar_text() {
        assert_eq!(
            Fragment::without_token("#collab=ws%3A%2F%2Fh&doc=1&token=abc&name=x"),
            "#collab=ws%3A%2F%2Fh&doc=1&name=x"
        );
        assert_eq!(Fragment::without_token("#token=abc"), "");
        assert_eq!(Fragment::without_token("token=abc&a=1"), "#a=1");
        assert_eq!(Fragment::without_token(""), "");
        // A key that only starts with "token" stays.
        assert_eq!(Fragment::without_token("#tokens=1"), "#tokens=1");
    }
}
