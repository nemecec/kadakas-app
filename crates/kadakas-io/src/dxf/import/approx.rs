//! Counts of what the import changed or approximated, for the notes of the report (IOP-14,
//! IOP-15). Nothing here changes the geometry. The walker counts, and the notes come at the end.

use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct Tally {
    /// B-splines cut into lines within the tolerance.
    pub splines: usize,
    /// Splines with fit points only: a smooth curve through the points (the tangents are not read).
    pub spline_fit: usize,
    /// Splines without usable knots: the control polygon was imported.
    pub spline_polygon: usize,
    /// Ellipses and elliptical arcs cut into lines within the tolerance.
    pub ellipses: usize,
    /// Circles and arcs under a non-uniform scale, cut into lines within the tolerance.
    pub scaled_arcs: usize,
    /// LEADER entities (the arrowhead is not drawn).
    pub leaders: usize,
    /// Pattern hatches by pattern name (imported as outlines).
    pub pattern_hatches: BTreeMap<String, usize>,
    /// Gradient hatches (imported with a flat fill).
    pub gradient_hatches: usize,
    /// Islands of a solid hatch (drawn as white filled outlines on the hatch).
    pub hatch_islands: usize,
    /// Entities in a tilted plane that became 3D polylines.
    pub tilted: usize,
    /// Hatches in a tilted plane: their boundaries became 3D polylines without a fill.
    pub tilted_hatches: usize,
    /// WIPEOUT entities (a polygon with a white fill).
    pub wipeouts: usize,
    /// MESH entities with a subdivision level above 0 (imported as the control mesh).
    pub subdivided_meshes: usize,
    /// Layouts of paper space that became sheet layers: (name, entities, viewports).
    pub sheets: Vec<(String, usize, usize)>,
    /// Layouts without content (not imported).
    pub empty_layouts: Vec<String>,
    /// Layouts with no usable paper size (the sheet is A3).
    pub default_page: Vec<String>,
    /// Viewports that became viewports of a sheet.
    pub viewports: usize,
    /// Viewports with a twist (a turned view): the view is not turned.
    pub viewport_twist: usize,
    /// Viewports with a perspective view: shown with a parallel projection.
    pub viewport_perspective: usize,
    /// Viewports whose view direction is not a standard view, by the standard view that was used.
    pub viewport_views: BTreeMap<String, usize>,
    /// Viewports whose clipping outline is not a polyline or a circle: the frame is the crop.
    pub viewport_clip_lost: usize,
    /// 3D polylines in paper space that were flattened.
    pub flattened: usize,
}

