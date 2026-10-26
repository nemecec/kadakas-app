//! The welcome dialog (PLAT-26): it only draws and passes the choices to `kadakas-welcome`.
//!
//! Keys: Enter is Start and Esc is the second button (Use defaults on the first start, Cancel
//! when the dialog is shown again), unless a drop-down is open (it takes the key) or a button has
//! the focus (it takes Enter). Tab goes through the cards, the language, the units, the number
//! format, More options, and the two buttons, in this order. The text is made again each frame
//! from the active language, so a change of the language shows at once.

use egui::{Align2, FontId, Sense, Stroke, StrokeKind, Vec2, WidgetInfo, WidgetType};
use kadakas_feedback::CrashPolicy;
use kadakas_l10n::tr;
use kadakas_update::settings::UpdateMode;
use kadakas_welcome::{Button, Choices, Kind, Mode, Navigation, NumberStyle, Theme, UnitSystem};

/// The width of the dialog, in points. The window is 560 points high: at 150 percent that is 840
/// pixels, which fits a 1080 pixel high screen. The content scrolls when the window is lower.
pub const WIDTH: f32 = 600.0;

pub struct WelcomeState {
    pub kind: Kind,
    pub choices: Choices,
    /// What the dialog showed when it opened. Only what the person changes from this is stored.
    pub initial: Choices,
    /// The defaults of the system, for the first start.
    pub detected: Choices,
    /// The locales of the system, the most wanted first.
    pub system: Vec<String>,
    /// The 3D modeler is installed. Without it only Landscape CAD can be chosen.
    pub modeler_installed: bool,
    focus_pending: bool,
}

impl WelcomeState {
    pub fn new(
        kind: Kind,
        choices: Choices,
        detected: Choices,
        system: Vec<String>,
        modeler_installed: bool,
    ) -> WelcomeState {
        WelcomeState {
            kind,
            initial: choices.clone(),
            choices,
            detected,
            system,
            modeler_installed,
            focus_pending: true,
        }
    }

    /// The tag of the language that the dialog speaks now.
    pub fn language_tag(&self) -> &'static str {
        self.choices.language_tag(&self.system)
    }
}

fn mode_texts(m: Mode) -> (String, String) {
    match m {
        Mode::Modeler => (
            tr!("welcome-mode-modeler"),
            tr!("welcome-mode-modeler-hint"),
        ),
        Mode::Landscape => (
            tr!("welcome-mode-landscape"),
            tr!("welcome-mode-landscape-hint"),
        ),
    }
}

fn theme_text(t: Theme) -> String {
    match t {
        Theme::System => tr!("theme-system"),
        Theme::Light => tr!("theme-light"),
        Theme::Dark => tr!("theme-dark"),
    }
}

fn nav_text(n: Navigation) -> String {
    match n {
        Navigation::Mouse => tr!("welcome-nav-mouse"),
        Navigation::Trackpad => tr!("welcome-nav-trackpad"),
    }
}

fn crash_text(p: CrashPolicy) -> String {
    match p {
        CrashPolicy::Ask => tr!("crash-ui-policy-ask"),
        CrashPolicy::Always => tr!("crash-ui-policy-always"),
        CrashPolicy::Never => tr!("crash-ui-policy-never"),
    }
}

fn updates_text(m: UpdateMode) -> String {
    match m {
        UpdateMode::AutoUpdate => tr!("app-updates-mode-auto-update"),
        UpdateMode::AutoDownload => tr!("app-updates-mode-auto-download"),
        UpdateMode::NotifyOnly => tr!("app-updates-mode-notify-only"),
    }
}

fn units_text(u: UnitSystem) -> String {
    match u {
        UnitSystem::Metric => tr!("welcome-units-metric"),
        UnitSystem::Imperial => tr!("welcome-units-imperial"),
    }
}

/// The names of the two buttons for a kind of dialog.
pub fn button_texts(kind: Kind) -> (String, String) {
    match kind {
        Kind::FirstRun => (tr!("welcome-start"), tr!("welcome-use-defaults")),
        Kind::Update => (tr!("welcome-start"), tr!("welcome-cancel")),
        Kind::Manual => (tr!("welcome-save"), tr!("welcome-cancel")),
    }
}

