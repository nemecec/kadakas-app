//! The logic of the Versions window (REV-01, REV-02), without egui.
//!
//! The window calls the `version.*` methods of the API and shows the answers. This module reads
//! the JSON of those answers into plain rows, checks the form of a new version, and lists the
//! changes of a comparison. The window only draws them. The texts are in the app
//! (`versions-ui.ftl`).

use serde_json::{Value, json};

/// The longest name of a version, in characters (the limit of the API).
pub const NAME_MAX: usize = 120;
/// The longest note of a version, in characters (the limit of the API).
pub const NOTE_MAX: usize = 2000;
/// The most rows of a comparison that the window keeps.
pub const MAX_ROWS: usize = 1000;

/// One version, as `version.list` gives it.
#[derive(Clone, Debug, PartialEq)]
pub struct VersionRow {
    pub id: u64,
    pub name: String,
    pub note: Option<String>,
    pub author: String,
    /// Milliseconds since 1970 (UTC); 0 when the time is not known.
    pub time_ms: u64,
    /// `user`, `agent`, `script` or `remote`.
    pub origin: String,
    /// A version that Kadakas saved by itself, before a restore.
    pub auto: bool,
}

/// The answer of `version.list`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VersionList {
    /// Oldest first.
    pub rows: Vec<VersionRow>,
    /// `None`: there is no version, or the answer was asked without the comparison.
    pub current_is_newest: Option<bool>,
    pub max_versions: usize,
    /// True: the history of the file could not be read completely. It is kept as it is, and no
    /// version can be saved, restored or deleted until the person decides (`version.repair`).
    pub read_only: bool,
    /// What could not be read.
    pub notes: Vec<String>,
    /// With `read_only`: how many versions `version.repair` would delete. `None`: not known.
    pub unreadable: Option<usize>,
}

impl VersionList {
    /// Reads the answer. A row that does not read is left out.
    pub fn parse(v: &Value) -> VersionList {
        let rows = v["versions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| {
                Some(VersionRow {
                    id: r["id"].as_u64()?,
                    name: r["name"].as_str()?.to_string(),
                    note: r["note"].as_str().map(str::to_string),
                    author: r["author"].as_str().unwrap_or("").to_string(),
                    time_ms: r["time_ms"].as_u64().unwrap_or(0),
                    origin: r["origin"].as_str().unwrap_or("user").to_string(),
                    auto: r["auto"].as_bool().unwrap_or(false),
                })
            })
            .collect();
        VersionList {
            rows,
            current_is_newest: v["current_is_newest"].as_bool(),
            max_versions: v["limits"]["max_versions"]
                .as_u64()
                .map_or(0, |n| usize::try_from(n).unwrap_or(usize::MAX)),
            read_only: v["read_only"].as_bool().unwrap_or(false),
            notes: v["notes"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|n| n.as_str().map(str::to_string))
                .collect(),
            unreadable: v["unreadable_versions"]
                .as_u64()
                .and_then(|n| usize::try_from(n).ok()),
        }
    }

    pub fn find(&self, id: u64) -> Option<&VersionRow> {
        self.rows.iter().find(|r| r.id == id)
    }

    /// The name of a version for a message, or its number if it is gone.
    pub fn name_of(&self, id: u64) -> String {
        self.find(id)
            .map_or_else(|| id.to_string(), |r| r.name.clone())
    }
}

/// Why the form of a new version is not good.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveError {
    NameEmpty,
    NameTooLong,
    NoteTooLong,
}

/// The form "Save Version".
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SaveForm {
    pub name: String,
    pub note: String,
}

impl SaveForm {
    /// The parameters of `version.create`.
    pub fn params(&self) -> Result<Value, SaveError> {
        let name = self.name.trim();
        if name.is_empty() {
            return Err(SaveError::NameEmpty);
        }
        if name.chars().count() > NAME_MAX {
            return Err(SaveError::NameTooLong);
        }
        let note = self.note.trim();
        if note.chars().count() > NOTE_MAX {
            return Err(SaveError::NoteTooLong);
        }
        let mut p = json!({"name": name});
        if !note.is_empty() {
            p["note"] = json!(note);
        }
        Ok(p)
    }

