//! Graphic resources: symbol definitions, hatches, tiles, line types and textures.
//!
//! A resource of one of these kinds is a named, reusable definition in the document. Objects
//! refer to it by its ID: a symbol instance (`Shape::Symbol`), the `fill_pattern` and
//! `line_type` attributes, or another resource (a tile uses a symbol, a symbol can use a hatch).
//! Changing the definition changes every user at once, because users keep only the ID.

use glam::DVec2;
use serde::{Deserialize, Serialize};

use crate::model::{Attributes, ClassId, Color, MAX_COORD, Shape};
use crate::resource::ResourceId;

/// Most components in one symbol definition.
pub const MAX_ITEMS: usize = 10_000;
/// Most hatch lines, line type elements or 3D solids in one definition.
pub const MAX_PARTS: usize = 1_000;

/// What the sizes of a pattern are measured in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatternUnits {
    /// Millimeters on paper. The pattern keeps its size on the sheet at any drawing scale.
    #[default]
    Paper,
    /// Millimeters in the model. The pattern scales with the drawing.
    World,
}

impl PatternUnits {
    /// Model millimeters for one unit of the pattern, when one paper millimeter is `scale` model millimeters.
    pub fn factor(self, scale: f64) -> f64 {
        match self {
            PatternUnits::Paper if scale.is_finite() && scale > 0.0 => scale,
            _ => 1.0,
        }
    }
}

/// One 2D component of a symbol definition. The shape is in definition coordinates.
/// Attributes that the component leaves empty come from the instance (and its class).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SymbolItem {
    pub shape: Shape,
    #[serde(default)]
    pub attrs: Attributes,
    /// The class of the component. Without one the component takes the class of the instance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassId>,
}

/// A 3D component of a symbol definition.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Solid3d {
    /// A prism: the outline in plan, from `base` up to `base + height`.
    Extrusion {
        outline: Vec<DVec2>,
        #[serde(default)]
        base: f64,
        height: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        color: Option<Color>,
    },
    Cylinder {
        center: DVec2,
        radius: f64,
        #[serde(default)]
        base: f64,
        height: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        color: Option<Color>,
    },
}

impl Solid3d {
    /// The outline in plan. A cylinder is a polygon with 24 sides.
    pub fn outline_points(&self) -> Vec<DVec2> {
        match self {
            Solid3d::Extrusion { outline, .. } => outline.clone(),
            Solid3d::Cylinder { center, radius, .. } => (0..24)
                .map(|k| {
                    *center + DVec2::from_angle(std::f64::consts::TAU * k as f64 / 24.0) * *radius
                })
                .collect(),
        }
    }

    /// Bottom and height.
    pub fn span(&self) -> (f64, f64) {
        match self {
            Solid3d::Extrusion { base, height, .. } | Solid3d::Cylinder { base, height, .. } => {
                (*base, *height)
            }
        }
    }

    pub fn color(&self) -> Option<Color> {
        match self {
            Solid3d::Extrusion { color, .. } | Solid3d::Cylinder { color, .. } => *color,
        }
    }

    fn problem(&self) -> Option<String> {
        let (base, height) = self.span();
        if !base.is_finite()
            || !height.is_finite()
            || height <= 0.0
            || base.abs() > MAX_COORD
            || height > MAX_COORD
        {
            return Some("a 3D component needs a positive height and finite numbers".into());
        }
        match self {
            Solid3d::Extrusion { outline, .. } if outline.len() < 3 => {
                Some("an extrusion needs 3 or more outline points".into())
            }
            Solid3d::Extrusion { outline, .. }
                if outline
                    .iter()
                    .any(|p| !p.is_finite() || p.abs().max_element() > MAX_COORD) =>
            {
                Some("the outline has an invalid point".into())
            }
            Solid3d::Cylinder { center, radius, .. }
                if !center.is_finite()
                    || !radius.is_finite()
                    || *radius <= 0.0
                    || *radius > MAX_COORD =>
            {
                Some("a cylinder needs a positive radius and a finite center".into())
            }
            _ => None,
        }
    }
}

/// A symbol definition: 2D components and optional 3D components (a hybrid symbol has both).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SymbolDef {
    /// The insertion point, in definition coordinates. An instance puts this point at its position.
    #[serde(default)]
    pub insertion: DVec2,
    #[serde(default)]
    pub items: Vec<SymbolItem>,
    /// 3D components. A symbol with none shows its 2D components flat in 3D views.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub solids: Vec<Solid3d>,
    /// `paper`: the components are in paper mm, so the instance keeps its size on the sheet at any
    /// drawing scale (north arrows, tags). `world` (default): the components are in model mm.
    #[serde(default = "world", skip_serializing_if = "is_world")]
    pub units: PatternUnits,
    /// The class that a new instance gets when the caller gives none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub insert_class: Option<ClassId>,
    /// A new instance is turned toward a second point that the caller gives (`rotate_to`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub rotate_on_insert: bool,
    /// 3D content of a component (MOD-07, see `crate::component`): surface models, groups and
    /// instances of other components. Instances are `Shape::Component`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<crate::component::Part>,
}

