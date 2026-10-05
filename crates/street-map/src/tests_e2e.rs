//! End to end over a local HTTP server: sizing, downloading, resuming, the faults a host or a
//! proxy can produce, and reading a finished pack back. Every map file here is synthetic
//! (`testutil`), so no real map data is involved.

use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Barrier, Mutex};
use std::time::Duration;

use serde_json::Value;

use crate::area::{area_ranges, count_tiles, maidenhead};
use crate::assets::sha256_hex;
use crate::http::{client, FetchError, HttpSource, RangeSource, RetryPolicy};
use crate::plan::{self, BuildInfo, Plan};
use crate::pmtiles::{gunzip, gzip, Header, Reader};
use crate::testutil::{scratch_dir, synth_archive, ustar, Faults, Scratch, Server, Synth};
use crate::verify::verify_file;
use crate::{Detail, ErrorKind, Options, StreetArea, StreetMaps, StreetProgress};

const LAT: f64 = 46.5;
const LON: f64 = -108.5;
const BUILD: &str = "/planet-20261004.pmtiles";
const UPSTREAM: &str = "https://upstream.invalid/20261004.pmtiles";

/// A 50 km square straddling the synthetic land's east edge: tiles of its own on one side, the
/// shared ocean tile on the other, and land tiles outside it interleaved by id, so the pack has
/// shared contents and gaps to merge across.
fn area() -> StreetArea {
    StreetArea {
        lat: LAT,
        lon: LON + 0.6,
        km: 50,
        detail: Detail::Streets,
    }
    .normalized()
}

/// Small requests, fast retries: many requests from a small file, and no waiting.
fn quick() -> Options {
    Options {
        parallel: 4,
        retry: RetryPolicy {
            delays: vec![Duration::from_millis(1); 3],
        },
        progress_every: Duration::ZERO,
        merge_gap: 256,
        max_request: 16 << 10,
    }
}

fn no_progress(_: StreetProgress) {}

struct Fixture {
    server: Server,
    synth: Synth,
    assets: Vec<u8>,
    dir: Scratch,
}

impl Fixture {
    fn new() -> Self {
        let server = Server::start();
        let synth = synth_archive(LAT, LON, 100, 14, Some(64), 40);
        let assets = gzip(&ustar(
            &[
                ("OFL.txt", b"SIL OFL 1.1".to_vec()),
                ("ICONS-LICENSE.txt", b"MIT".to_vec()),
                ("fonts/Noto Sans Regular/0-255.pbf", vec![7; 300]),
                ("sprites/light@2x.png", vec![1; 40]),
            ],
            &["fonts", "sprites"],
        ));
        server.put(BUILD, synth.bytes.clone());
        server.put("/street-assets-1.tar.gz", assets.clone());
        let f = Fixture {
            server,
            synth,
            assets,
            dir: scratch_dir("e2e"),
        };
        f.publish(
            &format!("{}{BUILD}", f.server.origin),
            &sha256_hex(&f.assets),
        );
        f
    }

    fn publish(&self, build_url: &str, assets_sha: &str) {
        let o = &self.server.origin;
        let index = format!(
            r#"{{"schema":1,"build":{{"id":"20261004","date":"2026-10-04","url":"{build_url}","upstream":"{UPSTREAM}"}},
               "assets":{{"url":"{o}/street-assets-1.tar.gz","sha256":"{assets_sha}","bytes":{}}}}}"#,
            self.assets.len()
        );
        self.server.put("/streetmaps.json", index.into_bytes());
    }

    fn maps_in(&self, sub: &str) -> StreetMaps {
        StreetMaps::new(self.dir.join(sub), &self.server.origin)
            .unwrap()
            .with_options(quick())
    }

    fn maps(&self) -> StreetMaps {
        self.maps_in("maps")
    }

    /// The plan the service makes, made directly so a test can see its requests.
    fn plan(&self, a: &StreetArea) -> Plan {
        let build = BuildInfo {
            id: "20261004".into(),
            date: "2026-10-04".into(),
            url: format!("{}{BUILD}", self.server.origin),
            upstream: Some(UPSTREAM.into()),
        };
        let c = client().unwrap();
        plan::plan(
            &HttpSource::new(&c, &build.url),
            &build,
            a,
            &quick(),
            &AtomicBool::new(false),
        )
        .unwrap()
    }

    /// Bytes of ranged reads that fell in the source's tile data (not its header or directories).
    fn data_bytes(&self, ranges: &[(u64, u64)]) -> u64 {
        let start = self.synth.header.data_offset;
        ranges
            .iter()
            .filter(|(a, _)| *a >= start)
            .map(|(_, n)| n)
            .sum()
    }

    fn leaf_bytes(&self, ranges: &[(u64, u64)]) -> u64 {
        let h = &self.synth.header;
        ranges
            .iter()
            .filter(|(a, _)| *a >= h.leaf_offset && *a < h.data_offset)
            .map(|(_, n)| n)
            .sum()
    }
}

