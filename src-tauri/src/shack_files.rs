//! The station's own files, fetched by a computer that drives it from afar: the QSO recordings,
//! ALL.TXT and the received SSTV pictures. At the shack those three are reached with Reveal, which
//! opens the shack's file manager on the shack's own screen; from afar that is refused, and these
//! are what the other computer has instead. It lists a folder, reads a file in pieces, and saves
//! the pieces into its own Downloads folder. The SSTV gallery's thumbnails and viewer read their
//! picture here too, because the path the gallery names is a path on the shack's disk.
//!
//! ⛔ A name or path comes from the other computer, so each read is confined to its own folder:
//! the file must resolve, after every `..` and link is followed, to a file directly in that folder,
//! and be one of the kinds the folder is listed for. Nothing else in those folders crosses (the
//! gallery's index, or the diagnostic log and crash report that sit beside ALL.TXT), and no file
//! anywhere else does.

use std::path::{Path, PathBuf};

/// The most one read carries. Each piece is one answer on the command link, so a big ALL.TXT
/// crosses as many modest answers rather than one that holds the link up.
pub(crate) const PIECE_BYTES: usize = 1024 * 1024;

/// The largest gallery picture read whole. The biggest picture a received mode makes is about
/// 1.5 MB as a BMP; anything far past that in the gallery folder is not a received picture.
pub(crate) const IMAGE_BYTES: u64 = 8 * 1024 * 1024;

/// The three folders a computer driving the station may fetch from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Folder {
    Recordings,
    AllTxt,
    Sstv,
}

impl Folder {
    fn dir(self) -> PathBuf {
        match self {
            Folder::Recordings => crate::recordings_dir(),
            Folder::AllTxt => crate::all_txt_path()
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from(".")),
            Folder::Sstv => crate::sstv_gallery_dir(),
        }
    }

    /// Whether a file of this name is one this folder is fetched for.
    fn admits(self, name: &str) -> bool {
        let lower = name.to_ascii_lowercase();
        match self {
            Folder::Recordings => lower.ends_with(".wav"),
            Folder::AllTxt => name == "ALL.TXT",
            Folder::Sstv => lower.ends_with(".png") || lower.ends_with(".bmp"),
        }
    }
}

/// One file in a folder, as the list shows it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ShackFile {
    pub name: String,
    pub bytes: u64,
    /// Last written, in milliseconds since 1970 (0 when the disk does not say).
    pub modified_ms: u64,
}

/// One piece of a file: its bytes as base64, where they start, and the file's whole size as it
/// was read.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Piece {
    pub data: String,
    pub offset: u64,
    pub size: u64,
}

/// `candidate` (a bare name, or a path the gallery named) as the file it is inside `dir`, or why
/// not. Both sides are canonicalised, so a `..` or a link that leads out lands outside and is
/// refused, and the file must sit directly in the folder and be a kind the folder admits.
fn confine(folder: Folder, dir: &Path, candidate: &Path) -> Result<PathBuf, String> {
    let refused = || "That file is not one the station shares from this folder".to_string();
    let canon_dir = dir
        .canonicalize()
        .map_err(|_| "The station has no such folder yet".to_string())?;
    let canon = dir
        .join(candidate)
        .canonicalize()
        .map_err(|_| "The station has no such file".to_string())?;
    if canon.parent() != Some(canon_dir.as_path()) || !canon.is_file() {
        return Err(refused());
    }
    match canon.file_name().and_then(|n| n.to_str()) {
        Some(name) if folder.admits(name) => Ok(canon),
        _ => Err(refused()),
    }
}

/// A bare file name, never a path: what the list gave and a read names.
fn bare(name: &str) -> Result<&Path, String> {
    let path = Path::new(name);
    match path.file_name() {
        Some(own) if own == path.as_os_str() => Ok(path),
        _ => Err("That file is not one the station shares from this folder".to_string()),
    }
}

pub(crate) fn list_in(folder: Folder, dir: &Path) -> Vec<ShackFile> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<ShackFile> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let path = confine(folder, dir, Path::new(&name)).ok()?;
            let meta = std::fs::metadata(path).ok()?;
            let modified_ms = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            Some(ShackFile {
                name,
                bytes: meta.len(),
                modified_ms,
            })
        })
        .collect();
    // Newest first: the recording just made is the one wanted.
    files.sort_by(|a, b| b.modified_ms.cmp(&a.modified_ms).then(a.name.cmp(&b.name)));
    files
}

pub(crate) fn read_in(
    folder: Folder,
    dir: &Path,
    name: &str,
    offset: u64,
) -> Result<Piece, String> {
    use std::io::{Read, Seek, SeekFrom};
    let path = confine(folder, dir, bare(name)?)?;
    let mut file = std::fs::File::open(&path).map_err(|e| format!("Could not read {name}: {e}"))?;
    let size = file.metadata().map_err(|e| e.to_string())?.len();
    let mut data = Vec::new();
    if offset < size {
        file.seek(SeekFrom::Start(offset))
            .map_err(|e| e.to_string())?;
        file.take(PIECE_BYTES as u64)
            .read_to_end(&mut data)
            .map_err(|e| format!("Could not read {name}: {e}"))?;
    }
    Ok(Piece {
        data: crate::b64_encode(&data),
        offset,
        size,
    })
}

pub(crate) fn gallery_image_in(dir: &Path, path: &str) -> Result<String, String> {
    let file = confine(Folder::Sstv, dir, Path::new(path))?;
    let size = std::fs::metadata(&file).map_err(|e| e.to_string())?.len();
    if size > IMAGE_BYTES {
        return Err("That picture is too large to fetch".to_string());
    }
    std::fs::read(&file)
        .map(|bytes| crate::b64_encode(&bytes))
        .map_err(|e| format!("Could not read the picture: {e}"))
}

