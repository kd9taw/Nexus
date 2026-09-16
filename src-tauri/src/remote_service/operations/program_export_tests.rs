//! The working channel list rendered to CHIRP/CSV for a browser: the desktop's own two writers,
//! the same bounded chunking an activation export uses, and an unreadable file reported as a
//! failure rather than exported as an empty list.
use super::*;
use serde_json::json;

fn dir(tag: &str) -> std::path::PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("remote-progexport-{tag}-{stamp}"));
    std::fs::create_dir(&dir).unwrap();
    dir
}

fn channel(i: usize) -> propagation::memchan::Channel {
    let mut c: propagation::memchan::Channel = serde_json::from_value(json!({"id":"seed",
        "name":"W1AW","rxMhz":146.94,"duplex":"minus","offsetMhz":0.6,"toneMode":"tone",
        "rtoneHz":100.0,"ctoneHz":100.0,"dtcsCode":23,"mode":"fm","comment":"Newington, CT",
        "dmrColorCode":null,"dmrTimeslot":null,"dmrTalkgroup":null,"dstarRpt1":null,
        "dstarRpt2":null,"source":null}))
    .unwrap();
    c.id = format!("manual:{i}");
    c.name = format!("REPEATER{i:04}");
    c.rx_mhz = 146.0 + i as f64 / 1000.0;
    c
}

/// Write a `radioprog.json` holding one project with `count` channels under `id`.
fn seed(path: &std::path::Path, id: &str, count: usize) -> Vec<propagation::memchan::Channel> {
    let channels: Vec<_> = (0..count).map(channel).collect();
    let file = crate::RadioProgFile {
        version: 1,
        projects: vec![crate::RadioProgProject {
            id: id.into(),
            name: "My channels".into(),
            created_utc: 1,
            updated_utc: 2,
            channels: channels.clone(),
            ..Default::default()
        }],
    };
    std::fs::write(path, serde_json::to_vec(&file).unwrap()).unwrap();
    channels
}

/// Every chunk of one export, stitched back together and checked against the digest the station
/// sent — the browser's own discipline, run here so the Rust half is proved end to end.
fn whole(path: &std::path::Path, format: Format, cap: u32) -> String {
    let first = respond(path, format, cap, 0).unwrap();
    let chunks = first["file"]["chunks"].as_u64().unwrap();
    let mut bytes = Vec::new();
    for index in 0..chunks {
        let value = respond(path, format, cap, index as u32).unwrap();
        assert_eq!(value["operation"], "programExport");
        assert_eq!(value["file"], first["file"], "the file description moved");
        assert_eq!(value["index"], index);
        bytes.extend(
            crate::b64_decode(value["base64"].as_str().unwrap()).expect("chunk is not base64"),
        );
    }
    assert_eq!(
        bytes.len() as u64,
        first["file"]["byteLength"].as_u64().unwrap()
    );
    let sha: String = ring::digest::digest(&ring::digest::SHA256, &bytes)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(first["file"]["sha256"], sha);
    String::from_utf8(bytes).unwrap()
}

