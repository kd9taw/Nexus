//! Sizing an area, and the finished pack worked out before a byte of tile data moves.
//!
//! Sizing reads the hosted file's header and root directory (one request), then only the leaf
//! directories whose tile ids meet the area: a few MB of a 138 GB file. From the entries found
//! it fixes everything about the pack: which source bytes are fetched (merged into requests),
//! where each lands, the pack's directories, metadata and header. So the exact size is known
//! before Download, and a resumed job can tell from a fingerprint whether its half-written file
//! still belongs to the same plan.
//!
//! A content shared by many tiles (open sea, empty land) is stored once, as in the source.
//! Contents are placed in the order the pack's own tiles first use them, so every pack is
//! clustered, and fetched in source order, so requests still merge across small gaps.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::atomic::AtomicBool;

use ring::digest::{Context, SHA256};
use serde_json::{json, Map, Value};

use crate::area::{area_ranges, BBox, StreetArea};
use crate::error::{ErrorKind, StreetError};
use crate::http::{with_retry, RangeSource};
use crate::pmtiles::{
    build_directories, check_order, decode_directory, decompress_internal, gzip, tile_id_to_zxy,
    Entry, Header, COMPRESSION_GZIP, COMPRESSION_NONE, HEADER_LEN, ROOT_LIMIT, TILE_TYPE_MVT,
};
use crate::store::pack_id;
use crate::Options;

const METADATA_LIMIT: u64 = 1 << 20;
const MAX_LEAF_DEPTH: u8 = 3;

/// The credit every pack carries, linked as the OpenStreetMap Foundation asks.
pub const OSM_ATTRIBUTION: &str =
    "<a href=\"https://www.openstreetmap.org/copyright\" target=\"_blank\">© OpenStreetMap</a>";
pub const ODBL_URI: &str = "https://opendatacommons.org/licenses/odbl/1-0/";
/// The notice the ODbL itself suggests (section 4.3).
pub const ODBL_NOTICE: &str = "Contains information from OpenStreetMap, which is made available \
     here under the Open Database License (ODbL).";
pub const LANDCOVER_CREDIT: &str = "Landcover at zoom 0 to 7: © ESA WorldCover project 2020 / \
     Contains modified Copernicus Sentinel data (2020) processed by ESA WorldCover consortium. \
     CC BY 4.0, https://creativecommons.org/licenses/by/4.0/";

/// Which build a pack is cut from, as the host's index names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildInfo {
    pub id: String,
    /// YYYY-MM-DD.
    pub date: String,
    pub url: String,
    /// Where the host's copy came from, when the index says.
    pub upstream: Option<String>,
}

/// Source bytes `src..src+len` land at `dst` in the pack's tile data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub src: u64,
    pub len: u64,
    pub dst: u64,
}

/// One range request: `src..src+len` of the hosted file, of which `segments` are kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub src: u64,
    pub len: u64,
    pub segments: Vec<Segment>,
}

#[derive(Debug, Clone)]
pub struct Plan {
    pub area: StreetArea,
    pub bbox: BBox,
    pub build: BuildInfo,
    pub pack_id: String,
    /// The pack's own header.
    pub header: Header,
    /// Everything before the pack's tile data: header, root directory, metadata, leaves.
    pub prefix: Vec<u8>,
    pub requests: Vec<Request>,
    /// SHA-256 (hex) of the plan: the source header and URL, the prefix and every request.
    pub fingerprint: String,
    /// The hosted file's size when it was sized. A download holds every response to it.
    pub source_len: Option<u64>,
}

impl Plan {
    /// The finished pack's size.
    pub fn total_len(&self) -> u64 {
        self.prefix.len() as u64 + self.header.data_length
    }

    /// What the download transfers, including the merged gaps.
    pub fn download_bytes(&self) -> u64 {
        self.requests.iter().map(|r| r.len).sum()
    }
}

/// The parts of `span` inside `want` (sorted and merged).
fn overlapping(want: &[Range<u64>], span: Range<u64>) -> impl Iterator<Item = Range<u64>> + '_ {
    let first = want.partition_point(|w| w.end <= span.start);
    want[first..]
        .iter()
        .take_while(move |w| w.start < span.end)
        .map(move |w| w.start.max(span.start)..w.end.min(span.end))
}

type ReadFn<'a> = dyn Fn(u64, u64) -> Result<Vec<u8>, StreetError> + 'a;

