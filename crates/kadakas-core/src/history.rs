//! The saved history of a document (REV-01), as the file keeps it.
//!
//! Core only holds the history and moves it with the document: it is saved in the field `history`
//! of the file, it is one value in the undo steps (a cheap clone), and a change of it is one undo
//! step (`Editor::set_history`). What the history means (versions, reverse deltas) is the
//! business of `kadakas-review`, which reads and writes the JSON. Core does one thing with it: at
//! load, a history that is not a JSON object or is too big gets a note. It is not dropped: the
//! versions of a file belong to the person, and the history is written back as it is.
//!
//! The history is local to the file. It is not part of the shared properties of collaboration.

use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

/// The most bytes (of JSON) that the history of a loaded file may have. A history above it is
/// noted at load (the history is kept, and read only). `kadakas-review` reads far less than this.
pub const MAX_HISTORY_BYTES: usize = 512 * 1024 * 1024;

type Items = Arc<Vec<Arc<Value>>>;
type Blobs = Arc<Vec<(String, Arc<str>)>>;

/// The history as JSON, in parts that are shared: a clone is cheap, and so is a new value that
/// differs from an older one in one version (an undo step holds the value before and after, and
/// the versions and pictures that did not change are the same memory in both).
///
/// The parts are `versions` (when that is an array, each element on its own), `newest.delta`
/// (when that is an array, each element on its own), `blobs` (when that is an object of texts,
/// each text on its own) and the rest of the object. The JSON is the same
/// as if the history were one `serde_json::Value`: `to_value` makes that value, and the
/// serialization writes the same text.
#[derive(Clone)]
pub struct HistoryData {
    /// The value without `versions` and `blobs` when those are split off; else the whole value.
    top: Arc<Value>,
    versions: Option<Items>,
    /// The array `delta` of the object `newest`, off the value.
    newest_delta: Option<Items>,
    blobs: Option<Blobs>,
    bytes: usize,
    /// A number that is new for each value that is made (a clone keeps it). Caches compare it
    /// instead of the value, which can be tens of megabytes.
    serial: u64,
    /// Something that the owner of the value wants to live as long as the value (and every clone
    /// of it, so every undo step that holds it). `kadakas-review` keeps the anchor drawing that
    /// the JSON is written against in it. It plays no part in equality or in the text.
    keep: Option<Arc<dyn std::any::Any + Send + Sync>>,
    /// The estimated size of what `keep` holds, in bytes. An undo step that holds a different
    /// thing than the step before counts it (see `bytes_not_in`).
    keep_bytes: usize,
}

impl std::fmt::Debug for HistoryData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HistoryData")
            .field("bytes", &self.bytes)
            .field("serial", &self.serial)
            .finish_non_exhaustive()
    }
}

/// The source of serial numbers (an id allocator, like the editor revision).
static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// A writer that counts the bytes and keeps none.
struct Counter(usize);

impl std::io::Write for Counter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0 = self.0.saturating_add(buf.len());
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn json_bytes(v: &impl Serialize) -> usize {
    let mut count = Counter(0);
    // Writing to a counter cannot fail.
    let _ = serde_json::to_writer(&mut count, v);
    count.0
}

