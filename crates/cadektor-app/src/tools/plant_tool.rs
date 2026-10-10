//! The Plant tool and its placement modes. The math is in `cadektor_core::plants::placement`.
//! This module collects the clicks and turns them into one `Batch` command.

use cadektor_core::command::Command;
use cadektor_core::curve;
use cadektor_core::model::{Attributes, Shape};
use cadektor_core::plants::placement::{self, Pattern, PlaceSpec, Variation};
use cadektor_core::plants::schedule::group_tag;
use cadektor_core::resource::ResourceId;
use glam::DVec2;
use serde_json::{Map, Value};

use super::{Ctx, Effect, Input, Preview, State, ToolMachine};

/// How the Plant tool places plants.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PlantMode {
    /// One plant per click.
    #[default]
    Single,
    /// Along a path: click the points, Enter finishes.
    Path,
    /// A rectangular grid in a rectangle: click two corners.
    Grid,
    /// A staggered grid in a rectangle: click two corners.
    Triangular,
    /// Fill an area: click the corners, Enter finishes.
    Fill,
    /// A plant at every click, placed together when Enter finishes.
    Poly,
}

impl PlantMode {
    pub const ALL: [PlantMode; 6] = [
        PlantMode::Single,
        PlantMode::Path,
        PlantMode::Grid,
        PlantMode::Triangular,
        PlantMode::Fill,
        PlantMode::Poly,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PlantMode::Single => "Single",
            PlantMode::Path => "Along path",
            PlantMode::Grid => "Grid",
            PlantMode::Triangular => "Triangular",
            PlantMode::Fill => "Fill area",
            PlantMode::Poly => "Poly",
        }
    }

    pub fn help(self) -> &'static str {
        match self {
            PlantMode::Single => "Click to place one plant.",
            PlantMode::Path => {
                "Click the points of a path. Enter or double-click places the plants along it."
            }
            PlantMode::Grid => "Click two corners. Plants go on a rectangular grid.",
            PlantMode::Triangular => "Click two corners. Plants go on a staggered grid.",
            PlantMode::Fill => "Click the corners of an area. Enter or double-click fills it.",
            PlantMode::Poly => "Click a point for each plant. Enter or double-click places them.",
        }
    }

    pub fn next(self) -> PlantMode {
        let i = Self::ALL.iter().position(|m| *m == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }
}

/// What decides the number of plants in the Path and Fill modes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum By {
    #[default]
    Spacing,
    Count,
    /// Path only: a fixed number of plants at a fixed spacing.
    SpacingAndCount,
    /// Fill only: plants per square meter.
    Density,
}

impl By {
    pub fn label(self) -> &'static str {
        match self {
            By::Spacing => "Spacing",
            By::Count => "Count",
            By::SpacingAndCount => "Spacing and count",
            By::Density => "Density",
        }
    }
}

/// Settings of the Plant tool.
#[derive(Clone, Debug, PartialEq)]
pub struct PlantSettings {
    pub mode: PlantMode,
    /// The plant style to place. Without one, the plants take species, spread and height from the Data bar.
    pub style: Option<ResourceId>,
    pub by: By,
    pub spacing: f64,
    /// Use the spread of the plant as the spacing, so canopies just touch.
    pub spacing_is_spread: bool,
    /// Spacing between rows of a rectangular grid.
    pub spacing_y: f64,
    pub count: usize,
    /// Plants per square meter.
    pub density: f64,
    pub pattern: Pattern,
    /// Distance from the edge of the area to the first plants.
    pub margin: f64,
    /// Add a plant tag for the group.
    pub tag: bool,
    /// Size changes by up to this fraction (0 to 0.9). Zero: all plants have the same size.
    pub size_variation: f64,
    /// Turn each plant by a random angle.
    pub random_rotation: bool,
    /// The same seed gives the same plants.
    pub seed: u64,
}

