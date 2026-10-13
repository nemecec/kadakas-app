//! Texture beds (3D-05): site modifiers that give a region of the terrain its own texture
//! without changing the grade. A bed is a closed shape (`Shape::SiteModifier` with the kind
//! `ModifierKind::TextureBed`). Only the data and the rules live here. The cut of the terrain
//! along the outlines is in `kadakas-site` (module `bed`).
//!
//! The rules:
//! - A bed acts on the site models on its layer whose box touches the box of the bed. It must be
//!   visible (layer and class).
//! - The texture is the material in `texture`, else the material of the slot of the bed object,
//!   else the material of its class ("by class"). A bed without a material has no effect: the
//!   beds behind it, or the look of the site model, show.
//! - The stacking order is the drawing order of the document (`Arrange`): the bed in front wins
//!   where beds overlap.
//! - The mapping is a planar projection from above. A bed with a `mapping` uses it; otherwise the
//!   tile, the offset and the rotation of the site model apply.
//! - At most [`MAX_TEXTURE_BEDS`] beds act on one site model (the frontmost ones), and one outline
//!   has at most [`MAX_BED_VERTICES`] vertices.

use glam::DVec2;
use serde::{Deserialize, Serialize};

use crate::curve::{self, Vertex};
use crate::model::{Document, Object, ObjectId, Shape};
use crate::present::{MappingKind, PART_BED, TextureMapping};
use crate::resource::ResourceId;
use crate::site::{
    MAX_TEXTURE_OFFSET, MAX_TEXTURE_ROTATION, MAX_TEXTURE_TILE, MIN_TEXTURE_TILE, ModifierKind,
    SiteSettings,
};

/// The most texture beds that act on one site model.
pub const MAX_TEXTURE_BEDS: usize = 64;
/// The most vertices of the boundary of a bed.
pub const MAX_BED_VERTICES: usize = 4096;
/// The most edges of the flattened outline of a bed (curves are flattened coarser to stay under it).
pub const MAX_BED_EDGES: usize = 4096;

fn default_tile() -> f64 {
    2000.0
}

/// The mapping of a bed: it replaces the mapping of the site model.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BedMapping {
    /// Size of one repeat of the texture on the ground (a square), in mm.
    pub tile: f64,
    /// Shift of the texture on the ground, in mm.
    pub offset: [f64; 2],
    /// Counterclockwise turn of the texture, in degrees.
    pub rotation: f64,
}

impl Default for BedMapping {
    fn default() -> Self {
        BedMapping {
            tile: default_tile(),
            offset: [0.0, 0.0],
            rotation: 0.0,
        }
    }
}

impl BedMapping {
    /// The mapping of a site model, as the starting point of a mapping of a bed.
    pub fn of_site(s: &SiteSettings) -> BedMapping {
        BedMapping {
            tile: s.texture_tile,
            offset: s.texture_offset,
            rotation: s.texture_rotation,
        }
    }

    pub fn is_finite(&self) -> bool {
        self.numbers().iter().all(|v| v.is_finite())
    }

    /// The numbers, for range checks.
    pub fn numbers(&self) -> Vec<f64> {
        vec![self.tile, self.offset[0], self.offset[1], self.rotation]
    }

    /// The planar projection from above, in world units.
    pub fn mapping(&self) -> TextureMapping {
        TextureMapping {
            kind: MappingKind::Planar,
            scale: [self.tile, self.tile],
            rotation: self.rotation.rem_euclid(360.0),
            offset: self.offset,
        }
    }

    /// Why the mapping is not usable, if it is not.
    pub fn problem(&self) -> Option<&'static str> {
        if !self.is_finite() {
            Some("texture bed numbers must be finite")
        } else if !(MIN_TEXTURE_TILE..=MAX_TEXTURE_TILE).contains(&self.tile) {
            Some("texture tile size must be from 10 mm to 100000 m")
        } else if self.offset.iter().any(|v| v.abs() > MAX_TEXTURE_OFFSET) {
            Some("texture offset must not be larger than 1000000 m")
        } else if self.rotation.abs() > MAX_TEXTURE_ROTATION {
            Some("texture rotation must be within a million degrees")
        } else {
            None
        }
    }
}