fn next_serial() -> u64 {
    SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

impl HistoryData {
    /// Takes the parts off a value. Anything is allowed; a value that is not an object is kept as
    /// it is (see `problem`).
    pub fn new(mut value: Value) -> HistoryData {
        let mut versions = None;
        let mut blobs = None;
        let mut newest_delta = None;
        if let Value::Object(map) = &mut value {
            if let Some(Value::Object(n)) = map.get_mut("newest")
                && n.get("delta").is_some_and(Value::is_array)
                && let Some(Value::Array(items)) = n.remove("delta")
            {
                newest_delta = Some(Arc::new(items.into_iter().map(Arc::new).collect()));
            }
            if map.get("versions").is_some_and(Value::is_array)
                && let Some(Value::Array(items)) = map.remove("versions")
            {
                versions = Some(Arc::new(items.into_iter().map(Arc::new).collect()));
            }
            if map
                .get("blobs")
                .and_then(Value::as_object)
                .is_some_and(|o| o.values().all(Value::is_string))
                && let Some(Value::Object(table)) = map.remove("blobs")
            {
                let list = table
                    .into_iter()
                    .filter_map(|(k, v)| match v {
                        Value::String(t) => Some((k, Arc::<str>::from(t))),
                        _ => None,
                    })
                    .collect();
                blobs = Some(Arc::new(list));
            }
        }
        HistoryData::assemble(Arc::new(value), versions, newest_delta, blobs)
    }

    /// Makes a history of an object `top` (without `versions`, `blobs` and `newest.delta`), the
    /// JSON of each version, the JSON of each operation of the newest change, and the pictures.
    /// The parts are shared with the caller, who keeps the ones that did not change.
    pub fn from_parts(
        top: Value,
        versions: Vec<Arc<Value>>,
        newest_delta: Vec<Arc<Value>>,
        blobs: Vec<(String, Arc<str>)>,
    ) -> HistoryData {
        HistoryData::assemble(
            Arc::new(top),
            Some(Arc::new(versions)),
            Some(Arc::new(newest_delta)),
            Some(Arc::new(blobs)),
        )
    }

    fn assemble(
        top: Arc<Value>,
        versions: Option<Items>,
        newest_delta: Option<Items>,
        blobs: Option<Blobs>,
    ) -> HistoryData {
        let mut h = HistoryData {
            top,
            versions,
            newest_delta,
            blobs,
            bytes: 0,
            serial: next_serial(),
            keep: None,
            keep_bytes: 0,
        };
        h.bytes = json_bytes(&h);
        h
    }

    /// Identifies this value for a cache: a clone has the same serial, and every value that was
    /// made by `new` (or read from a file) has another one, whatever its content.
    pub fn serial(&self) -> u64 {
        self.serial
    }

    /// The history as one JSON value (a copy: the parts are shared in `self`, not in the copy).
    pub fn to_value(&self) -> Value {
        let mut v = (*self.top).clone();
        if let Value::Object(map) = &mut v {
            if let Some(items) = &self.versions {
                map.insert(
                    "versions".into(),
                    Value::Array(items.iter().map(|i| (**i).clone()).collect()),
                );
            }
            if let (Some(items), Some(Value::Object(n))) =
                (&self.newest_delta, map.get_mut("newest"))
            {
                n.insert(
                    "delta".into(),
                    Value::Array(items.iter().map(|i| (**i).clone()).collect()),
                );
            }
            if let Some(list) = &self.blobs {
                map.insert(
                    "blobs".into(),
                    Value::Object(
                        list.iter()
                            .map(|(k, t)| (k.clone(), Value::String(t.to_string())))
                            .collect(),
                    ),
                );
            }
        }
        v
    }

    /// The value without its `versions` and `blobs` (when those are split off; else the whole
    /// value). A reader that needs only a few entries does not copy the history.
    pub fn top(&self) -> &Value {
        &self.top
    }

    /// The same value, which keeps `thing` alive as long as it (and every clone of it) lives.
    ///
    /// `bytes` estimates its size. The undo budget counts it for a step that holds another thing
    /// than the step before (`bytes_not_in`), so that trimming the undo steps bounds the memory of
    /// the things they keep.
    pub fn with_keepalive(
        mut self,
        thing: Arc<dyn std::any::Any + Send + Sync>,
        bytes: usize,
    ) -> HistoryData {
        self.keep = Some(thing);
        self.keep_bytes = bytes;
        self
    }

    /// How many versions the list has, if it is split off the value (it is an array).
    pub fn version_count(&self) -> Option<usize> {
        self.versions.as_ref().map(|v| v.len())
    }

    /// The versions, one JSON value each, if the list is split off the value (it is an array).
    pub fn version_items(&self) -> Option<&[Arc<Value>]> {
        self.versions.as_ref().map(|v| v.as_slice())
    }

    /// The operations of the newest change, one JSON value each, if they are split off (the
    /// array `newest.delta`).
    pub fn newest_items(&self) -> Option<&[Arc<Value>]> {
        self.newest_delta.as_ref().map(|v| v.as_slice())
    }

    /// True if the table `blobs` is split off (it is an object of texts).
    pub fn has_blob_table(&self) -> bool {
        self.blobs.is_some()
    }

    /// The pictures of the history (empty if `blobs` is not a table of texts).
    pub fn blob_list(&self) -> &[(String, Arc<str>)] {
        self.blobs.as_ref().map_or(&[], |b| b.as_slice())
    }

    /// The same history with the entry `key` of the object set to `value` (removed for `None`),
    /// the pictures `remove` taken out of the table, and the pictures `add` that the table lacks.
    /// The versions and the other pictures are shared, not copied. A history that is not an
    /// object stays as it is.
    pub fn with_entry(
        &self,
        key: &str,
        value: Option<Value>,
        add: Vec<(String, Arc<str>)>,
        remove: &[String],
    ) -> HistoryData {
        let mut top = (*self.top).clone();
        let Value::Object(map) = &mut top else {
            return self.clone();
        };
        match value {
            Some(v) => {
                map.insert(key.to_string(), v);
            }
            None => {
                map.remove(key);
            }
        }
        let blobs = if add.is_empty() && remove.is_empty() {
            self.blobs.clone()
        } else {
            let mut list = self.blobs.as_deref().cloned().unwrap_or_default();
            list.retain(|(h, _)| !remove.contains(h));
            for (h, t) in add {
                if list.iter().all(|(k, _)| *k != h) {
                    list.push((h, t));
                }
            }
            list.sort_by(|a, b| a.0.cmp(&b.0));
            Some(Arc::new(list))
        };
        let mut out = HistoryData::assemble(
            Arc::new(top),
            self.versions.clone(),
            self.newest_delta.clone(),
            blobs,
        );
        out.keep = self.keep.clone();
        out.keep_bytes = self.keep_bytes;
        out
    }

    /// The size of the JSON text in bytes.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// The bytes of this history that `before` does not hold already: what a step that changed
    /// the history from `before` to `self` adds to the memory of the editor. The versions and
    /// pictures that both hold are the same memory.
    pub fn bytes_not_in(&self, before: Option<&HistoryData>) -> usize {
        let Some(before) = before else {
            return self.bytes + self.keep_bytes;
        };
        let mut n = json_bytes(&*self.top);
        let other_keep = match (&self.keep, &before.keep) {
            (Some(a), Some(b)) => Arc::as_ptr(a).cast::<()>() != Arc::as_ptr(b).cast::<()>(),
            (Some(_), None) => true,
            _ => false,
        };
        if other_keep {
            n += self.keep_bytes;
        }
        let new_items = |items: &Option<Items>, old: &Option<Items>| {
            let Some(items) = items else {
                return 0;
            };
            let known: std::collections::HashSet<*const Value> =
                old.iter().flat_map(|v| v.iter().map(Arc::as_ptr)).collect();
            items
                .iter()
                .filter(|i| !known.contains(&Arc::as_ptr(i)))
                .map(|i| json_bytes(&**i) + 1)
                .sum::<usize>()
        };
        n += new_items(&self.versions, &before.versions);
        n += new_items(&self.newest_delta, &before.newest_delta);
        if let Some(list) = &self.blobs {
            let known: std::collections::HashSet<*const u8> = before
                .blobs
                .iter()
                .flat_map(|b| b.iter().map(|(_, t)| t.as_ptr()))
                .collect();
            n += list
                .iter()
                .filter(|(_, t)| !known.contains(&t.as_ptr()))
                .map(|(k, t)| k.len() + t.len() + 6)
                .sum::<usize>();
        }
        n
    }

    /// Why the history cannot stay in a document, if it cannot.
    pub fn problem(&self) -> Option<String> {
        if !self.top.is_object() {
            Some(
                "the history of versions is not a JSON object: it is kept as it is and read only"
                    .into(),
            )
        } else if self.bytes > MAX_HISTORY_BYTES {
            Some(format!(
                "the history of versions has {} bytes, more than {MAX_HISTORY_BYTES}: it is kept as it is and read only",
                self.bytes
            ))
        } else {
            None
        }
    }
}

impl PartialEq for HistoryData {
    fn eq(&self, other: &Self) -> bool {
        self.serial == other.serial
            || (self.top == other.top
                && opt_eq(&self.newest_delta, &other.newest_delta, |a, b| {
                    same_items(a, b)
                })
                && opt_eq(&self.versions, &other.versions, |a, b| same_items(a, b))
                && opt_eq(&self.blobs, &other.blobs, |a, b| {
                    a.len() == b.len()
                        && a.iter()
                            .zip(b.iter())
                            .all(|(x, y)| x.0 == y.0 && (Arc::ptr_eq(&x.1, &y.1) || x.1 == y.1))
                }))
    }
}

fn same_items(a: &[Arc<Value>], b: &[Arc<Value>]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b.iter())
            .all(|(x, y)| Arc::ptr_eq(x, y) || x == y)
}

