//! Paper space (IOP-16): a layout becomes a sheet layer, a viewport of the layout becomes a
//! viewport of the sheet, and the entities of the layout (annotations, title blocks, frames) stay
//! on the sheet layer in the order of the file, so what lies under a viewport stays under it.
//!
//! Sheet coordinates are millimeters on paper. The unit of paper space is the plot paper unit of
//! the layout (millimeters, or inches), and the origin is the lower left corner of its limits.

use kadakas_core::sheets::{ClassOverride, Crop, Orientation, PaperSize, Viewport};

use super::*;
use crate::dxf::views::view_for;

/// One layout of paper space.
pub(super) struct Layout<'a> {
    pub name: String,
    pub tab: i64,
    pub entities: Vec<&'a Entity>,
    /// The size of the paper in mm, if the layout says it.
    pub page_mm: Option<DVec2>,
    /// The limits of the layout in paper units.
    pub limits: Option<(DVec2, DVec2)>,
    /// Millimeters per unit of paper space.
    pub unit_mm: f64,
}

fn last(e: &Entity, code: i32) -> Option<&str> {
    e.items
        .iter()
        .rev()
        .find(|(c, _)| *c == code)
        .map(|(_, v)| v.as_str())
}

/// The layout object (LAYOUT) that belongs to the block record, if there is one.
fn layout_of<'a>(dxf: &'a Dxf, record: Option<&str>) -> Option<&'a Entity> {
    let record = record?;
    dxf.objects.iter().find(|o| {
        o.kind == "LAYOUT"
            && last(o, 330)
                .and_then(crate::dxf::hash::normal_handle)
                .as_deref()
                == Some(record)
    })
}

fn layout_from<'a>(
    object: Option<&Entity>,
    fallback: &str,
    entities: Vec<&'a Entity>,
) -> Layout<'a> {
    let mut l = Layout {
        name: fallback.to_string(),
        tab: i64::MAX,
        entities,
        page_mm: None,
        limits: None,
        unit_mm: 1.0,
    };
    let Some(o) = object else { return l };
    if let Some(n) = last(o, 1).map(str::trim).filter(|n| !n.is_empty()) {
        l.name = n.to_string();
    }
    l.tab = o.int(71).unwrap_or(i64::MAX);
    // The plot paper units: 0 inches, 1 millimeters, 2 pixels.
    if o.int(72) == Some(0) {
        l.unit_mm = 25.4;
    }
    let (w, h) = (o.fd(44, 0.0), o.fd(45, 0.0));
    if w > 0.0 && h > 0.0 {
        let mm = DVec2::new(w, h) * l.unit_mm;
        // A paper turned by 90 degrees (code 73 is 1 or 3) is wider than high.
        l.page_mm = Some(if matches!(o.int(73), Some(1 | 3)) {
            DVec2::new(mm.y, mm.x)
        } else {
            mm
        });
    }
    // The custom print scale (codes 142 and 143): paper units per unit of the drawing. It sets
    // what a coordinate of paper space is on paper. The paper itself keeps its size.
    if let (Some(n), Some(d)) = (o.f(142), o.f(143))
        && n > 0.0
        && d > 0.0
        && (n / d).is_finite()
        && (n / d - 1.0).abs() > 1e-12
    {
        l.unit_mm *= n / d;
    }
    if let (Some(x0), Some(y0), Some(x1), Some(y1)) = (o.f(10), o.f(20), o.f(11), o.f(21))
        && x1 > x0
        && y1 > y0
    {
        l.limits = Some((DVec2::new(x0, y0), DVec2::new(x1, y1)));
    }
    l
}