    pub fn clear(&mut self) {
        *self = SaveForm::default();
    }
}

/// One side of a comparison: a version or the open drawing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Version(u64),
    Current,
}

impl Side {
    pub fn to_json(self) -> Value {
        match self {
            Side::Version(id) => json!(id),
            Side::Current => json!("current"),
        }
    }
}

/// The parameters of `version.diff`.
pub fn diff_params(from: Side, to: Side, limit: usize) -> Value {
    json!({"from": from.to_json(), "to": to.to_json(), "limit": limit})
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Added,
    Removed,
    Changed,
}

/// One thing in a comparison.
#[derive(Clone, Debug, PartialEq)]
pub struct ChangeRow {
    pub change: Change,
    /// `objects`, `layers`, `classes`, `resources` or `document`.
    pub group: String,
    pub id: String,
    /// The kind of shape of an object.
    pub shape: Option<String>,
    /// The name of a layer, class or resource.
    pub name: Option<String>,
    /// The changed property paths of a changed thing.
    pub paths: Vec<String>,
}

impl ChangeRow {
    /// True for a thing that a click can select in the open drawing: an object that was not
    /// removed.
    pub fn selectable(&self) -> bool {
        self.group == "objects" && self.change != Change::Removed
    }
}

/// The counts of one group.
#[derive(Clone, Debug, PartialEq)]
pub struct GroupCounts {
    pub group: String,
    pub added: u64,
    pub removed: u64,
    pub changed: u64,
}

/// The answer of `version.diff`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DiffView {
    pub groups: Vec<GroupCounts>,
    pub total_added: u64,
    pub total_removed: u64,
    pub total_changed: u64,
    pub truncated: bool,
    /// False: the work limit stopped the comparison and the counts are partial.
    pub exact: bool,
    pub rows: Vec<ChangeRow>,
}

/// The groups in the order that the window shows them.
const GROUPS: [&str; 5] = ["objects", "layers", "classes", "resources", "document"];

impl DiffView {
    /// Reads the answer. Rows past [`MAX_ROWS`] are left out.
    pub fn parse(v: &Value) -> DiffView {
        let n = |x: &Value| x.as_u64().unwrap_or(0);
        let mut out = DiffView {
            total_added: n(&v["totals"]["added"]),
            total_removed: n(&v["totals"]["removed"]),
            total_changed: n(&v["totals"]["changed"]),
            truncated: v["truncated"].as_bool().unwrap_or(false),
            exact: v["exact"].as_bool().unwrap_or(true),
            ..DiffView::default()
        };
        for g in GROUPS {
            let c = &v["counts"][g];
            if c.is_object() {
                out.groups.push(GroupCounts {
                    group: g.to_string(),
                    added: n(&c["added"]),
                    removed: n(&c["removed"]),
                    changed: n(&c["changed"]),
                });
            }
            for (key, change) in [
                ("added", Change::Added),
                ("removed", Change::Removed),
                ("changed", Change::Changed),
            ] {
                for e in v[g][key].as_array().into_iter().flatten() {
                    if out.rows.len() >= MAX_ROWS {
                        out.truncated = true;
                        return out;
                    }
                    let Some(id) = e["id"].as_str() else { continue };
                    out.rows.push(ChangeRow {
                        change,
                        group: g.to_string(),
                        id: id.to_string(),
                        shape: e["shape"].as_str().map(str::to_string),
                        name: e["name"].as_str().map(str::to_string),
                        paths: e["properties"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|p| p["path"].as_str().map(str::to_string))
                            .collect(),
                    });
                }
            }
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.total_added + self.total_removed + self.total_changed == 0
    }
}

/// Which limit a refused save broke.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LimitKind {
    Versions,
    Bytes,
}

