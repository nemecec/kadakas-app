//! Grading and site analysis objects: landscape walls, grade objects, spot elevations, site
//! sections and the slope and elevation analysis of site models.
//!
//! These objects keep their plain shape (a polyline, a line or a circle) and their settings in
//! `Object::data`, like landscape areas do. The shape moves, snaps and exports as before. This
//! module reads the settings, asks the site model for the ground and draws the extras through the
//! extension hooks of core (see `hook`).

pub mod analysis;
pub mod grade;
pub mod hook;
pub mod section;
pub mod spot;
pub mod wall;

use std::sync::Arc;

use glam::DVec2;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use cadektor_core::model::{Document, Object, ObjectId, Shape};

use crate::model::{SiteCache, SiteResult, Surface};

/// Which surface of the site model an object follows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Follow {
    #[default]
    Existing,
    Proposed,
}

impl Follow {
    pub fn surface(self) -> Surface {
        match self {
            Follow::Existing => Surface::Existing,
            Follow::Proposed => Surface::Proposed,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Follow::Existing => "existing",
            Follow::Proposed => "proposed",
        }
    }
}

/// The height of the ground at a point, for a surface. `None` outside the site model.
pub type Ground<'a> = &'a dyn Fn(DVec2, Follow) -> Option<f64>;

/// The ground of one site model.
pub fn ground_of(site: &SiteResult) -> impl Fn(DVec2, Follow) -> Option<f64> + '_ {
    move |p, which| site.surface(which.surface()).elevation_at(p)
}

/// The settings under `key` of an object, if present and valid.
pub fn read<T: DeserializeOwned>(obj: &Object, key: &str) -> Option<T> {
    serde_json::from_value(obj.data.get(key)?.clone()).ok()
}

/// The site model that serves an object: the one it names, or the first visible site model on its
/// layer whose surface can be built.
pub fn host(doc: &Document, cache: &SiteCache, revision: u64, obj: &Object, named: Option<ObjectId>) -> Option<Arc<SiteResult>> {
    if let Some(id) = named {
        return cache.get(revision, doc, id).ok();
    }
    doc.objects
        .values()
        .filter(|o| o.layer == obj.layer && matches!(o.shape, Shape::SiteModel { .. }) && doc.is_visible(o))
        .find_map(|o| cache.get(revision, doc, o.id).ok())
}

/// Checks that a number is finite.
pub fn finite(name: &str, v: f64) -> Result<f64, String> {
    if v.is_finite() { Ok(v) } else { Err(format!("{name} must be a finite number")) }
}

/// Meters text for an elevation or length in mm, with `decimals` places.
pub fn meters(mm: f64, decimals: usize) -> String {
    format!("{:.*}", decimals, mm / 1000.0)
}

/// Escapes text for SVG.
pub(crate) fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

#[cfg(test)]
pub(crate) mod testutil {
    use super::*;
    use cadektor_core::command::{Command, Editor};
    use cadektor_core::curve;
    use cadektor_core::model::Attributes;
    use std::collections::BTreeMap;

    pub fn add(ed: &mut Editor, shape: Shape) -> ObjectId {
        let out = ed.execute(&Command::AddObject { shape, layer: None, class: None, attrs: Attributes::default() }).unwrap();
        ObjectId(out.created[0])
    }

    /// Adds a shape with data under one key.
    pub fn add_with(ed: &mut Editor, shape: Shape, key: &str, data: serde_json::Value) -> ObjectId {
        let id = add(ed, shape);
        ed.execute(&Command::SetData { id, set: BTreeMap::from([(key.to_string(), data)]), remove: vec![] }).unwrap();
        id
    }

    pub fn path(pts: &[(f64, f64)]) -> Shape {
        Shape::Polyline { points: curve::corners(&pts.iter().map(|p| DVec2::new(p.0, p.1)).collect::<Vec<_>>()), closed: false }
    }

    /// A 10 by 10 m site of 1 m steps with the elevation `f(x, y)`.
    pub fn site(ed: &mut Editor, f: impl Fn(f64, f64) -> f64) -> ObjectId {
        add(ed, crate::model::tests::grid_site(11, 1000.0, f))
    }
}
