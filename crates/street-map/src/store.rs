//! The maps folder: where packs live, the list of them, and the two reads the webview makes.
//!
//! `$LOCALDATA/Nexus/maps/` (Windows `%LOCALAPPDATA%\Nexus\maps`, macOS
//! `~/Library/Application Support/Nexus/maps`, Linux `~/.local/share/Nexus/maps`). Never the
//! shared data folder: that can be a NAS or a synced folder, and a 300 MB map syncing there
//! would be a surprise.
//!
//! ```text
//! street-<id>.pmtiles        a finished pack
//! street-<id>.pmtiles.part   a download in progress, at the pack's full size, filled by range
//! street-<id>.journal        which requests of that download are safely on disk
//! packs.json                 the finished packs
//! assets/                    glyphs, sprites, OFL.txt, ICONS-LICENSE.txt
//! README-maps.txt            the ODbL notice for the packs beside it
//! ```
//!
//! The webview never names a path. It names a pack by id, which must be `[a-z0-9-]` and becomes
//! `street-<id>.pmtiles` here, and an asset by a relative path that must pass
//! [`validate_asset_path`]. Each read is capped, so one call cannot pull a whole pack across
//! the IPC bridge.

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::area::{Detail, StreetArea};
use crate::error::{ErrorKind, StreetError};

/// The most one `street_map_read` returns.
pub const MAX_READ: u32 = 4 << 20;
/// The largest asset file served (glyph ranges and sprite sheets are far smaller).
pub const MAX_ASSET: u64 = 4 << 20;

/// A finished pack as the UI sees it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreetPack {
    pub id: String,
    /// Language-neutral: the centre's locator and the size, e.g. `DM79mr 200 km`.
    pub name: String,
    /// West, south, east, north. West > east when the square crosses the 180° meridian.
    pub bbox: [f64; 4],
    pub min_zoom: u8,
    pub max_zoom: u8,
    pub detail: Detail,
    pub bytes: u64,
    /// The source build's date, YYYY-MM-DD.
    pub data_date: String,
    pub sha256: String,
}

