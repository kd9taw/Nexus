//! A file Nexus cannot read is KEPT, never saved over.
//!
//! Every store this serves used to read a file it could not parse as an EMPTY one
//! (`from_str(..).ok()`, `read_to_string(..).unwrap_or_default()`, `if let Ok(..)`), and its
//! next save then wrote the empty store over the file: a journal torn by a crash, a hand edit,
//! or a file from a newer Nexus carrying a value this one does not know cost every record in
//! it, with nothing said. Program's saved channel lists were the first to be fixed; this is
//! that fix, shared by the stores that hold contacts, messages and records.
//!
//! The rule, for a file that is THERE and cannot be read. A file that is absent is a first
//! run, and a field this build does not know is not a failure (serde ignores it), so a file
//! from a newer Nexus that only adds fields still loads.
//! - It is renamed, untouched, to `<stem>.unreadable-YYYYMMDD-HHMMSS.<ext>` (UTC) beside it, or
//!   `-2`, `-3` … when that name is taken, so a second never replaces the first (a rename onto
//!   an existing file replaces it, on every platform). It is never deleted.
//! - The store starts fresh, and its next save makes a new file.
//! - If the rename fails, the file stays where it is and [`refuses`] answers true for its path
//!   for the rest of the run: every writer of that store asks it first, and neither writes
//!   over the file nor removes it.
//! - Either way it is recorded for the screen ([`kept`], which the snapshot carries) and in the
//!   diagnostic log.
//!
//! Process-wide on purpose, like the file it guards: a store's reader and its writers are in
//! different places (the settings are read before the Engine exists, a journal is written on
//! its own thread), and a path is the one thing they all share.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

/// A file this run could not read, and where it is now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Kept {
    /// Which store it is, as a token the screen has words for (`"pendingQso"`, …).
    pub store: &'static str,
    /// Where the file is now: the name it was moved aside to, or its own path when it could
    /// not be moved.
    pub path: PathBuf,
    /// The rename failed, so the file is where it was, and [`refuses`] now says so for it.
    pub kept_in_place: bool,
}

/// Every file kept this run, oldest first.
static KEPT: Mutex<Vec<Kept>> = Mutex::new(Vec::new());

fn kept_lock() -> MutexGuard<'static, Vec<Kept>> {
    KEPT.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A new name beside `path` for a file that could not be read:
/// `<stem>.unreadable-YYYYMMDD-HHMMSS.<ext>` (UTC), numbered on (`-2` … `-100`) when that name
/// is taken. `None` when every name is taken, which leaves the file where it is.
pub fn aside_path(path: &Path, now_unix: i64) -> Option<PathBuf> {
    let (y, mo, d, h, mi, s) = crate::logbook::datetime_utc(now_unix.max(0) as u64);
    let dir = path.parent()?;
    let stem = path.file_stem()?.to_string_lossy();
    let ext = path.extension().map(|e| e.to_string_lossy());
    let stamp = format!("{y:04}{mo:02}{d:02}-{h:02}{mi:02}{s:02}");
    (1..=100)
        .map(|n| {
            let numbered = match n {
                1 => format!("{stem}.unreadable-{stamp}"),
                n => format!("{stem}.unreadable-{stamp}-{n}"),
            };
            dir.join(match &ext {
                Some(ext) => format!("{numbered}.{ext}"),
                None => numbered,
            })
        })
        .find(|candidate| !candidate.exists())
}

/// Keep the file at `path`, which `store` could not read (`why`): renamed aside, or, when that
/// fails, left where it is with every later write to it refused. Recorded for the screen and
/// the diagnostic log either way.
pub fn keep_aside(store: &'static str, path: &Path, now_unix: i64, why: &str) -> Kept {
    let moved = aside_path(path, now_unix).filter(|aside| std::fs::rename(path, aside).is_ok());
    let said = format!(
        "{} could not be read ({why}); {}",
        path.display(),
        match &moved {
            Some(aside) => format!("kept as {}", aside.display()),
            None => "could not move it aside, so nothing writes over it this run".to_string(),
        }
    );
    // Both: the diagnostic log is inert until it is opened, and the settings are read first.
    eprintln!("tempo: {said}");
    crate::applog::warn("kept_aside", &said);
    let kept = Kept {
        store,
        kept_in_place: moved.is_none(),
        path: moved.unwrap_or_else(|| path.to_path_buf()),
    };
    kept_lock().push(kept.clone());
    kept
}

/// Read the store at `path` with `parse`. `None` when there is no file (a first run) and when
/// there is one `parse` rejects or that cannot be read at all, which is then kept
/// ([`keep_aside`]) rather than left for the store's next save to write over.
pub fn read_or_keep<T>(
    store: &'static str,
    path: &Path,
    now_unix: i64,
    parse: impl FnOnce(&str) -> Result<T, String>,
) -> Option<T> {
    let why = match std::fs::read_to_string(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => e.to_string(),
        Ok(text) => match parse(&text) {
            Ok(value) => return Some(value),
            Err(why) => why,
        },
    };
    keep_aside(store, path, now_unix, &why);
    None
}

