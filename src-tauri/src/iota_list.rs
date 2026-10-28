//! The IOTA island-group list on the operator's own machine: downloaded only with their yes,
//! kept beside `settings.json`, refreshed at most monthly, and read for the island names the
//! Logbook and the contact card show beside a reference.
//!
//! **Why consent, and why never bundled.** The list is © Radio Society of Great Britain and
//! © Islands On The Air (IOTA) Ltd. Its copyright notice lets anyone download it "for your own
//! personal non-commercial home use" (<https://www.iota-world.org/copyright-notice.html>), and
//! IOTA Ltd publishes the address for software on its Developers page; it does not allow
//! redistribution. So no copy is in the repository or the installer: each operator's Nexus
//! fetches it for them, after one line that names IOTA Ltd and what is fetched, and only then.
//! Saying no, or never answering, leaves every IOTA feature on the shape-only check (`XX-NNN`),
//! as before. Taking the yes back deletes the copy.
//!
//! **The consent is kept here, not in `settings.json`.** It is a decision about a file in this
//! data folder: a settings file copied to another computer, or a profile borrowed from a friend,
//! must not download anything on that computer without its operator's own yes.
//!
//! **Degrades, never breaks.** A failed download keeps the copy already held (or none); a
//! download whose shape changed is refused ([`propagation::iota::IotaGroups::plausible`]), so a
//! change to IOTA's file format leaves a stale list here, not an empty one. A failed attempt is retried no sooner than a day later.

use std::path::PathBuf;
use std::sync::{LazyLock, RwLock};

use propagation::iota::{IotaGroup, IotaGroups};

/// At most one download a month (operator ruling, 2026-10-27).
const REFRESH_SECS: i64 = 30 * 86_400;
/// After a failed attempt, the next one waits at least a day.
const RETRY_SECS: i64 = 86_400;

/// The list in memory; `None` until a copy is loaded.
static GROUPS: LazyLock<RwLock<Option<IotaGroups>>> = LazyLock::new(|| RwLock::new(None));

fn data_path() -> PathBuf {
    crate::config_dir().join("iota_groups.json")
}

fn meta_path() -> PathBuf {
    crate::config_dir().join("iota_groups.meta.json")
}

/// What is kept beside the list: the operator's answer and the download's history.
#[derive(serde::Serialize, serde::Deserialize, Default, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct IotaListMeta {
    /// `None`: never asked. `Some(false)`: said no. `Some(true)`: said yes.
    #[serde(default)]
    pub consent: Option<bool>,
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

fn read_meta() -> IotaListMeta {
    std::fs::read_to_string(meta_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_meta(m: &IotaListMeta) {
    if let Ok(json) = serde_json::to_string(m) {
        let _ = std::fs::write(meta_path(), json);
    }
}

/// Whether a download is due: only with the operator's yes, when none is held or the one held is a
/// month old, and never within a day of the last attempt.
pub(crate) fn due(m: &IotaListMeta, have_list: bool, now: i64) -> bool {
    m.consent == Some(true)
        && (!have_list || now - m.fetched_at >= REFRESH_SECS)
        && now - m.attempted_at >= RETRY_SECS
}

/// The Settings row's and the consent line's status.
#[derive(serde::Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub(crate) struct IotaListStatus {
    consent: Option<bool>,
    count: usize,
    fetched_at: i64,
    last_error: Option<String>,
}

fn status() -> IotaListStatus {
    let m = read_meta();
    IotaListStatus {
        consent: m.consent,
        count: GROUPS
            .read()
            .ok()
            .and_then(|g| g.as_ref().map(IotaGroups::len))
            .unwrap_or(0),
        fetched_at: m.fetched_at,
        last_error: m.last_error,
    }
}

fn have_list() -> bool {
    GROUPS.read().is_ok_and(|g| g.is_some())
}

/// Download, check and keep the list. Blocking: run it off any command's thread.
fn download(now: i64) -> Result<(), String> {
    let mut m = read_meta();
    m.attempted_at = now;
    let result = propagation::live::iota::fetch_groups().and_then(|json| {
        let groups = IotaGroups::parse(&json)?;
        if !groups.plausible() {
            return Err(format!(
                "the IOTA list came back with only {} island groups, so the copy already held is kept",
                groups.len()
            ));
        }
        std::fs::write(data_path(), &json).map_err(|e| e.to_string())?;
        Ok(groups)
    });
    match result {
        Ok(groups) => {
            m.fetched_at = now;
            m.last_error = None;
            write_meta(&m);
            if let Ok(mut g) = GROUPS.write() {
                *g = Some(groups);
            }
            Ok(())
        }
        Err(e) => {
            m.last_error = Some(e.clone());
            write_meta(&m);
            Err(e)
        }
    }
}

/// At startup: load the copy held, if the operator said yes, and refresh it in the background
/// when due (after a jitter, so a fleet of installs never meets on IOTA's server).
pub(crate) fn load_at_startup() {
    let m = read_meta();
    if m.consent != Some(true) {
        return;
    }
    if let Some(groups) = std::fs::read_to_string(data_path())
        .ok()
        .and_then(|s| IotaGroups::parse(&s).ok())
    {
        if let Ok(mut g) = GROUPS.write() {
            *g = Some(groups);
        }
    }
    if due(&m, have_list(), crate::now_unix()) {
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_secs(crate::subsec_jitter(120)));
            if let Err(e) = download(crate::now_unix()) {
                eprintln!("tempo: IOTA list refresh failed: {e}");
            }
        });
    }
}

