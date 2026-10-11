//! Resource libraries: who uses a resource, what a resource needs, purging unused resources, and
//! importing resources, classes and layers from another document.
//!
//! These functions work for every kind of resource and do not change a document. They make
//! commands (`Command::Batch`), and the host runs them, so every import and purge is one undo step.

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::command::Command;
use crate::model::{Attributes, ClassId, Document, LayerKind, Object, ObjectId, Shape};
use crate::resource::{Resource, ResourceBody, ResourceId};
use crate::uid::IdGen;

/// What uses a resource.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Usage {
    pub objects: Vec<ObjectId>,
    pub classes: Vec<ClassId>,
    /// Other resources that refer to it (a tile that uses a symbol, a landscape style with a plant).
    pub resources: Vec<ResourceId>,
}

impl Usage {
    pub fn total(&self) -> usize {
        self.objects.len() + self.classes.len() + self.resources.len()
    }

    pub fn is_unused(&self) -> bool {
        self.total() == 0
    }
}

/// The resources that a resource needs, in the order of its fields.
pub fn body_refs(body: &ResourceBody) -> Vec<ResourceId> {
    match body {
        ResourceBody::Graphic(g) => g.refs().into_iter().map(|(r, _)| r).collect(),
        ResourceBody::LandscapeAreaStyle(s) => s.mix.iter().map(|e| e.plant).collect(),
        ResourceBody::PlantStyle(s) => s.plan_symbol.into_iter().chain(s.image).collect(),
        _ => Vec::new(),
    }
}

/// The same body with every reference `r` replaced by `f(r)`.
pub fn remap_body(body: &ResourceBody, f: &dyn Fn(ResourceId) -> ResourceId) -> ResourceBody {
    let mut b = body.clone();
    match &mut b {
        ResourceBody::Graphic(g) => g.remap(f),
        ResourceBody::LandscapeAreaStyle(s) => s.mix.iter_mut().for_each(|e| e.plant = f(e.plant)),
        ResourceBody::PlantStyle(s) => {
            s.plan_symbol = s.plan_symbol.map(f);
            s.image = s.image.map(f);
        }
        _ => {}
    }
    b
}

fn object_uses(o: &Object, id: ResourceId, tag: &Value) -> bool {
    crate::plants::link::style_of(o) == Some(id)
        || matches!(o.shape, Shape::PlantTag { style, .. } if style == id)
        || matches!(o.shape, Shape::Symbol { symbol, .. } if symbol == id)
        || crate::hardscape::style_id(o) == Some(id)
        || crate::landscape::style_id(o) == Some(id)
        || o.attrs.fill_pattern == Some(id)
        || o.attrs.line_type == Some(id)
        || crate::worksheet_table::data(o).is_some_and(|d| d.worksheet == id)
        || o.data
            .values()
            .any(|v| v.get(crate::record::MARKER) == Some(tag))
}

/// Who uses the resource. A resource that does not exist has no users.
pub fn usage(doc: &Document, id: ResourceId) -> Usage {
    let tag = Value::from(id.0);
    Usage {
        objects: doc
            .objects
            .values()
            .filter(|o| object_uses(o, id, &tag))
            .map(|o| o.id)
            .collect(),
        classes: doc
            .classes
            .iter()
            .filter(|c| c.attrs.fill_pattern == Some(id) || c.attrs.line_type == Some(id))
            .map(|c| c.id)
            .collect(),
        resources: doc
            .resources
            .iter()
            .filter(|r| r.id != id && body_refs(&r.body).contains(&id))
            .map(|r| r.id)
            .collect(),
    }
}

/// One line of the Resource Manager.
#[derive(Clone, Debug, PartialEq)]
pub struct ResourceInfo {
    pub id: ResourceId,
    pub name: String,
    pub type_name: &'static str,
    /// Number of objects, classes and resources that use it.
    pub used_by: usize,
    pub depends_on: Vec<ResourceId>,
}

/// The resources of a document, filtered by type and by a search text (a part of the name or of the
/// type, compared without case). In creation order.
pub fn list(doc: &Document, type_filter: Option<&str>, search: Option<&str>) -> Vec<ResourceInfo> {
    let search = search
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty());
    let mut uses: HashMap<ResourceId, usize> = HashMap::new();
    // Count once for all resources: objects are many, resources are few.
    let ids: HashSet<ResourceId> = doc.resources.iter().map(|r| r.id).collect();
    if !ids.is_empty() {
        for r in doc.resources.iter() {
            for dep in body_refs(&r.body) {
                if dep != r.id {
                    *uses.entry(dep).or_default() += 1;
                }
            }
        }
        for c in &doc.classes {
            for r in [c.attrs.fill_pattern, c.attrs.line_type]
                .into_iter()
                .flatten()
            {
                *uses.entry(r).or_default() += 1;
            }
        }
        let tags: HashMap<String, ResourceId> = doc
            .resources
            .iter()
            .map(|r| (format!("\"{}\"", r.id.0), r.id))
            .collect();
        for o in doc.objects.values() {
            let mut seen: HashSet<ResourceId> = HashSet::new();
            let mut hit = |r: Option<ResourceId>| {
                if let Some(r) = r.filter(|r| ids.contains(r)) {
                    seen.insert(r);
                }
            };
            hit(crate::plants::link::style_of(o));
            hit(crate::hardscape::style_id(o));
            hit(crate::landscape::style_id(o));
            hit(o.attrs.fill_pattern);
            hit(o.attrs.line_type);
            hit(crate::worksheet_table::data(o).map(|d| d.worksheet));
            match o.shape {
                Shape::PlantTag { style, .. } => hit(Some(style)),
                Shape::Symbol { symbol, .. } => hit(Some(symbol)),
                _ => {}
            }
            for v in o.data.values() {
                if let Some(m) = v.get(crate::record::MARKER)
                    && let Some(r) = tags.get(&m.to_string())
                {
                    hit(Some(*r));
                }
            }
            for r in seen {
                *uses.entry(r).or_default() += 1;
            }
        }
    }
    doc.resources
        .iter()
        .filter(|r| type_filter.is_none_or(|t| r.body.type_name() == t))
        .filter(|r| {
            search.as_ref().is_none_or(|s| {
                r.name.to_lowercase().contains(s.as_str())
                    || r.body.type_name().contains(s.as_str())
            })
        })
        .map(|r| ResourceInfo {
            id: r.id,
            name: r.name.clone(),
            type_name: r.body.type_name(),
            used_by: uses.get(&r.id).copied().unwrap_or(0),
            depends_on: body_refs(&r.body),
        })
        .collect()
}