/// The layouts of paper space with their entities, in tab order. Layouts of the model are not
/// included.
pub(super) fn layouts(dxf: &Dxf) -> Vec<Layout<'_>> {
    let is_paper = |n: &str| n.to_ascii_lowercase().starts_with("*paper_space");
    let mut names: Vec<&String> = dxf.blocks.keys().filter(|n| is_paper(n)).collect();
    names.sort();
    let mut out: Vec<Layout> = Vec::new();
    let loose: Vec<&Entity> = dxf
        .entities
        .iter()
        .filter(|e| e.int(67) == Some(1))
        .collect();
    let mut loose_used = false;
    for n in &names {
        let block = &dxf.blocks[*n];
        let object = layout_of(dxf, block.record.as_deref());
        // The entities of the first layout are in the section ENTITIES, flagged as paper space.
        let mut entities: Vec<&Entity> = Vec::new();
        if n.eq_ignore_ascii_case("*Paper_Space") {
            entities.extend(loose.iter().copied());
            loose_used = true;
        }
        entities.extend(block.entities.iter());
        let fallback = n.trim_start_matches('*').replace('_', " ");
        out.push(layout_from(object, &fallback, entities));
    }
    if !loose_used && !loose.is_empty() {
        // A file without the block (release 12): all paper space entities are one layout.
        let object = dxf.objects.iter().find(|o| {
            o.kind == "LAYOUT" && last(o, 1).is_some_and(|n| !n.eq_ignore_ascii_case("Model"))
        });
        out.insert(0, layout_from(object, "Paper space", loose));
    }
    out.sort_by(|a, b| a.tab.cmp(&b.tab).then_with(|| a.name.cmp(&b.name)));
    out
}

/// The paper that is nearest to a size in mm, else a custom size.
fn paper_for(page: DVec2) -> (PaperSize, Orientation) {
    let (short, long) = (page.x.min(page.y), page.x.max(page.y));
    let orientation = if page.x >= page.y {
        Orientation::Landscape
    } else {
        Orientation::Portrait
    };
    let named = PaperSize::NAMED.into_iter().find(|p| {
        let d = p.portrait_mm();
        (d.x.min(d.y) - short).abs() <= 1.5 && (d.x.max(d.y) - long).abs() <= 1.5
    });
    (
        named.unwrap_or(PaperSize::Custom {
            width_mm: short,
            height_mm: long,
        }),
        orientation,
    )
}