fn world() -> PatternUnits {
    PatternUnits::World
}

fn is_world(u: &PatternUnits) -> bool {
    *u == PatternUnits::World
}

impl Default for SymbolDef {
    fn default() -> Self {
        SymbolDef {
            insertion: DVec2::ZERO,
            items: Vec::new(),
            solids: Vec::new(),
            units: PatternUnits::World,
            insert_class: None,
            rotate_on_insert: false,
            parts: Vec::new(),
        }
    }
}

impl SymbolDef {
    pub fn has_3d(&self) -> bool {
        !self.solids.is_empty()
    }

    pub fn has_2d(&self) -> bool {
        !self.items.is_empty()
    }

    /// True if the symbol is a component definition (it has 3D parts, MOD-07).
    pub fn is_component(&self) -> bool {
        !self.parts.is_empty()
    }

    /// "2d", "3d", "hybrid" or "component".
    pub fn kind(&self) -> &'static str {
        if self.is_component() {
            return "component";
        }
        match (self.has_2d(), self.has_3d()) {
            (true, true) => "hybrid",
            (false, true) => "3d",
            _ => "2d",
        }
    }
}

/// Shapes that can be a 2D component of a symbol.
pub fn allowed_in_symbol(shape: &Shape) -> bool {
    matches!(
        shape,
        Shape::Line { .. }
            | Shape::Rect { .. }
            | Shape::Circle { .. }
            | Shape::Arc { .. }
            | Shape::Polyline { .. }
            | Shape::Text { .. }
            | Shape::Symbol { .. }
    )
}

/// One family of parallel lines in a hatch.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HatchLine {
    /// Direction of the lines, in degrees counterclockwise from the X axis.
    #[serde(default)]
    pub angle: f64,
    /// Distance between neighboring lines, perpendicular to them.
    pub spacing: f64,
    /// Each line is moved along its direction by this much more than the one before. It staggers dashes.
    #[serde(default)]
    pub offset: f64,
    /// A point that one line of the family passes through.
    #[serde(default)]
    pub origin: DVec2,
    /// Lengths of dash, gap, dash, gap, ... Empty: a solid line.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dash: Vec<f64>,
    /// Color of these lines. Without one the pen color of the object is used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    /// Line weight in mm on paper. Without one the line weight of the object is used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<f64>,
}

impl HatchLine {
    pub fn solid(angle: f64, spacing: f64) -> Self {
        HatchLine {
            angle,
            spacing,
            offset: 0.0,
            origin: DVec2::ZERO,
            dash: Vec::new(),
            color: None,
            weight: None,
        }
    }
}

/// A hatch: one or more families of lines.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HatchDef {
    #[serde(default)]
    pub units: PatternUnits,
    pub lines: Vec<HatchLine>,
}

/// A tile: the 2D components of a symbol, repeated on a grid.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TileDef {
    /// The symbol that is repeated. Its insertion point sits on the grid points.
    pub symbol: ResourceId,
    /// Size of one cell of the grid.
    pub width: f64,
    pub height: f64,
    /// Every second row is moved along the row by this much (a brick pattern).
    #[serde(default)]
    pub row_shift: f64,
    /// Turns the whole grid, in degrees.
    #[serde(default)]
    pub angle: f64,
    /// Scale of the symbol in each cell.
    #[serde(default = "one")]
    pub scale: f64,
    #[serde(default)]
    pub units: PatternUnits,
}

fn one() -> f64 {
    1.0
}

/// A part of a line type.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LineElement {
    Dash {
        length: f64,
    },
    Gap {
        length: f64,
    },
    /// A symbol placed on the line, with no length of its own. It makes a complex line type.
    Mark {
        symbol: ResourceId,
        #[serde(default = "one")]
        scale: f64,
        /// Turn the symbol with the direction of the line (default true).
        #[serde(default = "yes")]
        rotate: bool,
    },
}

fn yes() -> bool {
    true
}

/// A line type: dashes, gaps and marks that repeat along a line.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LineTypeDef {
    #[serde(default)]
    pub units: PatternUnits,
    pub elements: Vec<LineElement>,
}

impl LineTypeDef {
    /// Length of one repeat of the pattern.
    pub fn period(&self) -> f64 {
        self.elements
            .iter()
            .map(|e| match e {
                LineElement::Dash { length } | LineElement::Gap { length } => *length,
                LineElement::Mark { .. } => 0.0,
            })
            .sum()
    }
}

