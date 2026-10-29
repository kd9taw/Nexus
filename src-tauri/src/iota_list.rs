//! The IOTA island-group list: a copy ships with Nexus, a newer one is downloaded at most monthly,
//! and the island names the Logbook and the contact card show beside a reference are read from it.
//!
//! **Why a copy ships.** The list is IOTA Ltd's (© Radio Society of Great Britain and © Islands On
//! The Air (IOTA) Ltd; <https://www.iota-world.org/copyright-notice.html>). IOTA Ltd gave Nexus
//! written permission to include it in the installer, so the copy fetched when the installer is
//! built (`resources/iota/`, by `scripts/gen-iota-groups.mjs`; see NOTICE) names island groups on
//! the first launch, with no network and nothing to answer first. The permission is for the
//! installer: the list is never in the repository, so a build made without fetching it has no
//! shipped copy, and works as a list-less station does — the shape check until the first
//! download, which is due at once.
//!
//! **The monthly check.** IOTA Ltd updates the list about monthly. With the Settings switch on, the
//! station fetches it from iota-world.org when the copy in use is a month old, counting from the
//! newer of the shipped copy and the last download. A download is kept beside `settings.json` and
//! used instead of the shipped copy while it is the newer of the two, so a later release's copy is
//! never hidden by an older download.
//!
//! **Off is off.** Turning the switch off stops the check, deletes the downloaded copy and shows no
//! names at all, the shipped copy included: every IOTA feature falls back to the shape-only check
//! (`XX-NNN`), as before there was a list. The switch is kept here, beside the copy it governs, not
//! in `settings.json`, under the key the old download consent used: an operator who said no to that
//! download, or turned it off, stays off; one never asked is on.
//!
//! **Degrades, never breaks.** A failed download keeps the copy in use (shipped or downloaded); a
//! download whose shape changed is refused ([`propagation::iota::IotaGroups::plausible`]), so a
//! change to IOTA's file format leaves a stale list here, not an empty one. A failed attempt is
//! retried no sooner than a day later.

use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, OnceLock, RwLock};

use propagation::iota::{IotaGroup, IotaGroups};

/// At most one download a month (operator ruling, 2026-10-27).
const REFRESH_SECS: i64 = 30 * 86_400;
/// After a failed attempt, the next one waits at least a day.
const RETRY_SECS: i64 = 86_400;

/// The copy in use: its groups, its date (Unix seconds; 0: unknown) and whether it is the one that
/// shipped.
#[derive(Debug, Clone, PartialEq)]
struct Held {
    groups: IotaGroups,
    at: i64,
    bundled: bool,
}

/// The list in memory; `None` while the switch is off, or with no copy at all.
static GROUPS: LazyLock<RwLock<Option<Held>>> = LazyLock::new(|| RwLock::new(None));

/// Held while the meta, the downloaded file and the copy in use change together: a download that
/// comes back and the switch turned off take it in turn, so an off made while a check was in flight
/// is never undone by it. Never held across the network fetch itself.
static KEEP: Mutex<()> = Mutex::new(());

fn keep() -> std::sync::MutexGuard<'static, ()> {
    KEEP.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The event every window hears when the copy in use changes, so names a background download
/// brought show without a restart (`ui/src/features/iotaGroups.ts` reads the list again on it).
const CHANGED_EVENT: &str = "iota-list-changed";

/// The app, to tell its windows; set once in `setup`. Unset (tests), nothing is told.
static APP: OnceLock<tauri::AppHandle> = OnceLock::new();

pub(crate) fn set_app(app: tauri::AppHandle) {
    let _ = APP.set(app);
}

fn tell_windows() {
    use tauri::Emitter;
    if let Some(app) = APP.get() {
        let _ = app.emit(CHANGED_EVENT, ());
    }
}

fn data_path(dir: &Path) -> PathBuf {
    dir.join("iota_groups.json")
}

fn meta_path(dir: &Path) -> PathBuf {
    dir.join("iota_groups.meta.json")
}