impl Default for PlantSettings {
    fn default() -> Self {
        PlantSettings {
            mode: PlantMode::Single,
            style: None,
            by: By::Spacing,
            spacing: 2000.0,
            spacing_is_spread: false,
            spacing_y: 2000.0,
            count: 10,
            density: 2.0,
            pattern: Pattern::Triangular,
            margin: 0.0,
            tag: false,
            size_variation: 0.0,
            random_rotation: false,
            seed: 1,
        }
    }
}

impl PlantSettings {
    /// The `by` choices that the current mode offers.
    pub fn by_choices(&self) -> &'static [By] {
        match self.mode {
            PlantMode::Path => &[By::Spacing, By::Count, By::SpacingAndCount],
            PlantMode::Fill => &[By::Spacing, By::Count, By::Density],
            _ => &[],
        }
    }

    /// `by`, or the first choice if the mode does not offer it.
    fn effective_by(&self) -> By {
        let c = self.by_choices();
        if c.contains(&self.by) {
            self.by
        } else {
            c.first().copied().unwrap_or(By::Spacing)
        }
    }
}

/// The plant parameters for a click: the style and the overrides.
fn plant_params(cx: &Ctx) -> (Option<ResourceId>, Map<String, Value>) {
    let s = cx.settings;
    if let Some(id) = s
        .plant
        .style
        .filter(|id| cx.doc.resources.plant_style(*id).is_some())
    {
        return (Some(id), Map::new());
    }
    let mut o = Map::new();
    o.insert("botanical_name".into(), Value::String(s.species.clone()));
    o.insert("spread".into(), Value::from(s.spread));
    o.insert("height".into(), Value::from(s.plant_height));
    (None, o)
}

/// Spread of the plants that the tool places now.
fn spread_now(cx: &Ctx) -> f64 {
    plant_params(cx)
        .0
        .and_then(|id| cx.doc.resources.plant_style(id))
        .map_or(cx.settings.spread, |s| s.spread)
}

/// The command for plants at these positions: one `Batch`, so one undo step.
pub fn place_command(cx: &Ctx, positions: Vec<DVec2>) -> Command {
    let (style, overrides) = plant_params(cx);
    let spread = spread_now(cx);
    let ps = &cx.settings.plant;
    let variation = Some(Variation {
        seed: ps.seed,
        size: ps.size_variation.clamp(0.0, 0.9),
        rotation: ps.random_rotation,
    })
    .filter(Variation::is_active);
    let mut commands = vec![Command::AddPlants {
        style,
        overrides,
        positions: positions.clone(),
        layer: None,
        class: None,
        variation,
    }];
    if let (true, Some(id)) = (cx.settings.plant.tag, style) {
        commands.push(Command::AddObject {
            shape: group_tag(id, &positions, spread),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        });
    }
    Command::Batch { commands }
}

/// The placement request for the clicked points, or an error text.
pub fn spec_for(ps: &PlantSettings, pts: &[DVec2], spread: f64) -> Result<PlaceSpec, String> {
    let vertices = curve::corners(pts);
    let (spacing_x, spacing_y) = if ps.spacing_is_spread {
        (spread, spread)
    } else {
        (ps.spacing, ps.spacing_y)
    };
    let by = ps.effective_by();
    Ok(match ps.mode {
        PlantMode::Single => PlaceSpec::Single {
            at: *pts.first().ok_or("click a point")?,
        },
        PlantMode::Path => {
            if pts.len() < 2 {
                return Err("a path needs 2 or more points".into());
            }
            let spacing = matches!(by, By::Spacing | By::SpacingAndCount).then_some(spacing_x);
            let count = matches!(by, By::Count | By::SpacingAndCount).then_some(ps.count);
            PlaceSpec::Path {
                path: vertices,
                closed: false,
                spacing,
                count,
            }
        }
        PlantMode::Grid | PlantMode::Triangular => {
            let [a, b] = pts else {
                return Err("click two corners".into());
            };
            let (min, max) = (a.min(*b), a.max(*b));
            if ps.mode == PlantMode::Grid {
                PlaceSpec::Grid {
                    min,
                    max,
                    spacing: None,
                    spacing_x: Some(spacing_x),
                    spacing_y: Some(spacing_y),
                    margin: ps.margin,
                }
            } else {
                PlaceSpec::Triangular {
                    min,
                    max,
                    spacing: spacing_x,
                    margin: ps.margin,
                }
            }
        }
        PlantMode::Fill => {
            if pts.len() < 3 {
                return Err("an area needs 3 or more points".into());
            }
            PlaceSpec::Fill {
                boundary: vertices,
                spacing: (by == By::Spacing).then_some(spacing_x),
                count: (by == By::Count).then_some(ps.count),
                density: (by == By::Density).then_some(ps.density),
                pattern: ps.pattern,
                margin: ps.margin,
                holes: vec![],
            }
        }
        PlantMode::Poly => PlaceSpec::Poly { points: vertices },
    })
}

