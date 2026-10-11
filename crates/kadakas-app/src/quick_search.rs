//! Quick search for commands (the F key): type a few letters of a command, pick it with the
//! arrow keys, run it with Enter.
//!
//! The search is fuzzy: the letters of the query must appear in the name in the same order. A
//! match at the start of a word, letters next to each other and a short name score higher.

use crate::keymap::Action;
use crate::layout::Tab;
use crate::panels::MenuAction;
use crate::theme::ThemeChoice;
use kadakas_l10n::tr;

/// What a result does when you run it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuickCommand {
    Action(Action),
    Menu(MenuAction),
    Theme(ThemeChoice),
    /// Switch a snap category on or off.
    SnapToggle(&'static str),
    /// Show a palette (a tab of the dock).
    ShowTab(Tab),
    SaveLayout,
    ResetLayout,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuickItem {
    pub label: String,
    /// The key that runs the same command, for the right side of the list.
    pub keys: String,
    pub command: QuickCommand,
}

/// How well `query` matches `label`: `None` if the letters do not all appear in order.
/// Higher is better.
pub fn score(query: &str, label: &str) -> Option<i32> {
    let q: Vec<char> = query
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    if q.is_empty() {
        return Some(0);
    }
    let l: Vec<char> = label.chars().collect();
    let low: Vec<char> = l.iter().flat_map(|c| c.to_lowercase()).collect();
    // Same length after lowercasing in practice; stop otherwise.
    if low.len() != l.len() {
        return None;
    }
    let mut score = 0i32;
    let mut qi = 0usize;
    let mut last: Option<usize> = None;
    for (i, c) in low.iter().enumerate() {
        if qi < q.len() && *c == q[qi] {
            let word_start = i == 0 || !l[i - 1].is_alphanumeric();
            score += 10;
            if word_start {
                score += 15;
            }
            if last == Some(i.wrapping_sub(1)) && i > 0 {
                score += 12;
            }
            if qi == 0 && i == 0 {
                score += 10;
            }
            last = Some(i);
            qi += 1;
        }
    }
    if qi < q.len() {
        return None;
    }
    // A whole-name match and a short name come first.
    if low.iter().collect::<String>() == q.iter().collect::<String>() {
        score += 100;
    }
    Some(score - (l.len() as i32) / 4)
}

/// The indices of the items that match, best first. With an empty query, all items in order.
pub fn search(items: &[QuickItem], query: &str, limit: usize) -> Vec<usize> {
    let mut hits: Vec<(i32, usize)> = items
        .iter()
        .enumerate()
        .filter_map(|(i, it)| score(query, &it.label).map(|s| (s, i)))
        .collect();
    hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    hits.into_iter().take(limit).map(|(_, i)| i).collect()
}

#[derive(Default)]
pub struct QuickSearch {
    open: bool,
    query: String,
    selected: usize,
    focus: bool,
}

/// How many results show.
const SHOWN: usize = 12;

impl QuickSearch {
    pub fn open(&mut self) {
        self.open = true;
        self.query.clear();
        self.selected = 0;
        self.focus = true;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    #[cfg(test)]
    pub fn close(&mut self) {
        self.open = false;
    }

    /// Shows the box. Returns the command that the user ran.
    pub fn show(&mut self, ctx: &egui::Context, items: &[QuickItem]) -> Option<QuickCommand> {
        if !self.open {
            return None;
        }
        let mut chosen = None;
        let (down, up, enter, esc) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
            )
        });
        if esc {
            self.open = false;
            return None;
        }
        let hits = search(items, &self.query, SHOWN);
        if down && !hits.is_empty() {
            self.selected = (self.selected + 1) % hits.len();
        }
        if up && !hits.is_empty() {
            self.selected = (self.selected + hits.len() - 1) % hits.len();
        }
        if enter && let Some(i) = hits.get(self.selected) {
            chosen = Some(items[*i].command);
        }
        egui::Window::new(tr!("quick-search-quick-search"))
            .id(egui::Id::new("win-quick-search"))
            .title_bar(false)
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 60.0))
            .resizable(false)
            .collapsible(false)
            .fixed_size(egui::vec2(420.0, 0.0))
            .show(ctx, |ui| {
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut self.query)
                        .hint_text(tr!("quick-search-type-a-command"))
                        .desired_width(f32::INFINITY),
                );
                if self.focus {
                    edit.request_focus();
                    self.focus = false;
                }
                if edit.changed() {
                    self.selected = 0;
                }
                ui.add_space(4.0);
                if hits.is_empty() {
                    ui.weak(tr!("quick-search-no-command-matches"));
                }
                for (row, i) in hits.iter().enumerate() {
                    let it = &items[*i];
                    let r = ui.horizontal(|ui| {
                        let r = ui.selectable_label(row == self.selected, &it.label);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.weak(&it.keys);
                        });
                        r
                    });
                    if r.inner.clicked() {
                        chosen = Some(it.command);
                    }
                }
            });
        if chosen.is_some() {
            self.open = false;
        }
        chosen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(label: &str, command: QuickCommand) -> QuickItem {
        QuickItem {
            label: label.into(),
            keys: String::new(),
            command,
        }
    }

    fn items() -> Vec<QuickItem> {
        vec![
            item("Fillet tool", QuickCommand::Action(Action::ToolFillet)),
            item("Chamfer tool", QuickCommand::Action(Action::ToolChamfer)),
            item(
                "Duplicate Array",
                QuickCommand::Action(Action::CmdDuplicateArray),
            ),
            item("Dark theme", QuickCommand::Theme(ThemeChoice::Dark)),
            item("Zoom to Fit", QuickCommand::Menu(MenuAction::ZoomFit)),
            item("Undo", QuickCommand::Menu(MenuAction::Undo)),
        ]
    }

    #[test]
    fn letters_must_appear_in_order() {
        assert!(score("fil", "Fillet tool").is_some());
        assert!(score("flt", "Fillet tool").is_some());
        assert!(score("tlf", "Fillet tool").is_none());
        assert!(score("xyz", "Fillet tool").is_none());
        assert_eq!(score("", "anything"), Some(0));
        assert_eq!(score("   ", "anything"), Some(0));
        assert!(
            score("FILLET", "fillet tool").is_some(),
            "case does not matter"
        );
    }

    #[test]
    fn better_matches_come_first() {
        let it = items();
        // Word starts beat letters scattered in the middle.
        let r = search(&it, "dupa", 10);
        assert_eq!(it[r[0]].label, "Duplicate Array");
        let r = search(&it, "undo", 10);
        assert_eq!(it[r[0]].label, "Undo");
        let r = search(&it, "tool", 10);
        assert_eq!(r.len(), 2);
        assert!(search(&it, "qqq", 10).is_empty());
        // An empty query lists everything in order, up to the limit.
        assert_eq!(search(&it, "", 3), vec![0, 1, 2]);
        // A whole word and a shorter name win over a longer one.
        let it = vec![
            item(
                "Dark theme and more",
                QuickCommand::Theme(ThemeChoice::Dark),
            ),
            item("Dark", QuickCommand::Theme(ThemeChoice::Dark)),
        ];
        assert_eq!(search(&it, "dark", 5), vec![1, 0]);
    }

    #[test]
    fn the_box_opens_filters_and_runs_a_command() {
        let ctx = egui::Context::default();
        let mut q = QuickSearch::default();
        let it = items();
        assert!(q.show(&ctx, &it).is_none(), "a closed box does nothing");
        q.open();
        assert!(q.is_open());
        let mut ran = None;
        let _ = ctx.run_ui(Default::default(), |_| {
            ran = q.show(&ctx, &it);
        });
        assert!(ran.is_none());
        // Type, then press Enter.
        q.query = "cham".into();
        let enter = egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        let _ = ctx.run_ui(
            egui::RawInput {
                events: vec![enter],
                ..Default::default()
            },
            |_| {
                ran = q.show(&ctx, &it);
            },
        );
        assert_eq!(ran, Some(QuickCommand::Action(Action::ToolChamfer)));
        assert!(!q.is_open());
        // Escape closes without running.
        q.open();
        let esc = egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        let mut ran2 = Some(QuickCommand::SaveLayout);
        let _ = ctx.run_ui(
            egui::RawInput {
                events: vec![esc],
                ..Default::default()
            },
            |_| {
                ran2 = q.show(&ctx, &it);
            },
        );
        assert!(ran2.is_none() && !q.is_open());
        q.open();
        q.close();
        assert!(!q.is_open());
    }
}
