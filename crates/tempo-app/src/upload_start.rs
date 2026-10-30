//! Where QRZ Logbook's, ClubLog's and eQSL's automatic uploads start ([`UploadStart`]), kept in
//! ONE file beside the shared log, in the data folder every window reads the log from.
//!
//! A start point is a fact about the log ("the contacts in this log before then were not to be sent
//! there"), and the log and the logins are shared by every window: the base window, each radio's
//! `--profile r<id>` window and a hand-named profile. Kept in one window's own settings, it was
//! unknown to the others, and saving the stored login in another window sent the old log the
//! operator had declined. So no settings file holds it, and a factory reset or a settings restore,
//! which replace one window's settings, cannot touch it.
//!
//! - **Nothing is created until a start point is set.** A station that never answered "Only
//!   contacts made from now on" has no file here, and reading one that is not there writes nothing.
//! - **Two windows can save at once.** A change is read, made and written holding an OS lock on
//!   [`LOCK_NAME`] (released by the OS if the process dies), and written whole: a temporary file,
//!   flushed to disk, renamed over the old one. A reader without the lock sees the old file or the
//!   new one, never half of either.

use crate::station::UploadStart;
use std::io::Write as _;
use std::path::Path;

/// The file, in the shared data folder beside the log.
pub const FILE_NAME: &str = "upload-start.json";

/// What a change holds while it reads, makes and writes [`FILE_NAME`]: the file itself is
/// replaced by a rename, so the lock cannot be on it.
pub const LOCK_NAME: &str = "upload-start.lock";

/// The start points kept in `dir`. No file is no start point anywhere (every contact not yet sent
/// is owed, as on every station before this question existed). A file that cannot be read or
/// understood is an error, never "no start point": that answer would send the log the operator
/// declined.
pub fn read(dir: &Path) -> Result<UploadStart, String> {
    match std::fs::read(dir.join(FILE_NAME)) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|e| format!("{FILE_NAME} could not be understood: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(UploadStart::default()),
        Err(e) => Err(format!("{FILE_NAME} could not be read: {e}")),
    }
}

/// Change the start points kept in `dir` with `edit`, which may run more than once and must give
/// the same answer each time. Returns them as they were and as they are now. Writes nothing, and
/// creates nothing, when `edit` changes nothing; otherwise the change is made holding the lock,
/// on the file as another window may just have left it.
pub fn update(
    dir: &Path,
    edit: impl Fn(&mut UploadStart),
) -> Result<(UploadStart, UploadStart), String> {
    let mut seen = read(dir)?;
    let before = seen;
    edit(&mut seen);
    if seen == before {
        return Ok((before, seen));
    }
    std::fs::create_dir_all(dir)
        .map_err(|e| format!("{} could not be made: {e}", dir.display()))?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join(LOCK_NAME))
        .map_err(|e| format!("{LOCK_NAME} could not be opened: {e}"))?;
    match lock.lock() {
        Ok(()) => {}
        // A folder whose filesystem has no locks (some network shares): the write is still whole,
        // and two windows answering at the same instant there is the one case left unordered.
        Err(e) if e.kind() == std::io::ErrorKind::Unsupported => {}
        Err(e) => return Err(format!("{LOCK_NAME} could not be locked: {e}")),
    }
    let before = read(dir)?;
    let mut after = before;
    edit(&mut after);
    if after != before {
        write_whole(dir, &after)?;
    }
    drop(lock);
    Ok((before, after))
}

/// Write `start` over [`FILE_NAME`] in one rename.
fn write_whole(dir: &Path, start: &UploadStart) -> Result<(), String> {
    let path = dir.join(FILE_NAME);
    // A name of this write's own, so no other write, in this process or another, shares it.
    static WRITES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = WRITES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = dir.join(format!("{FILE_NAME}.{}-{n}.tmp", std::process::id()));
    let json = serde_json::to_vec_pretty(start).map_err(|e| e.to_string())?;
    let written = std::fs::File::create(&tmp)
        .and_then(|mut f| {
            f.write_all(&json)?;
            f.sync_all()
        })
        .and_then(|()| std::fs::rename(&tmp, &path));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("{FILE_NAME} could not be written: {e}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::upload_legs;

    fn folder(name: &str) -> std::path::PathBuf {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "nexus-upload-start-{name}-{}-{:?}-{}",
            std::process::id(),
            std::time::SystemTime::now(),
            N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn listing(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// ⭐ A station with no start point anywhere: reading one, and every change that changes
    /// nothing ("Send them" with none set, a move of none), create no file at all.
    #[test]
    fn no_start_point_creates_nothing() {
        let dir = folder("none");
        std::fs::write(dir.join("log.adi"), "<EOH>\n").unwrap();
        assert_eq!(read(&dir), Ok(UploadStart::default()));
        let (before, after) = update(&dir, |s| s.answer(upload_legs::QRZ, true, 1_000)).unwrap();
        assert_eq!(
            (before, after),
            (UploadStart::default(), UploadStart::default())
        );
        update(&dir, |s| *s = s.keep_earliest(UploadStart::default())).unwrap();
        assert_eq!(listing(&dir), ["log.adi"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The file holds what was set, read back by value, and only the services that have one.
    #[test]
    fn a_start_point_is_kept_and_read_back() {
        let dir = folder("kept");
        let (before, after) = update(&dir, |s| {
            s.answer(upload_legs::CLUBLOG, false, 1_790_000_000)
        })
        .unwrap();
        assert_eq!(before, UploadStart::default());
        let want = UploadStart {
            clublog: Some(1_790_000_000),
            ..UploadStart::default()
        };
        assert_eq!(after, want);
        assert_eq!(read(&dir), Ok(want));
        assert_eq!(
            std::fs::read_to_string(dir.join(FILE_NAME)).unwrap(),
            "{\n  \"clublog\": 1790000000\n}"
        );
        assert_eq!(listing(&dir), [FILE_NAME, LOCK_NAME]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A file that cannot be understood is an error, never "no start point", and a change is
    /// refused rather than written over it.
    #[test]
    fn a_file_that_cannot_be_understood_is_an_error() {
        let dir = folder("bad");
        std::fs::write(dir.join(FILE_NAME), "{ not json").unwrap();
        assert!(read(&dir).is_err());
        assert!(update(&dir, |s| s.answer(upload_legs::QRZ, false, 5)).is_err());
        assert_eq!(
            std::fs::read_to_string(dir.join(FILE_NAME)).unwrap(),
            "{ not json"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ⛔ Two windows answering at once, each for its own service, from many threads: every
    /// answer is kept, because each change is made on the file as the last one left it.
    #[test]
    fn answers_made_at_once_are_all_kept() {
        let dir = folder("race");
        let threads: Vec<_> = (0..24u64)
            .map(|i| {
                let dir = dir.clone();
                std::thread::spawn(move || {
                    let leg = [upload_legs::QRZ, upload_legs::CLUBLOG, upload_legs::EQSL]
                        [(i % 3) as usize];
                    update(&dir, |s| s.answer(leg, false, 1_000 + i)).unwrap();
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        assert_eq!(
            read(&dir),
            Ok(UploadStart {
                qrz: Some(1_000),
                clublog: Some(1_001),
                eqsl: Some(1_002),
            }),
            "each service keeps its earliest answer"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