/// A card that the person picks one of. It is a radio button for the keyboard and for readers.
fn card(
    ui: &mut egui::Ui,
    selected: bool,
    enabled: bool,
    title: &str,
    hint: &str,
) -> egui::Response {
    let height = 72.0;
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), sense);
    response
        .widget_info(|| WidgetInfo::selected(WidgetType::RadioButton, enabled, selected, title));
    let visuals = ui.visuals();
    let fill = if selected {
        visuals.selection.bg_fill.gamma_multiply(0.5)
    } else if response.hovered() && enabled {
        visuals.widgets.hovered.weak_bg_fill
    } else {
        visuals.widgets.inactive.weak_bg_fill
    };
    let stroke = if response.has_focus() {
        Stroke::new(2.0, visuals.selection.stroke.color)
    } else if selected {
        visuals.selection.stroke
    } else {
        visuals.widgets.noninteractive.bg_stroke
    };
    let text = if enabled {
        visuals.text_color()
    } else {
        visuals.weak_text_color()
    };
    let weak = visuals.weak_text_color();
    let painter = ui.painter();
    painter.rect(rect, 6.0, fill, stroke, StrokeKind::Inside);
    let inner = rect.shrink(10.0);
    painter.text(
        inner.left_top(),
        Align2::LEFT_TOP,
        title,
        FontId::proportional(17.0),
        text,
    );
    let galley = painter.layout(
        hint.to_string(),
        FontId::proportional(13.0),
        weak,
        inner.width(),
    );
    painter.galley(inner.left_top() + Vec2::new(0.0, 26.0), galley, weak);
    if selected {
        painter.text(
            inner.right_top(),
            Align2::RIGHT_TOP,
            "\u{2713}",
            FontId::proportional(17.0),
            text,
        );
    }
    response
}

/// Draws the dialog for one frame. Returns the button that was pressed.
pub fn show(ui: &mut egui::Ui, st: &mut WelcomeState) -> Option<Button> {
    let ctx = ui.ctx().clone();
    // A drop-down that is open takes Esc and Enter itself.
    let popup_open = egui::Popup::is_any_open(&ctx);
    let mut pressed = None;
    let (primary_text, secondary_text) = button_texts(st.kind);

    egui::CentralPanel::default().show(ui, |ui| {
        // The buttons stay at the bottom; the rest scrolls when the window is too low (a large
        // scale, a small screen). The content comes first, so Tab visits it before the buttons.
        let buttons_height = 52.0;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .max_height((ui.available_height() - buttons_height).max(80.0))
            .show(ui, |ui| {
                content(ui, st);
            });
        ui.separator();
        let size = Vec2::new(140.0, 28.0);
        ui.horizontal(|ui| {
            let gap = ui.spacing().item_spacing.x;
            ui.add_space((ui.available_width() - 2.0 * size.x - gap).max(0.0));
            if ui
                .add(egui::Button::new(&secondary_text).min_size(size))
                .clicked()
            {
                pressed = Some(Button::Secondary);
            }
            let start = ui.add(
                egui::Button::new(&primary_text)
                    .min_size(size)
                    .fill(ui.visuals().selection.bg_fill),
            );
            if st.focus_pending {
                start.request_focus();
                st.focus_pending = false;
            }
            if start.clicked() {
                pressed = Some(Button::Primary);
            }
        });
    });

    if pressed.is_none() && !popup_open {
        let no_focus = ctx.memory(|m| m.focused()).is_none();
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            pressed = Some(Button::Secondary);
        } else if no_focus
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
        {
            pressed = Some(Button::Primary);
        }
    }
    pressed
}

