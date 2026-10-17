//! The agent drives the view and points (AGR-02, AGR-03). Logic only, without egui.
//!
//! An agent can move the view of the person (`view.zoom`, `view.pan`, `view.camera`) and point at
//! things (`view.highlight`). None of this is part of the document: nothing here makes a command,
//! an undo step or a revision. The person stays in control:
//!
//! - [`NavGuard`] tells when the person navigates. An agent request that comes then is dropped
//!   with a reply that says so, and a running animation stops.
//! - [`Back`] keeps the views from before the agent's view changes, so the person can step back.
//! - [`HighlightSet`] holds the pointers of the agent, with bounds on their number, the text
//!   length and the objects of one pointer. A pointer that goes over a bound is refused with a
//!   message; nothing is cut.
//!
//! Time is an argument (milliseconds on any steady clock), so everything is pure and the tests
//! need no clock.

use glam::{DVec2, DVec3, Vec3};
use kadakas_core::uid::Uid;
use kadakas_render::{Camera, Projection};
use serde_json::Value;

use crate::collab::Peer;
use crate::view::{MAX_ZOOM, MIN_ZOOM, View};

/// The most pointers at one time.
pub const MAX_HIGHLIGHTS: usize = 32;
/// The longest text of a pointer, in characters.
pub const MAX_TEXT_CHARS: usize = 80;
/// The most objects in one pointer.
pub const MAX_IDS: usize = 500;
/// The longest time of a pointer, in milliseconds (one hour).
pub const MAX_UNTIL_MS: u64 = 3_600_000;
/// The length of an animated view change.
pub const ANIM_MS: f64 = 250.0;
/// The most views that [`Back`] keeps.
pub const BACK_DEPTH: usize = 20;
/// The person navigated this recently (ms): the agent waits, or its request is dropped.
pub const BUSY_MS: f64 = 500.0;
/// The agent cursor stays this long (ms) after the last thing the agent did, and then goes.
pub const CURSOR_MS: u64 = 120_000;
/// The agent cursor is shown as quiet after this long (ms).
pub const QUIET_MS: u64 = 30_000;
/// The agent shows as a peer with a color index that no person's palette uses (the color itself is
/// `collab::AGENT_RGB`).
pub const AGENT_COLOR: usize = 0;
/// Two view changes of the agent within this time (ms) count as one for Back.
pub const MERGE_MS: f64 = 120.0;
/// The smallest box that a zoom to objects shows, in mm. A single point does not zoom to the
/// limit.
pub const MIN_FRAME_MM: f64 = 500.0;
/// Coordinates beyond this are refused.
pub const MAX_COORD: f64 = 1.0e9;
/// The field of view of the 3D camera, in degrees (see `kadakas_render::camera::FOV_Y`).
pub const FOV_DEG: f64 = 45.0;

// ---------------------------------------------------------------- requests

/// What a request aims at.
#[derive(Clone, Debug, PartialEq)]
pub enum Aim {
    /// Objects, by ID.
    Objects(Vec<Uid>),
    /// A box in drawing coordinates (mm).
    Box { lo: DVec2, hi: DVec2 },
    /// A point in drawing coordinates (mm). `z` is NaN when the request gave none: the 3D view
    /// then uses the height of the ground at the point (`ground_height`).
    At(DVec3),
}

fn finite_all(v: &[f64]) -> bool {
    v.iter().all(|x| x.is_finite() && x.abs() <= MAX_COORD)
}

fn numbers(v: &Value, name: &str, counts: &[usize]) -> Result<Vec<f64>, String> {
    let list = v
        .as_array()
        .ok_or_else(|| format!("{name} must be a list of numbers"))?;
    if !counts.contains(&list.len()) {
        let want: Vec<String> = counts.iter().map(|c| c.to_string()).collect();
        return Err(format!("{name} needs {} numbers", want.join(" or ")));
    }
    let nums: Option<Vec<f64>> = list.iter().map(Value::as_f64).collect();
    let nums = nums.ok_or_else(|| format!("{name} must hold numbers only"))?;
    if !finite_all(&nums) {
        return Err(format!(
            "{name} must hold finite numbers of at most {MAX_COORD:e}"
        ));
    }
    Ok(nums)
}

fn object_ids(v: &Value) -> Result<Vec<Uid>, String> {
    let list = v.as_array().ok_or("ids must be a list of object IDs")?;
    if list.is_empty() {
        return Err("ids is empty".into());
    }
    if list.len() > MAX_IDS {
        return Err(format!(
            "ids holds {} objects; at most {MAX_IDS} are allowed in one request",
            list.len()
        ));
    }
    let mut out = Vec::with_capacity(list.len());
    for item in list {
        let uid = item
            .as_str()
            .and_then(Uid::parse)
            .ok_or_else(|| format!("{item} is not an object ID"))?;
        if !out.contains(&uid) {
            out.push(uid);
        }
    }
    Ok(out)
}

impl Aim {
    /// Reads `ids`, `box` or `at` from the parameters. Exactly one must be there.
    pub fn parse(p: &Value) -> Result<Aim, String> {
        let get = |k: &str| p.get(k).filter(|v| !v.is_null());
        let given = ["ids", "box", "at"]
            .iter()
            .filter(|k| get(k).is_some())
            .count();
        if given != 1 {
            return Err("give exactly one of ids, box or at".into());
        }
        if let Some(v) = get("ids") {
            return Ok(Aim::Objects(object_ids(v)?));
        }
        if let Some(v) = get("box") {
            let n = numbers(v, "box", &[4])?;
            let (lo, hi) = (DVec2::new(n[0], n[1]), DVec2::new(n[2], n[3]));
            return Ok(Aim::Box {
                lo: lo.min(hi),
                hi: lo.max(hi),
            });
        }
        let n = numbers(get("at").expect("one is given"), "at", &[2, 3])?;
        Ok(Aim::At(DVec3::new(
            n[0],
            n[1],
            n.get(2).copied().unwrap_or(f64::NAN),
        )))
    }
}

/// `view.zoom`.
#[derive(Clone, Debug, PartialEq)]
pub struct ZoomReq {
    pub aim: Aim,
    /// Pixels per mm of the plan (as the zoom of a saved view).
    pub scale: Option<f64>,
    /// Space around the box, as a fraction of its size on each side.
    pub margin: f64,
}

/// The default `margin` of `view.zoom`.
pub const DEFAULT_MARGIN: f64 = 0.1;
/// The largest `margin`.
pub const MAX_MARGIN: f64 = 10.0;

impl ZoomReq {
    pub fn parse(p: &Value) -> Result<ZoomReq, String> {
        let aim = Aim::parse(p)?;
        let scale = match p.get("scale").filter(|v| !v.is_null()) {
            None => None,
            Some(v) => {
                let s = v
                    .as_f64()
                    .filter(|s| s.is_finite())
                    .ok_or("scale must be a number")?;
                if !(MIN_ZOOM..=MAX_ZOOM).contains(&s) {
                    return Err(format!(
                        "scale is pixels per mm and must be from {MIN_ZOOM} to {MAX_ZOOM}"
                    ));
                }
                Some(s)
            }
        };
        let margin = match p.get("margin").filter(|v| !v.is_null()) {
            None => DEFAULT_MARGIN,
            Some(v) => v
                .as_f64()
                .filter(|m| m.is_finite() && (0.0..=MAX_MARGIN).contains(m))
                .ok_or_else(|| format!("margin must be a number from 0 to {MAX_MARGIN}"))?,
        };
        Ok(ZoomReq { aim, scale, margin })
    }
}

