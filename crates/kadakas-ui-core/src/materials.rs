//! The logic of the Materials panel of the 3D modeler (MOD-05): the list of the materials of the
//! document and the built-in ones, the swatches, a small editor, and the actions new, duplicate,
//! delete (a used material asks first) and purge.
//!
//! Pure code: the modeler draws the panel and calls [`MaterialsPanel::apply`]. A material is a
//! resource (`kadakas_core::present::Material`); nothing is stored beside the document. Every
//! action is one undo step.

use kadakas_core::command::{Command, Editor};
use kadakas_core::model::{Color, Document};
use kadakas_core::present::{
    self, MappingKind, Material, MaterialUses, Procedural, RenderResource, Texture,
};
use kadakas_core::resource::{Resource, ResourceBody, ResourceId};
use kadakas_core::surface::{Edit, PaintSide};
use kadakas_l10n::tr;

/// One line of the list.
#[derive(Clone, Debug, PartialEq)]
pub struct MaterialRow {
    pub id: ResourceId,
    pub name: String,
    pub color: [u8; 4],
    pub opacity: f32,
    pub textured: bool,
    /// Slots and face sides that use the material.
    pub uses: usize,
    /// Changes when the look of the swatch changes (the color, the opacity, the texture).
    pub thumb_key: u64,
}

/// A number that changes when the swatch of the material would look different.
pub fn thumb_key(m: &Material) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    m.base_color.0.hash(&mut h);
    m.opacity.to_bits().hash(&mut h);
    match &m.texture {
        None => 0u8.hash(&mut h),
        Some(Texture::Procedural { kind, seed }) => (1u8, kind.id(), seed).hash(&mut h),
        Some(Texture::Image { png_base64, .. }) => (
            2u8,
            png_base64.len(),
            png_base64.get(..64),
            png_base64.get(png_base64.len().saturating_sub(64)..),
        )
            .hash(&mut h),
    }
    h.finish()
}

/// The materials of the document in the order they were made, with how often each is used.
pub fn rows(doc: &Document) -> Vec<MaterialRow> {
    doc.resources
        .iter()
        .filter_map(|r| {
            let m = present::material(doc, r.id)?;
            Some(MaterialRow {
                id: r.id,
                name: r.name.clone(),
                color: m.base_color.0,
                opacity: m.opacity,
                textured: m.texture.is_some(),
                uses: present::material_uses(doc, r.id).total(),
                thumb_key: thumb_key(m),
            })
        })
        .collect()
}

/// The names of the built-in materials that the document does not have yet.
pub fn missing_defaults(doc: &Document) -> Vec<&'static str> {
    present::default_library()
        .into_iter()
        .map(|(n, _)| n)
        .filter(|n| !has_material_named(doc, n))
        .collect()
}

fn has_material_named(doc: &Document, name: &str) -> bool {
    doc.resources
        .iter()
        .any(|r| r.name.eq_ignore_ascii_case(name) && present::material(doc, r.id).is_some())
}

/// A name that no material has: `base`, else `base 2`, `base 3`...
fn free_name(doc: &Document, base: &str) -> String {
    if !has_material_named(doc, base) {
        return base.to_string();
    }
    (2..)
        .map(|n| format!("{base} {n}"))
        .find(|n| !has_material_named(doc, n))
        .expect("an endless range")
}

/// The pixels of a swatch: `size` by `size` RGBA, the color or one repeat of the texture, over a
/// light checker where the material is not opaque.
pub fn swatch(m: &Material, size: u32) -> Vec<[u8; 4]> {
    let size = size.clamp(2, 256);
    let base = [m.base_color.0[0], m.base_color.0[1], m.base_color.0[2]];
    let img = m
        .texture
        .as_ref()
        .and_then(|t| kadakas_render::texture::load(t, base));
    let alpha = m.opacity.clamp(0.0, 1.0);
    let mut out = Vec::with_capacity((size * size) as usize);
    for y in 0..size {
        for x in 0..size {
            let (u, v) = (
                (x as f32 + 0.5) / size as f32,
                1.0 - (y as f32 + 0.5) / size as f32,
            );
            let c = match &img {
                Some(i) => {
                    let s = i.sample(u, v);
                    [s[0], s[1], s[2]]
                }
                None => base.map(f32::from),
            };
            let checker = if (x * 4 / size + y * 4 / size).is_multiple_of(2) {
                235.0
            } else {
                205.0
            };
            let mix = |k: usize| (c[k] * alpha + checker * (1.0 - alpha)).clamp(0.0, 255.0) as u8;
            out.push([mix(0), mix(1), mix(2), 255]);
        }
    }
    out
}