#[test]
fn renders_the_working_project_with_the_desktops_own_writers() {
    let dir = dir("writers");
    let path = dir.join("radioprog.json");
    let channels = seed(&path, "working", 3);
    // The CHIRP file is byte-for-byte what `export_channels` would have produced at the shack,
    // with no attribution line — the station does not know which directory the rows came from.
    assert_eq!(
        whole(&path, Format::Chirp, 7),
        propagation::chirp::to_chirp_csv(&channels, 7, "")
    );
    assert_eq!(
        whole(&path, Format::Csv, 7),
        propagation::memchan::to_generic_csv(&channels, "")
    );
    // POSITIVE CONTROL for the cap being carried at all: a different rig cap MUST change the
    // CHIRP text. Without this, a `respond` that ignored `nameCap` would pass the line above.
    assert_ne!(
        whole(&path, Format::Chirp, 7),
        whole(&path, Format::Chirp, 16),
        "the name cap reached neither writer"
    );
    // ...and the clamp is the desktop command's, not a browser's: 0 and 2 both mean 4.
    assert_eq!(
        whole(&path, Format::Chirp, 0),
        propagation::chirp::to_chirp_csv(&channels, 4, "")
    );
    assert_eq!(
        whole(&path, Format::Chirp, 9_999),
        propagation::chirp::to_chirp_csv(&channels, 16, "")
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_long_list_arrives_in_chunks_and_an_index_past_the_end_is_refused() {
    let dir = dir("chunks");
    let path = dir.join("radioprog.json");
    let channels = seed(&path, "working", 900);
    let text = whole(&path, Format::Csv, 7);
    assert_eq!(text, propagation::memchan::to_generic_csv(&channels, ""));
    let first = respond(&path, Format::Csv, 7, 0).unwrap();
    let chunks = first["file"]["chunks"].as_u64().unwrap();
    // The positive control for the chunking itself: this list MUST NOT fit in one reply, or the
    // stitching above proved nothing.
    assert!(chunks > 1, "the fixture fits in one chunk ({chunks})");
    assert_eq!(
        crate::b64_decode(first["base64"].as_str().unwrap())
            .unwrap()
            .len(),
        super::export::CHUNK_BYTES
    );
    assert_eq!(
        respond(&path, Format::Csv, 7, chunks as u32),
        Err("invalidRequest")
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn nothing_to_export_is_said_rather_than_sent_as_an_empty_file() {
    let dir = dir("empty");
    let path = dir.join("radioprog.json");
    // No file at all — a station that has never opened the Program view.
    assert_eq!(
        respond(&path, Format::Chirp, 7, 0).unwrap()["refused"],
        "notFound"
    );
    // A file whose only project is a NAMED one: the working list is what this export is of, and
    // there isn't one.
    seed(&path, "denver-trip", 4);
    assert_eq!(
        respond(&path, Format::Chirp, 7, 0).unwrap()["refused"],
        "notFound"
    );
    // POSITIVE CONTROL: the same fixture under the working id exports. Without it, a `respond`
    // that refused everything would pass both assertions above.
    seed(&path, "working", 4);
    assert!(respond(&path, Format::Chirp, 7, 0).unwrap()["file"].is_object());
    // An empty working list is nothing to export either.
    seed(&path, "working", 0);
    assert_eq!(
        respond(&path, Format::Csv, 7, 0).unwrap()["refused"],
        "notFound"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// ⛔ The `load_radioprog` trap: the desktop loader turns an unreadable file into an empty default.
/// Exporting that would hand the operator a blank CSV and call it their channel list.
#[test]
fn an_unreadable_file_is_a_failure_not_a_blank_export() {
    let dir = dir("unreadable");
    let path = dir.join("radioprog.json");
    std::fs::write(&path, b"{ this is not json").unwrap();
    assert_eq!(
        respond(&path, Format::Csv, 7, 0),
        Err("applicationUnavailable")
    );
    // A file from a future format version is refused the same way, not read as v1.
    std::fs::write(&path, br#"{"version":2,"projects":[]}"#).unwrap();
    assert_eq!(
        respond(&path, Format::Csv, 7, 0),
        Err("applicationUnavailable")
    );
    // A directory where the file should be is refused before it is opened.
    let blocked = dir.join("blocked");
    std::fs::create_dir(&blocked).unwrap();
    assert_eq!(
        respond(&blocked, Format::Csv, 7, 0),
        Err("applicationUnavailable")
    );
    // POSITIVE CONTROL: the same path, readable, exports.
    seed(&path, "working", 2);
    assert!(respond(&path, Format::Csv, 7, 0).unwrap()["file"].is_object());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_format_word_is_closed() {
    assert_eq!(
        serde_json::from_value::<Format>(json!("chirp")).unwrap(),
        Format::Chirp
    );
    assert_eq!(
        serde_json::from_value::<Format>(json!("csv")).unwrap(),
        Format::Csv
    );
    for word in ["adif", "CSV", "", "chirp "] {
        assert!(
            serde_json::from_value::<Format>(json!(word)).is_err(),
            "{word} was admitted as a format"
        );
    }
}