fn opt_eq<T>(a: &Option<Arc<T>>, b: &Option<Arc<T>>, same: impl Fn(&T, &T) -> bool) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Arc::ptr_eq(a, b) || same(a, b),
        (None, None) => true,
        _ => false,
    }
}

/// The versions as a JSON array, for the serialization.
struct Seq<'a>(&'a [Arc<Value>]);

impl Serialize for Seq<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(self.0.iter().map(|v| &**v))
    }
}

/// The pictures as a JSON object, for the serialization.
struct Table<'a>(&'a [(String, Arc<str>)]);

impl Serialize for Table<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_map(self.0.iter().map(|(k, t)| (k, &**t)))
    }
}

/// The object `newest` with its `delta` put back, for the serialization.
struct Newest<'a>(&'a serde_json::Map<String, Value>, &'a [Arc<Value>]);

impl Serialize for Newest<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut out = s.serialize_map(Some(self.0.len() + 1))?;
        let mut delta_done = false;
        for (k, v) in self.0 {
            if !delta_done && k.as_str() > "delta" {
                out.serialize_entry("delta", &Seq(self.1))?;
                delta_done = true;
            }
            out.serialize_entry(k, v)?;
        }
        if !delta_done {
            out.serialize_entry("delta", &Seq(self.1))?;
        }
        out.end()
    }
}