/// Whether `reference` is an island group: on the list when one is held, else by its shape alone —
/// the check the Needed board drops a misread spot reference by, and the one the log-time offer
/// (`tempo_core::logbook::offer::iota_offer`'s `known`) is to be given.
pub(crate) fn known(reference: &str) -> bool {
    match GROUPS.read().ok().as_ref().and_then(|g| g.as_ref()) {
        Some(groups) => groups.contains(reference),
        None => propagation::iota::canonical(reference).is_some(),
    }
}

/// The list's status: the operator's answer, how many groups are held, when it was fetched.
#[tauri::command]
pub(crate) fn get_iota_list_status() -> IotaListStatus {
    status()
}

/// The operator's answer to "download the IOTA island-group list?". Yes downloads it now when
/// due; no deletes any copy held and stops the refresh.
#[tauri::command]
pub(crate) async fn set_iota_list_consent(consent: bool) -> Result<IotaListStatus, String> {
    let mut m = read_meta();
    m.consent = Some(consent);
    if !consent {
        let _ = std::fs::remove_file(data_path());
        m.fetched_at = 0;
        m.last_error = None;
        write_meta(&m);
        if let Ok(mut g) = GROUPS.write() {
            *g = None;
        }
        return Ok(status());
    }
    // A yes is the operator asking now: the day's retry wait does not hold it back.
    m.attempted_at = 0;
    write_meta(&m);
    if due(&m, have_list(), crate::now_unix()) {
        tauri::async_runtime::spawn_blocking(|| download(crate::now_unix()))
            .await
            .map_err(|e| e.to_string())??;
    }
    Ok(status())
}

/// Every island group held, for the names shown beside a reference; empty without a list.
#[tauri::command]
pub(crate) fn get_iota_groups() -> Vec<IotaGroup> {
    GROUPS
        .read()
        .ok()
        .and_then(|g| g.as_ref().map(IotaGroups::groups))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(consent: Option<bool>, fetched_at: i64, attempted_at: i64) -> IotaListMeta {
        IotaListMeta {
            consent,
            fetched_at,
            attempted_at,
            last_error: None,
        }
    }

    /// THE CONSENT GATE: nothing is ever due without the operator's yes, held list or not.
    #[test]
    fn nothing_is_downloaded_without_a_yes() {
        let now = 100 * 86_400;
        for consent in [None, Some(false)] {
            assert!(!due(&meta(consent, 0, 0), false, now), "{consent:?}");
            assert!(!due(&meta(consent, 0, 0), true, now), "{consent:?}");
        }
        assert!(due(&meta(Some(true), 0, 0), false, now));
    }

    #[test]
    fn a_held_list_is_refreshed_at_most_monthly_and_a_failure_waits_a_day() {
        let now = 100 * 86_400;
        let fresh = meta(Some(true), now - 29 * 86_400, now - 29 * 86_400);
        assert!(!due(&fresh, true, now), "a 29-day-old list is kept");
        let old = meta(Some(true), now - 30 * 86_400, now - 30 * 86_400);
        assert!(due(&old, true, now), "a month-old list is refreshed");
        let failed_today = meta(Some(true), 0, now - 3_600);
        assert!(!due(&failed_today, false, now), "an hour after a failure");
        let failed_yesterday = meta(Some(true), 0, now - 86_400);
        assert!(due(&failed_yesterday, false, now));
    }

    #[test]
    fn an_older_meta_file_reads_as_never_asked() {
        let m: IotaListMeta = serde_json::from_str("{}").unwrap();
        assert_eq!(m.consent, None);
        assert!(!due(&m, false, 1_000_000));
    }
}
