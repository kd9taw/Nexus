//! The optional street map's commands: thin glue over `crates/street-map`, which holds every
//! rule and its tests. Each command resolves the maps folder, hands the work to the blocking
//! pool (file and network I/O never run on the async workers or the main thread), and returns
//! what the crate returns.
//!
//! The packs live in Tauri's local-data folder under `Nexus/maps` (Windows
//! `%LOCALAPPDATA%\Nexus\maps`, macOS `~/Library/Application Support/Nexus/maps`, Linux
//! `~/.local/share/Nexus/maps`), never in the shared data folder. Nothing here runs at startup:
//! the crate's service is created on the first street-map command, and only
//! `street_map_size`, `street_map_download` and `street_map_updates`, each called when the
//! operator presses something, reach the network.
//!
//! Reads come back as raw bytes through `tauri::ipc::Response`, not the asset protocol: Tauri
//! 2.11's asset protocol cuts every range response at 1000 KiB, and the pmtiles reader would
//! take a short directory or tile without noticing. The webview names a pack by id and an
//! asset by a checked relative path; it never names a file.
//!
//! THE BENCH (`street_map_info`, `street_map_install_file`). The Street choice stays hidden until
//! the hosted index exists. `NEXUS_STREET_MAP=1` in the environment shows it on one computer
//! anyway, with a bench aid that installs a map file the operator already has: the OS file picker
//! names the file, so the webview still never does.

use std::sync::{Arc, OnceLock};

use serde::Serialize;
use street_map::{
    ErrorKind, StreetArea, StreetError, StreetInstalled, StreetMaps, StreetPack, StreetProgress,
    StreetSize, StreetUnfinished, StreetUpdate,
};
use tauri::ipc::{Channel, Response};
use tauri::{AppHandle, Manager};

/// The crate's service, made on first use.
#[derive(Default)]
pub struct StreetMapState {
    maps: OnceLock<Arc<StreetMaps>>,
}

/// `$LOCALDATA/Nexus/maps`.
fn maps_folder(app: &AppHandle) -> Result<std::path::PathBuf, StreetError> {
    Ok(app
        .path()
        .local_data_dir()
        .map_err(|e| StreetError::new(ErrorKind::Io, format!("no local data folder: {e}")))?
        .join("Nexus")
        .join("maps"))
}

fn maps(app: &AppHandle) -> Result<Arc<StreetMaps>, StreetError> {
    let state = app.state::<StreetMapState>();
    if let Some(m) = state.maps.get() {
        return Ok(Arc::clone(m));
    }
    let made = Arc::new(StreetMaps::new(
        maps_folder(app)?,
        &street_map::default_origin(),
    )?);
    Ok(Arc::clone(state.maps.get_or_init(|| made)))
}

/// Run `f` with the service on the blocking pool.
async fn on_pool<T: Send + 'static>(
    app: AppHandle,
    f: impl FnOnce(&StreetMaps) -> Result<T, StreetError> + Send + 'static,
) -> Result<T, StreetError> {
    tauri::async_runtime::spawn_blocking(move || f(&*maps(&app)?))
        .await
        .map_err(|e| StreetError::new(ErrorKind::Io, format!("street map task failed: {e}")))?
}

fn logged<T>(what: &str, r: Result<T, StreetError>) -> Result<T, StreetError> {
    if let Err(e) = &r {
        tempo_core::applog::warn("street-map", &format!("{what}: {e}"));
    }
    r
}

/// The finished packs on this computer.
#[tauri::command]
pub async fn street_map_packs(app: AppHandle) -> Vec<StreetPack> {
    logged("list packs", on_pool(app, |m| Ok(m.packs())).await).unwrap_or_default()
}

/// `length` bytes of pack `pack_id` from `offset`: exactly that many, or fewer only at the end
/// of the file. Over 4 MiB, or an id with no pack, is an error.
#[tauri::command]
pub async fn street_map_read(
    app: AppHandle,
    pack_id: String,
    offset: u64,
    length: u32,
) -> Result<Response, StreetError> {
    on_pool(app, move |m| m.read(&pack_id, offset, length))
        .await
        .map(Response::new)
}

/// A font or icon file from `maps/assets/`, named by a relative path such as
/// `fonts/Noto Sans Regular/0-255.pbf` or `sprites/light@2x.png`.
#[tauri::command]
pub async fn street_map_asset(app: AppHandle, path: String) -> Result<Response, StreetError> {
    on_pool(app, move |m| m.asset(&path))
        .await
        .map(Response::new)
}

/// The exact size of an area against the host's current build, and whether it fits on disk.
#[tauri::command]
pub async fn street_map_size(app: AppHandle, area: StreetArea) -> Result<StreetSize, StreetError> {
    logged("size", on_pool(app, move |m| m.size(&area)).await)
}