/// A change of one field of a material, from the small editor.
#[derive(Clone, Debug, PartialEq)]
pub enum MaterialEdit {
    Name(String),
    Color([u8; 3]),
    Opacity(f32),
    Roughness(f32),
    Metallic(f32),
    /// A generated pattern, or none.
    Pattern(Option<Procedural>),
    MappingKind(MappingKind),
    /// Size of one repeat along U and V (mm).
    Scale([f64; 2]),
    /// Turn in degrees.
    Rotation(f64),
    /// Shift in mm.
    Offset([f64; 2]),
}

/// The material with one edit applied. The name is not part of the material.
pub fn edited(m: &Material, e: &MaterialEdit) -> Result<Material, String> {
    let mut m = m.clone();
    match e {
        MaterialEdit::Name(_) => {}
        MaterialEdit::Color(c) => m.base_color = Color([c[0], c[1], c[2], 255]),
        MaterialEdit::Opacity(v) => m.opacity = *v,
        MaterialEdit::Roughness(v) => m.roughness = *v,
        MaterialEdit::Metallic(v) => m.metallic = *v,
        MaterialEdit::Pattern(p) => {
            m.texture = p.map(|kind| Texture::Procedural { kind, seed: 1 });
        }
        MaterialEdit::MappingKind(k) => m.mapping.kind = *k,
        MaterialEdit::Scale(s) => m.mapping.scale = *s,
        MaterialEdit::Rotation(r) => m.mapping.rotation = *r,
        MaterialEdit::Offset(o) => m.mapping.offset = *o,
    }
    m.validate()?;
    Ok(m)
}

fn material_body(m: Material) -> ResourceBody {
    ResourceBody::Render(Box::new(RenderResource::Material(m)))
}

/// What the person asked the panel to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    /// Add the built-in materials the document lacks.
    AddDefaults,
    /// A new material (a gray default).
    New,
    Duplicate(ResourceId),
    Edit(ResourceId, MaterialEdit),
    /// Delete: an unused material goes; a used one asks first.
    Delete(ResourceId),
    ConfirmDelete,
    CancelDelete,
    /// Delete all materials that nothing uses.
    Purge,
    /// Open the small editor for a material, or close it.
    Editor(Option<ResourceId>),
    /// Put the material (or none) in the bucket.
    Select(Option<ResourceId>),
    /// Put the material on the given side of faces of the model (Entity Info).
    SetFaces {
        model: kadakas_core::model::ObjectId,
        faces: Vec<kadakas_core::surface::FaceId>,
        side: PaintSide,
        material: Option<ResourceId>,
    },
}

/// A delete that waits for the answer of the person.
#[derive(Clone, Debug, PartialEq)]
pub struct PendingDelete {
    pub id: ResourceId,
    pub name: String,
    pub uses: MaterialUses,
}

/// The state of the panel.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MaterialsPanel {
    /// The material whose fields are open in the small editor.
    pub editing: Option<ResourceId>,
    pub pending_delete: Option<PendingDelete>,
}