impl Walker<'_> {
    /// The class of a DXF layer name, with the pen and line weight of the layer. A class that the
    /// layer switches off is remembered for the viewports.
    pub(super) fn class_for(&mut self, name: &str) -> (ClassId, Attributes) {
        let def = self.defs.get(name).copied();
        let visible = def.is_none_or(|d| d.color >= 0 && !d.frozen);
        let attrs = Attributes {
            pen: Some(def.and_then(layer_pen).unwrap_or(Color::BLACK)),
            fill: Some(Color::NONE),
            line_weight: Some(def.and_then(|d| weight_mm(d.lineweight)).unwrap_or(0.25)),
            ..Default::default()
        };
        let class = self.b.class(&decode_percent(name), &attrs, visible);
        self.class_vis.insert(name.to_string(), (class, visible));
        (class, Attributes::default())
    }

    /// Counts the entities of the layout blocks as skipped (the option `paper_space` is off).
    pub(super) fn skip_paper_space(&mut self) {
        let dxf = self.dxf;
        let mut names: Vec<&String> = dxf
            .blocks
            .keys()
            .filter(|n| n.to_ascii_lowercase().starts_with("*paper_space"))
            .collect();
        names.sort();
        for n in names {
            for e in &dxf.blocks[n].entities {
                self.b.report.entities_read += 1;
                self.skip_entity(e, "in paper space");
            }
        }
    }

    /// Imports the layouts of paper space as sheet layers.
    pub(super) fn paper_space(&mut self) {
        let layouts = layouts(self.dxf);
        let mut empty: Vec<String> = Vec::new();
        for lay in &layouts {
            let content = lay
                .entities
                .iter()
                .filter(|e| !(e.kind == "VIEWPORT" && e.int(69) == Some(1)))
                .count();
            if content == 0 {
                // The overall viewport of an empty layout is the sheet itself.
                self.b.report.entities_read += lay.entities.len();
                self.b.report.entities_imported += lay.entities.len();
                empty.push(lay.name.clone());
                continue;
            }
            self.layout(lay);
        }
        self.tally.empty_layouts.extend(empty);
    }

    fn layout(&mut self, lay: &Layout) {
        // The page: the paper of the layout, else the limits, else the A3 sheet.
        let limits = lay.limits;
        let page = lay
            .page_mm
            .or_else(|| limits.map(|(a, b)| (b - a) * lay.unit_mm))
            .filter(|p| {
                p.is_finite()
                    && p.min_element() >= 10.0
                    && p.max_element() <= kadakas_core::sheets::MAX_PAPER
            });
        if page.is_none() {
            self.tally.default_page.push(lay.name.clone());
        }
        let (paper, orientation) = paper_for(page.unwrap_or(DVec2::new(420.0, 297.0)));
        let sheet = self.b.sheet(&lay.name, paper, orientation);
        let viewports_before = self.tally.viewports;
        let origin = limits.map_or(DVec2::ZERO, |(a, _)| a) * lay.unit_mm;
        let xf = Xf {
            cols: [DVec2::new(lay.unit_mm, 0.0), DVec2::new(0.0, lay.unit_mm)],
            t: -origin,
            zs: lay.unit_mm,
            zt: 0.0,
        };
        let root = Cx {
            xf,
            layer: None,
            pen: None,
            weight: None,
            depth: 0,
            path: Path::default(),
            clips: Vec::new(),
            tilt: None,
        };
        // What the sheet needs from the walker: no offset (paper is not shifted), no source tags,
        // and the sheet layer as the target of every object.
        let saved_offset = self.b.offset();
        let saved_tracking = self.tracking.take();
        self.b.set_offset(DVec2::ZERO);
        self.sheet = Some(sheet);
        let clips: std::collections::HashSet<String> = lay
            .entities
            .iter()
            .filter(|e| e.kind == "VIEWPORT")
            .filter_map(|e| e.get(340).and_then(crate::dxf::hash::normal_handle))
            .collect();
        let by_handle: HashMap<String, &Entity> = lay
            .entities
            .iter()
            .filter_map(|e| Some((e.handle()?, *e)))
            .collect();
        let mut viewports_3d = 0;
        for e in &lay.entities {
            if e.kind == "VIEWPORT" {
                self.b.report.entities_read += 1;
                let ok = self.viewport(
                    e,
                    sheet,
                    lay,
                    &root,
                    saved_offset,
                    &by_handle,
                    &mut viewports_3d,
                );
                self.count(ok);
            } else if e.handle().is_some_and(|h| clips.contains(&h)) {
                self.b.report.entities_read += 1;
                self.skip_entity(
                    e,
                    "outline that crops a viewport (it is the crop of the viewport)",
                );
            } else {
                self.entity(e, &root);
            }
        }
        self.sheet = None;
        self.tracking = saved_tracking;
        self.b.set_offset(saved_offset);
        self.tally.sheets.push((
            lay.name.clone(),
            lay.entities.len(),
            self.tally.viewports - viewports_before,
        ));
    }

    /// One VIEWPORT of a layout. False when it gave no viewport (the reason is in the report).
    #[allow(clippy::too_many_arguments)]
    fn viewport(
        &mut self,
        e: &Entity,
        sheet: LayerId,
        lay: &Layout,
        cx: &Cx,
        model_offset: DVec2,
        by_handle: &HashMap<String, &Entity>,
        viewports_3d: &mut usize,
    ) -> bool {
        // Id 1 is the viewport of the paper itself: the sheet.
        if e.int(69) == Some(1) {
            return true;
        }
        if e.int(68) == Some(0) {
            self.b.report.skip("VIEWPORT", "viewport is turned off");
            return false;
        }
        let (w, h) = (e.fd(40, 0.0), e.fd(41, 0.0));
        let view_height = e.fd(45, 0.0);
        if !(w > 0.0 && h > 0.0 && view_height > 0.0) {
            self.b
                .report
                .skip("VIEWPORT", "viewport without a size or a view height");
            return false;
        }
        let unit_mm = cx.xf.zs;
        let model_mm = self.xform.0;
        let center = cx.xf.apply(DVec2::new(e.fd(10, 0.0), e.fd(20, 0.0)));
        let half = DVec2::new(w, h) * unit_mm / 2.0;
        let scale = view_height * model_mm / (h * unit_mm);
        let target = DVec2::new(e.fd(17, 0.0), e.fd(27, 0.0));
        let view_center = DVec2::new(e.fd(12, 0.0), e.fd(22, 0.0));
        let model_center = (target + view_center) * model_mm - model_offset;
        let dir = DVec3::new(e.fd(16, 0.0), e.fd(26, 0.0), e.fd(36, 1.0));
        let (view, exact) = view_for(dir);
        let layers = self.viewport_layers();
        let mut v = Viewport::new(center - half, center + half, model_center, scale, layers);
        v.view = view.to_string();
        v.name = format!("{} {}", lay.name, self.tally.viewports + 1);
        v.show_frame = !e.layer().eq_ignore_ascii_case("defpoints")
            && self
                .defs
                .get(e.layer())
                .is_none_or(|d| d.color >= 0 && !d.frozen);
        // The render mode: 1 wireframe, 2 hidden line, 3 and more shaded.
        v.mode = match e.int(281) {
            Some(1) => "wireframe",
            Some(2) => "hidden_line",
            _ => "shaded",
        }
        .into();
        let (layer_overrides, class_overrides) = self.viewport_visibility(e);
        v.layer_overrides = layer_overrides;
        v.class_overrides = class_overrides;
        if let Some(h) = e.get(340).and_then(crate::dxf::hash::normal_handle)
            && let Some(clip) = by_handle.get(&h)
        {
            match self.clip_polygon(clip, cx) {
                Some(points) => v.crop = Some(Crop::Polygon { points }),
                None => self.tally.viewport_clip_lost += 1,
            }
        }
        if !v.is_plan() {
            if *viewports_3d >= kadakas_core::sheets::MAX_3D_VIEWPORTS {
                self.b.report.skip(
                    "VIEWPORT",
                    "a sheet layer holds at most 8 viewports that show a 3D view",
                );
                return false;
            }
            *viewports_3d += 1;
        }
        if let Some(why) = v.problem() {
            self.b.report.skip(
                "VIEWPORT",
                &format!("viewport is not valid on a sheet: {why}"),
            );
            return false;
        }
        if e.fd(51, 0.0).abs() > 1e-9 {
            self.tally.viewport_twist += 1;
        }
        if e.int(90).unwrap_or(0) & 1 != 0 {
            self.tally.viewport_perspective += 1;
        }
        if !exact {
            *self
                .tally
                .viewport_views
                .entry(view.to_string())
                .or_default() += 1;
        }
        self.tally.viewports += 1;
        self.b.add(
            "VIEWPORT",
            Shape::Viewport(Box::new(v)),
            sheet,
            self.b.default_class,
            Attributes::default(),
            BTreeMap::new(),
        )
    }

    /// The design layers that a viewport shows: the layer of the objects (classes mode), or the
    /// layer of every DXF layer that has objects.
    fn viewport_layers(&self) -> Vec<LayerId> {
        let mut layers: Vec<LayerId> = match self.opts.layer_mode {
            LayerMode::Classes => vec![self.target_layer],
            LayerMode::Layers => self.mapped.values().map(|(l, ..)| *l).collect(),
        };
        layers.sort();
        layers.dedup();
        layers
    }

    /// The layers and classes that a viewport hides: those that the drawing hides, and those that
    /// the viewport freezes (group code 331, or the MVIEW data of old files).
    #[allow(clippy::type_complexity)]
    fn viewport_visibility(
        &self,
        e: &Entity,
    ) -> (BTreeMap<LayerId, bool>, BTreeMap<ClassId, ClassOverride>) {
        let mut frozen: Vec<String> = e
            .items
            .iter()
            .filter(|(c, _)| *c == 331)
            .filter_map(|(_, h)| crate::dxf::hash::normal_handle(h))
            .filter_map(|h| {
                self.dxf
                    .layers
                    .iter()
                    .find(|l| l.handle.as_deref() == Some(h.as_str()))
                    .map(|l| l.name.clone())
            })
            .collect();
        // Releases 13 and 14 keep the names of the frozen layers as extended data of MVIEW.
        if let Some(at) = e
            .items
            .iter()
            .position(|(c, v)| *c == 1001 && v.trim().eq_ignore_ascii_case("MVIEW"))
        {
            frozen.extend(
                e.items[at + 1..]
                    .iter()
                    .take_while(|(c, _)| *c != 1001)
                    .filter(|(c, _)| *c == 1003)
                    .map(|(_, n)| n.trim().to_string()),
            );
        }
        let mut layers = BTreeMap::new();
        let mut classes = BTreeMap::new();
        let hide = ClassOverride {
            visible: Some(false),
            ..ClassOverride::default()
        };
        match self.opts.layer_mode {
            LayerMode::Classes => {
                for (name, (class, visible)) in &self.class_vis {
                    if !visible || frozen.iter().any(|f| f.eq_ignore_ascii_case(name)) {
                        classes.insert(*class, hide.clone());
                    }
                }
            }
            LayerMode::Layers => {
                for (name, (layer, ..)) in &self.mapped {
                    let off = self
                        .defs
                        .get(name.as_str())
                        .is_some_and(|d| d.color < 0 || d.frozen);
                    if off || frozen.iter().any(|f| f.eq_ignore_ascii_case(name)) {
                        layers.insert(*layer, false);
                    }
                }
            }
        }
        (layers, classes)
    }

    /// The crop polygon of a viewport from its clipping entity, in sheet mm. `None` when the
    /// entity is not a closed polyline or a circle.
    fn clip_polygon(&self, clip: &Entity, cx: &Cx) -> Option<Vec<DVec2>> {
        let xf = &cx.xf;
        let pts: Vec<DVec2> = match clip.kind.as_str() {
            "LWPOLYLINE" => {
                let mut raw: Vec<(DVec2, f64)> = Vec::new();
                for (code, value) in &clip.items {
                    match code {
                        10 => raw.push((DVec2::new(parse::parse_f64(value)?, 0.0), 0.0)),
                        20 => raw.last_mut()?.0.y = parse::parse_f64(value)?,
                        42 => raw.last_mut()?.1 = parse::parse_f64(value)?,
                        _ => {}
                    }
                }
                // An arc (a bulge) is cut into short chords, within the tolerance of the import.
                let tol = self.opts.tolerance / xf.length_scale().max(1e-12);
                let n = raw.len();
                let mut v: Vec<DVec2> = Vec::new();
                for i in 0..n {
                    v.push(raw[i].0);
                    let (a, b) = (raw[i].0, raw[(i + 1) % n].0);
                    if raw[i].1.abs() > 1e-9 && a.distance(b) > 1e-12 && n >= 2 {
                        let seg = bulge_points(a, b, raw[i].1, tol);
                        if seg.len() > 2 {
                            v.extend(seg[1..seg.len() - 1].iter().copied());
                        }
                    }
                }
                v
            }
            "CIRCLE" => {
                let c = DVec2::new(clip.fd(10, 0.0), clip.fd(20, 0.0));
                let r = clip.f(40)?;
                (0..64)
                    .map(|i| c + DVec2::from_angle(TAU * f64::from(i) / 64.0) * r)
                    .collect()
            }
            _ => return None,
        };
        (pts.len() >= 3).then(|| pts.into_iter().map(|p| xf.apply(p)).collect())
    }

    /// The shapes of an entity on a sheet layer: a sheet holds 2D shapes. A 3D polyline is
    /// flattened, other 3D shapes are left out (the walker reports them).
    pub(super) fn sheet_items(&mut self, items: Vec<Item>) -> Vec<Item> {
        let mut out = Vec::new();
        for mut item in items {
            if let Shape::Polyline3d { points, closed } = &item.shape {
                let pts: Vec<DVec2> = points.iter().map(|p| p.truncate()).collect();
                item.shape = Shape::Polyline {
                    points: corners(&pts),
                    closed: *closed,
                };
                self.tally.flattened += 1;
            }
            if kadakas_core::sheets::allowed_on_sheet(&item.shape) {
                out.push(item);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn papers_are_named_or_custom() {
        assert_eq!(
            paper_for(DVec2::new(420.0, 297.0)),
            (PaperSize::A3, Orientation::Landscape)
        );
        assert_eq!(
            paper_for(DVec2::new(297.0, 420.0)),
            (PaperSize::A3, Orientation::Portrait)
        );
        assert_eq!(paper_for(DVec2::new(841.0, 594.0)).0, PaperSize::A1);
        assert_eq!(paper_for(DVec2::new(279.4, 215.9)).0, PaperSize::Letter);
        assert!(matches!(
            paper_for(DVec2::new(500.0, 300.0)).0,
            PaperSize::Custom { width_mm, height_mm } if width_mm == 300.0 && height_mm == 500.0
        ));
    }
}