/// The request indexes a journal records as done.
fn journal_done(path: &std::path::Path) -> Vec<usize> {
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .skip(2)
        .filter_map(|l| l.split_once(' ')?.0.parse().ok())
        .collect()
}

#[test]
fn nothing_touches_the_network_until_asked_to_size_or_download() {
    let f = Fixture::new();
    let maps = f.maps();
    assert!(maps.packs().is_empty());
    assert!(maps.unfinished().is_empty());
    assert!(!maps.cancel());
    assert!(maps.read("20261004-streets-50km-0n0e", 0, 16).is_err());
    assert!(maps.asset("OFL.txt").is_err());
    assert!(maps.remove("20261004-streets-50km-0n0e").is_err());
    assert_eq!(f.server.hits.load(Ordering::SeqCst), 0);
    maps.size(&area()).unwrap();
    assert!(
        f.server.hits.load(Ordering::SeqCst) > 0,
        "control: sizing does reach the host"
    );
}

#[test]
fn sizing_gives_the_exact_tiles_and_bytes_and_reads_no_tile_data() {
    let f = Fixture::new();
    let maps = f.maps();
    let a = area();
    let size = maps.size(&a).unwrap();
    let reads = f.server.take_ranges();
    assert_eq!(
        f.data_bytes(&reads),
        0,
        "sizing reads directories, never tiles"
    );
    let small = f.leaf_bytes(&reads);
    assert!(
        small > 0 && small < f.synth.header.leaf_length / 2,
        "{small} of {}",
        f.synth.header.leaf_length
    );
    maps.size(&StreetArea { km: 100, ..a }).unwrap();
    assert!(
        f.leaf_bytes(&f.server.take_ranges()) > small,
        "control: a bigger square reads more"
    );

    // The tiles: every one touching the square, zoom 0 to 14.
    let bbox = a.bbox().unwrap();
    let ranges = area_ranges(&bbox, 14);
    assert_eq!(size.tiles, count_tiles(&ranges));
    assert_eq!(size.bbox, bbox.to_array());
    assert_eq!(
        (size.min_zoom, size.max_zoom, size.detail),
        (0, 14, Detail::Streets)
    );
    assert_eq!(size.pack_id, "20261004-streets-50km-4650n10790w");
    assert_eq!(
        (size.build_id.as_str(), size.data_date.as_str()),
        ("20261004", "2026-10-04")
    );
    assert_eq!(size.assets_bytes, f.assets.len() as u64);
    assert!(size.enough_space && !size.installed && size.resume_bytes == 0);

    // The bytes: each distinct content of those tiles once (the shared ocean tile once), plus
    // the pack's own header, directories and metadata.
    let distinct: BTreeSet<&[u8]> = ranges
        .iter()
        .cloned()
        .flatten()
        .map(|id| f.synth.tile(id))
        .collect();
    assert!(
        distinct.contains(f.synth.ocean.as_slice()),
        "the square reaches the ocean"
    );
    let tile_bytes: u64 = distinct.iter().map(|c| c.len() as u64).sum();
    let plan = f.plan(&a);
    assert_eq!(plan.header.data_length, tile_bytes);
    assert_eq!(plan.header.tile_contents, distinct.len() as u64);
    assert_eq!(size.bytes, plan.prefix.len() as u64 + tile_bytes);
    assert_eq!(size.download_bytes, plan.download_bytes());
    assert_eq!(size.requests, plan.requests.len());
    assert!(
        size.download_bytes > tile_bytes,
        "merged gaps are fetched and dropped"
    );
}