/// `view.pan`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PanReq {
    /// Move the middle of the view by this much (mm, east and north).
    By(DVec3),
    /// Put the middle of the view here. `z` is kept when it is not given.
    To(DVec2, Option<f64>),
}

impl PanReq {
    pub fn parse(p: &Value) -> Result<PanReq, String> {
        let get = |k: &str| p.get(k).filter(|v| !v.is_null());
        match (get("by"), get("to")) {
            (Some(_), Some(_)) | (None, None) => Err("give exactly one of by or to".into()),
            (Some(v), None) => {
                let n = numbers(v, "by", &[2, 3])?;
                Ok(PanReq::By(DVec3::new(
                    n[0],
                    n[1],
                    n.get(2).copied().unwrap_or(0.0),
                )))
            }
            (None, Some(v)) => {
                let n = numbers(v, "to", &[2, 3])?;
                Ok(PanReq::To(DVec2::new(n[0], n[1]), n.get(2).copied()))
            }
        }
    }
}

/// `view.camera`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraReq {
    pub eye: DVec3,
    pub target: DVec3,
    pub projection: Option<Projection>,
}

impl CameraReq {
    /// The 3D camera orbits a target and has no roll, so `up` must point upward (it is checked,
    /// not used) and `fov` must be the field of view that the camera has.
    pub fn parse(p: &Value) -> Result<CameraReq, String> {
        let triple = |k: &str| -> Result<Option<DVec3>, String> {
            match p.get(k).filter(|v| !v.is_null()) {
                None => Ok(None),
                Some(v) => {
                    let n = numbers(v, k, &[3])?;
                    Ok(Some(DVec3::new(n[0], n[1], n[2])))
                }
            }
        };
        let eye = triple("eye")?.ok_or("eye is required")?;
        let target = triple("target")?.ok_or("target is required")?;
        if (eye - target).length() < 1.0 {
            return Err("eye and target are less than 1 mm apart".into());
        }
        if let Some(up) = triple("up")? {
            let forward = (target - eye).normalize();
            let side = forward.cross(DVec3::Z);
            // The camera keeps the world up: it has no roll. An `up` is fine when it leans to
            // the same side as the world up does.
            let ok = up.length() > 0.0 && (side.length() < 1e-9 || up.dot(DVec3::Z) > 0.0);
            if !ok {
                return Err(
                    "up must point upward (z above 0): the 3D camera keeps the world up and has no roll"
                        .into(),
                );
            }
        }
        if let Some(v) = p.get("fov").filter(|v| !v.is_null()) {
            let f = v.as_f64().ok_or("fov must be a number (degrees)")?;
            if (f - FOV_DEG).abs() > 0.5 {
                return Err(format!(
                    "the field of view of the 3D camera is fixed at {FOV_DEG} degrees; leave fov out or give {FOV_DEG}"
                ));
            }
        }
        let projection = match p.get("projection").filter(|v| !v.is_null()) {
            None => None,
            Some(v) => Some(
                v.as_str()
                    .and_then(Projection::from_name)
                    .ok_or("projection must be ortho or perspective")?,
            ),
        };
        Ok(CameraReq {
            eye,
            target,
            projection,
        })
    }

    /// The orbit camera that looks from the eye to the target. The camera works in scene
    /// coordinates, which are the drawing coordinates less the scene `origin`.
    pub fn camera(&self, origin: DVec3, projection: Projection) -> Camera {
        let (eye, target) = (self.eye - origin, self.target - origin);
        let d = eye - target;
        let dist = d.length();
        Camera {
            target: target.as_vec3(),
            yaw: (d.y.atan2(d.x) as f32).rem_euclid(std::f32::consts::TAU),
            pitch: ((d.z / dist).clamp(-1.0, 1.0).asin() as f32)
                .clamp(-std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2),
            distance: (dist as f32).clamp(1.0, 1.0e9),
            projection: self.projection.unwrap_or(projection),
        }
    }
}

// ---------------------------------------------------------------- highlights

/// How long a pointer stays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Until {
    /// Until the document changes next.
    NextChange,
    /// Until the person dismisses it.
    Dismissed,
    /// For this many milliseconds.
    Ms(u64),
}

impl Until {
    pub fn parse(v: Option<&Value>) -> Result<Until, String> {
        match v.filter(|v| !v.is_null()) {
            None => Ok(Until::Dismissed),
            Some(Value::String(s)) => match s.as_str() {
                "next_change" => Ok(Until::NextChange),
                "dismissed" => Ok(Until::Dismissed),
                _ => Err("until is next_change, dismissed or a time in ms".into()),
            },
            Some(v) => match v.as_u64() {
                Some(ms) if (1..=MAX_UNTIL_MS).contains(&ms) => Ok(Until::Ms(ms)),
                _ => Err(format!(
                    "until in ms must be a whole number from 1 to {MAX_UNTIL_MS}"
                )),
            },
        }
    }

    pub fn id(self) -> Value {
        match self {
            Until::NextChange => "next_change".into(),
            Until::Dismissed => "dismissed".into(),
            Until::Ms(ms) => ms.into(),
        }
    }
}

/// The mark that goes with a color, so the color is not the only sign.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    Diamond,
    Triangle,
    Square,
    Circle,
    Plus,
}

/// How the line of a pointer is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Line {
    Solid,
    Dashed,
    Dotted,
}

/// The color of a pointer. The default is the color of the agent in the People palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HColor {
    Agent,
    Red,
    Blue,
    Green,
    Orange,
}

impl HColor {
    pub const ALL: [HColor; 5] = [
        HColor::Agent,
        HColor::Red,
        HColor::Blue,
        HColor::Green,
        HColor::Orange,
    ];

    pub fn id(self) -> &'static str {
        match self {
            HColor::Agent => "agent",
            HColor::Red => "red",
            HColor::Blue => "blue",
            HColor::Green => "green",
            HColor::Orange => "orange",
        }
    }

    pub fn from_id(s: &str) -> Option<HColor> {
        HColor::ALL.into_iter().find(|c| c.id() == s)
    }

    pub fn parse(v: Option<&Value>) -> Result<HColor, String> {
        match v.filter(|v| !v.is_null()) {
            None => Ok(HColor::Agent),
            Some(v) => v.as_str().and_then(HColor::from_id).ok_or_else(|| {
                let names: Vec<&str> = HColor::ALL.iter().map(|c| c.id()).collect();
                format!("color is one of {}", names.join(", "))
            }),
        }
    }

    /// The colors are those of the presence palette (PLAT-08). The agent has its own, which no
    /// person gets.
    pub fn rgb(self) -> [u8; 3] {
        let i = match self {
            HColor::Agent => return crate::collab::AGENT_RGB,
            HColor::Red => 0,
            HColor::Blue => 1,
            HColor::Green => 2,
            HColor::Orange => 3,
        };
        crate::collab::PALETTE[i]
    }

    pub fn mark(self) -> Mark {
        match self {
            HColor::Agent => Mark::Diamond,
            HColor::Red => Mark::Triangle,
            HColor::Blue => Mark::Square,
            HColor::Green => Mark::Circle,
            HColor::Orange => Mark::Plus,
        }
    }

    pub fn line(self) -> Line {
        match self {
            HColor::Agent | HColor::Blue => Line::Solid,
            HColor::Red | HColor::Orange => Line::Dashed,
            HColor::Green => Line::Dotted,
        }
    }
}

