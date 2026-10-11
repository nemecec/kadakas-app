//! Dockable palettes and saved layouts.
//!
//! The window is a dock (`egui_dock`): the drawing and the palettes are tabs. Drag a tab to move
//! it to another side or next to another tab, or out of the window area into a floating window.
//! The current layout is saved for the user when the app closes and when it changes (`layouts.json`
//! in the config folder, see `prefs`). Named layouts are saved there too: Window > Save Layout As.
//! A layout without the drawing tab is not used: the default layout is used instead.

use kadakas_l10n::tr;
use std::collections::BTreeMap;

use egui_dock::{DockState, NodeIndex};
use serde::{Deserialize, Serialize};

/// The tabs of the dock.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Tab {
    /// The drawing area (the plan or the 3D view).
    Drawing,
    Tools,
    ObjectInfo,
    Navigation,
    Resources,
    Schedule,
    Snapping,
    /// The other people in a shared document (PLAT-09).
    People,
}

impl Tab {
    pub const ALL: [Tab; 8] = [
        Tab::Drawing,
        Tab::Tools,
        Tab::ObjectInfo,
        Tab::Navigation,
        Tab::Resources,
        Tab::Schedule,
        Tab::Snapping,
        Tab::People,
    ];

    pub fn title(self) -> String {
        match self {
            Tab::Drawing => tr!("layout-drawing"),
            Tab::Tools => tr!("layout-tools"),
            Tab::ObjectInfo => tr!("layout-object-info"),
            Tab::Navigation => tr!("layout-navigation"),
            Tab::Resources => tr!("layout-resources"),
            Tab::Schedule => tr!("layout-plant-schedule"),
            Tab::Snapping => tr!("layout-snapping"),
            Tab::People => tr!("layout-people"),
        }
    }

    /// The drawing cannot be closed.
    pub fn closeable(self) -> bool {
        self != Tab::Drawing
    }
}

/// The layout that a new user gets: tools and snapping on the left, the drawing in the middle, the
/// information palettes on the right.
pub fn default_state() -> DockState<Tab> {
    let mut state = DockState::new(vec![Tab::Drawing]);
    let main = state.main_surface_mut();
    let [center, _right] = main.split_right(
        NodeIndex::root(),
        0.78,
        vec![
            Tab::ObjectInfo,
            Tab::Navigation,
            Tab::Resources,
            Tab::Schedule,
            Tab::People,
        ],
    );
    main.split_left(center, 0.82, vec![Tab::Tools, Tab::Snapping]);
    state
}

/// True if the drawing tab is in the layout.
pub fn has_drawing(state: &DockState<Tab>) -> bool {
    state.iter_all_tabs().any(|(_, t)| *t == Tab::Drawing)
}

#[derive(Serialize, Deserialize)]
struct File {
    current: DockState<Tab>,
    #[serde(default)]
    saved: BTreeMap<String, DockState<Tab>>,
}

/// The current layout and the named ones.
pub struct Layouts {
    pub current: Option<DockState<Tab>>,
    saved: BTreeMap<String, DockState<Tab>>,
    /// The text of the last saved copy, to see when the layout has changed.
    last_saved: String,
}

impl Default for Layouts {
    fn default() -> Self {
        Layouts {
            current: Some(default_state()),
            saved: BTreeMap::new(),
            last_saved: String::new(),
        }
    }
}

/// The areas (rect and viewport) of a layout that was never shown are infinite, and JSON writes
/// them as null. Zero areas read back fine and the next frame lays them out again.
fn repair(v: &mut serde_json::Value) {
    match v {
        serde_json::Value::Object(m) => {
            for area in ["rect", "viewport"] {
                let Some(rect) = m.get_mut(area) else {
                    continue;
                };
                for corner in ["min", "max"] {
                    let Some(c) = rect.get_mut(corner).and_then(|c| c.as_object_mut()) else {
                        continue;
                    };
                    for k in ["x", "y"] {
                        if c.get(k).is_some_and(serde_json::Value::is_null) {
                            c.insert(k.into(), 0.0.into());
                        }
                    }
                }
            }
            m.values_mut().for_each(repair);
        }
        serde_json::Value::Array(a) => a.iter_mut().for_each(repair),
        _ => {}
    }
}

fn text(state: &DockState<Tab>) -> String {
    serde_json::to_string(state).unwrap_or_default()
}

impl Layouts {
    /// Reads `layouts.json` from `dir`. A missing or damaged file gives the default layout.
    pub fn load_from(dir: &std::path::Path) -> (Layouts, Option<String>) {
        let path = dir.join("layouts.json");
        let mut warning = None;
        let file = match std::fs::read_to_string(&path) {
            Ok(t) => match serde_json::from_str::<serde_json::Value>(&t).and_then(|mut v| {
                repair(&mut v);
                serde_json::from_value::<File>(v)
            }) {
                Ok(f) => Some(f),
                Err(e) => {
                    warning = Some(tr!("layout-is-ignored", path = path.display(), e = e));
                    None
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                warning = Some(tr!("layout-is-ignored", path = path.display(), e = e));
                None
            }
        };
        let Some(mut f) = file else {
            return (Layouts::default(), warning);
        };
        f.saved.retain(|_, s| has_drawing(s));
        let current = if has_drawing(&f.current) {
            f.current
        } else {
            default_state()
        };
        let last_saved = text(&current);
        (
            Layouts {
                current: Some(current),
                saved: f.saved,
                last_saved,
            },
            warning,
        )
    }

    pub fn load() -> (Layouts, Option<String>) {
        match crate::prefs::dir() {
            Some(d) => Layouts::load_from(&d),
            None => (Layouts::default(), None),
        }
    }

    pub fn names(&self) -> Vec<String> {
        self.saved.keys().cloned().collect()
    }

    /// True if the layout differs from the last saved copy.
    pub fn changed(&self) -> bool {
        self.current
            .as_ref()
            .is_some_and(|s| text(s) != self.last_saved)
    }

    /// Writes `layouts.json` into `dir`.
    pub fn save_to(&mut self, dir: &std::path::Path) -> Result<(), String> {
        let Some(current) = self.current.clone() else {
            return Ok(());
        };
        let file = File {
            current,
            saved: self.saved.clone(),
        };
        let json = serde_json::to_string_pretty(&file).map_err(|e| e.to_string())?;
        crate::prefs::write_file(dir, "layouts.json", &json)?;
        self.last_saved = text(&file.current);
        Ok(())
    }

    pub fn save(&mut self) -> Result<(), String> {
        match crate::prefs::dir() {
            Some(d) => self.save_to(&d),
            None => Ok(()),
        }
    }

    /// Keeps the current layout under a name.
    pub fn save_as(&mut self, name: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err(tr!("layout-give-the-layout-a-name"));
        }
        let Some(cur) = self.current.clone() else {
            return Err(tr!("layout-no-layout"));
        };
        self.saved.insert(name.to_string(), cur);
        Ok(())
    }

