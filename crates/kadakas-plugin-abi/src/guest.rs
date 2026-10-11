//! Helpers for the plug-in side.
//!
//! A plug-in is a `cdylib` crate for `wasm32-unknown-unknown`. It writes a function that makes the
//! manifest and a function that does the reset, and exports them with [`export_plugin!`]:
//!
//! ```ignore
//! use kadakas_plugin_abi::{Manifest, OutShape, ResetRequest, export_plugin};
//!
//! fn manifest() -> Manifest { /* ... */ }
//! fn reset(request: ResetRequest) -> Result<Vec<OutShape>, String> { /* ... */ }
//!
//! export_plugin!(describe = manifest, reset = reset);
//! ```
//!
//! The functions of this module that touch module memory exist only on `wasm32`. The others
//! ([`pack`], [`unpack`], [`describe_bytes`], [`reset_bytes`]) are plain functions with tests.

use crate::manifest::Manifest;
use crate::message::{OutShape, ResetRequest, ResetResponse};
use crate::tool::{ToolReply, ToolRequest, ToolResponse};

/// Packs an address and a length into the `u64` that `describe` and `reset` return.
pub fn pack(ptr: u32, len: u32) -> u64 {
    (u64::from(ptr) << 32) | u64::from(len)
}

/// The address and the length of a packed result.
pub fn unpack(packed: u64) -> (u32, u32) {
    ((packed >> 32) as u32, packed as u32)
}

/// The bytes that `describe` returns: the manifest as JSON.
pub fn describe_bytes(manifest: &Manifest) -> Vec<u8> {
    serde_json::to_vec(manifest).expect("a manifest is always valid JSON")
}

/// The bytes that `reset` returns. The request is JSON. A request that does not parse, and an
/// error from `f`, become an error answer, so the plug-in never has to trap.
pub fn reset_bytes(
    input: &[u8],
    f: impl FnOnce(ResetRequest) -> Result<Vec<OutShape>, String>,
) -> Vec<u8> {
    let response = match serde_json::from_slice::<ResetRequest>(input) {
        Err(e) => ResetResponse::Error {
            message: format!("bad request: {e}"),
        },
        Ok(request) => match f(request) {
            Ok(shapes) => ResetResponse::Ok { shapes },
            Err(message) => ResetResponse::Error { message },
        },
    };
    serde_json::to_vec(&response).expect("an answer is always valid JSON")
}

/// The bytes that `tool` returns. A request that does not parse, and an error from `f`, become an
/// error answer, so the plug-in never has to trap.
pub fn tool_bytes(
    input: &[u8],
    f: impl FnOnce(ToolRequest) -> Result<ToolReply, String>,
) -> Vec<u8> {
    let response = match serde_json::from_slice::<ToolRequest>(input) {
        Err(e) => ToolResponse::Error {
            message: format!("bad request: {e}"),
        },
        Ok(request) => match f(request) {
            Ok(reply) => ToolResponse::Ok(reply),
            Err(message) => ToolResponse::Error { message },
        },
    };
    serde_json::to_vec(&response).expect("an answer is always valid JSON")
}

/// The `reset` function of a module that makes no objects. [`export_plugin!`] uses it when you
/// leave out `reset`.
pub fn no_reset(_: ResetRequest) -> Result<Vec<OutShape>, String> {
    Err("this plug-in makes no objects".into())
}

/// The `tool` function of a module that has no tool. [`export_plugin!`] uses it when you leave
/// out `tool`.
pub fn no_tool(_: ToolRequest) -> Result<ToolReply, String> {
    Err("this plug-in has no tool".into())
}

/// Reserves a block of `len` bytes and returns its address. Use it as the export `alloc`.
#[cfg(target_arch = "wasm32")]
pub fn alloc(len: u32) -> u32 {
    let block = vec![0u8; len as usize].into_boxed_slice();
    Box::into_raw(block) as *mut u8 as usize as u32
}

/// Frees a block from [`alloc`] or [`leak`]. Use it as the export `dealloc`.
#[cfg(target_arch = "wasm32")]
pub fn dealloc(ptr: u32, len: u32) {
    let slice = std::ptr::slice_from_raw_parts_mut(ptr as usize as *mut u8, len as usize);
    // SAFETY: the host passes back a block of exactly this address and length from `alloc` or `leak`.
    drop(unsafe { Box::from_raw(slice) });
}

/// Hands bytes to the host: the block stays until the host calls `dealloc`.
#[cfg(target_arch = "wasm32")]
pub fn leak(bytes: Vec<u8>) -> u64 {
    let len = bytes.len() as u32;
    let ptr = Box::into_raw(bytes.into_boxed_slice()) as *mut u8 as usize as u32;
    pack(ptr, len)
}

/// Reads the block that the host wrote with `alloc`.
#[cfg(target_arch = "wasm32")]
pub fn read<'a>(ptr: u32, len: u32) -> &'a [u8] {
    // SAFETY: the host wrote `len` bytes at `ptr` and keeps the block until `reset` returns.
    unsafe { std::slice::from_raw_parts(ptr as usize as *const u8, len as usize) }
}

