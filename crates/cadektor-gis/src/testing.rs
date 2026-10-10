//! Helpers for unit tests: a fake server that answers from a table.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::http::{Fetch, Response};
use crate::raster::Rgba;
use crate::{GisError, Result};

#[derive(Default)]
pub struct Fake {
    pub answers: HashMap<String, Response>,
    pub log: Mutex<Vec<String>>,
}

impl Fetch for Fake {
    fn get(&self, url: &str) -> Result<Response> {
        self.log.lock().unwrap().push(url.to_string());
        self.answers
            .get(url)
            .cloned()
            .ok_or_else(|| GisError::Http(format!("404 {url}")))
    }
}

pub fn solid(w: u32, h: u32, c: [u8; 4]) -> Rgba {
    Rgba {
        width: w,
        height: h,
        data: c
            .iter()
            .copied()
            .cycle()
            .take(w as usize * h as usize * 4)
            .collect(),
    }
}

/// A PNG answer of one color.
pub fn png(w: u32, h: u32, c: [u8; 4]) -> Response {
    Response {
        status: 200,
        content_type: "image/png".into(),
        body: solid(w, h, c).to_png(),
    }
}

/// A text answer.
pub fn text(body: &str, content_type: &str) -> Response {
    Response {
        status: 200,
        content_type: content_type.into(),
        body: body.as_bytes().to_vec(),
    }
}
