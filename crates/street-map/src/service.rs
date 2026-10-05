//! Everything the street-map commands do, behind one value the Tauri shell keeps. Each command
//! is one call here, so every rule is tested in this crate and the shell stays thin.
//!
//! One download runs at a time. Sizing an area keeps its result, and a Download of the same
//! area right after uses it instead of reading the directories again. A pack that is being
//! downloaded cannot be removed until the download ends; Cancel ends it.
//!
//! [`StreetMaps::install_file`] is the bench aid: it installs a map file the operator already has,
//! checked as a download is, so the street map can be tried before any host serves packs.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use reqwest::blocking::Client;
use serde::Serialize;
use serde_json::Value;

use crate::area::{maidenhead, Detail, StreetArea};
use crate::assets::{self, sha256_hex, MAX_ARCHIVE};
use crate::error::{ErrorKind, StreetError};
use crate::http::{client, get_all, with_retry, HttpSource};
use crate::job::{self, sha256_file, StreetProgress};
use crate::manifest::{self, Manifest};
use crate::plan::{self, Plan};
use crate::store::{
    check_disk, ensure_folder, list_packs, load_registry, now_unix, read_asset, read_pack,
    save_registry, valid_pack_id, AssetsRecord, MapsDir, PackRecord, StreetPack,
};
use crate::verify::verify_file;
use crate::Options;

/// The largest index file accepted.
const MANIFEST_LIMIT: u64 = 1 << 20;

/// The `build_id` of a pack installed from a file: there is no build on the host to update it from.
pub const LOCAL_BUILD: &str = "local";

/// What [`StreetMaps::install_file`] installed.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum StreetInstalled {
    /// A pack, listed with the downloaded ones.
    Pack { pack: StreetPack },
    /// The fonts and icons.
    Assets,
}

/// A pack's data date from its metadata: this crate's own `source_date`, else a Protomaps build's
/// OpenStreetMap replication time; empty when the file states neither.
fn data_date(m: &Value) -> String {
    ["source_date", "planetiler:osm:osmosisreplicationtime"]
        .iter()
        .filter_map(|k| m.get(*k).and_then(Value::as_str))
        .find(|d| d.len() >= 10 && d.as_bytes()[4] == b'-' && d.as_bytes()[7] == b'-')
        .map(|d| d[..10].to_string())
        .unwrap_or_default()
}

/// What the download sheet shows before Download: the exact size and whether it fits.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreetSize {
    pub pack_id: String,
    /// The finished pack's size.
    pub bytes: u64,
    /// What the download transfers, merged gaps included.
    pub download_bytes: u64,
    /// The fonts and icons, when this download fetches them too; 0 when already installed or
    /// when the host's index does not state their size.
    pub assets_bytes: u64,
    pub tiles: u64,
    pub requests: usize,
    pub bbox: [f64; 4],
    pub min_zoom: u8,
    pub max_zoom: u8,
    pub detail: Detail,
    pub build_id: String,
    /// The build's date, YYYY-MM-DD: what the data is as of.
    pub data_date: String,
    /// Free space where the maps folder is, when it could be read.
    pub free_bytes: Option<u64>,
    /// False when free space is under twice what the download still has to write.
    pub enough_space: bool,
    /// What an unfinished download of this same pack already has.
    pub resume_bytes: u64,
    /// This exact pack (same build, same area) is already installed.
    pub installed: bool,
}

/// A download that stopped before it finished; Download with its `area` resumes it while its
/// build is current.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreetUnfinished {
    pub pack_id: String,
    pub area: StreetArea,
    pub build_id: String,
    pub data_date: String,
    pub done_bytes: u64,
    pub total_bytes: u64,
}

/// A pack cut from an older build than the host now serves. The app offers the update; it
/// never fetches one by itself. Download with `area` gets the new pack, and Remove with
/// `pack_id` the old one.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreetUpdate {
    pub pack_id: String,
    pub area: StreetArea,
    pub data_date: String,
    pub new_build_id: String,
    pub new_data_date: String,
}

struct Running {
    pack_id: Option<String>,
    cancel: Arc<AtomicBool>,
}