/// A pointer of the agent: objects, a box or a place, with a short text.
#[derive(Clone, Debug, PartialEq)]
pub struct Highlight {
    pub id: u32,
    pub aim: Aim,
    pub text: String,
    pub color: HColor,
    pub until: Until,
    /// When it was made (ms).
    pub made_ms: f64,
    /// The revision of the document then, for `Until::NextChange`.
    pub made_rev: u64,
    /// The height (mm) of the ground at the pointer when it was made: a box and a place without a
    /// height are drawn there in 3D.
    pub ground: f64,
}

/// `view.highlight`.
#[derive(Clone, Debug, PartialEq)]
pub struct HighlightReq {
    pub aim: Aim,
    pub text: String,
    pub color: HColor,
    pub until: Until,
    /// The height of the ground at the pointer. NaN: not known yet (the app fills it in when 3D is
    /// shown).
    pub ground: f64,
}

/// A text of a pointer: plain, one line, within the bound.
pub fn check_text(text: &str) -> Result<(), String> {
    if text.chars().count() > MAX_TEXT_CHARS {
        return Err(format!(
            "text is longer than {MAX_TEXT_CHARS} characters; shorten it (nothing is cut)"
        ));
    }
    if text.chars().any(|c| c.is_control() || is_format_char(c)) {
        return Err(
            "text must be one line without control or invisible formatting characters (such as text direction marks)"
                .into(),
        );
    }
    Ok(())
}

/// Invisible characters that change how text reads: zero-width characters, direction marks and
/// overrides, isolates, the word joiner and the byte order mark (Unicode category Cf, the part
/// that matters for a label).
fn is_format_char(c: char) -> bool {
    matches!(c,
        '\u{00AD}' | '\u{061C}' | '\u{180E}'
        | '\u{200B}'..='\u{200F}'
        | '\u{202A}'..='\u{202E}'
        | '\u{2060}'..='\u{206F}'
        | '\u{FEFF}'
        | '\u{FFF9}'..='\u{FFFB}')
}

impl HighlightReq {
    pub fn parse(p: &Value) -> Result<HighlightReq, String> {
        let aim = Aim::parse(p)?;
        let text = match p.get("text").filter(|v| !v.is_null()) {
            None => String::new(),
            Some(v) => v
                .as_str()
                .ok_or("text must be a string")?
                .trim()
                .to_string(),
        };
        check_text(&text)?;
        Ok(HighlightReq {
            aim,
            text,
            color: HColor::parse(p.get("color"))?,
            until: Until::parse(p.get("until"))?,
            ground: f64::NAN,
        })
    }
}

/// Why a pointer is refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HighlightError {
    /// The set is full.
    Full,
}

impl std::fmt::Display for HighlightError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HighlightError::Full => write!(
                f,
                "there are already {MAX_HIGHLIGHTS} pointers; clear some with view.highlight_clear first (the person can also dismiss them)"
            ),
        }
    }
}

/// The pointers of the agent. They are never saved.
#[derive(Clone, Debug, Default)]
pub struct HighlightSet {
    items: Vec<Highlight>,
    next_id: u32,
}

impl HighlightSet {
    pub fn items(&self) -> &[Highlight] {
        &self.items
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn get(&self, id: u32) -> Option<&Highlight> {
        self.items.iter().find(|h| h.id == id)
    }

    /// Adds a pointer and gives its ID.
    pub fn add(&mut self, req: HighlightReq, now_ms: f64, rev: u64) -> Result<u32, HighlightError> {
        if self.items.len() >= MAX_HIGHLIGHTS {
            return Err(HighlightError::Full);
        }
        self.next_id += 1;
        let id = self.next_id;
        self.items.push(Highlight {
            id,
            aim: req.aim,
            text: req.text,
            color: req.color,
            until: req.until,
            made_ms: now_ms,
            made_rev: rev,
            ground: req.ground,
        });
        Ok(id)
    }

    /// Removes one pointer. True if it was there.
    pub fn remove(&mut self, id: u32) -> bool {
        let n = self.items.len();
        self.items.retain(|h| h.id != id);
        self.items.len() != n
    }

    /// Removes all pointers. Gives how many there were.
    pub fn clear(&mut self) -> usize {
        std::mem::take(&mut self.items).len()
    }

    /// Removes the pointers that are over: a pointer with a time, a pointer until the next
    /// change after the document changed, and the objects that are gone (a pointer with no
    /// object left goes too). Gives true if something changed.
    pub fn tick(&mut self, now_ms: f64, rev: u64, exists: &dyn Fn(Uid) -> bool) -> bool {
        let before = self.items.clone();
        self.items.retain(|h| match h.until {
            Until::Ms(ms) => now_ms - h.made_ms < ms as f64,
            Until::NextChange => h.made_rev == rev,
            Until::Dismissed => true,
        });
        for h in &mut self.items {
            if let Aim::Objects(ids) = &mut h.aim
                && rev != h.made_rev
            {
                ids.retain(|u| exists(*u));
            }
        }
        self.items
            .retain(|h| !matches!(&h.aim, Aim::Objects(ids) if ids.is_empty()));
        self.items != before
    }

    /// The pointer after `current` in the list (the first one when `current` is `None` or last).
    pub fn next_after(&self, current: Option<u32>) -> Option<&Highlight> {
        let pos = current.and_then(|id| self.items.iter().position(|h| h.id == id));
        match pos {
            Some(i) => self.items.get(i + 1).or_else(|| self.items.first()),
            None => self.items.first(),
        }
    }

    /// True when a pointer waits for the height of the ground (it was made in the plan).
    pub fn needs_ground(&self) -> bool {
        self.items.iter().any(|h| match &h.aim {
            Aim::At(p) => p.z.is_nan(),
            Aim::Box { .. } => h.ground.is_nan(),
            Aim::Objects(_) => false,
        })
    }

    /// Gives the pointers that wait for it the height of the ground (`ground_at` takes a point of
    /// the drawing). The plan does not need the height, so it is asked only when 3D is shown.
    pub fn resolve_ground(&mut self, ground_at: &mut dyn FnMut(DVec2) -> f64) {
        for h in &mut self.items {
            match &mut h.aim {
                Aim::At(p) if p.z.is_nan() => p.z = ground_at(DVec2::new(p.x, p.y)),
                Aim::Box { lo, hi } if h.ground.is_nan() => h.ground = ground_at((*lo + *hi) / 2.0),
                _ => {}
            }
        }
    }

    /// The IDs of all objects that are pointed at.
    pub fn object_ids(&self) -> Vec<Uid> {
        let mut out = Vec::new();
        for h in &self.items {
            if let Aim::Objects(ids) = &h.aim {
                for u in ids {
                    if !out.contains(u) {
                        out.push(*u);
                    }
                }
            }
        }
        out
    }
}

// ---------------------------------------------------------------- the agent as a collaborator

/// The agent shown as a collaborator: a cursor at the last place it pointed at or zoomed to.
#[derive(Clone, Debug, Default)]
pub struct AgentPresence {
    at: Option<[f64; 2]>,
    last_ms: Option<f64>,
}

impl AgentPresence {
    /// The agent did something at this place (or nowhere in particular).
    pub fn touch(&mut self, at: Option<DVec2>, now_ms: f64) {
        if let Some(p) = at {
            self.at = Some([p.x, p.y]);
        }
        self.last_ms = Some(now_ms);
    }