/// A version in the list of a refused save.
#[derive(Clone, Debug, PartialEq)]
pub struct LimitRow {
    pub id: u64,
    pub name: String,
    pub bytes: u64,
    pub time_ms: u64,
    pub auto: bool,
    /// The person ticked it for deletion.
    pub ticked: bool,
}

/// The facts of the error "the version limit is reached" (`data` of a `version.create` or
/// `version.restore` error).
#[derive(Clone, Debug, PartialEq)]
pub struct LimitReached {
    pub kind: LimitKind,
    pub max: u64,
    pub needed: u64,
    pub rows: Vec<LimitRow>,
}

impl LimitReached {
    /// Reads the details of the error. `None` if they do not have the facts.
    pub fn parse(details: &Value) -> Option<LimitReached> {
        let kind = match details["limit"]["kind"].as_str()? {
            "versions" => LimitKind::Versions,
            "bytes" => LimitKind::Bytes,
            _ => return None,
        };
        let rows = details["versions"]
            .as_array()?
            .iter()
            .filter_map(|r| {
                Some(LimitRow {
                    id: r["id"].as_u64()?,
                    name: r["name"].as_str()?.to_string(),
                    bytes: r["bytes"].as_u64().unwrap_or(0),
                    time_ms: r["time_ms"].as_u64().unwrap_or(0),
                    auto: r["auto"].as_bool().unwrap_or(false),
                    ticked: false,
                })
            })
            .collect();
        Some(LimitReached {
            kind,
            max: details["limit"]["max"].as_u64()?,
            needed: details["limit"]["needed"].as_u64()?,
            rows,
        })
    }

    /// The IDs that the person ticked.
    pub fn ticked(&self) -> Vec<u64> {
        self.rows
            .iter()
            .filter(|r| r.ticked)
            .map(|r| r.id)
            .collect()
    }

    /// The bytes that deleting the ticked versions frees, about.
    pub fn freed(&self) -> u64 {
        self.rows.iter().filter(|r| r.ticked).map(|r| r.bytes).sum()
    }

    /// True if deleting the ticked versions would make room for the new version.
    pub fn enough(&self) -> bool {
        match self.kind {
            LimitKind::Versions => {
                self.needed.saturating_sub(self.ticked().len() as u64) <= self.max
            }
            LimitKind::Bytes => self.needed.saturating_sub(self.freed()) <= self.max,
        }
    }
}

/// The unit of a size that a person reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SizeUnit {
    Kib,
    Mib,
}

/// A size in bytes as a number and a unit: KiB below one MiB, else MiB.
pub fn size_parts(bytes: u64) -> (f64, SizeUnit) {
    const MIB: u64 = 1024 * 1024;
    if bytes < MIB {
        ((bytes as f64 / 1024.0).max(0.1), SizeUnit::Kib)
    } else {
        (bytes as f64 / MIB as f64, SizeUnit::Mib)
    }
}