/// Frees the one download slot however the download ends.
struct Claim<'a>(&'a Mutex<Option<Running>>);

impl Drop for Claim<'_> {
    fn drop(&mut self) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }
}

pub struct StreetMaps {
    dir: MapsDir,
    origin: String,
    client: Client,
    opts: Options,
    free_space: fn(&Path) -> io::Result<u64>,
    /// Held while `packs.json` is read and rewritten.
    registry: Mutex<()>,
    running: Mutex<Option<Running>>,
    sized: Mutex<Option<(Manifest, Plan)>>,
}

pub fn pack_name(area: &StreetArea) -> String {
    format!("{} {} km", maidenhead(area.lat, area.lon), area.km)
}

impl StreetMaps {
    /// Packs live in `maps_root`; the index is fetched from `origin` (`https://<host>`).
    pub fn new(maps_root: std::path::PathBuf, origin: &str) -> Result<Self, StreetError> {
        Ok(Self {
            dir: MapsDir::new(maps_root),
            origin: origin.trim_end_matches('/').to_string(),
            client: client()?,
            opts: Options::default(),
            free_space: crate::store::free_space,
            registry: Mutex::new(()),
            running: Mutex::new(None),
            sized: Mutex::new(None),
        })
    }

    pub fn with_options(mut self, opts: Options) -> Self {
        self.opts = opts;
        self
    }

    /// Replace the free-space probe (tests stand in a full disk).
    pub fn with_free_space(mut self, probe: fn(&Path) -> io::Result<u64>) -> Self {
        self.free_space = probe;
        self
    }

    pub fn dir(&self) -> &MapsDir {
        &self.dir
    }

    /// The finished packs on this computer.
    pub fn packs(&self) -> Vec<StreetPack> {
        let _held = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
        list_packs(&self.dir, &load_registry(&self.dir))
    }

    pub fn read(&self, pack_id: &str, offset: u64, length: u32) -> Result<Vec<u8>, StreetError> {
        read_pack(&self.dir, pack_id, offset, length)
    }

    pub fn asset(&self, path: &str) -> Result<Vec<u8>, StreetError> {
        read_asset(&self.dir, path)
    }

    fn manifest(&self, cancel: &AtomicBool) -> Result<Manifest, StreetError> {
        let url = format!("{}/streetmaps.json", self.origin);
        let bytes = with_retry(&self.opts.retry, cancel, &|_, _, _| {}, || {
            get_all(&self.client, &url, MANIFEST_LIMIT, cancel, &|_| {})
        })
        .map_err(|e| e.into_street(ErrorKind::Network))?;
        manifest::parse(&bytes, &self.origin)
    }

    fn assets_current(&self, m: &Manifest) -> bool {
        let reg = load_registry(&self.dir);
        reg.assets.is_some_and(|a| a.sha256 == m.assets.sha256) && assets::installed(&self.dir)
    }

    fn plan_for(
        &self,
        area: &StreetArea,
        cancel: &AtomicBool,
    ) -> Result<(Manifest, Plan), StreetError> {
        let manifest = self.manifest(cancel)?;
        let build = manifest.build_info();
        let src = HttpSource::new(&self.client, &build.url);
        let plan = plan::plan(&src, &build, area, &self.opts, cancel)?;
        Ok((manifest, plan))
    }

    fn is_installed(&self, plan: &Plan) -> bool {
        self.dir.pack(&plan.pack_id).is_file()
            && load_registry(&self.dir)
                .packs
                .iter()
                .any(|r| r.pack.id == plan.pack_id)
    }

