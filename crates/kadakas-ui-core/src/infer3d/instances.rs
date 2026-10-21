//! Scenes for component instances (MOD-07).
//!
//! A component definition is read by every instance, so the work that depends only on the
//! definition is done once: [`DefinitionScene`] holds the points, edges and faces of the
//! definition in its own coordinates, with the nested components already placed inside it. An
//! instance adds a placed copy of those items ([`SceneBuilder::add_placed`]): the surface models
//! of the definition are read, never copied, whatever the number of instances. The scene of many
//! instances has one index (one BVH per kind), so a frame costs as much as for a single model.
//!
//! The tags of the geometry of an instance carry a model number: the number that the caller gave
//! for the instance plus the ordinal of the surface model inside the definition (in the order of
//! `kadakas_core::component::leaves`). [`InstanceScenes::instance_of`] turns a tag back into the
//! instance and the model.

use std::collections::HashMap;
use std::sync::Arc;

use kadakas_core::component::{self, ComponentError};
use kadakas_core::model::{Document, ObjectId};
use kadakas_core::resource::{ResourceId, Resources};

use super::scene::{Scene, SceneBuilder, Tag};

/// The largest model number that a tag holds.
const MAX_MODEL: u64 = 0x3fff_ffff;

/// The geometry of a component definition in the coordinates of the definition. It is `Send` and
/// `Sync`: build it on a worker thread and share it between instances.
#[derive(Clone, Debug, Default)]
pub struct DefinitionScene {
    local: SceneBuilder,
    models: u32,
}

impl DefinitionScene {
    /// Builds the scene of a definition. Fails like `component::leaves`: a missing definition, a
    /// cycle, or nesting and size over the limits.
    pub fn of(res: &Resources, symbol: ResourceId) -> Result<DefinitionScene, ComponentError> {
        let leaves = component::leaves(res, symbol)?;
        let mut local = SceneBuilder::new();
        for (i, leaf) in leaves.iter().enumerate() {
            local.add_surface_model_placed(leaf.model, &leaf.transform, i as u32);
        }
        Ok(DefinitionScene {
            local,
            models: leaves.len() as u32,
        })
    }

    /// The number of surface models in the definition (nested components counted each time).
    pub fn model_count(&self) -> u32 {
        self.models
    }
}

/// The definition scenes of a document, kept between frames. An entry is kept with the content
/// version of the definition at the time it was built (`component::version`: the revisions of
/// the surface models, the placement of nested components, and the nested definitions). An undo,
/// a change by another user and a change of a nested component all change the version, so a
/// stale scene is never returned and nothing has to be forgotten by hand.
#[derive(Clone, Debug, Default)]
pub struct DefinitionCache {
    map: HashMap<ResourceId, (u64, Arc<DefinitionScene>)>,
}

impl DefinitionCache {
    /// The scene of a definition: built the first time, and again whenever its content version
    /// is not the one that was kept.
    pub fn get(
        &mut self,
        res: &Resources,
        symbol: ResourceId,
    ) -> Result<Arc<DefinitionScene>, ComponentError> {
        let Some(version) = component::version(res, symbol) else {
            // Gives the reason: a missing definition, a cycle, a limit.
            self.map.remove(&symbol);
            return DefinitionScene::of(res, symbol).map(Arc::new);
        };
        if let Some((v, s)) = self.map.get(&symbol)
            && *v == version
        {
            return Ok(s.clone());
        }
        let s = Arc::new(DefinitionScene::of(res, symbol)?);
        self.map.insert(symbol, (version, s.clone()));
        Ok(s)
    }

    /// Drops one definition. Not needed for correctness (the version decides), it frees memory.
    pub fn forget(&mut self, symbol: ResourceId) {
        self.map.remove(&symbol);
    }

    pub fn clear(&mut self) {
        self.map.clear();
    }

    /// The number of definitions that are kept.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// Where the models of one instance are numbered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placed {
    pub instance: ObjectId,
    /// The model number of the first model of the instance.
    pub first_model: u32,
    /// The number of models.
    pub models: u32,
}

/// The scene of some instances, and which tag belongs to which instance.
#[derive(Clone, Debug)]
pub struct InstanceScenes {
    pub scene: Scene,
    pub placed: Vec<Placed>,
}

impl InstanceScenes {
    /// The instance and the ordinal of the model in its definition for a tag of the scene.
    pub fn instance_of(&self, tag: Tag) -> Option<(ObjectId, u32)> {
        let m = tag.model();
        self.placed
            .iter()
            .find(|p| m >= p.first_model && m - p.first_model < p.models)
            .map(|p| (p.instance, m - p.first_model))
    }
}

/// Builds one scene of the given instances (objects with `Shape::Component`), with model numbers
/// from `first_model` on. The surface models of a definition are shared by all its instances.
pub fn scene_of_instances(
    doc: &Document,
    instances: &[ObjectId],
    first_model: u32,
) -> Result<InstanceScenes, ComponentError> {
    scene_of_instances_cached(doc, instances, first_model, &mut DefinitionCache::default())
}

/// Like [`scene_of_instances`], with the definition scenes kept in `cache`.
pub fn scene_of_instances_cached(
    doc: &Document,
    instances: &[ObjectId],
    first_model: u32,
    cache: &mut DefinitionCache,
) -> Result<InstanceScenes, ComponentError> {
    let mut builder = SceneBuilder::new();
    let mut placed = Vec::with_capacity(instances.len());
    let mut next = u64::from(first_model);
    // One look-up (and one version) for each definition in this call, however many instances.
    let mut local: HashMap<ResourceId, Arc<DefinitionScene>> = HashMap::new();
    for &id in instances {
        let o = doc
            .object(id)
            .ok_or_else(|| ComponentError::Invalid(format!("object {id} does not exist")))?;
        let (symbol, xf) = component::placement(&o.shape).ok_or_else(|| {
            ComponentError::Invalid(format!("object {id} is not a component instance"))
        })?;
        let def = match local.get(&symbol) {
            Some(d) => d.clone(),
            None => {
                let d = cache.get(&doc.resources, symbol)?;
                local.insert(symbol, d.clone());
                d
            }
        };
        if next + u64::from(def.models) > MAX_MODEL {
            return Err(ComponentError::Invalid(
                "there are too many surface models for the tags of the scene".into(),
            ));
        }
        builder.add_placed(&def.local, &xf, next as u32);
        placed.push(Placed {
            instance: id,
            first_model: next as u32,
            models: def.models,
        });
        next += u64::from(def.models);
    }
    Ok(InstanceScenes {
        scene: builder.finish(),
        placed,
    })
}