/// Exports `alloc`, `dealloc`, `describe`, `reset` and `tool` for the host.
///
/// `describe` is a function `fn() -> Manifest`. `reset` is a function
/// `fn(ResetRequest) -> Result<Vec<OutShape>, String>`. `tool` is a function
/// `fn(ToolRequest) -> Result<ToolReply, String>`. Give `reset`, `tool` or both. The one that you
/// leave out answers with an error. Use the macro once, in the crate root of a `cdylib` for
/// `wasm32-unknown-unknown`.
#[macro_export]
macro_rules! export_plugin {
    (describe = $describe:path, reset = $reset:path) => {
        $crate::export_plugin!(
            describe = $describe,
            reset = $reset,
            tool = $crate::guest::no_tool
        );
    };
    (describe = $describe:path, tool = $tool:path) => {
        $crate::export_plugin!(
            describe = $describe,
            reset = $crate::guest::no_reset,
            tool = $tool
        );
    };
    (describe = $describe:path, reset = $reset:path, tool = $tool:path) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn alloc(len: u32) -> u32 {
            $crate::guest::alloc(len)
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn dealloc(ptr: u32, len: u32) {
            $crate::guest::dealloc(ptr, len)
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn describe() -> u64 {
            $crate::guest::leak($crate::guest::describe_bytes(&$describe()))
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn reset(ptr: u32, len: u32) -> u64 {
            $crate::guest::leak($crate::guest::reset_bytes(
                $crate::guest::read(ptr, len),
                $reset,
            ))
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn tool(ptr: u32, len: u32) -> u64 {
            $crate::guest::leak($crate::guest::tool_bytes(
                $crate::guest::read(ptr, len),
                $tool,
            ))
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{ObjectSpec, ShapeType};
    use crate::message::ShapeData;

    #[test]
    fn pack_and_unpack_agree() {
        for (p, l) in [(0, 0), (1, 2), (u32::MAX, u32::MAX), (65536, 7)] {
            assert_eq!(unpack(pack(p, l)), (p, l));
        }
        assert_eq!(pack(1, 2), (1u64 << 32) | 2);
    }

    #[test]
    fn describe_gives_the_manifest_json() {
        let m = Manifest::object(
            "org.example.a",
            "A",
            "1",
            ObjectSpec {
                shape_type: ShapeType::Point,
                params: vec![],
            },
        );
        assert_eq!(Manifest::from_bytes(&describe_bytes(&m)).unwrap(), m);
    }

    #[test]
    fn reset_bytes_turns_every_outcome_into_an_answer() {
        let input = br#"{"params":{"n":2},"geometry":{"type":"point","at":[1,2]}}"#;
        let ok = reset_bytes(input, |r| {
            assert_eq!(r.params["n"], 2);
            Ok(vec![OutShape::new(ShapeData::Circle {
                center: [1.0, 2.0],
                radius: 3.0,
            })])
        });
        let v: ResetResponse = serde_json::from_slice(&ok).unwrap();
        assert!(matches!(v, ResetResponse::Ok { shapes } if shapes.len() == 1));
        let err = reset_bytes(input, |_| Err("no room".into()));
        let v: ResetResponse = serde_json::from_slice(&err).unwrap();
        assert_eq!(
            v,
            ResetResponse::Error {
                message: "no room".into()
            }
        );
        let bad = reset_bytes(b"[]", |_| unreachable!());
        let v: ResetResponse = serde_json::from_slice(&bad).unwrap();
        assert!(
            matches!(v, ResetResponse::Error { message } if message.starts_with("bad request"))
        );
    }

    #[test]
    fn tool_bytes_turns_every_outcome_into_an_answer() {
        let input = br#"{"event":{"type":"click","at":[1,2]},"state":{"n":1}}"#;
        let ok = tool_bytes(input, |r| {
            assert_eq!(r.state["n"], 1);
            Ok(ToolReply {
                hint: "next".into(),
                ..ToolReply::default()
            })
        });
        let v: ToolResponse = serde_json::from_slice(&ok).unwrap();
        assert!(matches!(v, ToolResponse::Ok(r) if r.hint == "next"));
        let err = tool_bytes(input, |_| Err("no".into()));
        let v: ToolResponse = serde_json::from_slice(&err).unwrap();
        assert_eq!(
            v,
            ToolResponse::Error {
                message: "no".into()
            }
        );
        let bad = tool_bytes(b"[]", |_| unreachable!());
        let v: ToolResponse = serde_json::from_slice(&bad).unwrap();
        assert!(matches!(v, ToolResponse::Error { message } if message.starts_with("bad request")));
    }

    #[test]
    fn the_missing_function_of_a_one_kind_module_answers_with_an_error() {
        let req = ResetRequest::new(
            serde_json::Map::new(),
            crate::message::Geometry::Point {
                at: [0.0, 0.0],
                rotation: 0.0,
            },
        );
        assert!(no_reset(req).unwrap_err().contains("no objects"));
        let t = ToolRequest::new(
            crate::tool::ToolEvent::Start,
            serde_json::Value::Null,
            serde_json::Map::new(),
        );
        assert!(no_tool(t).unwrap_err().contains("no tool"));
    }
}
