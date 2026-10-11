//! Names of values that the core crates know only by an API id (`miter`, `shrub_mound`,
//! `left_isometric`). The ids are for agents and files, so they stay in English. The UI shows
//! the names from here, which come from the message files: the message of the id `shrub_mound`
//! in the domain `form` is `enum-form-shrub-mound`.

use kadakas_l10n::msg;
use kadakas_render::StandardView;

/// The name of a value in the language of the UI. A value without a message shows its id with
/// spaces instead of underscores (a newer core crate may know more values than this program).
pub fn name(domain: &str, id: &str) -> String {
    let key = format!("enum-{domain}-{}", id.replace('_', "-"));
    let text = kadakas_l10n::translate(&key, &[]);
    if text == key {
        id.replace('_', " ")
    } else {
        text
    }
}

/// The name of a standard view, for example "Left Isometric".
pub fn standard_view(view: StandardView) -> String {
    name("view", view.id())
}

/// Every message of this module. The CI check finds the message ids in this list, and the test
/// `every_value_has_a_message` compares it with the values of the core crates.
#[allow(dead_code)]
pub const KNOWN: &[&str] = &[
    msg!("enum-join-miter"),
    msg!("enum-join-round"),
    msg!("enum-join-bevel"),
    msg!("enum-slope-none"),
    msg!("enum-slope-edges"),
    msg!("enum-slope-cross"),
    msg!("enum-slope-angle"),
    msg!("enum-slope-rise-run"),
    msg!("enum-condition-good"),
    msg!("enum-condition-fair"),
    msg!("enum-condition-poor"),
    msg!("enum-condition-dead"),
    msg!("enum-action-retain"),
    msg!("enum-action-remove"),
    msg!("enum-action-relocate"),
    msg!("enum-category-tree"),
    msg!("enum-category-shrub"),
    msg!("enum-category-perennial"),
    msg!("enum-category-grass"),
    msg!("enum-category-groundcover"),
    msg!("enum-graphic-circle"),
    msg!("enum-graphic-cloud"),
    msg!("enum-graphic-star"),
    msg!("enum-graphic-spokes"),
    msg!("enum-graphic-rosette"),
    msg!("enum-graphic-clover"),
    msg!("enum-form-tree"),
    msg!("enum-form-columnar"),
    msg!("enum-form-conical"),
    msg!("enum-form-spreading"),
    msg!("enum-form-shrub-mound"),
    msg!("enum-form-weeping"),
    msg!("enum-form-palm"),
    msg!("enum-form-columnar-conifer"),
    msg!("enum-form-multi-stem"),
    msg!("enum-form-hedge"),
    msg!("enum-quantity-count"),
    msg!("enum-quantity-area"),
    msg!("enum-water-need-very-low"),
    msg!("enum-water-need-low"),
    msg!("enum-water-need-moderate"),
    msg!("enum-water-need-high"),
    msg!("enum-line-type-mainline"),
    msg!("enum-line-type-lateral"),
    msg!("enum-line-type-drip"),
    msg!("enum-phase-existing"),
    msg!("enum-phase-new"),
    msg!("enum-phase-demolished"),
    msg!("enum-view-plan"),
    msg!("enum-view-top"),
    msg!("enum-view-bottom"),
    msg!("enum-view-front"),
    msg!("enum-view-back"),
    msg!("enum-view-left"),
    msg!("enum-view-right"),
    msg!("enum-view-left-isometric"),
    msg!("enum-view-right-isometric"),
    msg!("enum-view-left-rear-isometric"),
    msg!("enum-view-right-rear-isometric"),
    msg!("enum-projection-ortho"),
    msg!("enum-projection-perspective"),
    msg!("enum-summary-none"),
    msg!("enum-summary-sum"),
    msg!("enum-summary-average"),
    msg!("enum-summary-min"),
    msg!("enum-summary-max"),
    msg!("enum-summary-count"),
    msg!("enum-terminator-arrow"),
    msg!("enum-terminator-open-arrow"),
    msg!("enum-terminator-tick"),
    msg!("enum-terminator-dot"),
    msg!("enum-terminator-none"),
    msg!("enum-justify-left"),
    msg!("enum-justify-center"),
    msg!("enum-justify-right"),
    msg!("enum-frame-none"),
    msg!("enum-frame-box"),
    msg!("enum-frame-underline"),
    msg!("enum-frame-circle"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_core::hardscape::Join;
    use kadakas_core::irrigation::LineType;
    use kadakas_core::plants::care::WaterNeed;
    use kadakas_core::plants::style::{PlantCategory, PlantForm, PlantGraphic, QuantityType};
    use kadakas_core::plants::tree::{TreeAction, TreeCondition};
    use kadakas_core::viewrules::Phase;

    fn key(domain: &str, id: &str) -> String {
        format!("enum-{domain}-{}", id.replace('_', "-"))
    }

    #[test]
    fn every_value_has_a_message() {
        let mut want: Vec<String> = Vec::new();
        want.extend(Join::ALL.iter().map(|v| key("join", v.id())));
        for id in ["none", "edges", "cross", "angle", "rise_run"] {
            want.push(key("slope", id));
        }
        want.extend(TreeCondition::ALL.iter().map(|v| key("condition", v.id())));
        want.extend(TreeAction::ALL.iter().map(|v| key("action", v.id())));
        want.extend(PlantCategory::ALL.iter().map(|v| key("category", v.id())));
        want.extend(PlantGraphic::ALL.iter().map(|v| key("graphic", v.id())));
        want.extend(PlantForm::ALL.iter().map(|v| key("form", v.id())));
        for v in [QuantityType::Count, QuantityType::Area] {
            want.push(key("quantity", v.id()));
        }
        want.extend(WaterNeed::ALL.iter().map(|v| key("water-need", v.id())));
        want.extend(LineType::ALL.iter().map(|v| key("line-type", v.id())));
        want.extend(Phase::ALL.iter().map(|v| key("phase", v.id())));
        want.extend(StandardView::ALL.iter().map(|v| key("view", v.id())));
        for id in ["ortho", "perspective"] {
            want.push(key("projection", id));
        }
        for id in ["none", "sum", "average", "min", "max", "count"] {
            want.push(key("summary", id));
        }
        want.extend(
            kadakas_ui_core::style_edit::TERMINATORS
                .iter()
                .map(|c| key(kadakas_ui_core::style_edit::TERMINATOR_DOMAIN, c.id)),
        );
        want.extend(
            kadakas_ui_core::style_edit::JUSTIFICATIONS
                .iter()
                .map(|c| key(kadakas_ui_core::style_edit::JUSTIFY_DOMAIN, c.id)),
        );
        want.extend(
            kadakas_ui_core::style_edit::FRAMES
                .iter()
                .map(|c| key(kadakas_ui_core::style_edit::FRAME_DOMAIN, c.id)),
        );
        for k in &want {
            assert!(KNOWN.contains(&k.as_str()), "{k} is not in KNOWN");
        }
        for k in KNOWN {
            assert!(want.contains(&k.to_string()), "{k} names no value");
        }
    }

    #[test]
    fn names_follow_the_language() {
        assert_eq!(name("join", "miter"), "miter");
        assert_eq!(standard_view(StandardView::LeftIsometric), "Left Isometric");
        kadakas_l10n::with_language("et", || {
            assert_eq!(name("join", "miter"), "terav");
            assert_eq!(standard_view(StandardView::Plan), "Plaan (ülalt)");
        });
        // A value without a message shows its id.
        assert_eq!(name("join", "new_kind"), "new kind");
    }
}
