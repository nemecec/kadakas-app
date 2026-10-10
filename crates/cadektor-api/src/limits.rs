//! Limits of one session: memory, time and sizes of results.
//!
//! A host that serves many documents in one process gives each session its own limits. The caps
//! that were constants before (image sizes, import text, tile cache, script time) are fields
//! here, with the old values as defaults. The memory figure is an estimate: it counts what a
//! session holds in a typical case (objects, resources, pictures, history, the cached scene),
//! not every byte.

use std::time::Duration;

use crate::{RpcError, Session};

/// The limits of one session. `Default` gives the limits that Cadektor always had, and no limit
/// for the number of objects, the memory and the time of a request.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionLimits {
    /// Most objects in the document. A call that adds more is undone and fails with a limit error.
    pub max_objects: usize,
    /// Most estimated memory of the session, in bytes (see `Session::estimated_bytes`). A call
    /// that grows the session beyond it is undone and fails with a limit error.
    pub max_memory_bytes: u64,
    /// Time budget of one call. A call that takes longer fails with a limit error; if it changed
    /// the document, the change is undone. The check runs after the call, so it does not stop a
    /// long calculation: it keeps its result from the document. `None`: no budget. Native only.
    pub request_time: Option<Duration>,
    /// Longest side of a rendered image, in px.
    pub render_side: u32,
    /// Most pixels in a rendered image.
    pub render_pixels: u64,
    /// Most pixels of 3D viewport images on one sheet page and in one export.
    pub raster_pixels: u64,
    /// The most text that an import reads, in bytes.
    pub import_text_bytes: u64,
    /// Time limit of one script reset.
    pub script_timeout: Duration,
    /// Time limit of one request to a map service.
    pub gis_timeout: Duration,
    /// Size of the tile cache on disk, in bytes.
    pub gis_cache_bytes: u64,
}

impl Default for SessionLimits {
    fn default() -> Self {
        SessionLimits {
            max_objects: usize::MAX,
            max_memory_bytes: u64::MAX,
            request_time: None,
            render_side: 2048,
            render_pixels: 4_000_000,
            raster_pixels: 64_000_000,
            import_text_bytes: 256 * 1024 * 1024,
            script_timeout: Duration::from_secs(5),
            gis_timeout: Duration::from_secs(20),
            gis_cache_bytes: 256 * 1024 * 1024,
        }
    }
}

impl SessionLimits {
    /// Limits for documents of free and demo accounts, where many sessions share a process.
    pub fn strict() -> Self {
        SessionLimits {
            max_objects: 50_000,
            max_memory_bytes: 512 * 1024 * 1024,
            request_time: Some(Duration::from_secs(10)),
            render_pixels: 1_000_000,
            raster_pixels: 8_000_000,
            import_text_bytes: 16 * 1024 * 1024,
            script_timeout: Duration::from_secs(2),
            gis_cache_bytes: 32 * 1024 * 1024,
            ..SessionLimits::default()
        }
    }

    /// The size to render: each side 16 to `render_side`, scaled down in one piece if the area is
    /// too big.
    pub fn image_size(&self, width: u32, height: u32) -> (u32, u32) {
        let side = self.render_side.max(16);
        let (w, h) = (width.clamp(16, side), height.clamp(16, side));
        let area = w as u64 * h as u64;
        if area <= self.render_pixels {
            return (w, h);
        }
        let k = (self.render_pixels as f64 / area as f64).sqrt();
        (
            ((w as f64 * k) as u32).max(16),
            ((h as f64 * k) as u32).max(16),
        )
    }
}

/// Costs in the estimate of the memory of a session, in bytes.
const OBJECT_BYTES: u64 = 320;
const RESOURCE_BYTES: u64 = 2048;
const HISTORY_STEP_BYTES: u64 = 1024;

impl Session {
    /// The limits of this session.
    pub fn limits(&self) -> &SessionLimits {
        &self.limits
    }

