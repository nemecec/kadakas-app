//! The link between a plant object and its style.
//!
//! A plant is either "by instance" or "by style".
//!
//! - A plant without a link keeps its own species, spread and height in its shape.
//!   Files from before plant styles load as these plants, unchanged.
//! - A plant with a link has `data["plant"] = {"style": <id>, "overrides": {...}}`.
//!   The style gives every field. Each key in `overrides` replaces one style field for
//!   this plant only.
//!
//! The shape of a linked plant also holds species, spread and height. They are a copy of
//! the effective values, so bounds, picking and snapping need no document access.
//! Commands keep the copy up to date.

use serde_json::{Map, Value};

use super::style::{PlantForm, PlantGraphic, PlantStyle};
use crate::model::{Color, Document, Object, Shape};
use crate::resource::ResourceId;

/// The key in `Object::data` that holds the link.
pub const KEY: &str = "plant";

/// Keys in `Object::data` that only commands may write. `SetData` refuses them, so a link cannot
/// point to a missing style or skip the checks of the style commands.
pub const RESERVED_DATA_KEYS: [&str; 1] = [KEY];

/// A plant's link to a style, with its overrides.
#[derive(Clone, Debug, PartialEq)]
pub struct PlantLink {
    pub style: ResourceId,
    pub overrides: Map<String, Value>,
}

/// The link of a plant object, if it has a valid one.
pub fn link(obj: &Object) -> Option<PlantLink> {
    let v = obj.data.get(KEY)?;
    let style = ResourceId(v.get("style")?.as_u64()?);
    let overrides = v.get("overrides").and_then(Value::as_object).cloned().unwrap_or_default();
    Some(PlantLink { style, overrides })
}

/// The style a plant refers to, without copying the overrides.
pub fn style_of(obj: &Object) -> Option<ResourceId> {
    obj.data.get(KEY)?.get("style")?.as_u64().map(ResourceId)
}

/// Writes or removes the link of an object.
pub fn set_link(obj: &mut Object, l: Option<&PlantLink>) {
    match l {
        None => {
            obj.data.remove(KEY);
        }
        Some(l) => {
            let mut v = serde_json::json!({ "style": l.style.0 });
            if !l.overrides.is_empty() {
                v["overrides"] = Value::Object(l.overrides.clone());
            }
            obj.data.insert(KEY.into(), v);
        }
    }
}

/// A style with the overrides applied. Fails if an override has an unknown key or a bad value.
pub fn merge(style: &PlantStyle, overrides: &Map<String, Value>) -> Result<PlantStyle, String> {
    let mut v = serde_json::to_value(style).expect("a style is JSON");
    let obj = v.as_object_mut().expect("a style is an object");
    for (k, x) in overrides {
        if !obj.contains_key(k) {
            return Err(format!("unknown plant field '{k}'"));
        }
        obj.insert(k.clone(), x.clone());
    }
    let merged: PlantStyle = serde_json::from_value(v).map_err(|e| e.to_string())?;
    merged.validate()?;
    Ok(merged)
}

/// What a plant object shows: its style fields after the overrides.
#[derive(Clone, Debug, PartialEq)]
pub struct Effective {
    /// The style that the plant refers to, if the style exists.
    pub style_id: Option<ResourceId>,
    pub style_name: String,
    pub style: PlantStyle,
}

/// The fields that drawing needs, found without copying the whole style.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Look {
    pub graphic: PlantGraphic,
    pub form: PlantForm,
    pub fill: Option<Color>,
    pub pen: Option<Color>,
}

/// How a plant looks: graphic, form and colors, from its style and overrides. A plant without a
/// style gets the default look. This is cheap, so drawing can call it for every plant in every frame.
pub fn look(doc: &Document, obj: &Object) -> Look {
    let Some(l) = link(obj) else { return Look::default() };
    let Some(base) = doc.resources.plant_style(l.style) else { return Look::default() };
    let mut look = Look { graphic: base.graphic, form: base.form, fill: base.fill, pen: base.pen };
    // A bad override value is ignored here, like `effective` ignores a bad override set.
    let get = |k: &str| l.overrides.get(k);
    if let Some(g) = get("graphic").and_then(|v| serde_json::from_value(v.clone()).ok()) {
        look.graphic = g;
    }
    if let Some(f) = get("form").and_then(|v| serde_json::from_value(v.clone()).ok()) {
        look.form = f;
    }
    if let Some(v) = get("fill") {
        look.fill = serde_json::from_value(v.clone()).ok().flatten();
    }
    if let Some(v) = get("pen") {
        look.pen = serde_json::from_value(v.clone()).ok().flatten();
    }
    look
}

