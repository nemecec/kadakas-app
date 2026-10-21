//! Reference rules for graphic resources (symbols, hatches, tiles, line types): what a resource, a
//! symbol instance or an attribute may refer to, and what happens when a resource is deleted.

use super::*;
use crate::graphics::{GraphicResource, RefKind};
use crate::resource::Resource;

fn invalid(m: impl Into<String>) -> CommandError {
    CommandError::Invalid(m.into())
}

impl Tx<'_> {
    /// Every resource that a graphic resource uses must exist and be the right kind. A symbol cannot
    /// contain itself, also not through other symbols. `id` is the resource that gets the body (none for a new one).
    pub(super) fn check_graphic_refs(
        &self,
        id: Option<ResourceId>,
        body: &ResourceBody,
    ) -> Result<(), CommandError> {
        let ResourceBody::Graphic(g) = body else {
            return Ok(());
        };
        for (r, kind) in g.refs() {
            if self.doc.resources.get(r).is_none() {
                return Err(CommandError::NoResource(r));
            }
            if !self.doc.resources.is_kind(r, kind) {
                return Err(invalid(format!("resource {r} is not a {}", kind.label())));
            }
            if id.is_some_and(|me| self.doc.resources.symbol_reaches(r, me)) {
                return Err(invalid("a symbol or line type cannot contain itself"));
            }
        }
        Ok(())
    }

    /// After the resource `id` changed or was added: it and every component definition that uses
    /// it must still expand within the limits (nesting 16, 20,000 models, no cycle). Nothing is
    /// cut off silently: the command fails and the document stays as it was.
    pub(super) fn check_component_expansion(&self, id: ResourceId) -> Result<(), CommandError> {
        crate::component::check_expansion(&self.doc.resources, id).map_err(|(bad, e)| {
            let name = self.doc.resources.get(bad).map_or("", |r| r.name.as_str());
            if bad == id {
                invalid(format!("component '{name}' is over a limit: {e}"))
            } else {
                // A definition that uses this one is over a limit (it was before, if it came
                // with a file, a restored version or another user).
                invalid(format!(
                    "this definition is used by component '{name}', which is over a limit: {e}"
                ))
            }
        })
    }

    /// The parts of a component definition must stay within the size of a definition: at most
    /// [`crate::component::MAX_DEFINITION_EDGES`] edges and
    /// [`crate::component::MAX_DEFINITION_BYTES`] bytes of saved text. The command fails and the
    /// document stays as it was.
    pub(super) fn check_definition_size(&self, body: &ResourceBody) -> Result<(), CommandError> {
        if let ResourceBody::Graphic(g) = body
            && let GraphicResource::Symbol(d) = &**g
            && let Some(why) = crate::component::definition_size_problem(&d.parts)
        {
            return Err(invalid(why));
        }
        Ok(())
    }

    /// A symbol body that changes must keep the kind that its users expect: a definition that
    /// has component instances must keep its 3D parts, and one that has symbol instances must
    /// keep a 2D side.
    pub(super) fn check_instance_kinds(
        &self,
        id: ResourceId,
        body: &ResourceBody,
    ) -> Result<(), CommandError> {
        let ResourceBody::Graphic(g) = body else {
            return Ok(());
        };
        let GraphicResource::Symbol(d) = &**g else {
            return Ok(());
        };
        let flat = d.items.is_empty() && d.solids.is_empty();
        let comp_users = self
            .doc
            .objects
            .values()
            .any(|o| matches!(&o.shape, Shape::Component { symbol, .. } if *symbol == id))
            || self.doc.resources.iter().any(|r| {
                self.doc
                    .resources
                    .symbol(r.id)
                    .is_some_and(|x| crate::component::part_refs(&x.parts).any(|s| s == id))
            });
        if comp_users && !d.is_component() {
            return Err(invalid(format!(
                "resource {id} is used as a component. Its 3D parts cannot be removed"
            )));
        }
        let sym_users = self
            .doc
            .objects
            .values()
            .any(|o| matches!(&o.shape, Shape::Symbol { symbol, .. } if *symbol == id));
        if sym_users && d.is_component() && flat {
            return Err(invalid(format!(
                "resource {id} is used as a symbol. A 3D component cannot replace its 2D side"
            )));
        }
        Ok(())
    }

    /// A symbol instance must refer to a symbol that exists.
    pub(super) fn check_symbol_shape(&self, shape: &Shape) -> Result<(), CommandError> {
        match shape {
            Shape::Symbol { symbol, .. } if self.doc.resources.get(*symbol).is_none() => {
                Err(CommandError::NoResource(*symbol))
            }
            Shape::Symbol { symbol, .. } if self.doc.resources.symbol(*symbol).is_none() => {
                Err(invalid(format!("resource {symbol} is not a symbol")))
            }
            Shape::Component { symbol, .. } if self.doc.resources.get(*symbol).is_none() => {
                Err(CommandError::NoResource(*symbol))
            }
            Shape::Component { symbol, .. }
                if !self
                    .doc
                    .resources
                    .symbol(*symbol)
                    .is_some_and(|d| d.is_component()) =>
            {
                Err(invalid(format!(
                    "resource {symbol} is not a component (it has no 3D parts)"
                )))
            }
            Shape::Symbol { symbol, .. }
                if self.doc.resources.symbol(*symbol).is_some_and(|d| {
                    d.is_component() && d.items.is_empty() && d.solids.is_empty()
                }) =>
            {
                Err(invalid(format!(
                    "resource {symbol} is a 3D component. Use a component instance, not a symbol instance"
                )))
            }
            _ => Ok(()),
        }
    }

    /// The hatch, tile and line type of attributes must exist.
    pub(super) fn check_attr_refs(&self, attrs: &Attributes) -> Result<(), CommandError> {
        for (r, kind) in attrs
            .fill_pattern
            .map(|r| (r, RefKind::Pattern))
            .into_iter()
            .chain(attrs.line_type.map(|r| (r, RefKind::LineType)))
        {
            if self.doc.resources.get(r).is_none() {
                return Err(CommandError::NoResource(r));
            }
            if !self.doc.resources.is_kind(r, kind) {
                return Err(invalid(format!("resource {r} is not a {}", kind.label())));
            }
        }
        Ok(())
    }

    /// Deletes a graphic resource. Returns false if the resource is of another kind (the caller handles it).
    /// Users keep working only if `reassign` names another resource of the same type that takes over.
    pub(super) fn graphic_delete(
        &mut self,
        id: ResourceId,
        reassign: Option<ResourceId>,
    ) -> Result<bool, CommandError> {
        let Some(current) = self.doc.resources.graphic(id) else {
            return Ok(false);
        };
        let type_name = current.type_name();
        let usage = crate::library::usage(self.doc, id);
        match reassign {
            None if !usage.is_unused() => {
                return Err(CommandError::ResourceInUse(id, usage.total()));
            }
            None => {}
            Some(to) => {
                let target = self
                    .doc
                    .resources
                    .get(to)
                    .ok_or(CommandError::NoResource(to))?;
                if to == id {
                    return Err(invalid("cannot reassign a resource to itself"));
                }
                if target.body.type_name() != type_name {
                    return Err(invalid("reassign to a resource of the same type"));
                }
                if type_name == "symbol"
                    && usage
                        .resources
                        .iter()
                        .any(|r| self.doc.resources.symbol_reaches(to, *r))
                {
                    return Err(invalid("a symbol cannot contain itself"));
                }
                let swap = |r: ResourceId| if r == id { to } else { r };
                for oid in &usage.objects {
                    self.touch(*oid);
                    let o = self.doc.object_mut(*oid).expect("exists");
                    if let Shape::Symbol { symbol, .. } | Shape::Component { symbol, .. } =
                        &mut o.shape
                    {
                        *symbol = swap(*symbol);
                    }
                    o.attrs.fill_pattern = o.attrs.fill_pattern.map(swap);
                    o.attrs.line_type = o.attrs.line_type.map(swap);
                }
                for cid in &usage.classes {
                    if let Some(c) = self.doc.classes.iter_mut().find(|c| c.id == *cid) {
                        c.attrs.fill_pattern = c.attrs.fill_pattern.map(swap);
                        c.attrs.line_type = c.attrs.line_type.map(swap);
                    }
                }
                for rid in &usage.resources {
                    if let Some(Resource {
                        body: ResourceBody::Graphic(g),
                        ..
                    }) = self.doc.resources.get_mut(*rid)
                    {
                        g.remap(&swap);
                    }
                }
            }
        }
        self.doc.resources.remove(id);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphics::{
        GraphicResource, HatchDef, HatchLine, LineElement, LineTypeDef, PatternUnits, SymbolDef,
        SymbolItem, TileDef,
    };
    use crate::resource::ResourceBody as B;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    fn add_res(
        ed: &mut Editor,
        name: &str,
        g: GraphicResource,
    ) -> Result<ResourceId, CommandError> {
        ed.execute(&Command::AddResource {
            name: name.into(),
            body: B::Graphic(Box::new(g)),
        })
        .map(|o| ResourceId(o.created[0]))
    }

    fn hatch() -> GraphicResource {
        GraphicResource::Hatch(HatchDef {
            units: PatternUnits::Paper,
            lines: vec![HatchLine::solid(45.0, 2.0)],
        })
    }

    fn symbol_with(attrs: Attributes, shape: Shape) -> GraphicResource {
        GraphicResource::Symbol(SymbolDef {
            items: vec![SymbolItem {
                shape,
                attrs,
                class: None,
            }],
            ..Default::default()
        })
    }

    fn circle() -> Shape {
        Shape::Circle {
            center: DVec2::ZERO,
            radius: 4.0,
        }
    }

    #[test]
    fn references_must_exist_and_have_the_right_kind() {
        let mut ed = Editor::default();
        let h = add_res(&mut ed, "Diag", hatch()).unwrap();
        let sym = add_res(&mut ed, "Dot", symbol_with(Attributes::default(), circle())).unwrap();
        // A symbol that uses a hatch.
        let ok = Attributes {
            fill_pattern: Some(h),
            ..Default::default()
        };
        add_res(&mut ed, "Dot2", symbol_with(ok, circle())).unwrap();
        // The wrong kind, and a missing resource.
        let wrong = Attributes {
            fill_pattern: Some(sym),
            ..Default::default()
        };
        assert!(
            matches!(add_res(&mut ed, "Bad", symbol_with(wrong, circle())), Err(CommandError::Invalid(m)) if m.contains("hatch or tile"))
        );
        let missing = Attributes {
            line_type: Some(ResourceId::from_num(999)),
            ..Default::default()
        };
        assert_eq!(
            add_res(&mut ed, "Bad2", symbol_with(missing, circle())),
            Err(CommandError::NoResource(ResourceId::from_num(999)))
        );
        let tile = |s| {
            GraphicResource::Tile(TileDef {
                symbol: s,
                width: 10.0,
                height: 10.0,
                row_shift: 0.0,
                angle: 0.0,
                scale: 1.0,
                units: PatternUnits::Paper,
            })
        };
        assert!(
            matches!(add_res(&mut ed, "T", tile(h)), Err(CommandError::Invalid(m)) if m.contains("not a symbol"))
        );
        add_res(&mut ed, "T", tile(sym)).unwrap();
        let lt = GraphicResource::LineType(LineTypeDef {
            units: PatternUnits::Paper,
            elements: vec![
                LineElement::Dash { length: 3.0 },
                LineElement::Mark {
                    symbol: sym,
                    scale: 1.0,
                    rotate: true,
                },
                LineElement::Gap { length: 2.0 },
            ],
        });
        add_res(&mut ed, "Fence", lt).unwrap();
        // Instances and attributes.
        let inst = |s| Shape::Symbol {
            symbol: s,
            at: v(0.0, 0.0),
            rotation: 0.0,
            scale: 1.0,
            scale_y: None,
        };
        let add_obj = |ed: &mut Editor, shape, attrs| {
            ed.execute(&Command::AddObject {
                shape,
                layer: None,
                class: None,
                attrs,
            })
        };
        assert!(add_obj(&mut ed, inst(sym), Attributes::default()).is_ok());
        assert_eq!(
            add_obj(
                &mut ed,
                inst(ResourceId::from_num(777)),
                Attributes::default()
            ),
            Err(CommandError::NoResource(ResourceId::from_num(777)))
        );
        assert!(
            matches!(add_obj(&mut ed, inst(h), Attributes::default()), Err(CommandError::Invalid(m)) if m.contains("not a symbol"))
        );
        assert!(
            add_obj(
                &mut ed,
                circle(),
                Attributes {
                    fill_pattern: Some(h),
                    ..Default::default()
                }
            )
            .is_ok()
        );
        assert!(matches!(
            add_obj(
                &mut ed,
                circle(),
                Attributes {
                    fill_pattern: Some(sym),
                    ..Default::default()
                }
            ),
            Err(CommandError::Invalid(_))
        ));
        assert!(matches!(
            add_obj(
                &mut ed,
                circle(),
                Attributes {
                    line_type: Some(h),
                    ..Default::default()
                }
            ),
            Err(CommandError::Invalid(_))
        ));
        assert_eq!(
            add_obj(
                &mut ed,
                circle(),
                Attributes {
                    line_type: Some(ResourceId::from_num(55)),
                    ..Default::default()
                }
            ),
            Err(CommandError::NoResource(ResourceId::from_num(55)))
        );
        assert!(
            matches!(add_obj(&mut ed, Shape::Symbol { symbol: sym, at: v(0.0, 0.0), rotation: 0.0, scale: 0.0, scale_y: None }, Attributes::default()), Err(CommandError::Invalid(m)) if m.contains("zero"))
        );
        // Class attributes.
        assert!(
            ed.execute(&Command::ModifyClass {
                id: NONE_CLASS,
                name: None,
                visible: None,
                attrs: Some(Attributes {
                    fill_pattern: Some(h),
                    ..Default::default()
                })
            })
            .is_ok()
        );
        assert!(
            ed.execute(&Command::AddClass {
                name: "C".into(),
                attrs: Attributes {
                    fill_pattern: Some(ResourceId::from_num(404)),
                    ..Default::default()
                }
            })
            .is_err()
        );
    }

    #[test]
    fn a_symbol_cannot_contain_itself() {
        let mut ed = Editor::default();
        let a = add_res(&mut ed, "A", symbol_with(Attributes::default(), circle())).unwrap();
        let inst = |s| Shape::Symbol {
            symbol: s,
            at: DVec2::ZERO,
            rotation: 0.0,
            scale: 1.0,
            scale_y: None,
        };
        let b = add_res(&mut ed, "B", symbol_with(Attributes::default(), inst(a))).unwrap();
        // A contains B, which contains A.
        let cyc = ed.execute(&Command::ModifyResource {
            id: a,
            name: None,
            body: Some(B::Graphic(Box::new(symbol_with(
                Attributes::default(),
                inst(b),
            )))),
        });
        assert!(matches!(cyc, Err(CommandError::Invalid(m)) if m.contains("contain itself")));
        let direct = ed.execute(&Command::ModifyResource {
            id: a,
            name: None,
            body: Some(B::Graphic(Box::new(symbol_with(
                Attributes::default(),
                inst(a),
            )))),
        });
        assert!(matches!(direct, Err(CommandError::Invalid(m)) if m.contains("contain itself")));
    }

    #[test]
    fn deleting_needs_no_users_or_a_reassign() {
        let mut ed = Editor::default();
        let h1 = add_res(&mut ed, "H1", hatch()).unwrap();
        let h2 = add_res(&mut ed, "H2", hatch()).unwrap();
        let sym = add_res(
            &mut ed,
            "S",
            symbol_with(
                Attributes {
                    fill_pattern: Some(h1),
                    ..Default::default()
                },
                circle(),
            ),
        )
        .unwrap();
        let obj = ObjectId(
            ed.execute(&Command::AddObject {
                shape: circle(),
                layer: None,
                class: None,
                attrs: Attributes {
                    fill_pattern: Some(h1),
                    ..Default::default()
                },
            })
            .unwrap()
            .created[0],
        );
        ed.execute(&Command::ModifyClass {
            id: NONE_CLASS,
            name: None,
            visible: None,
            attrs: Some(Attributes {
                fill_pattern: Some(h1),
                pen: Some(Color::BLACK),
                ..Default::default()
            }),
        })
        .unwrap();
        // Used by an object, a class and a symbol: 3.
        assert_eq!(
            ed.execute(&Command::DeleteResource {
                id: h1,
                reassign: None
            }),
            Err(CommandError::ResourceInUse(h1, 3))
        );
        assert!(matches!(
            ed.execute(&Command::DeleteResource {
                id: h1,
                reassign: Some(h1)
            }),
            Err(CommandError::Invalid(_))
        ));
        assert!(
            matches!(ed.execute(&Command::DeleteResource { id: h1, reassign: Some(sym) }), Err(CommandError::Invalid(m)) if m.contains("same type"))
        );
        assert_eq!(
            ed.execute(&Command::DeleteResource {
                id: h1,
                reassign: Some(ResourceId::from_num(999))
            }),
            Err(CommandError::NoResource(ResourceId::from_num(999)))
        );
        let before = ed.doc().clone();
        ed.execute(&Command::DeleteResource {
            id: h1,
            reassign: Some(h2),
        })
        .unwrap();
        assert!(ed.doc().resources.get(h1).is_none());
        assert_eq!(ed.doc().object(obj).unwrap().attrs.fill_pattern, Some(h2));
        assert_eq!(
            ed.doc().class(NONE_CLASS).unwrap().attrs.fill_pattern,
            Some(h2)
        );
        assert_eq!(
            ed.doc().resources.symbol(sym).unwrap().items[0]
                .attrs
                .fill_pattern,
            Some(h2)
        );
        assert!(ed.undo());
        assert_eq!(*ed.doc(), before);
        // Reassigning a symbol cannot make a loop.
        let inner = add_res(
            &mut ed,
            "Inner",
            symbol_with(Attributes::default(), circle()),
        )
        .unwrap();
        let outer = add_res(
            &mut ed,
            "Outer",
            symbol_with(
                Attributes::default(),
                Shape::Symbol {
                    symbol: inner,
                    at: DVec2::ZERO,
                    rotation: 0.0,
                    scale: 1.0,
                    scale_y: None,
                },
            ),
        )
        .unwrap();
        assert!(
            matches!(ed.execute(&Command::DeleteResource { id: inner, reassign: Some(outer) }), Err(CommandError::Invalid(m)) if m.contains("contain itself"))
        );
        // An unused one goes.
        let spare = add_res(&mut ed, "Spare", hatch()).unwrap();
        ed.execute(&Command::DeleteResource {
            id: spare,
            reassign: None,
        })
        .unwrap();
        // A symbol instance is a user of the symbol.
        let inst = ObjectId(
            ed.execute(&Command::AddObject {
                shape: Shape::Symbol {
                    symbol: outer,
                    at: DVec2::ZERO,
                    rotation: 0.0,
                    scale: 1.0,
                    scale_y: None,
                },
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap()
            .created[0],
        );
        assert_eq!(
            ed.execute(&Command::DeleteResource {
                id: outer,
                reassign: None
            }),
            Err(CommandError::ResourceInUse(outer, 1))
        );
        let other = add_res(
            &mut ed,
            "Other",
            symbol_with(Attributes::default(), circle()),
        )
        .unwrap();
        ed.execute(&Command::DeleteResource {
            id: outer,
            reassign: Some(other),
        })
        .unwrap();
        assert!(
            matches!(ed.doc().object(inst).unwrap().shape, Shape::Symbol { symbol, .. } if symbol == other)
        );
    }
}