fn content(ui: &mut egui::Ui, st: &mut WelcomeState) {
    ui.heading(tr!("welcome-title"));
    ui.label(tr!("welcome-intro"));
    ui.add_space(10.0);

    // 1. How to work.
    ui.strong(tr!("welcome-work-heading"));
    ui.add_space(4.0);
    ui.columns(2, |cols| {
        for (col, mode) in cols.iter_mut().zip(Mode::ALL) {
            let (title, hint) = mode_texts(mode);
            let enabled = mode != Mode::Modeler || st.modeler_installed;
            let hint = if enabled {
                hint
            } else {
                tr!("welcome-not-installed")
            };
            if card(col, st.choices.mode == mode, enabled, &title, &hint).clicked() {
                st.choices.mode = mode;
            }
        }
    });
    ui.add_space(12.0);

    // 2. Language, 3. Units and number format.
    egui::Grid::new("welcome-main")
        .num_columns(2)
        .spacing([16.0, 8.0])
        .show(ui, |ui| {
            ui.label(tr!("welcome-language"));
            let shown = kadakas_l10n::languages()
                .iter()
                .find(|l| l.tag == st.language_tag())
                .map_or("", |l| l.native_name);
            egui::ComboBox::from_id_salt("welcome-language")
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    for l in kadakas_l10n::languages() {
                        let on = l.tag == st.language_tag();
                        if ui.selectable_label(on, l.native_name).clicked() {
                            st.choices.language = Some(l.tag.to_string());
                            ui.ctx().request_repaint();
                        }
                    }
                });
            ui.end_row();

            ui.label(tr!("welcome-units"));
            ui.horizontal(|ui| {
                for u in UnitSystem::ALL {
                    ui.selectable_value(&mut st.choices.units, u, units_text(u));
                }
            });
            ui.end_row();

            ui.label(tr!("welcome-number-format"));
            egui::ComboBox::from_id_salt("welcome-number")
                .selected_text(st.choices.number.example())
                .show_ui(ui, |ui| {
                    for n in NumberStyle::ALL {
                        ui.selectable_value(&mut st.choices.number, n, n.example());
                    }
                });
            ui.end_row();
        });
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(tr!(
            "welcome-example",
            length = st.choices.length_example(st.language_tag()),
            number = st.choices.number.example()
        ))
        .weak(),
    );
    ui.add_space(10.0);

    // More options, folded.
    egui::CollapsingHeader::new(tr!("welcome-more"))
        .id_salt("welcome-more")
        .default_open(false)
        .show(ui, |ui| {
            egui::Grid::new("welcome-more-grid")
                .num_columns(2)
                .spacing([16.0, 8.0])
                .show(ui, |ui| {
                    ui.label(tr!("welcome-theme"));
                    egui::ComboBox::from_id_salt("welcome-theme")
                        .selected_text(theme_text(st.choices.theme))
                        .show_ui(ui, |ui| {
                            for t in Theme::ALL {
                                ui.selectable_value(&mut st.choices.theme, t, theme_text(t));
                            }
                        });
                    ui.end_row();

                    ui.label(tr!("welcome-navigation"));
                    egui::ComboBox::from_id_salt("welcome-navigation")
                        .selected_text(nav_text(st.choices.navigation))
                        .show_ui(ui, |ui| {
                            for n in Navigation::ALL {
                                ui.selectable_value(&mut st.choices.navigation, n, nav_text(n));
                            }
                        });
                    ui.end_row();

                    ui.label(tr!("welcome-crash"));
                    egui::ComboBox::from_id_salt("welcome-crash")
                        .selected_text(crash_text(st.choices.crash))
                        .show_ui(ui, |ui| {
                            for p in CrashPolicy::ALL {
                                ui.selectable_value(&mut st.choices.crash, p, crash_text(p));
                            }
                        });
                    ui.end_row();

                    ui.label(tr!("welcome-updates"));
                    egui::ComboBox::from_id_salt("welcome-updates")
                        .selected_text(updates_text(st.choices.updates))
                        .show_ui(ui, |ui| {
                            for m in UpdateMode::ALL {
                                ui.selectable_value(&mut st.choices.updates, m, updates_text(m));
                            }
                        });
                    ui.end_row();
                });
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(tr!("welcome-crash-hint"))
                    .weak()
                    .small(),
            );
        });
    ui.add_space(12.0);
    ui.label(tr!("welcome-later"));
    ui.label(egui::RichText::new(tr!("welcome-later-help")).weak());
    if st.kind != Kind::FirstRun {
        ui.label(egui::RichText::new(tr!("welcome-applies-later")).weak());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, Key, Modifiers, Pos2, Rect, Shape};
    use kadakas_welcome::{SystemLocale, choices::detect};

    /// A text on the screen with its box and the box that clips it.
    #[derive(Debug, Clone)]
    struct Seen {
        text: String,
        rect: Rect,
        clip: Rect,
    }

    fn collect(shape: &Shape, clip: Rect, out: &mut Vec<Seen>) {
        match shape {
            Shape::Text(t) => out.push(Seen {
                text: t.galley.text().to_string(),
                rect: Rect::from_min_size(t.pos, t.galley.size()),
                clip,
            }),
            Shape::Vec(v) => v.iter().for_each(|s| collect(s, clip, out)),
            _ => {}
        }
    }

    struct Harness {
        ctx: egui::Context,
        st: WelcomeState,
        screen: Vec2,
        seen: Vec<Seen>,
    }

    impl Harness {
        fn new(kind: Kind) -> Harness {
            let system = vec!["en-US".to_string()];
            let detected = detect(&SystemLocale::from_languages(system.clone()), true);
            Harness {
                ctx: egui::Context::default(),
                st: WelcomeState::new(kind, detected.clone(), detected, system, true),
                screen: Vec2::new(WIDTH, 560.0),
                seen: Vec::new(),
            }
        }

        fn frame(&mut self, events: Vec<Event>) -> Option<Button> {
            let input = egui::RawInput {
                events,
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.screen)),
                ..Default::default()
            };
            let mut pressed = None;
            let tag = self.st.language_tag();
            let st = &mut self.st;
            let out = self.ctx.run_ui(input, |ui| {
                kadakas_l10n::with_language(tag, || pressed = show(ui, st));
            });
            self.seen.clear();
            for c in &out.shapes {
                collect(&c.shape, c.clip_rect, &mut self.seen);
            }
            pressed
        }

        /// Frames until the layout settles (the first frames place the widgets).
        fn settle(&mut self) {
            for _ in 0..3 {
                self.frame(vec![]);
            }
        }

        fn has(&self, text: &str) -> bool {
            self.seen.iter().any(|s| s.text == text)
        }

        fn seen(&self, text: &str) -> &Seen {
            self.seen
                .iter()
                .find(|s| s.text == text)
                .unwrap_or_else(|| panic!("no text {text:?} in {:#?}", self.seen))
        }

        /// The text is on the screen and not cut off by a scroll area.
        fn visible(&self, text: &str) -> bool {
            let s = self.seen(text);
            let screen = Rect::from_min_size(Pos2::ZERO, self.screen);
            screen.contains_rect(s.rect.shrink(0.5)) && s.clip.contains_rect(s.rect.shrink(0.5))
        }
    }

    fn key_event(key: Key, pressed: bool) -> Event {
        Event::Key {
            key,
            physical_key: Some(key),
            pressed,
            repeat: false,
            modifiers: Modifiers::NONE,
        }
    }

    fn key(key: Key) -> Vec<Event> {
        vec![key_event(key, true)]
    }

    impl Harness {
        /// A key press and its release in the next frame. Returns the first button pressed.
        fn tap(&mut self, k: Key) -> Option<Button> {
            let down = self.frame(key(k));
            let up = self.frame(vec![key_event(k, false)]);
            down.or(up)
        }
    }

    #[test]
    fn the_first_screen_is_calm_and_the_fold_is_closed() {
        let mut h = Harness::new(Kind::FirstRun);
        h.settle();
        for t in [
            "Welcome to Kadakas",
            "How do you want to work?",
            "3D Modeler",
            "Landscape CAD",
            "Simple push and pull shape modeling.",
            "Plans, sheets and plants.",
            "Language",
            "English",
            "Units",
            "Metric (mm, m)",
            "Imperial (ft, in)",
            "Number format",
            "1,234.56",
            "More options",
            "You can change all of this later in Preferences (in the 3D modeler: the View menu).",
            "Start",
            "Use defaults",
        ] {
            assert!(h.has(t), "missing {t:?} in {:#?}", h.seen);
        }
        assert!(
            h.has("Example: 3,048 mm and 1,234.56")
                || h.seen.iter().any(|s| s.text.starts_with("Example:"))
        );
        // The fold is closed: the options in it are not drawn.
        for t in ["Theme", "Crash reports", "Updates", "Mouse or trackpad"] {
            assert!(!h.has(t), "{t} is inside the fold");
        }
        // No trademark names and no message ids in the text.
        for s in &h.seen {
            assert!(!s.text.contains("welcome-"), "{}", s.text);
            for brand in ["SketchUp", "Vectorworks", "AutoCAD"] {
                assert!(!s.text.contains(brand), "{}", s.text);
            }
        }
        assert!(h.visible("Start") && h.visible("Use defaults"));
    }

    #[test]
    fn enter_is_start_and_escape_is_the_second_button() {
        let mut h = Harness::new(Kind::FirstRun);
        h.settle();
        assert_eq!(h.tap(Key::Enter), Some(Button::Primary));
        let mut h = Harness::new(Kind::FirstRun);
        h.settle();
        assert_eq!(h.tap(Key::Escape), Some(Button::Secondary));
        // With nothing focused (a click on the empty window took the focus away), Enter starts.
        let mut h = Harness::new(Kind::FirstRun);
        h.settle();
        if let Some(id) = h.ctx.memory(|m| m.focused()) {
            h.ctx.memory_mut(|m| m.surrender_focus(id));
        }
        h.frame(vec![]);
        assert!(h.ctx.memory(|m| m.focused()).is_none());
        assert_eq!(h.tap(Key::Enter), Some(Button::Primary));
        // Nothing is pressed by itself.
        let mut h = Harness::new(Kind::FirstRun);
        h.settle();
        assert_eq!(h.frame(vec![]), None);
    }

    #[test]
    fn the_buttons_are_named_for_the_reason_of_the_dialog() {
        for (kind, a, b) in [
            (Kind::FirstRun, "Start", "Use defaults"),
            (Kind::Update, "Start", "Cancel"),
            (Kind::Manual, "Save", "Cancel"),
        ] {
            let mut h = Harness::new(kind);
            h.settle();
            assert!(h.has(a) && h.has(b), "{kind:?}");
            assert_eq!(
                h.has("Some changes apply the next time Kadakas starts."),
                kind != Kind::FirstRun
            );
        }
    }

    #[test]
    fn tab_visits_the_cards_first_and_a_key_picks_a_card() {
        let mut h = Harness::new(Kind::FirstRun);
        h.settle();
        assert_eq!(h.st.choices.mode, Mode::Modeler);
        // From the Start button, Tab goes around to the first card, then to the second.
        h.tap(Key::Tab);
        h.tap(Key::Tab);
        // Enter on a focused card picks it and does not start.
        let pressed = h.tap(Key::Enter);
        assert_eq!(pressed, None, "Enter on a card does not start");
        assert_eq!(h.st.choices.mode, Mode::Landscape);
    }

    #[test]
    fn a_change_of_the_language_shows_in_the_next_frame() {
        let mut h = Harness::new(Kind::FirstRun);
        h.settle();
        assert!(h.has("Start") && h.has("English"));
        h.st.choices.language = Some("et".to_string());
        h.frame(vec![]);
        h.frame(vec![]);
        assert!(
            h.has("Alusta") && h.has("Eesti") && h.has("Kasuta vaikeväärtusi"),
            "{:#?}",
            h.seen
        );
        assert!(!h.has("Start"));
        assert!(
            h.has("Seda kõike saad hiljem muuta eelistustes (3D-modelleerijas: menüüs Vaade).")
        );
        // Back to the language of the system.
        h.st.choices.language = None;
        h.frame(vec![]);
        h.frame(vec![]);
        assert!(h.has("Start"));
    }

    #[test]
    fn the_example_follows_the_units_and_the_number_format_live() {
        let mut h = Harness::new(Kind::FirstRun);
        h.settle();
        let example = |h: &Harness| {
            h.seen
                .iter()
                .find(|s| s.text.starts_with("Example:"))
                .map(|s| s.text.clone())
                .unwrap()
        };
        h.st.choices.units = UnitSystem::Metric;
        h.st.choices.number = NumberStyle::CommaDot;
        h.settle();
        assert_eq!(example(&h), "Example: 3,048 mm and 1,234.56");
        h.st.choices.units = UnitSystem::Imperial;
        h.st.choices.number = NumberStyle::SpaceComma;
        h.settle();
        assert_eq!(example(&h), "Example: 10' 0\" and 1\u{a0}234,56");
        h.st.choices.units = UnitSystem::Metric;
        h.st.choices.number = NumberStyle::PlainComma;
        h.settle();
        assert_eq!(example(&h), "Example: 3048 mm and 1234,56");
    }

    /// The window in points on a screen of `pixels` at `scale`: the window height is the
    /// height of the screen less the room of the title bar and the task bar (80 points), at most
    /// 560 points, and never less than the smallest window.
    fn window_on(pixels: (f32, f32), scale: f32) -> Vec2 {
        let points = Vec2::new(pixels.0 / scale, pixels.1 / scale);
        Vec2::new(WIDTH.min(points.x), (points.y - 80.0).clamp(420.0, 560.0))
    }

    #[test]
    fn the_buttons_stay_in_view_on_real_screens_in_both_languages() {
        // 1366 x 768 at 125 percent, 1920 x 1080 at 150 percent, 1280 x 720 at 100 percent and
        // the smallest window. The sizes are in points, as the window gets them.
        for (pixels, scale) in [
            ((1366.0, 768.0), 1.25),
            ((1920.0, 1080.0), 1.5),
            ((1280.0, 720.0), 1.0),
            ((800.0, 600.0), 1.0),
        ] {
            for tag in ["en-US", "et"] {
                {
                    let mut h = Harness::new(Kind::FirstRun);
                    h.screen = window_on(pixels, scale);
                    h.ctx.set_pixels_per_point(scale);
                    h.st.choices.language = Some(tag.to_string());
                    h.settle();
                    let (start, second) = if tag == "et" {
                        ("Alusta", "Kasuta vaikeväärtusi")
                    } else {
                        ("Start", "Use defaults")
                    };
                    let what = format!("{pixels:?} at {scale} in {tag}, window {:?}", h.screen);
                    assert!(h.visible(start), "{start}: {what}");
                    assert!(h.visible(second), "{second}: {what}");
                    // Nothing runs out of the window on the right (long Estonian texts wrap).
                    for s in &h.seen {
                        assert!(
                            s.rect.right() <= h.screen.x + 0.5,
                            "{:?} is too wide: {what}",
                            s.text
                        );
                    }
                    // The heading and the first card are on the first screen.
                    let heading = if tag == "et" {
                        "Tere tulemast Kadakasse"
                    } else {
                        "Welcome to Kadakas"
                    };
                    assert!(h.visible(heading), "{heading}: {what}");
                }
            }
        }
    }

    #[test]
    fn a_missing_modeler_leaves_only_landscape_cad() {
        let mut h = Harness::new(Kind::FirstRun);
        h.st.modeler_installed = false;
        h.st.choices.mode = Mode::Landscape;
        h.settle();
        assert!(h.has("Not installed"));
        // The disabled card cannot be picked with the keyboard.
        h.tap(Key::Tab);
        h.tap(Key::Enter);
        assert_eq!(h.st.choices.mode, Mode::Landscape);
    }

    #[test]
    fn every_text_of_the_dialog_exists_in_estonian_and_no_message_id_shows() {
        for tag in ["en-US", "et"] {
            let mut h = Harness::new(Kind::FirstRun);
            h.st.choices.language = Some(tag.to_string());
            h.settle();
            assert!(h.seen.len() > 12);
            for s in &h.seen {
                assert!(
                    !s.text.contains("welcome-") && !s.text.is_empty(),
                    "{tag}: {}",
                    s.text
                );
            }
        }
        for (kind, tag) in [(Kind::Manual, "et"), (Kind::Update, "et")] {
            let (a, b) = kadakas_l10n::with_language(tag, || button_texts(kind));
            assert!(!a.is_empty() && !b.is_empty() && a != "Start");
        }
    }
}
