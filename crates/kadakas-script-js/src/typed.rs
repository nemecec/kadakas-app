//! Typed arrays between the script and the API.
//!
//! In a script, geometry can be a `Float64Array` (or `Float32Array`, `Int32Array`, `Uint32Array`,
//! `Uint8Array`) with a `stride` (2 for `[x, y]`, 3 for `[x, y, z]`). It crosses the sandbox as
//! bytes, not as JSON text. In the JSON of a call it is a marker
//! `{"$t": "f64", "i": 0, "n": 2}`: the kind, the index of the buffer and the stride.
//!
//! - [`resolve`] turns the markers of a request into plain JSON (flat numbers, or a list of
//!   `n`-tuples), so every method of the API sees the parameters it knows.
//! - [`extract`] does the opposite for a reply when the script asked for `typed: true`: a list
//!   of at least [`MIN_POINTS`] points (`[[x, y], ...]` or with `z`) and a list of at least
//!   [`MIN_NUMBERS`] numbers become markers.

use serde_json::{Map, Number, Value};

/// The shortest list of points that `extract` turns into a typed array.
pub const MIN_POINTS: usize = 8;
/// The shortest flat list of numbers that `extract` turns into a typed array.
pub const MIN_NUMBERS: usize = 64;

fn is_marker(m: &Map<String, Value>) -> bool {
    m.get("$t").is_some_and(Value::is_string) && m.get("i").is_some_and(Value::is_u64)
}

fn number(v: f64) -> Result<Value, String> {
    Number::from_f64(v)
        .map(Value::Number)
        .ok_or_else(|| "a typed array holds a number that is not finite".to_string())
}

fn elements(kind: &str, bytes: &[u8]) -> Result<Vec<Value>, String> {
    fn chunks<const N: usize>(bytes: &[u8]) -> Result<std::slice::ChunksExact<'_, u8>, String> {
        if bytes.len() % N != 0 {
            return Err(format!(
                "a typed array of {N}-byte elements has {} bytes",
                bytes.len()
            ));
        }
        Ok(bytes.chunks_exact(N))
    }
    match kind {
        "f64" => chunks::<8>(bytes)?
            .map(|c| number(f64::from_le_bytes(c.try_into().expect("8 bytes"))))
            .collect(),
        "f32" => chunks::<4>(bytes)?
            .map(|c| number(f64::from(f32::from_le_bytes(c.try_into().expect("4 bytes")))))
            .collect(),
        "i32" => Ok(chunks::<4>(bytes)?
            .map(|c| Value::from(i32::from_le_bytes(c.try_into().expect("4 bytes"))))
            .collect()),
        "u32" => Ok(chunks::<4>(bytes)?
            .map(|c| Value::from(u32::from_le_bytes(c.try_into().expect("4 bytes"))))
            .collect()),
        "u8" => Ok(bytes.iter().map(|b| Value::from(*b)).collect()),
        other => Err(format!("unknown typed array kind '{other}'")),
    }
}

/// Replaces every marker in `value` by the array it stands for. `buffers[i]` is the bytes of
/// buffer `i`.
pub fn resolve<B: AsRef<[u8]>>(value: &mut Value, buffers: &[B]) -> Result<(), String> {
    match value {
        Value::Array(items) => {
            // A list that starts with a number holds no marker (the script side does the same).
            if matches!(items.first(), Some(Value::Number(_))) {
                return Ok(());
            }
            items.iter_mut().try_for_each(|v| resolve(v, buffers))
        }
        Value::Object(map) => {
            if is_marker(map) {
                let kind = map["$t"].as_str().unwrap_or_default();
                let at = map["i"].as_u64().unwrap_or(u64::MAX) as usize;
                let stride = map.get("n").and_then(Value::as_u64).unwrap_or(1) as usize;
                let bytes = buffers
                    .get(at)
                    .ok_or_else(|| format!("typed array {at} is not there"))?
                    .as_ref();
                let flat = elements(kind, bytes)?;
                *value = if stride <= 1 {
                    Value::Array(flat)
                } else {
                    if flat.len() % stride != 0 {
                        return Err(format!(
                            "a typed array of {} numbers cannot be cut in groups of {stride}",
                            flat.len()
                        ));
                    }
                    Value::Array(
                        flat.chunks(stride)
                            .map(|c| Value::Array(c.to_vec()))
                            .collect(),
                    )
                };
                return Ok(());
            }
            map.values_mut().try_for_each(|v| resolve(v, buffers))
        }
        _ => Ok(()),
    }
}

fn f64_bytes(values: impl Iterator<Item = f64>) -> Vec<u8> {
    values.flat_map(f64::to_le_bytes).collect()
}