    /// Sets the limits and passes them on to the parts that enforce them (the page cache, the map
    /// services and the scripts). The values stay when the document is replaced.
    pub fn set_limits(&mut self, limits: SessionLimits) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.sheet_cache.raster_limit = Some(limits.raster_pixels);
            self.gis.timeout = limits.gis_timeout;
            self.gis.cache_limit = limits.gis_cache_bytes;
            self.scripting.limits.timeout = limits.script_timeout;
        }
        self.limits = limits;
    }

    /// A rough figure for the memory of the session in bytes: the document, its pictures, the
    /// history and the cached scene.
    pub fn estimated_bytes(&self) -> u64 {
        let doc = self.editor.doc();
        let pictures: u64 = doc
            .objects
            .values()
            .map(|o| match &o.shape {
                cadektor_core::model::Shape::Underlay(u) => u.png.0.len() as u64,
                _ => 0,
            })
            .sum();
        doc.objects.len() as u64 * OBJECT_BYTES
            + doc.resources.len() as u64 * RESOURCE_BYTES
            + pictures
            + self.editor.undo_depth() as u64 * HISTORY_STEP_BYTES
            + self.mesh_cache.approx_bytes()
    }

    /// Checks the limits after a call. A call that changed the document and broke a limit is
    /// undone (`steps` is the number of undo steps it added). A call that took too long, or broke
    /// a limit, gives a limit error instead of its result.
    pub(crate) fn enforce_limits(
        &mut self,
        steps: usize,
        elapsed: Option<Duration>,
        result: Result<serde_json::Value, RpcError>,
    ) -> Result<serde_json::Value, RpcError> {
        let Ok(value) = result else {
            return result;
        };
        let broken = if self.editor.doc().objects.len() > self.limits.max_objects {
            Some(format!(
                "the document may have at most {} objects",
                self.limits.max_objects
            ))
        } else if self.limits.max_memory_bytes != u64::MAX
            && self.estimated_bytes() > self.limits.max_memory_bytes
        {
            Some(format!(
                "the session may use at most {} bytes of memory",
                self.limits.max_memory_bytes
            ))
        } else {
            match (self.limits.request_time, elapsed) {
                (Some(budget), Some(took)) if took > budget => Some(format!(
                    "the call took {:.1} s, the budget is {:.1} s",
                    took.as_secs_f64(),
                    budget.as_secs_f64()
                )),
                _ => None,
            }
        };
        match broken {
            None => Ok(value),
            Some(message) => {
                self.editor.rollback(steps);
                Err(RpcError::limit(message))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn add_circles(s: &mut Session, n: usize) -> Result<serde_json::Value, RpcError> {
        let ops: Vec<_> = (0..n)
            .map(|i| {
                json!({"op": "add_object", "shape": {"kind": "circle", "center": [i as f64 * 10.0, 0.0], "radius": 2.0}})
            })
            .collect();
        s.handle(
            "execute",
            json!({"command": {"op": "batch", "commands": ops}}),
        )
    }

    #[test]
    fn defaults_are_the_old_caps() {
        let l = SessionLimits::default();
        assert_eq!(l.image_size(99999, 99999), (2000, 2000));
        assert_eq!(l.image_size(2048, 2048), (2000, 2000));
        assert_eq!(l.image_size(2000, 2000), (2000, 2000));
        assert_eq!(l.image_size(2048, 1024), (2048, 1024));
        assert_eq!(l.image_size(1, 1), (16, 16));
        let strict = SessionLimits::strict();
        assert!(strict.max_objects < usize::MAX && strict.request_time.is_some());
        assert!(strict.image_size(2048, 2048).0 <= 1000);
    }

    #[test]
    fn set_limits_reach_the_subsystems_and_survive_a_new_document() {
        let mut s = Session::default();
        let l = SessionLimits {
            script_timeout: Duration::from_millis(1500),
            gis_timeout: Duration::from_secs(3),
            gis_cache_bytes: 1234,
            raster_pixels: 777,
            ..SessionLimits::default()
        };
        s.set_limits(l.clone());
        assert_eq!(s.limits(), &l);
        assert_eq!(s.gis.timeout, Duration::from_secs(3));
        assert_eq!(s.gis.cache_limit, 1234);
        assert_eq!(s.scripting.limits.timeout, Duration::from_millis(1500));
        assert_eq!(s.sheet_cache.raster_pixels(), 777);
        s.replace_document(Default::default());
        assert_eq!(s.limits(), &l);
        assert_eq!(s.sheet_cache.raster_pixels(), 777);
    }

    #[test]
    fn a_call_over_the_object_limit_is_undone() {
        let mut s = Session::default();
        s.set_limits(SessionLimits {
            max_objects: 5,
            ..SessionLimits::default()
        });
        add_circles(&mut s, 5).unwrap();
        let before = s.editor.doc().objects.len();
        let e = add_circles(&mut s, 3).unwrap_err();
        assert_eq!(e.kind, "limit");
        assert_eq!(s.editor.doc().objects.len(), before, "the batch is undone");
        assert!(!s.editor.can_redo(), "a rolled back step cannot be redone");
        // Reads still work, and undo goes back to the earlier step.
        s.handle("objects.list", json!({})).unwrap();
        assert!(s.editor.can_undo());
    }

    #[test]
    fn a_call_over_the_memory_limit_is_undone() {
        let mut s = Session::default();
        let base = s.estimated_bytes();
        s.set_limits(SessionLimits {
            max_memory_bytes: base + 10 * OBJECT_BYTES + HISTORY_STEP_BYTES,
            ..SessionLimits::default()
        });
        add_circles(&mut s, 4).unwrap();
        assert!(s.estimated_bytes() > base);
        let e = add_circles(&mut s, 40).unwrap_err();
        assert_eq!(e.kind, "limit");
        assert_eq!(s.editor.doc().objects.len(), 4);
    }

    #[test]
    fn a_slow_call_gives_a_limit_error() {
        let mut s = Session::default();
        s.set_limits(SessionLimits {
            request_time: Some(Duration::from_secs(5)),
            ..SessionLimits::default()
        });
        // Ask the check directly with a time over the budget.
        let e = s
            .enforce_limits(0, Some(Duration::from_secs(6)), Ok(json!({})))
            .unwrap_err();
        assert_eq!(e.kind, "limit");
        assert!(
            s.enforce_limits(0, Some(Duration::from_secs(1)), Ok(json!({})))
                .is_ok()
        );
        // Errors pass through unchanged.
        let err = RpcError::limit("x");
        assert!(
            s.enforce_limits(0, None, Err(err))
                .unwrap_err()
                .message
                .contains('x')
        );
    }
}
