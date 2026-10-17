//! The report of an import or export: counts, and what was skipped and why.

use std::collections::BTreeMap;

use serde::Serialize;

/// Entities of one kind that were skipped for one reason.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Skipped {
    pub entity: String,
    pub reason: String,
    pub count: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Report {
    pub format: String,
    /// Objects written or created, by shape kind.
    pub objects: BTreeMap<String, usize>,
    pub skipped: Vec<Skipped>,
    pub layers_created: Vec<String>,
    pub classes_created: Vec<String>,
    /// Remarks for the user, for example about an unknown CRS or a cut-off file.
    pub notes: Vec<String>,
    /// Problems in what was written that the person must look at, for example a unit pattern
    /// with a hole (units missing). Left out when there are none.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    /// True if the import set or used a georeference.
    pub georeferenced: bool,
    /// DXF and DWG: the drawing and revision that the objects carry in `data.import` (IOP-22).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drawing: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    /// DXF and DWG with source tracking: which update data the import kept. `auto` (the summaries
    /// of the entities are in the document), `link` (a link to the original file is on the layer
    /// source) or `none`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
}

impl Report {
    pub fn new(format: &str) -> Report {
        Report {
            format: format.into(),
            ..Report::default()
        }
    }

    /// Counts an object of this shape kind.
    pub fn count(&mut self, kind: &str) {
        *self.objects.entry(kind.to_string()).or_default() += 1;
    }

    pub fn total(&self) -> usize {
        self.objects.values().sum()
    }

    pub fn skip(&mut self, entity: &str, reason: &str) {
        match self
            .skipped
            .iter_mut()
            .find(|s| s.entity == entity && s.reason == reason)
        {
            Some(s) => s.count += 1,
            None => self.skipped.push(Skipped {
                entity: entity.into(),
                reason: reason.into(),
                count: 1,
            }),
        }
    }

    pub fn skipped_total(&self) -> usize {
        self.skipped.iter().map(|s| s.count).sum()
    }

    pub fn warn(&mut self, text: &str) {
        if !self.warnings.iter().any(|n| n == text) {
            self.warnings.push(text.into());
        }
    }

    pub fn note(&mut self, text: &str) {
        if !self.notes.iter().any(|n| n == text) {
            self.notes.push(text.into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_and_skips() {
        let mut r = Report::new("dxf");
        r.count("line");
        r.count("line");
        r.count("circle");
        r.skip("DIMENSION", "unsupported");
        r.skip("DIMENSION", "unsupported");
        r.skip("DIMENSION", "other");
        r.note("a");
        r.note("a");
        assert_eq!(r.total(), 3);
        assert_eq!(r.skipped_total(), 3);
        assert_eq!(r.skipped.len(), 2);
        assert_eq!(r.skipped[0].count, 2);
        assert_eq!(r.notes.len(), 1);
        assert_eq!(serde_json::to_value(&r).unwrap()["objects"]["line"], 2);
    }
}