#[test]
fn a_download_reads_back_tile_for_tile_through_the_packs_own_reader() {
    let f = Fixture::new();
    let maps = f.maps();
    let a = area();
    let events = Mutex::new(Vec::new());
    let pack = maps
        .download(&a, &|p| events.lock().unwrap().push(p))
        .unwrap();

    let file = fs::read(maps.dir().pack(&pack.id)).unwrap();
    assert_eq!(pack.bytes, file.len() as u64);
    assert_eq!(
        pack.sha256,
        sha256_hex(&file),
        "the recorded SHA-256 is the file's own"
    );
    assert_eq!(pack.name, format!("{} 50 km", maidenhead(a.lat, a.lon)));
    assert_eq!(
        (pack.detail, pack.min_zoom, pack.max_zoom),
        (Detail::Streets, 0, 14)
    );
    assert_eq!(pack.data_date, "2026-10-04");
    assert_eq!(maps.packs(), vec![pack.clone()]);
    assert!(!maps.dir().part(&pack.id).exists() && !maps.dir().journal(&pack.id).exists());

    // Read back through the same capped reads the webview makes.
    let reader = Reader::open(|off, len| maps.read(&pack.id, off, len as u32)).unwrap();
    let ranges = area_ranges(&a.bbox().unwrap(), 14);
    for id in ranges.iter().cloned().flatten() {
        assert_eq!(
            reader.tile(id).unwrap().as_deref(),
            Some(f.synth.tile(id)),
            "tile {id}"
        );
    }
    let outside = f
        .synth
        .land
        .keys()
        .find(|id| !ranges.iter().any(|r| r.contains(id)))
        .unwrap();
    assert_eq!(
        reader.tile(*outside).unwrap(),
        None,
        "a tile outside the square is not in the pack"
    );

    // The notice travels inside the file.
    let h = reader.header();
    let meta = maps
        .read(&pack.id, h.metadata_offset, h.metadata_length as u32)
        .unwrap();
    let meta: Value = serde_json::from_slice(&gunzip(&meta, 1 << 20).unwrap()).unwrap();
    assert_eq!(meta["license"], "ODbL-1.0");
    assert_eq!(
        meta["license_uri"],
        "https://opendatacommons.org/licenses/odbl/1-0/"
    );
    assert_eq!(meta["source"], format!("{}{BUILD}", f.server.origin));
    assert_eq!(meta["source_date"], "2026-10-04");
    assert_eq!(meta["source_upstream"], UPSTREAM);
    assert_eq!(meta["extract_maxzoom"], 14);
    assert_eq!(meta["name"], "Nexus street map");
    assert_eq!(
        meta["vector_layers"][0]["id"], "land",
        "the source's layer list is kept"
    );
    assert!(meta["attribution"]
        .as_str()
        .unwrap()
        .contains("openstreetmap.org/copyright"));

    assert!(fs::read_to_string(maps.dir().readme())
        .unwrap()
        .contains("ODbL"));
    assert_eq!(maps.asset("OFL.txt").unwrap(), b"SIL OFL 1.1");
    assert_eq!(
        maps.asset("fonts/Noto Sans Regular/0-255.pbf").unwrap(),
        vec![7; 300]
    );

    // Progress: fonts first, tiles to the end, then every tile checked.
    let ev = events.into_inner().unwrap();
    assert!(
        matches!(ev[0], StreetProgress::Assets { .. }),
        "{:?}",
        ev[0]
    );
    assert!(ev
        .iter()
        .any(|p| matches!(p, StreetProgress::Tiles { done, total, .. } if done == total)));
    assert!(matches!(ev.last(), Some(StreetProgress::Verifying { done, total }) if done == total));
    let last_tiles = ev
        .iter()
        .rev()
        .find(|p| matches!(p, StreetProgress::Tiles { .. }))
        .unwrap();
    let wire = serde_json::to_value(last_tiles).unwrap();
    assert_eq!(wire["phase"], "tiles");
    assert!(
        wire.get("bytesPerSec").is_some() && wire.get("etaSecs").is_some(),
        "{wire}"
    );

    // A second Download of the same area is the installed pack, fetched again from nowhere.
    f.server.take_ranges();
    assert_eq!(maps.download(&a, &no_progress).unwrap(), pack);
    assert_eq!(f.data_bytes(&f.server.take_ranges()), 0);
    assert!(maps.size(&a).unwrap().installed);
}