/// Every tile entry in `dir` (and the leaves below it) that meets `want`, clipped to it.
fn collect(
    read: &ReadFn,
    h: &Header,
    dir: &[Entry],
    want: &[Range<u64>],
    end: u64,
    depth: u8,
    out: &mut Vec<Entry>,
) -> Result<(), StreetError> {
    for (i, e) in dir.iter().enumerate() {
        if e.run_length > 0 {
            let span = e.tile_id..e.tile_id.saturating_add(u64::from(e.run_length));
            for w in overlapping(want, span) {
                out.push(Entry {
                    tile_id: w.start,
                    run_length: (w.end - w.start) as u32,
                    ..*e
                });
            }
            continue;
        }
        // A leaf covers the ids up to the next entry.
        let next = dir.get(i + 1).map_or(end, |n| n.tile_id.min(end));
        if overlapping(want, e.tile_id..next).next().is_none() {
            continue;
        }
        if depth >= MAX_LEAF_DEPTH {
            return Err(StreetError::archive("leaf directories nest too deep"));
        }
        if e.offset.saturating_add(u64::from(e.length)) > h.leaf_length {
            return Err(StreetError::archive(
                "a leaf directory lies outside its section",
            ));
        }
        let raw = read(h.leaf_offset + e.offset, u64::from(e.length))?;
        if raw.len() != e.length as usize {
            return Err(StreetError::archive(
                "the file ends inside a leaf directory",
            ));
        }
        let leaf = decode_directory(&decompress_internal(h.internal_compression, &raw)?)?;
        collect(read, h, &leaf, want, next, depth + 1, out)?;
    }
    Ok(())
}

fn check_source(h: &Header, max_zoom: u8) -> Result<(), StreetError> {
    if ![COMPRESSION_NONE, COMPRESSION_GZIP].contains(&h.internal_compression)
        || ![COMPRESSION_NONE, COMPRESSION_GZIP].contains(&h.tile_compression)
    {
        return Err(StreetError::archive(format!(
            "unsupported compression (internal {}, tiles {})",
            h.internal_compression, h.tile_compression
        )));
    }
    if h.tile_type != TILE_TYPE_MVT {
        return Err(StreetError::archive(format!(
            "tile type {} is not a vector tile",
            h.tile_type
        )));
    }
    if h.max_zoom < max_zoom {
        return Err(StreetError::archive(format!(
            "the hosted map stops at zoom {}, short of {max_zoom}",
            h.max_zoom
        )));
    }
    Ok(())
}

/// One distinct content: its bytes at `src` in the source's tile data, and its place `dst` in
/// the pack's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Piece {
    src: u64,
    len: u64,
    dst: u64,
}

/// Place each distinct content once, in the order the pack's own tiles first use it, and point
/// the entries at the new places. That order makes every pack clustered (spec): a source keeps
/// a shared content where the whole world first uses it, which is often outside the area.
/// Returns the pieces, the pack's entries and the tile data length.
fn layout(entries: &[Entry]) -> (Vec<Piece>, Vec<Entry>, u64) {
    let mut placed: HashMap<(u64, u32), u64> = HashMap::new();
    let mut pieces = Vec::new();
    let mut cursor = 0u64;
    let mut out: Vec<Entry> = Vec::with_capacity(entries.len());
    for e in entries {
        let dst = *placed.entry((e.offset, e.length)).or_insert_with(|| {
            let at = cursor;
            pieces.push(Piece {
                src: e.offset,
                len: u64::from(e.length),
                dst: at,
            });
            cursor += u64::from(e.length);
            at
        });
        let moved = Entry { offset: dst, ..*e };
        match out.last_mut() {
            // Clipping can cut one source run into neighbours that share a content; rejoin them.
            Some(p)
                if p.end_id() == moved.tile_id
                    && p.offset == moved.offset
                    && p.length == moved.length
                    && u64::from(p.run_length) + u64::from(moved.run_length)
                        <= u64::from(u32::MAX) =>
            {
                p.run_length += moved.run_length;
            }
            _ => out.push(moved),
        }
    }
    (pieces, out, cursor)
}