    /// The agent as a [`Peer`], or `None` before it did anything and long after it stopped.
    pub fn peer(&self, now_ms: f64, name: String) -> Option<Peer> {
        let last = self.last_ms?;
        if now_ms - last > CURSOR_MS as f64 {
            return None;
        }
        Some(Peer {
            client: crate::collab::AGENT_CLIENT.into(),
            user: crate::collab::AGENT_CLIENT.into(),
            name,
            cursor: self.at,
            selection: Vec::new(),
            viewport: None,
            color: AGENT_COLOR,
            changed_ms: last as u64,
        })
    }

    pub fn last_ms(&self) -> Option<f64> {
        self.last_ms
    }

    pub fn forget(&mut self) {
        *self = AgentPresence::default();
    }
}

// ---------------------------------------------------------------- the person navigates

/// When the person navigates. The agent never wins against the person.
#[derive(Clone, Copy, Debug, Default)]
pub struct NavGuard {
    last_ms: Option<f64>,
    dragging: bool,
}

impl NavGuard {
    /// The person moved the view (a drag, the wheel, a key) at this time.
    pub fn note(&mut self, now_ms: f64) {
        self.last_ms = Some(now_ms);
    }

    /// The person holds a drag on the view now.
    pub fn set_dragging(&mut self, on: bool, now_ms: f64) {
        self.dragging = on;
        if on {
            self.last_ms = Some(now_ms);
        }
    }

    pub fn busy(&self, now_ms: f64) -> bool {
        self.dragging || self.last_ms.is_some_and(|t| now_ms - t < BUSY_MS)
    }
}

// ---------------------------------------------------------------- back

/// The views from before the changes of the agent, newest last. One entry is protected: the
/// newest view that the person made themselves. When the stack is full, the oldest entry that is
/// not protected goes, so Back can always return to the person's own view.
#[derive(Clone, Debug)]
pub struct Back<T> {
    stack: Vec<(T, bool)>,
}

impl<T> Default for Back<T> {
    fn default() -> Self {
        Back { stack: Vec::new() }
    }
}

impl<T> Back<T> {
    fn make_room(&mut self) {
        if self.stack.len() >= BACK_DEPTH {
            let at = self.stack.iter().position(|(_, p)| !p).unwrap_or(0);
            self.stack.remove(at);
        }
    }

    /// Keeps the view before a change of the agent.
    pub fn push(&mut self, view: T) {
        self.make_room();
        self.stack.push((view, false));
    }

    /// Keeps the view before a change of the agent, when that view is one that the person made.
    /// It replaces the protected entry of before.
    pub fn push_protected(&mut self, view: T) {
        for (_, p) in &mut self.stack {
            *p = false;
        }
        self.make_room();
        self.stack.push((view, true));
    }

    /// The view to go back to; it leaves the stack.
    pub fn pop(&mut self) -> Option<T> {
        self.stack.pop().map(|(v, _)| v)
    }

    pub fn len(&self) -> usize {
        self.stack.len()
    }

    pub fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }

    pub fn clear(&mut self) {
        self.stack.clear();
    }
}

// ---------------------------------------------------------------- the plan

/// The middle of the plan view and its zoom (px per mm).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlanPose {
    pub center: DVec2,
    pub zoom: f64,
}

impl PlanPose {
    /// The pose of a view that shows a canvas at `min` (the corner in window coordinates, px) of
    /// this size. `View::origin` is in window coordinates, so the canvas offset counts.
    pub fn of(view: View, min: DVec2, size: DVec2) -> PlanPose {
        PlanPose {
            center: view.model_of(min + size / 2.0),
            zoom: view.zoom,
        }
    }

    /// The view with this pose on a canvas at `min` of this size.
    pub fn view(self, min: DVec2, size: DVec2) -> View {
        let zoom = self.zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        let c = min + size / 2.0;
        View {
            origin: DVec2::new(c.x - self.center.x * zoom, c.y + self.center.y * zoom),
            zoom,
        }
    }
}

/// The pose that shows a box with a margin on a canvas of this size, and whether the zoom hit the
/// limit. A box smaller than [`MIN_FRAME_MM`] counts as that size.
pub fn frame_plan(
    lo: DVec2,
    hi: DVec2,
    margin: f64,
    scale: Option<f64>,
    size: DVec2,
) -> (PlanPose, bool) {
    let center = (lo + hi) / 2.0;
    if let Some(s) = scale {
        return (PlanPose { center, zoom: s }, false);
    }
    let extent = (hi - lo).max(DVec2::splat(MIN_FRAME_MM)) * (1.0 + 2.0 * margin);
    let want = (size.x.max(1.0) / extent.x).min(size.y.max(1.0) / extent.y);
    let zoom = want.clamp(MIN_ZOOM, MAX_ZOOM);
    (PlanPose { center, zoom }, zoom != want)
}

/// The height of the highest surface of the scene above the scene point `p` (x and y, scene
/// coordinates): the ground or what stands on it. `None` when no triangle covers the point or the
/// scene is too big to look through (more than [`GROUND_MAX_TRIANGLES`] triangles).
pub fn ground_height(scene: &kadakas_core::mesh::Scene, p: glam::Vec2) -> Option<f32> {
    let mesh = &scene.mesh;
    if mesh.triangle_count() > GROUND_MAX_TRIANGLES {
        return None;
    }
    let mut best: Option<f32> = None;
    for t in 0..mesh.triangle_count() {
        let [a, b, c] = mesh.triangle(t);
        let d = (b.y - c.y) * (a.x - c.x) + (c.x - b.x) * (a.y - c.y);
        if d.abs() < 1e-9 {
            continue;
        }
        let u = ((b.y - c.y) * (p.x - c.x) + (c.x - b.x) * (p.y - c.y)) / d;
        let v = ((c.y - a.y) * (p.x - c.x) + (a.x - c.x) * (p.y - c.y)) / d;
        let w = 1.0 - u - v;
        let eps = -1e-4;
        if u >= eps && v >= eps && w >= eps {
            let z = u * a.z + v * b.z + w * c.z;
            if z.is_finite() {
                best = Some(best.map_or(z, |m| m.max(z)));
            }
        }
    }
    best
}

/// The most triangles that [`ground_height`] looks through.
pub const GROUND_MAX_TRIANGLES: usize = 2_000_000;