/// Why a texture bed is not usable: its mapping or the size of its boundary. `None` for other
/// shapes.
pub fn problem(shape: &Shape) -> Option<&'static str> {
    let Shape::SiteModifier {
        modifier: ModifierKind::TextureBed { mapping, .. },
        boundary,
    } = shape
    else {
        return None;
    };
    if boundary.len() > MAX_BED_VERTICES {
        Some("a texture bed boundary has at most 4096 points")
    } else {
        mapping.and_then(|m| m.problem())
    }
}

/// Puts a bad mapping of a texture bed back to "the mapping of the site model". True if it did.
pub fn repair(shape: &mut Shape) -> bool {
    match shape {
        Shape::SiteModifier {
            modifier: ModifierKind::TextureBed { mapping, .. },
            ..
        } if mapping.is_some_and(|m| m.problem().is_some()) => {
            *mapping = None;
            true
        }
        _ => false,
    }
}

/// A texture bed that acts on a site model.
#[derive(Clone, Debug, PartialEq)]
pub struct Bed {
    /// The bed object.
    pub object: ObjectId,
    /// The part of the terrain triangles inside the bed: `PART_BED + number`. The number counts
    /// from the back of the stacking order.
    pub part: u8,
    /// The material that gives the texture.
    pub material: ResourceId,
    /// The mapping: the one of the bed, or else the one of the site model.
    pub mapping: TextureMapping,
    /// The flattened outline (at most [`MAX_BED_EDGES`] points).
    pub outline: Vec<DVec2>,
}

/// The flattened outline of a boundary. Curves are flattened coarser when the outline would have
/// more than [`MAX_BED_EDGES`] points. Empty if a number is not finite.
pub fn outline(boundary: &[Vertex]) -> Vec<DVec2> {
    if !boundary.iter().all(Vertex::is_finite) {
        return Vec::new();
    }
    let mut tol = 2.0;
    let mut pts = curve::flatten(boundary, true, tol);
    while pts.len() > MAX_BED_EDGES && tol < 1.0e9 {
        tol *= 2.0;
        pts = curve::flatten(boundary, true, tol);
    }
    if pts.len() > MAX_BED_EDGES {
        let step = pts.len().div_ceil(MAX_BED_EDGES);
        pts = pts.into_iter().step_by(step).collect();
    }
    pts
}

/// The texture beds that act on a site model, from the back to the front of the stacking order.
/// `material_of` gives the material of the slot of an object or of its class. Beds without a
/// material are left out. If there are more than [`MAX_TEXTURE_BEDS`], the frontmost remain.
pub fn beds_for(
    doc: &Document,
    site: &Object,
    material_of: &dyn Fn(&Object) -> Option<ResourceId>,
) -> Vec<Bed> {
    let Shape::SiteModel { settings, .. } = &site.shape else {
        return Vec::new();
    };
    let (lo, hi) = site.shape.bounds();
    let site_mapping = settings.texture_mapping();
    let mut found: Vec<Bed> = doc
        .objects
        .values()
        .filter(|o| o.layer == site.layer && doc.is_visible(o))
        .filter_map(|o| {
            let Shape::SiteModifier {
                modifier: ModifierKind::TextureBed { texture, mapping },
                boundary,
            } = &o.shape
            else {
                return None;
            };
            let (a, b) = o.shape.bounds();
            if a.x > hi.x || b.x < lo.x || a.y > hi.y || b.y < lo.y {
                return None;
            }
            let material = texture
                .filter(|t| crate::present::material(doc, *t).is_some())
                .or_else(|| material_of(o))?;
            let outline = outline(boundary);
            (outline.len() >= 3).then(|| Bed {
                object: o.id,
                part: 0,
                material,
                mapping: mapping.map_or(site_mapping, |m| m.mapping()),
                outline,
            })
        })
        .collect();
    if found.len() > MAX_TEXTURE_BEDS {
        found.drain(..found.len() - MAX_TEXTURE_BEDS);
    }
    for (i, b) in found.iter_mut().enumerate() {
        b.part = PART_BED + i as u8;
    }
    found
}

#[cfg(test)]
mod tests;
