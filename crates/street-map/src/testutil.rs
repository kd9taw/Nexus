//! Test fixtures, built while the tests run: scratch folders, synthetic vector tiles and map
//! files, a ustar writer, and a local HTTP server with byte ranges and faults on demand.
//!
//! The map data is made up. No OpenStreetMap-derived byte is in this repository or produced by
//! these tests: ODbL material cannot be combined into GPL-licensed files.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::area::{area_ranges, Detail, StreetArea};
use crate::pmtiles::{
    build_directories, build_with_leaves, gzip, tile_id_to_zxy, write_varint, zoom_base, Entry,
    Header, COMPRESSION_GZIP, HEADER_LEN, TILE_TYPE_MVT,
};

/// A folder of its own under the system temp folder, removed when dropped.
pub struct Scratch(PathBuf);

impl Deref for Scratch {
    type Target = PathBuf;
    fn deref(&self) -> &PathBuf {
        &self.0
    }
}

impl AsRef<std::path::Path> for Scratch {
    fn as_ref(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub fn scratch_dir(tag: &str) -> Scratch {
    static N: AtomicU64 = AtomicU64::new(0);
    let p = std::env::temp_dir().join(format!(
        "nexus-street-map-test-{}-{}-{tag}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).unwrap();
    Scratch(p)
}

fn put_bytes(out: &mut Vec<u8>, field: u64, b: &[u8]) {
    write_varint(out, field << 3 | 2);
    write_varint(out, b.len() as u64);
    out.extend_from_slice(b);
}

fn put_varint(out: &mut Vec<u8>, field: u64, v: u64) {
    write_varint(out, field << 3);
    write_varint(out, v);
}

/// A small, valid vector tile: one layer, one point feature tagged with `label`, and `pad`
/// bytes of filler keys that do not compress (so tiles differ in size like real ones).
pub fn mvt_with(layer_name: &str, label: &str, pad: usize, seed: u64) -> Vec<u8> {
    let mut value = Vec::new();
    put_bytes(&mut value, 1, label.as_bytes());
    let mut tags = Vec::new();
    write_varint(&mut tags, 0);
    write_varint(&mut tags, 0);
    let mut geometry = Vec::new();
    // MoveTo × 1, then (dx, dy) = (10, 20) zigzag-encoded.
    for v in [9u64, 20, 40] {
        write_varint(&mut geometry, v);
    }
    let mut feature = Vec::new();
    put_varint(&mut feature, 1, 1);
    put_bytes(&mut feature, 2, &tags);
    put_varint(&mut feature, 3, 1);
    put_bytes(&mut feature, 4, &geometry);
    let mut layer = Vec::new();
    put_varint(&mut layer, 15, 2);
    put_bytes(&mut layer, 1, layer_name.as_bytes());
    put_bytes(&mut layer, 2, &feature);
    put_bytes(&mut layer, 3, b"label");
    put_bytes(&mut layer, 4, &value);
    let mut s = seed | 1;
    let filler: String = (0..pad)
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            (b'a' + (s % 26) as u8) as char
        })
        .collect();
    if pad > 0 {
        put_bytes(&mut layer, 3, filler.as_bytes());
    }
    put_varint(&mut layer, 5, 4096);
    let mut tile = Vec::new();
    put_bytes(&mut tile, 3, &layer);
    tile
}

pub fn mvt_tile(z: u8, x: u32, y: u32) -> Vec<u8> {
    mvt_with("land", &format!("{z}/{x}/{y}"), 0, 1)
}

/// A synthetic map file: every tile from zoom 0 to `max_zoom` exists. Tiles touching a square of
/// `land_km` around the centre each have content of their own; every other tile shares one
/// "ocean" content through run-length entries, as open sea does in a real planet file.
pub struct Synth {
    pub bytes: Vec<u8>,
    /// The stored (gzipped) content of every tile with its own.
    pub land: BTreeMap<u64, Vec<u8>>,
    pub ocean: Vec<u8>,
    pub header: Header,
}

impl Synth {
    pub fn tile(&self, id: u64) -> &[u8] {
        self.land.get(&id).map_or(&self.ocean, |v| v)
    }
}

fn ocean_run(
    from: u64,
    to: u64,
    ocean: &[u8],
    at: &mut Option<u64>,
    data: &mut Vec<u8>,
    out: &mut Vec<Entry>,
) {
    let off = *at.get_or_insert_with(|| {
        let o = data.len() as u64;
        data.extend_from_slice(ocean);
        o
    });
    let mut a = from;
    while a < to {
        let n = (to - a).min(u64::from(u32::MAX));
        out.push(Entry {
            tile_id: a,
            offset: off,
            length: ocean.len() as u32,
            run_length: n as u32,
        });
        a += n;
    }
}

pub fn synth_archive(
    lat: f64,
    lon: f64,
    land_km: u32,
    max_zoom: u8,
    leaf_size: Option<usize>,
    pad: usize,
) -> Synth {
    let land_box = StreetArea {
        lat,
        lon,
        km: land_km,
        detail: Detail::Streets,
    }
    .bbox()
    .unwrap();
    let ocean = gzip(&mvt_with("water", "ocean", 0, 7));
    let (mut data, mut entries, mut land) = (Vec::new(), Vec::new(), BTreeMap::new());
    let mut ocean_at = None;
    let mut cursor = 0u64;
    for id in area_ranges(&land_box, max_zoom).into_iter().flatten() {
        if cursor < id {
            ocean_run(cursor, id, &ocean, &mut ocean_at, &mut data, &mut entries);
        }
        let (z, x, y) = tile_id_to_zxy(id).unwrap();
        let c = gzip(&mvt_with(
            "land",
            &format!("{z}/{x}/{y}"),
            pad + (id % 97) as usize,
            id,
        ));
        entries.push(Entry {
            tile_id: id,
            offset: data.len() as u64,
            length: c.len() as u32,
            run_length: 1,
        });
        data.extend_from_slice(&c);
        land.insert(id, c);
        cursor = id + 1;
    }
    let end = zoom_base(max_zoom + 1);
    if cursor < end {
        ocean_run(cursor, end, &ocean, &mut ocean_at, &mut data, &mut entries);
    }
    let (root, leaves) = match leaf_size {
        Some(n) => build_with_leaves(&entries, n),
        None => build_directories(&entries),
    };
    let meta = gzip(
        br#"{"name":"synthetic test map","attribution":"<a href=\"https://www.openstreetmap.org/copyright\">synthetic</a>","vector_layers":[{"id":"land"},{"id":"water"}]}"#,
    );
    let mut h = Header {
        root_offset: HEADER_LEN as u64,
        root_length: root.len() as u64,
        metadata_length: meta.len() as u64,
        leaf_length: leaves.len() as u64,
        data_length: data.len() as u64,
        addressed_tiles: end,
        tile_entries: entries.len() as u64,
        tile_contents: land.len() as u64 + 1,
        clustered: true,
        internal_compression: COMPRESSION_GZIP,
        tile_compression: COMPRESSION_GZIP,
        tile_type: TILE_TYPE_MVT,
        min_zoom: 0,
        max_zoom,
        min_lon_e7: -1_800_000_000,
        min_lat_e7: -850_511_287,
        max_lon_e7: 1_800_000_000,
        max_lat_e7: 850_511_287,
        ..Header::default()
    };
    h.metadata_offset = h.root_offset + h.root_length;
    h.leaf_offset = h.metadata_offset + h.metadata_length;
    h.data_offset = h.leaf_offset + h.leaf_length;
    let mut bytes = h.to_bytes().to_vec();
    for part in [&root, &meta, &leaves, &data] {
        bytes.extend_from_slice(part);
    }
    Synth {
        bytes,
        land,
        ocean,
        header: h,
    }
}

fn ustar_header(out: &mut Vec<u8>, name: &str, size: usize, kind: u8) {
    let mut h = [0u8; 512];
    h[..name.len()].copy_from_slice(name.as_bytes());
    h[100..108].copy_from_slice(b"0000644\0");
    h[108..116].copy_from_slice(b"0000000\0");
    h[116..124].copy_from_slice(b"0000000\0");
    h[124..136].copy_from_slice(format!("{size:011o}\0").as_bytes());
    h[136..148].copy_from_slice(b"00000000000\0");
    h[148..156].copy_from_slice(b"        ");
    h[156] = kind;
    h[257..263].copy_from_slice(b"ustar\0");
    h[263..265].copy_from_slice(b"00");
    let sum: u64 = h.iter().map(|&b| u64::from(b)).sum();
    h[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
    out.extend_from_slice(&h);
}

/// A ustar archive of `dirs` (as folder entries) and `files`, with its end marker.
pub fn ustar(files: &[(&str, Vec<u8>)], dirs: &[&str]) -> Vec<u8> {
    let mut out = Vec::new();
    for d in dirs {
        ustar_header(&mut out, &format!("{d}/"), 0, b'5');
    }
    for (name, data) in files {
        ustar_header(&mut out, name, data.len(), b'0');
        out.extend_from_slice(data);
        out.resize(out.len().div_ceil(512) * 512, 0);
    }
    out.extend_from_slice(&[0u8; 1024]);
    out
}

/// What the test server does wrong, on purpose.
#[derive(Debug, Clone, Default)]
pub struct Faults {
    /// Answer a ranged request with 200 and the whole file, as a careless proxy does.
    pub ignore_range: bool,
    /// Answer every ranged request with this status.
    pub status: Option<u16>,
    /// After this many ranged requests, answer 404.
    pub gone_after: Option<u64>,
    /// Cut this many ranged responses off halfway through their body.
    pub drop_bodies: u64,
}

#[derive(Clone)]
pub struct Server {
    pub origin: String,
    files: Arc<Mutex<HashMap<String, Arc<Vec<u8>>>>>,
    faults: Arc<Mutex<Faults>>,
    /// Every ranged request served: (first byte, length sent).
    pub ranges: Arc<Mutex<Vec<(u64, u64)>>>,
    /// Every request of any kind.
    pub hits: Arc<AtomicU64>,
}

impl Server {
    pub fn start() -> Server {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", l.local_addr().unwrap());
        let s = Server {
            origin,
            files: Arc::default(),
            faults: Arc::default(),
            ranges: Arc::default(),
            hits: Arc::default(),
        };
        let me = s.clone();
        std::thread::spawn(move || {
            for conn in l.incoming().flatten() {
                let me = me.clone();
                std::thread::spawn(move || me.handle(conn));
            }
        });
        s
    }

    pub fn put(&self, path: &str, bytes: Vec<u8>) {
        self.files
            .lock()
            .unwrap()
            .insert(path.to_string(), Arc::new(bytes));
    }

    pub fn remove(&self, path: &str) {
        self.files.lock().unwrap().remove(path);
    }

    pub fn set_faults(&self, f: Faults) {
        *self.faults.lock().unwrap() = f;
    }

    pub fn take_ranges(&self) -> Vec<(u64, u64)> {
        std::mem::take(&mut *self.ranges.lock().unwrap())
    }

    fn handle(&self, mut s: TcpStream) {
        self.hits.fetch_add(1, Ordering::SeqCst);
        let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            match s.read(&mut byte) {
                Ok(1) => head.push(byte[0]),
                _ => return,
            }
        }
        let head = String::from_utf8_lossy(&head).to_string();
        let path = head.split_whitespace().nth(1).unwrap_or("/").to_string();
        let range = head.lines().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            if !k.trim().eq_ignore_ascii_case("range") {
                return None;
            }
            let (a, b) = v.trim().strip_prefix("bytes=")?.split_once('-')?;
            Some((a.parse::<u64>().ok()?, b.parse::<u64>().ok()?))
        });
        let body = self.files.lock().unwrap().get(&path).cloned();
        let Some(body) = body else {
            return reply(&mut s, "404 Not Found", &[], b"");
        };
        let Some((a, b)) = range else {
            return reply(&mut s, "200 OK", &[], &body);
        };
        let mut f = self.faults.lock().unwrap();
        let served = self.ranges.lock().unwrap().len() as u64;
        if f.ignore_range {
            drop(f);
            return reply(&mut s, "200 OK", &[], &body);
        }
        if let Some(code) = f.status {
            drop(f);
            return reply(&mut s, &format!("{code} Failing"), &[], b"");
        }
        if f.gone_after.is_some_and(|n| served >= n) {
            drop(f);
            return reply(&mut s, "404 Not Found", &[], b"");
        }
        let len = body.len() as u64;
        if a >= len {
            drop(f);
            return reply(&mut s, "416 Range Not Satisfiable", &[], b"");
        }
        let end = b.min(len - 1);
        let part = &body[a as usize..=end as usize];
        let cut = f.drop_bodies > 0;
        if cut {
            f.drop_bodies -= 1;
        }
        drop(f);
        let cr = format!("bytes {a}-{end}/{len}");
        if cut {
            let _ = write!(
                s,
                "HTTP/1.1 206 Partial Content\r\nContent-Range: {cr}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                part.len()
            );
            let _ = s.write_all(&part[..part.len() / 2]);
            return;
        }
        self.ranges.lock().unwrap().push((a, part.len() as u64));
        reply(
            &mut s,
            "206 Partial Content",
            &[("Content-Range", &cr)],
            part,
        );
    }
}

fn reply(s: &mut TcpStream, status: &str, headers: &[(&str, &str)], body: &[u8]) {
    let mut head = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for (k, v) in headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    let _ = s.write_all(head.as_bytes());
    let _ = s.write_all(body);
    let _ = s.flush();
}