/// A texture for 3D views. It is a resource type for now: objects do not use it yet.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextureDef {
    pub color: Color,
    /// Size of one repeat of the texture, in mm.
    #[serde(default = "default_texture_size")]
    pub size: DVec2,
    #[serde(default)]
    pub reflectivity: f64,
    #[serde(default)]
    pub transparency: f64,
    /// A name or path of an image, kept as text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
}

fn default_texture_size() -> DVec2 {
    DVec2::splat(1000.0)
}

/// The graphic resource kinds. The `type` key in JSON names the variant.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GraphicResource {
    Symbol(SymbolDef),
    Hatch(HatchDef),
    Tile(TileDef),
    LineType(LineTypeDef),
    Texture(TextureDef),
}

/// What a reference points at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefKind {
    Symbol,
    /// A hatch or a tile.
    Pattern,
    LineType,
    PlantStyle,
}

impl RefKind {
    pub fn label(self) -> &'static str {
        match self {
            RefKind::Symbol => "symbol",
            RefKind::Pattern => "hatch or tile",
            RefKind::LineType => "line type",
            RefKind::PlantStyle => "plant style",
        }
    }
}

/// The type names of the graphic resources, as in JSON.
pub const TYPE_NAMES: [&str; 5] = ["symbol", "hatch", "tile", "line_type", "texture"];