#[test]
fn a_killed_download_resumes_with_only_what_it_lacked() {
    let f = Fixture::new();
    let a = area();
    let plan = f.plan(&a);
    assert!(
        plan.requests.len() >= 8,
        "enough requests to stop part way: {}",
        plan.requests.len()
    );

    // The reference: one uninterrupted download fetches every request once.
    let whole = f.maps_in("reference");
    f.server.take_ranges();
    let reference = whole.download(&a, &no_progress).unwrap();
    assert_eq!(f.data_bytes(&f.server.take_ranges()), plan.download_bytes());
    let reference = fs::read(whole.dir().pack(&reference.id)).unwrap();

    // Stopped at about 40%.
    let maps = f.maps();
    let stop_at = plan.download_bytes() * 2 / 5;
    let e = maps
        .download(&a, &|p| {
            if matches!(p, StreetProgress::Tiles { done, .. } if done >= stop_at) {
                maps.cancel();
            }
        })
        .unwrap_err();
    assert_eq!(e.kind, ErrorKind::Cancelled);
    let journal = maps.dir().journal(&plan.pack_id);
    let done = journal_done(&journal);
    assert!(
        !done.is_empty() && done.len() < plan.requests.len(),
        "{} of {}",
        done.len(),
        plan.requests.len()
    );

    // What a hard kill can leave besides: a journal line cut off mid-write, and garbage where
    // an unfinished request was being written.
    let missing: Vec<usize> = (0..plan.requests.len())
        .filter(|i| !done.contains(i))
        .collect();
    write!(
        OpenOptions::new().append(true).open(&journal).unwrap(),
        "{}",
        missing[0]
    )
    .unwrap();
    let seg = &plan.requests[missing[0]].segments[0];
    let part = OpenOptions::new()
        .write(true)
        .open(maps.dir().part(&plan.pack_id))
        .unwrap();
    crate::store::write_all_at(
        &part,
        &vec![0xff; seg.len as usize],
        plan.prefix.len() as u64 + seg.dst,
    )
    .unwrap();
    drop(part);
    let listed = maps.unfinished();
    assert_eq!(listed.len(), 1);
    assert_eq!(
        (listed[0].pack_id.as_str(), listed[0].area),
        (plan.pack_id.as_str(), a)
    );
    assert!(listed[0].done_bytes > 0 && listed[0].done_bytes < listed[0].total_bytes);
    assert_eq!(maps.size(&a).unwrap().resume_bytes, listed[0].done_bytes);

    f.server.take_ranges();
    let pack = maps.download(&a, &no_progress).unwrap();
    let expect: u64 = missing.iter().map(|&i| plan.requests[i].len).sum();
    assert_eq!(
        f.data_bytes(&f.server.take_ranges()),
        expect,
        "only what the journal lacked is fetched again"
    );
    assert_eq!(
        fs::read(maps.dir().pack(&pack.id)).unwrap(),
        reference,
        "byte for byte the uninterrupted download"
    );
    assert!(maps.unfinished().is_empty());

    // Control: a journal from a different plan is not trusted, and everything is fetched again.
    maps.remove(&pack.id).unwrap();
    let stop = AtomicBool::new(false);
    let _ = maps.download(&a, &|p| {
        if matches!(p, StreetProgress::Tiles { done, .. } if done >= stop_at)
            && !stop.swap(true, Ordering::SeqCst)
        {
            maps.cancel();
        }
    });
    let text = fs::read_to_string(&journal).unwrap();
    let forged = text.replacen(&plan.fingerprint, &"0".repeat(64), 1);
    assert_ne!(forged, text);
    fs::write(&journal, forged).unwrap();
    f.server.take_ranges();
    maps.download(&a, &no_progress).unwrap();
    assert_eq!(f.data_bytes(&f.server.take_ranges()), plan.download_bytes());
}

#[test]
fn a_proxy_that_ignores_byte_ranges_stops_the_job_and_keeps_its_partial() {
    let f = Fixture::new();
    let maps = f.maps();
    let a = area();
    let id = maps.size(&a).unwrap().pack_id;
    f.server.set_faults(Faults {
        ignore_range: true,
        ..Faults::default()
    });
    assert_eq!(
        maps.download(&a, &no_progress).unwrap_err().kind,
        ErrorKind::RangeIgnored
    );
    assert!(
        maps.dir().part(&id).exists() && maps.dir().journal(&id).exists(),
        "kept for a later try"
    );
    assert!(maps.packs().is_empty());
    assert_eq!(
        maps.size(&a).unwrap_err().kind,
        ErrorKind::RangeIgnored,
        "sizing stops the same way"
    );
    // Control: without the proxy the same download completes.
    f.server.set_faults(Faults::default());
    maps.download(&a, &no_progress).unwrap();
}

#[test]
fn a_retired_build_stops_the_job_and_discards_its_partial() {
    let f = Fixture::new();
    let maps = f.maps();
    let a = area();
    let id = maps.size(&a).unwrap().pack_id;
    f.server.take_ranges();
    f.server.set_faults(Faults {
        gone_after: Some(2),
        ..Faults::default()
    });
    assert_eq!(
        maps.download(&a, &no_progress).unwrap_err().kind,
        ErrorKind::BuildGone
    );
    assert!(
        !maps.dir().part(&id).exists() && !maps.dir().journal(&id).exists(),
        "nothing of it is kept"
    );
    f.server.set_faults(Faults::default());
    f.server.remove(BUILD);
    assert_eq!(
        maps.size(&a).unwrap_err().kind,
        ErrorKind::BuildGone,
        "sizing a retired build says so"
    );
}