    /// Makes a named layout the current one. False if there is none with that name.
    pub fn apply(&mut self, name: &str) -> bool {
        match self.saved.get(name) {
            Some(s) => {
                self.current = Some(s.clone());
                true
            }
            None => false,
        }
    }

    pub fn delete(&mut self, name: &str) -> bool {
        self.saved.remove(name).is_some()
    }

    pub fn reset(&mut self) {
        self.current = Some(default_state());
    }

    /// Shows a tab: it gets focus if it is in the layout, else it joins the focused leaf.
    pub fn show_tab(&mut self, tab: Tab) {
        let Some(state) = self.current.as_mut() else {
            return;
        };
        if let Some(path) = state.find_tab(&tab) {
            let _ = state.set_active_tab(path);
        } else {
            state.push_to_focused_leaf(tab);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("kadakas-layout-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn tabs(state: &DockState<Tab>) -> Vec<Tab> {
        let mut v: Vec<Tab> = state.iter_all_tabs().map(|(_, t)| *t).collect();
        v.sort_by_key(|t| t.title());
        v
    }

    #[test]
    fn the_default_layout_has_every_tab_once() {
        let s = default_state();
        let mut want: Vec<Tab> = Tab::ALL.to_vec();
        want.sort_by_key(|t| t.title());
        assert_eq!(tabs(&s), want);
        assert!(has_drawing(&s));
        assert!(!Tab::Drawing.closeable() && Tab::Tools.closeable());
        for t in Tab::ALL {
            assert!(!t.title().is_empty());
        }
    }

    #[test]
    fn a_layout_survives_saving_and_loading() {
        let d = temp("rt");
        let (mut l, w) = Layouts::load_from(&d);
        assert!(w.is_none());
        // Move a tab: close Snapping.
        let state = l.current.as_mut().unwrap();
        let path = state.find_tab(&Tab::Snapping).unwrap();
        state.remove_tab(path);
        assert!(l.changed() || l.last_saved.is_empty());
        l.save_as("Drafting").unwrap();
        l.save_to(&d).unwrap();
        assert!(!l.changed());
        let (back, w) = Layouts::load_from(&d);
        assert!(w.is_none(), "{w:?}");
        assert!(!tabs(back.current.as_ref().unwrap()).contains(&Tab::Snapping));
        assert_eq!(back.names(), vec!["Drafting".to_string()]);
        // Reset goes back to the default layout, and the named one comes back.
        let mut back = back;
        back.reset();
        assert!(tabs(back.current.as_ref().unwrap()).contains(&Tab::Snapping));
        assert!(back.apply("Drafting"));
        assert!(!tabs(back.current.as_ref().unwrap()).contains(&Tab::Snapping));
        assert!(!back.apply("Nope"));
        assert!(back.delete("Drafting") && !back.delete("Drafting"));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn bad_files_and_names_are_refused() {
        let d = temp("bad");
        crate::prefs::write_file(&d, "layouts.json", "{oops").unwrap();
        let (l, w) = Layouts::load_from(&d);
        assert!(w.unwrap().contains("ignored"));
        assert!(has_drawing(l.current.as_ref().unwrap()));
        // A layout without the drawing tab is replaced by the default one.
        let no_drawing = serde_json::json!({"current": DockState::new(vec![Tab::Tools])});
        crate::prefs::write_file(&d, "layouts.json", &no_drawing.to_string()).unwrap();
        let (l, _) = Layouts::load_from(&d);
        assert!(has_drawing(l.current.as_ref().unwrap()));
        let mut l = l;
        assert!(l.save_as("   ").is_err());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn show_tab_adds_a_closed_tab_and_focuses_an_open_one() {
        let mut l = Layouts::default();
        let state = l.current.as_mut().unwrap();
        let path = state.find_tab(&Tab::Schedule).unwrap();
        state.remove_tab(path);
        assert!(!tabs(l.current.as_ref().unwrap()).contains(&Tab::Schedule));
        l.show_tab(Tab::Schedule);
        assert!(tabs(l.current.as_ref().unwrap()).contains(&Tab::Schedule));
        // Showing a tab twice does not add it twice.
        l.show_tab(Tab::Schedule);
        assert_eq!(
            tabs(l.current.as_ref().unwrap())
                .iter()
                .filter(|t| **t == Tab::Schedule)
                .count(),
            1
        );
        l.current = None;
        l.show_tab(Tab::Tools);
        assert!(!l.changed());
        assert!(l.save_to(&temp("none")).is_ok());
    }
}