fn finish(cx: &Ctx, pts: &[DVec2]) -> Vec<Effect> {
    let result = spec_for(&cx.settings.plant, pts, spread_now(cx))
        .and_then(|spec| placement::place(&spec).map_err(|e| e.0));
    match result {
        Ok(positions) => vec![Effect::Exec(place_command(cx, positions))],
        Err(e) => vec![Effect::Notice(format!("Plant: {e}"))],
    }
}

impl ToolMachine {
    pub(super) fn handle_plant(&mut self, input: Input, cx: &Ctx) -> Vec<Effect> {
        let mode = cx.settings.plant.mode;
        match input {
            Input::Press { p, .. } => match mode {
                PlantMode::Single => finish(cx, &[p]),
                PlantMode::Grid | PlantMode::Triangular => {
                    match std::mem::replace(&mut self.state, State::Idle) {
                        State::Points(pts) if pts.len() == 1 && pts[0] != p => {
                            finish(cx, &[pts[0], p])
                        }
                        _ => {
                            self.state = State::Points(vec![p]);
                            vec![]
                        }
                    }
                }
                PlantMode::Path | PlantMode::Fill | PlantMode::Poly => {
                    match &mut self.state {
                        State::Points(pts) => {
                            if pts.last() != Some(&p) {
                                pts.push(p);
                            }
                        }
                        state => *state = State::Points(vec![p]),
                    }
                    vec![]
                }
            },
            Input::DoubleClick | Input::Enter
                if matches!(mode, PlantMode::Path | PlantMode::Fill | PlantMode::Poly) =>
            {
                match std::mem::replace(&mut self.state, State::Idle) {
                    State::Points(pts) => finish(cx, &pts),
                    _ => vec![],
                }
            }
            _ => vec![],
        }
    }

