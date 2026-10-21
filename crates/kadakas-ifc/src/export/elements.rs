//! One handler per kind of object: geometry, element, standard property sets and quantities.

use super::*;
use crate::step::format_real;

/// The geometry of a hardscape, computed before anything is written.
struct SlabGeometry {
    ring: Vec<DVec2>,
    /// Holes in the slab (open parts): the profile has voids and the walls close them.
    holes: Vec<Vec<DVec2>>,
    thickness: f64,
    /// Height of a flat top surface. `None`: the top is sloped or draped, and `mesh` holds the solid.
    flat_top: Option<f64>,
    mesh: Option<(Vec<DVec3>, Vec<u32>)>,
    layers: Vec<(String, f64)>,
    quantities: hardscape::report::Quantities,
    grade: f64,
}

impl<W: Write> Exporter<'_, W> {
    pub(super) fn object(&mut self, obj: &Object) -> Outcome {
        match &obj.shape {
            Shape::Polyline { .. } if kadakas_site::grading::wall::data(obj).is_some() => {
                self.wall(obj)
            }
            Shape::Hardscape { .. } => self.hardscape(obj),
            Shape::LandscapeArea { .. } => self.landscape_area(obj),
            Shape::Plant { .. } | Shape::ExistingTree { .. } => self.plant(obj),
            Shape::SiteModel { .. } => self.terrain(obj),
            Shape::Massing(_) => self.massing(obj),
            Shape::Mesh3d(_) => self.mesh3d(obj),
            Shape::Extrude(_) | Shape::Sweep(_) => self.solid(obj),
            Shape::Underlay(_) => skip("map underlay (a picture)"),
            Shape::SurfaceModel(_) => {
                skip("surface model: IFC export of surface models is not done yet (MOD-12)")
            }
            Shape::Component { .. } => {
                skip("component: IFC export of components is not done yet (MOD-12)")
            }
            Shape::SiteModifier {
                modifier: kadakas_core::site::ModifierKind::TextureBed { .. },
                ..
            } => skip("texture bed: IFC has no textures, the terrain keeps its plain color"),
            Shape::SiteModifier { .. } => skip("site modifier: its effect is in the terrain"),
            Shape::Viewport(_) | Shape::TitleBlock(_) => skip("sheet object (paper space)"),
            Shape::Irrigation(_) => skip(
                "irrigation object: IFC export does not write irrigation yet (the DXF export does)",
            ),
            Shape::Locus3d { .. } | Shape::Polyline3d { .. } if !self.opts.include_2d => skip(
                "survey geometry: the terrain carries it (set include_2d to export it as IfcAnnotation)",
            ),
            Shape::Line { .. }
            | Shape::Rect { .. }
            | Shape::Circle { .. }
            | Shape::Arc { .. }
            | Shape::Polyline { .. }
            | Shape::Text { .. }
            | Shape::PlantTag { .. }
                if !self.opts.include_2d =>
            {
                skip("2D object (set include_2d to export it as IfcAnnotation)")
            }
            _ => self.annotation(obj),
        }
    }

    // ------------------------------------------------------------ bodies

    fn polyline2(&mut self, pts: &[DVec2], close: bool) -> R<u64> {
        let mut ids = Vec::with_capacity(pts.len() + 1);
        for p in pts {
            ids.push(self.pt2(*p)?);
        }
        if close && let Some(first) = ids.first().copied() {
            ids.push(first);
        }
        self.ent("IFCPOLYLINE", vec![Value::refs(ids)])
    }

    pub(super) fn profile(&mut self, outer: &[DVec2], holes: &[Vec<DVec2>]) -> R<u64> {
        let curve = self.polyline2(outer, true)?;
        if holes.is_empty() {
            return self.ent(
                "IFCARBITRARYCLOSEDPROFILEDEF",
                vec![Value::e("AREA"), Value::Null, Value::Ref(curve)],
            );
        }
        let mut inner = Vec::new();
        for h in holes {
            inner.push(self.polyline2(h, true)?);
        }
        self.ent(
            "IFCARBITRARYPROFILEDEFWITHVOIDS",
            vec![
                Value::e("AREA"),
                Value::Null,
                Value::Ref(curve),
                Value::refs(inner),
            ],
        )
    }

    /// An extrusion upward from height `z0` of the placement.
    pub(super) fn extrude(&mut self, profile: u64, z0: f64, depth: f64) -> R<u64> {
        let position = self.axis3([0.0, 0.0, z0])?;
        let up = self.up()?;
        self.ent(
            "IFCEXTRUDEDAREASOLID",
            vec![
                Value::Ref(profile),
                Value::Ref(position),
                Value::Ref(up),
                Value::Real(r4(depth)),
            ],
        )
    }

    /// An `IfcTriangulatedFaceSet`, written straight to the file.
    pub(super) fn tessellation(&mut self, points: &[DVec3], idx: &[u32]) -> R<u64> {
        if self.schema == Schema::Ifc2x3 {
            return self.surface_model(points, idx);
        }
        let list = self.w.entity_with("IFCCARTESIANPOINTLIST3D", |out| {
            out.write_all(b"(")?;
            for (i, p) in points.iter().enumerate() {
                if i > 0 {
                    out.write_all(b",")?;
                }
                write!(
                    out,
                    "({},{},{})",
                    format_real(r4(p.x)),
                    format_real(r4(p.y)),
                    format_real(r4(p.z))
                )?;
            }
            out.write_all(b")")
        })?;
        self.w
            .entity_with("IFCTRIANGULATEDFACESET", |out| {
                write!(out, "#{list},$,$,(")?;
                let mut first = true;
                for t in idx.as_chunks::<3>().0 {
                    if t[0] == t[1] || t[1] == t[2] || t[0] == t[2] {
                        continue;
                    }
                    if !first {
                        out.write_all(b",")?;
                    }
                    first = false;
                    write!(out, "({},{},{})", t[0] + 1, t[1] + 1, t[2] + 1)?;
                }
                out.write_all(b"),$")
            })
            .map_err(Into::into)
    }

    /// A mesh for IFC2x3: an `IfcFaceBasedSurfaceModel` of triangular faces.
    fn surface_model(&mut self, points: &[DVec3], idx: &[u32]) -> R<u64> {
        let mut pts = Vec::with_capacity(points.len());
        for p in points {
            pts.push(self.pt3([p.x, p.y, p.z])?);
        }
        let mut faces = Vec::new();
        for t in idx.as_chunks::<3>().0 {
            if t[0] == t[1] || t[1] == t[2] || t[0] == t[2] {
                continue;
            }
            let corners = t.iter().map(|i| pts[*i as usize]);
            let poly = self.ent("IFCPOLYLOOP", vec![Value::refs(corners)])?;
            let bound = self.ent(
                "IFCFACEOUTERBOUND",
                vec![Value::Ref(poly), Value::Bool(true)],
            )?;
            faces.push(self.ent("IFCFACE", vec![Value::refs([bound])])?);
        }
        let set = self.ent("IFCCONNECTEDFACESET", vec![Value::refs(faces)])?;
        self.ent("IFCFACEBASEDSURFACEMODEL", vec![Value::refs([set])])
    }

    pub(super) fn color_of(&self, obj: &Object, default: [u8; 4]) -> [u8; 4] {
        let a = hardscape::styled_attrs(self.doc, obj, self.doc.resolved_attrs(obj));
        a.fill.filter(|c| c.0[3] > 0).map_or(default, |c| c.0)
    }

    // ------------------------------------------------------------ hardscape

    fn slab_geometry(&self, obj: &Object) -> Result<SlabGeometry, &'static str> {
        let Shape::Hardscape { boundary, .. } = &obj.shape else {
            return Err("not a hardscape");
        };
        // The holes are merged and cut to the outline. A hole that touches the outline is part of
        // it then, so a void never touches the outer curve. Holes that split the slab in two
        // are refused.
        let hole_list = hardscape::data(obj).map(|d| d.holes).unwrap_or_default();
        let region = kadakas_core::layout::Region::new(boundary, &hole_list, 2.0)
            .map_err(|_| "the outline has no area, or the holes cover all of it")?;
        if region.shapes.len() != 1 {
            return Err("the holes split the slab into several parts");
        }
        let ring = geom::clean_ring(&region.shapes[0][0]);
        if ring.is_empty() {
            return Err("the outline has no area");
        }
        let holes: Vec<Vec<DVec2>> = region.shapes[0]
            .iter()
            .skip(1)
            .map(|h| geom::clean_ring(h))
            .filter(|h| !h.is_empty())
            .collect();
        let q =
            hardscape::report::quantities_on(self.doc, obj, &self.ground).ok_or("no quantities")?;
        let layers: Vec<(String, f64)> = q
            .layers
            .iter()
            .filter(|l| l.kind != "border")
            .map(|l| {
                (
                    if l.material.trim().is_empty() {
                        l.name.clone()
                    } else {
                        l.material.clone()
                    },
                    l.thickness_mm,
                )
            })
            .collect();
        let thickness: f64 = layers.iter().map(|l| l.1).sum();
        if !(thickness.is_finite() && thickness > 0.0) {
            return Err("the thickness is zero");
        }
        let d = hardscape::data(obj);
        let at = |p: DVec2| self.ground.elevation(obj, p);
        let surf = surface::surface_of(
            obj,
            d.as_ref(),
            q.draped.then_some(&at as hardscape::Terrain<'_>),
        );
        let grade = surf.plane.a.hypot(surf.plane.b);
        let (flat_top, mesh) = if !surf.is_draped() && grade < 1.0e-9 {
            (Some(surf.z(ring[0])), None)
        } else {
            let z = |p: DVec2| surf.z(p);
            let step = surf.is_draped().then_some(geom::STEP);
            (
                None,
                Some(geom::solid_mesh(&ring, &holes, &z, thickness, step)),
            )
        };
        Ok(SlabGeometry {
            ring,
            holes,
            thickness,
            flat_top,
            mesh,
            layers,
            quantities: q,
            grade: if surf.is_draped() { 0.0 } else { grade },
        })
    }

    fn hardscape(&mut self, obj: &Object) -> Outcome {
        let doc = self.doc;
        let g = match self.slab_geometry(obj) {
            Ok(g) => g,
            Err(reason) => return skip(reason),
        };
        let mapped = datamap::evaluate(doc, &self.map, obj);
        let placement = self.place_in_layer(obj.layer, [0.0, 0.0, g.flat_top.unwrap_or(0.0)])?;
        let (item, rep_type) = match &g.mesh {
            None => {
                let profile = self.profile(&g.ring, &g.holes)?;
                (
                    self.extrude(profile, -g.thickness, g.thickness)?,
                    "SweptSolid",
                )
            }
            Some((points, idx)) => (self.tessellation(points, idx)?, "Tessellation"),
        };
        self.style_items(&[item], self.color_of(obj, hardscape::DEFAULT_COLOR))?;
        let rep = self.body_rep(rep_type, &[item])?;
        let name = hardscape::data::style_of(doc, obj)
            .and_then(|(sid, _)| doc.resources.get(sid).map(|r| r.name.clone()))
            .unwrap_or_else(|| self.object_name(obj));
        let (id, class) = self.element(
            ElementIn {
                obj,
                kind: "hardscape",
                name,
                placement,
                body: Body {
                    reps: vec![rep],
                    main: Some(rep),
                },
            },
            &mapped,
        )?;

        let style_name = hardscape::data::style_of(doc, obj)
            .and_then(|(sid, _)| doc.resources.get(sid).map(|r| r.name.clone()));
        let label = style_name.clone().unwrap_or_default();
        self.associate_layers(id, &label, &g.layers, g.mesh.is_none())?;

        let reference = style_name.unwrap_or_else(|| match &obj.shape {
            Shape::Hardscape { material, .. } => material.clone(),
            _ => String::new(),
        });
        let mut common = Vec::new();
        if !reference.trim().is_empty() {
            common.push((
                "Reference".to_string(),
                Value::typed("IFCIDENTIFIER", Value::s(reference.trim())),
            ));
        }
        common.push((
            "IsExternal".to_string(),
            Value::typed("IFCBOOLEAN", Value::Bool(true)),
        ));
        if class == "IFCSLAB" {
            common.push((
                "LoadBearing".to_string(),
                Value::typed("IFCBOOLEAN", Value::Bool(false)),
            ));
            if g.grade > 1.0e-9 {
                common.push((
                    "PitchAngle".to_string(),
                    Value::typed("IFCPLANEANGLEMEASURE", Value::Real(r4(g.grade.atan()))),
                ));
            }
            self.pset("Pset_SlabCommon", common);
        } else if class == "IFCCOVERING" {
            self.pset("Pset_CoveringCommon", common);
        }

        let q = &g.quantities;
        // A standard quantity set only for the classes that have one. Another class (a wall, a
        // proxy) gets the Kadakas quantities alone.
        let base_name = match class {
            "IFCCOVERING" => Some("Qto_CoveringBaseQuantities"),
            "IFCSLAB" => Some("Qto_SlabBaseQuantities"),
            _ => None,
        };
        let mut standard = vec![
            (
                "area",
                "GrossArea".to_string(),
                q.plan_area_m2 + q.holes_area_m2,
            ),
            ("area", "NetArea".to_string(), q.plan_area_m2),
        ];
        if class == "IFCSLAB" {
            standard.insert(0, ("length", "Depth".to_string(), g.thickness));
            standard.insert(
                1,
                ("length", "Perimeter".to_string(), q.perimeter_m * 1000.0),
            );
            standard.push(("volume", "GrossVolume".to_string(), q.volume_m3));
            standard.push(("volume", "NetVolume".to_string(), q.volume_m3));
        }
        if let Some(base_name) = base_name {
            self.quantities(obj, base_name, &standard)?;
        }
        let mut extra = vec![("area", "SurfaceArea".to_string(), q.surface_area_m2)];
        for (i, l) in q.layers.iter().enumerate() {
            extra.push((
                "volume",
                format!("Volume {} {}", i + 1, l.name.trim()),
                l.volume_m3,
            ));
            extra.push((
                "area",
                format!("Area {} {}", i + 1, l.name.trim()),
                l.area_m2,
            ));
        }
        self.quantities(obj, "Kadakas_HardscapeQuantities", &extra)?;
        self.data_psets(obj, &mapped)?;
        Ok(None)
    }

    // ------------------------------------------------------------ landscape area

    fn landscape_area(&mut self, obj: &Object) -> Outcome {
        let doc = self.doc;
        let Shape::LandscapeArea { boundary, .. } = &obj.shape else {
            return skip("not a landscape area");
        };
        let ring = geom::clean_ring(&curve::flatten(boundary, true, 2.0));
        if ring.is_empty() {
            return skip("the outline has no area");
        }
        let data = landscape::data(obj).unwrap_or_default();
        let holes: Vec<Vec<DVec2>> = data
            .holes
            .iter()
            .map(|h| geom::clean_ring(&curve::flatten(h, true, 2.0)))
            .filter(|h| !h.is_empty())
            .collect();
        let thickness = self.opts.area_thickness;
        // A draped area follows the site model. Its holes are left out of the mesh.
        let covered = data.drape
            && ring
                .iter()
                .step_by((ring.len() / 64).max(1))
                .any(|p| self.ground.covered(obj, *p));
        let mesh = if covered {
            let z = |p: DVec2| self.ground.elevation(obj, p).unwrap_or(0.0) + surface::LIFT;
            Some(geom::solid_mesh(
                &ring,
                &holes,
                &z,
                thickness,
                Some(geom::STEP),
            ))
        } else {
            None
        };
        let mapped = datamap::evaluate(doc, &self.map, obj);
        let placement = self.place_in_layer(obj.layer, [0.0; 3])?;
        let (item, rep_type) = match &mesh {
            None => {
                let profile = self.profile(&ring, &holes)?;
                (self.extrude(profile, -thickness, thickness)?, "SweptSolid")
            }
            Some((points, idx)) => (self.tessellation(points, idx)?, "Tessellation"),
        };
        self.style_items(&[item], self.color_of(obj, DEFAULT_GREEN))?;
        let rep = self.body_rep(rep_type, &[item])?;
        let name = self.object_name(obj);
        self.element(
            ElementIn {
                obj,
                kind: "landscape_area",
                name,
                placement,
                body: Body {
                    reps: vec![rep],
                    main: Some(rep),
                },
            },
            &mapped,
        )?;

        if let Some(q) = landscape::quantities(doc, obj) {
            let mut list = vec![
                ("area", "GrossArea".to_string(), q.gross_area_m2),
                ("area", "HolesArea".to_string(), q.holes_area_m2),
                ("area", "NetArea".to_string(), q.net_area_m2),
            ];
            let mut mix = Vec::new();
            for row in &q.rows {
                list.push((
                    "count",
                    format!("Count {}", row.name.trim()),
                    row.count as f64,
                ));
                mix.push((
                    row.name.trim().to_string(),
                    Value::typed(
                        "IFCLABEL",
                        Value::s(format!(
                            "{} plants, {}%",
                            row.count,
                            format_real(r4(row.percent))
                        )),
                    ),
                ));
            }
            self.quantities(obj, "Kadakas_AreaQuantities", &list)?;
            self.pset("Kadakas_PlantMix", mix);
        }
        self.data_psets(obj, &mapped)?;
        Ok(None)
    }

    // ------------------------------------------------------------ plants and trees

    fn plant(&mut self, obj: &Object) -> Outcome {
        let doc = self.doc;
        let (at, spread, height, trunk_d, category) = match &obj.shape {
            Shape::Plant {
                at, spread, height, ..
            } => {
                let e = link::effective(doc, obj);
                let cat = e
                    .as_ref()
                    .map_or(PlantCategory::Shrub, |e| e.style.category);
                let trunk = e.as_ref().map_or(0.0, |e| e.style.trunk_diameter);
                (*at, *spread, *height, trunk, cat)
            }
            Shape::ExistingTree {
                at,
                spread,
                height,
                dbh,
                ..
            } => (*at, *spread, *height, *dbh, PlantCategory::Tree),
            _ => return skip("not a plant"),
        };
        if !(at.is_finite() && spread.is_finite() && height.is_finite()) {
            return skip("the size is not finite");
        }
        let (spread, height) = (spread.max(0.0), height.max(0.0));
        // The plant stands on the site model where there is one.
        let ground = if self.ground.covered(obj, at) {
            self.ground.elevation(obj, at).unwrap_or(0.0)
        } else {
            0.0
        };
        let mapped = datamap::evaluate(doc, &self.map, obj);
        let placement = self.place_in_layer(obj.layer, [at.x, at.y, ground])?;

        let mut reps = Vec::new();
        let mut main = None;
        if spread >= MIN_SIZE && height >= MIN_SIZE {
            let is_tree = category == PlantCategory::Tree;
            let trunk_height = if is_tree { height * 0.35 } else { 0.0 };
            let fill = link::look(doc, obj)
                .fill
                .filter(|c| c.0[3] > 0)
                .map(|c| c.0);
            let canopy_color = self.color_of(obj, fill.unwrap_or(DEFAULT_GREEN));
            let mut items = Vec::new();
            if trunk_height > 0.0 {
                let radius = if trunk_d > 0.0 {
                    trunk_d / 2.0
                } else {
                    (spread * 0.03).max(25.0)
                }
                .min(spread / 2.0)
                .max(1.0);
                let profile = self.ent(
                    "IFCCIRCLEPROFILEDEF",
                    vec![
                        Value::e("AREA"),
                        Value::Null,
                        Value::Null,
                        Value::Real(r4(radius)),
                    ],
                )?;
                let trunk = self.extrude(profile, 0.0, trunk_height)?;
                self.style_items(&[trunk], TRUNK_BROWN)?;
                items.push(trunk);
            }
            let profile = self.ent(
                "IFCCIRCLEPROFILEDEF",
                vec![
                    Value::e("AREA"),
                    Value::Null,
                    Value::Null,
                    Value::Real(r4(spread / 2.0)),
                ],
            )?;
            let canopy = self.extrude(profile, trunk_height, height - trunk_height)?;
            self.style_items(&[canopy], canopy_color)?;
            items.push(canopy);
            let rep = self.body_rep("SweptSolid", &items)?;
            main = Some(rep);
            reps.push(rep);
        }
        // The bounding box is the fallback for viewers that skip the solids, and the only body of a plant with no size.
        let (w, h) = (spread.max(MIN_SIZE), height.max(MIN_SIZE));
        let corner = self.pt3([-w / 2.0, -w / 2.0, 0.0])?;
        let bbox = self.ent(
            "IFCBOUNDINGBOX",
            vec![
                Value::Ref(corner),
                Value::Real(r4(w)),
                Value::Real(r4(w)),
                Value::Real(r4(h)),
            ],
        )?;
        reps.push(self.shape_rep("Box", "Model", "MODEL_VIEW", "Box", "BoundingBox", &[bbox])?);
        if main.is_none() {
            self.note(format!(
                "plant {} has no size: only its bounding box is written",
                obj.id
            ));
        }

        let name = self.object_name(obj);
        let kind = obj.shape.kind();
        self.element(
            ElementIn {
                obj,
                kind,
                name,
                placement,
                body: Body { reps, main },
            },
            &mapped,
        )?;
        // The botanical name stays in the file, so an import can give the plant its species back.
        let species = match &obj.shape {
            Shape::Plant { species, .. } | Shape::ExistingTree { species, .. } => species.trim(),
            _ => "",
        };
        if !species.is_empty() {
            self.pset(
                "Kadakas_Plant",
                vec![(
                    "Species".into(),
                    Value::typed("IFCLABEL", Value::s(species)),
                )],
            );
        }
        if let Shape::ExistingTree {
            condition,
            action,
            dbh,
            rpz_factor,
            rpz_cap,
            ..
        } = &obj.shape
        {
            let radius = tree::rpz_radius(*dbh, *rpz_factor, *rpz_cap);
            self.pset(
                "Kadakas_Tree",
                vec![
                    (
                        "Condition".into(),
                        Value::typed("IFCLABEL", Value::s(condition.id())),
                    ),
                    (
                        "Action".into(),
                        Value::typed("IFCLABEL", Value::s(action.id())),
                    ),
                    (
                        "RootProtectionRadius".into(),
                        Value::typed("IFCLENGTHMEASURE", Value::Real(r4(radius))),
                    ),
                ],
            );
        }
        self.data_psets(obj, &mapped)?;
        Ok(None)
    }

    // ------------------------------------------------------------ generic 3D mesh

    /// A generic 3D mesh (for example from an IFC import) as a closed or open `IfcTriangulatedFaceSet`.
    /// The class is `IfcBuildingElementProxy`, or the class in the object data when the writer can make it.
    fn mesh3d(&mut self, obj: &Object) -> Outcome {
        let Shape::Mesh3d(m) = &obj.shape else {
            return skip("not a mesh");
        };
        if let Some(why) = m.problem() {
            return skip(&why);
        }
        let mut mapped = datamap::evaluate(self.doc, &self.map, obj);
        if mapped.ifc_class.is_none() {
            // The class of the source file, when this writer can make it.
            let class = obj.data.get("ifc_class").and_then(|v| v.as_str());
            if let Some(c) = class.filter(|c| ["IfcSlab", "IfcCovering"].contains(c)) {
                mapped.ifc_class = Some(c.to_string());
                mapped.predefined_type = obj
                    .data
                    .get("ifc")
                    .and_then(|i| i.get("predefined_type"))
                    .and_then(|p| p.as_str())
                    .map(String::from);
            }
        }
        let placement = self.place_in_layer(obj.layer, [0.0; 3])?;
        // The vertices are absolute; the placement carries the elevation of the layer.
        let base = self.base_z(obj.layer);
        let points: Vec<DVec3> = m
            .vertices
            .iter()
            .map(|p| DVec3::new(p.x, p.y, p.z - base))
            .collect();
        let idx: Vec<u32> = m.triangles.iter().flatten().copied().collect();
        let item = self.tessellation(&points, &idx)?;
        let default = m.color.map_or(MESH_GREY, |c| c.0);
        self.style_items(&[item], self.color_of(obj, default))?;
        let rep = self.body_rep("Tessellation", &[item])?;
        let name = if m.name.trim().is_empty() {
            self.object_name(obj)
        } else {
            m.name.trim().to_string()
        };
        self.element(
            ElementIn {
                obj,
                kind: "mesh3d",
                name,
                placement,
                body: Body {
                    reps: vec![rep],
                    main: Some(rep),
                },
            },
            &mapped,
        )?;
        self.data_psets(obj, &mapped)?;
        Ok(None)
    }

    // ------------------------------------------------------------ terrain

    fn terrain(&mut self, obj: &Object) -> Outcome {
        let doc = self.doc;
        let site = match SiteResult::compute(doc, obj.id) {
            Ok(s) => s,
            Err(e) => return Ok(Some(format!("no surface: {e}"))),
        };
        let tin = site.surface(Surface::Proposed);
        let mapped = datamap::evaluate(doc, &self.map, obj);
        let placement = self.place_in_layer(obj.layer, [0.0; 3])?;
        let mut idx = Vec::with_capacity(tin.triangles.len() * 3);
        for t in &tin.triangles {
            idx.extend_from_slice(t);
        }
        let item = self.tessellation(&tin.vertices, &idx)?;
        let default = match &obj.shape {
            Shape::SiteModel { settings, .. } => settings
                .proposed_color
                .or(settings.existing_color)
                .map_or(TERRAIN_BROWN, |c| c.0),
            _ => TERRAIN_BROWN,
        };
        self.style_items(&[item], self.color_of(obj, default))?;
        let rep = self.body_rep("Tessellation", &[item])?;
        let name = self.object_name(obj);
        self.element(
            ElementIn {
                obj,
                kind: "site_model",
                name,
                placement,
                body: Body {
                    reps: vec![rep],
                    main: Some(rep),
                },
            },
            &mapped,
        )?;
        let (lo, hi) = tin.z_range();
        self.quantities(
            obj,
            "Kadakas_TerrainQuantities",
            &[
                ("area", "ProjectedArea".into(), tin.area_xy() / 1.0e6),
                ("length", "MinElevation".into(), lo),
                ("length", "MaxElevation".into(), hi),
                ("count", "Triangles".into(), tin.triangles.len() as f64),
            ],
        )?;
        self.data_psets(obj, &mapped)?;
        Ok(None)
    }

    // ------------------------------------------------------------ 2D objects and survey points

    fn annotation(&mut self, obj: &Object) -> Outcome {
        let doc = self.doc;
        let tol = 1.0;
        let (rep_type, items): (&str, Vec<u64>) = match &obj.shape {
            Shape::Line { a, b } => ("Curve2D", vec![self.polyline2(&[*a, *b], false)?]),
            Shape::Rect { min, max } => (
                "Curve2D",
                vec![self.polyline2(
                    &[
                        *min,
                        DVec2::new(max.x, min.y),
                        *max,
                        DVec2::new(min.x, max.y),
                    ],
                    true,
                )?],
            ),
            Shape::Circle { center, radius } => (
                "Curve2D",
                vec![self.polyline2(
                    &geom::arc_points(*center, *radius, 0.0, std::f64::consts::TAU, tol),
                    false,
                )?],
            ),
            Shape::Arc {
                center,
                radius,
                start,
                end,
            } => (
                "Curve2D",
                vec![self.polyline2(
                    &geom::arc_points(*center, *radius, *start, arc_sweep(*start, *end), tol),
                    false,
                )?],
            ),
            Shape::Polyline { points, closed } => (
                "Curve2D",
                vec![self.polyline2(&curve::flatten(points, *closed, tol), *closed)?],
            ),
            Shape::PlantTag { at, anchor, .. } => {
                ("Curve2D", vec![self.polyline2(&[*at, *anchor], false)?])
            }
            Shape::Text { at, text, .. } => {
                let a = self.axis3([at.x, at.y, 0.0])?;
                (
                    "Annotation2D",
                    vec![self.ent(
                        "IFCTEXTLITERAL",
                        vec![Value::s(text), Value::Ref(a), Value::e("LEFT")],
                    )?],
                )
            }
            Shape::Locus3d { p } => ("Point", vec![self.pt3([p.x, p.y, p.z])?]),
            Shape::Polyline3d { points, closed } => {
                let mut ids = Vec::new();
                for p in points {
                    ids.push(self.pt3([p.x, p.y, p.z])?);
                }
                if *closed && let Some(f) = ids.first().copied() {
                    ids.push(f);
                }
                (
                    "Curve3D",
                    vec![self.ent("IFCPOLYLINE", vec![Value::refs(ids)])?],
                )
            }
            _ => return skip("not an annotation"),
        };
        let mapped = datamap::evaluate(doc, &self.map, obj);
        let rep = self.shape_rep(
            "Annotation",
            "Plan",
            "PLAN_VIEW",
            "Annotation",
            rep_type,
            &items,
        )?;
        let placement = self.place_in_layer(obj.layer, [0.0; 3])?;
        let name = match &obj.shape {
            Shape::Text { text, .. } => text.clone(),
            _ => self.object_name(obj),
        };
        let shape = self.ent(
            "IFCPRODUCTDEFINITIONSHAPE",
            vec![Value::Null, Value::Null, Value::refs([rep])],
        )?;
        let id = self.ent(
            "IFCANNOTATION",
            vec![
                self.guid_of("obj", obj.id.0),
                Value::Null,
                Self::name_value(&name),
                Value::Null,
                Value::Null,
                Value::Ref(placement),
                Value::Ref(shape),
            ],
        )?;
        self.register(obj, id, Some(rep))?;
        self.data_psets(obj, &mapped)?;
        Ok(None)
    }
}