impl Tally {
    /// The notes for the report. `tol` is the tolerance of the import in mm.
    pub fn notes(&self, tol: f64) -> Vec<String> {
        let mut out = Vec::new();
        let plural =
            |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
        if self.splines > 0 {
            out.push(format!(
                "{} became polylines within {tol} mm: the curve is no longer exact",
                plural(self.splines, "spline", "splines")
            ));
        }
        if self.spline_fit > 0 {
            out.push(format!(
                "{} with fit points only became smooth curves through the points: the tangents of the file are not read",
                plural(self.spline_fit, "spline", "splines")
            ));
        }
        if self.spline_polygon > 0 {
            out.push(format!(
                "{} had no usable knots, so the control polygon was imported and the shape is not the curve",
                plural(self.spline_polygon, "spline", "splines")
            ));
        }
        if self.ellipses > 0 {
            out.push(format!(
                "{} became polylines within {tol} mm: the curve is no longer exact",
                plural(
                    self.ellipses,
                    "ellipse or elliptical arc",
                    "ellipses and elliptical arcs"
                )
            ));
        }
        if self.scaled_arcs > 0 {
            out.push(format!(
                "{} under a block scale that is not the same in x and y became polylines within {tol} mm",
                plural(self.scaled_arcs, "circle or arc", "circles and arcs")
            ));
        }
        if self.leaders > 0 {
            out.push(format!(
                "{} imported as lines: the arrowhead is not drawn and a spline leader is a polyline",
                plural(self.leaders, "LEADER was", "LEADERs were")
            ));
        }
        if !self.pattern_hatches.is_empty() {
            let total: usize = self.pattern_hatches.values().sum();
            let names: Vec<String> = self
                .pattern_hatches
                .iter()
                .map(|(n, c)| format!("{n} {c}"))
                .collect();
            out.push(format!(
                "{} with a pattern {} imported as outlines without the pattern: {}",
                plural(total, "hatch", "hatches"),
                if total == 1 { "was" } else { "were" },
                names.join(", ")
            ));
        }
        if self.gradient_hatches > 0 {
            out.push(format!(
                "{} imported with a flat fill of its first color",
                plural(
                    self.gradient_hatches,
                    "gradient hatch was",
                    "gradient hatches were"
                )
            ));
        }
        if self.hatch_islands > 0 {
            out.push(format!(
                "{} of solid hatches {} drawn as white filled outlines (data.hatch_island) because a filled shape has no holes",
                plural(self.hatch_islands, "island", "islands"),
                if self.hatch_islands == 1 { "is" } else { "are" }
            ));
        }
        if self.tilted > 0 {
            out.push(format!(
                "{} in a tilted plane {} imported as {}",
                plural(self.tilted, "entity", "entities"),
                if self.tilted == 1 { "was" } else { "were" },
                if self.tilted == 1 {
                    "a 3D polyline"
                } else {
                    "3D polylines"
                }
            ));
        }
        if self.tilted_hatches > 0 {
            out.push(format!(
                "{} in a tilted plane {} imported as outlines without a fill",
                plural(self.tilted_hatches, "hatch", "hatches"),
                if self.tilted_hatches == 1 {
                    "was"
                } else {
                    "were"
                }
            ));
        }
        if self.wipeouts > 0 {
            out.push(format!(
                "{} became polygons with a white fill (data.wipeout)",
                plural(self.wipeouts, "WIPEOUT", "WIPEOUTs")
            ));
        }
        if self.subdivided_meshes > 0 {
            out.push(format!(
                "{} subdivided: only the control mesh was imported",
                plural(self.subdivided_meshes, "MESH is", "MESHes are")
            ));
        }
        for (name, entities, viewports) in &self.sheets {
            out.push(format!(
                "layout '{name}' of paper space became a sheet layer with {} and {}",
                plural(*entities, "entity", "entities"),
                plural(*viewports, "viewport", "viewports")
            ));
        }
        if !self.sheets.is_empty() && !self.empty_layouts.is_empty() {
            out.push(format!(
                "{} of paper space without content {} not imported: {}",
                plural(self.empty_layouts.len(), "layout", "layouts"),
                if self.empty_layouts.len() == 1 {
                    "was"
                } else {
                    "were"
                },
                self.empty_layouts.join(", ")
            ));
        }
        if !self.default_page.is_empty() {
            out.push(format!(
                "the layouts {} have no paper size: their sheets are A3",
                self.default_page.join(", ")
            ));
        }
        if self.viewport_twist > 0 {
            out.push(format!(
                "{} a twist, which a viewport of a sheet does not have: the view is not turned",
                plural(self.viewport_twist, "viewport has", "viewports have")
            ));
        }
        if self.viewport_perspective > 0 {
            out.push(format!(
                "{} a perspective view: it shows a parallel projection",
                plural(self.viewport_perspective, "viewport has", "viewports have")
            ));
        }
        for (view, n) in &self.viewport_views {
            out.push(format!(
                "{} of a view direction that is not a standard view: the nearest, {view}, is used",
                plural(*n, "viewport", "viewports")
            ));
        }
        if self.viewport_clip_lost > 0 {
            out.push(format!(
                "{} cropped by an outline that is not a polyline or a circle: the whole frame shows",
                plural(self.viewport_clip_lost, "viewport is", "viewports are")
            ));
        }
        if self.flattened > 0 {
            out.push(format!(
                "{} in paper space {} flattened to 2D (a sheet holds 2D shapes)",
                plural(self.flattened, "3D polyline", "3D polylines"),
                if self.flattened == 1 { "was" } else { "were" }
            ));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_tally_has_no_notes() {
        assert!(Tally::default().notes(2.0).is_empty());
    }

    #[test]
    fn every_counter_has_a_note_with_its_count() {
        let mut t = Tally {
            splines: 2,
            spline_fit: 1,
            spline_polygon: 3,
            ellipses: 1,
            scaled_arcs: 4,
            leaders: 2,
            gradient_hatches: 1,
            hatch_islands: 5,
            tilted: 6,
            tilted_hatches: 2,
            wipeouts: 1,
            subdivided_meshes: 2,
            ..Tally::default()
        };
        t.pattern_hatches.insert("ANSI31".into(), 2);
        t.pattern_hatches.insert("NET".into(), 1);
        let notes = t.notes(1.5);
        assert_eq!(notes.len(), 13);
        let all = notes.join("\n");
        for needle in [
            "2 splines became polylines within 1.5 mm",
            "1 spline with fit points",
            "3 splines had no usable knots",
            "1 ellipse or elliptical arc became",
            "4 circles and arcs under a block scale",
            "2 LEADERs were imported as lines",
            "3 hatches with a pattern were imported",
            "ANSI31 2, NET 1",
            "1 gradient hatch was imported",
            "5 islands of solid hatches are drawn",
            "6 entities in a tilted plane were imported as 3D polylines",
            "2 hatches in a tilted plane were imported as outlines",
            "1 WIPEOUT became",
            "2 MESHes are subdivided",
        ] {
            assert!(all.contains(needle), "{needle}\n{all}");
        }
    }
}