/// Requests for `pieces`, in source order, merged across gaps under `merge_gap` and capped at
/// `max_request`. Each piece lands at its own place in the pack, whatever the source order.
fn requests(pieces: &[Piece], data_offset: u64, merge_gap: u64, max_request: u64) -> Vec<Request> {
    let mut sorted = pieces.to_vec();
    sorted.sort_unstable_by_key(|p| (p.src, p.dst));
    let mut out: Vec<Request> = Vec::new();
    for p in &sorted {
        let mut done = 0;
        while done < p.len {
            let take = (p.len - done).min(max_request);
            let seg = Segment {
                src: data_offset + p.src + done,
                len: take,
                dst: p.dst + done,
            };
            let seg_end = seg.src + seg.len;
            match out.last_mut() {
                // A malformed source can overlap contents; the gap is then zero.
                Some(q)
                    if seg.src.saturating_sub(q.src + q.len) < merge_gap
                        && seg_end.max(q.src + q.len) - q.src <= max_request =>
                {
                    q.len = seg_end.max(q.src + q.len) - q.src;
                    match q.segments.last_mut() {
                        // Contiguous in the source and in the pack: one write.
                        Some(s) if s.src + s.len == seg.src && s.dst + s.len == seg.dst => {
                            s.len += seg.len;
                        }
                        _ => q.segments.push(seg),
                    }
                }
                _ => out.push(Request {
                    src: seg.src,
                    len: take,
                    segments: vec![seg],
                }),
            }
            done += take;
        }
    }
    out
}

/// A pack's tile contents are clustered when each new content starts where the last ended, in
/// tile-id order (spec).
fn is_clustered(entries: &[Entry]) -> bool {
    let mut next = 0u64;
    for e in entries {
        if e.offset == next {
            next += u64::from(e.length);
        } else if e.offset > next {
            return false;
        }
    }
    true
}

fn e7(deg: f64) -> i32 {
    (deg * 1e7).round() as i32
}

/// A zoom that shows the whole square: about 2^z squares across the equator.
fn center_zoom(km: u32, max_zoom: u8) -> u8 {
    ((40_075.0 / f64::from(km)).log2().round() as u8).min(max_zoom)
}

/// The pack's metadata: the source's (layer list and all), renamed, with the ODbL notice, the
/// source build and the area added. Nothing time-dependent goes in, so the same plan always
/// produces the same bytes.
pub fn pack_metadata(source: &Value, build: &BuildInfo, area: &StreetArea, bbox: &BBox) -> Vec<u8> {
    let mut m = match source {
        Value::Object(m) => m.clone(),
        _ => Map::new(),
    };
    let attribution = m
        .get("attribution")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(OSM_ATTRIBUTION)
        .to_string();
    let z = area.detail.max_zoom();
    let mut put = |k: &str, v: Value| {
        m.insert(k.to_string(), v);
    };
    // A modified tileset may not carry the Protomaps name, so the pack has its own.
    put("name", json!("Nexus street map"));
    put(
        "description",
        json!(format!(
            "A {} km square around {:.4}, {:.4}, zoom 0 to {z}, extracted from the {} build of \
             the Protomaps basemap. Map data © OpenStreetMap contributors (ODbL).",
            area.km, area.lat, area.lon, build.date
        )),
    );
    put("attribution", json!(attribution));
    put("license", json!("ODbL-1.0"));
    put("license_uri", json!(ODBL_URI));
    put("license_notice", json!(ODBL_NOTICE));
    put("landcover_attribution", json!(LANDCOVER_CREDIT));
    put("source", json!(build.url));
    put("source_build", json!(build.id));
    put("source_date", json!(build.date));
    if let Some(u) = &build.upstream {
        put("source_upstream", json!(u));
    }
    put("extract_bbox", json!(bbox.to_array()));
    put("extract_center", json!([area.lon, area.lat]));
    put("extract_km", json!(area.km));
    put("extract_maxzoom", json!(z));
    put(
        "extract_method",
        json!(
            "Every tile of the source build that touches extract_bbox, zoom 0 to \
             extract_maxzoom, copied unmodified."
        ),
    );
    serde_json::to_vec(&Value::Object(m)).expect("plain JSON values serialise")
}

