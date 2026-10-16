//! The IFC schema versions that the writer can produce, and the changes that each needs.
//!
//! The writer is made for IFC4. For IFC2x3 and IFC4x3 it keeps the same code and adapts each entity
//! when it is written (see [`adapt`]). IFC4x3 is IFC4 plus a few changes. IFC2x3 needs more: an owner
//! history on every rooted entity, shorter attribute lists for a few entities, no
//! `IfcGeographicElement`, and no map conversion (it becomes a property set).

use crate::step::Value;

/// The schema of the file that is written.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Schema {
    Ifc2x3,
    #[default]
    Ifc4,
    /// IFC4X3_ADD2.
    Ifc4x3,
}

impl Schema {
    /// Reads a version name: `IFC2X3`, `IFC4`, `IFC4X3` or `IFC4X3_ADD2`, in any case. The short
    /// forms `2X3`, `4` and `4X3` work too.
    pub fn from_name(name: &str) -> Option<Schema> {
        match name.trim().to_ascii_uppercase().as_str() {
            "IFC2X3" | "2X3" | "IFC2X3_TC1" => Some(Schema::Ifc2x3),
            "IFC4" | "4" | "IFC4_ADD2" | "IFC4_ADD2_TC1" => Some(Schema::Ifc4),
            "IFC4X3" | "4X3" | "IFC4X3_ADD2" => Some(Schema::Ifc4x3),
            _ => None,
        }
    }

    /// The name in `FILE_SCHEMA`.
    pub fn file_schema(self) -> &'static str {
        match self {
            Schema::Ifc2x3 => "IFC2X3",
            Schema::Ifc4 => "IFC4",
            Schema::Ifc4x3 => "IFC4X3_ADD2",
        }
    }

    /// The model view definition in the file description.
    pub fn view(self) -> &'static str {
        match self {
            Schema::Ifc2x3 => "ViewDefinition [CoordinationView_V2.0]",
            Schema::Ifc4 => "ViewDefinition [ReferenceView_V1.2]",
            Schema::Ifc4x3 => "ViewDefinition [ReferenceView]",
        }
    }
}

/// Entities that are rooted (have a GlobalId and an OwnerHistory) and that the writer makes.
const ROOTED: [&str; 16] = [
    "IFCWALL",
    "IFCPROJECT",
    "IFCSITE",
    "IFCBUILDING",
    "IFCBUILDINGSTOREY",
    "IFCSLAB",
    "IFCCOVERING",
    "IFCBUILDINGELEMENTPROXY",
    "IFCGEOGRAPHICELEMENT",
    "IFCANNOTATION",
    "IFCRELAGGREGATES",
    "IFCRELCONTAINEDINSPATIALSTRUCTURE",
    "IFCRELDEFINESBYPROPERTIES",
    "IFCRELASSOCIATESMATERIAL",
    "IFCPROPERTYSET",
    "IFCELEMENTQUANTITY",
];

/// The covering types that IFC2x3 knows.
const COVERING_2X3: [&str; 9] = [
    "CEILING",
    "FLOORING",
    "CLADDING",
    "ROOFING",
    "INSULATION",
    "MEMBRANE",
    "SLEEVING",
    "WRAPPING",
    "NOTDEFINED",
];

