//! One error type for every street-map operation, shaped for the UI.
//!
//! The UI decides what to offer from [`ErrorKind`] alone: Resume after `paused` or `cancelled`,
//! a fresh download after `buildGone`, a smaller area after `diskSpace`. `message` is English
//! detail for the diagnostic log and a fallback line; the operator-facing sentence for each kind
//! belongs to the UI's translated catalogs, never to this crate.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorKind {
    /// The area itself is unusable: a coordinate out of range, a size that is not one of the
    /// offered squares, or a square wholly beyond the map's ±85° edge.
    InvalidArea,
    /// A request the webview should never make: a read over the cap, a malformed pack id, an
    /// asset path outside the strict form.
    BadRequest,
    /// No pack with that id is on this computer.
    UnknownPack,
    /// No such font or icon file in the installed assets.
    NotFound,
    /// Another download is running, or the pack is the one being downloaded.
    Busy,
    /// Less free space than twice the pack's size.
    DiskSpace,
    /// The host could not be reached, or kept failing while the area was being sized.
    Network,
    /// The host answered with an error status other than the ones below.
    Server,
    /// A download stopped after its retries ran out. It resumes where it stopped.
    Paused,
    /// The operator cancelled. It resumes where it stopped.
    Cancelled,
    /// The map build this download reads from is no longer on the host (404). Its partial
    /// download is removed; a new download uses the current build.
    BuildGone,
    /// The build changed size under its name, which an immutable build never does. Treated
    /// like `BuildGone`.
    BuildChanged,
    /// Something between Nexus and the host ignored the byte range and sent the whole file. The
    /// job stops rather than download 138 GB; the partial download is kept.
    RangeIgnored,
    /// The hosted index file is malformed, or points somewhere other than the street-map host.
    InvalidManifest,
    /// A map file or the font/icon archive failed its structure, tile or SHA-256 check.
    InvalidArchive,
    /// A local file could not be read or written.
    Io,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StreetError {
    pub kind: ErrorKind,
    pub message: String,
}

impl StreetError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub(crate) fn archive(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::InvalidArchive, message)
    }

    pub(crate) fn io(what: &str, e: &std::io::Error) -> Self {
        Self::new(ErrorKind::Io, format!("{what}: {e}"))
    }
}

impl std::fmt::Display for StreetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)
    }
}

impl std::error::Error for StreetError {}