/// The effective fields of a plant object, or `None` if the object is not a plant.
/// A plant without a style (or with a style that no longer exists) gives its shape fields.
pub fn effective(doc: &Document, obj: &Object) -> Option<Effective> {
    let Shape::Plant { species, spread, height, .. } = &obj.shape else { return None };
    if let Some(l) = link(obj)
        && let Some(base) = doc.resources.plant_style(l.style)
    {
        let name = doc.resources.get(l.style).map(|r| r.name.clone()).unwrap_or_default();
        // Without overrides there is nothing to merge.
        let merged = if l.overrides.is_empty() { Ok(base.clone()) } else { merge(base, &l.overrides) };
        if let Ok(style) = merged {
            return Some(Effective { style_id: Some(l.style), style_name: name, style });
        }
    }
    let style = PlantStyle { botanical_name: species.clone(), spread: *spread, height: *height, ..PlantStyle::default() };
    Some(Effective { style_id: None, style_name: species.clone(), style })
}

/// Copies the effective species, spread and height of a linked plant into its shape.
/// Does nothing for plants without a valid link.
pub fn sync_shape(doc: &Document, obj: &mut Object) {
    let Some(e) = effective(doc, obj) else { return };
    if e.style_id.is_none() {
        return;
    }
    if let Shape::Plant { species, spread, height, .. } = &mut obj.shape {
        *species = e.style.species(&e.style_name);
        *spread = e.style.spread;
        *height = e.style.height;
    }
}