/// Every file kept this run, oldest first: what the screen says.
pub fn kept() -> Vec<Kept> {
    kept_lock().clone()
}

/// Whether `path` is a file this run could not read and could not move aside. Its store's
/// writers must neither write over it nor remove it.
pub fn refuses(path: &Path) -> bool {
    kept_lock()
        .iter()
        .any(|k| k.kept_in_place && k.path == path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-09-30 14:22:33 UTC.
    const NOW: i64 = 1_790_778_153;

    fn scratch(label: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("nexus-keep-aside-{label}-{nanos}"));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut found: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
            .collect();
        found.sort();
        found
    }

    #[test]
    fn a_kept_file_gets_a_dated_name_that_never_replaces_another() {
        let dir = scratch("names");
        let path = dir.join("pending_qso.json");
        assert_eq!(
            aside_path(&path, NOW),
            Some(dir.join("pending_qso.unreadable-20260930-142233.json"))
        );
        std::fs::write(
            dir.join("pending_qso.unreadable-20260930-142233.json"),
            b"a",
        )
        .unwrap();
        assert_eq!(
            aside_path(&path, NOW),
            Some(dir.join("pending_qso.unreadable-20260930-142233-2.json")),
            "a name that is taken is never reused"
        );
        assert_eq!(
            aside_path(&dir.join("journal"), NOW),
            Some(dir.join("journal.unreadable-20260930-142233")),
            "a file with no extension keeps having none"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_file_that_cannot_be_read_is_moved_aside_untouched_and_recorded() {
        let dir = scratch("moved");
        let path = dir.join("pending_msgs.json");
        std::fs::write(&path, b"{ torn").unwrap();
        let kept = read_or_keep("pendingMsgs", &path, NOW, |t| {
            serde_json::from_str::<Vec<u32>>(t).map_err(|e| e.to_string())
        });
        assert_eq!(kept, None, "nothing is read out of it");
        let aside = dir.join("pending_msgs.unreadable-20260930-142233.json");
        assert_eq!(
            names(&dir),
            vec!["pending_msgs.unreadable-20260930-142233.json"]
        );
        assert_eq!(std::fs::read(&aside).unwrap(), b"{ torn", "byte for byte");
        assert!(
            kept_for(&dir).contains(&Kept {
                store: "pendingMsgs",
                path: aside.clone(),
                kept_in_place: false,
            }),
            "recorded for the screen"
        );
        assert!(!refuses(&path), "a new file may be written in its place");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_readable_file_and_no_file_are_left_alone_and_unrecorded() {
        let dir = scratch("alone");
        let path = dir.join("js8_station.json");
        assert_eq!(
            read_or_keep("js8Inbox", &path, NOW, |t| Ok(t.len())),
            None,
            "no file is a first run"
        );
        std::fs::write(&path, b"[1,2]").unwrap();
        assert_eq!(
            read_or_keep("js8Inbox", &path, NOW, |t| {
                serde_json::from_str::<Vec<u32>>(t).map_err(|e| e.to_string())
            }),
            Some(vec![1, 2])
        );
        assert_eq!(names(&dir), vec!["js8_station.json"]);
        assert!(kept_for(&dir).is_empty(), "nothing to say");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_file_that_cannot_be_moved_is_left_in_place_and_refused_to_writers() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("stuck");
        let path = dir.join("conversations.json");
        std::fs::write(&path, b"{ torn").unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        // CONTROL: a user the mode does not bind (root) could rename anyway, so the premise would
        // not hold. Say so instead of passing on nothing.
        if std::fs::write(dir.join("probe"), b"").is_ok() {
            let _ = std::fs::remove_file(dir.join("probe"));
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
            std::fs::remove_dir_all(&dir).unwrap();
            eprintln!("skipped: this user can write a read-only folder, so no rename fails here");
            return;
        }
        let kept = keep_aside("conversations", &path, NOW, "torn");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            kept,
            Kept {
                store: "conversations",
                path: path.clone(),
                kept_in_place: true,
            }
        );
        assert!(refuses(&path), "every writer of it must now refuse");
        assert_eq!(std::fs::read(&path).unwrap(), b"{ torn", "untouched");
        assert!(!refuses(&dir.join("other.json")), "only that file");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The kept files under `dir` — the list is the whole process's, and tests run in parallel.
    fn kept_for(dir: &Path) -> Vec<Kept> {
        kept()
            .into_iter()
            .filter(|k| k.path.starts_with(dir))
            .collect()
    }
}