pub(crate) fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Size `area` against the map file `src` (the build `build`) and lay out its pack.
pub fn plan(
    src: &dyn RangeSource,
    build: &BuildInfo,
    area: &StreetArea,
    opts: &Options,
    cancel: &AtomicBool,
) -> Result<Plan, StreetError> {
    let area = &area.normalized();
    let bbox = area.bbox()?;
    let max_zoom = area.detail.max_zoom();
    let want = area_ranges(&bbox, max_zoom);
    let read = |off: u64, len: u64| {
        with_retry(&opts.retry, cancel, &|_, _, _| {}, || {
            src.read(off, len, cancel)
        })
        .map_err(|e| e.into_street(ErrorKind::Network))
    };

    let head = read(0, ROOT_LIMIT as u64)?;
    let h = Header::parse(&head)?;
    check_source(&h, max_zoom)?;
    let root_end = h.root_offset.saturating_add(h.root_length);
    if h.root_offset < HEADER_LEN as u64 || root_end > head.len() as u64 {
        return Err(StreetError::archive(
            "the root directory is not in the first 16 KiB",
        ));
    }
    let root = decode_directory(&decompress_internal(
        h.internal_compression,
        &head[h.root_offset as usize..root_end as usize],
    )?)?;
    let mut entries = Vec::new();
    collect(&read, &h, &root, &want, u64::MAX, 0, &mut entries)?;
    check_order(&entries)?;
    if entries.is_empty() {
        return Err(StreetError::archive(
            "the hosted map has no tiles for this area",
        ));
    }
    if entries
        .iter()
        .any(|e| e.offset.saturating_add(u64::from(e.length)) > h.data_length)
    {
        return Err(StreetError::archive("a tile lies outside the tile data"));
    }

    let source_meta = if h.metadata_length == 0 {
        Value::Object(Map::new())
    } else {
        if h.metadata_length > METADATA_LIMIT {
            return Err(StreetError::archive("the metadata is over 1 MiB"));
        }
        let raw = read(h.metadata_offset, h.metadata_length)?;
        let json = decompress_internal(h.internal_compression, &raw)?;
        serde_json::from_slice(&json)
            .map_err(|e| StreetError::archive(format!("the metadata is not JSON: {e}")))?
    };

    let (pieces, out, data_len) = layout(&entries);
    let contents = pieces.len() as u64;
    let (root, leaves) = build_directories(&out);
    let meta = gzip(&pack_metadata(&source_meta, build, area, &bbox));
    let last_id = out.last().map_or(0, |e| e.end_id() - 1);
    let zoom_of = |id: u64| tile_id_to_zxy(id).map_or(0, |(z, _, _)| z);
    let wraps = bbox.crosses_antimeridian();
    let mut header = Header {
        root_offset: HEADER_LEN as u64,
        root_length: root.len() as u64,
        metadata_length: meta.len() as u64,
        leaf_length: leaves.len() as u64,
        data_length: data_len,
        addressed_tiles: out.iter().map(|e| u64::from(e.run_length)).sum(),
        tile_entries: out.len() as u64,
        tile_contents: contents,
        clustered: is_clustered(&out),
        internal_compression: COMPRESSION_GZIP,
        tile_compression: h.tile_compression,
        tile_type: h.tile_type,
        min_zoom: zoom_of(out[0].tile_id),
        max_zoom: zoom_of(last_id),
        min_lon_e7: e7(if wraps { -180.0 } else { bbox.w }),
        min_lat_e7: e7(bbox.s),
        max_lon_e7: e7(if wraps { 180.0 } else { bbox.e }),
        max_lat_e7: e7(bbox.n),
        center_zoom: center_zoom(area.km, max_zoom),
        center_lon_e7: e7(area.lon),
        center_lat_e7: e7(area.lat),
        ..Header::default()
    };
    header.metadata_offset = header.root_offset + header.root_length;
    header.leaf_offset = header.metadata_offset + header.metadata_length;
    header.data_offset = header.leaf_offset + header.leaf_length;

    let mut prefix = Vec::with_capacity(header.data_offset as usize);
    prefix.extend_from_slice(&header.to_bytes());
    prefix.extend_from_slice(&root);
    prefix.extend_from_slice(&meta);
    prefix.extend_from_slice(&leaves);
    let requests = requests(&pieces, h.data_offset, opts.merge_gap, opts.max_request);

    let mut fp = Context::new(&SHA256);
    fp.update(b"nexus street-map plan 1\n");
    fp.update(build.url.as_bytes());
    fp.update(&head[..HEADER_LEN]);
    fp.update(&prefix);
    for r in &requests {
        for v in [r.src, r.len] {
            fp.update(&v.to_le_bytes());
        }
        for s in &r.segments {
            for v in [s.src, s.len, s.dst] {
                fp.update(&v.to_le_bytes());
            }
        }
    }

    Ok(Plan {
        area: *area,
        bbox,
        build: build.clone(),
        pack_id: pack_id(&build.id, area),
        header,
        prefix,
        requests,
        fingerprint: hex(fp.finish().as_ref()),
        source_len: src.total_len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MAX_REQUEST, MERGE_GAP};

    fn piece(src: u64, len: u64, dst: u64) -> Piece {
        Piece { src, len, dst }
    }

    fn e(tile_id: u64, run_length: u32, offset: u64, length: u32) -> Entry {
        Entry {
            tile_id,
            run_length,
            offset,
            length,
        }
    }

    #[test]
    fn pieces_merge_across_small_gaps_and_split_when_long() {
        // Gaps of 64 KiB − 1 merge; 64 KiB does not.
        let pieces = [
            piece(0, 100, 0),
            piece(100 + MERGE_GAP - 1, 50, 100),
            piece(100 + MERGE_GAP - 1 + 50 + MERGE_GAP, 10, 150),
        ];
        let reqs = requests(&pieces, 1000, MERGE_GAP, MAX_REQUEST);
        assert_eq!(reqs.len(), 2);
        assert_eq!(reqs[0].src, 1000);
        assert_eq!(reqs[0].len, 100 + MERGE_GAP - 1 + 50);
        assert_eq!(reqs[0].segments.len(), 2);
        assert_eq!(
            reqs[0].segments[1],
            Segment {
                src: 1000 + 100 + MERGE_GAP - 1,
                len: 50,
                dst: 100
            }
        );
        assert_eq!(
            reqs[1].segments,
            vec![Segment {
                src: 1000 + 100 + 2 * MERGE_GAP + 49,
                len: 10,
                dst: 150
            }]
        );

        // A piece longer than the cap is cut into capped requests that tile it exactly.
        let reqs = requests(
            &[piece(0, 2 * MAX_REQUEST + 5, 0)],
            0,
            MERGE_GAP,
            MAX_REQUEST,
        );
        assert_eq!(
            reqs.iter().map(|r| r.len).collect::<Vec<_>>(),
            [MAX_REQUEST, MAX_REQUEST, 5]
        );
        assert!(reqs.iter().all(|r| r.segments.len() == 1
            && r.segments[0].src == r.src
            && r.segments[0].dst == r.src));

        // 1 MiB pieces 1 KiB apart merge until the next would pass the cap: seven per request.
        let many: Vec<Piece> = (0..100)
            .map(|i| piece(i * ((1 << 20) + 1024), 1 << 20, i * (1 << 20)))
            .collect();
        let reqs = requests(&many, 0, MERGE_GAP, MAX_REQUEST);
        assert!(reqs.iter().all(|r| r.len <= MAX_REQUEST));
        assert_eq!(reqs.len(), 15);
        assert_eq!(reqs[0].segments.len(), 7);
        let kept: u64 = reqs.iter().flat_map(|r| &r.segments).map(|s| s.len).sum();
        assert_eq!(kept, 100 << 20, "every piece is kept exactly once");

        // Pieces touching in the source and in the pack are one write.
        let reqs = requests(
            &[piece(0, 10, 0), piece(10, 5, 10)],
            0,
            MERGE_GAP,
            MAX_REQUEST,
        );
        assert_eq!(
            reqs[0].segments,
            vec![Segment {
                src: 0,
                len: 15,
                dst: 0
            }]
        );
    }

    #[test]
    fn shared_contents_are_stored_once_and_clipped_runs_rejoin() {
        // Ocean (offset 0) shared by tiles 0..10 and 20..30; land contents in between.
        let entries = [
            e(0, 10, 0, 50),
            e(10, 1, 900, 30),
            e(11, 1, 930, 40),
            e(20, 5, 0, 50),
            e(25, 5, 0, 50),
        ];
        let (pieces, out, len) = layout(&entries);
        assert_eq!(pieces.len(), 3, "three distinct contents");
        assert_eq!(len, 50 + 30 + 40);
        assert_eq!(
            out,
            vec![
                e(0, 10, 0, 50),
                e(10, 1, 50, 30),
                e(11, 1, 80, 40),
                e(20, 10, 0, 50)
            ]
        );
        assert!(is_clustered(&out));
        assert!(!is_clustered(&[e(0, 1, 30, 10)]));
    }

    #[test]
    fn contents_are_placed_in_first_use_order_so_every_pack_is_clustered() {
        // The source keeps tile 0's content after tile 1's, as it does when tile 0's content is
        // first used by a tile outside the area.
        let entries = [e(0, 1, 500, 10), e(1, 1, 100, 20), e(2, 1, 500, 10)];
        let (pieces, out, len) = layout(&entries);
        assert_eq!(out, vec![e(0, 1, 0, 10), e(1, 1, 10, 20), e(2, 1, 0, 10)]);
        assert!(is_clustered(&out));
        assert_eq!(len, 30);
        // Fetched in source order, each piece landing at its own place in the pack.
        let reqs = requests(&pieces, 1000, 1 << 10, MAX_REQUEST);
        let expect = Request {
            src: 1100,
            len: 410,
            segments: vec![
                Segment {
                    src: 1100,
                    len: 20,
                    dst: 10,
                },
                Segment {
                    src: 1500,
                    len: 10,
                    dst: 0,
                },
            ],
        };
        assert_eq!(reqs, vec![expect]);
    }
}
