//! The one writer of the SSTV gallery's `gallery.json`.
//!
//! Two parties write the file: the SSTV decode thread (a finished picture, its FSK ID) and the
//! shell's gallery delete. Neither may write while holding the Engine lock. The radio loop takes
//! that lock every tick and Stop TX takes it too, and the gallery folder lives under Pictures,
//! which is often a OneDrive-backed or network-redirected folder where one write can take
//! seconds. Holding the lock across it would hold an FT over's unkey and Stop TX for as long.
//!
//! So each writer takes the gallery and its generation ([`Engine::sstv_gallery_snapshot`]) under
//! the Engine lock, lets go, and calls [`write_gallery_json`]. The generation decides which write
//! wins: a snapshot no newer than the last one written for that folder is dropped, so an older
//! copy that loses the race to a newer one can never land after it.
//!
//! ⛔ Lock order: this module's lock is only ever taken with the Engine lock released, and the
//! Engine lock is never taken while it is held. Neither is held while the other is taken.
//!
//! [`Engine::sstv_gallery_snapshot`]: crate::engine::Engine::sstv_gallery_snapshot

use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use crate::dto::SstvGalleryEntry;

/// The gallery metadata sidecar's filename, inside the gallery directory.
pub const GALLERY_JSON: &str = "gallery.json";

/// The newest generation written (or attempted) per gallery folder. Keyed by folder because the
/// generation is counted per Engine: one app has one Engine and one folder, but tests run many of
/// each in one process.
static LAST_WRITTEN: Mutex<Vec<(PathBuf, u64)>> = Mutex::new(Vec::new());

/// Write `entries`, the gallery at generation `gen`, to `dir`/`gallery.json`, unless a snapshot
/// at least as new has already been written there. Best-effort: a failed write is dropped, and
/// the next change writes again.
///
/// ⛔ Never call with the Engine lock held (see the module doc).
///
/// The scratch file is per PROCESS, never a fixed `gallery.json.tmp`, for two builds running
/// against one gallery: on a shared scratch name their writes land in the same file and the
/// rename can publish a mix of both, which loads as an empty gallery. With one each, the rename is
/// the only thing shared, and it is atomic. The `Settings::tmp_path` shape. Within the process,
/// this function's lock makes the writes take turns.
pub fn write_gallery_json(dir: &Path, gen: u64, entries: &[SstvGalleryEntry]) {
    #[cfg(any(test, feature = "test-util"))]
    if let Some(hook) = BEFORE_WRITE.with(|h| h.borrow_mut().take()) {
        hook();
    }
    let mut last = LAST_WRITTEN.lock().unwrap_or_else(PoisonError::into_inner);
    match last.iter_mut().find(|(d, _)| d == dir) {
        Some((_, written)) if gen <= *written => return,
        Some((_, written)) => *written = gen,
        None => last.push((dir.to_path_buf(), gen)),
    }
    let _ = std::fs::create_dir_all(dir);
    if let Ok(text) = serde_json::to_string(entries) {
        let path = dir.join(GALLERY_JSON);
        let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
        if std::fs::write(&tmp, text).is_ok() && std::fs::rename(&tmp, &path).is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
    }
}

#[cfg(any(test, feature = "test-util"))]
thread_local! {
    static BEFORE_WRITE: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
        const { std::cell::RefCell::new(None) };
}

/// FOR TESTS: run `hook` once, on this thread, at the start of its next [`write_gallery_json`]
/// call, before the generation check. A test lands a competing write there, or checks that the
/// caller let go of the Engine lock before writing.
#[cfg(any(test, feature = "test-util"))]
pub fn before_next_write(hook: impl FnOnce() + 'static) {
    BEFORE_WRITE.with(|h| *h.borrow_mut() = Some(Box::new(hook)));
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn scratch(label: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("nexus-sstv-gallery-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn entry(path: &str) -> SstvGalleryEntry {
        SstvGalleryEntry {
            path: path.into(),
            mode: "Scottie 1".into(),
            ..SstvGalleryEntry::default()
        }
    }

    fn on_disk(dir: &Path) -> Vec<String> {
        let text = std::fs::read_to_string(dir.join(GALLERY_JSON)).unwrap();
        serde_json::from_str::<Vec<SstvGalleryEntry>>(&text)
            .unwrap()
            .into_iter()
            .map(|e| e.path)
            .collect()
    }

    /// The writers take their snapshot under the Engine lock and write after letting go, so two
    /// can reach the file in either order. The older snapshot arriving second must leave the
    /// newer one in place; a newer one still lands.
    #[test]
    fn an_older_gallery_snapshot_never_overwrites_a_newer_one() {
        let dir = scratch("gen");
        write_gallery_json(&dir, 5, &[entry("newer.png")]);
        write_gallery_json(&dir, 4, &[entry("older.png")]);
        assert_eq!(
            on_disk(&dir),
            vec!["newer.png"],
            "an older snapshot is dropped"
        );
        write_gallery_json(&dir, 5, &[entry("same-gen.png")]);
        assert_eq!(on_disk(&dir), vec!["newer.png"], "so is one no newer");
        write_gallery_json(&dir, 6, &[entry("newest.png")]);
        assert_eq!(on_disk(&dir), vec!["newest.png"], "a newer snapshot lands");

        // Another folder keeps its own count: generation 1 there is its first write.
        let other = scratch("gen-other");
        write_gallery_json(&other, 1, &[entry("other.png")]);
        assert_eq!(on_disk(&other), vec!["other.png"]);
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&other);
    }

    /// Two Nexus builds can run against one gallery (a tester build beside the release). Each
    /// writes gallery.json through a scratch file and renames it into place; on one shared
    /// scratch name the two writes land in the same file and the rename can publish a mix of
    /// both, which is invalid JSON and loads as an empty gallery, losing every picture's
    /// frequency, line count and FSK ID. A write must leave a scratch file it did not create alone.
    #[test]
    fn a_gallery_write_leaves_another_writers_scratch_file_alone() {
        let dir = scratch("other-writer");
        // The fixed scratch name every earlier build writes through, mid-write.
        let theirs = dir.join("gallery.json.tmp");
        std::fs::write(&theirs, "[{\"path\":").unwrap();
        write_gallery_json(&dir, 1, &[]);
        assert_eq!(
            std::fs::read_to_string(&theirs).ok().as_deref(),
            Some("[{\"path\":"),
            "another writer's half-written scratch file must not be overwritten or moved"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join(GALLERY_JSON)).unwrap(),
            "[]",
            "and this write still landed"
        );
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|f| f.file_name().to_string_lossy().into_owned())
            .filter(|n| n != GALLERY_JSON && n != "gallery.json.tmp")
            .collect();
        assert_eq!(
            leftovers,
            Vec::<String>::new(),
            "no scratch file left behind"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
