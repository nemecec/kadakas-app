//! Globally unique 128-bit IDs (UUID version 7) and the per-document generator.
//!
//! Every object, layer, class and resource has an ID that no other client makes by accident, so
//! documents that were edited offline can be merged later. An ID is a 128-bit value. In text it
//! is the canonical UUID string, for example `0190f1c2-7a3b-7c1d-8e2f-123456789abc`.
//!
//! A new ID is a UUID version 7: the first 48 bits are a time in milliseconds, so IDs sort by
//! creation time, then come a counter (32 bits) and random bits (42 bits). The counter keeps the
//! IDs of one document in creation order. The random bits keep two documents that start in the
//! same millisecond apart.
//!
//! The generator belongs to the document ([`IdGen`]), not to the process, so sessions in one
//! process share nothing. It is a sequence: the next IDs can be read without taking them
//! ([`IdGen::peek`]), so a program can plan the IDs of a batch of commands, and the commands then
//! get exactly these IDs.

use std::fmt;
use std::str::FromStr;

use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize, Serializer};

/// Defines a typed ID: a newtype around [`Uid`], so a layer ID cannot be used as an object ID.
macro_rules! id_type {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(
            Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub $crate::uid::Uid);

        impl $name {
            /// An ID from a small number: for the well-known IDs and for tests.
            pub const fn from_num(n: u64) -> Self {
                $name($crate::uid::Uid::from_num(n))
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }

        impl From<$name> for $crate::uid::Uid {
            fn from(id: $name) -> $crate::uid::Uid {
                id.0
            }
        }

        impl From<$crate::uid::Uid> for $name {
            fn from(u: $crate::uid::Uid) -> $name {
                $name(u)
            }
        }
    };
}
pub(crate) use id_type;

/// A 128-bit identifier. Text form: `8-4-4-4-12` lowercase hex digits.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Uid(u128);

impl Uid {
    /// The all-zero ID. Nothing in a document has it.
    pub const NIL: Uid = Uid(0);

    pub const fn from_u128(v: u128) -> Uid {
        Uid(v)
    }

    pub const fn as_u128(self) -> u128 {
        self.0
    }

    /// An ID from a small number: the number is the low 64 bits. For the well-known IDs that
    /// every document has, and for tests. Generated IDs never look like this.
    pub const fn from_num(n: u64) -> Uid {
        Uid(n as u128)
    }

    /// The low 64 bits.
    pub const fn low_u64(self) -> u64 {
        self.0 as u64
    }

    /// The version nibble of the UUID (7 for generated IDs).
    pub const fn version(self) -> u8 {
        ((self.0 >> 76) & 0xf) as u8
    }

    /// The time part of a version 7 ID, in milliseconds since 1970.
    pub const fn unix_ms(self) -> u64 {
        (self.0 >> 80) as u64
    }

    /// Builds a version 7 UUID from its parts.
    fn v7(ms: u64, counter: u32, random: u64) -> Uid {
        let ms = (ms as u128) & 0xffff_ffff_ffff;
        let a = ((counter >> 20) & 0xfff) as u128;
        let b = ((counter & 0xf_ffff) as u128) << 42 | (random & ((1u64 << 42) - 1)) as u128;
        Uid(ms << 80 | 0x7 << 76 | a << 64 | 0b10 << 62 | b)
    }

    /// Reads the canonical text form (case does not matter).
    pub fn parse(s: &str) -> Option<Uid> {
        let b = s.as_bytes();
        if b.len() != 36 {
            return None;
        }
        let mut v: u128 = 0;
        for (i, &c) in b.iter().enumerate() {
            if matches!(i, 8 | 13 | 18 | 23) {
                if c != b'-' {
                    return None;
                }
                continue;
            }
            v = v << 4 | (c as char).to_digit(16)? as u128;
        }
        Some(Uid(v))
    }
}

impl fmt::Display for Uid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let v = self.0;
        write!(
            f,
            "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
            (v >> 96) as u32,
            (v >> 80) as u16,
            (v >> 64) as u16,
            (v >> 48) as u16,
            (v & 0xffff_ffff_ffff) as u64
        )
    }
}

impl fmt::Debug for Uid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