#[test]
fn a_build_that_changes_after_sizing_is_caught_at_the_first_request() {
    let f = Fixture::new();
    let maps = f.maps();
    let a = area();
    let id = maps.size(&a).unwrap().pack_id;
    // The same name now serves a longer file: an immutable build never does this.
    let mut changed = f.synth.bytes.clone();
    changed.extend_from_slice(&[0; 4096]);
    f.server.put(BUILD, changed);
    f.server.take_ranges();
    assert_eq!(
        maps.download(&a, &no_progress).unwrap_err().kind,
        ErrorKind::BuildChanged
    );
    assert!(
        f.data_bytes(&f.server.take_ranges()) <= quick().max_request * quick().parallel as u64,
        "stopped at once"
    );
    assert!(!maps.dir().part(&id).exists() && !maps.dir().journal(&id).exists());
    // Control: sized again, it plans against the file as it is now.
    f.server.put(BUILD, f.synth.bytes.clone());
    maps.size(&a).unwrap();
    maps.download(&a, &no_progress).unwrap();
}

#[test]
fn dropped_connections_are_retried_and_a_failing_host_pauses_the_job() {
    let f = Fixture::new();
    let maps = f.maps();
    let a = area();
    maps.size(&a).unwrap();
    f.server.set_faults(Faults {
        drop_bodies: 3,
        ..Faults::default()
    });
    let retries = AtomicUsize::new(0);
    let pack = maps
        .download(&a, &|p| {
            if matches!(p, StreetProgress::Retrying { .. }) {
                retries.fetch_add(1, Ordering::SeqCst);
            }
        })
        .unwrap();
    assert_eq!(retries.load(Ordering::SeqCst), 3);
    maps.remove(&pack.id).unwrap();

    // A host failing every request: retried, then paused, the partial kept, then resumed.
    maps.size(&a).unwrap();
    f.server.set_faults(Faults {
        status: Some(503),
        ..Faults::default()
    });
    assert_eq!(
        maps.download(&a, &no_progress).unwrap_err().kind,
        ErrorKind::Paused
    );
    assert!(maps.dir().part(&pack.id).exists() && maps.dir().journal(&pack.id).exists());
    assert_eq!(
        maps.size(&a).unwrap_err().kind,
        ErrorKind::Network,
        "while sizing it is a network failure"
    );
    f.server.set_faults(Faults::default());
    assert_eq!(maps.download(&a, &no_progress).unwrap().sha256, pack.sha256);
}

#[test]
fn truncated_or_corrupt_packs_are_rejected() {
    let f = Fixture::new();
    let maps = f.maps();
    let a = area();
    let pack = maps.download(&a, &no_progress).unwrap();
    let good = fs::read(maps.dir().pack(&pack.id)).unwrap();
    let check = |bytes: &[u8]| {
        let p = f.dir.join("check.pmtiles");
        fs::write(&p, bytes).unwrap();
        verify_file(
            &File::open(&p).unwrap(),
            bytes.len() as u64,
            &AtomicBool::new(false),
            &mut |_, _| {},
        )
        .map(|_| ())
    };
    assert!(check(&good).is_ok(), "control: the pack itself passes");
    for cut in [good.len() - 1, good.len() / 2, 300, 126, 0] {
        assert_eq!(
            check(&good[..cut]).unwrap_err().kind,
            ErrorKind::InvalidArchive,
            "cut at {cut}"
        );
    }
    let h = Header::parse(&good).unwrap();
    let flipped = |at: u64, bit: u8| {
        let mut b = good.clone();
        b[at as usize] ^= bit;
        check(&b)
    };
    // A byte inside a tile's compressed data. (Not just any byte of the tile: gzip's header
    // carries a time and an OS byte that no checksum covers, and changing those changes no
    // tile.)
    let ranges = area_ranges(&a.bbox().unwrap(), 14);
    let tile = f
        .synth
        .land
        .iter()
        .find(|(id, _)| ranges.iter().any(|r| r.contains(id)))
        .unwrap()
        .1;
    let at = good
        .windows(tile.len())
        .position(|w| w == tile.as_slice())
        .unwrap() as u64;
    assert!(
        flipped(at + tile.len() as u64 / 2, 0x40).is_err(),
        "a tile byte"
    );
    assert!(
        flipped(at + tile.len() as u64 - 1, 0x01).is_err(),
        "a tile's length trailer"
    );
    assert!(
        flipped(at + tile.len() as u64 - 6, 0x01).is_err(),
        "a tile's CRC"
    );
    assert!(
        flipped(h.root_offset + h.root_length / 2, 0x01).is_err(),
        "a directory byte"
    );
    assert!(flipped(72, 0x01).is_err(), "the header's tile count");

    // A tile damaged on the host: every request completes, the check fails, and the partial
    // is removed so the next attempt starts clean.
    maps.remove(&pack.id).unwrap();
    let mut evil = f.synth.bytes.clone();
    let at = evil
        .windows(tile.len())
        .position(|w| w == tile.as_slice())
        .unwrap();
    evil[at + tile.len() / 2] ^= 0x40;
    f.server.put(BUILD, evil);
    assert_eq!(
        maps.download(&a, &no_progress).unwrap_err().kind,
        ErrorKind::InvalidArchive
    );
    assert!(!maps.dir().part(&pack.id).exists() && !maps.dir().journal(&pack.id).exists());
    assert!(maps.packs().is_empty());
}