/// The easing of an animation: slow start, fast middle, slow end.
pub fn ease(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// An animated change of the plan view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlanAnim {
    pub from: PlanPose,
    pub to: PlanPose,
    pub start_ms: f64,
    pub duration_ms: f64,
}

impl PlanAnim {
    /// The pose at a time, and whether the animation is over. The zoom changes in steps of equal
    /// ratio, so the speed looks the same on the way in and out.
    pub fn at(&self, now_ms: f64) -> (PlanPose, bool) {
        let t = if self.duration_ms <= 0.0 {
            1.0
        } else {
            (now_ms - self.start_ms) / self.duration_ms
        };
        if t >= 1.0 {
            return (self.to, true);
        }
        let e = ease(t);
        let zoom = (self.from.zoom.ln() * (1.0 - e) + self.to.zoom.ln() * e).exp();
        (
            PlanPose {
                center: self.from.center.lerp(self.to.center, e),
                zoom,
            },
            false,
        )
    }
}

/// An animated change of the 3D camera.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CamAnim {
    pub from: Camera,
    pub to: Camera,
    pub start_ms: f64,
    pub duration_ms: f64,
}

impl CamAnim {
    pub fn at(&self, now_ms: f64) -> (Camera, bool) {
        let t = if self.duration_ms <= 0.0 {
            1.0
        } else {
            (now_ms - self.start_ms) / self.duration_ms
        };
        if t >= 1.0 {
            return (self.to, true);
        }
        let e = ease(t) as f32;
        let tau = std::f32::consts::TAU;
        // The short way round for the angle.
        let mut dyaw = (self.to.yaw - self.from.yaw).rem_euclid(tau);
        if dyaw > tau / 2.0 {
            dyaw -= tau;
        }
        let cam = Camera {
            target: self.from.target.lerp(self.to.target, e),
            yaw: (self.from.yaw + dyaw * e).rem_euclid(tau),
            pitch: self.from.pitch + (self.to.pitch - self.from.pitch) * e,
            distance: (self.from.distance.ln() * (1.0 - e) + self.to.distance.ln() * e).exp(),
            projection: self.to.projection,
        };
        (cam, false)
    }
}

/// The camera that shows a 3D box with the same direction as `cam`. A `scale` (px per mm of the
/// canvas height) sets the zoom instead of the box.
pub fn frame_camera(
    cam: Camera,
    lo: Vec3,
    hi: Vec3,
    margin: f64,
    scale: Option<f64>,
    size: DVec2,
) -> Camera {
    let mut out = cam;
    let aspect = (size.x / size.y.max(1.0)) as f32;
    if let Some(s) = scale {
        out.target = (lo + hi) / 2.0;
        let half = size.y / (2.0 * s);
        out.distance = (half / (f64::from(kadakas_render::camera::FOV_Y) / 2.0).tan())
            .clamp(1.0, 1.0e9) as f32;
        return out;
    }
    // A small box shows at least MIN_FRAME_MM; the margin widens the box on each side.
    let center = (lo + hi) / 2.0;
    let half = ((hi - lo) / 2.0).max(Vec3::splat(MIN_FRAME_MM as f32 / 2.0))
        * (1.0 + 2.0 * margin as f32)
        / 1.08;
    out.fit(Some((center - half, center + half)), aspect);
    out
}