    /// The exact size of `area` against the host's current build. Reads the host's index and
    /// a few MB of directories; downloads no tile data.
    pub fn size(&self, area: &StreetArea) -> Result<StreetSize, StreetError> {
        let area = &area.normalized();
        area.validate()?;
        let never = AtomicBool::new(false);
        let (manifest, plan) = self.plan_for(area, &never)?;
        let assets_bytes = if self.assets_current(&manifest) {
            0
        } else {
            manifest.assets.bytes.unwrap_or(0)
        };
        let (resume_bytes, on_disk) = job::resume_state(&self.dir, &plan).unwrap_or((0, 0));
        let free_bytes = (self.free_space)(self.dir.root()).ok();
        let enough_space = free_bytes
            .is_none_or(|free| check_disk(free, plan.total_len() + assets_bytes, on_disk).is_ok());
        let size = StreetSize {
            pack_id: plan.pack_id.clone(),
            bytes: plan.total_len(),
            download_bytes: plan.download_bytes(),
            assets_bytes,
            tiles: plan.header.addressed_tiles,
            requests: plan.requests.len(),
            bbox: plan.bbox.to_array(),
            min_zoom: plan.header.min_zoom,
            max_zoom: plan.header.max_zoom,
            detail: area.detail,
            build_id: plan.build.id.clone(),
            data_date: plan.build.date.clone(),
            free_bytes,
            enough_space,
            resume_bytes,
            installed: self.is_installed(&plan),
        };
        *self.sized.lock().unwrap_or_else(PoisonError::into_inner) = Some((manifest, plan));
        Ok(size)
    }

    /// Download `area` (fonts and icons first, if this is the first pack or the host has newer
    /// ones), resuming an unfinished download of the same pack. Progress goes to `progress`.
    pub fn download(
        &self,
        area: &StreetArea,
        progress: &(dyn Fn(StreetProgress) + Sync),
    ) -> Result<StreetPack, StreetError> {
        let area = &area.normalized();
        area.validate()?;
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut running = self.running.lock().unwrap_or_else(PoisonError::into_inner);
            if running.is_some() {
                return Err(StreetError::new(
                    ErrorKind::Busy,
                    "another street map is downloading",
                ));
            }
            *running = Some(Running {
                pack_id: None,
                cancel: Arc::clone(&cancel),
            });
        }
        let _claim = Claim(&self.running);

        let cached = self
            .sized
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
            .filter(|(_, p)| p.area == *area);
        let (manifest, plan) = match cached {
            Some(sized) => sized,
            None => self.plan_for(area, &cancel)?,
        };
        {
            let mut running = self.running.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(r) = running.as_mut() {
                r.pack_id = Some(plan.pack_id.clone());
            }
        }
        if self.is_installed(&plan) {
            let reg = load_registry(&self.dir);
            if let Some(r) = reg.packs.into_iter().find(|r| r.pack.id == plan.pack_id) {
                return Ok(r.pack);
            }
        }

        let need_assets = !self.assets_current(&manifest);
        let assets_bytes = if need_assets {
            manifest.assets.bytes.unwrap_or(0)
        } else {
            0
        };
        let on_disk = job::resume_state(&self.dir, &plan).map_or(0, |(_, size)| size);
        if let Ok(free) = (self.free_space)(self.dir.root()) {
            check_disk(free, plan.total_len() + assets_bytes, on_disk)?;
        }
        ensure_folder(&self.dir)?;
        if need_assets {
            self.install_assets(&manifest, &cancel, progress)?;
        }

