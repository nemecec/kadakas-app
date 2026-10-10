//! Big values as content-addressed blobs.
//!
//! The picture of an underlay is megabytes of base64 text. If it were in the operation, every
//! edit of the underlay (a move, a new opacity) would send it again, and every entry in the log and
//! every snapshot would hold a copy. So the client replaces the picture by a *reference*,
//! `{"$blob": "<sha256 hex>"}`, and sends the picture once, as a blob:
//!
//! - `blob_put {hash, data}` (client to server): the server checks that the hash is the SHA-256 of
//!   `data` and keeps it. The client sends it before the transaction that uses it (messages of one
//!   connection keep their order), and again after a sign-in for every blob that a waiting
//!   transaction uses.
//! - `blob_get {hash}` (client to server) and `blob {hash, data}` or `blob_missing {hash}` (server
//!   to client): a client that sees a reference to a blob it does not have asks for it. Until the
//!   blob arrives, the object that uses it is left out of the document (quarantine: "the image has
//!   not arrived yet"), and it appears when the blob comes.
//!
//! The blob is the text of the picture (the base64 string of the PNG), so no decoding is needed.
//! A reference is the same for the same picture, so a picture that is used twice is sent once. The
//! server keeps blobs as long as the log refers to them. The checkpoint and the journal of a
//! client hold the blobs they need.
//!
//! Only the key `png` in a shape of kind `underlay`, with a text of at least [`MIN_BLOB_BYTES`], is
//! replaced.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

/// The smallest picture text that becomes a blob.
pub const MIN_BLOB_BYTES: usize = 4 * 1024;
/// The largest blob (the text of one picture).
pub const MAX_BLOB_BYTES: usize = 32 * 1024 * 1024;
/// The key of a reference.
pub const REF_KEY: &str = "$blob";

/// SHA-256 of some bytes, as lowercase hex (the `sha2` crate).
pub fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn is_underlay(shape: &Value) -> bool {
    shape.get("kind").and_then(Value::as_str) == Some("underlay")
}

/// If this value is a shape of an underlay with a big picture, the value with a reference instead,
/// and the blob (hash and text).
pub fn externalize(shape: &Value) -> Option<(Value, String, String)> {
    if !is_underlay(shape) {
        return None;
    }
    let png = shape
        .get("png")?
        .as_str()
        .filter(|p| p.len() >= MIN_BLOB_BYTES)?;
    let hash = sha256_hex(png.as_bytes());
    let mut v = shape.clone();
    v["png"] = Value::Object(Map::from_iter([(
        REF_KEY.to_string(),
        Value::String(hash.clone()),
    )]));
    Some((v, hash, png.to_string()))
}

/// The hash in a reference value (`{"$blob": hash}`).
fn ref_of(v: &Value) -> Option<&str> {
    v.as_object()
        .filter(|m| m.len() == 1)
        .and_then(|m| m.get(REF_KEY))
        .and_then(Value::as_str)
}

/// The hash of the blob that a shape value refers to, if it does.
pub fn blob_ref(shape: &Value) -> Option<&str> {
    if !is_underlay(shape) {
        return None;
    }
    ref_of(shape.get("png")?)
}

/// The properties with the pictures put back. `Err(hash)` names a blob that `resolve` does not
/// have.
pub fn inline_props(
    props: &Map<String, Value>,
    resolve: &dyn Fn(&str) -> Option<String>,
) -> Result<Map<String, Value>, String> {
    let mut out = props.clone();
    if let Some(shape) = out.get_mut("shape")
        && let Some(hash) = blob_ref(shape).map(str::to_string)
    {
        let text = resolve(&hash).ok_or(hash)?;
        shape["png"] = Value::String(text);
    }
    Ok(out)
}

/// The blobs that a map of properties refers to.
pub fn refs_in(props: &BTreeMap<String, Value>) -> Vec<String> {
    props
        .get("shape")
        .and_then(blob_ref)
        .map(str::to_string)
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sha256_matches_the_known_answers() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        // Over several blocks.
        assert_eq!(
            sha256_hex(&vec![b'a'; 1_000_000]),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    #[test]
    fn a_big_picture_becomes_a_reference_and_comes_back() {
        let text = "A".repeat(MIN_BLOB_BYTES + 10);
        let shape = json!({"kind": "underlay", "png": text, "opacity": 1.0});
        let (v, hash, data) = externalize(&shape).unwrap();
        assert_eq!(hash, sha256_hex(text.as_bytes()));
        assert_eq!(data, text);
        assert_eq!(blob_ref(&v), Some(hash.as_str()));
        assert_eq!(v["opacity"], 1.0);
        // Small pictures and other shapes stay as they are.
        assert!(externalize(&json!({"kind": "underlay", "png": "AAAA"})).is_none());
        assert!(externalize(&json!({"kind": "rect", "png": text})).is_none());
        assert!(externalize(&json!({"kind": "underlay"})).is_none());
        assert_eq!(blob_ref(&shape), None);
        // Put back.
        let mut props = Map::new();
        props.insert("shape".into(), v);
        let back = inline_props(&props, &|h| (h == hash).then(|| text.clone())).unwrap();
        assert_eq!(back["shape"]["png"], text);
        assert_eq!(
            inline_props(&props, &|_| None).unwrap_err(),
            hash,
            "an unknown blob is named"
        );
        let plain = Map::new();
        assert!(inline_props(&plain, &|_| None).unwrap().is_empty());
        let by_key: BTreeMap<String, Value> = props.into_iter().collect();
        assert_eq!(refs_in(&by_key), vec![hash]);
        assert!(refs_in(&BTreeMap::new()).is_empty());
    }
}