/// One entry of the object that is written.
enum Item<'a> {
    Newest(&'a serde_json::Map<String, Value>, &'a [Arc<Value>]),
    Top(&'a Value),
    Versions(&'a [Arc<Value>]),
    Blobs(&'a [(String, Arc<str>)]),
}

impl Serialize for HistoryData {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let Value::Object(map) = &*self.top else {
            return self.top.serialize(s);
        };
        // The keys are sorted, like those of `serde_json::Value`, so that the text is the same
        // as that of the whole value.
        let mut items: std::collections::BTreeMap<&str, Item<'_>> = map
            .iter()
            .map(|(k, v)| (k.as_str(), Item::Top(v)))
            .collect();
        if let (Some(d), Some(Value::Object(n))) = (&self.newest_delta, map.get("newest")) {
            items.insert("newest", Item::Newest(n, d));
        }
        if let Some(v) = &self.versions {
            items.insert("versions", Item::Versions(v));
        }
        if let Some(b) = &self.blobs {
            items.insert("blobs", Item::Blobs(b));
        }
        let mut out = s.serialize_map(Some(items.len()))?;
        for (k, item) in items {
            match item {
                Item::Top(v) => out.serialize_entry(k, v)?,
                Item::Newest(n, d) => out.serialize_entry(k, &Newest(n, d))?,
                Item::Versions(v) => out.serialize_entry(k, &Seq(v))?,
                Item::Blobs(b) => out.serialize_entry(k, &Table(b))?,
            }
        }
        out.end()
    }
}

impl<'de> Deserialize<'de> for HistoryData {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(HistoryData::new(Value::deserialize(d)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn counts_bytes_and_compares_by_value() {
        let a = HistoryData::new(json!({"format": 1, "versions": []}));
        assert_eq!(a.bytes(), a.to_value().to_string().len());
        let b = a.clone();
        assert_eq!(a, b);
        assert_eq!(a.serial(), b.serial());
        let same_text = HistoryData::new(json!({"format": 1, "versions": []}));
        assert_ne!(
            a.serial(),
            same_text.serial(),
            "a new value has a new serial"
        );
        assert_eq!(a, same_text, "the content is still equal");
        assert_eq!(a, HistoryData::new(json!({"versions": [], "format": 1})));
        assert_ne!(a, HistoryData::new(json!({"format": 2})));
        assert!(a.problem().is_none());
    }

    fn big() -> Value {
        json!({
            "format": 2, "next_id": 4, "head": 3,
            "newest": {"id": 3, "anchor": "ab", "delta": [{"op": "x"}, {"op": "y"}]},
            "blobs": {"h1": "text one", "h2": "text two"},
            "versions": [{"id": 1, "delta": [1]}, {"id": 2}, {"id": 3, "name": "n"}],
        })
    }

    #[test]
    fn the_parts_write_the_same_text_as_the_whole_value() {
        let value = big();
        let h = HistoryData::new(value.clone());
        assert_eq!(serde_json::to_string(&h).unwrap(), value.to_string());
        assert_eq!(h.to_value(), value);
        assert_eq!(h.bytes(), value.to_string().len());
        assert_eq!(h.blob_list().len(), 2);
        assert!(h.top().get("versions").is_none());
        assert!(h.top()["newest"].get("delta").is_none());
        let back: HistoryData = serde_json::from_str(&value.to_string()).unwrap();
        assert_eq!(back, h);
        // Shapes that are not split stay in the value.
        for odd in [
            json!({"versions": 5, "blobs": [1], "newest": {"delta": "x"}}),
            json!({"blobs": {"h": 7}, "versions": []}),
            json!({"newest": 3}),
        ] {
            let h = HistoryData::new(odd.clone());
            assert_eq!(serde_json::to_string(&h).unwrap(), odd.to_string());
            assert_eq!(h.to_value(), odd);
        }
    }

    #[test]
    fn a_new_value_shares_its_parts_and_counts_only_what_is_new() {
        let one = HistoryData::new(big());
        let before = one.bytes();
        // One more version and one more operation, the rest is the same memory.
        let two = {
            let mut v = big();
            v["versions"].as_array_mut().unwrap().push(json!({"id": 4}));
            HistoryData::new(v)
        };
        assert_eq!(two.bytes_not_in(None), two.bytes());
        // Not shared by `new` (it reads a new value): everything is new.
        assert!(two.bytes_not_in(Some(&one)) > two.bytes() * 3 / 4);
        // Parts that are shared are not counted again.
        let mut top = one.top().clone();
        top["head"] = json!(4);
        let versions: Vec<Arc<Value>> = one.versions.as_ref().unwrap().iter().cloned().collect();
        let delta: Vec<Arc<Value>> = one.newest_delta.as_ref().unwrap().iter().cloned().collect();
        let mut with_one_more = versions.clone();
        with_one_more.push(Arc::new(json!({"id": 4, "name": "new"})));
        let next = HistoryData::from_parts(top, with_one_more, delta, one.blob_list().to_vec());
        let added = next.bytes_not_in(Some(&one));
        assert!(added < before / 2, "{added} of {before}");
        assert!(added > 20);
        assert_ne!(next, one);
        assert_eq!(next.bytes_not_in(Some(&next)), json_bytes(next.top()));
    }

    #[test]
    fn a_step_that_holds_another_thing_counts_its_size() {
        let anchor_a: Arc<dyn std::any::Any + Send + Sync> = Arc::new(1u8);
        let anchor_b: Arc<dyn std::any::Any + Send + Sync> = Arc::new(2u8);
        let one = HistoryData::new(big()).with_keepalive(anchor_a.clone(), 5_000_000);
        // The first value of a document counts the thing.
        assert!(one.bytes_not_in(None) >= 5_000_000);
        // The same thing in the next value: not counted again.
        let same = HistoryData::new(big()).with_keepalive(anchor_a, 5_000_000);
        assert!(same.bytes_not_in(Some(&one)) < 1_000);
        // Another thing: counted.
        let other = HistoryData::new(big()).with_keepalive(anchor_b, 7_000_000);
        assert!(other.bytes_not_in(Some(&one)) >= 7_000_000);
        // A clone and `with_entry` keep it.
        let kept = other.with_entry("x", Some(json!(1)), vec![], &[]);
        assert!(kept.bytes_not_in(Some(&one)) >= 7_000_000);
    }

    #[test]
    fn an_entry_can_be_set_and_removed_without_copying_the_versions() {
        let h = HistoryData::new(big());
        let with = h.with_entry(
            "base",
            Some(json!({"anchor": "cd"})),
            vec![("h3".into(), Arc::from("text three"))],
            &[],
        );
        let v = with.to_value();
        assert_eq!(v["base"]["anchor"], "cd");
        assert_eq!(v["blobs"].as_object().unwrap().len(), 3);
        assert_eq!(v["versions"], big()["versions"]);
        // An existing picture is not added twice.
        let again = with.with_entry(
            "base",
            None,
            vec![("h3".into(), Arc::from("other"))],
            &["h1".to_string()],
        );
        assert!(again.blob_list().iter().all(|(h, _)| h != "h1"));
        assert_eq!(again.to_value()["blobs"]["h3"], "text three");
        assert!(again.to_value().get("base").is_none());
        assert_eq!(again.versions.as_ref().unwrap().len(), 3);
        assert!(Arc::ptr_eq(
            &again.versions.as_ref().unwrap()[0],
            &h.versions.as_ref().unwrap()[0]
        ));
        // A value that is not an object has no entries.
        let odd = HistoryData::new(json!([1]));
        assert_eq!(odd.with_entry("base", Some(json!(1)), vec![], &[]), odd);
    }

    #[test]
    fn problems_are_named() {
        assert!(HistoryData::new(json!([1])).problem().is_some());
        assert!(
            HistoryData::new(json!("x"))
                .problem()
                .unwrap()
                .contains("object")
        );
        let mut big = HistoryData::new(json!({}));
        big.bytes = MAX_HISTORY_BYTES + 1;
        assert!(big.problem().unwrap().contains("more than"));
    }

    #[test]
    fn json_round_trip_keeps_the_value() {
        let a = HistoryData::new(json!({"format": 1}));
        let text = serde_json::to_string(&a).unwrap();
        assert_eq!(text, r#"{"format":1}"#);
        let back: HistoryData = serde_json::from_str(&text).unwrap();
        assert_eq!(back, a);
    }
}