/// The date and the time of day (UTC) of a time in milliseconds: `(unix seconds, hours, minutes)`.
/// `None` for 0, which means that the time is not known.
pub fn time_parts(time_ms: u64) -> Option<(i64, u32, u32)> {
    if time_ms == 0 {
        return None;
    }
    let secs = i64::try_from(time_ms / 1000).ok()?;
    let day = secs.rem_euclid(86_400);
    Some((secs, (day / 3600) as u32, (day % 3600 / 60) as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list() -> Value {
        json!({
            "versions": [
                {"id": 1, "name": "Concept", "note": "first", "author": "Mari", "time_ms": 1_700_000_000_000u64, "origin": "user"},
                {"id": 2, "name": "Before restoring Concept", "author": "agent", "time_ms": 0, "origin": "agent", "auto": true},
                {"name": "no id"},
                5
            ],
            "limits": {"max_versions": 1000, "max_bytes": 1},
            "current_is_newest": false
        })
    }

    #[test]
    fn a_refused_save_gives_the_versions_to_choose_from() {
        let details = json!({
            "limit": {"kind": "versions", "max": 3, "needed": 4},
            "versions": [
                {"id": 1, "name": "A", "bytes": 5000, "time_ms": 1000, "auto": false},
                {"id": 2, "name": "B", "bytes": 3_000_000, "time_ms": 0, "auto": true},
                {"name": "no id"}
            ]
        });
        let mut l = LimitReached::parse(&details).unwrap();
        assert_eq!(
            (l.kind, l.max, l.needed, l.rows.len()),
            (LimitKind::Versions, 3, 4, 2)
        );
        assert!(l.rows[1].auto && !l.rows[0].ticked);
        assert!(l.ticked().is_empty() && !l.enough());
        l.rows[1].ticked = true;
        assert_eq!(l.ticked(), vec![2]);
        assert_eq!(l.freed(), 3_000_000);
        assert!(l.enough());
        let mut b = LimitReached::parse(&json!({
            "limit": {"kind": "bytes", "max": 1000, "needed": 1500},
            "versions": [{"id": 7, "name": "x", "bytes": 400}, {"id": 8, "name": "y", "bytes": 900}]
        }))
        .unwrap();
        assert!(!b.enough());
        b.rows[0].ticked = true;
        assert!(!b.enough(), "400 bytes are not enough");
        b.rows[1].ticked = true;
        assert!(b.enough());
        for bad in [
            json!({}),
            json!({"limit": {"kind": "other", "max": 1, "needed": 2}, "versions": []}),
            json!({"limit": {"kind": "bytes", "max": 1}, "versions": []}),
            json!({"limit": {"kind": "bytes", "max": 1, "needed": 2}}),
        ] {
            assert!(LimitReached::parse(&bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn sizes_are_shown_in_kib_or_mib() {
        assert_eq!(size_parts(0).1, SizeUnit::Kib);
        assert!(size_parts(0).0 >= 0.1);
        assert_eq!(size_parts(2048), (2.0, SizeUnit::Kib));
        assert_eq!(size_parts(1024 * 1024 - 1).1, SizeUnit::Kib);
        assert_eq!(size_parts(3 * 1024 * 1024), (3.0, SizeUnit::Mib));
    }

    #[test]
    fn a_list_tells_when_the_history_is_read_only() {
        let l =
            VersionList::parse(&json!({"read_only": true, "notes": ["a", 5, "b"], "versions": []}));
        assert!(l.read_only);
        assert_eq!(l.notes, vec!["a", "b"]);
        assert!(!VersionList::parse(&json!({})).read_only);
        let l = VersionList::parse(&json!({"read_only": true, "unreadable_versions": 7}));
        assert_eq!(l.unreadable, Some(7));
        assert_eq!(
            VersionList::parse(&json!({"read_only": true})).unreadable,
            None
        );
    }

    #[test]
    fn the_limits_are_those_of_the_version_store() {
        assert_eq!(NAME_MAX, kadakas_review::history::NAME_MAX);
        assert_eq!(NOTE_MAX, kadakas_review::history::NOTE_MAX);
    }

    #[test]
    fn the_list_is_read_and_bad_rows_are_left_out() {
        let l = VersionList::parse(&list());
        assert_eq!(l.rows.len(), 2);
        assert_eq!(l.rows[0].note.as_deref(), Some("first"));
        assert!(l.rows[1].auto && l.rows[1].note.is_none());
        assert_eq!(l.current_is_newest, Some(false));
        assert_eq!(l.max_versions, 1000);
        assert_eq!(l.find(2).unwrap().origin, "agent");
        assert_eq!(l.name_of(1), "Concept");
        assert_eq!(l.name_of(9), "9");
        let empty = VersionList::parse(&json!({}));
        assert!(empty.rows.is_empty() && empty.current_is_newest.is_none());
        assert_eq!(
            VersionList::parse(&json!({"current_is_newest": null})).max_versions,
            0
        );
    }

    #[test]
    fn the_save_form_checks_and_builds_the_parameters() {
        let mut f = SaveForm::default();
        assert_eq!(f.params(), Err(SaveError::NameEmpty));
        f.name = "  Plan A  ".into();
        assert_eq!(f.params().unwrap(), json!({"name": "Plan A"}));
        f.note = " for the client ".into();
        assert_eq!(
            f.params().unwrap(),
            json!({"name": "Plan A", "note": "for the client"})
        );
        f.note = "n".repeat(NOTE_MAX + 1);
        assert_eq!(f.params(), Err(SaveError::NoteTooLong));
        f.note.clear();
        f.name = "n".repeat(NAME_MAX + 1);
        assert_eq!(f.params(), Err(SaveError::NameTooLong));
        f.name = "é".repeat(NAME_MAX);
        assert!(f.params().is_ok(), "the limit counts characters");
        f.clear();
        assert_eq!(f, SaveForm::default());
    }

    #[test]
    fn the_diff_parameters_name_the_sides() {
        assert_eq!(
            diff_params(Side::Version(3), Side::Current, 50),
            json!({"from": 3, "to": "current", "limit": 50})
        );
        assert_eq!(Side::Version(1).to_json(), json!(1));
    }

    fn diff() -> Value {
        json!({
            "counts": {
                "objects": {"added": 1, "removed": 1, "changed": 1},
                "layers": {"added": 1, "removed": 0, "changed": 0}
            },
            "totals": {"added": 2, "removed": 1, "changed": 1},
            "truncated": true,
            "exact": true,
            "objects": {
                "added": [{"id": "a", "shape": "rect"}],
                "removed": [{"id": "b", "shape": "circle"}, {"shape": "bad"}],
                "changed": [{"id": "c", "shape": "line", "properties": [{"path": "shape"}, {"path": "attrs.pen"}, {}]}]
            },
            "layers": {"added": [{"id": "l", "name": "Paths"}], "removed": [], "changed": []}
        })
    }

    #[test]
    fn the_diff_is_read_into_rows_in_group_order() {
        let d = DiffView::parse(&diff());
        assert_eq!(d.groups.len(), 2);
        assert_eq!(d.groups[0].group, "objects");
        assert_eq!((d.total_added, d.total_removed, d.total_changed), (2, 1, 1));
        assert!(d.truncated && d.exact && !d.is_empty());
        let ids: Vec<&str> = d.rows.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["a", "b", "c", "l"]);
        assert_eq!(d.rows[2].paths, ["shape", "attrs.pen"]);
        assert_eq!(d.rows[3].name.as_deref(), Some("Paths"));
        assert!(d.rows[0].selectable() && d.rows[2].selectable());
        assert!(
            !d.rows[1].selectable(),
            "a removed object is not in the drawing"
        );
        assert!(!d.rows[3].selectable());
        let none = DiffView::parse(&json!({"totals": {"added": 0}, "exact": false}));
        assert!(none.is_empty() && !none.exact && none.rows.is_empty());
    }

    #[test]
    fn the_rows_are_bounded() {
        let many: Vec<Value> = (0..MAX_ROWS + 5)
            .map(|i| json!({"id": i.to_string()}))
            .collect();
        let d = DiffView::parse(&json!({"objects": {"added": many}, "totals": {"added": 1005}}));
        assert_eq!(d.rows.len(), MAX_ROWS);
        assert!(d.truncated);
    }

    #[test]
    fn times_split_into_date_and_clock() {
        assert_eq!(time_parts(0), None);
        let (secs, h, m) = time_parts(1_700_000_000_000).unwrap();
        assert_eq!(secs, 1_700_000_000);
        assert_eq!((h, m), (22, 13));
        assert_eq!(time_parts(86_400_000 + 61_000).unwrap().1, 0);
        assert_eq!(time_parts(u64::MAX).map(|t| t.1 < 24), Some(true));
    }
}