/// The entity as the schema wants it: its name and attributes. `owner_history` is the ID of the
/// `IfcOwnerHistory` (IFC2x3 only).
pub fn adapt(
    schema: Schema,
    name: &str,
    mut attrs: Vec<Value>,
    owner_history: Option<u64>,
) -> (String, Vec<Value>) {
    match schema {
        Schema::Ifc4 => return (name.to_string(), attrs),
        Schema::Ifc4x3 => {
            // IfcAnnotation got a PredefinedType.
            if name == "IFCANNOTATION" && attrs.len() == 7 {
                attrs.push(Value::Null);
            }
            // IfcCountMeasure is an integer in IFC4x3.
            if name == "IFCQUANTITYCOUNT"
                && let Some(Value::Real(x)) = attrs.get(3).cloned()
            {
                attrs[3] = Value::Int(x.round() as i64);
            }
            return (name.to_string(), attrs);
        }
        Schema::Ifc2x3 => {}
    }
    let mut name = name.to_string();
    if ROOTED.contains(&name.as_str())
        && let (Some(oh), Some(slot)) = (owner_history, attrs.get_mut(1))
        && *slot == Value::Null
    {
        *slot = Value::Ref(oh);
    }
    match name.as_str() {
        // No geographic elements in IFC2x3: a building element proxy, with the meaning in ObjectType.
        "IFCGEOGRAPHICELEMENT" => {
            name = "IFCBUILDINGELEMENTPROXY".into();
            if attrs.len() == 9 {
                if attrs[4] == Value::Null
                    && let Value::Enum(p) = &attrs[8]
                    && p == "TERRAIN"
                {
                    attrs[4] = Value::s("TERRAIN");
                }
                attrs[8] = Value::e("ELEMENT");
            }
        }
        "IFCBUILDINGELEMENTPROXY" => {
            // PredefinedType became CompositionType.
            if let Some(last) = attrs.get_mut(8)
                && matches!(last, Value::Enum(_))
            {
                *last = Value::e("ELEMENT");
            }
        }
        // IfcWall has no PredefinedType in IFC2x3.
        "IFCWALL" => attrs.truncate(8),
        "IFCCOVERING" => {
            if let Some(Value::Enum(p)) = attrs.get_mut(8)
                && !COVERING_2X3.contains(&p.as_str())
            {
                *p = "NOTDEFINED".into();
            }
        }
        // Shorter attribute lists.
        "IFCQUANTITYLENGTH" | "IFCQUANTITYAREA" | "IFCQUANTITYVOLUME" | "IFCQUANTITYCOUNT" => {
            attrs.truncate(4)
        }
        "IFCMATERIAL" => attrs.truncate(1),
        "IFCMATERIALLAYER" => attrs.truncate(3),
        "IFCMATERIALLAYERSET" => attrs.truncate(2),
        "IFCMATERIALLAYERSETUSAGE" => attrs.truncate(4),
        // Transparency is an attribute of the rendering, not of the shading.
        "IFCSURFACESTYLESHADING" => {
            name = "IFCSURFACESTYLERENDERING".into();
            attrs.truncate(2);
            attrs.extend([
                Value::Null,
                Value::Null,
                Value::Null,
                Value::Null,
                Value::Null,
                Value::Null,
                Value::e("NOTDEFINED"),
            ]);
        }
        // A site has at most three numbers in an angle in IFC2x3.
        "IFCSITE" => {
            for i in [9, 10] {
                if let Some(Value::List(l)) = attrs.get_mut(i) {
                    l.truncate(3);
                }
            }
        }
        _ => {}
    }
    (name, attrs)
}