/// The types that a purge removes when you name none. Worksheets and data maps are work of their own,
/// so they stay unless you name them.
pub const PURGE_TYPES: [&str; 10] = [
    "image",
    "symbol",
    "hatch",
    "tile",
    "line_type",
    "texture",
    "plant_style",
    "hardscape_style",
    "landscape_area_style",
    "record_format",
];

/// The unused resources of these types, in an order that is safe to delete in: a resource comes
/// before the resources that it uses. Deleting a resource can free others (an unused tile frees its
/// symbol), so the list includes them.
pub fn purge_plan(doc: &Document, types: &[&str]) -> Vec<ResourceId> {
    let mut gone: HashSet<ResourceId> = HashSet::new();
    let mut order = Vec::new();
    loop {
        let mut changed = false;
        for r in doc.resources.iter() {
            if gone.contains(&r.id) || !types.contains(&r.body.type_name()) {
                continue;
            }
            let u = usage(doc, r.id);
            if u.objects.is_empty()
                && u.classes.is_empty()
                && u.resources.iter().all(|x| gone.contains(x))
            {
                gone.insert(r.id);
                order.push(r.id);
                changed = true;
            }
        }
        if !changed {
            return order;
        }
    }
}

/// What to do when the document has a resource of that type and name already.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Conflict {
    /// Add the import under a new name.
    #[default]
    Rename,
    /// Use the resource that is there. The import is skipped.
    Reuse,
    /// Replace the content of the resource that is there.
    Replace,
}

impl Conflict {
    pub fn from_name(s: &str) -> Option<Conflict> {
        match s.trim().to_ascii_lowercase().as_str() {
            "rename" => Some(Conflict::Rename),
            "reuse" | "skip" => Some(Conflict::Reuse),
            "replace" | "overwrite" => Some(Conflict::Replace),
            _ => None,
        }
    }
}

/// What happened to one resource in an import.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportStatus {
    Added,
    /// Added under another name, because the name was taken.
    Renamed,
    /// The document has the same resource already (the same name and content, or `Conflict::Reuse`).
    Reused,
    Replaced,
}