#[test]
fn a_full_disk_refuses_the_download_before_anything_is_fetched() {
    let f = Fixture::new();
    let a = area();
    let full = f.maps().with_free_space(|_| Ok(1000));
    let size = full.size(&a).unwrap();
    assert_eq!((size.free_bytes, size.enough_space), (Some(1000), false));
    f.server.take_ranges();
    assert_eq!(
        full.download(&a, &no_progress).unwrap_err().kind,
        ErrorKind::DiskSpace
    );
    assert_eq!(f.data_bytes(&f.server.take_ranges()), 0);
    assert!(!full.dir().assets().exists(), "not even the fonts");
    // Control: with room, the same download goes ahead.
    let roomy = f.maps().with_free_space(|_| Ok(u64::MAX));
    assert!(roomy.size(&a).unwrap().enough_space);
    roomy.download(&a, &no_progress).unwrap();
}

#[test]
fn one_download_at_a_time_and_the_one_running_cannot_be_removed() {
    let f = Fixture::new();
    let maps = f.maps();
    let a = area();
    let id = maps.size(&a).unwrap().pack_id;
    let gate = Barrier::new(2);
    let first = AtomicBool::new(true);
    std::thread::scope(|s| {
        let running = s.spawn(|| {
            maps.download(&a, &|p| {
                if matches!(p, StreetProgress::Tiles { .. }) && first.swap(false, Ordering::SeqCst)
                {
                    gate.wait();
                    gate.wait();
                }
            })
        });
        gate.wait();
        assert_eq!(
            maps.download(&a, &no_progress).unwrap_err().kind,
            ErrorKind::Busy
        );
        assert_eq!(maps.remove(&id).unwrap_err().kind, ErrorKind::Busy);
        assert!(maps.cancel());
        gate.wait();
        assert_eq!(
            running.join().unwrap().unwrap_err().kind,
            ErrorKind::Cancelled
        );
    });
    assert!(!maps.cancel(), "nothing runs now");
    let left = maps.unfinished();
    assert_eq!((left.len(), left[0].pack_id.as_str()), (1, id.as_str()));
    assert!(
        maps.remove(&id).unwrap() > 0,
        "Remove clears what a cancelled download left"
    );
    assert!(maps.unfinished().is_empty());
    assert_eq!(maps.remove(&id).unwrap_err().kind, ErrorKind::UnknownPack);
}

#[test]
fn remove_frees_the_pack_and_reads_of_it_stop() {
    let f = Fixture::new();
    let maps = f.maps();
    let pack = maps.download(&area(), &no_progress).unwrap();
    assert_eq!(maps.read(&pack.id, 0, 7).unwrap(), b"PMTiles");
    assert_eq!(maps.remove(&pack.id).unwrap(), pack.bytes);
    assert!(maps.packs().is_empty());
    assert_eq!(
        maps.read(&pack.id, 0, 7).unwrap_err().kind,
        ErrorKind::UnknownPack
    );
    assert_eq!(
        maps.remove("../packs").unwrap_err().kind,
        ErrorKind::BadRequest
    );
}

