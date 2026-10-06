//! The optional street map's downloadable packs.
//!
//! The street map is an opt-in base map at street level. Its data is OpenStreetMap, as one
//! Protomaps basemap build (a single PMTiles file of about 138 GB) copied to the street-map
//! host. The operator downloads only an area: a square 50, 100, 200 or 400 km across around the
//! station or the map centre, at "all streets" (tile zoom 14) or "main roads" (zoom 12). This
//! crate does everything about those packs except draw them:
//!
//! - [`area`]: the square, the tiles that touch it at each zoom, and their tile ids.
//! - [`plan`]: the exact size, read from only the directories the area needs, and the pack's
//!   layout and requests, fixed before any tile data moves.
//! - [`job`]: the download: merged range reads about four at a time, a journal so a killed job
//!   resumes, retry with backoff and then a pause, and the checks before the file is named.
//! - [`verify`]: structure, directories and every tile of a finished pack.
//! - [`store`]: the maps folder, `packs.json`, and the two reads the webview makes.
//! - [`assets`]: the fonts and icons, fetched once with the first pack.
//! - [`manifest`]: the host's `streetmaps.json`.
//! - [`pmtiles`] and [`http`]: the file format and the byte ranges underneath.
//! - [`service`]: all of it behind the few calls the Tauri commands make.
//!
//! Nothing here touches the network unless a caller asks it to size or download an area. The
//! data is ODbL-licensed and separate from the program: it is never committed or bundled, and
//! the tests build synthetic map files while they run (see NOTICE, "Redistributed OpenStreetMap
//! data (street maps)").

pub mod area;
pub mod assets;
mod error;
pub mod http;
pub mod job;
pub mod manifest;
pub mod plan;
pub mod pmtiles;
pub mod service;
pub mod store;
pub mod verify;

#[cfg(test)]
mod tests_e2e;
#[cfg(test)]
mod testutil;

use std::time::Duration;

pub use area::{Detail, StreetArea};
pub use error::{ErrorKind, StreetError};
pub use job::StreetProgress;
pub use service::{StreetInstalled, StreetMaps, StreetSize, StreetUnfinished, StreetUpdate};
pub use store::StreetPack;

/// The host that serves the street map's index, the planet copy, and the fonts and icons.
///
/// An R2 bucket behind this custom domain, which .github/workflows/street-maps.yml fills and
/// prunes. This is the one place its name lives on the Rust side.
pub const STREET_MAP_HOST: &str = "maps.hamradiotools.io";

/// `https://` and [`STREET_MAP_HOST`]: where the index is fetched from, and the only origin an
/// index may point downloads at.
pub fn default_origin() -> String {
    format!("https://{STREET_MAP_HOST}")
}

/// Byte runs closer than this are fetched in one request and the gap is discarded: about half
/// the requests for 1.5–3.8% more bytes, as measured on real areas.
pub const MERGE_GAP: u64 = 64 << 10;
/// No request is longer than this, so a dropped connection costs at most this much again.
pub const MAX_REQUEST: u64 = 8 << 20;

/// How downloads are paced. The defaults are the product's; tests shrink them.
#[derive(Debug, Clone)]
pub struct Options {
    /// Requests in flight at once.
    pub parallel: usize,
    pub retry: http::RetryPolicy,
    /// The least time between two progress messages.
    pub progress_every: Duration,
    pub merge_gap: u64,
    pub max_request: u64,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            parallel: 4,
            retry: http::RetryPolicy::default(),
            progress_every: Duration::from_millis(250),
            merge_gap: MERGE_GAP,
            max_request: MAX_REQUEST,
        }
    }
}