/// Where a saved piece went: the file's name in this computer's Downloads folder, which the later
/// pieces of the same save name, and its full path, which the operator is shown.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Saved {
    pub name: String,
    pub path: String,
}

/// The most names a save tries before it gives up: `name`, then `name (1)` up to this.
const FREE_NAMES: u32 = 1000;

/// A new file in `dir`, never one that is there already: `name` itself when it is free, else
/// `stem (1).ext`, `stem (2).ext` and on, as a browser names a download whose name is taken.
/// `create_new` makes "not there" and "now this save's" one step, so a file that appears
/// meanwhile is skipped, never opened over.
fn create_unused(dir: &Path, name: &str) -> Result<(String, std::fs::File), String> {
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, Some(ext)),
        _ => (name, None),
    };
    for n in 0..=FREE_NAMES {
        let candidate = match (n, ext) {
            (0, _) => name.to_string(),
            (_, Some(ext)) => format!("{stem} ({n}).{ext}"),
            (_, None) => format!("{stem} ({n})"),
        };
        let path = dir.join(&candidate);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => return Ok((candidate, file)),
            // Taken: a file, or a folder of that name (which Windows refuses as access denied).
            Err(_) if path.symlink_metadata().is_ok() => continue,
            Err(e) => return Err(format!("Could not write {}: {e}", path.display())),
        }
    }
    Err(format!(
        "Could not save {name}: {} already holds it and {FREE_NAMES} numbered copies",
        dir.display()
    ))
}

/// Whether a save writes a file of this name: only the kinds the station shares (a recording, an
/// SSTV picture, ALL.TXT), under the station's own name or the numbered one [`create_unused`]
/// gave it here (`ALL (1).TXT`), which the later pieces of that save name.
fn saves(name: &str) -> bool {
    let shared = |name: &str| {
        [Folder::Recordings, Folder::AllTxt, Folder::Sstv]
            .into_iter()
            .any(|folder| folder.admits(name))
    };
    shared(name) || unnumbered(name).is_some_and(|own| shared(&own))
}

/// The name [`create_unused`] numbered into `name` (`ALL.TXT` for `ALL (1).TXT`), or None when
/// `name` carries no number. The extension splits off as it does there.
fn unnumbered(name: &str) -> Option<String> {
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, Some(ext)),
        _ => (name, None),
    };
    let (own, n) = stem.strip_suffix(')')?.rsplit_once(" (")?;
    if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(ext.map_or_else(|| own.to_string(), |ext| format!("{own}.{ext}")))
}

/// Writes one piece of a fetched file into `dir` (this computer's Downloads folder). The piece at
/// offset 0 starts a new file under the bare `filename`, or under the next free name when a file
/// of that name is there already, which is never written over. Each later piece names the file
/// the first one started ([`Saved::name`]) and must start where that file now ends, so a piece
/// sent twice or out of order is refused rather than written twice. A name of a kind the station
/// does not share ([`saves`]) is refused before anything is written.
pub(crate) fn save_piece_in(
    dir: &Path,
    filename: &str,
    base64: &str,
    offset: u64,
) -> Result<Saved, String> {
    use std::io::Write;
    let name = bare(filename)
        .ok()
        .and_then(Path::to_str)
        .filter(|n| !n.is_empty())
        .ok_or("Invalid file name")?;
    if !saves(name) {
        return Err(format!(
            "{name} is not a recording, an SSTV picture or ALL.TXT"
        ));
    }
    let bytes = crate::b64_decode(base64)?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let (name, mut file) = if offset == 0 {
        create_unused(dir, name)?
    } else {
        let path = dir.join(name);
        let held = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        if held != offset {
            return Err(format!(
                "The save of {name} lost its place: it holds {held} bytes, not {offset}"
            ));
        }
        let file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .map_err(|e| format!("Could not write {}: {e}", path.display()))?;
        (name.to_string(), file)
    };
    let path = dir.join(&name);
    file.write_all(&bytes)
        .map_err(|e| format!("Could not write {}: {e}", path.display()))?;
    Ok(Saved {
        name,
        path: path.display().to_string(),
    })
}

/// The files in one of the three folders, newest first.
#[tauri::command(async)]
pub(crate) fn shack_file_list(folder: Folder) -> Vec<ShackFile> {
    list_in(folder, &folder.dir())
}

/// One piece of one file in one of the three folders, from `offset`, at most [`PIECE_BYTES`].
#[tauri::command(async)]
pub(crate) fn shack_file_read(folder: Folder, name: String, offset: u64) -> Result<Piece, String> {
    read_in(folder, &folder.dir(), &name, offset)
}

/// One received SSTV picture, whole, by the path the gallery names it by.
#[tauri::command(async)]
pub(crate) fn shack_gallery_image(path: String) -> Result<String, String> {
    gallery_image_in(&Folder::Sstv.dir(), &path)
}

/// One piece of a fetched file, written into THIS computer's Downloads folder.
#[tauri::command(async)]
pub(crate) fn shack_file_save(
    app: tauri::AppHandle,
    filename: String,
    base64: String,
    offset: u64,
) -> Result<Saved, String> {
    use tauri::Manager;
    let dir = app
        .path()
        .download_dir()
        .or_else(|_| app.path().home_dir())
        .map_err(|e| format!("Could not locate your Downloads folder: {e}"))?;
    save_piece_in(&dir, &filename, &base64, offset)
}

#[cfg(test)]
mod tests;