#[test]
fn an_update_is_offered_for_a_pack_from_an_older_build_and_never_fetched() {
    let f = Fixture::new();
    let maps = f.maps();
    let pack = maps.download(&area(), &no_progress).unwrap();
    assert!(
        maps.updates().unwrap().is_empty(),
        "the pack is from the current build"
    );

    // The host moves to a newer build (here the same bytes under a new name).
    f.server
        .put("/planet-20270104.pmtiles", f.synth.bytes.clone());
    let o = &f.server.origin;
    let index = format!(
        r#"{{"schema":1,"build":{{"id":"20270104","date":"2027-01-04","url":"{o}/planet-20270104.pmtiles"}},
           "assets":{{"url":"{o}/street-assets-1.tar.gz","sha256":"{}"}}}}"#,
        sha256_hex(&f.assets)
    );
    f.server.put("/streetmaps.json", index.into_bytes());
    f.server.take_ranges();
    let offered = maps.updates().unwrap();
    assert_eq!(offered.len(), 1);
    assert_eq!(
        (offered[0].pack_id.as_str(), offered[0].area),
        (pack.id.as_str(), area())
    );
    assert_eq!(
        (
            offered[0].data_date.as_str(),
            offered[0].new_data_date.as_str()
        ),
        ("2026-10-04", "2027-01-04")
    );
    assert!(
        f.server.take_ranges().is_empty(),
        "offering reads the index only"
    );

    // Taking it: the same square from the new build is a new pack beside the old one.
    let newer = maps.download(&offered[0].area, &no_progress).unwrap();
    assert_eq!(newer.id, "20270104-streets-50km-4650n10790w");
    assert_eq!(newer.data_date, "2027-01-04");
    assert_eq!(maps.packs().len(), 2);
    maps.remove(&offered[0].pack_id).unwrap();
    assert!(maps.updates().unwrap().is_empty());
}

#[test]
fn a_bad_index_or_font_archive_stops_before_any_map_data_moves() {
    let f = Fixture::new();
    let a = area();
    let maps = f.maps();
    f.publish(
        "https://elsewhere.invalid/planet.pmtiles",
        &sha256_hex(&f.assets),
    );
    assert_eq!(maps.size(&a).unwrap_err().kind, ErrorKind::InvalidManifest);

    f.publish(&format!("{}{BUILD}", f.server.origin), &"0".repeat(64));
    f.server.take_ranges();
    assert_eq!(
        maps.download(&a, &no_progress).unwrap_err().kind,
        ErrorKind::InvalidArchive
    );
    assert_eq!(f.data_bytes(&f.server.take_ranges()), 0);
    assert!(!maps.dir().assets().exists());
}

#[test]
fn a_range_read_accepts_only_the_exact_partial_response() {
    let server = Server::start();
    let body: Vec<u8> = (0..=255u8).cycle().take(5000).collect();
    server.put("/f", body);
    let c = client().unwrap();
    let cancel = AtomicBool::new(false);
    let src = HttpSource::new(&c, &format!("{}/f", server.origin));
    assert_eq!(src.read(10, 5, &cancel).unwrap(), [10, 11, 12, 13, 14]);
    assert_eq!(
        src.read(4990, 100, &cancel).unwrap().len(),
        10,
        "short only where the file ends"
    );
    assert_eq!(src.total_len(), Some(5000));

    let fault = |f: Faults| {
        server.set_faults(f);
        src.read(0, 4000, &cancel)
    };
    assert!(matches!(
        fault(Faults {
            ignore_range: true,
            ..Faults::default()
        }),
        Err(FetchError::RangeIgnored(_))
    ));
    assert!(matches!(
        fault(Faults {
            drop_bodies: 1,
            ..Faults::default()
        }),
        Err(FetchError::Transient(_))
    ));
    assert!(matches!(
        fault(Faults {
            status: Some(503),
            ..Faults::default()
        }),
        Err(FetchError::Transient(_))
    ));
    assert!(matches!(
        fault(Faults {
            status: Some(403),
            ..Faults::default()
        }),
        Err(FetchError::Fatal(_))
    ));
    assert_eq!(fault(Faults::default()).unwrap().len(), 4000, "control");

    server.put("/f", vec![0; 6000]);
    assert!(
        matches!(src.read(0, 5, &cancel), Err(FetchError::Changed(_))),
        "the file changed size"
    );
    server.remove("/f");
    assert_eq!(src.read(0, 5, &cancel), Err(FetchError::BuildGone));
    cancel.store(true, Ordering::SeqCst);
    assert_eq!(src.read(0, 5, &cancel), Err(FetchError::Cancelled));
}

