//! Script resources (SCR-03, lite): the text of a JavaScript script that is stored in the
//! document (SCR-08). The core only keeps the text and its SHA-256; `kadakas-api` runs it.
//!
//! A script from a document runs only after the person trusted its exact text (the SHA-256, as for
//! plug-ins). A script that an agent sends in `script.eval` is not a resource and needs no trust:
//! it is the agent's own text, and it can do no more than the API allows.

use serde::{Deserialize, Serialize};

/// The most bytes of the text of a script.
pub const MAX_CODE_BYTES: usize = 256 * 1024;

/// The language of a script resource. Only one exists now.
pub const LANGUAGE: &str = "javascript";

fn language() -> String {
    LANGUAGE.to_string()
}

/// A script in the document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScriptResource {
    /// `javascript`.
    pub language: String,
    /// The text: the body of a function. For a generator (PAT-09) it gets `args` and returns the
    /// unit shapes.
    pub code: String,
    /// What the script is for, for the person who reads the list.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
}

impl Default for ScriptResource {
    fn default() -> Self {
        ScriptResource {
            language: language(),
            code: String::new(),
            description: String::new(),
        }
    }
}

impl ScriptResource {
    pub fn new(code: impl Into<String>) -> Self {
        ScriptResource {
            code: code.into(),
            ..Default::default()
        }
    }

    /// The SHA-256 of the text, as 64 hex digits. This is what trust is bound to.
    pub fn sha256(&self) -> String {
        crate::blob::text_hash(&self.code)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.language != LANGUAGE {
            return Err(format!(
                "the language of a script must be '{LANGUAGE}', not '{}'",
                self.language
            ));
        }
        if self.code.len() > MAX_CODE_BYTES {
            return Err(format!(
                "a script has at most {MAX_CODE_BYTES} bytes, this one has {}",
                self.code.len()
            ));
        }
        Ok(())
    }
}
