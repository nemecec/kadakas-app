//! Options of the IFC export. Every field has a default.

use serde::{Deserialize, Serialize};

use crate::IfcError;
pub use crate::export::Schema;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct IfcOptions {
    /// The IFC version: `IFC4` (IFC4 ADD2 TC1, the default), `IFC2X3` (TC1) or `IFC4X3` (IFC4X3 ADD2).
    pub version: String,
    /// Write one presentation layer (`IfcPresentationLayerAssignment`) per class. Default: on.
    pub classes_as_layers: bool,
    /// Write 2D objects (lines, polylines, text, ...) and survey points as `IfcAnnotation`. Default: off.
    pub include_2d: bool,
    /// Write a default `IfcBuilding` with one `IfcBuildingStorey` per design layer. Default: off,
    /// all elements belong to the `IfcSite`.
    pub layers_as_storeys: bool,
    /// Write massing models as `IfcBuilding` (in the site) instead of `IfcBuildingElementProxy`. Default: off.
    pub massing_as_building: bool,
    /// Skip objects on hidden layers and classes.
    pub visible_only: bool,
    /// Name of the data map (Data Manager) that gives IFC classes and property sets. Default: the
    /// first data map of the document, else the built-in landscape mapping.
    pub data_map: Option<String>,
    /// Thickness in mm of the solid of a landscape area. Default 100.
    pub area_thickness: f64,
    /// Time stamp for the file header (`YYYY-MM-DDThh:mm:ss`). Default: the project date, else a fixed
    /// date, so the same document always gives the same bytes.
    pub time_stamp: Option<String>,
    /// File name for the header. Set by `export_file`.
    pub file_name: Option<String>,
}

impl Default for IfcOptions {
    fn default() -> Self {
        IfcOptions {
            version: "IFC4".into(),
            classes_as_layers: true,
            include_2d: false,
            layers_as_storeys: false,
            massing_as_building: false,
            visible_only: false,
            data_map: None,
            area_thickness: 100.0,
            time_stamp: None,
            file_name: None,
        }
    }
}

impl IfcOptions {
    /// Reads options from JSON. `null` gives the defaults.
    pub fn from_json(v: &serde_json::Value) -> Result<IfcOptions, IfcError> {
        if v.is_null() {
            return Ok(IfcOptions::default());
        }
        let o: IfcOptions = serde_json::from_value(v.clone())
            .map_err(|e| IfcError::Option(format!("bad IFC options: {e}")))?;
        o.check()?;
        Ok(o)
    }

    /// Checks the values.
    pub fn check(&self) -> Result<(), IfcError> {
        if Schema::from_name(&self.version).is_none() {
            return Err(IfcError::Option(format!(
                "IFC version '{}' is not supported: write IFC2X3, IFC4 or IFC4X3",
                self.version
            )));
        }
        if !(self.area_thickness.is_finite()
            && self.area_thickness > 0.0
            && self.area_thickness <= 1.0e6)
        {
            return Err(IfcError::Option(
                "area_thickness must be a positive number of mm".into(),
            ));
        }
        if let Some(t) = &self.time_stamp
            && !valid_time_stamp(t)
        {
            return Err(IfcError::Option(
                "time_stamp must look like 2026-10-10T12:00:00".into(),
            ));
        }
        Ok(())
    }
}

impl IfcOptions {
    /// The schema that the version names. IFC4 for a bad name (`check` refuses those).
    pub fn schema(&self) -> Schema {
        Schema::from_name(&self.version).unwrap_or_default()
    }
}

fn valid_time_stamp(t: &str) -> bool {
    let b = t.as_bytes();
    b.len() == 19
        && b.iter().enumerate().all(|(i, c)| match i {
            4 | 7 => *c == b'-',
            10 => *c == b'T',
            13 | 16 => *c == b':',
            _ => c.is_ascii_digit(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_and_json() {
        assert_eq!(
            IfcOptions::from_json(&json!(null)).unwrap(),
            IfcOptions::default()
        );
        let o = IfcOptions::from_json(&json!({"classes_as_layers": false, "include_2d": true, "version": "ifc4", "time_stamp": "2026-10-10T12:00:00"})).unwrap();
        assert!(!o.classes_as_layers && o.include_2d);
        assert!(IfcOptions::default().classes_as_layers);
        assert!(!IfcOptions::default().include_2d);
        assert_eq!(IfcOptions::default().schema(), Schema::Ifc4);
        for (v, s) in [("IFC2x3", Schema::Ifc2x3), ("ifc4x3", Schema::Ifc4x3)] {
            let o = IfcOptions::from_json(&json!({"version": v})).unwrap();
            assert_eq!(o.schema(), s);
        }
    }

    #[test]
    fn bad_options_are_refused() {
        for bad in [
            json!({"version": "IFC5"}),
            json!({"area_thickness": 0}),
            json!({"area_thickness": -1}),
            json!({"time_stamp": "yesterday"}),
            json!({"time_stamp": "2026-10-10 12:00:00"}),
            json!({"colour": 1}),
            json!({"include_2d": "yes"}),
        ] {
            assert!(
                matches!(IfcOptions::from_json(&bad), Err(IfcError::Option(_))),
                "{bad}"
            );
        }
    }
}