/// THE BENCH AID. A pack downloaded here stands in for the file the operator already has: it is a
/// real PMTiles vector map, made from synthetic data.
#[test]
fn a_map_file_the_operator_has_installs_as_a_pack_checked_like_a_download() {
    let f = Fixture::new();
    let got = f.maps().download(&area(), &no_progress).unwrap();
    let file = f.dir.join("my-area.pmtiles");
    fs::copy(f.maps().dir().pack(&got.id), &file).unwrap();

    let bench = f.maps_in("bench");
    let crate::StreetInstalled::Pack { pack } = bench.install_file(&file).unwrap() else {
        panic!("a PMTiles file installs as a pack")
    };
    assert!(pack.id.starts_with("local-") && crate::store::valid_pack_id(&pack.id));
    assert_eq!(pack.bytes, fs::metadata(&file).unwrap().len());
    assert_eq!(pack.sha256, got.sha256, "the same bytes");
    assert_eq!(
        (pack.min_zoom, pack.max_zoom, pack.detail),
        (got.min_zoom, got.max_zoom, Detail::Streets)
    );
    for (a, b) in pack.bbox.iter().zip(got.bbox) {
        assert!((a - b).abs() < 1e-6, "{:?} vs {:?}", pack.bbox, got.bbox);
    }
    assert_eq!(
        pack.data_date, "2026-10-04",
        "the date the file's metadata states"
    );
    assert_eq!(bench.packs(), vec![pack.clone()]);
    assert_eq!(bench.read(&pack.id, 0, 7).unwrap(), b"PMTiles");

    // The same file twice is one pack.
    bench.install_file(&file).unwrap();
    assert_eq!(bench.packs().len(), 1);
    // The host serves another build, and a file is never offered an update from it.
    assert!(bench.updates().unwrap().is_empty());
    // It goes like any pack.
    assert_eq!(bench.remove(&pack.id).unwrap(), pack.bytes);
    assert!(bench.packs().is_empty());
}

#[test]
fn a_fonts_and_icons_file_installs_the_assets_the_map_reads() {
    let f = Fixture::new();
    let file = f.dir.join("street-assets.tar.gz");
    fs::write(&file, &f.assets).unwrap();
    let maps = f.maps();
    assert!(
        maps.asset("fonts/Noto Sans Regular/0-255.pbf").is_err(),
        "control: none yet"
    );
    assert_eq!(
        maps.install_file(&file).unwrap(),
        crate::StreetInstalled::Assets
    );
    assert_eq!(
        maps.asset("fonts/Noto Sans Regular/0-255.pbf").unwrap(),
        vec![7; 300]
    );
    assert!(
        maps.dir().readme().is_file(),
        "the folder carries its notice"
    );
}

#[test]
fn a_file_that_is_not_a_whole_street_map_is_refused_and_nothing_is_kept() {
    let f = Fixture::new();
    let got = f.maps().download(&area(), &no_progress).unwrap();
    let good = fs::read(f.maps().dir().pack(&got.id)).unwrap();
    // A byte inside one of the pack's own tiles (as in `truncated_or_corrupt_packs_are_rejected`).
    let ranges = area_ranges(&area().bbox().unwrap(), 14);
    let tile = f
        .synth
        .land
        .iter()
        .find(|(id, _)| ranges.iter().any(|r| r.contains(id)))
        .unwrap()
        .1;
    let at = good
        .windows(tile.len())
        .position(|w| w == tile.as_slice())
        .expect("the tile is in the pack");
    let mut flipped = good.clone();
    flipped[at + tile.len() / 2] ^= 0x40;
    let bench = f.maps_in("bench");
    for (name, bytes) in [
        ("cut.pmtiles", good[..good.len() / 2].to_vec()),
        ("notes.txt", b"not a map".to_vec()),
        ("junk.tar.gz", gzip(b"not a tar archive")),
    ] {
        let file = f.dir.join(name);
        fs::write(&file, &bytes).unwrap();
        assert_eq!(
            bench.install_file(&file).unwrap_err().kind,
            ErrorKind::InvalidArchive,
            "{name}"
        );
    }
    let file = f.dir.join("flipped.pmtiles");
    fs::write(&file, &flipped).unwrap();
    assert_eq!(
        bench.install_file(&file).unwrap_err().kind,
        ErrorKind::InvalidArchive,
        "a damaged tile"
    );
    assert!(bench.packs().is_empty());
    let kept: Vec<_> = fs::read_dir(bench.dir().root())
        .map(|d| d.filter_map(|e| e.ok()).map(|e| e.file_name()).collect())
        .unwrap_or_default();
    assert!(
        kept.iter()
            .all(|n| !n.to_string_lossy().starts_with("street-")),
        "{kept:?}"
    );
}