        let src = HttpSource::expecting(&self.client, &plan.build.url, plan.source_len);
        let done = job::download(&plan, &src, &self.dir, &self.opts, &cancel, progress)?;
        let pack = StreetPack {
            id: plan.pack_id.clone(),
            name: pack_name(area),
            bbox: plan.bbox.to_array(),
            min_zoom: plan.header.min_zoom,
            max_zoom: plan.header.max_zoom,
            detail: area.detail,
            bytes: done.bytes,
            data_date: plan.build.date.clone(),
            sha256: done.sha256,
        };
        let _held = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
        let mut reg = load_registry(&self.dir);
        reg.packs.retain(|r| r.pack.id != pack.id);
        reg.packs.push(PackRecord {
            pack: pack.clone(),
            build_id: plan.build.id.clone(),
            source_url: plan.build.url.clone(),
            area: *area,
            created_unix: now_unix(),
        });
        save_registry(&self.dir, &reg)?;
        Ok(pack)
    }

    fn install_assets(
        &self,
        m: &Manifest,
        cancel: &AtomicBool,
        progress: &(dyn Fn(StreetProgress) + Sync),
    ) -> Result<(), StreetError> {
        let total = m.assets.bytes.unwrap_or(0);
        progress(StreetProgress::Assets { done: 0, total });
        let on_retry = |attempt: u32, wait: std::time::Duration, why: &str| {
            progress(StreetProgress::Retrying {
                attempt,
                wait_secs: wait.as_secs(),
                reason: why.to_string(),
            });
        };
        let bytes = with_retry(&self.opts.retry, cancel, &on_retry, || {
            get_all(&self.client, &m.assets.url, MAX_ARCHIVE, cancel, &|done| {
                progress(StreetProgress::Assets {
                    done,
                    total: total.max(done),
                });
            })
        })
        .map_err(|e| e.into_street(ErrorKind::Paused))?;
        assets::install(&self.dir, &bytes, &m.assets.sha256)?;
        let _held = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
        let mut reg = load_registry(&self.dir);
        reg.assets = Some(AssetsRecord {
            sha256: m.assets.sha256.clone(),
            url: m.assets.url.clone(),
            installed_unix: now_unix(),
        });
        save_registry(&self.dir, &reg)
    }

    /// Ask the running download to stop. It keeps what it has; Download resumes it. False when
    /// nothing is downloading.
    pub fn cancel(&self) -> bool {
        let running = self.running.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(r) = running.as_ref() else {
            return false;
        };
        r.cancel.store(true, Ordering::Relaxed);
        true
    }

    /// Remove a pack, or what an unfinished download of it left. Returns the bytes freed.
    pub fn remove(&self, pack_id: &str) -> Result<u64, StreetError> {
        if !valid_pack_id(pack_id) {
            return Err(StreetError::new(
                ErrorKind::BadRequest,
                "malformed street map id",
            ));
        }
        {
            let running = self.running.lock().unwrap_or_else(PoisonError::into_inner);
            if running
                .as_ref()
                .is_some_and(|r| r.pack_id.as_deref() == Some(pack_id))
            {
                return Err(StreetError::new(
                    ErrorKind::Busy,
                    "that street map is downloading; cancel it first",
                ));
            }
        }
        let _held = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
        let mut reg = load_registry(&self.dir);
        let listed = reg.packs.iter().any(|r| r.pack.id == pack_id);
        let mut freed = 0;
        let mut found = listed;
        for path in [
            self.dir.pack(pack_id),
            self.dir.part(pack_id),
            self.dir.journal(pack_id),
        ] {
            match fs::metadata(&path) {
                Ok(m) => {
                    fs::remove_file(&path)
                        .map_err(|e| StreetError::io("remove the street map", &e))?;
                    freed += m.len();
                    found = true;
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(StreetError::io("remove the street map", &e)),
            }
        }
        if !found {
            return Err(StreetError::new(
                ErrorKind::UnknownPack,
                format!("no street map {pack_id}"),
            ));
        }
        if listed {
            reg.packs.retain(|r| r.pack.id != pack_id);
            save_registry(&self.dir, &reg)?;
        }
        Ok(freed)
    }

    /// The installed packs a newer build could replace. Reads the host's index; downloads
    /// nothing.
    pub fn updates(&self) -> Result<Vec<StreetUpdate>, StreetError> {
        let manifest = self.manifest(&AtomicBool::new(false))?;
        let reg = {
            let _held = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
            load_registry(&self.dir)
        };
        Ok(reg
            .packs
            .into_iter()
            .filter(|r| {
                r.build_id != manifest.build.id
                    && r.build_id != LOCAL_BUILD
                    && self.dir.pack(&r.pack.id).is_file()
            })
            .map(|r| StreetUpdate {
                pack_id: r.pack.id,
                area: r.area,
                data_date: r.pack.data_date,
                new_build_id: manifest.build.id.clone(),
                new_data_date: manifest.build.date.clone(),
            })
            .collect())
    }

    /// Install a street map file the operator already has, before any host serves one (the bench
    /// aid). A gzip file is the fonts-and-icons archive, checked and unpacked as the first download
    /// would. Anything else must be a PMTiles vector map that passes the same checks as a finished
    /// download (structure, directories, every tile); it is copied into the maps folder under a
    /// `local-` id from its SHA-256 and listed with the downloaded packs. Installing the same file
    /// twice lists it once. Nothing reaches the network, and a local pack is never offered an update.
    pub fn install_file(&self, path: &Path) -> Result<StreetInstalled, StreetError> {
        let opened = |e: io::Error| StreetError::io("open the street map file", &e);
        let mut f = File::open(path).map_err(opened)?;
        let len = f.metadata().map_err(opened)?.len();
        let mut magic = [0u8; 2];
        let gzip = f.read(&mut magic).map_err(opened)? == 2 && magic == [0x1f, 0x8b];
        if gzip {
            if len > MAX_ARCHIVE {
                return Err(StreetError::archive(
                    "the fonts and icons archive is too large",
                ));
            }
            let bytes = fs::read(path).map_err(opened)?;
            let sha256 = sha256_hex(&bytes);
            ensure_folder(&self.dir)?;
            assets::install(&self.dir, &bytes, &sha256)?;
            let _held = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
            let mut reg = load_registry(&self.dir);
            reg.assets = Some(AssetsRecord {
                sha256,
                url: String::new(),
                installed_unix: now_unix(),
            });
            save_registry(&self.dir, &reg)?;
            return Ok(StreetInstalled::Assets);
        }

        let checked = verify_file(&f, len, &AtomicBool::new(false), &mut |_, _| {})?;
        let sha256 = sha256_file(&f, len)?;
        let id = format!("local-{}", &sha256[..12]);
        let h = &checked.header;
        let e7 = |v: i32| f64::from(v) / 1e7;
        let bbox = [
            e7(h.min_lon_e7),
            e7(h.min_lat_e7),
            e7(h.max_lon_e7),
            e7(h.max_lat_e7),
        ];
        let (lat, lon) = ((bbox[1] + bbox[3]) / 2.0, (bbox[0] + bbox[2]) / 2.0);
        let km = (((bbox[3] - bbox[1]) * 111.195).round() as u32).max(1);
        let detail = if h.max_zoom >= Detail::Streets.max_zoom() {
            Detail::Streets
        } else {
            Detail::Roads
        };
        let pack = StreetPack {
            id: id.clone(),
            name: format!("{} {km} km", maidenhead(lat, lon)),
            bbox,
            min_zoom: h.min_zoom,
            max_zoom: h.max_zoom,
            detail,
            bytes: len,
            data_date: data_date(&checked.metadata),
            sha256,
        };
        let listed = || {
            let _held = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
            load_registry(&self.dir)
                .packs
                .iter()
                .any(|r| r.pack.id == id)
        };
        if listed() && self.dir.pack(&id).is_file() {
            return Ok(StreetInstalled::Pack { pack });
        }
        ensure_folder(&self.dir)?;
        if let Ok(free) = (self.free_space)(self.dir.root()) {
            check_disk(free, len, 0)?;
        }
        // Copied under the in-progress name, then named: a copy cut short is never a pack.
        let part = self.dir.part(&id);
        if let Err(e) = fs::copy(path, &part) {
            let _ = fs::remove_file(&part);
            return Err(StreetError::io("copy the street map file", &e));
        }
        fs::rename(&part, self.dir.pack(&id))
            .map_err(|e| StreetError::io("name the street map file", &e))?;
        let _held = self.registry.lock().unwrap_or_else(PoisonError::into_inner);
        let mut reg = load_registry(&self.dir);
        reg.packs.retain(|r| r.pack.id != id);
        reg.packs.push(PackRecord {
            pack: pack.clone(),
            build_id: LOCAL_BUILD.to_string(),
            source_url: String::new(),
            area: StreetArea {
                lat,
                lon,
                km,
                detail,
            },
            created_unix: now_unix(),
        });
        save_registry(&self.dir, &reg)?;
        Ok(StreetInstalled::Pack { pack })
    }

    /// Downloads that stopped before they finished.
    pub fn unfinished(&self) -> Vec<StreetUnfinished> {
        job::unfinished(&self.dir)
            .into_iter()
            .map(|(info, done)| StreetUnfinished {
                pack_id: info.pack_id,
                area: info.area,
                build_id: info.build_id,
                data_date: info.data_date,
                done_bytes: done,
                total_bytes: info.download_bytes,
            })
            .collect()
    }
}