/// Seconds since 1970-01-01 for a time stamp `YYYY-MM-DDThh:mm:ss` (UTC). 0 for a bad text.
pub fn epoch_seconds(t: &str) -> i64 {
    let b = t.as_bytes();
    if b.len() != 19 {
        return 0;
    }
    let num = |a: usize, z: usize| t[a..z].parse::<i64>().ok();
    let (Some(y), Some(mo), Some(d), Some(h), Some(mi), Some(s)) = (
        num(0, 4),
        num(5, 7),
        num(8, 10),
        num(11, 13),
        num(14, 16),
        num(17, 19),
    ) else {
        return 0;
    };
    // Days from the civil date (Howard Hinnant's algorithm).
    let y = if mo <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    days * 86_400 + h * 3600 + mi * 60 + s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_of_versions() {
        assert_eq!(Schema::from_name("ifc2x3"), Some(Schema::Ifc2x3));
        assert_eq!(Schema::from_name("IFC4"), Some(Schema::Ifc4));
        assert_eq!(Schema::from_name(" 4x3 "), Some(Schema::Ifc4x3));
        assert_eq!(Schema::from_name("IFC4X3_ADD2"), Some(Schema::Ifc4x3));
        assert_eq!(Schema::from_name("IFC5"), None);
        for s in [Schema::Ifc2x3, Schema::Ifc4, Schema::Ifc4x3] {
            assert_eq!(Schema::from_name(s.file_schema()), Some(s));
            assert!(s.view().starts_with("ViewDefinition"));
        }
        assert_eq!(Schema::default(), Schema::Ifc4);
    }

    #[test]
    fn ifc4_is_unchanged_and_ifc4x3_adds_the_annotation_type() {
        let a = vec![Value::Null; 7];
        assert_eq!(
            adapt(Schema::Ifc4, "IFCANNOTATION", a.clone(), None)
                .1
                .len(),
            7
        );
        assert_eq!(
            adapt(Schema::Ifc4x3, "IFCANNOTATION", a.clone(), None)
                .1
                .len(),
            8
        );
        assert_eq!(adapt(Schema::Ifc4x3, "IFCSLAB", a.clone(), None).1.len(), 7);
        let q = vec![
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Real(128.0),
            Value::Null,
        ];
        assert_eq!(
            adapt(Schema::Ifc4x3, "IFCQUANTITYCOUNT", q.clone(), None).1[3],
            Value::Int(128)
        );
        assert_eq!(
            adapt(Schema::Ifc4, "IFCQUANTITYCOUNT", q, None).1[3],
            Value::Real(128.0)
        );
    }

    #[test]
    fn ifc2x3_gets_an_owner_history_and_short_lists() {
        let (n, a) = adapt(
            Schema::Ifc2x3,
            "IFCSLAB",
            vec![Value::s("g"), Value::Null, Value::s("n")],
            Some(7),
        );
        assert_eq!(n, "IFCSLAB");
        assert_eq!(a[1], Value::Ref(7));
        // An owner history that is set stays.
        let (_, a) = adapt(
            Schema::Ifc2x3,
            "IFCSLAB",
            vec![Value::Null, Value::Ref(3)],
            Some(7),
        );
        assert_eq!(a[1], Value::Ref(3));
        // Not rooted: untouched.
        let (_, a) = adapt(
            Schema::Ifc2x3,
            "IFCCARTESIANPOINT",
            vec![Value::Null, Value::Null],
            Some(7),
        );
        assert_eq!(a[1], Value::Null);
        let q = vec![Value::Null; 5];
        assert_eq!(
            adapt(Schema::Ifc2x3, "IFCQUANTITYAREA", q.clone(), None)
                .1
                .len(),
            4
        );
        assert_eq!(
            adapt(Schema::Ifc2x3, "IFCMATERIAL", vec![Value::Null; 3], None)
                .1
                .len(),
            1
        );
        assert_eq!(
            adapt(
                Schema::Ifc2x3,
                "IFCMATERIALLAYER",
                vec![Value::Null; 7],
                None
            )
            .1
            .len(),
            3
        );
        assert_eq!(
            adapt(
                Schema::Ifc2x3,
                "IFCMATERIALLAYERSET",
                vec![Value::Null; 3],
                None
            )
            .1
            .len(),
            2
        );
        assert_eq!(
            adapt(
                Schema::Ifc2x3,
                "IFCMATERIALLAYERSETUSAGE",
                vec![Value::Null; 5],
                None
            )
            .1
            .len(),
            4
        );
    }

    #[test]
    fn ifc2x3_has_no_geographic_element() {
        let mut a = vec![Value::Null; 9];
        a[8] = Value::e("TERRAIN");
        let (n, a) = adapt(Schema::Ifc2x3, "IFCGEOGRAPHICELEMENT", a, Some(1));
        assert_eq!(n, "IFCBUILDINGELEMENTPROXY");
        assert_eq!(a[4], Value::s("TERRAIN"));
        assert_eq!(a[8], Value::e("ELEMENT"));
        // A user-defined type keeps its object type.
        let mut b = vec![Value::Null; 9];
        b[4] = Value::s("VEGETATION");
        b[8] = Value::e("USERDEFINED");
        let (_, b) = adapt(Schema::Ifc2x3, "IFCGEOGRAPHICELEMENT", b, None);
        assert_eq!(b[4], Value::s("VEGETATION"));
        // A proxy: PredefinedType becomes CompositionType.
        let mut p = vec![Value::Null; 9];
        p[8] = Value::e("NOTDEFINED");
        assert_eq!(
            adapt(Schema::Ifc2x3, "IFCBUILDINGELEMENTPROXY", p, None).1[8],
            Value::e("ELEMENT")
        );
    }

    #[test]
    fn ifc2x3_coverings_shading_and_sites() {
        let mut c = vec![Value::Null; 9];
        c[8] = Value::e("MOLDING");
        assert_eq!(
            adapt(Schema::Ifc2x3, "IFCCOVERING", c.clone(), None).1[8],
            Value::e("NOTDEFINED")
        );
        c[8] = Value::e("CEILING");
        assert_eq!(
            adapt(Schema::Ifc2x3, "IFCCOVERING", c, None).1[8],
            Value::e("CEILING")
        );
        let (n, a) = adapt(
            Schema::Ifc2x3,
            "IFCSURFACESTYLESHADING",
            vec![Value::Ref(1), Value::Real(0.2)],
            None,
        );
        assert_eq!(n, "IFCSURFACESTYLERENDERING");
        assert_eq!(a.len(), 9);
        assert_eq!(a[8], Value::e("NOTDEFINED"));
        let mut s = vec![Value::Null; 14];
        s[9] = Value::List(vec![
            Value::Int(1),
            Value::Int(2),
            Value::Int(3),
            Value::Int(4),
        ]);
        s[10] = Value::List(vec![
            Value::Int(1),
            Value::Int(2),
            Value::Int(3),
            Value::Int(4),
        ]);
        let (_, s) = adapt(Schema::Ifc2x3, "IFCSITE", s, None);
        assert!(matches!(&s[9], Value::List(l) if l.len() == 3));
        assert!(matches!(&s[10], Value::List(l) if l.len() == 3));
    }

    #[test]
    fn epoch_from_a_time_stamp() {
        assert_eq!(epoch_seconds("1970-01-01T00:00:00"), 0);
        assert_eq!(epoch_seconds("2000-01-01T00:00:00"), 946_684_800);
        assert_eq!(epoch_seconds("2026-10-10T12:30:15"), 1_791_635_415);
        assert_eq!(epoch_seconds("2024-02-29T00:00:00"), 1_709_164_800);
        assert_eq!(epoch_seconds("bad"), 0);
        assert_eq!(epoch_seconds("2026-xx-10T12:30:15"), 0);
    }
}