/// The ground footprint of what the camera sees, as a box on the plane `z`, or `None` when a
/// corner of the view does not reach the plane.
pub fn ground_box(cam: &Camera, aspect: f32, z: f32) -> Option<(DVec2, DVec2)> {
    let mut lo = DVec2::splat(f64::INFINITY);
    let mut hi = DVec2::splat(f64::NEG_INFINITY);
    for (x, y) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
        let (o, d) = cam.ray(glam::Vec2::new(x, y), aspect);
        if d.z.abs() < 1e-6 {
            return None;
        }
        let t = (z - o.z) / d.z;
        if t < 0.0 {
            return None;
        }
        let p = o + d * t;
        lo = lo.min(DVec2::new(p.x as f64, p.y as f64));
        hi = hi.max(DVec2::new(p.x as f64, p.y as f64));
    }
    (lo.is_finite() && hi.is_finite()).then_some((lo, hi))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn uid(n: u64) -> String {
        Uid::from_num(n).to_string()
    }

    #[test]
    fn an_aim_needs_exactly_one_of_ids_box_and_at() {
        assert!(Aim::parse(&json!({})).is_err());
        assert!(Aim::parse(&json!({"ids": [uid(1)], "at": [0, 0]})).is_err());
        assert_eq!(
            Aim::parse(&json!({"ids": [uid(1), uid(1), uid(2)]})).unwrap(),
            Aim::Objects(vec![Uid::from_num(1), Uid::from_num(2)])
        );
        assert_eq!(
            Aim::parse(&json!({"box": [10, 20, 0, 5]})).unwrap(),
            Aim::Box {
                lo: DVec2::new(0.0, 5.0),
                hi: DVec2::new(10.0, 20.0)
            }
        );
        // No height: the 3D view asks the ground.
        match Aim::parse(&json!({"at": [1, 2]})).unwrap() {
            Aim::At(p) => assert!(p.x == 1.0 && p.y == 2.0 && p.z.is_nan()),
            other => panic!("{other:?}"),
        }
        assert_eq!(
            Aim::parse(&json!({"at": [1, 2, 3]})).unwrap(),
            Aim::At(DVec3::new(1.0, 2.0, 3.0))
        );
    }

    #[test]
    fn bad_numbers_and_ids_are_refused() {
        assert!(Aim::parse(&json!({"ids": []})).is_err());
        assert!(Aim::parse(&json!({"ids": ["nope"]})).is_err());
        assert!(Aim::parse(&json!({"ids": "x"})).is_err());
        assert!(Aim::parse(&json!({"at": [1]})).is_err());
        assert!(Aim::parse(&json!({"at": [1, "a"]})).is_err());
        assert!(Aim::parse(&json!({"at": [1e12, 0]})).is_err());
        assert!(Aim::parse(&json!({"box": [0, 0, 1]})).is_err());
        let many: Vec<String> = (0..=MAX_IDS as u64).map(uid).collect();
        let e = Aim::parse(&json!({ "ids": many })).unwrap_err();
        assert!(e.contains("500"), "{e}");
    }

    #[test]
    fn zoom_scale_and_margin_have_bounds() {
        let r = ZoomReq::parse(&json!({"at": [0, 0], "scale": 0.5, "margin": 0.2})).unwrap();
        assert_eq!((r.scale, r.margin), (Some(0.5), 0.2));
        assert_eq!(
            ZoomReq::parse(&json!({"at": [0, 0]})).unwrap().margin,
            DEFAULT_MARGIN
        );
        assert!(ZoomReq::parse(&json!({"at": [0, 0], "scale": 0})).is_err());
        assert!(ZoomReq::parse(&json!({"at": [0, 0], "scale": 1000})).is_err());
        assert!(ZoomReq::parse(&json!({"at": [0, 0], "scale": "x"})).is_err());
        assert!(ZoomReq::parse(&json!({"at": [0, 0], "margin": -1})).is_err());
        assert!(ZoomReq::parse(&json!({"at": [0, 0], "margin": 11})).is_err());
    }

    #[test]
    fn pan_takes_by_or_to() {
        assert_eq!(
            PanReq::parse(&json!({"by": [10, -5]})).unwrap(),
            PanReq::By(DVec3::new(10.0, -5.0, 0.0))
        );
        assert_eq!(
            PanReq::parse(&json!({"to": [1, 2, 3]})).unwrap(),
            PanReq::To(DVec2::new(1.0, 2.0), Some(3.0))
        );
        assert!(PanReq::parse(&json!({})).is_err());
        assert!(PanReq::parse(&json!({"by": [1, 2], "to": [1, 2]})).is_err());
        assert!(PanReq::parse(&json!({"by": [1]})).is_err());
    }

    #[test]
    fn the_camera_request_is_checked_and_turned_into_an_orbit_camera() {
        let r = CameraReq::parse(&json!({
            "eye": [10000, 0, 0], "target": [0, 0, 0], "up": [0, 0, 1], "fov": 45, "projection": "perspective"
        }))
        .unwrap();
        let c = r.camera(DVec3::ZERO, Projection::Orthographic);
        assert_eq!(c.projection, Projection::Perspective);
        assert!((c.distance - 10000.0).abs() < 1.0);
        assert!(c.yaw.abs() < 1e-5 && c.pitch.abs() < 1e-5);
        assert!((c.eye() - Vec3::new(10000.0, 0.0, 0.0)).length() < 1.0);
        // The scene origin goes off the points, in f64: a far drawing keeps its precision.
        let far = DVec3::new(6.5e9, 5.9e9, 0.0);
        let shifted = CameraReq {
            eye: far + DVec3::new(10_000.0, 0.0, 0.0),
            target: far,
            projection: None,
        };
        let c = shifted.camera(far, Projection::Orthographic);
        assert_eq!(c.target, Vec3::ZERO);
        assert!((c.eye() - Vec3::new(10_000.0, 0.0, 0.0)).length() < 1.0);
        // Straight above.
        let top = CameraReq::parse(&json!({"eye": [0, 0, 500], "target": [0, 0, 0]})).unwrap();
        assert!(
            (top.camera(DVec3::ZERO, Projection::Orthographic).pitch - std::f32::consts::FRAC_PI_2)
                .abs()
                < 1e-5
        );
        assert!(CameraReq::parse(&json!({"target": [0, 0, 0]})).is_err());
        assert!(CameraReq::parse(&json!({"eye": [0, 0, 0], "target": [0, 0, 0]})).is_err());
        assert!(
            CameraReq::parse(&json!({"eye": [1000, 0, 0], "target": [0, 0, 0], "up": [0, 0, -1]}))
                .is_err()
        );
        let e = CameraReq::parse(&json!({"eye": [1000, 0, 0], "target": [0, 0, 0], "fov": 90}))
            .unwrap_err();
        assert!(e.contains("45"), "{e}");
        assert!(
            CameraReq::parse(
                &json!({"eye": [1000, 0, 0], "target": [0, 0, 0], "projection": "fisheye"})
            )
            .is_err()
        );
    }

    #[test]
    fn until_color_and_text_are_checked() {
        assert_eq!(Until::parse(None).unwrap(), Until::Dismissed);
        assert_eq!(
            Until::parse(Some(&json!("next_change"))).unwrap(),
            Until::NextChange
        );
        assert_eq!(Until::parse(Some(&json!(1500))).unwrap(), Until::Ms(1500));
        assert!(Until::parse(Some(&json!(0))).is_err());
        assert!(Until::parse(Some(&json!(MAX_UNTIL_MS + 1))).is_err());
        assert!(Until::parse(Some(&json!("later"))).is_err());
        assert!(Until::parse(Some(&json!(-3))).is_err());
        assert_eq!(Until::Ms(5).id(), json!(5));
        assert_eq!(Until::NextChange.id(), json!("next_change"));
        assert_eq!(Until::Dismissed.id(), json!("dismissed"));
        assert_eq!(HColor::parse(None).unwrap(), HColor::Agent);
        assert_eq!(HColor::parse(Some(&json!("red"))).unwrap(), HColor::Red);
        assert!(HColor::parse(Some(&json!("pink"))).is_err());
        assert!(check_text(&"x".repeat(MAX_TEXT_CHARS)).is_ok());
        assert!(check_text(&"x".repeat(MAX_TEXT_CHARS + 1)).is_err());
        assert!(check_text("two\nlines").is_err());
        // Direction marks and zero-width characters change how a label reads.
        for bad in [
            "a\u{202E}b",
            "a\u{200B}b",
            "a\u{2066}b",
            "\u{FEFF}a",
            "a\u{061C}",
        ] {
            assert!(check_text(bad).is_err(), "{bad:?}");
        }
        assert!(check_text("Käsi \u{2014} ok").is_ok());
        // The bound counts characters, not bytes.
        assert!(check_text(&"ä".repeat(MAX_TEXT_CHARS)).is_ok());
    }

    #[test]
    fn every_color_has_its_own_mark_and_a_presence_color() {
        let marks: Vec<Mark> = HColor::ALL.iter().map(|c| c.mark()).collect();
        for (i, m) in marks.iter().enumerate() {
            assert!(!marks[..i].contains(m), "marks differ");
        }
        // The agent has a color that no person gets.
        assert!(!crate::collab::PALETTE.contains(&HColor::Agent.rgb()));
        for c in HColor::ALL {
            assert_eq!(HColor::from_id(c.id()), Some(c));
        }
        assert_eq!(HColor::Green.line(), Line::Dotted);
    }

    fn req(aim: Aim, until: Until) -> HighlightReq {
        HighlightReq {
            aim,
            text: "here".into(),
            color: HColor::Agent,
            until,
            ground: 0.0,
        }
    }

    #[test]
    fn the_set_has_a_bound_and_refuses_over_it() {
        let mut set = HighlightSet::default();
        for i in 0..MAX_HIGHLIGHTS {
            set.add(
                req(Aim::At(DVec3::new(i as f64, 0.0, 0.0)), Until::Dismissed),
                0.0,
                0,
            )
            .unwrap();
        }
        let e = set
            .add(req(Aim::At(DVec3::ZERO), Until::Dismissed), 0.0, 0)
            .unwrap_err();
        assert!(e.to_string().contains("32"));
        assert_eq!(set.len(), MAX_HIGHLIGHTS);
        assert_eq!(set.clear(), MAX_HIGHLIGHTS);
        assert!(set.is_empty());
        // IDs are not reused.
        let id = set
            .add(req(Aim::At(DVec3::ZERO), Until::Dismissed), 0.0, 0)
            .unwrap();
        assert_eq!(id, MAX_HIGHLIGHTS as u32 + 1);
    }

    #[test]
    fn a_pointer_ends_by_its_until() {
        let mut set = HighlightSet::default();
        let all = |_: Uid| true;
        let a = set
            .add(req(Aim::At(DVec3::ZERO), Until::Ms(1000)), 100.0, 7)
            .unwrap();
        let b = set
            .add(req(Aim::At(DVec3::ZERO), Until::NextChange), 100.0, 7)
            .unwrap();
        let c = set
            .add(req(Aim::At(DVec3::ZERO), Until::Dismissed), 100.0, 7)
            .unwrap();
        assert!(!set.tick(1099.0, 7, &all));
        assert_eq!(set.len(), 3);
        assert!(set.tick(1100.0, 7, &all));
        assert!(set.get(a).is_none() && set.get(b).is_some());
        // The document changed.
        assert!(set.tick(1200.0, 8, &all));
        assert!(set.get(b).is_none() && set.get(c).is_some());
        // Dismissed ends only by the person.
        assert!(!set.tick(1e12, 99, &all));
        assert!(set.remove(c));
        assert!(!set.remove(c));
    }

    #[test]
    fn deleted_objects_leave_their_pointers() {
        let mut set = HighlightSet::default();
        let (u1, u2) = (Uid::from_num(1), Uid::from_num(2));
        let a = set
            .add(req(Aim::Objects(vec![u1, u2]), Until::Dismissed), 0.0, 1)
            .unwrap();
        let b = set
            .add(req(Aim::Objects(vec![u2]), Until::Dismissed), 0.0, 1)
            .unwrap();
        // The document did not change: nothing is looked up.
        assert!(!set.tick(1.0, 1, &|_| false));
        assert!(set.tick(1.0, 2, &|u| u == u1));
        assert_eq!(set.get(a).unwrap().aim, Aim::Objects(vec![u1]));
        assert!(set.get(b).is_none(), "a pointer with no object left goes");
        assert_eq!(set.object_ids(), vec![u1]);
    }

    #[test]
    fn next_after_cycles_through_the_pointers() {
        let mut set = HighlightSet::default();
        assert!(set.next_after(None).is_none());
        let a = set
            .add(req(Aim::At(DVec3::ZERO), Until::Dismissed), 0.0, 0)
            .unwrap();
        let b = set
            .add(req(Aim::At(DVec3::X), Until::Dismissed), 0.0, 0)
            .unwrap();
        assert_eq!(set.next_after(None).unwrap().id, a);
        assert_eq!(set.next_after(Some(a)).unwrap().id, b);
        assert_eq!(set.next_after(Some(b)).unwrap().id, a);
        assert_eq!(set.next_after(Some(999)).unwrap().id, a);
    }

    #[test]
    fn the_agent_cursor_shows_for_a_while() {
        let mut p = AgentPresence::default();
        assert!(p.peer(0.0, "Agent".into()).is_none());
        p.touch(Some(DVec2::new(5.0, 6.0)), 1000.0);
        let peer = p.peer(2000.0, "Agent".into()).unwrap();
        assert_eq!(peer.cursor, Some([5.0, 6.0]));
        assert_eq!(peer.rgb(), crate::collab::AGENT_RGB);
        assert!(!crate::collab::PALETTE.contains(&peer.rgb()));
        let _ = AGENT_COLOR;
        assert_eq!(peer.label(), "Agent");
        // A touch with no place keeps the old place.
        p.touch(None, 3000.0);
        assert_eq!(p.peer(3000.0, "A".into()).unwrap().cursor, Some([5.0, 6.0]));
        assert!(
            p.peer(3000.0 + CURSOR_MS as f64 + 1.0, "A".into())
                .is_none()
        );
        p.forget();
        assert!(p.last_ms().is_none());
    }

    #[test]
    fn the_person_is_busy_while_dragging_and_a_moment_after() {
        let mut g = NavGuard::default();
        assert!(!g.busy(0.0));
        g.note(100.0);
        assert!(g.busy(100.0 + BUSY_MS - 1.0));
        assert!(!g.busy(100.0 + BUSY_MS));
        g.set_dragging(true, 1000.0);
        assert!(g.busy(1e9));
        g.set_dragging(false, 1000.0);
        assert!(g.busy(1100.0));
        assert!(!g.busy(1000.0 + BUSY_MS));
    }

    #[test]
    fn back_keeps_a_bounded_stack() {
        let mut b: Back<u32> = Back::default();
        assert!(b.pop().is_none());
        for i in 0..(BACK_DEPTH as u32 + 5) {
            b.push(i);
        }
        assert_eq!(b.len(), BACK_DEPTH);
        assert_eq!(b.pop(), Some(BACK_DEPTH as u32 + 4));
        b.clear();
        assert!(b.is_empty());
        // Over the depth, the protected entry (the view of the person) stays.
        let mut b: Back<u32> = Back::default();
        b.push_protected(0);
        for i in 1..(BACK_DEPTH as u32 * 3) {
            b.push(i);
        }
        assert_eq!(b.len(), BACK_DEPTH);
        let mut all = vec![];
        while let Some(x) = b.pop() {
            all.push(x);
        }
        assert_eq!(all.last(), Some(&0), "the person's view is at the bottom");
        assert_eq!(all[0], BACK_DEPTH as u32 * 3 - 1);
        // The newest view of the person is protected instead of the older one.
        let mut b: Back<u32> = Back::default();
        b.push_protected(0);
        for i in 1..10 {
            b.push(i);
        }
        b.push_protected(100);
        for i in 11..(BACK_DEPTH as u32 * 3) {
            b.push(i);
        }
        let mut all = vec![];
        while let Some(x) = b.pop() {
            all.push(x);
        }
        assert!(all.contains(&100), "the newest view of the person stays");
        assert!(!all.contains(&0), "the older one may go");
    }

    #[test]
    fn framing_a_box_shows_it_with_the_margin() {
        let size = DVec2::new(800.0, 600.0);
        let (lo, hi) = (DVec2::new(0.0, 0.0), DVec2::new(10_000.0, 5_000.0));
        let (pose, clamped) = frame_plan(lo, hi, 0.1, None, size);
        assert!(!clamped);
        let v = pose.view(DVec2::ZERO, size);
        for p in [lo, hi] {
            let s = v.screen_of(p);
            assert!(
                s.x >= 0.0 && s.x <= 800.0 && s.y >= 0.0 && s.y <= 600.0,
                "{s:?}"
            );
        }
        // 10 percent on each side: the box takes 1/1.2 of the width.
        assert!((v.zoom * 10_000.0 * 1.2 - 800.0).abs() < 1e-6);
        assert!(v.screen_of((lo + hi) / 2.0).distance(size / 2.0) < 1e-9);
        // A point gets the smallest frame, and the scale is used as given.
        let (p, _) = frame_plan(DVec2::ZERO, DVec2::ZERO, 0.1, None, size);
        assert!(p.zoom < MAX_ZOOM);
        let (p, c) = frame_plan(DVec2::ZERO, DVec2::ZERO, 0.0, Some(0.25), size);
        assert_eq!((p.zoom, c), (0.25, false));
        // The zoom limit is reported.
        let (p, c) = frame_plan(DVec2::ZERO, DVec2::splat(1e9), 0.0, None, size);
        assert!(c && p.zoom == MIN_ZOOM);
    }

    #[test]
    fn a_pose_survives_the_round_trip_through_a_view() {
        let size = DVec2::new(640.0, 480.0);
        let v = View {
            origin: DVec2::new(100.0, 50.0),
            zoom: 0.2,
        };
        let min = DVec2::new(250.0, 80.0);
        let pose = PlanPose::of(v, min, size);
        let back = pose.view(min, size);
        assert!((back.origin - v.origin).length() < 1e-9 && back.zoom == v.zoom);
        // The middle of the canvas, which is offset in the window, is the middle of the pose.
        assert!(v.screen_of(pose.center).distance(min + size / 2.0) < 1e-9);
    }

    #[test]
    fn the_plan_animation_goes_from_start_to_end_without_overshoot() {
        let a = PlanAnim {
            from: PlanPose {
                center: DVec2::ZERO,
                zoom: 0.01,
            },
            to: PlanPose {
                center: DVec2::new(1000.0, 0.0),
                zoom: 1.0,
            },
            start_ms: 100.0,
            duration_ms: ANIM_MS,
        };
        assert!((a.at(100.0).0.zoom - 0.01).abs() < 1e-12);
        assert_eq!(a.at(100.0).0.center, a.from.center);
        assert_eq!(a.at(100.0 + ANIM_MS), (a.to, true));
        assert_eq!(a.at(1e9), (a.to, true));
        let mut last = 0.0;
        for i in 0..=10 {
            let (p, _) = a.at(100.0 + ANIM_MS * i as f64 / 10.0);
            assert!(p.zoom >= last && p.zoom <= 1.0 + 1e-12, "{}", p.zoom);
            assert!(p.center.x >= 0.0 && p.center.x <= 1000.0);
            last = p.zoom;
        }
        // The same time gives the same pose.
        assert_eq!(a.at(180.0), a.at(180.0));
        // No duration: at once.
        let now = PlanAnim {
            duration_ms: 0.0,
            ..a
        };
        assert!(now.at(0.0).1);
    }

    #[test]
    fn the_camera_animation_turns_the_short_way() {
        let from = Camera {
            target: Vec3::ZERO,
            yaw: 0.1,
            pitch: 0.0,
            distance: 1000.0,
            projection: Projection::Orthographic,
        };
        let to = Camera {
            yaw: std::f32::consts::TAU - 0.1,
            distance: 4000.0,
            target: Vec3::new(100.0, 0.0, 0.0),
            projection: Projection::Perspective,
            ..from
        };
        let a = CamAnim {
            from,
            to,
            start_ms: 0.0,
            duration_ms: ANIM_MS,
        };
        let (mid, done) = a.at(ANIM_MS / 2.0);
        assert!(!done);
        // Half way round the short way is yaw 0 (or 2 pi), not pi.
        let d = mid.yaw.min(std::f32::consts::TAU - mid.yaw);
        assert!(d < 0.05, "{}", mid.yaw);
        assert!((mid.distance - 2000.0).abs() < 1.0, "{}", mid.distance);
        assert_eq!(a.at(ANIM_MS), (to, true));
    }

    #[test]
    fn a_3d_frame_keeps_the_direction_and_shows_the_box() {
        let cam = Camera::standard(
            kadakas_render::StandardView::Front,
            Projection::Orthographic,
        );
        let size = DVec2::new(800.0, 600.0);
        let (lo, hi) = (
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(10_000.0, 2_000.0, 3_000.0),
        );
        let out = frame_camera(cam, lo, hi, 0.1, None, size);
        assert_eq!((out.yaw, out.pitch), (cam.yaw, cam.pitch));
        assert!((out.target - (lo + hi) / 2.0).length() < 1e-3);
        let aspect = 800.0 / 600.0;
        for i in 0..8 {
            let p = Vec3::new(
                if i & 1 == 0 { lo.x } else { hi.x },
                if i & 2 == 0 { lo.y } else { hi.y },
                if i & 4 == 0 { lo.z } else { hi.z },
            );
            let n = out.project(p, aspect, None).unwrap();
            assert!(n.x.abs() <= 1.0 && n.y.abs() <= 1.0, "{n:?}");
        }
        // A scale sets the height of the view: 600 px at 0.5 px/mm are 1200 mm.
        let s = frame_camera(cam, lo, hi, 0.1, Some(0.5), size);
        assert!(
            (s.half_height() * 2.0 - 1200.0).abs() < 1.0,
            "{}",
            s.half_height()
        );
    }

    #[test]
    fn pointers_made_in_the_plan_wait_for_the_ground() {
        let mut set = HighlightSet::default();
        let mut p = HighlightReq::parse(&json!({"at": [10, 20]})).unwrap();
        p.ground = f64::NAN;
        set.add(p, 0.0, 0).unwrap();
        let b = HighlightReq::parse(&json!({"box": [0, 0, 10, 10]})).unwrap();
        set.add(b, 0.0, 0).unwrap();
        let o = HighlightReq::parse(&json!({"ids": [uid(1)]})).unwrap();
        set.add(o, 0.0, 0).unwrap();
        assert!(set.needs_ground());
        let mut asked = vec![];
        set.resolve_ground(&mut |xy| {
            asked.push(xy);
            42.0
        });
        assert_eq!(asked, vec![DVec2::new(10.0, 20.0), DVec2::new(5.0, 5.0)]);
        assert!(!set.needs_ground());
        // A second pass asks nothing.
        set.resolve_ground(&mut |_| panic!("already known"));
    }

    #[test]
    fn the_ground_height_is_the_highest_surface_over_a_point() {
        let mut scene = kadakas_core::mesh::Scene::default();
        let m = &mut scene.mesh;
        // A slab 0..100 at height 5, and a higher plate over part of it at height 40.
        for (z, x0, x1) in [(5.0, 0.0, 100.0), (40.0, 0.0, 50.0)] {
            let i = m.positions.len() as u32;
            m.positions.extend([
                Vec3::new(x0, 0.0, z),
                Vec3::new(x1, 0.0, z),
                Vec3::new(x1, 100.0, z),
                Vec3::new(x0, 100.0, z),
            ]);
            m.indices.extend([i, i + 1, i + 2, i, i + 2, i + 3]);
        }
        assert_eq!(
            ground_height(&scene, glam::Vec2::new(75.0, 50.0)),
            Some(5.0)
        );
        assert_eq!(
            ground_height(&scene, glam::Vec2::new(25.0, 50.0)),
            Some(40.0)
        );
        assert_eq!(ground_height(&scene, glam::Vec2::new(500.0, 50.0)), None);
    }

    #[test]
    fn the_ground_box_of_a_top_view_is_the_visible_area() {
        let cam = Camera {
            target: Vec3::new(1000.0, 2000.0, 0.0),
            distance: 1000.0,
            ..Camera::standard(kadakas_render::StandardView::Top, Projection::Orthographic)
        };
        let (lo, hi) = ground_box(&cam, 2.0, 0.0).unwrap();
        let h = f64::from(cam.half_height());
        assert!((hi.y - lo.y - 2.0 * h).abs() < 1.0, "{lo:?} {hi:?}");
        assert!((hi.x - lo.x - 4.0 * h).abs() < 1.0);
        // A level camera never reaches the ground.
        let level = Camera::standard(
            kadakas_render::StandardView::Front,
            Projection::Orthographic,
        );
        assert!(ground_box(&level, 1.0, 0.0).is_none());
    }
}