impl ImportStatus {
    pub fn id(self) -> &'static str {
        match self {
            ImportStatus::Added => "added",
            ImportStatus::Renamed => "renamed",
            ImportStatus::Reused => "reused",
            ImportStatus::Replaced => "replaced",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ImportItem {
    pub src: ResourceId,
    /// The ID in the document (for an added resource, the ID it will get).
    pub dst: ResourceId,
    pub name: String,
    pub type_name: &'static str,
    pub status: ImportStatus,
    /// True for a resource that was not asked for but that another one needs.
    pub dependency: bool,
}

/// What an import will do. Run `command` on the document, or do nothing if it is `None`.
#[derive(Clone, Debug, PartialEq)]
pub struct ImportPlan {
    pub command: Option<Command>,
    pub items: Vec<ImportItem>,
    /// Classes added because components of an imported symbol use them.
    pub classes_added: Vec<String>,
}

/// A name that no resource of the type has, from `base`: "Base 2", "Base 3", ...
pub fn unique_name(base: &str, taken: &dyn Fn(&str) -> bool) -> String {
    if !taken(base) {
        return base.to_string();
    }
    (2..)
        .map(|n| format!("{base} {n}"))
        .find(|n| !taken(n))
        .expect("endless")
}

struct Planner<'a> {
    dst: &'a Document,
    src: &'a Document,
    conflict: Conflict,
    ids: IdGen,
    cmds: Vec<Command>,
    map: HashMap<ResourceId, ResourceId>,
    class_map: HashMap<ClassId, Option<ClassId>>,
    items: Vec<ImportItem>,
    /// Names added by this plan, as (type, lower case name).
    added: HashSet<(&'static str, String)>,
    classes_added: Vec<String>,
}

impl Planner<'_> {
    fn class_for(&mut self, c: ClassId) -> Option<ClassId> {
        if let Some(m) = self.class_map.get(&c) {
            return *m;
        }
        let m = self.src.class(c).map(|sc| {
            if let Some(dc) = self
                .dst
                .classes
                .iter()
                .find(|d| d.name.eq_ignore_ascii_case(&sc.name))
            {
                return dc.id;
            }
            let id = ClassId(self.ids.take());
            self.cmds.push(Command::AddClass {
                name: sc.name.clone(),
                attrs: Attributes {
                    fill_pattern: None,
                    line_type: None,
                    ..sc.attrs.clone()
                },
            });
            self.classes_added.push(sc.name.clone());
            id
        });
        self.class_map.insert(c, m);
        m
    }

    fn place(
        &mut self,
        id: ResourceId,
        dependency: bool,
        stack: &mut Vec<ResourceId>,
    ) -> Result<ResourceId, String> {
        if let Some(d) = self.map.get(&id) {
            return Ok(*d);
        }
        let r: &Resource = self
            .src
            .resources
            .get(id)
            .ok_or_else(|| format!("the file has no resource {id}"))?;
        if stack.len() > 32 {
            return Err(format!(
                "resource {} needs too many other resources one after the other",
                r.name
            ));
        }
        if stack.contains(&id) {
            return Err(format!("resource {} uses itself", r.name));
        }
        // The same picture file is stored once: reuse the picture the document has.
        if let ResourceBody::Image(img) = &r.body
            && let Some(eid) = self.dst.resources.find_image(&img.sha256)
        {
            self.map.insert(id, eid);
            self.items.push(ImportItem {
                src: id,
                dst: eid,
                name: r.name.clone(),
                type_name: "image",
                status: ImportStatus::Reused,
                dependency,
            });
            return Ok(eid);
        }
        let type_name = r.body.type_name();
        let existing = self
            .dst
            .resources
            .iter()
            .find(|d| {
                d.body.type_name() == type_name
                    && d.name.eq_ignore_ascii_case(&r.name)
                    && type_name != "unknown"
            })
            .map(|d| (d.id, d.body.clone()));
        // Without a name clash or with "reuse" the resource itself needs no look at its dependencies.
        if let (Some((eid, _)), Conflict::Reuse) = (&existing, self.conflict) {
            self.map.insert(id, *eid);
            self.items.push(ImportItem {
                src: id,
                dst: *eid,
                name: r.name.clone(),
                type_name,
                status: ImportStatus::Reused,
                dependency,
            });
            return Ok(*eid);
        }
        stack.push(id);
        let deps = body_refs(&r.body);
        for d in deps {
            self.place(d, true, stack)?;
        }
        stack.pop();
        let map = self.map.clone();
        let mut body = remap_body(&r.body, &|x| map.get(&x).copied().unwrap_or(x));
        if let ResourceBody::Graphic(g) = &mut body {
            let classes: Vec<ClassId> = g.classes();
            let mapped: HashMap<ClassId, Option<ClassId>> = classes
                .into_iter()
                .map(|c| (c, self.class_for(c)))
                .collect();
            g.remap_classes(&|c| mapped.get(&c).copied().flatten());
        }
        let (dst_id, status, name) = match existing {
            // The same name and content: nothing to import.
            Some((eid, old)) if old == body => (eid, ImportStatus::Reused, r.name.clone()),
            Some((eid, _)) if self.conflict == Conflict::Replace => {
                self.cmds.push(Command::ModifyResource {
                    id: eid,
                    name: None,
                    body: Some(body),
                });
                (eid, ImportStatus::Replaced, r.name.clone())
            }
            existing => {
                let (name, status) = if existing.is_some()
                    || self.added.contains(&(type_name, r.name.to_lowercase()))
                {
                    let n = unique_name(&r.name, &|n| {
                        self.dst.resources.iter().any(|d| {
                            d.body.type_name() == type_name && d.name.eq_ignore_ascii_case(n)
                        }) || self.added.contains(&(type_name, n.to_lowercase()))
                    });
                    (n, ImportStatus::Renamed)
                } else {
                    (r.name.clone(), ImportStatus::Added)
                };
                let new_id = ResourceId(self.ids.take());
                self.added.insert((type_name, name.to_lowercase()));
                self.cmds.push(Command::AddResource {
                    name: name.clone(),
                    body,
                });
                (new_id, status, name)
            }
        };
        self.map.insert(id, dst_id);
        self.items.push(ImportItem {
            src: id,
            dst: dst_id,
            name,
            type_name,
            status,
            dependency,
        });
        Ok(dst_id)
    }
}

/// Plans the import of resources from another document, with everything they need. A symbol that
/// uses a hatch brings the hatch. Classes that symbol components use are added when the document
/// has no class of that name. `picks` are IDs in `src`.
pub fn import_plan(
    dst: &Document,
    src: &Document,
    picks: &[ResourceId],
    conflict: Conflict,
) -> Result<ImportPlan, String> {
    let mut p = Planner {
        dst,
        src,
        conflict,
        ids: dst.id_plan(),
        cmds: Vec::new(),
        map: HashMap::new(),
        class_map: HashMap::new(),
        items: Vec::new(),
        added: HashSet::new(),
        classes_added: Vec::new(),
    };
    for &id in picks {
        p.place(id, false, &mut Vec::new())?;
    }
    // A dependency that is also picked is not a dependency.
    for it in &mut p.items {
        it.dependency = it.dependency && !picks.contains(&it.src);
    }
    let command = match p.cmds.len() {
        0 => None,
        1 => p.cmds.pop(),
        _ => Some(Command::Batch { commands: p.cmds }),
    };
    Ok(ImportPlan {
        command,
        items: p.items,
        classes_added: p.classes_added,
    })
}

/// Plans the import of classes (by name) from another document. Classes the document has are skipped.
/// Returns the command (if any) and the names that were added.
pub fn import_classes_plan(
    dst: &Document,
    src: &Document,
    names: &[String],
) -> Result<(Option<Command>, Vec<String>), String> {
    let mut cmds = Vec::new();
    let mut added = Vec::new();
    for n in names {
        let c = src
            .classes
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(n))
            .ok_or_else(|| format!("the file has no class '{n}'"))?;
        if dst
            .classes
            .iter()
            .any(|d| d.name.eq_ignore_ascii_case(&c.name))
            || added
                .iter()
                .any(|a: &String| a.eq_ignore_ascii_case(&c.name))
        {
            continue;
        }
        cmds.push(Command::AddClass {
            name: c.name.clone(),
            attrs: Attributes {
                fill_pattern: None,
                line_type: None,
                ..c.attrs.clone()
            },
        });
        added.push(c.name.clone());
    }
    Ok((
        if cmds.is_empty() {
            None
        } else {
            Some(Command::Batch { commands: cmds })
        },
        added,
    ))
}