    pub(super) fn plant_preview(&self, cursor: DVec2, cx: &Ctx) -> Preview {
        let mode = cx.settings.plant.mode;
        match (&self.state, mode) {
            (State::Points(pts), PlantMode::Path | PlantMode::Poly | PlantMode::Fill) => {
                let mut v: Vec<DVec2> = pts.clone();
                v.push(cursor);
                Preview::Path {
                    vertices: curve::corners(&v),
                    closed: mode == PlantMode::Fill,
                }
            }
            (State::Points(pts), PlantMode::Grid | PlantMode::Triangular) => Preview::Box {
                min: pts[0].min(cursor),
                max: pts[0].max(cursor),
                crossing: false,
            },
            (State::Idle, PlantMode::Single) => {
                let (style, overrides) = plant_params(cx);
                let (species, spread, height) =
                    match style.and_then(|id| cx.doc.resources.plant_style(id)) {
                        Some(st) => (st.species("Plant"), st.spread, st.height),
                        None => (
                            overrides["botanical_name"]
                                .as_str()
                                .unwrap_or("")
                                .to_string(),
                            cx.settings.spread,
                            cx.settings.plant_height,
                        ),
                    };
                if spread > 0.0 {
                    Preview::Shape(Shape::Plant {
                        at: cursor,
                        species,
                        spread,
                        height,
                    })
                } else {
                    Preview::None
                }
            }
            _ => Preview::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use super::spec_for;
    use cadektor_api::Session;
    use cadektor_core::plants::placement::Pattern;
    use cadektor_core::plants::style::PlantStyle;
    use cadektor_core::resource::ResourceBody;
    use cadektor_core::resource::ResourceId;

    struct Fx {
        s: Session,
        settings: ToolSettings,
        m: ToolMachine,
        notices: Vec<String>,
    }

    impl Fx {
        fn new(mode: PlantMode) -> Fx {
            let mut m = ToolMachine::default();
            m.set_tool(Tool::Plant);
            let mut settings = ToolSettings::default();
            settings.plant.mode = mode;
            Fx {
                s: Session::default(),
                settings,
                m,
                notices: vec![],
            }
        }

        fn style(&mut self) -> ResourceId {
            let style = PlantStyle {
                spread: 1000.0,
                height: 2000.0,
                id_code: "AB".into(),
                ..PlantStyle::default()
            };
            let id = ResourceId(
                self.s
                    .editor
                    .execute(&Command::AddResource {
                        name: "Box".into(),
                        body: ResourceBody::plant_style(style),
                    })
                    .unwrap()
                    .created[0],
            );
            self.settings.plant.style = Some(id);
            id
        }

        fn send(&mut self, input: Input) -> Vec<Effect> {
            let fx = {
                let cx = Ctx {
                    doc: self.s.editor.doc(),
                    selection: &self.s.selection,
                    settings: &self.settings,
                    tol: 5.0,
                };
                self.m.handle(input, &cx)
            };
            for e in &fx {
                match e {
                    Effect::Exec(c) => {
                        self.s.editor.execute(c).unwrap();
                    }
                    Effect::SetSelection(_) => {}
                    Effect::Notice(n) => self.notices.push(n.clone()),
                }
            }
            fx
        }

        fn click(&mut self, x: f64, y: f64) {
            self.send(Input::Press {
                p: DVec2::new(x, y),
                shift: false,
            });
        }

        fn plants(&self) -> usize {
            self.s
                .editor
                .doc()
                .objects
                .values()
                .filter(|o| matches!(o.shape, Shape::Plant { .. }))
                .count()
        }

        fn preview(&self, at: DVec2) -> Preview {
            let cx = Ctx {
                doc: self.s.editor.doc(),
                selection: &self.s.selection,
                settings: &self.settings,
                tol: 5.0,
            };
            self.m.preview(at, &cx)
        }
    }

    #[test]
    fn single_uses_the_data_bar_values_without_a_style() {
        let mut f = Fx::new(PlantMode::Single);
        f.click(10.0, 20.0);
        let o = f.s.editor.doc().objects.values().next().unwrap();
        assert!(
            matches!(&o.shape, Shape::Plant { at, species, spread, height }
            if *at == DVec2::new(10.0, 20.0) && species == "Acer campestre" && *spread == 4000.0 && *height == 8000.0)
        );
        assert!(o.data.is_empty());
    }

    #[test]
    fn single_with_a_style_links_the_plant() {
        let mut f = Fx::new(PlantMode::Single);
        let id = f.style();
        f.click(0.0, 0.0);
        let o = f.s.editor.doc().objects.values().next().unwrap();
        assert_eq!(cadektor_core::plants::link::link(o).unwrap().style, id);
        assert!(matches!(o.shape, Shape::Plant { spread, .. } if spread == 1000.0));
        // A style that was deleted falls back to the Data bar values.
        f.s.editor
            .execute(&Command::DeleteResource { id, reassign: None })
            .unwrap_err();
    }

    #[test]
    fn path_mode_places_on_enter_in_one_undo_step() {
        let mut f = Fx::new(PlantMode::Path);
        f.style();
        f.settings.plant.spacing = 500.0;
        f.click(0.0, 0.0);
        f.click(2000.0, 0.0);
        assert!(f.m.is_busy() && f.plants() == 1 - 1);
        assert!(matches!(
            f.preview(DVec2::new(5.0, 5.0)),
            Preview::Path { closed: false, .. }
        ));
        f.send(Input::Enter);
        assert_eq!(f.plants(), 5);
        assert!(!f.m.is_busy());
        assert!(f.s.editor.undo());
        assert_eq!(f.plants(), 0);
        // By count, and by spacing and count.
        f.settings.plant.by = By::Count;
        f.settings.plant.count = 3;
        f.click(0.0, 0.0);
        f.click(1000.0, 0.0);
        f.send(Input::DoubleClick);
        assert_eq!(f.plants(), 3);
    }

    #[test]
    fn grid_and_triangular_take_two_corners() {
        let mut f = Fx::new(PlantMode::Grid);
        f.settings.plant.spacing = 500.0;
        f.settings.plant.spacing_y = 1000.0;
        f.click(0.0, 0.0);
        assert!(matches!(
            f.preview(DVec2::new(1000.0, 1000.0)),
            Preview::Box { .. }
        ));
        f.click(1000.0, 1000.0);
        assert_eq!(f.plants(), 3 * 2);
        f.settings.plant.mode = PlantMode::Triangular;
        f.settings.plant.spacing = 500.0;
        f.click(0.0, 0.0);
        f.click(1000.0, 1000.0);
        assert_eq!(f.plants(), 6 + 8);
        // A second click on the same point starts over.
        f.click(5.0, 5.0);
        f.click(5.0, 5.0);
        assert!(f.m.is_busy());
        f.send(Input::Enter);
        assert!(f.m.is_busy(), "Enter does nothing in the grid modes");
    }

    #[test]
    fn fill_by_spacing_count_and_density_with_a_tag() {
        let mut f = Fx::new(PlantMode::Fill);
        f.style();
        f.settings.plant.tag = true;
        f.settings.plant.pattern = Pattern::Rectangular;
        f.settings.plant.spacing = 1000.0;
        for (x, y) in [(0.0, 0.0), (2000.0, 0.0), (2000.0, 2000.0), (0.0, 2000.0)] {
            f.click(x, y);
        }
        assert!(matches!(
            f.preview(DVec2::ZERO),
            Preview::Path { closed: true, .. }
        ));
        f.send(Input::Enter);
        assert_eq!(f.plants(), 9);
        assert!(
            f.s.editor
                .doc()
                .objects
                .values()
                .any(|o| matches!(o.shape, Shape::PlantTag { .. }))
        );
        f.s.editor.undo();
        assert_eq!(f.s.editor.doc().objects.len(), 0);
        f.settings.plant.tag = false;
        f.settings.plant.by = By::Count;
        f.settings.plant.count = 7;
        for (x, y) in [(0.0, 0.0), (2000.0, 0.0), (2000.0, 2000.0)] {
            f.click(x, y);
        }
        f.send(Input::Enter);
        assert_eq!(f.plants(), 7);
        f.settings.plant.by = By::Density;
        f.settings.plant.density = 4.0;
        for (x, y) in [(0.0, 0.0), (2000.0, 0.0), (2000.0, 2000.0), (0.0, 2000.0)] {
            f.click(x, y);
        }
        f.send(Input::Enter);
        assert!(f.plants() > 7 + 5);
    }

    #[test]
    fn spacing_can_follow_the_spread_of_the_style() {
        let mut f = Fx::new(PlantMode::Path);
        f.style(); // spread 1000
        f.settings.plant.spacing = 250.0;
        f.settings.plant.spacing_is_spread = true;
        f.click(0.0, 0.0);
        f.click(3000.0, 0.0);
        f.send(Input::Enter);
        assert_eq!(f.plants(), 4, "plants 1000 apart on 3000 mm");
        f.settings.plant.mode = PlantMode::Triangular;
        f.click(0.0, 0.0);
        f.click(2000.0, 0.0);
        assert_eq!(f.plants(), 4 + 3);
    }

    #[test]
    fn variation_settings_make_varied_plants() {
        let mut f = Fx::new(PlantMode::Poly);
        f.style();
        f.settings.plant.size_variation = 0.3;
        f.settings.plant.random_rotation = true;
        f.settings.plant.seed = 9;
        for i in 0..6 {
            f.click(i as f64 * 2000.0, 0.0);
        }
        f.send(Input::Enter);
        let objs: Vec<_> =
            f.s.editor
                .doc()
                .objects
                .values()
                .filter(|o| matches!(o.shape, Shape::Plant { .. }))
                .collect();
        let sizes: std::collections::HashSet<u64> = objs
            .iter()
            .map(|o| match o.shape {
                Shape::Plant { spread, .. } => spread.to_bits(),
                _ => 0,
            })
            .collect();
        assert!(sizes.len() > 1);
        assert!(
            objs.iter()
                .any(|o| cadektor_core::plants::link::rotation(o) > 0.0)
        );
        // No variation: no rotation data.
        f.settings.plant.size_variation = 0.0;
        f.settings.plant.random_rotation = false;
        f.click(0.0, 5000.0);
        f.send(Input::Enter);
        assert_eq!(f.plants(), 7);
        let last = f.s.editor.doc().objects.values().last().unwrap();
        assert!(!last.data.contains_key("rotation"));
    }

    #[test]
    fn poly_places_a_plant_at_each_click() {
        let mut f = Fx::new(PlantMode::Poly);
        f.click(0.0, 0.0);
        f.click(0.0, 0.0);
        f.click(100.0, 100.0);
        f.click(300.0, 0.0);
        f.send(Input::Enter);
        assert_eq!(f.plants(), 3);
    }

    #[test]
    fn too_few_points_and_bad_input_give_a_notice() {
        let mut f = Fx::new(PlantMode::Path);
        f.click(0.0, 0.0);
        f.send(Input::Enter);
        assert!(f.notices[0].contains("2 or more"), "{:?}", f.notices);
        f.settings.plant.mode = PlantMode::Fill;
        f.click(0.0, 0.0);
        f.click(10.0, 0.0);
        f.send(Input::Enter);
        assert!(f.notices[1].contains("3 or more"));
        f.settings.plant.spacing = 1.0;
        f.settings.plant.mode = PlantMode::Poly;
        f.send(Input::Enter);
        assert_eq!(f.notices.len(), 2, "Enter with no points does nothing");
        f.settings.plant.mode = PlantMode::Path;
        f.click(0.0, 0.0);
        f.click(100.0, 0.0);
        f.send(Input::Enter);
        assert!(f.notices[2].contains("spacing"));
        assert_eq!(f.plants(), 0);
    }

    #[test]
    fn single_preview_shows_the_style() {
        let mut f = Fx::new(PlantMode::Single);
        assert!(
            matches!(f.preview(DVec2::ZERO), Preview::Shape(Shape::Plant { spread, .. }) if spread == 4000.0)
        );
        f.style();
        assert!(
            matches!(f.preview(DVec2::ZERO), Preview::Shape(Shape::Plant { spread, .. }) if spread == 1000.0)
        );
        f.settings.spread = 0.0;
        f.settings.plant.style = None;
        assert_eq!(f.preview(DVec2::ZERO), Preview::None);
        f.settings.plant.mode = PlantMode::Grid;
        assert_eq!(f.preview(DVec2::ZERO), Preview::None);
    }

    #[test]
    fn modes_cycle_and_describe_themselves() {
        let mut m = PlantMode::Single;
        for _ in 0..PlantMode::ALL.len() {
            m = m.next();
        }
        assert_eq!(m, PlantMode::Single);
        for m in PlantMode::ALL {
            assert!(!m.label().is_empty() && !m.help().is_empty());
        }
        for b in [By::Spacing, By::Count, By::SpacingAndCount, By::Density] {
            assert!(!b.label().is_empty());
        }
        let mut ps = PlantSettings::default();
        assert!(ps.by_choices().is_empty());
        ps.mode = PlantMode::Path;
        ps.by = By::Density;
        assert_eq!(ps.effective_by(), By::Spacing);
        ps.mode = PlantMode::Fill;
        assert_eq!(ps.by_choices().len(), 3);
        assert!(spec_for(&ps, &[], 1.0).is_err());
        ps.mode = PlantMode::Single;
        assert!(spec_for(&ps, &[], 1.0).is_err());
        ps.mode = PlantMode::Grid;
        assert!(spec_for(&ps, &[DVec2::ZERO], 1.0).is_err());
    }
}