/// Why a text is not an ID.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("not an ID: {0:?}. An ID is a UUID text such as 0190f1c2-7a3b-7c1d-8e2f-123456789abc")]
pub struct ParseUidError(pub String);

impl FromStr for Uid {
    type Err = ParseUidError;

    fn from_str(s: &str) -> Result<Uid, ParseUidError> {
        Uid::parse(s).ok_or_else(|| ParseUidError(s.chars().take(60).collect()))
    }
}

impl Serialize for Uid {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Uid {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Uid, D::Error> {
        struct V;
        impl de::Visitor<'_> for V {
            type Value = Uid;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an ID: a UUID text such as 0190f1c2-7a3b-7c1d-8e2f-123456789abc")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Uid, E> {
                v.parse().map_err(E::custom)
            }
        }
        d.deserialize_str(V)
    }
}

impl From<Uid> for serde_json::Value {
    fn from(u: Uid) -> serde_json::Value {
        serde_json::Value::String(u.to_string())
    }
}

/// A JSON text compares equal to the ID it holds.
impl PartialEq<Uid> for serde_json::Value {
    fn eq(&self, other: &Uid) -> bool {
        self.as_str() == Some(other.to_string().as_str())
    }
}

impl PartialEq<serde_json::Value> for Uid {
    fn eq(&self, other: &serde_json::Value) -> bool {
        other == self
    }
}

/// Reads an ID out of JSON data. IDs are strings in JSON.
pub trait AsUid {
    fn as_uid(&self) -> Option<Uid>;
}

impl AsUid for serde_json::Value {
    fn as_uid(&self) -> Option<Uid> {
        self.as_str().and_then(Uid::parse)
    }
}

/// The generator of new IDs. Part of a document, not saved in files.
#[derive(Clone, Debug)]
pub struct IdGen {
    ms: u64,
    salt: u64,
    counter: u64,
    /// Predictable numbers instead of UUIDs (see [`IdGen::sequential`]).
    sequential: bool,
}

/// A step of the splitmix64 mixer: a well spread number from a number.
fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Milliseconds since 1970 now. The browser build has no clock in `std`: there it is 0, and the
/// host calls `IdGen::seed` with the real time.
fn now_ms() -> u64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64)
    }
    #[cfg(target_arch = "wasm32")]
    {
        0
    }
}

/// Random bits that differ between calls and between processes.
fn entropy() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u64(now_ms());
    h.write_u128(std::process::id() as u128);
    h.finish()
}

impl Default for IdGen {
    fn default() -> Self {
        IdGen::seed(now_ms(), entropy())
    }
}

impl IdGen {
    /// A generator that starts at `ms` and mixes `salt` into the random bits. Two generators with
    /// the same `ms` and `salt` make the same IDs.
    pub fn seed(ms: u64, salt: u64) -> IdGen {
        IdGen {
            ms,
            salt,
            counter: 0,
            sequential: false,
        }
    }

    /// A generator that counts: `first`, `first + 1`, and so on, as small numbers (`from_num`).
    /// For tests, fixtures and tools that need IDs they can predict. These IDs are not unique in
    /// the world, so a host never uses them for documents that are merged with others.
    pub fn sequential(first: u64) -> IdGen {
        IdGen {
            ms: 0,
            salt: 0,
            counter: first,
            sequential: true,
        }
    }

    fn at(&self, ms: u64, counter: u64) -> Uid {
        let random = mix(self.salt ^ mix(ms) ^ counter.wrapping_mul(0xA24B_AED4_963E_E407));
        Uid::v7(ms, counter as u32, random)
    }

    /// The next `n` IDs, without taking them.
    pub fn peek(&self, n: usize) -> Vec<Uid> {
        let mut g = self.clone();
        (0..n).map(|_| g.take()).collect()
    }