/// Plans the import of design layers (by name). Layers the document has are skipped. A sheet layer cannot be imported this way.
pub fn import_layers_plan(
    dst: &Document,
    src: &Document,
    names: &[String],
) -> Result<(Option<Command>, Vec<String>), String> {
    let mut cmds = Vec::new();
    let mut added = Vec::new();
    for n in names {
        let l = src
            .layers
            .iter()
            .find(|l| l.name.eq_ignore_ascii_case(n))
            .ok_or_else(|| format!("the file has no layer '{n}'"))?;
        if l.kind != LayerKind::Design {
            return Err(format!(
                "layer '{}' is a sheet layer. Only design layers can be imported",
                l.name
            ));
        }
        if dst
            .layers
            .iter()
            .any(|d| d.name.eq_ignore_ascii_case(&l.name))
            || added
                .iter()
                .any(|a: &String| a.eq_ignore_ascii_case(&l.name))
        {
            continue;
        }
        cmds.push(Command::AddLayer {
            name: l.name.clone(),
            elevation: l.elevation,
            scale: Some(l.scale),
            kind: Some(LayerKind::Design),
        });
        added.push(l.name.clone());
    }
    Ok((
        if cmds.is_empty() {
            None
        } else {
            Some(Command::Batch { commands: cmds })
        },
        added,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Editor;
    use crate::graphics::{
        GraphicResource, HatchDef, HatchLine, LineElement, LineTypeDef, PatternUnits, SymbolDef,
        SymbolItem, TileDef,
    };
    use crate::model::{Color, NONE_CLASS};
    use crate::plants::style::PlantStyle;
    use glam::DVec2;

    fn add(ed: &mut Editor, name: &str, g: GraphicResource) -> ResourceId {
        ResourceId(
            ed.execute(&Command::AddResource {
                name: name.into(),
                body: ResourceBody::Graphic(Box::new(g)),
            })
            .unwrap()
            .created[0],
        )
    }

    fn hatch(angle: f64) -> GraphicResource {
        GraphicResource::Hatch(HatchDef {
            units: PatternUnits::Paper,
            lines: vec![HatchLine::solid(angle, 2.0)],
        })
    }

    fn symbol(pattern: Option<ResourceId>, class: Option<ClassId>) -> GraphicResource {
        GraphicResource::Symbol(SymbolDef {
            items: vec![SymbolItem {
                shape: Shape::Circle {
                    center: DVec2::ZERO,
                    radius: 5.0,
                },
                attrs: Attributes {
                    fill_pattern: pattern,
                    ..Default::default()
                },
                class,
            }],
            ..Default::default()
        })
    }

    fn tile(sym: ResourceId) -> GraphicResource {
        GraphicResource::Tile(TileDef {
            symbol: sym,
            width: 10.0,
            height: 10.0,
            row_shift: 0.0,
            angle: 0.0,
            scale: 1.0,
            units: PatternUnits::Paper,
        })
    }

    /// A source document: hatch Diag, symbol Dot (uses Diag), tile Dots (uses Dot), a plant style, a worksheet.
    fn source() -> (Document, ResourceId, ResourceId, ResourceId) {
        let mut ed = Editor::default();
        let class = ClassId(
            ed.execute(&Command::AddClass {
                name: "Trees".into(),
                attrs: Attributes {
                    pen: Some(Color([0, 90, 0, 255])),
                    fill_pattern: None,
                    ..Default::default()
                },
            })
            .unwrap()
            .created[0],
        );
        let h = add(&mut ed, "Diag", hatch(45.0));
        let s = add(&mut ed, "Dot", symbol(Some(h), Some(class)));
        let t = add(&mut ed, "Dots", tile(s));
        (ed.doc().clone(), h, s, t)
    }

    #[test]
    fn usage_counts_objects_classes_and_resources() {
        let mut ed = Editor::default();
        let h = add(&mut ed, "H", hatch(0.0));
        let s = add(&mut ed, "S", symbol(Some(h), None));
        let t = add(&mut ed, "T", tile(s));
        assert!(usage(ed.doc(), t).is_unused());
        assert_eq!(usage(ed.doc(), s).resources, vec![t]);
        assert_eq!(usage(ed.doc(), h).resources, vec![s]);
        ed.execute(&Command::AddObject {
            shape: Shape::Symbol {
                symbol: s,
                at: DVec2::ZERO,
                rotation: 0.0,
                scale: 1.0,
                scale_y: None,
            },
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap();
        ed.execute(&Command::AddObject {
            shape: Shape::Circle {
                center: DVec2::ZERO,
                radius: 1.0,
            },
            layer: None,
            class: None,
            attrs: Attributes {
                fill_pattern: Some(h),
                ..Default::default()
            },
        })
        .unwrap();
        ed.execute(&Command::ModifyClass {
            id: NONE_CLASS,
            name: None,
            visible: None,
            attrs: Some(Attributes {
                fill_pattern: Some(h),
                ..Default::default()
            }),
        })
        .unwrap();
        let u = usage(ed.doc(), h);
        assert_eq!(
            (
                u.objects.len(),
                u.classes.len(),
                u.resources.len(),
                u.total()
            ),
            (1, 1, 1, 3)
        );
        assert!(!u.is_unused());
        assert_eq!(usage(ed.doc(), s).objects.len(), 1);
        // The list agrees with usage.
        let l = list(ed.doc(), None, None);
        assert_eq!(l.len(), 3);
        assert_eq!(l.iter().find(|i| i.id == h).unwrap().used_by, 3);
        assert_eq!(l.iter().find(|i| i.id == s).unwrap().used_by, 2);
        assert_eq!(l.iter().find(|i| i.id == t).unwrap().used_by, 0);
        assert_eq!(l.iter().find(|i| i.id == t).unwrap().depends_on, vec![s]);
        assert_eq!(l[0].type_name, "hatch");
        // Filters.
        assert_eq!(list(ed.doc(), Some("symbol"), None).len(), 1);
        // The search looks at names and type names: "t" is in T (the name and the type tile) and in "symbol"? no: S has no t. It is in "hatch".
        assert_eq!(list(ed.doc(), None, Some("  t ")).len(), 2);
        assert_eq!(list(ed.doc(), None, Some("hat")).len(), 1);
        assert_eq!(list(ed.doc(), Some("hatch"), Some("zzz")).len(), 0);
        assert!(list(&Document::default(), None, None).is_empty());
    }

    #[test]
    fn records_worksheets_and_styles_count_as_used() {
        let mut ed = Editor::default();
        let plant = ResourceId(
            ed.execute(&Command::AddResource {
                name: "Oak".into(),
                body: ResourceBody::plant_style(PlantStyle::default()),
            })
            .unwrap()
            .created[0],
        );
        let p = ed
            .execute(&Command::AddPlants {
                style: Some(plant),
                overrides: Default::default(),
                positions: vec![DVec2::ZERO],
                layer: None,
                class: None,
                variation: None,
            })
            .unwrap()
            .created[0];
        assert_eq!(usage(ed.doc(), plant).objects, vec![ObjectId(p)]);
        assert_eq!(list(ed.doc(), None, None)[0].used_by, 1);
        let fmt = crate::record::RecordFormat {
            fields: vec![crate::record::FieldDef::new(
                "Depth",
                crate::record::FieldType::Dimension,
            )],
        };
        let rec = ResourceId(
            ed.execute(&Command::AddResource {
                name: "Soil".into(),
                body: ResourceBody::RecordFormat(Box::new(fmt)),
            })
            .unwrap()
            .created[0],
        );
        ed.execute(&Command::AttachRecord {
            ids: vec![ObjectId(p)],
            record: rec,
            values: Default::default(),
        })
        .unwrap();
        assert_eq!(usage(ed.doc(), rec).objects.len(), 1);
        assert_eq!(list(ed.doc(), Some("record_format"), None)[0].used_by, 1);
        // A purge keeps what is used and removes the rest.
        let spare = add(&mut ed, "Spare", hatch(10.0));
        assert_eq!(purge_plan(ed.doc(), &PURGE_TYPES), vec![spare]);
        assert!(purge_plan(ed.doc(), &["worksheet"]).is_empty());
    }

    #[test]
    fn a_purge_frees_resources_in_a_safe_order() {
        let mut ed = Editor::default();
        let h = add(&mut ed, "H", hatch(0.0));
        let s = add(&mut ed, "S", symbol(Some(h), None));
        let t = add(&mut ed, "T", tile(s));
        let keep = add(&mut ed, "Keep", hatch(30.0));
        ed.execute(&Command::AddObject {
            shape: Shape::Circle {
                center: DVec2::ZERO,
                radius: 1.0,
            },
            layer: None,
            class: None,
            attrs: Attributes {
                fill_pattern: Some(keep),
                ..Default::default()
            },
        })
        .unwrap();
        let plan = purge_plan(ed.doc(), &PURGE_TYPES);
        assert_eq!(plan, vec![t, s, h], "the tile first, then what it used");
        let cmds: Vec<Command> = plan
            .iter()
            .map(|id| Command::DeleteResource {
                id: *id,
                reassign: None,
            })
            .collect();
        ed.execute(&Command::Batch { commands: cmds }).unwrap();
        assert_eq!(ed.doc().resources.len(), 1);
        assert!(ed.undo());
        assert_eq!(ed.doc().resources.len(), 4);
        // A symbol with an instance stays, and so does what it needs.
        ed.execute(&Command::AddObject {
            shape: Shape::Symbol {
                symbol: s,
                at: DVec2::ZERO,
                rotation: 0.0,
                scale: 1.0,
                scale_y: None,
            },
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap();
        assert_eq!(purge_plan(ed.doc(), &PURGE_TYPES), vec![t]);
        // Only the types you name.
        assert!(purge_plan(ed.doc(), &["hatch"]).is_empty());
    }

    #[test]
    fn importing_a_tile_brings_its_symbol_and_hatch() {
        let (src, _h, _s, t) = source();
        let mut ed = Editor::default();
        let plan = import_plan(ed.doc(), &src, &[t], Conflict::Rename).unwrap();
        assert_eq!(plan.items.len(), 3);
        assert_eq!(plan.items.iter().filter(|i| i.dependency).count(), 2);
        assert!(plan.items.iter().all(|i| i.status == ImportStatus::Added));
        assert_eq!(plan.classes_added, vec!["Trees".to_string()]);
        ed.execute(plan.command.as_ref().unwrap()).unwrap();
        let doc = ed.doc();
        assert_eq!(doc.resources.len(), 3);
        // The IDs in the plan are the real IDs, and the references follow them.
        for it in &plan.items {
            assert_eq!(doc.resources.get(it.dst).unwrap().name, it.name);
        }
        let dst_tile = doc.resources.find_graphic("tile", "Dots").unwrap();
        let dst_sym = doc.resources.find_graphic("symbol", "Dot").unwrap();
        let dst_hatch = doc.resources.find_graphic("hatch", "Diag").unwrap();
        assert_eq!(doc.resources.tile(dst_tile).unwrap().symbol, dst_sym);
        let item = &doc.resources.symbol(dst_sym).unwrap().items[0];
        assert_eq!(item.attrs.fill_pattern, Some(dst_hatch));
        // The class of the component was added by name and the component uses the new ID.
        let class = doc.classes.iter().find(|c| c.name == "Trees").unwrap();
        assert_eq!(item.class, Some(class.id));
        assert_eq!(class.attrs.pen, Some(Color([0, 90, 0, 255])));
        // It is one undo step.
        assert!(ed.undo());
        assert!(ed.doc().resources.is_empty() && ed.doc().classes.len() == 1);
        assert!(!ed.undo());
    }

    #[test]
    fn name_clashes_rename_reuse_or_replace() {
        let (src, h, s, _) = source();
        let mut ed = Editor::default();
        // The document has a different hatch named Diag.
        let mine = add(&mut ed, "diag", hatch(10.0));
        let plan = import_plan(ed.doc(), &src, &[s], Conflict::Rename).unwrap();
        let it = plan.items.iter().find(|i| i.src == h).unwrap();
        assert_eq!(
            (it.status, it.name.as_str()),
            (ImportStatus::Renamed, "Diag 2")
        );
        ed.execute(plan.command.as_ref().unwrap()).unwrap();
        assert_eq!(ed.doc().resources.hatch(mine).unwrap().lines[0].angle, 10.0);
        assert!(ed.doc().resources.find_graphic("hatch", "Diag 2").is_some());
        // The same import again changes nothing: the symbol and the renamed hatch are the same now? The names differ, so the first clash renames again.
        let again = import_plan(ed.doc(), &src, &[s], Conflict::Rename).unwrap();
        assert!(
            again.items.iter().find(|i| i.src == s).unwrap().status == ImportStatus::Reused
                || again.items.iter().find(|i| i.src == s).unwrap().status == ImportStatus::Renamed
        );

        // Reuse: the symbol in the document is taken as it is, and its hatch is not imported.
        let mut ed2 = Editor::default();
        let s2 = add(&mut ed2, "Dot", symbol(None, None));
        let plan = import_plan(ed2.doc(), &src, &[s], Conflict::Reuse).unwrap();
        assert_eq!(plan.items.len(), 1);
        assert_eq!(
            (plan.items[0].status, plan.items[0].dst),
            (ImportStatus::Reused, s2)
        );
        assert!(plan.command.is_none());
        // Replace: the content of the symbol changes, the ID stays.
        let plan = import_plan(ed2.doc(), &src, &[s], Conflict::Replace).unwrap();
        let it = plan.items.iter().find(|i| i.src == s).unwrap();
        assert_eq!((it.status, it.dst), (ImportStatus::Replaced, s2));
        ed2.execute(plan.command.as_ref().unwrap()).unwrap();
        let new_hatch = ed2.doc().resources.find_graphic("hatch", "Diag").unwrap();
        assert_eq!(
            ed2.doc().resources.symbol(s2).unwrap().items[0]
                .attrs
                .fill_pattern,
            Some(new_hatch)
        );
        assert_eq!(ed2.doc().resources.len(), 2);
        // Importing what is the same already is a no-op.
        let same = import_plan(ed2.doc(), &src, &[s], Conflict::Rename).unwrap();
        assert!(same.command.is_none());
        assert!(same.items.iter().all(|i| i.status == ImportStatus::Reused));
    }

    #[test]
    fn two_picks_with_one_name_get_two_names() {
        let (src, h, _, _) = source();
        let mut other = src.clone();
        // A second hatch with the same name in another type is no clash. Two with one name in the source cannot exist, so rename through the document.
        let mut ed = Editor::new(other.clone());
        let again = add(&mut ed, "Hatch B", hatch(5.0));
        other = ed.doc().clone();
        let mut dst = Editor::default();
        add(&mut dst, "Diag", hatch(99.0));
        let plan = import_plan(dst.doc(), &other, &[h, again], Conflict::Rename).unwrap();
        let names: Vec<&str> = plan.items.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, vec!["Diag 2", "Hatch B"]);
        dst.execute(plan.command.as_ref().unwrap()).unwrap();
        assert_eq!(dst.doc().resources.len(), 3);
    }

    #[test]
    fn line_types_bring_their_marks_and_errors_are_clear() {
        let mut ed = Editor::default();
        let s = add(&mut ed, "Post", symbol(None, None));
        let lt = add(
            &mut ed,
            "Fence",
            GraphicResource::LineType(LineTypeDef {
                units: PatternUnits::Paper,
                elements: vec![
                    LineElement::Dash { length: 5.0 },
                    LineElement::Mark {
                        symbol: s,
                        scale: 1.0,
                        rotate: true,
                    },
                    LineElement::Gap { length: 1.0 },
                ],
            }),
        );
        let mut dst = Editor::default();
        let plan = import_plan(dst.doc(), ed.doc(), &[lt], Conflict::Rename).unwrap();
        dst.execute(plan.command.as_ref().unwrap()).unwrap();
        let new_lt = dst
            .doc()
            .resources
            .find_graphic("line_type", "Fence")
            .unwrap();
        let new_s = dst.doc().resources.find_graphic("symbol", "Post").unwrap();
        assert!(
            matches!(dst.doc().resources.line_type(new_lt).unwrap().elements[1], LineElement::Mark { symbol, .. } if symbol == new_s)
        );
        assert!(
            import_plan(
                dst.doc(),
                ed.doc(),
                &[ResourceId::from_num(9999)],
                Conflict::Rename
            )
            .unwrap_err()
            .contains("no resource")
        );
        // A damaged file where a symbol contains itself must not hang.
        let mut loop_doc = ed.doc().clone();
        if let Some(Resource {
            body: ResourceBody::Graphic(g),
            ..
        }) = loop_doc.resources.get_mut(s)
        {
            **g = GraphicResource::Symbol(SymbolDef {
                items: vec![SymbolItem {
                    shape: Shape::Symbol {
                        symbol: s,
                        at: DVec2::ZERO,
                        rotation: 0.0,
                        scale: 1.0,
                        scale_y: None,
                    },
                    attrs: Attributes::default(),
                    class: None,
                }],
                ..Default::default()
            });
        }
        assert!(
            import_plan(dst.doc(), &loop_doc, &[s], Conflict::Rename)
                .unwrap_err()
                .contains("uses itself")
        );
        assert_eq!(Conflict::from_name("Skip"), Some(Conflict::Reuse));
        assert_eq!(Conflict::from_name("overwrite"), Some(Conflict::Replace));
        assert_eq!(Conflict::from_name("rename"), Some(Conflict::Rename));
        assert_eq!(Conflict::from_name("x"), None);
        for s in [
            ImportStatus::Added,
            ImportStatus::Renamed,
            ImportStatus::Reused,
            ImportStatus::Replaced,
        ] {
            assert!(!s.id().is_empty());
        }
    }

    #[test]
    fn landscape_styles_bring_their_plants() {
        let mut ed = Editor::default();
        let plant = ResourceId(
            ed.execute(&Command::AddResource {
                name: "Lavender".into(),
                body: ResourceBody::plant_style(PlantStyle::default()),
            })
            .unwrap()
            .created[0],
        );
        let mut style = crate::landscape::style::LandscapeAreaStyle::default();
        style.mix.push(crate::landscape::style::MixEntry {
            plant,
            percent: 100.0,
            ..Default::default()
        });
        let area = ResourceId(
            ed.execute(&Command::AddResource {
                name: "Meadow".into(),
                body: ResourceBody::LandscapeAreaStyle(Box::new(style)),
            })
            .unwrap()
            .created[0],
        );
        assert_eq!(
            body_refs(&ed.doc().resources.get(area).unwrap().body),
            vec![plant]
        );
        let mut dst = Editor::default();
        // The plant is there under another ID already, with the same content: it is reused.
        dst.execute(&Command::AddObject {
            shape: Shape::Circle {
                center: DVec2::ZERO,
                radius: 1.0,
            },
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap();
        dst.execute(&Command::AddResource {
            name: "Lavender".into(),
            body: ResourceBody::plant_style(PlantStyle::default()),
        })
        .unwrap();
        let plan = import_plan(dst.doc(), ed.doc(), &[area], Conflict::Rename).unwrap();
        assert_eq!(
            plan.items.iter().map(|i| i.status).collect::<Vec<_>>(),
            vec![ImportStatus::Reused, ImportStatus::Added]
        );
        dst.execute(plan.command.as_ref().unwrap()).unwrap();
        let new_area = dst
            .doc()
            .resources
            .find_landscape_area_style("Meadow")
            .unwrap();
        let new_plant = dst.doc().resources.find_plant_style("Lavender").unwrap();
        assert_eq!(
            dst.doc()
                .resources
                .landscape_area_style(new_area)
                .unwrap()
                .mix[0]
                .plant,
            new_plant
        );
        assert_eq!(dst.doc().resources.plant_styles().count(), 1);
        let remapped = remap_body(&ed.doc().resources.get(area).unwrap().body, &|_| {
            ResourceId::from_num(77)
        });
        assert_eq!(body_refs(&remapped), vec![ResourceId::from_num(77)]);
    }

    #[test]
    fn classes_and_layers_import_by_name() {
        let mut src = Editor::default();
        src.execute(&Command::AddClass {
            name: "Paving".into(),
            attrs: Attributes {
                pen: Some(Color([1, 2, 3, 255])),
                ..Default::default()
            },
        })
        .unwrap();
        src.execute(&Command::AddClass {
            name: "Trees".into(),
            attrs: Attributes::default(),
        })
        .unwrap();
        src.execute(&Command::AddLayer {
            name: "Site".into(),
            elevation: 250.0,
            scale: Some(200.0),
            kind: None,
        })
        .unwrap();
        src.execute(&Command::AddSheet {
            name: "Sheet A".into(),
            paper: None,
            orientation: None,
            number: None,
            title: None,
        })
        .unwrap();
        let mut dst = Editor::default();
        dst.execute(&Command::AddClass {
            name: "trees".into(),
            attrs: Attributes::default(),
        })
        .unwrap();
        let names = vec![
            "Paving".to_string(),
            "Trees".to_string(),
            "paving".to_string(),
        ];
        let (cmd, added) = import_classes_plan(dst.doc(), src.doc(), &names).unwrap();
        assert_eq!(added, vec!["Paving".to_string()]);
        dst.execute(&cmd.unwrap()).unwrap();
        assert_eq!(
            dst.doc()
                .classes
                .iter()
                .find(|c| c.name == "Paving")
                .unwrap()
                .attrs
                .pen,
            Some(Color([1, 2, 3, 255]))
        );
        assert!(
            import_classes_plan(dst.doc(), src.doc(), &["Nope".to_string()])
                .unwrap_err()
                .contains("no class")
        );
        assert_eq!(
            import_classes_plan(dst.doc(), src.doc(), &["Trees".to_string()])
                .unwrap()
                .0,
            None
        );

        let (cmd, added) = import_layers_plan(
            dst.doc(),
            src.doc(),
            &[
                "Site".to_string(),
                "site".to_string(),
                "Layer 1".to_string(),
            ],
        )
        .unwrap();
        assert_eq!(added, vec!["Site".to_string()]);
        dst.execute(&cmd.unwrap()).unwrap();
        let l = dst.doc().layers.iter().find(|l| l.name == "Site").unwrap();
        assert_eq!((l.elevation, l.scale), (250.0, 200.0));
        assert!(
            import_layers_plan(dst.doc(), src.doc(), &["Sheet A".to_string()])
                .unwrap_err()
                .contains("sheet layer")
        );
        assert!(
            import_layers_plan(dst.doc(), src.doc(), &["Nope".to_string()])
                .unwrap_err()
                .contains("no layer")
        );
        assert_eq!(unique_name("A", &|n| n == "A" || n == "A 2"), "A 3");
        assert_eq!(unique_name("B", &|_| false), "B");
    }

    #[test]
    fn a_very_long_chain_of_dependencies_is_refused() {
        let mut doc = Document::default();
        for k in 0..40u64 {
            let items = if k == 0 {
                vec![]
            } else {
                vec![SymbolItem {
                    shape: Shape::Symbol {
                        symbol: ResourceId(crate::uid::Uid::from_num(100 + k - 1)),
                        at: DVec2::ZERO,
                        rotation: 0.0,
                        scale: 1.0,
                        scale_y: None,
                    },
                    attrs: Attributes::default(),
                    class: None,
                }]
            };
            doc.resources.push(Resource {
                id: ResourceId(crate::uid::Uid::from_num(100 + k)),
                name: format!("S{k}"),
                body: ResourceBody::Graphic(Box::new(GraphicResource::Symbol(SymbolDef {
                    items,
                    ..Default::default()
                }))),
            });
        }
        let err = import_plan(
            &Document::default(),
            &doc,
            &[ResourceId::from_num(139)],
            Conflict::Rename,
        )
        .unwrap_err();
        assert!(err.contains("too many"), "{err}");
        assert!(
            import_plan(
                &Document::default(),
                &doc,
                &[ResourceId::from_num(120)],
                Conflict::Rename
            )
            .is_ok()
        );
    }

    #[test]
    fn a_picture_is_used_by_its_style_and_travels_with_it() {
        let mut ed = Editor::default();
        let style = ResourceId(
            ed.execute(&Command::AddResource {
                name: "Oak".into(),
                body: ResourceBody::plant_style(PlantStyle::default()),
            })
            .unwrap()
            .created[0],
        );
        let out = ed
            .execute(&Command::SetPlantPicture {
                style,
                picture: Some(Box::new(crate::picture::tests::sample(1))),
            })
            .unwrap();
        let img = ResourceId(out.created[0]);
        assert_eq!(usage(ed.doc(), img).resources, vec![style]);
        let row = list(ed.doc(), Some("image"), None);
        assert_eq!((row.len(), row[0].used_by), (1, 1));
        assert_eq!(
            body_refs(&ed.doc().resources.get(style).unwrap().body),
            vec![img]
        );
        let moved = remap_body(&ed.doc().resources.get(style).unwrap().body, &|_| style);
        assert!(matches!(moved, ResourceBody::PlantStyle(s) if s.image == Some(style)));
    }

    #[test]
    fn an_import_reuses_a_picture_the_document_has_and_purge_removes_unused_pictures() {
        use crate::picture::tests::sample;
        let style = |ed: &mut Editor, name: &str| {
            ResourceId(
                ed.execute(&Command::AddResource {
                    name: name.into(),
                    body: ResourceBody::plant_style(PlantStyle::default()),
                })
                .unwrap()
                .created[0],
            )
        };
        let mut src = Editor::default();
        let s1 = style(&mut src, "Oak");
        src.execute(&Command::SetPlantPicture {
            style: s1,
            picture: Some(Box::new(sample(1))),
        })
        .unwrap();
        let mut dst = Editor::default();
        let d1 = style(&mut dst, "Maple");
        dst.execute(&Command::SetPlantPicture {
            style: d1,
            picture: Some(Box::new(sample(1))),
        })
        .unwrap();
        let plan = import_plan(dst.doc(), src.doc(), &[s1], Conflict::Rename).unwrap();
        dst.execute(&plan.command.unwrap()).unwrap();
        let images = list(dst.doc(), Some("image"), None);
        assert_eq!(images.len(), 1, "one file, one picture");
        assert_eq!(images[0].used_by, 2);
        // A picture that no style uses is in the default purge.
        let id = images[0].id;
        for sid in dst
            .doc()
            .resources
            .plant_styles()
            .map(|(i, _, _)| i)
            .collect::<Vec<_>>()
        {
            let mut st = dst.doc().resources.plant_style(sid).unwrap().clone();
            st.image = None;
            dst.execute(&Command::ModifyResource {
                id: sid,
                name: None,
                body: Some(ResourceBody::plant_style(st)),
            })
            .unwrap();
        }
        assert_eq!(purge_plan(dst.doc(), &["image"]), vec![id]);
        assert!(purge_plan(dst.doc(), &PURGE_TYPES).contains(&id));
    }
}
