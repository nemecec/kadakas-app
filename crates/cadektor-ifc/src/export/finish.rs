//! Making the element entity, its data, and the relationships at the end of the file.

use super::*;

impl<W: Write> Exporter<'_, W> {
    /// The up direction (0, 0, 1).
    pub(super) fn up(&mut self) -> R<u64> {
        if let Some(u) = self.up {
            return Ok(u);
        }
        let u = self.ent("IFCDIRECTION", vec![Value::reals(&[0.0, 0.0, 1.0])])?;
        self.up = Some(u);
        Ok(u)
    }

    /// A local placement in the container of a layer. `loc` is relative to the layer.
    pub(super) fn place_in_layer(&mut self, layer: LayerId, loc: [f64; 3]) -> R<u64> {
        let (_, _, container_placement) = self.container(layer)?;
        let z = loc[2] + self.base_z(layer);
        self.placement(Some(container_placement), [loc[0], loc[1], z])
    }

    /// The class and predefined type for an object kind. A class that the writer cannot make
    /// falls back to the default of the kind, with a note.
    pub(super) fn resolve_class(
        &mut self,
        kind: &str,
        mapped: &Mapped,
    ) -> (&'static str, Option<String>, Option<String>) {
        let (default, default_pre) = default_class(kind);
        let entry = |name: &str| {
            CLASSES
                .iter()
                .find(|(n, _, _)| n.eq_ignore_ascii_case(name.trim()))
        };
        let default_entry = entry(default).expect("default classes are in the table");
        let (class, wanted_default) = match mapped.ifc_class.as_deref() {
            None => (default_entry, true),
            Some(req) => match entry(req) {
                Some(e) => (e, e.0 == default),
                None => {
                    self.note(format!(
                        "the data map asks for {req} on {kind}, which IFC4 export cannot write: used {default} instead (supported: IfcSlab, IfcCovering, IfcGeographicElement, IfcBuildingElementProxy)"
                    ));
                    (default_entry, true)
                }
            },
        };
        let fallback = if wanted_default {
            default_pre.map(String::from)
        } else {
            None
        };
        let mut object_type = None;
        let pre = match mapped
            .predefined_type
            .as_deref()
            .map(|p| p.trim().to_ascii_uppercase())
        {
            None => fallback,
            Some(p) if class.2.contains(&p.as_str()) => Some(p),
            // The Data Manager template says VEGETATION, which IFC4x3 knows. IFC4 writes it as a user-defined type.
            Some(p) if p == "VEGETATION" && class.0 == "IfcGeographicElement" => {
                Some("USERDEFINED".to_string())
            }
            Some(p) => {
                self.note(format!(
                    "{p} is not a predefined type of {}: used the default",
                    class.0
                ));
                fallback
            }
        };
        if pre.as_deref() == Some("USERDEFINED") {
            let wanted = mapped
                .predefined_type
                .as_deref()
                .map(|p| p.trim().to_ascii_uppercase())
                .filter(|p| p != "USERDEFINED");
            let vegetation = kind == "plant" || kind == "existing_tree" || kind == "landscape_area";
            object_type = Some(wanted.unwrap_or_else(|| {
                if vegetation {
                    "VEGETATION".to_string()
                } else {
                    kind.to_ascii_uppercase()
                }
            }));
        }
        (class.1, pre, object_type)
    }

    /// The name of an object for the IFC `Name` attribute.
    pub(super) fn object_name(&self, obj: &Object) -> String {
        match props::property(self.doc, obj, &["Name"]) {
            Prop::Value(Json::String(s)) if !s.trim().is_empty() => s,
            _ => obj.shape.kind().replace('_', " "),
        }
    }

    /// Writes the element entity and registers it in its container and presentation layer.
    pub(super) fn element(&mut self, e: ElementIn<'_>, mapped: &Mapped) -> R<(u64, &'static str)> {
        let (class, pre, object_type) = self.resolve_class(e.kind, mapped);
        let shape = if e.body.reps.is_empty() {
            Value::Null
        } else {
            Value::Ref(self.ent(
                "IFCPRODUCTDEFINITIONSHAPE",
                vec![
                    Value::Null,
                    Value::Null,
                    Value::refs(e.body.reps.iter().copied()),
                ],
            )?)
        };
        let id = self.ent(
            class,
            vec![
                self.guid("obj", e.obj.id.0),
                Value::Null,
                Self::name_value(&e.name),
                Value::Null,
                object_type.map_or(Value::Null, Value::Str),
                Value::Ref(e.placement),
                shape,
                Value::s(e.obj.id.0.to_string()),
                pre.map_or(Value::Null, Value::Enum),
            ],
        )?;
        self.register(e.obj, id, e.body.main)?;
        Ok((id, class))
    }

    pub(super) fn register(&mut self, obj: &Object, id: u64, main_rep: Option<u64>) -> R<()> {
        let (key, _, _) = self.container(obj.layer)?;
        self.contained.entry(key).or_default().push(id);
        self.elements.insert(obj.id.0, id);
        if let Some(rep) = main_rep
            && self.opts.classes_as_layers
        {
            self.class_layers.entry(obj.class).or_default().push(rep);
        }
        Ok(())
    }

    /// The property sets of the data map and the records of an object.
    pub(super) fn data_psets(&mut self, obj: &Object, mapped: &Mapped) -> R<()> {
        for p in mapped.problems.iter().take(20) {
            self.note(format!("data map, object {}: {p}", obj.id));
        }
        for (set, props) in &mapped.properties {
            let list: Vec<(String, Value)> = props
                .iter()
                .map(|(n, v)| (n.clone(), Self::nominal(v)))
                .collect();
            self.pset(set, list);
        }
        for name in obj.data.keys() {
            let Some(rec) = record::values(obj, name) else {
                continue;
            };
            let list: Vec<(String, Value)> = record::fields_only(rec)
                .iter()
                .map(|(k, v)| (k.clone(), json_nominal(v)))
                .collect();
            self.pset(&format!("ePset_{name}"), list);
        }
        self.write_psets(obj)
    }

    /// An `IfcElementQuantity`. Each quantity is a kind (`length`, `area`, `volume`, `count`), a name and a value.
    pub(super) fn quantities(
        &mut self,
        obj: &Object,
        name: &str,
        list: &[(&str, String, f64)],
    ) -> R<()> {
        if list.is_empty() {
            return Ok(());
        }
        let mut ids = Vec::new();
        for (kind, n, v) in list {
            let entity = match *kind {
                "length" => "IFCQUANTITYLENGTH",
                "area" => "IFCQUANTITYAREA",
                "volume" => "IFCQUANTITYVOLUME",
                _ => "IFCQUANTITYCOUNT",
            };
            let value = if *kind == "count" {
                Value::Real(v.max(0.0).round())
            } else {
                Value::Real(r4(*v))
            };
            ids.push(self.ent(
                entity,
                vec![Value::s(n), Value::Null, Value::Null, value, Value::Null],
            )?);
        }
        let q = self.ent(
            "IFCELEMENTQUANTITY",
            vec![
                self.guid(&format!("qto:{name}"), obj.id.0),
                Value::Null,
                Value::s(name),
                Value::Null,
                Value::Null,
                Value::refs(ids),
            ],
        )?;
        self.define_by(obj, &format!("qto:{name}"), q)
    }

    /// Relationships that need all elements: containment, aggregation, materials, presentation layers.
    pub(super) fn flush(&mut self) -> R<()> {
        let contained = std::mem::take(&mut self.contained);
        for (key, elements) in contained {
            let container = if key == 0 {
                self.site
            } else {
                self.storeys[&LayerId(key)].0
            };
            self.ent(
                "IFCRELCONTAINEDINSPATIALSTRUCTURE",
                vec![
                    self.guid("rel_contained", key),
                    Value::Null,
                    Value::Null,
                    Value::Null,
                    Value::refs(elements),
                    Value::Ref(container),
                ],
            )?;
        }
        if let Some((building, _)) = self.building {
            self.ent(
                "IFCRELAGGREGATES",
                vec![
                    self.guid("rel_site", 0),
                    Value::Null,
                    Value::Null,
                    Value::Null,
                    Value::Ref(self.site),
                    Value::refs([building]),
                ],
            )?;
            let storeys: Vec<u64> = self.storeys.values().map(|s| s.0).collect();
            self.ent(
                "IFCRELAGGREGATES",
                vec![
                    self.guid("rel_building", 0),
                    Value::Null,
                    Value::Null,
                    Value::Null,
                    Value::Ref(building),
                    Value::refs(storeys),
                ],
            )?;
        }
        if !self.massing_buildings.is_empty() {
            let buildings = std::mem::take(&mut self.massing_buildings);
            self.ent(
                "IFCRELAGGREGATES",
                vec![
                    self.guid("rel_massing", 0),
                    Value::Null,
                    Value::Null,
                    Value::Null,
                    Value::Ref(self.site),
                    Value::refs(buildings),
                ],
            )?;
        }
        let sets = std::mem::take(&mut self.layer_sets);
        for (key, (target, elements)) in sets {
            let tag = format!("rel_material:{key}");
            self.ent(
                "IFCRELASSOCIATESMATERIAL",
                vec![
                    self.guid(&tag, 0),
                    Value::Null,
                    Value::Null,
                    Value::Null,
                    Value::refs(elements),
                    Value::Ref(target),
                ],
            )?;
        }
        let layers = std::mem::take(&mut self.class_layers);
        for (class, reps) in layers {
            if class == NONE_CLASS || reps.is_empty() {
                continue;
            }
            let name = self
                .doc
                .class(class)
                .map_or_else(|| format!("Class {class}"), |c: &Class| c.name.clone());
            self.ent(
                "IFCPRESENTATIONLAYERASSIGNMENT",
                vec![
                    Value::s(name.trim()),
                    Value::Null,
                    Value::refs(reps),
                    Value::s(class.0.to_string()),
                ],
            )?;
        }
        Ok(())
    }
}

/// A JSON value from a record as a typed IFC value.
fn json_nominal(v: &Json) -> Value {
    match v {
        Json::Bool(b) => Value::typed("IFCBOOLEAN", Value::Bool(*b)),
        Json::Number(n) => match n.as_i64() {
            Some(i) => Value::typed("IFCINTEGER", Value::Int(i)),
            None => Value::typed(
                "IFCREAL",
                Value::Real(n.as_f64().filter(|x| x.is_finite()).unwrap_or(0.0)),
            ),
        },
        Json::String(s) => Value::typed("IFCLABEL", Value::s(s)),
        other => Value::typed("IFCTEXT", Value::s(other.to_string())),
    }
}