/// After a plant's shape was edited: a linked plant keeps its style, and each shape field
/// that no longer matches the effective value becomes an override. `before` is the plant
/// before the edit.
pub fn adopt_shape_edit(doc: &Document, before: &Object, after: &mut Object) {
    let (Some(mut l), Some(e)) = (link(before), effective(doc, before)) else { return };
    if e.style_id.is_none() {
        return;
    }
    let (Shape::Plant { species: old_sp, spread: old_w, height: old_h, .. }, Shape::Plant { species, spread, height, .. }) =
        (&before.shape, &after.shape)
    else {
        // The shape is no longer a plant. The link means nothing.
        set_link(after, None);
        return;
    };
    if species != old_sp {
        l.overrides.insert("botanical_name".into(), Value::String(species.clone()));
    }
    if spread != old_w {
        l.overrides.insert("spread".into(), Value::from(*spread));
    }
    if height != old_h {
        l.overrides.insert("height".into(), Value::from(*height));
    }
    set_link(after, Some(&l));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{FIRST_LAYER, NONE_CLASS, ObjectId};
    use crate::resource::{Resource, ResourceBody};
    use glam::DVec2;
    use serde_json::json;

    fn plant(species: &str, spread: f64, height: f64) -> Object {
        Object::new(ObjectId(50), FIRST_LAYER, NONE_CLASS, Shape::Plant { at: DVec2::ZERO, species: species.into(), spread, height })
    }

    fn doc_with_style() -> Document {
        let mut doc = Document::default();
        let style = PlantStyle { botanical_name: "Acer campestre".into(), spread: 5000.0, height: 9000.0, price: 40.0, ..PlantStyle::default() };
        doc.resources.push(Resource { id: ResourceId(7), name: "Field maple".into(), body: ResourceBody::plant_style(style) });
        doc
    }

    fn linked(overrides: Value) -> Object {
        let mut o = plant("old", 1.0, 1.0);
        set_link(&mut o, Some(&PlantLink { style: ResourceId(7), overrides: overrides.as_object().unwrap().clone() }));
        o
    }

    #[test]
    fn link_round_trips_through_data() {
        let mut o = plant("a", 1.0, 1.0);
        assert_eq!(link(&o), None);
        let l = PlantLink { style: ResourceId(7), overrides: json!({"height": 5.0}).as_object().unwrap().clone() };
        set_link(&mut o, Some(&l));
        assert_eq!(link(&o), Some(l));
        set_link(&mut o, Some(&PlantLink { style: ResourceId(7), overrides: Map::new() }));
        assert_eq!(o.data[KEY], json!({"style": 7}));
        set_link(&mut o, None);
        assert!(o.data.is_empty());
        o.data.insert(KEY.into(), json!("junk"));
        assert_eq!(link(&o), None);
    }

    #[test]
    fn unstyled_plant_keeps_its_own_fields() {
        let doc = doc_with_style();
        let e = effective(&doc, &plant("Quercus", 6000.0, 12000.0)).unwrap();
        assert_eq!(e.style_id, None);
        assert_eq!((e.style.botanical_name.as_str(), e.style.spread, e.style.height), ("Quercus", 6000.0, 12000.0));
        let mut o = plant("Quercus", 6000.0, 12000.0);
        sync_shape(&doc, &mut o);
        assert_eq!(o.shape, plant("Quercus", 6000.0, 12000.0).shape);
        let line = Object::new(ObjectId(1), FIRST_LAYER, NONE_CLASS, Shape::Circle { center: DVec2::ZERO, radius: 1.0 });
        assert!(effective(&doc, &line).is_none());
    }

    #[test]
    fn style_fields_and_overrides_make_the_effective_values() {
        let doc = doc_with_style();
        let e = effective(&doc, &linked(json!({}))).unwrap();
        assert_eq!((e.style_id, e.style.spread, e.style.price), (Some(ResourceId(7)), 5000.0, 40.0));
        assert_eq!(e.style_name, "Field maple");
        let e = effective(&doc, &linked(json!({"height": 3000.0, "price": 10.0}))).unwrap();
        assert_eq!((e.style.height, e.style.price, e.style.spread), (3000.0, 10.0, 5000.0));
    }

    #[test]
    fn dangling_or_bad_link_falls_back_to_the_shape() {
        let doc = doc_with_style();
        let mut o = linked(json!({}));
        set_link(&mut o, Some(&PlantLink { style: ResourceId(99), overrides: Map::new() }));
        assert_eq!(effective(&doc, &o).unwrap().style_id, None);
        let bad = linked(json!({"nope": 1}));
        assert_eq!(effective(&doc, &bad).unwrap().style_id, None);
    }

    #[test]
    fn merge_checks_keys_values_and_numbers() {
        let s = PlantStyle::default();
        assert!(merge(&s, &json!({"spread": 100.0}).as_object().unwrap().clone()).is_ok());
        assert!(merge(&s, &json!({"x": 1}).as_object().unwrap().clone()).unwrap_err().contains("unknown"));
        assert!(merge(&s, &json!({"spread": "wide"}).as_object().unwrap().clone()).is_err());
        assert!(merge(&s, &json!({"spread": -1.0}).as_object().unwrap().clone()).is_err());
    }

    #[test]
    fn look_follows_the_style_and_its_overrides() {
        let mut doc = doc_with_style();
        if let Some(r) = doc.resources.get_mut(ResourceId(7)) {
            let ResourceBody::PlantStyle(s) = &mut r.body else { panic!() };
            s.graphic = PlantGraphic::Star;
            s.fill = Some(Color([1, 2, 3, 255]));
        }
        assert_eq!(look(&doc, &plant("a", 1.0, 1.0)), Look::default());
        let l = look(&doc, &linked(json!({})));
        assert_eq!((l.graphic, l.form, l.fill, l.pen), (PlantGraphic::Star, PlantForm::Tree, Some(Color([1, 2, 3, 255])), None));
        let l = look(&doc, &linked(json!({"graphic": "clover", "form": "conical", "fill": null, "pen": [9, 9, 9, 255]})));
        assert_eq!((l.graphic, l.form, l.fill, l.pen), (PlantGraphic::Clover, PlantForm::Conical, None, Some(Color([9, 9, 9, 255]))));
        // Bad override values are ignored. A missing style gives the default look.
        let l = look(&doc, &linked(json!({"graphic": 5, "form": "cube", "fill": "red"})));
        assert_eq!((l.graphic, l.form, l.fill), (PlantGraphic::Star, PlantForm::Tree, None));
        let mut o = linked(json!({}));
        set_link(&mut o, Some(&PlantLink { style: ResourceId(99), overrides: Map::new() }));
        assert_eq!(look(&doc, &o), Look::default());
        // The look agrees with the effective style.
        let o = linked(json!({"graphic": "rosette", "pen": [4, 4, 4, 255]}));
        let (e, l) = (effective(&doc, &o).unwrap(), look(&doc, &o));
        assert_eq!((e.style.graphic, e.style.form, e.style.fill, e.style.pen), (l.graphic, l.form, l.fill, l.pen));
    }

    #[test]
    fn sync_shape_copies_effective_values() {
        let doc = doc_with_style();
        let mut o = linked(json!({"height": 3000.0}));
        sync_shape(&doc, &mut o);
        assert!(matches!(&o.shape, Shape::Plant { species, spread, height, .. }
            if species == "Acer campestre" && *spread == 5000.0 && *height == 3000.0));
    }

    #[test]
    fn shape_edit_of_a_linked_plant_becomes_overrides() {
        let doc = doc_with_style();
        let mut before = linked(json!({}));
        sync_shape(&doc, &mut before);
        let mut after = before.clone();
        after.shape = Shape::Plant { at: DVec2::new(5.0, 5.0), species: "Mine".into(), spread: 2000.0, height: 7000.0 };
        adopt_shape_edit(&doc, &before, &mut after);
        let l = link(&after).unwrap();
        assert_eq!(l.overrides["botanical_name"], "Mine");
        assert_eq!(l.overrides["spread"], 2000.0);
        assert_eq!(l.overrides["height"], 7000.0);
        // A move alone adds no override.
        let mut moved = before.clone();
        moved.shape = Shape::Plant { at: DVec2::new(1.0, 1.0), species: "Acer campestre".into(), spread: 5000.0, height: 9000.0 };
        adopt_shape_edit(&doc, &before, &mut moved);
        assert!(link(&moved).unwrap().overrides.is_empty());
        // Changing the kind of shape drops the link.
        let mut circle = before.clone();
        circle.shape = Shape::Circle { center: DVec2::ZERO, radius: 1.0 };
        adopt_shape_edit(&doc, &before, &mut circle);
        assert_eq!(link(&circle), None);
        // An unlinked plant is left alone.
        let plain = plant("a", 1.0, 1.0);
        let mut p2 = plain.clone();
        p2.shape = Shape::Plant { at: DVec2::ZERO, species: "b".into(), spread: 2.0, height: 2.0 };
        adopt_shape_edit(&doc, &plain, &mut p2);
        assert_eq!(link(&p2), None);
    }
}