impl GraphicResource {
    pub fn type_name(&self) -> &'static str {
        match self {
            GraphicResource::Symbol(_) => "symbol",
            GraphicResource::Hatch(_) => "hatch",
            GraphicResource::Tile(_) => "tile",
            GraphicResource::LineType(_) => "line_type",
            GraphicResource::Texture(_) => "texture",
        }
    }

    /// Checks the numbers and rules of the content. References are checked by the commands,
    /// which know the other resources.
    pub fn validate(&self) -> Result<(), String> {
        let fin = |x: f64| x.is_finite() && x.abs() <= MAX_COORD;
        match self {
            GraphicResource::Symbol(s) => {
                if !s.insertion.is_finite() || s.insertion.abs().max_element() > MAX_COORD {
                    return Err("the insertion point is not valid".into());
                }
                if s.items.len() > MAX_ITEMS || s.solids.len() > MAX_PARTS {
                    return Err("the symbol has too many components".into());
                }
                for (i, it) in s.items.iter().enumerate() {
                    if !allowed_in_symbol(&it.shape) {
                        return Err(format!(
                            "component {i} is a {}. A symbol holds line, rect, circle, arc, polyline, text and symbol shapes",
                            it.shape.kind()
                        ));
                    }
                    if !it.shape.is_finite() || !it.shape.is_in_range() || !it.attrs.is_finite() {
                        return Err(format!("component {i} has a number that is not valid"));
                    }
                    if let Some(m) = item_problem(&it.shape) {
                        return Err(format!("component {i}: {m}"));
                    }
                }
                if let Some(m) = s.solids.iter().find_map(Solid3d::problem) {
                    return Err(m);
                }
                if let Some(m) = crate::component::parts_problem(&s.parts) {
                    return Err(m);
                }
                Ok(())
            }
            GraphicResource::Hatch(h) => {
                if h.lines.is_empty() || h.lines.len() > MAX_PARTS {
                    return Err("a hatch needs 1 to 1000 lines".into());
                }
                for l in &h.lines {
                    if !fin(l.angle)
                        || !l.origin.is_finite()
                        || !fin(l.offset)
                        || !l.spacing.is_finite()
                        || l.spacing <= 0.0
                        || l.spacing > MAX_COORD
                    {
                        return Err(
                            "a hatch line needs a positive spacing and finite numbers".into()
                        );
                    }
                    if l.dash.len() > 64
                        || l.dash.iter().any(|d| !d.is_finite() || *d < 0.0)
                        || (!l.dash.is_empty() && l.dash.iter().sum::<f64>() <= 0.0)
                    {
                        return Err(
                            "a dash pattern needs lengths of zero or more, and not all zero".into(),
                        );
                    }
                    if l.weight.is_some_and(|w| !w.is_finite() || w < 0.0) {
                        return Err("the line weight of a hatch line must be zero or more".into());
                    }
                }
                Ok(())
            }
            GraphicResource::Tile(t) => {
                if !t.width.is_finite()
                    || !t.height.is_finite()
                    || t.width <= 0.0
                    || t.height <= 0.0
                    || t.width > MAX_COORD
                    || t.height > MAX_COORD
                {
                    return Err("a tile needs a positive width and height".into());
                }
                if !fin(t.row_shift)
                    || !fin(t.angle)
                    || !t.scale.is_finite()
                    || t.scale <= 0.0
                    || t.scale > MAX_COORD
                {
                    return Err("a tile needs a positive scale and finite numbers".into());
                }
                Ok(())
            }
            GraphicResource::LineType(l) => {
                if l.elements.is_empty() || l.elements.len() > MAX_PARTS {
                    return Err("a line type needs 1 to 1000 elements".into());
                }
                for e in &l.elements {
                    match e {
                        LineElement::Dash { length } | LineElement::Gap { length }
                            if !length.is_finite() || *length < 0.0 || *length > MAX_COORD =>
                        {
                            return Err("dash and gap lengths must be zero or more".into());
                        }
                        LineElement::Mark { scale, .. }
                            if !scale.is_finite() || *scale <= 0.0 || *scale > MAX_COORD =>
                        {
                            return Err("the scale of a mark must be positive".into());
                        }
                        _ => {}
                    }
                }
                if l.period() <= 0.0 {
                    return Err("a line type needs a dash or a gap that is longer than zero".into());
                }
                Ok(())
            }
            GraphicResource::Texture(t) => {
                if !t.size.is_finite()
                    || t.size.min_element() <= 0.0
                    || t.size.max_element() > MAX_COORD
                {
                    return Err("a texture needs a positive size".into());
                }
                if !(0.0..=1.0).contains(&t.reflectivity) || !(0.0..=1.0).contains(&t.transparency)
                {
                    return Err("reflectivity and transparency are between 0 and 1".into());
                }
                Ok(())
            }
        }
    }

    /// The resources that this one uses, with what each must be.
    pub fn refs(&self) -> Vec<(ResourceId, RefKind)> {
        match self {
            GraphicResource::Symbol(s) => s
                .items
                .iter()
                .flat_map(|it| {
                    let mut v = Vec::new();
                    if let Some(p) = it.attrs.fill_pattern {
                        v.push((p, RefKind::Pattern));
                    }
                    if let Some(l) = it.attrs.line_type {
                        v.push((l, RefKind::LineType));
                    }
                    if let Shape::Symbol { symbol, .. } = &it.shape {
                        v.push((*symbol, RefKind::Symbol));
                    }
                    v
                })
                .chain(crate::component::part_refs(&s.parts).map(|r| (r, RefKind::Symbol)))
                .collect(),
            GraphicResource::Tile(t) => vec![(t.symbol, RefKind::Symbol)],
            GraphicResource::LineType(l) => l
                .elements
                .iter()
                .filter_map(|e| match e {
                    LineElement::Mark { symbol, .. } => Some((*symbol, RefKind::Symbol)),
                    _ => None,
                })
                .collect(),
            GraphicResource::Hatch(_) | GraphicResource::Texture(_) => Vec::new(),
        }
    }

    /// The classes that the components of a symbol use.
    pub fn classes(&self) -> Vec<ClassId> {
        match self {
            GraphicResource::Symbol(s) => s.items.iter().filter_map(|i| i.class).collect(),
            _ => Vec::new(),
        }
    }

    /// Replaces the class of every component: `f` gives the new class, or none to use the class of the instance.
    pub fn remap_classes(&mut self, f: &dyn Fn(ClassId) -> Option<ClassId>) {
        if let GraphicResource::Symbol(s) = self {
            for it in &mut s.items {
                it.class = it.class.and_then(f);
            }
        }
    }

    /// Replaces every reference `r` by `f(r)`.
    pub fn remap(&mut self, f: &dyn Fn(ResourceId) -> ResourceId) {
        match self {
            GraphicResource::Symbol(s) => {
                for it in &mut s.items {
                    it.attrs.fill_pattern = it.attrs.fill_pattern.map(f);
                    it.attrs.line_type = it.attrs.line_type.map(f);
                    if let Shape::Symbol { symbol, .. } = &mut it.shape {
                        *symbol = f(*symbol);
                    }
                }
                for p in &mut s.parts {
                    if let Shape::Component { symbol, .. } = &mut p.shape {
                        *symbol = f(*symbol);
                    }
                }
            }
            GraphicResource::Tile(t) => t.symbol = f(t.symbol),
            GraphicResource::LineType(l) => {
                for e in &mut l.elements {
                    if let LineElement::Mark { symbol, .. } = e {
                        *symbol = f(*symbol);
                    }
                }
            }
            GraphicResource::Hatch(_) | GraphicResource::Texture(_) => {}
        }
    }
}

/// Rules for the geometry of one component (sizes that must be positive and so on).
fn item_problem(shape: &Shape) -> Option<String> {
    match shape {
        Shape::Circle { radius, .. } | Shape::Arc { radius, .. } if *radius <= 0.0 => {
            Some("a radius must be positive".into())
        }
        Shape::Arc { start, end, .. } if crate::model::arc_sweep(*start, *end) == 0.0 => {
            Some("an arc sweep must not be zero".into())
        }
        Shape::Polyline { points, .. } if points.len() < 2 => {
            Some("a polyline needs 2 or more points".into())
        }
        Shape::Text { height, .. } if *height <= 0.0 => {
            Some("a text height must be positive".into())
        }
        Shape::Symbol { scale, scale_y, .. }
            if *scale == 0.0 || scale_y.is_some_and(|y| y == 0.0) =>
        {
            Some("a nested symbol cannot have a scale of zero".into())
        }
        _ => None,
    }
}