/// Download an area (fonts and icons first, with the first pack), resuming an unfinished
/// download of the same pack. Progress arrives on `on_progress`.
#[tauri::command]
pub async fn street_map_download(
    app: AppHandle,
    area: StreetArea,
    on_progress: Channel<StreetProgress>,
) -> Result<StreetPack, StreetError> {
    let done = on_pool(app, move |m| {
        // A closed window drops the channel; the download carries on regardless.
        m.download(&area, &|p| {
            let _ = on_progress.send(p);
        })
    })
    .await;
    if let Ok(p) = &done {
        tempo_core::applog::info(
            "street-map",
            &format!("downloaded {} ({} bytes)", p.id, p.bytes),
        );
    }
    logged("download", done)
}

/// Stop the running download; it keeps what it has. False when nothing is downloading.
#[tauri::command]
pub async fn street_map_cancel(app: AppHandle) -> bool {
    on_pool(app, |m| Ok(m.cancel())).await.unwrap_or(false)
}

/// Remove a pack, or what an unfinished download of it left. Returns the bytes freed.
#[tauri::command]
pub async fn street_map_remove(app: AppHandle, pack_id: String) -> Result<u64, StreetError> {
    logged("remove", on_pool(app, move |m| m.remove(&pack_id)).await)
}

/// Downloads that stopped before they finished, for "Resume".
#[tauri::command]
pub async fn street_map_unfinished(app: AppHandle) -> Vec<StreetUnfinished> {
    logged(
        "list unfinished",
        on_pool(app, |m| Ok(m.unfinished())).await,
    )
    .unwrap_or_default()
}

/// Installed packs that a newer build on the host could replace. Downloads nothing.
#[tauri::command]
pub async fn street_map_updates(app: AppHandle) -> Result<Vec<StreetUpdate>, StreetError> {
    logged("check for updates", on_pool(app, |m| m.updates()).await)
}

/// What the Settings block shows about this computer, and whether this run benches the street map.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreetInfo {
    /// The maps folder, for removing it by hand.
    folder: String,
    /// `NEXUS_STREET_MAP=1`: show the hidden Street choice and the bench aid on this computer.
    bench: bool,
}

/// The maps folder and the bench switch. Touches neither the disk nor the network.
#[tauri::command]
pub async fn street_map_info(app: AppHandle) -> Result<StreetInfo, StreetError> {
    Ok(StreetInfo {
        folder: maps_folder(&app)?.display().to_string(),
        bench: std::env::var("NEXUS_STREET_MAP").is_ok_and(|v| v.trim() == "1"),
    })
}

/// THE BENCH AID: the OS file picker, then the crate's `install_file` (a PMTiles pack, or the fonts
/// and icons archive). `None` when the picker is cancelled. The callback form of the picker, for
/// the reason `pick_data_folder` gives.
#[tauri::command]
pub async fn street_map_install_file(
    app: AppHandle,
) -> Result<Option<StreetInstalled>, StreetError> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .add_filter("PMTiles, fonts and icons", &["pmtiles", "gz", "tgz"])
        .pick_file(move |picked| {
            let _ = tx.send(picked);
        });
    let Some(path) = rx.await.ok().flatten().and_then(|p| p.into_path().ok()) else {
        return Ok(None);
    };
    let done = logged(
        "install a file",
        on_pool(app, move |m| m.install_file(&path)).await,
    );
    if let Ok(StreetInstalled::Pack { pack }) = &done {
        tempo_core::applog::info(
            "street-map",
            &format!("installed {} from a file ({} bytes)", pack.id, pack.bytes),
        );
    }
    done.map(Some)
}

#[cfg(test)]
mod tests {
    /// Every command here is registered, and registered once: a command missing from the
    /// handler list fails in the webview as "command not found" with nothing in any log.
    #[test]
    fn every_street_map_command_is_registered() {
        let src = include_str!("lib.rs");
        let handlers = src
            .split_once("tauri::generate_handler![")
            .and_then(|(_, rest)| rest.split_once("])"))
            .map(|(list, _)| list)
            .expect("lib.rs has a generate_handler! list");
        let own = include_str!("street_map.rs");
        let commands: Vec<&str> = own
            .split("#[tauri::command]\npub async fn ")
            .skip(1)
            .filter_map(|rest| rest.split_once('('))
            .map(|(name, _)| name)
            .collect();
        assert_eq!(commands.len(), 11, "{commands:?}");
        for name in commands {
            let entry = format!("street_map::{name},");
            assert_eq!(
                handlers.matches(&entry).count(),
                1,
                "{entry} in generate_handler!"
            );
        }
        assert!(src.contains(".manage(street_map::StreetMapState::default())"));
    }
}