    /// Takes the next ID.
    pub fn take(&mut self) -> Uid {
        if self.sequential {
            let id = Uid::from_num(self.counter);
            self.counter += 1;
            return id;
        }
        if self.counter > u32::MAX as u64 {
            // Used up the counter of this millisecond: go on in the next one.
            self.ms += 1;
            self.counter = 0;
        }
        let id = self.at(self.ms, self.counter);
        self.counter += 1;
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_form_is_the_canonical_uuid() {
        let u = Uid::from_u128(0x0190_f1c2_7a3b_7c1d_8e2f_1234_5678_9abc);
        assert_eq!(u.to_string(), "0190f1c2-7a3b-7c1d-8e2f-123456789abc");
        assert_eq!(Uid::parse("0190F1C2-7A3B-7C1D-8E2F-123456789ABC"), Some(u));
        assert_eq!(format!("{u:?}"), u.to_string());
        assert_eq!(Uid::NIL.to_string(), "00000000-0000-0000-0000-000000000000");
        assert_eq!(
            Uid::from_num(5).to_string(),
            "00000000-0000-0000-0000-000000000005"
        );
        assert_eq!(Uid::from_num(5).low_u64(), 5);
        for bad in [
            "",
            "5",
            "0190f1c27a3b7c1d8e2f123456789abc",
            "0190f1c2-7a3b-7c1d-8e2f-123456789abg",
            "0190f1c2-7a3b-7c1d-8e2f-123456789ab",
            "0190f1c2_7a3b_7c1d_8e2f_123456789abc",
            "0190f1c2-7a3b-7c1d-8e2f-123456789abcd",
            "+190f1c2-7a3b-7c1d-8e2f-123456789abc",
        ] {
            assert_eq!(Uid::parse(bad), None, "{bad}");
        }
        assert!("zz".parse::<Uid>().unwrap_err().to_string().contains("zz"));
    }

    #[test]
    fn json_is_a_string() {
        let u = Uid::from_num(0x1234);
        let v = serde_json::to_value(u).unwrap();
        assert_eq!(v, serde_json::json!("00000000-0000-0000-0000-000000001234"));
        assert_eq!(serde_json::from_value::<Uid>(v).unwrap(), u);
        // A number is not an ID.
        let e = serde_json::from_str::<Uid>("5").unwrap_err().to_string();
        assert!(e.contains("UUID"), "{e}");
        assert!(serde_json::from_str::<Uid>("\"nope\"").is_err());
        assert!(serde_json::from_str::<Uid>("null").is_err());
    }

    #[test]
    fn generated_ids_are_version_7_unique_and_in_order() {
        let mut g = IdGen::seed(1_700_000_000_123, 99);
        let ids: Vec<Uid> = (0..5000).map(|_| g.take()).collect();
        for id in &ids {
            assert_eq!(id.version(), 7);
            assert_eq!(id.unix_ms(), 1_700_000_000_123);
            // The variant bits are 10.
            assert_eq!((id.as_u128() >> 62) & 0b11, 0b10);
            assert_eq!(Uid::parse(&id.to_string()), Some(*id));
        }
        let set: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(set.len(), ids.len());
        assert!(ids.windows(2).all(|w| w[0] < w[1]), "creation order");
    }

    #[test]
    fn peek_shows_the_ids_that_next_takes() {
        let mut g = IdGen::seed(5, 6);
        let plan = g.peek(4);
        let taken: Vec<Uid> = (0..4).map(|_| g.take()).collect();
        assert_eq!(plan, taken);
        assert_ne!(g.peek(1)[0], plan[0]);
        // The same seed gives the same IDs; another salt gives others.
        assert_eq!(IdGen::seed(5, 6).peek(4), plan);
        assert_ne!(IdGen::seed(5, 7).peek(4), plan);
        assert_ne!(IdGen::seed(6, 6).peek(4), plan);
    }

    #[test]
    fn a_full_counter_goes_on_in_the_next_millisecond() {
        let mut g = IdGen::seed(10, 1);
        g.counter = u32::MAX as u64;
        let a = g.take();
        let b = g.take();
        assert_eq!((a.unix_ms(), b.unix_ms()), (10, 11));
        assert!(a < b);
    }

    #[test]
    fn a_sequential_generator_counts_small_numbers() {
        let mut g = IdGen::sequential(2);
        assert_eq!(g.peek(2), vec![Uid::from_num(2), Uid::from_num(3)]);
        assert_eq!(g.take(), Uid::from_num(2));
        assert_eq!(g.take(), Uid::from_num(3));
        assert_eq!(g.take().version(), 0);
    }

    #[test]
    fn fresh_generators_differ() {
        let (a, b) = (IdGen::default(), IdGen::default());
        assert_ne!(a.peek(1), b.peek(1));
    }
}