impl crate::resource::Resources {
    /// The graphic resource with this ID.
    pub fn graphic(&self, id: ResourceId) -> Option<&GraphicResource> {
        match &self.get(id)?.body {
            crate::resource::ResourceBody::Graphic(g) => Some(g.as_ref()),
            _ => None,
        }
    }

    pub fn symbol(&self, id: ResourceId) -> Option<&SymbolDef> {
        match self.graphic(id)? {
            GraphicResource::Symbol(s) => Some(s),
            _ => None,
        }
    }

    pub fn hatch(&self, id: ResourceId) -> Option<&HatchDef> {
        match self.graphic(id)? {
            GraphicResource::Hatch(h) => Some(h),
            _ => None,
        }
    }

    pub fn tile(&self, id: ResourceId) -> Option<&TileDef> {
        match self.graphic(id)? {
            GraphicResource::Tile(t) => Some(t),
            _ => None,
        }
    }

    pub fn line_type(&self, id: ResourceId) -> Option<&LineTypeDef> {
        match self.graphic(id)? {
            GraphicResource::LineType(l) => Some(l),
            _ => None,
        }
    }

    /// The ID of the graphic resource of this type (`symbol`, `hatch`, ...) with this name (compared without case).
    pub fn find_graphic(&self, type_name: &str, name: &str) -> Option<ResourceId> {
        self.iter()
            .find(|r| r.body.type_name() == type_name && r.name.eq_ignore_ascii_case(name.trim()))
            .map(|r| r.id)
    }

    /// True if the resource exists and is of this kind.
    pub fn is_kind(&self, id: ResourceId, kind: RefKind) -> bool {
        match kind {
            RefKind::Symbol => self.symbol(id).is_some(),
            RefKind::Pattern => self.hatch(id).is_some() || self.tile(id).is_some(),
            RefKind::LineType => self.line_type(id).is_some(),
            RefKind::PlantStyle => self.plant_style(id).is_some(),
        }
    }

    /// The first graphic resource that uses itself, directly or through others.
    pub fn find_cycle(&self) -> Option<ResourceId> {
        self.on_cycle().into_iter().next()
    }

    /// Every graphic resource that uses itself, directly or through others, in the order of the
    /// list. One pass over the references (the strongly connected parts of the graph), so a
    /// file with thousands of definitions that use one another costs the size of the graph and
    /// not that size for every definition.
    pub fn on_cycle(&self) -> Vec<ResourceId> {
        let ids: Vec<ResourceId> = self
            .iter()
            .filter(|r| matches!(r.body, crate::resource::ResourceBody::Graphic(_)))
            .map(|r| r.id)
            .collect();
        let index: std::collections::HashMap<ResourceId, usize> =
            ids.iter().enumerate().map(|(i, id)| (*id, i)).collect();
        let edges: Vec<Vec<usize>> = ids
            .iter()
            .map(|id| {
                self.graphic(*id)
                    .map(|g| {
                        g.refs()
                            .into_iter()
                            .filter_map(|(r, _)| index.get(&r).copied())
                            .collect()
                    })
                    .unwrap_or_default()
            })
            .collect();
        // Tarjan, with an explicit stack (a chain of references can be long).
        let n = ids.len();
        let mut order = vec![usize::MAX; n];
        let mut low = vec![0usize; n];
        let mut on_stack = vec![false; n];
        let mut stack: Vec<usize> = Vec::new();
        let mut cyclic = vec![false; n];
        let mut next = 0usize;
        for root in 0..n {
            if order[root] != usize::MAX {
                continue;
            }
            // (node, next edge to look at)
            let mut work: Vec<(usize, usize)> = vec![(root, 0)];
            order[root] = next;
            low[root] = next;
            next += 1;
            stack.push(root);
            on_stack[root] = true;
            while let Some(&mut (v, ref mut k)) = work.last_mut() {
                if let Some(&w) = edges[v].get(*k) {
                    *k += 1;
                    if w == v {
                        cyclic[v] = true;
                    }
                    if order[w] == usize::MAX {
                        order[w] = next;
                        low[w] = next;
                        next += 1;
                        stack.push(w);
                        on_stack[w] = true;
                        work.push((w, 0));
                    } else if on_stack[w] {
                        low[v] = low[v].min(order[w]);
                    }
                } else {
                    work.pop();
                    if let Some(&(parent, _)) = work.last() {
                        low[parent] = low[parent].min(low[v]);
                    }
                    if low[v] == order[v] {
                        let mut part = Vec::new();
                        while let Some(w) = stack.pop() {
                            on_stack[w] = false;
                            part.push(w);
                            if w == v {
                                break;
                            }
                        }
                        if part.len() > 1 {
                            for w in part {
                                cyclic[w] = true;
                            }
                        }
                    }
                }
            }
        }
        ids.into_iter()
            .zip(cyclic)
            .filter_map(|(id, c)| c.then_some(id))
            .collect()
    }