/// A pack as `packs.json` keeps it: what the UI sees plus what an update needs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackRecord {
    #[serde(flatten)]
    pub pack: StreetPack,
    pub build_id: String,
    pub source_url: String,
    /// The area as asked for, so an update can cut the same square from a newer build.
    pub area: StreetArea,
    pub created_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetsRecord {
    pub sha256: String,
    pub url: String,
    pub installed_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Registry {
    pub version: u32,
    #[serde(default)]
    pub packs: Vec<PackRecord>,
    #[serde(default)]
    pub assets: Option<AssetsRecord>,
}

impl Default for Registry {
    fn default() -> Self {
        Self {
            version: 1,
            packs: Vec::new(),
            assets: None,
        }
    }
}

/// The maps folder's paths. Nothing here touches the disk.
#[derive(Debug, Clone)]
pub struct MapsDir {
    root: PathBuf,
}

impl MapsDir {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn pack(&self, id: &str) -> PathBuf {
        self.root.join(format!("street-{id}.pmtiles"))
    }

    pub fn part(&self, id: &str) -> PathBuf {
        self.root.join(format!("street-{id}.pmtiles.part"))
    }

    pub fn journal(&self, id: &str) -> PathBuf {
        self.root.join(format!("street-{id}.journal"))
    }

    pub fn registry(&self) -> PathBuf {
        self.root.join("packs.json")
    }

    pub fn assets(&self) -> PathBuf {
        self.root.join("assets")
    }

    pub fn readme(&self) -> PathBuf {
        self.root.join("README-maps.txt")
    }
}

pub(crate) fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// `<build>-<detail>-<km>km-<lat><n|s><lon><e|w>`, the centre in hundredths of a degree:
/// `20261004-streets-200km-3974n10499w`. The build is part of the id, so an update is a new
/// pack and a reader's cached directories can never point into a different build's bytes.
pub fn pack_id(build_id: &str, area: &StreetArea) -> String {
    let lat = (area.lat * 100.0).round() as i64;
    let lon = (area.lon * 100.0).round() as i64;
    format!(
        "{build_id}-{}-{}km-{}{}{}{}",
        area.detail.as_str(),
        area.km,
        lat.unsigned_abs(),
        if lat < 0 { 's' } else { 'n' },
        lon.unsigned_abs(),
        if lon < 0 { 'w' } else { 'e' },
    )
}

pub fn valid_pack_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && !id.starts_with('-')
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// `packs.json`, or an empty list. An unreadable file is set aside as
/// `packs.json.corrupt-<time>` rather than overwritten, so nothing is lost by reading it.
pub fn load_registry(dir: &MapsDir) -> Registry {
    let path = dir.registry();
    let Ok(bytes) = fs::read(&path) else {
        return Registry::default();
    };
    match serde_json::from_slice(&bytes) {
        Ok(r) => r,
        Err(_) => {
            let aside = path.with_extension(format!("json.corrupt-{}", now_unix()));
            let _ = fs::rename(&path, aside);
            Registry::default()
        }
    }
}

/// Write `packs.json` through a temporary file and a rename, so a crash leaves the old list or
/// the new one, never half of one.
pub fn save_registry(dir: &MapsDir, reg: &Registry) -> Result<(), StreetError> {
    fs::create_dir_all(dir.root()).map_err(|e| StreetError::io("create the maps folder", &e))?;
    let path = dir.registry();
    let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    let bytes = serde_json::to_vec_pretty(reg).expect("plain JSON values serialise");
    fs::write(&tmp, bytes).map_err(|e| StreetError::io("write packs.json", &e))?;
    fs::rename(&tmp, &path).map_err(|e| StreetError::io("replace packs.json", &e))
}

/// The finished packs: listed in `packs.json` and present on disk.
pub fn list_packs(dir: &MapsDir, reg: &Registry) -> Vec<StreetPack> {
    reg.packs
        .iter()
        .filter(|r| dir.pack(&r.pack.id).is_file())
        .map(|r| r.pack.clone())
        .collect()
}

#[cfg(unix)]
pub(crate) fn read_exact_at(f: &File, buf: &mut [u8], off: u64) -> io::Result<()> {
    std::os::unix::fs::FileExt::read_exact_at(f, buf, off)
}

#[cfg(windows)]
pub(crate) fn read_exact_at(f: &File, mut buf: &mut [u8], mut off: u64) -> io::Result<()> {
    use std::os::windows::fs::FileExt;
    while !buf.is_empty() {
        match f.seek_read(buf, off) {
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(n) => {
                buf = &mut buf[n..];
                off += n as u64;
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

#[cfg(unix)]
pub(crate) fn write_all_at(f: &File, buf: &[u8], off: u64) -> io::Result<()> {
    std::os::unix::fs::FileExt::write_all_at(f, buf, off)
}

#[cfg(windows)]
pub(crate) fn write_all_at(f: &File, mut buf: &[u8], mut off: u64) -> io::Result<()> {
    use std::os::windows::fs::FileExt;
    while !buf.is_empty() {
        match f.seek_write(buf, off) {
            Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
            Ok(n) => {
                buf = &buf[n..];
                off += n as u64;
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// `length` bytes of pack `id` from `offset`: exactly that many, or fewer only at the end of
/// the file. Over [`MAX_READ`], a malformed id or an id with no pack is an error.
pub fn read_pack(
    dir: &MapsDir,
    id: &str,
    offset: u64,
    length: u32,
) -> Result<Vec<u8>, StreetError> {
    if length > MAX_READ {
        return Err(StreetError::new(
            ErrorKind::BadRequest,
            format!("a read of {length} bytes is over the {MAX_READ}-byte limit"),
        ));
    }
    if !valid_pack_id(id) {
        return Err(StreetError::new(
            ErrorKind::BadRequest,
            "malformed street map id",
        ));
    }
    let f = File::open(dir.pack(id)).map_err(|e| match e.kind() {
        io::ErrorKind::NotFound => {
            StreetError::new(ErrorKind::UnknownPack, format!("no street map {id}"))
        }
        _ => StreetError::io("open the street map", &e),
    })?;
    let len = f
        .metadata()
        .map_err(|e| StreetError::io("read the street map", &e))?
        .len();
    let n = u64::from(length).min(len.saturating_sub(offset));
    let mut buf = vec![0u8; n as usize];
    read_exact_at(&f, &mut buf, offset).map_err(|e| StreetError::io("read the street map", &e))?;
    Ok(buf)
}

/// Device names Windows opens in every folder: `assets/fonts/con.pbf` would be the console.
fn is_reserved_windows_name(stem: &str) -> bool {
    let u = stem.to_ascii_uppercase();
    matches!(u.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((u.starts_with("COM") || u.starts_with("LPT"))
            && u.len() == 4
            && u.as_bytes()[3].is_ascii_digit())
}

/// A path under `assets/` as the webview may name it: relative, `/`-separated, at most four
/// segments of letters, digits, space, comma, `.`, `_`, `@` and `-` (glyph ranges live in
/// folders such as `Noto Sans Regular`, and a font stack can be a comma-separated list). No
/// `..`, no absolute path, no backslash, no NUL, no colon (a drive letter or an NTFS stream),
/// nothing hidden, no trailing dot or space (Windows drops them, so two names would mean one
/// file), and no Windows device name.
pub fn validate_asset_path(p: &str) -> Result<Vec<&str>, StreetError> {
    let refuse = |why: &str| {
        Err(StreetError::new(
            ErrorKind::BadRequest,
            format!("asset path refused: {why}"),
        ))
    };
    if p.is_empty() || p.len() > 200 {
        return refuse("empty or too long");
    }
    if p.contains('\0') {
        return refuse("NUL");
    }
    if p.contains('\\') {
        return refuse("backslash");
    }
    if p.starts_with('/') {
        return refuse("absolute");
    }
    let segments: Vec<&str> = p.split('/').collect();
    if segments.len() > 4 {
        return refuse("too deep");
    }
    for s in &segments {
        if s.is_empty() {
            return refuse("empty segment");
        }
        if *s == "." || *s == ".." {
            return refuse("dot segment");
        }
        if !s
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b" ,._@-".contains(&b))
        {
            return refuse("character outside the allowed set");
        }
        if s.starts_with(['.', ' ']) || s.ends_with(['.', ' ']) {
            return refuse("leading or trailing dot or space");
        }
        let stem = s.split('.').next().unwrap_or(s).trim_end();
        if is_reserved_windows_name(stem) {
            return refuse("device name");
        }
    }
    Ok(segments)
}

/// The asset at `p` under the maps folder's `assets/`.
pub fn read_asset(dir: &MapsDir, p: &str) -> Result<Vec<u8>, StreetError> {
    let segments = validate_asset_path(p)?;
    let base = dir.assets();
    let mut path = base.clone();
    for s in segments {
        path.push(s);
    }
    let not_found = || StreetError::new(ErrorKind::NotFound, format!("no map asset {p}"));
    let meta = fs::symlink_metadata(&path).map_err(|_| not_found())?;
    if !meta.is_file() {
        return Err(not_found());
    }
    if meta.len() > MAX_ASSET {
        return Err(StreetError::new(
            ErrorKind::BadRequest,
            format!("map asset {p} is over {MAX_ASSET} bytes"),
        ));
    }
    // The installer writes plain files only; a link anywhere on the way out of the folder is
    // refused rather than followed.
    let inside = match (fs::canonicalize(&path), fs::canonicalize(&base)) {
        (Ok(f), Ok(b)) => f.starts_with(b),
        _ => false,
    };
    if !inside {
        return Err(not_found());
    }
    fs::read(&path).map_err(|e| StreetError::io("read the map asset", &e))
}

/// Free bytes on the volume holding `path` (or its nearest existing parent).
pub fn free_space(path: &Path) -> io::Result<u64> {
    let mut p = path;
    while !p.exists() {
        p = p
            .parent()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no existing parent"))?;
    }
    free_space_at(p)
}

#[cfg(unix)]
fn free_space_at(path: &Path) -> io::Result<u64> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in path"))?;
    let mut st = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: `c` is a NUL-terminated path and `st` is writable space for one statvfs.
    if unsafe { libc::statvfs(c.as_ptr(), st.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: statvfs returned 0, so it filled `st`.
    let st = unsafe { st.assume_init() };
    // The field types differ between Linux (u64) and macOS (u32), so the casts are needed on one.
    #[allow(clippy::unnecessary_cast)]
    Ok((st.f_bavail as u64).saturating_mul(st.f_frsize as u64))
}

#[cfg(windows)]
fn free_space_at(path: &Path) -> io::Result<u64> {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut avail = 0u64;
    // SAFETY: `wide` is NUL-terminated; the out-pointers are valid or null, as the API allows.
    let ok = unsafe {
        windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut avail,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(avail)
}

/// Refuse a download when free space is under twice what it still needs to write.
/// `on_disk` is what an unfinished download of the same pack already occupies.
pub fn check_disk(free: u64, pack_bytes: u64, on_disk: u64) -> Result<(), StreetError> {
    let need = pack_bytes.saturating_mul(2).saturating_sub(on_disk);
    if free < need {
        return Err(StreetError::new(
            ErrorKind::DiskSpace,
            format!("the street map needs {need} bytes free and {free} are available"),
        ));
    }
    Ok(())
}

const README: &str = "Nexus street maps
=================

The street-map packs in this folder (street-*.pmtiles) are extracts of
OpenStreetMap data, (c) OpenStreetMap contributors, made available under the
Open Database License (ODbL) 1.0:

    https://opendatacommons.org/licenses/odbl/1-0/
    https://www.openstreetmap.org/copyright

Contains information from OpenStreetMap, which is made available here under
the Open Database License (ODbL).

Each pack is a subset of a Protomaps basemap build, copied unmodified. Besides
OpenStreetMap it holds:

- water and land polygons from osmdata.openstreetmap.de (ODbL);
- Natural Earth data (public domain);
- landcover at zoom 0 to 7, derived from ESA WorldCover: (c) ESA WorldCover
  project 2020 / Contains modified Copernicus Sentinel data (2020) processed by
  ESA WorldCover consortium. CC BY 4.0,
  https://creativecommons.org/licenses/by/4.0/

Each pack's own metadata records the build it was cut from, that build's date
and the area it covers. You may use, share and adapt these files under the
ODbL; a database you derive from them must also be offered under the ODbL.

The assets folder holds the map's fonts (Noto Sans, SIL Open Font License 1.1,
see assets/OFL.txt) and its icons (MIT, see assets/ICONS-LICENSE.txt).

To free the space, remove a street map in Nexus, or delete this folder while
Nexus is closed.
";

/// Create the folder and write `README-maps.txt` when it is missing or out of date.
pub fn ensure_folder(dir: &MapsDir) -> Result<(), StreetError> {
    fs::create_dir_all(dir.root()).map_err(|e| StreetError::io("create the maps folder", &e))?;
    let path = dir.readme();
    if fs::read(&path).ok().as_deref() != Some(README.as_bytes()) {
        fs::write(&path, README).map_err(|e| StreetError::io("write README-maps.txt", &e))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::scratch_dir;

    #[test]
    fn pack_reads_are_bounded_and_ids_never_become_paths() {
        let tmp = scratch_dir("reads");
        let dir = MapsDir::new(tmp.clone());
        fs::write(dir.pack("20261004-streets-50km-100n200w"), b"0123456789").unwrap();

        let ok = |o, l| read_pack(&dir, "20261004-streets-50km-100n200w", o, l).unwrap();
        assert_eq!(ok(0, 4), b"0123", "exactly the length asked for");
        assert_eq!(ok(8, 4), b"89", "fewer only at the end of the file");
        assert_eq!(ok(10, 4), b"");
        assert_eq!(ok(99, 4), b"");
        assert_eq!(ok(0, 0), b"");
        assert_eq!(
            ok(0, MAX_READ).len(),
            10,
            "control: the cap itself is allowed"
        );

        let err = |id: &str, l| read_pack(&dir, id, 0, l).unwrap_err().kind;
        assert_eq!(
            err("20261004-streets-50km-100n200w", MAX_READ + 1),
            ErrorKind::BadRequest
        );
        assert_eq!(
            err("20261004-streets-50km-999n999e", 4),
            ErrorKind::UnknownPack
        );
        for id in [
            "",
            "../packs",
            "a/b",
            "A",
            "x\\y",
            "c:x",
            "x.pmtiles",
            "-x",
            "x\0y",
            "street-x",
        ] {
            let e = read_pack(&dir, id, 0, 4).unwrap_err().kind;
            assert!(
                matches!(e, ErrorKind::BadRequest | ErrorKind::UnknownPack),
                "{id:?}: {e:?}"
            );
        }
        // A file beside the packs is not reachable by any id.
        fs::write(tmp.join("packs.json"), b"{}").unwrap();
        assert_eq!(err("packs", 4), ErrorKind::UnknownPack);
    }

    #[test]
    fn asset_paths_are_strict() {
        for good in [
            "fonts/Noto Sans Regular/0-255.pbf",
            "fonts/Noto Sans Regular,Noto Sans Medium/256-511.pbf",
            "sprites/light@2x.png",
            "sprites/dark.json",
            "OFL.txt",
        ] {
            assert!(validate_asset_path(good).is_ok(), "{good}");
        }
        for bad in [
            "",
            "../packs.json",
            "fonts/../../x",
            "./OFL.txt",
            "/etc/passwd",
            "C:/Windows/win.ini",
            "fonts\\x.pbf",
            "fonts//x.pbf",
            "fonts/x.pbf/",
            "OFL.txt\0.png",
            ".hidden",
            "fonts/trailing.",
            "fonts/trailing ",
            "fonts/con.pbf",
            "NUL",
            "fonts/COM1.txt",
            "fonts/LPT9",
            "OFL.txt:stream",
            "a/b/c/d/e",
            "fonts/x%2e%2e.pbf",
            "fonts/x?y",
            "sprites/ünï.png",
        ] {
            assert_eq!(
                validate_asset_path(bad).unwrap_err().kind,
                ErrorKind::BadRequest,
                "{bad:?}"
            );
        }
        assert!(validate_asset_path(&"a".repeat(201)).is_err());
        // Control: COM alone and CONSOLE are ordinary names.
        assert!(validate_asset_path("COM").is_ok() && validate_asset_path("CONSOLE.txt").is_ok());
    }

    #[test]
    fn assets_are_read_from_the_assets_folder_only_and_capped() {
        let tmp = scratch_dir("assets");
        let dir = MapsDir::new(tmp.clone());
        fs::create_dir_all(dir.assets().join("fonts/Noto Sans Regular")).unwrap();
        fs::write(
            dir.assets().join("fonts/Noto Sans Regular/0-255.pbf"),
            b"glyphs",
        )
        .unwrap();
        fs::write(
            dir.assets().join("big.png"),
            vec![0u8; MAX_ASSET as usize + 1],
        )
        .unwrap();
        fs::write(tmp.join("secret.txt"), b"outside").unwrap();

        assert_eq!(
            read_asset(&dir, "fonts/Noto Sans Regular/0-255.pbf").unwrap(),
            b"glyphs"
        );
        assert_eq!(
            read_asset(&dir, "fonts/Noto Sans Regular/256-511.pbf")
                .unwrap_err()
                .kind,
            ErrorKind::NotFound
        );
        assert_eq!(
            read_asset(&dir, "fonts").unwrap_err().kind,
            ErrorKind::NotFound,
            "a folder"
        );
        assert_eq!(
            read_asset(&dir, "big.png").unwrap_err().kind,
            ErrorKind::BadRequest
        );
        assert_eq!(
            read_asset(&dir, "../secret.txt").unwrap_err().kind,
            ErrorKind::BadRequest
        );

        #[cfg(unix)]
        {
            // A link inside the folder that leads out of it is not followed.
            std::os::unix::fs::symlink(tmp.join("secret.txt"), dir.assets().join("link.txt"))
                .unwrap();
            assert_eq!(
                read_asset(&dir, "link.txt").unwrap_err().kind,
                ErrorKind::NotFound
            );
            std::os::unix::fs::symlink(&tmp, dir.assets().join("up")).unwrap();
            assert_eq!(
                read_asset(&dir, "up/secret.txt").unwrap_err().kind,
                ErrorKind::NotFound
            );
        }
    }

    #[test]
    fn a_download_is_refused_below_twice_its_size() {
        let mb = 1_000_000;
        assert_eq!(
            check_disk(199 * mb, 100 * mb, 0).unwrap_err().kind,
            ErrorKind::DiskSpace
        );
        assert!(
            check_disk(200 * mb, 100 * mb, 0).is_ok(),
            "control: exactly twice is enough"
        );
        // A resumed download already holds its file's space.
        assert!(check_disk(150 * mb, 100 * mb, 50 * mb).is_ok());
        assert!(check_disk(149 * mb, 100 * mb, 50 * mb).is_err());
        // The real probe answers for a real folder, and for one not created yet.
        let tmp = scratch_dir("disk");
        assert!(free_space(&tmp).unwrap() > 0);
        assert!(free_space(&tmp.join("not/yet/made")).unwrap() > 0);
    }

    #[test]
    fn the_registry_round_trips_and_a_corrupt_one_is_set_aside() {
        let tmp = scratch_dir("registry");
        let dir = MapsDir::new(tmp.clone());
        assert_eq!(load_registry(&dir), Registry::default());
        let area = StreetArea {
            lat: 39.74,
            lon: -104.99,
            km: 200,
            detail: Detail::Streets,
        };
        let id = pack_id("20261004", &area);
        assert_eq!(id, "20261004-streets-200km-3974n10499w");
        let rec = PackRecord {
            pack: StreetPack {
                id: id.clone(),
                name: "DM79mr 200 km".into(),
                bbox: [-106.0, 38.8, -103.9, 40.6],
                min_zoom: 0,
                max_zoom: 14,
                detail: Detail::Streets,
                bytes: 76_000_000,
                data_date: "2026-10-04".into(),
                sha256: "ab".repeat(32),
            },
            build_id: "20261004".into(),
            source_url: "https://maps.example/planet-20261004.pmtiles".into(),
            area,
            created_unix: 1,
        };
        let reg = Registry {
            packs: vec![rec.clone()],
            ..Registry::default()
        };
        save_registry(&dir, &reg).unwrap();
        assert_eq!(load_registry(&dir), reg);
        assert!(
            list_packs(&dir, &reg).is_empty(),
            "listed but not on disk: not offered"
        );
        fs::write(dir.pack(&id), b"x").unwrap();
        assert_eq!(list_packs(&dir, &reg), vec![rec.pack.clone()]);
        // The UI's shape: camelCase, detail as a word, bbox as an array.
        let v = serde_json::to_value(&rec.pack).unwrap();
        assert_eq!(v["minZoom"], 0);
        assert_eq!(v["dataDate"], "2026-10-04");
        assert_eq!(v["detail"], "streets");
        assert_eq!(v["bbox"][0], -106.0);

        fs::write(dir.registry(), b"{ not json").unwrap();
        assert_eq!(load_registry(&dir), Registry::default());
        let aside = fs::read_dir(&tmp).unwrap().filter_map(|e| e.ok()).any(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("packs.json.corrupt-")
        });
        assert!(aside, "the unreadable list is kept, not overwritten");
    }

    #[test]
    fn the_readme_is_written_once_and_kept_current() {
        let tmp = scratch_dir("readme");
        let dir = MapsDir::new(tmp.join("maps"));
        ensure_folder(&dir).unwrap();
        let text = fs::read_to_string(dir.readme()).unwrap();
        assert!(text.contains("Open Database License (ODbL) 1.0"));
        assert!(text.contains("https://opendatacommons.org/licenses/odbl/1-0/"));
        fs::write(dir.readme(), "stale").unwrap();
        ensure_folder(&dir).unwrap();
        assert_eq!(fs::read_to_string(dir.readme()).unwrap(), text);
    }
}