impl MaterialsPanel {
    /// Does a request. `bucket` is the material of the Paint Bucket. Returns the message for the
    /// status line. An error leaves the document as it was.
    pub fn apply(
        &mut self,
        editor: &mut Editor,
        bucket: &mut Option<ResourceId>,
        req: Request,
    ) -> Result<String, String> {
        if !matches!(req, Request::ConfirmDelete) {
            self.pending_delete = None;
        }
        match req {
            Request::AddDefaults => {
                let doc = editor.doc();
                let commands: Vec<Command> = present::default_library()
                    .into_iter()
                    .filter(|(n, _)| !has_material_named(doc, n))
                    .map(|(n, m)| Command::AddResource {
                        name: n.into(),
                        body: material_body(m),
                    })
                    .collect();
                if commands.is_empty() {
                    return Ok(tr!("uic-materials-defaults-there"));
                }
                let n = commands.len();
                editor
                    .execute(&Command::Batch { commands })
                    .map_err(|e| e.to_string())?;
                Ok(tr!("uic-materials-defaults-added", count = n))
            }
            Request::New => {
                let name = free_name(editor.doc(), &tr!("uic-materials-new-name"));
                let out = editor
                    .execute(&Command::AddResource {
                        name: name.clone(),
                        body: material_body(Material::default()),
                    })
                    .map_err(|e| e.to_string())?;
                let id = ResourceId(out.created[0]);
                self.editing = Some(id);
                *bucket = Some(id);
                Ok(tr!("uic-materials-created", name = name))
            }
            Request::Duplicate(id) => {
                let doc = editor.doc();
                let r = doc.resources.get(id).ok_or("the material is gone")?;
                let m = present::material(doc, id).ok_or("it is not a material")?;
                let name = free_name(doc, &tr!("uic-materials-copy-name", name = r.name.clone()));
                let out = editor
                    .execute(&Command::AddResource {
                        name: name.clone(),
                        body: material_body(m.clone()),
                    })
                    .map_err(|e| e.to_string())?;
                let new = ResourceId(out.created[0]);
                self.editing = Some(new);
                *bucket = Some(new);
                Ok(tr!("uic-materials-created", name = name))
            }
            Request::Edit(id, edit) => {
                let doc = editor.doc();
                let r = doc.resources.get(id).ok_or("the material is gone")?;
                let m = present::material(doc, id).ok_or("it is not a material")?;
                let new_name = match &edit {
                    MaterialEdit::Name(n) => {
                        let n = n.trim();
                        if n.is_empty() {
                            return Err(tr!("uic-materials-name-empty"));
                        }
                        if doc.resources.iter().any(|o| {
                            o.id != id
                                && o.name.eq_ignore_ascii_case(n)
                                && present::material(doc, o.id).is_some()
                        }) {
                            return Err(tr!("uic-materials-name-taken", name = n.to_string()));
                        }
                        Some(n.to_string())
                    }
                    _ => None,
                };
                let new = edited(m, &edit)?;
                let changed = new != *m;
                if !changed && new_name.as_deref().is_none_or(|n| n == r.name) {
                    return Ok(String::new());
                }
                editor
                    .execute(&Command::ModifyResource {
                        id,
                        name: new_name,
                        body: changed.then(|| material_body(new)),
                    })
                    .map_err(|e| e.to_string())?;
                Ok(String::new())
            }
            Request::Delete(id) => {
                let doc = editor.doc();
                let r = doc.resources.get(id).ok_or("the material is gone")?;
                let uses = present::material_uses(doc, id);
                if !uses.is_unused() {
                    // A used material asks first: the panel shows the question.
                    self.pending_delete = Some(PendingDelete {
                        id,
                        name: r.name.clone(),
                        uses,
                    });
                    return Ok(String::new());
                }
                self.delete(editor, bucket, id)
            }
            Request::ConfirmDelete => {
                let p = self.pending_delete.take().ok_or("nothing to delete")?;
                self.delete(editor, bucket, p.id)
            }
            Request::CancelDelete => Ok(String::new()),
            Request::Editor(id) => {
                self.editing = id.filter(|i| resource(editor.doc(), *i).is_some());
                Ok(String::new())
            }
            Request::Purge => {
                let doc = editor.doc();
                let unused: Vec<ResourceId> = rows(doc)
                    .into_iter()
                    .filter(|r| r.uses == 0)
                    .map(|r| r.id)
                    .collect();
                if unused.is_empty() {
                    return Ok(tr!("uic-materials-purge-none"));
                }
                let n = unused.len();
                editor
                    .execute(&Command::Batch {
                        commands: unused
                            .iter()
                            .map(|id| Command::DeleteResource {
                                id: *id,
                                reassign: None,
                            })
                            .collect(),
                    })
                    .map_err(|e| e.to_string())?;
                if bucket.is_some_and(|b| unused.contains(&b)) {
                    *bucket = None;
                }
                if self.editing.is_some_and(|b| unused.contains(&b)) {
                    self.editing = None;
                }
                Ok(tr!("uic-materials-purged", count = n))
            }
            Request::Select(id) => {
                *bucket = id;
                Ok(String::new())
            }
            Request::SetFaces {
                model,
                faces,
                side,
                material,
            } => {
                editor
                    .execute(&Command::EditSurface {
                        id: model,
                        edit: Edit::SetFaceMaterial {
                            faces,
                            side,
                            material: material.map(|m| m.to_string()),
                        },
                    })
                    .map_err(|e| e.to_string())?;
                Ok(String::new())
            }
        }
    }

    /// Deletes a material in one undo step: the faces that use it go back to the default look
    /// first (the person said yes), then the resource and its slots go.
    fn delete(
        &mut self,
        editor: &mut Editor,
        bucket: &mut Option<ResourceId>,
        id: ResourceId,
    ) -> Result<String, String> {
        let doc = editor.doc();
        let name = doc
            .resources
            .get(id)
            .map(|r| r.name.clone())
            .ok_or("the material is gone")?;
        let uses = present::material_uses(doc, id);
        let mut commands: Vec<Command> = Vec::new();
        for (model, front, back) in uses.models {
            for (faces, side) in [(front, PaintSide::Front), (back, PaintSide::Back)] {
                if !faces.is_empty() {
                    commands.push(Command::EditSurface {
                        id: model,
                        edit: Edit::SetFaceMaterial {
                            faces,
                            side,
                            material: None,
                        },
                    });
                }
            }
        }
        commands.push(Command::DeleteResource { id, reassign: None });
        editor
            .execute(&Command::Batch { commands })
            .map_err(|e| e.to_string())?;
        if *bucket == Some(id) {
            *bucket = None;
        }
        if self.editing == Some(id) {
            self.editing = None;
        }
        Ok(tr!("uic-materials-deleted", name = name))
    }
}

/// The resource entry of a material by id, for the panel.
pub fn resource(doc: &Document, id: ResourceId) -> Option<&Resource> {
    doc.resources
        .get(id)
        .filter(|_| present::material(doc, id).is_some())
}

#[cfg(test)]
mod tests;