    /// True if the resource `target` is `from` or is used by it, directly or through other resources.
    pub fn symbol_reaches(&self, from: ResourceId, target: ResourceId) -> bool {
        let mut seen = std::collections::HashSet::new();
        let mut todo = vec![from];
        while let Some(id) = todo.pop() {
            if id == target {
                return true;
            }
            if !seen.insert(id) {
                continue;
            }
            if let Some(g) = self.graphic(id) {
                todo.extend(g.refs().into_iter().map(|(r, _)| r));
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Shape;

    fn item(shape: Shape) -> SymbolItem {
        SymbolItem {
            shape,
            attrs: Attributes::default(),
            class: None,
        }
    }

    fn circle(r: f64) -> Shape {
        Shape::Circle {
            center: DVec2::ZERO,
            radius: r,
        }
    }

    #[test]
    fn units_scale_only_in_paper() {
        assert_eq!(PatternUnits::Paper.factor(50.0), 50.0);
        assert_eq!(PatternUnits::World.factor(50.0), 1.0);
        assert_eq!(PatternUnits::Paper.factor(f64::NAN), 1.0);
        assert_eq!(PatternUnits::Paper.factor(0.0), 1.0);
    }

    #[test]
    fn symbol_rules() {
        let ok = GraphicResource::Symbol(SymbolDef {
            items: vec![item(circle(5.0))],
            ..Default::default()
        });
        assert!(ok.validate().is_ok());
        assert_eq!(ok.type_name(), "symbol");
        let bad = |shape| {
            GraphicResource::Symbol(SymbolDef {
                items: vec![item(shape)],
                ..Default::default()
            })
            .validate()
            .unwrap_err()
        };
        assert!(
            bad(Shape::Group {
                name: String::new()
            })
            .contains("group")
        );
        assert!(bad(circle(-1.0)).contains("radius"));
        assert!(bad(circle(f64::NAN)).contains("not valid"));
        assert!(
            bad(Shape::Arc {
                center: DVec2::ZERO,
                radius: 1.0,
                start: 1.0,
                end: 1.0
            })
            .contains("sweep")
        );
        assert!(
            bad(Shape::Polyline {
                points: vec![crate::model::Vertex::corner(DVec2::ZERO)],
                closed: false
            })
            .contains("2 or more")
        );
        assert!(
            bad(Shape::Text {
                at: DVec2::ZERO,
                text: "x".into(),
                height: 0.0
            })
            .contains("height")
        );
        assert!(
            bad(Shape::Symbol {
                symbol: ResourceId::from_num(1),
                at: DVec2::ZERO,
                rotation: 0.0,
                scale: 0.0,
                scale_y: None
            })
            .contains("zero")
        );
        let far = GraphicResource::Symbol(SymbolDef {
            insertion: DVec2::new(f64::INFINITY, 0.0),
            ..Default::default()
        });
        assert!(far.validate().is_err());
        let many = GraphicResource::Symbol(SymbolDef {
            items: vec![item(circle(1.0)); MAX_ITEMS + 1],
            ..Default::default()
        });
        assert!(many.validate().unwrap_err().contains("too many"));
        let mut s = SymbolDef {
            items: vec![item(circle(1.0))],
            ..Default::default()
        };
        assert_eq!(s.kind(), "2d");
        s.solids.push(Solid3d::Cylinder {
            center: DVec2::ZERO,
            radius: 1.0,
            base: 0.0,
            height: 2.0,
            color: None,
        });
        assert_eq!(s.kind(), "hybrid");
        assert!(s.has_3d() && s.has_2d());
        s.items.clear();
        assert_eq!(s.kind(), "3d");
        assert_eq!(SymbolDef::default().kind(), "2d");
    }

    #[test]
    fn solids() {
        let cyl = Solid3d::Cylinder {
            center: DVec2::new(1.0, 1.0),
            radius: 2.0,
            base: 5.0,
            height: 3.0,
            color: Some(Color::BLACK),
        };
        assert_eq!(cyl.outline_points().len(), 24);
        assert_eq!(cyl.span(), (5.0, 3.0));
        assert_eq!(cyl.color(), Some(Color::BLACK));
        let ext = Solid3d::Extrusion {
            outline: vec![DVec2::ZERO, DVec2::X, DVec2::Y],
            base: 0.0,
            height: 1.0,
            color: None,
        };
        assert_eq!(ext.outline_points().len(), 3);
        assert_eq!(ext.color(), None);
        assert!(ext.problem().is_none() && cyl.problem().is_none());
        let flat = Solid3d::Extrusion {
            outline: vec![DVec2::ZERO, DVec2::X, DVec2::Y],
            base: 0.0,
            height: 0.0,
            color: None,
        };
        assert!(flat.problem().is_some());
        let two = Solid3d::Extrusion {
            outline: vec![DVec2::ZERO, DVec2::X],
            base: 0.0,
            height: 1.0,
            color: None,
        };
        assert!(two.problem().unwrap().contains("3 or more"));
        let nan = Solid3d::Extrusion {
            outline: vec![DVec2::ZERO, DVec2::X, DVec2::new(f64::NAN, 1.0)],
            base: 0.0,
            height: 1.0,
            color: None,
        };
        assert!(nan.problem().unwrap().contains("invalid"));
        let thin = Solid3d::Cylinder {
            center: DVec2::ZERO,
            radius: 0.0,
            base: 0.0,
            height: 1.0,
            color: None,
        };
        assert!(thin.problem().unwrap().contains("radius"));
        let sym = |s| {
            GraphicResource::Symbol(SymbolDef {
                solids: vec![s],
                ..Default::default()
            })
            .validate()
        };
        assert!(sym(flat).is_err() && sym(cyl).is_ok());
    }

    #[test]
    fn hatch_rules() {
        let h = |l: HatchLine| {
            GraphicResource::Hatch(HatchDef {
                units: PatternUnits::Paper,
                lines: vec![l],
            })
            .validate()
        };
        assert!(h(HatchLine::solid(45.0, 2.0)).is_ok());
        assert!(h(HatchLine::solid(45.0, 0.0)).is_err());
        assert!(h(HatchLine::solid(f64::NAN, 1.0)).is_err());
        assert!(
            h(HatchLine {
                dash: vec![1.0, -1.0],
                ..HatchLine::solid(0.0, 1.0)
            })
            .is_err()
        );
        assert!(
            h(HatchLine {
                dash: vec![0.0, 0.0],
                ..HatchLine::solid(0.0, 1.0)
            })
            .is_err()
        );
        assert!(
            h(HatchLine {
                dash: vec![2.0, 1.0],
                ..HatchLine::solid(0.0, 1.0)
            })
            .is_ok()
        );
        assert!(
            h(HatchLine {
                weight: Some(-1.0),
                ..HatchLine::solid(0.0, 1.0)
            })
            .is_err()
        );
        assert!(
            GraphicResource::Hatch(HatchDef::default())
                .validate()
                .is_err()
        );
    }

    #[test]
    fn tile_line_type_and_texture_rules() {
        let tile = TileDef {
            symbol: ResourceId::from_num(1),
            width: 10.0,
            height: 5.0,
            row_shift: 5.0,
            angle: 0.0,
            scale: 1.0,
            units: PatternUnits::World,
        };
        assert!(GraphicResource::Tile(tile.clone()).validate().is_ok());
        assert!(
            GraphicResource::Tile(TileDef {
                width: 0.0,
                ..tile.clone()
            })
            .validate()
            .is_err()
        );
        assert!(
            GraphicResource::Tile(TileDef {
                scale: 0.0,
                ..tile.clone()
            })
            .validate()
            .is_err()
        );
        assert!(
            GraphicResource::Tile(TileDef {
                angle: f64::NAN,
                ..tile
            })
            .validate()
            .is_err()
        );

        let lt = |elements| {
            GraphicResource::LineType(LineTypeDef {
                units: PatternUnits::Paper,
                elements,
            })
            .validate()
        };
        assert!(
            lt(vec![
                LineElement::Dash { length: 3.0 },
                LineElement::Gap { length: 1.0 }
            ])
            .is_ok()
        );
        assert!(lt(vec![]).is_err());
        assert!(lt(vec![LineElement::Dash { length: -1.0 }]).is_err());
        assert!(
            lt(vec![LineElement::Dash { length: 0.0 }])
                .unwrap_err()
                .contains("longer than zero")
        );
        assert!(
            lt(vec![
                LineElement::Gap { length: 1.0 },
                LineElement::Mark {
                    symbol: ResourceId::from_num(2),
                    scale: 0.0,
                    rotate: true
                }
            ])
            .is_err()
        );
        let lt2 = LineTypeDef {
            units: PatternUnits::Paper,
            elements: vec![
                LineElement::Dash { length: 3.0 },
                LineElement::Mark {
                    symbol: ResourceId::from_num(2),
                    scale: 1.0,
                    rotate: true,
                },
                LineElement::Gap { length: 1.0 },
            ],
        };
        assert_eq!(lt2.period(), 4.0);

        let tex = TextureDef {
            color: Color::BLACK,
            size: DVec2::splat(500.0),
            reflectivity: 0.2,
            transparency: 0.0,
            image: Some("oak.png".into()),
        };
        assert!(GraphicResource::Texture(tex.clone()).validate().is_ok());
        assert!(
            GraphicResource::Texture(TextureDef {
                size: DVec2::new(0.0, 5.0),
                ..tex.clone()
            })
            .validate()
            .is_err()
        );
        assert!(
            GraphicResource::Texture(TextureDef {
                reflectivity: 2.0,
                ..tex.clone()
            })
            .validate()
            .is_err()
        );
        let names: Vec<&str> = [
            GraphicResource::Texture(tex),
            GraphicResource::Hatch(HatchDef::default()),
            GraphicResource::Tile(TileDef {
                symbol: ResourceId::from_num(1),
                width: 1.0,
                height: 1.0,
                row_shift: 0.0,
                angle: 0.0,
                scale: 1.0,
                units: PatternUnits::Paper,
            }),
            GraphicResource::LineType(LineTypeDef::default()),
            GraphicResource::Symbol(SymbolDef::default()),
        ]
        .iter()
        .map(GraphicResource::type_name)
        .collect();
        for n in names {
            assert!(TYPE_NAMES.contains(&n));
        }
    }

    #[test]
    fn refs_and_remap() {
        let attrs = Attributes {
            fill_pattern: Some(ResourceId::from_num(3)),
            line_type: Some(ResourceId::from_num(4)),
            ..Default::default()
        };
        let nested = Shape::Symbol {
            symbol: ResourceId::from_num(5),
            at: DVec2::ZERO,
            rotation: 0.0,
            scale: 1.0,
            scale_y: None,
        };
        let mut sym = GraphicResource::Symbol(SymbolDef {
            items: vec![
                SymbolItem {
                    shape: circle(1.0),
                    attrs,
                    class: None,
                },
                item(nested),
            ],
            ..Default::default()
        });
        let refs = sym.refs();
        assert_eq!(
            refs,
            vec![
                (ResourceId::from_num(3), RefKind::Pattern),
                (ResourceId::from_num(4), RefKind::LineType),
                (ResourceId::from_num(5), RefKind::Symbol)
            ]
        );
        sym.remap(&|r| ResourceId::from_num(r.0.low_u64() + 10));
        assert_eq!(
            sym.refs()
                .iter()
                .map(|(r, _)| r.0.low_u64())
                .collect::<Vec<_>>(),
            vec![13, 14, 15]
        );
        let mut tile = GraphicResource::Tile(TileDef {
            symbol: ResourceId::from_num(1),
            width: 1.0,
            height: 1.0,
            row_shift: 0.0,
            angle: 0.0,
            scale: 1.0,
            units: PatternUnits::Paper,
        });
        tile.remap(&|r| ResourceId::from_num(r.0.low_u64() + 1));
        assert_eq!(
            tile.refs(),
            vec![(ResourceId::from_num(2), RefKind::Symbol)]
        );
        let mut lt = GraphicResource::LineType(LineTypeDef {
            units: PatternUnits::Paper,
            elements: vec![
                LineElement::Dash { length: 1.0 },
                LineElement::Mark {
                    symbol: ResourceId::from_num(7),
                    scale: 1.0,
                    rotate: false,
                },
            ],
        });
        lt.remap(&|_| ResourceId::from_num(8));
        assert_eq!(lt.refs(), vec![(ResourceId::from_num(8), RefKind::Symbol)]);
        let mut h = GraphicResource::Hatch(HatchDef {
            units: PatternUnits::Paper,
            lines: vec![HatchLine::solid(0.0, 1.0)],
        });
        h.remap(&|_| ResourceId::from_num(1));
        assert!(h.refs().is_empty());
        assert_eq!(RefKind::Pattern.label(), "hatch or tile");
        for k in [RefKind::Symbol, RefKind::LineType, RefKind::PlantStyle] {
            assert!(!k.label().is_empty());
        }
    }

    #[test]
    fn json_round_trip() {
        let h = GraphicResource::Hatch(HatchDef {
            units: PatternUnits::World,
            lines: vec![HatchLine {
                dash: vec![5.0, 2.0],
                color: Some(Color([1, 2, 3, 255])),
                weight: Some(0.1),
                ..HatchLine::solid(30.0, 4.0)
            }],
        });
        let v = serde_json::to_value(&h).unwrap();
        assert_eq!(v["type"], "hatch");
        assert_eq!(serde_json::from_value::<GraphicResource>(v).unwrap(), h);
        let v = serde_json::json!({"type": "line_type", "elements": [{"kind": "dash", "length": 3}, {"kind": "mark", "symbol": "00000000-0000-0000-0000-000000000004"}]});
        let lt: GraphicResource = serde_json::from_value(v).unwrap();
        let GraphicResource::LineType(l) = &lt else {
            panic!()
        };
        assert_eq!(l.units, PatternUnits::Paper);
        assert_eq!(
            l.elements[1],
            LineElement::Mark {
                symbol: ResourceId::from_num(4),
                scale: 1.0,
                rotate: true
            }
        );
    }
}
