//! Deleting a received SSTV picture from a browser.
//!
//! It is the one destructive gesture in the gallery: a received picture is the only copy of
//! something somebody sent, and there is no re-download. So the browser names the picture by what
//! its own gallery ROW showed — the finish time and the mode — and the station resolves the file
//! itself; a path never crosses the wire in either direction, and a row the station no longer has,
//! or has twice, is refused rather than guessed at.
use super::receiver_filter::run;
use super::*;
use std::path::PathBuf;

/// One received picture on disk, plus the gallery entry that names it.
fn picture(
    name: &str,
    mode: &str,
    finished_utc: &str,
) -> (PathBuf, tempo_app::dto::SstvGalleryEntry) {
    let dir = crate::test_pictures_dir().join("Nexus SSTV");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    // A 1×1 24-bit BMP: real bytes, so the delete removes a real file.
    std::fs::write(&path, [0u8; 58]).unwrap();
    let entry = tempo_app::dto::SstvGalleryEntry {
        path: path.to_string_lossy().into_owned(),
        mode: mode.into(),
        finished_utc: finished_utc.into(),
        fsk_id: None,
        freq_mhz: 14.23,
        lines: 256,
    };
    (path, entry)
}

fn station(entries: Vec<tempo_app::dto::SstvGalleryEntry>) -> Fixture {
    let f = Fixture::new();
    f.engine.lock().unwrap().load_sstv_gallery(entries);
    f
}

fn del(finished_utc: &str, mode: &str) -> Value {
    json!({"action":"sstv.deleteImage","finishedUtc":finished_utc,"mode":mode})
}

#[test]
fn a_browser_deletes_the_row_it_was_shown_and_names_no_path_to_do_it() {
    let (path, entry) = picture(
        "20260915T120000_Scottie1.bmp",
        "Scottie 1",
        "2026-09-15T12:00:00Z",
    );
    let (kept, keep) = picture(
        "20260915T121000_Martin1.bmp",
        "Martin 1",
        "2026-09-15T12:10:00Z",
    );
    let f = station(vec![entry, keep]);
    let state = acquire_controls_version(&f, Instant::now(), 3);
    assert!(state["controls"]["capabilities"]
        .as_array()
        .unwrap()
        .contains(&json!("sstvGallery")));
    let command = control_request(&state, del("2026-09-15T12:00:00Z", "Scottie 1"));
    // An older station does not know the action at all.
    assert_eq!(run(&f, 2, &command), Err("stationUnsupported"));
    let applied = run(&f, 3, &command).unwrap();
    assert_eq!(applied["outcome"], "applied");
    assert_eq!(applied["evidence"], "stationState");
    // A dropped reply is answered from the receipt; nothing else is deleted.
    assert_eq!(run(&f, 3, &command).unwrap(), applied);
    assert!(!path.exists(), "the picture is gone from disk");
    assert!(kept.exists(), "and the other one is not");
    // The index is reconciled from the directory, which every test in this process shares, so
    // membership is what is asserted: the deleted row is gone from it and the other one is not.
    let engine = f.engine.lock().unwrap();
    let paths: Vec<&str> = engine
        .sstv_gallery()
        .iter()
        .map(|g| g.path.as_str())
        .collect();
    assert!(!paths.contains(&path.to_string_lossy().as_ref()));
    assert!(paths.contains(&kept.to_string_lossy().as_ref()));
    // Nothing about the transmit path was touched by a gallery delete.
    assert!(!engine.tx_enabled());
}

#[test]
fn a_row_the_station_no_longer_has_is_refused_and_deletes_nothing() {
    let (path, entry) = picture(
        "20260915T130000_Scottie2.bmp",
        "Scottie 2",
        "2026-09-15T13:00:00Z",
    );
    let f = station(vec![entry]);
    acquire_controls_version(&f, Instant::now(), 3);
    for (when, mode) in [
        ("2026-09-15T13:00:01Z", "Scottie 2"),
        ("2026-09-15T13:00:00Z", "Scottie 1"),
    ] {
        let state = control_state_version(&f, Instant::now(), 3);
        let result = run(&f, 3, &control_request(&state, del(when, mode)));
        // Nothing here is a real row, however plausible it looks.
        let result = result.unwrap();
        assert_eq!(result["outcome"], "rejected", "{when} {mode}");
        assert_eq!(result["reason"], "contextChanged");
        assert!(path.exists());
    }
    // Positive control: the row the station really has is deleted by the same path.
    let state = control_state_version(&f, Instant::now(), 3);
    let applied = run(
        &f,
        3,
        &control_request(&state, del("2026-09-15T13:00:00Z", "Scottie 2")),
    )
    .unwrap();
    assert_eq!(applied["outcome"], "applied");
    assert!(!path.exists());
}

#[test]
fn the_gallery_delete_needs_station_control_and_admits_no_other_argument() {
    let (path, entry) = picture("20260915T140000_PD120.bmp", "PD120", "2026-09-15T14:00:00Z");
    let f = station(vec![entry]);
    // A logging-only browser: refused before anything is removed.
    f.acquire(Instant::now());
    let state = control_state_version(&f, Instant::now(), 3);
    assert_eq!(
        run(
            &f,
            3,
            &control_request(&state, del("2026-09-15T14:00:00Z", "PD120"))
        ),
        Err("localPermissionRequired")
    );
    assert!(path.exists());
    // A path is not expressible: the desktop command's argument has no way in from a browser.
    for bad in [
        json!({"action":"sstv.deleteImage","finishedUtc":"2026-09-15T14:00:00Z","mode":"PD120","path":"/etc/passwd"}),
        json!({"action":"sstv.deleteImage","path":"/etc/passwd"}),
        json!({"action":"sstv.deleteImage","finishedUtc":"2026-09-15T14:00:00Z"}),
        json!({"action":"sstv.deleteImage","mode":"PD120"}),
    ] {
        assert!(
            serde_json::from_value::<station::Action>(bad.clone()).is_err(),
            "{bad}"
        );
    }
    assert!(path.exists());
    // Positive control: with station control the same request is admitted and removes the file.
    let state = acquire_controls_version(&f, Instant::now(), 3);
    let applied = run(
        &f,
        3,
        &control_request(&state, del("2026-09-15T14:00:00Z", "PD120")),
    )
    .unwrap();
    assert_eq!(applied["outcome"], "applied");
    assert!(!path.exists());
}