/// What is kept beside the list: the switch and the download's history.
#[derive(serde::Serialize, serde::Deserialize, Default, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct IotaListMeta {
    /// The Settings switch, under the old consent's key so its answer carries over: `None` (never
    /// set) and `Some(true)` are on, `Some(false)` is off.
    #[serde(default, rename = "consent")]
    pub on: Option<bool>,
    /// Unix seconds of the last download that was kept (0: none).
    #[serde(default)]
    pub fetched_at: i64,
    /// Unix seconds of the last attempt, kept or not (0: none).
    #[serde(default)]
    pub attempted_at: i64,
    /// Why the last attempt was not kept, in plain words; `None` when it was.
    #[serde(default)]
    pub last_error: Option<String>,
}

impl IotaListMeta {
    fn enabled(&self) -> bool {
        self.on != Some(false)
    }
}

fn read_meta(dir: &Path) -> IotaListMeta {
    std::fs::read_to_string(meta_path(dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_meta(dir: &Path, m: &IotaListMeta) {
    if let Ok(json) = serde_json::to_string(m) {
        let _ = std::fs::write(meta_path(dir), json);
    }
}

/// Whether a download is due: only with the switch on, when the copy in use (dated `list_at`, 0
/// for none) is a month old, and never within a day of the last attempt.
pub(crate) fn due(m: &IotaListMeta, list_at: i64, now: i64) -> bool {
    m.enabled() && now - list_at >= REFRESH_SECS && now - m.attempted_at >= RETRY_SECS
}

/// The shipped copy's folder candidates, in the order the TLE seed's are tried
/// (`tle_seed_path`): the packaged app's resource dir, beside the executable, then the crate root
/// a `tauri dev` run and `cargo test` stand in.
fn bundled_dir() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(res) = crate::RESOURCE_DIR.get() {
        candidates.push(res.join("resources").join("iota"));
        candidates.push(res.join("iota"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(d) = exe.parent() {
            candidates.push(d.join("resources").join("iota"));
        }
    }
    candidates.push(PathBuf::from("resources/iota"));
    candidates
        .into_iter()
        .find(|d| d.join("iota_groups.json").is_file())
}

/// The shipped copy in `dir`, dated by the manifest the script writes beside it (0 without one).
/// `None` when it is missing or does not parse: the download, or the shape check, stands in.
fn read_bundled(dir: &Path) -> Option<Held> {
    let groups = IotaGroups::parse(&std::fs::read_to_string(data_path(dir)).ok()?).ok()?;
    let at = std::fs::read_to_string(dir.join("iota_groups.manifest.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("fetchedAt")?.as_i64())
        .unwrap_or(0);
    Some(Held {
        groups,
        at,
        bundled: true,
    })
}

/// The copy to use with the switch on: the operator's own download (in `dir`) while it is at least
/// as new as the shipped copy, else the shipped copy. `None` with the switch off.
fn pick(dir: &Path, m: &IotaListMeta, bundled: Option<Held>) -> Option<Held> {
    if !m.enabled() {
        return None;
    }
    let downloaded = (m.fetched_at > 0)
        .then(|| std::fs::read_to_string(data_path(dir)).ok())
        .flatten()
        .and_then(|s| IotaGroups::parse(&s).ok())
        .map(|groups| Held {
            groups,
            at: m.fetched_at,
            bundled: false,
        });
    match (downloaded, bundled) {
        (Some(d), Some(b)) => Some(if d.at >= b.at { d } else { b }),
        (d, b) => d.or(b),
    }
}

/// Fetch with `fetch`, check, and keep the list in `dir`. On success the new copy is returned;
/// on failure nothing on disk but the meta changes, so the copy in use stays. The meta is read
/// after the fetch, not before: if the switch was turned off while it ran, the result is dropped,
/// nothing is written and the off stands.
fn fetch_and_keep(
    dir: &Path,
    now: i64,
    fetch: impl FnOnce() -> Result<String, String>,
) -> Result<Held, String> {
    let fetched = fetch();
    let _keep = keep();
    let mut m = read_meta(dir);
    if !m.enabled() {
        // The off path deleted the file already; this one is for a copy it could not.
        let _ = std::fs::remove_file(data_path(dir));
        return Err(
            "the IOTA list was switched off during the check, so the download was dropped".into(),
        );
    }
    m.attempted_at = now;
    let result = fetched.and_then(|json| {
        let groups = IotaGroups::parse(&json)?;
        if !groups.plausible() {
            return Err(format!(
                "the IOTA list came back with only {} island groups, so the copy already held is kept",
                groups.len()
            ));
        }
        std::fs::write(data_path(dir), &json).map_err(|e| e.to_string())?;
        Ok(groups)
    });
    match result {
        Ok(groups) => {
            m.fetched_at = now;
            m.last_error = None;
            write_meta(dir, &m);
            Ok(Held {
                groups,
                at: now,
                bundled: false,
            })
        }
        Err(e) => {
            m.last_error = Some(e.clone());
            write_meta(dir, &m);
            Err(e)
        }
    }
}

/// The Settings row's status.
#[derive(serde::Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub(crate) struct IotaListStatus {
    enabled: bool,
    count: usize,
    /// The date of the copy in use (Unix seconds; 0: unknown or none).
    fetched_at: i64,
    /// Whether the copy in use is the one that shipped with Nexus.
    bundled: bool,
    last_error: Option<String>,
}

fn status() -> IotaListStatus {
    let m = read_meta(&crate::config_dir());
    let held = GROUPS.read().ok().and_then(|g| g.clone());
    IotaListStatus {
        enabled: m.enabled(),
        count: held.as_ref().map_or(0, |h| h.groups.len()),
        fetched_at: held.as_ref().map_or(0, |h| h.at),
        bundled: held.as_ref().is_some_and(|h| h.bundled),
        last_error: m.last_error,
    }
}

fn set_held(held: Option<Held>) {
    if let Ok(mut g) = GROUPS.write() {
        *g = held;
    }
}

/// The date of the copy in use, 0 for none.
fn held_at() -> i64 {
    GROUPS
        .read()
        .ok()
        .and_then(|g| g.as_ref().map(|h| h.at))
        .unwrap_or(0)
}

/// Download from iota-world.org and use the result. Blocking: run it off any command's thread.
fn download(now: i64) -> Result<(), String> {
    let dir = crate::config_dir();
    let held = fetch_and_keep(&dir, now, propagation::live::iota::fetch_groups)?;
    {
        // An off between the keep above and here wins: it is the operator's latest word.
        let _keep = keep();
        if !read_meta(&dir).enabled() {
            return Ok(());
        }
        set_held(Some(held));
    }
    tell_windows();
    Ok(())
}

/// Load the copy to use (switch on), or none (switch off).
fn load() -> IotaListMeta {
    let dir = crate::config_dir();
    let m = read_meta(&dir);
    set_held(pick(&dir, &m, bundled_dir().and_then(|d| read_bundled(&d))));
    m
}

/// At startup: load the copy to use and refresh it in the background when due (after a jitter, so
/// a fleet of installs never meets on IOTA's server).
pub(crate) fn load_at_startup() {
    let m = load();
    if due(&m, held_at(), crate::now_unix()) {
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_secs(crate::subsec_jitter(120)));
            if let Err(e) = download(crate::now_unix()) {
                eprintln!("tempo: IOTA list refresh failed: {e}");
            }
        });
    }
}

/// Whether `reference` is an island group: on the list when one is in use, else by its shape alone —
/// the check the Needed board drops a misread spot reference by, and the one the log-time offer
/// (`tempo_core::logbook::offer::iota_offer`'s `known`) is to be given.
pub(crate) fn known(reference: &str) -> bool {
    match GROUPS.read().ok().as_ref().and_then(|g| g.as_ref()) {
        Some(h) => h.groups.contains(reference),
        None => propagation::iota::canonical(reference).is_some(),
    }
}

/// The list's status: the switch, how many groups are in use, and the date of that copy.
#[tauri::command]
pub(crate) fn get_iota_list_status() -> IotaListStatus {
    status()
}

/// The Settings switch. On loads the copy to use and downloads a newer one when due; off deletes
/// the downloaded copy, stops the check and shows no names.
#[tauri::command]
pub(crate) async fn set_iota_list_enabled(enabled: bool) -> Result<IotaListStatus, String> {
    let dir = crate::config_dir();
    let keeping = keep();
    let mut m = read_meta(&dir);
    m.on = Some(enabled);
    if !enabled {
        let _ = std::fs::remove_file(data_path(&dir));
        m.fetched_at = 0;
        m.last_error = None;
        write_meta(&dir, &m);
        set_held(None);
        drop(keeping);
        tell_windows();
        return Ok(status());
    }
    // Turning it on is the operator asking now: the day's retry wait does not hold it back.
    m.attempted_at = 0;
    write_meta(&dir, &m);
    let m = load();
    drop(keeping);
    tell_windows();
    if due(&m, held_at(), crate::now_unix()) {
        tauri::async_runtime::spawn_blocking(|| download(crate::now_unix()))
            .await
            .map_err(|e| e.to_string())??;
    }
    Ok(status())
}

/// Every island group in use, for the names shown beside a reference; empty with the switch off.
#[tauri::command]
pub(crate) fn get_iota_groups() -> Vec<IotaGroup> {
    GROUPS
        .read()
        .ok()
        .and_then(|g| g.as_ref().map(|h| h.groups.groups()))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The list is never in the repository, so the tests make their own: 1,000 groups in IOTA's
    /// `groups.json` shape, EU-005 named `gb`.
    fn list(gb: &str) -> String {
        let groups: Vec<serde_json::Value> = (0..1000)
            .map(|i| {
                let name = if i == 5 {
                    gb.to_string()
                } else {
                    format!("Group {i}")
                };
                serde_json::json!({ "refno": format!("EU-{i:03}"), "name": name, "dxcc_num": "1" })
            })
            .collect();
        serde_json::to_string(&groups).unwrap()
    }
    /// The shipped copy's date, as the manifest gives it.
    const SHIPPED_AT: i64 = 1_793_000_000;
    const NOW: i64 = 1_800_000_000;

    fn meta(on: Option<bool>, fetched_at: i64, attempted_at: i64) -> IotaListMeta {
        IotaListMeta {
            on,
            fetched_at,
            attempted_at,
            last_error: None,
        }
    }

    /// This test's own config folder (`test_dirs`), made.
    fn dir() -> PathBuf {
        let d = crate::config_dir();
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A resource folder as `scripts/gen-iota-groups.mjs` leaves it, in this test's own folder.
    fn staged() -> PathBuf {
        let d = crate::test_dirs::root().join("resources-iota");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(data_path(&d), list("Great Britain")).unwrap();
        std::fs::write(
            d.join("iota_groups.manifest.json"),
            format!(r#"{{"fetchedAt":{SHIPPED_AT},"groups":1000}}"#),
        )
        .unwrap();
        d
    }

    fn shipped() -> Held {
        read_bundled(&staged()).expect("the staged copy reads")
    }

    /// A list unlike the shipped one, so a test can tell which copy is in use by a name.
    fn newer_list() -> String {
        list("Great Britain (newer)")
    }

    #[test]
    fn the_shipped_list_names_a_known_group_with_no_network() {
        let h = shipped();
        assert!(h.bundled);
        assert_eq!(h.at, SHIPPED_AT, "dated by its manifest");
        assert_eq!(h.groups.name("EU-005"), Some("Great Britain"));
        assert!(
            !h.groups.contains("AF-999"),
            "CONTROL: a reference it lacks"
        );
        // Without a manifest the copy still reads, undated, so it is due for a check at once.
        let d = staged();
        std::fs::remove_file(d.join("iota_groups.manifest.json")).unwrap();
        assert_eq!(read_bundled(&d).unwrap().at, 0);
    }

    /// The list is never committed: a build made without fetching it has no shipped copy, and the
    /// station is a list-less one — no names, the shape check, and a download due at once.
    #[test]
    fn a_build_without_the_list_falls_back_to_the_shape_until_the_first_check() {
        let empty = crate::test_dirs::root().join("resources-empty");
        std::fs::create_dir_all(&empty).unwrap();
        assert!(read_bundled(&empty).is_none());
        let d = dir();
        let m = read_meta(&d);
        assert!(pick(&d, &m, None).is_none(), "no copy at all");
        assert!(due(&m, 0, NOW), "the first check is due at once");
        // The first check brings the names.
        let got = fetch_and_keep(&d, NOW, || Ok(newer_list())).unwrap();
        assert_eq!(got.groups.name("EU-005"), Some("Great Britain (newer)"));
        assert!(pick(&d, &read_meta(&d), None).is_some());
    }

    #[test]
    fn with_no_download_the_shipped_copy_is_used_and_no_answer_is_needed() {
        let d = dir();
        let m = read_meta(&d);
        assert_eq!(m.on, None, "a fresh install was never asked anything");
        let h = pick(&d, &m, Some(shipped())).expect("on by default");
        assert!(h.bundled);
        assert!(
            !due(&m, h.at, h.at + 29 * 86_400),
            "a fresh shipped copy is not re-fetched"
        );
    }

    #[test]
    fn the_monthly_check_replaces_the_shipped_copy_with_a_newer_one() {
        let d = dir();
        let s = shipped();
        let now = s.at + 30 * 86_400;
        assert!(
            due(&read_meta(&d), s.at, now),
            "a month after the shipped copy"
        );
        let got = fetch_and_keep(&d, now, || Ok(newer_list())).expect("kept");
        assert!(!got.bundled);
        assert_eq!(got.groups.name("EU-005"), Some("Great Britain (newer)"));
        // And at the next start, the download is the copy in use.
        let m = read_meta(&d);
        assert_eq!(m.fetched_at, now);
        let h = pick(&d, &m, Some(s.clone())).unwrap();
        assert_eq!(h.groups.name("EU-005"), Some("Great Britain (newer)"));
        assert!(
            !due(&m, h.at, now + 86_400),
            "checked again only a month later"
        );
        // CONTROL: a later release's copy, newer than that download, wins over it.
        let later = Held { at: now + 1, ..s };
        assert!(pick(&d, &m, Some(later)).unwrap().bundled);
    }

    #[test]
    fn a_failed_check_keeps_the_copy_in_use() {
        let d = dir();
        let s = shipped();
        let now = s.at + 31 * 86_400;
        for bad in [
            Err("HTTP 503".to_string()),
            Ok("<html>maintenance</html>".to_string()),
            Ok(r#"[{"refno":"EU-005","name":"Great Britain"}]"#.to_string()),
        ] {
            assert!(fetch_and_keep(&d, now, || bad.clone()).is_err(), "{bad:?}");
            let m = read_meta(&d);
            assert!(m.last_error.is_some());
            assert_eq!(m.fetched_at, 0);
            assert!(!data_path(&d).exists(), "nothing written");
            let h = pick(&d, &m, Some(s.clone())).unwrap();
            assert!(h.bundled, "the shipped copy stays in use");
            assert!(!due(&m, h.at, now + 3_600), "the retry waits a day");
        }
        // CONTROL: a downloaded copy held from before also stays, untouched.
        fetch_and_keep(&d, now, || Ok(newer_list())).unwrap();
        assert!(fetch_and_keep(&d, now + 31 * 86_400, || Err("offline".into())).is_err());
        let h = pick(&d, &read_meta(&d), Some(s)).unwrap();
        assert_eq!(h.groups.name("EU-005"), Some("Great Britain (newer)"));
    }

    /// Off: no names at all (the shipped copy included), nothing due, the downloaded copy deleted;
    /// on again uses the shipped copy.
    #[test]
    fn turning_it_off_hides_every_name_and_stops_the_check() {
        let d = crate::config_dir();
        std::fs::create_dir_all(&d).unwrap();
        fetch_and_keep(&d, NOW, || Ok(newer_list())).unwrap();
        assert!(data_path(&d).exists());
        let off = meta(Some(false), 0, 0);
        assert!(
            pick(&d, &off, Some(shipped())).is_none(),
            "off hides the shipped copy too"
        );
        assert!(!due(&off, 0, NOW), "nothing is fetched while off");
        // `block_on` polls on this thread, so the command's folder is this test's own.
        tauri::async_runtime::block_on(set_iota_list_enabled(false)).unwrap();
        assert!(!data_path(&d).exists(), "off deletes the downloaded copy");
        let m = read_meta(&d);
        assert_eq!(m.on, Some(false));
        assert_eq!(m.fetched_at, 0);
        let on = meta(Some(true), 0, 0);
        assert!(
            pick(&d, &on, Some(shipped())).unwrap().bundled,
            "CONTROL: on again"
        );
    }

    /// Off while a check is in flight: the download that comes back is dropped, neither kept on
    /// disk nor put in use, and the operator's off is not overwritten by the meta read before it.
    #[test]
    fn a_check_in_flight_when_the_list_is_turned_off_is_dropped() {
        let d = dir();
        let got = fetch_and_keep(&d, NOW, || {
            // `block_on` polls on this thread, so the command's folder is this test's own.
            tauri::async_runtime::block_on(set_iota_list_enabled(false)).unwrap();
            Ok(newer_list())
        });
        assert!(got.is_err(), "the download is not handed back to be used");
        assert!(!data_path(&d).exists(), "nor kept on disk");
        let m = read_meta(&d);
        assert_eq!(m.on, Some(false), "the operator's off stands");
        assert_eq!(m.fetched_at, 0);
        // CONTROL: with the switch still on when the download returns, it is kept.
        write_meta(&d, &meta(Some(true), 0, 0));
        let kept = fetch_and_keep(&d, NOW, || Ok(newer_list())).expect("kept");
        assert_eq!(kept.groups.name("EU-005"), Some("Great Britain (newer)"));
        assert!(data_path(&d).exists());
        assert_eq!(read_meta(&d).on, Some(true));
    }

    /// The old consent's answer carries over: a no stays off, a yes and never-asked are on.
    #[test]
    fn the_old_consent_reads_as_the_switch() {
        for (json, on) in [
            ("{}", true),
            (r#"{"consent":null}"#, true),
            (r#"{"consent":true,"fetchedAt":5}"#, true),
            (r#"{"consent":false}"#, false),
        ] {
            let m: IotaListMeta = serde_json::from_str(json).unwrap();
            assert_eq!(m.enabled(), on, "{json}");
        }
        let round: IotaListMeta =
            serde_json::from_str(&serde_json::to_string(&meta(Some(false), 0, 0)).unwrap())
                .unwrap();
        assert!(!round.enabled(), "written back under the same key");
    }

    #[test]
    fn a_held_list_is_refreshed_at_most_monthly_and_a_failure_waits_a_day() {
        let now = 100 * 86_400;
        let fresh = meta(None, now - 29 * 86_400, now - 29 * 86_400);
        assert!(
            !due(&fresh, fresh.fetched_at, now),
            "a 29-day-old list is kept"
        );
        let old = meta(None, now - 30 * 86_400, now - 30 * 86_400);
        assert!(
            due(&old, old.fetched_at, now),
            "a month-old list is refreshed"
        );
        let failed_today = meta(None, 0, now - 3_600);
        assert!(!due(&failed_today, 0, now), "an hour after a failure");
        let failed_yesterday = meta(None, 0, now - 86_400);
        assert!(due(&failed_yesterday, 0, now));
    }
}