fn marker(index: usize, stride: usize) -> Value {
    let mut m = Map::new();
    m.insert("$t".into(), Value::from("f64"));
    m.insert("i".into(), Value::from(index));
    if stride > 1 {
        m.insert("n".into(), Value::from(stride));
    }
    Value::Object(m)
}

/// The tuple size if `items` is a list of points: all of them lists of 2 or all of 3 numbers.
fn point_stride(items: &[Value]) -> Option<usize> {
    let n = items.first()?.as_array()?.len();
    if !(2..=3).contains(&n) {
        return None;
    }
    items
        .iter()
        .all(|p| {
            p.as_array()
                .is_some_and(|a| a.len() == n && a.iter().all(Value::is_number))
        })
        .then_some(n)
}

/// Turns long lists of points and long lists of numbers in `value` into markers and returns their
/// bytes (all `f64`).
pub fn extract(value: &mut Value) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    walk(value, &mut out);
    out
}

fn walk(value: &mut Value, out: &mut Vec<Vec<u8>>) {
    match value {
        Value::Array(items) => {
            if items.len() >= MIN_POINTS
                && let Some(n) = point_stride(items)
            {
                let bytes = f64_bytes(
                    items
                        .iter()
                        .flat_map(|p| p.as_array().into_iter().flatten())
                        .filter_map(Value::as_f64),
                );
                out.push(bytes);
                *value = marker(out.len() - 1, n);
            } else if items.len() >= MIN_NUMBERS && items.iter().all(Value::is_number) {
                out.push(f64_bytes(items.iter().filter_map(Value::as_f64)));
                *value = marker(out.len() - 1, 1);
            } else {
                items.iter_mut().for_each(|v| walk(v, out));
            }
        }
        Value::Object(map) => map.values_mut().for_each(|v| walk(v, out)),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn f64s(v: &[f64]) -> Vec<u8> {
        f64_bytes(v.iter().copied())
    }

    #[test]
    fn a_marker_with_a_stride_becomes_a_list_of_points() {
        let mut v = json!({"points": {"$t": "f64", "i": 0, "n": 2}, "close": true});
        resolve(&mut v, &[f64s(&[1.0, 2.0, 3.5, 4.0])]).unwrap();
        assert_eq!(v, json!({"points": [[1.0, 2.0], [3.5, 4.0]], "close": true}));
    }

    #[test]
    fn a_flat_marker_becomes_numbers_and_integers_stay_integers() {
        let mut v = json!([{"ids": {"$t": "u32", "i": 0}}]);
        let ids: Vec<u8> = [7u32, 9].iter().flat_map(|n| n.to_le_bytes()).collect();
        resolve(&mut v, &[ids]).unwrap();
        assert_eq!(v, json!([{"ids": [7, 9]}]));
    }

    #[test]
    fn bad_markers_are_refused() {
        let b = [f64s(&[1.0, 2.0, 3.0])];
        let cut = |mut v: Value| resolve(&mut v, &b);
        assert!(cut(json!({"$t": "f64", "i": 5})).is_err(), "no such buffer");
        assert!(cut(json!({"$t": "f64", "i": 0, "n": 2})).is_err(), "3 numbers in pairs");
        assert!(cut(json!({"$t": "f16", "i": 0})).is_err(), "unknown kind");
        let nan = [f64s(&[f64::NAN])];
        assert!(resolve(&mut json!({"$t": "f64", "i": 0}), &nan).is_err());
        let odd = [vec![0u8; 7]];
        assert!(resolve(&mut json!({"$t": "f64", "i": 0}), &odd).is_err());
    }

    #[test]
    fn long_lists_of_points_leave_as_typed_arrays_and_come_back_equal() {
        let pts: Vec<Value> = (0..10).map(|i| json!([i as f64, i as f64 * 0.5])).collect();
        let original = json!({"ring": pts, "short": [[0.0, 1.0], [2.0, 3.0]], "n": 3});
        let mut v = original.clone();
        let bufs = extract(&mut v);
        assert_eq!(bufs.len(), 1);
        assert_eq!(v["ring"], json!({"$t": "f64", "i": 0, "n": 2}));
        assert_eq!(v["short"], original["short"], "short lists stay JSON");
        resolve(&mut v, &bufs).unwrap();
        assert_eq!(v, original);
    }

    #[test]
    fn long_lists_of_numbers_leave_as_flat_arrays() {
        let nums: Vec<Value> = (0..MIN_NUMBERS).map(|i| json!(i as f64)).collect();
        let mut v = json!({"d": nums});
        let bufs = extract(&mut v);
        assert_eq!(v["d"], json!({"$t": "f64", "i": 0}));
        assert_eq!(bufs[0].len(), MIN_NUMBERS * 8);
    }
}
