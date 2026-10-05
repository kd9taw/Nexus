//! The checks a finished pack passes before it takes its name: a valid PMTiles v3 file whose
//! directories hold together and whose every tile is a whole vector tile.
//!
//! - **Structure:** every section lies inside the file, the sections do not overlap, and the
//!   root directory sits in the first 16 KiB.
//! - **Directories:** every directory decodes; entries are in tile-id order and do not overlap;
//!   every tile lies inside the tile data; the counts match the header; the tiles' zooms match
//!   the header's.
//! - **Tiles:** every distinct tile is gunzipped (which checks its CRC) and parsed as a Mapbox
//!   Vector Tile 2.1: the protobuf wire format throughout, each layer named, every string UTF-8,
//!   every feature's tags pointing at keys and values that exist, every geometry a sequence of
//!   well-formed commands. A truncated or altered tile cannot pass.
//! - **Metadata:** JSON, carrying the ODbL notice ([`check_pack_metadata`]).

use std::collections::HashSet;
use std::fs::File;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::Value;

use crate::error::{ErrorKind, StreetError};
use crate::pmtiles::{
    check_order, decode_directory, decompress_internal, gunzip, read_varint, tile_id_to_zxy, Entry,
    Header, COMPRESSION_GZIP, COMPRESSION_NONE, HEADER_LEN, ROOT_LIMIT, TILE_TYPE_MVT,
};
use crate::store::read_exact_at;

/// A street-scale vector tile is well under a megabyte unpacked; this bounds a hostile one.
const TILE_LIMIT: u64 = 32 << 20;
/// Tiles are checked from windows of the file this large.
const WINDOW: u64 = 4 << 20;

/// What a checked file holds.
#[derive(Debug, Clone)]
pub struct Checked {
    pub header: Header,
    pub metadata: Value,
    pub addressed_tiles: u64,
    pub entries: u64,
    pub contents: u64,
}

fn read_at(f: &File, off: u64, n: u64) -> Result<Vec<u8>, StreetError> {
    let mut b = vec![0u8; n as usize];
    read_exact_at(f, &mut b, off)
        .map_err(|e| StreetError::archive(format!("cannot read {n} bytes at {off}: {e}")))?;
    Ok(b)
}

/// Every tile entry in `dir` and the leaves below it.
fn walk(
    f: &File,
    h: &Header,
    dir: &[Entry],
    depth: u8,
    out: &mut Vec<Entry>,
) -> Result<(), StreetError> {
    for e in dir {
        if e.run_length > 0 {
            out.push(*e);
            continue;
        }
        if depth >= 3 {
            return Err(StreetError::archive("leaf directories nest too deep"));
        }
        if e.offset.saturating_add(u64::from(e.length)) > h.leaf_length {
            return Err(StreetError::archive(
                "a leaf directory lies outside its section",
            ));
        }
        let raw = read_at(f, h.leaf_offset + e.offset, u64::from(e.length))?;
        let leaf = decode_directory(&decompress_internal(h.internal_compression, &raw)?)?;
        walk(f, h, &leaf, depth + 1, out)?;
    }
    Ok(())
}

/// Check the PMTiles file `f` of `len` bytes. `on_tile(done, total)` follows the tile checks.
pub fn verify_file(
    f: &File,
    len: u64,
    cancel: &AtomicBool,
    on_tile: &mut dyn FnMut(u64, u64),
) -> Result<Checked, StreetError> {
    let head = read_at(f, 0, len.min(ROOT_LIMIT as u64))?;
    let h = Header::parse(&head)?;
    if ![COMPRESSION_NONE, COMPRESSION_GZIP].contains(&h.internal_compression)
        || ![COMPRESSION_NONE, COMPRESSION_GZIP].contains(&h.tile_compression)
        || h.tile_type != TILE_TYPE_MVT
    {
        return Err(StreetError::archive(
            "not a gzip or plain vector-tile archive",
        ));
    }
    let sections = [
        ("root directory", h.root_offset, h.root_length),
        ("metadata", h.metadata_offset, h.metadata_length),
        ("leaf directories", h.leaf_offset, h.leaf_length),
        ("tile data", h.data_offset, h.data_length),
    ];
    let mut spans = Vec::new();
    for (name, off, n) in sections {
        if n == 0 {
            continue;
        }
        let end = off
            .checked_add(n)
            .filter(|&end| end <= len && off >= HEADER_LEN as u64)
            .ok_or_else(|| StreetError::archive(format!("the {name} lies outside the file")))?;
        spans.push((off, end, name));
    }
    spans.sort_unstable();
    for pair in spans.windows(2) {
        if pair[0].1 > pair[1].0 {
            return Err(StreetError::archive(format!(
                "the {} overlaps the {}",
                pair[0].2, pair[1].2
            )));
        }
    }
    if h.root_offset + h.root_length > ROOT_LIMIT as u64 {
        return Err(StreetError::archive(
            "the root directory is not in the first 16 KiB",
        ));
    }

    let root_raw = &head[h.root_offset as usize..(h.root_offset + h.root_length) as usize];
    let root = decode_directory(&decompress_internal(h.internal_compression, root_raw)?)?;
    let mut tiles = Vec::new();
    walk(f, &h, &root, 0, &mut tiles)?;
    check_order(&tiles)?;
    if tiles
        .iter()
        .any(|e| e.offset + u64::from(e.length) > h.data_length)
    {
        return Err(StreetError::archive("a tile lies outside the tile data"));
    }
    let addressed: u64 = tiles.iter().map(|e| u64::from(e.run_length)).sum();
    let mut contents: Vec<(u64, u32)> = tiles.iter().map(|e| (e.offset, e.length)).collect();
    contents.sort_unstable();
    contents.dedup();
    let stated = [
        ("addressed tiles", h.addressed_tiles, addressed),
        ("tile entries", h.tile_entries, tiles.len() as u64),
        ("tile contents", h.tile_contents, contents.len() as u64),
    ];
    for (what, header_says, found) in stated {
        if header_says != 0 && header_says != found {
            return Err(StreetError::archive(format!(
                "the header counts {header_says} {what}; the directories hold {found}"
            )));
        }
    }
    if let (Some(first), Some(last)) = (tiles.first(), tiles.last()) {
        let zoom = |id| tile_id_to_zxy(id).map(|(z, _, _)| z);
        let (lo, hi) = (zoom(first.tile_id), zoom(last.end_id() - 1));
        if lo.is_none_or(|z| z < h.min_zoom) || hi.is_none_or(|z| z > h.max_zoom) {
            return Err(StreetError::archive(
                "tiles lie outside the header's zoom range",
            ));
        }
    }

    let metadata = if h.metadata_length == 0 {
        Value::Null
    } else {
        let raw = read_at(f, h.metadata_offset, h.metadata_length)?;
        serde_json::from_slice(&decompress_internal(h.internal_compression, &raw)?)
            .map_err(|e| StreetError::archive(format!("the metadata is not JSON: {e}")))?
    };

    let total = contents.len() as u64;
    let mut window: (u64, Vec<u8>) = (0, Vec::new());
    for (i, &(off, n)) in contents.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err(StreetError::new(ErrorKind::Cancelled, "cancelled"));
        }
        let (at, n) = (h.data_offset + off, u64::from(n));
        let (w0, wlen) = (window.0, window.1.len() as u64);
        if at < w0 || at + n > w0 + wlen {
            let want = n.max(WINDOW).min(h.data_offset + h.data_length - at);
            window = (at, read_at(f, at, want)?);
        }
        let start = (at - window.0) as usize;
        let bytes = &window.1[start..start + n as usize];
        let raw;
        let tile = if h.tile_compression == COMPRESSION_GZIP {
            raw = gunzip(bytes, TILE_LIMIT)
                .map_err(|e| StreetError::archive(format!("the tile at {off}: {e}")))?;
            &raw[..]
        } else {
            bytes
        };
        check_tile(tile).map_err(|e| {
            StreetError::archive(format!("the tile at {off} is not a vector tile: {e}"))
        })?;
        on_tile(i as u64 + 1, total);
    }

    Ok(Checked {
        header: h,
        metadata,
        addressed_tiles: addressed,
        entries: tiles.len() as u64,
        contents: total,
    })
}

/// A pack's metadata must carry its licence and the OpenStreetMap credit, which travel with
/// the file wherever it is copied (ODbL 4.2).
pub fn check_pack_metadata(m: &Value) -> Result<(), StreetError> {
    let has = |k: &str| {
        m.get(k)
            .and_then(Value::as_str)
            .is_some_and(|s| !s.trim().is_empty())
    };
    if m.get("license").and_then(Value::as_str) != Some("ODbL-1.0")
        || !has("license_uri")
        || !has("attribution")
        || !has("source")
        || !has("source_date")
    {
        return Err(StreetError::archive(
            "the pack's metadata lacks its licence notice or its source",
        ));
    }
    Ok(())
}

/// Protobuf wire-format reading, just enough for a vector tile.
struct Pb<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Pb<'a> {
    fn new(b: &'a [u8]) -> Self {
        Self { b, pos: 0 }
    }

    fn done(&self) -> bool {
        self.pos >= self.b.len()
    }

    fn varint(&mut self) -> Result<u64, String> {
        read_varint(self.b, &mut self.pos).map_err(|e| e.message)
    }

    fn key(&mut self) -> Result<Option<(u64, u8)>, String> {
        if self.done() {
            return Ok(None);
        }
        let k = self.varint()?;
        if k >> 3 == 0 {
            return Err("field number 0".into());
        }
        Ok(Some((k >> 3, (k & 7) as u8)))
    }

    fn bytes(&mut self) -> Result<&'a [u8], String> {
        let n = self.varint()?;
        let end = usize::try_from(n)
            .ok()
            .and_then(|n| self.pos.checked_add(n))
            .filter(|&end| end <= self.b.len())
            .ok_or("a field runs past its message")?;
        let s = &self.b[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    fn advance(&mut self, n: usize) -> Result<(), String> {
        if self.b.len() - self.pos < n {
            return Err("a fixed-size field runs past its message".into());
        }
        self.pos += n;
        Ok(())
    }

    fn skip(&mut self, wire: u8) -> Result<(), String> {
        match wire {
            0 => {
                self.varint()?;
            }
            1 => self.advance(8)?,
            2 => {
                self.bytes()?;
            }
            5 => self.advance(4)?,
            w => return Err(format!("wire type {w}")),
        }
        Ok(())
    }
}

fn utf8(b: &[u8], what: &str) -> Result<(), String> {
    match std::str::from_utf8(b) {
        Ok(_) => Ok(()),
        Err(_) => Err(format!("{what} is not UTF-8")),
    }
}

/// A Mapbox Vector Tile 2.1, checked as described in the module header.
pub(crate) fn check_tile(b: &[u8]) -> Result<(), String> {
    let mut r = Pb::new(b);
    while let Some((field, wire)) = r.key()? {
        match (field, wire) {
            (3, 2) => check_layer(r.bytes()?)?,
            (3, _) => return Err("a layer with the wrong wire type".into()),
            _ => r.skip(wire)?,
        }
    }
    Ok(())
}

fn check_layer(b: &[u8]) -> Result<(), String> {
    let mut r = Pb::new(b);
    let (mut named, mut keys, mut values) = (false, 0u64, 0u64);
    let (mut max_key, mut max_value): (Option<u64>, Option<u64>) = (None, None);
    while let Some((field, wire)) = r.key()? {
        match (field, wire) {
            (15, 0) => {
                let v = r.varint()?;
                if !(1..=2).contains(&v) {
                    return Err(format!("layer version {v}"));
                }
            }
            (1, 2) => {
                utf8(r.bytes()?, "a layer name")?;
                named = true;
            }
            (2, 2) => check_feature(r.bytes()?, &mut max_key, &mut max_value)?,
            (3, 2) => {
                utf8(r.bytes()?, "a key")?;
                keys += 1;
            }
            (4, 2) => {
                check_value(r.bytes()?)?;
                values += 1;
            }
            (5, 0) => {
                if r.varint()? == 0 {
                    return Err("a layer extent of 0".into());
                }
            }
            (1..=5 | 15, _) => return Err(format!("layer field {field} has wire type {wire}")),
            _ => r.skip(wire)?,
        }
    }
    if !named {
        return Err("a layer without a name".into());
    }
    if max_key.is_some_and(|k| k >= keys) || max_value.is_some_and(|v| v >= values) {
        return Err("a feature tag points at a key or value the layer does not have".into());
    }
    Ok(())
}

fn check_feature(
    b: &[u8],
    max_key: &mut Option<u64>,
    max_value: &mut Option<u64>,
) -> Result<(), String> {
    let mut r = Pb::new(b);
    while let Some((field, wire)) = r.key()? {
        match (field, wire) {
            (1, 0) => {
                r.varint()?;
            }
            (2, 2) => {
                let mut t = Pb::new(r.bytes()?);
                let mut n = 0u64;
                while !t.done() {
                    let v = t.varint()?;
                    let slot = if n.is_multiple_of(2) {
                        &mut *max_key
                    } else {
                        &mut *max_value
                    };
                    *slot = Some(slot.map_or(v, |m| m.max(v)));
                    n += 1;
                }
                if !n.is_multiple_of(2) {
                    return Err("an odd number of feature tags".into());
                }
            }
            (3, 0) => {
                if r.varint()? > 3 {
                    return Err("an unknown geometry type".into());
                }
            }
            (4, 2) => check_geometry(r.bytes()?)?,
            (1..=4, _) => return Err(format!("feature field {field} has wire type {wire}")),
            _ => r.skip(wire)?,
        }
    }
    Ok(())
}

fn check_geometry(b: &[u8]) -> Result<(), String> {
    let mut r = Pb::new(b);
    while !r.done() {
        let c = r.varint()?;
        let (id, count) = (c & 7, c >> 3);
        match id {
            // MoveTo and LineTo carry `count` (dx, dy) pairs.
            1 | 2 => {
                if count == 0 || count > (r.b.len() - r.pos) as u64 {
                    return Err(format!("a geometry command with {count} points"));
                }
                for _ in 0..count * 2 {
                    r.varint()?;
                }
            }
            // ClosePath takes no parameters and must say 1.
            7 if count == 1 => {}
            _ => return Err(format!("geometry command {id} × {count}")),
        }
    }
    Ok(())
}

fn check_value(b: &[u8]) -> Result<(), String> {
    let mut r = Pb::new(b);
    let mut seen = HashSet::new();
    while let Some((field, wire)) = r.key()? {
        match (field, wire) {
            (1, 2) => utf8(r.bytes()?, "a string value")?,
            (2, 5) => r.advance(4)?,
            (3, 1) => r.advance(8)?,
            (4..=7, 0) => {
                r.varint()?;
            }
            (1..=7, _) => return Err(format!("value field {field} has wire type {wire}")),
            _ => r.skip(wire)?,
        }
        if field <= 7 {
            seen.insert(field);
        }
    }
    if seen.len() != 1 {
        return Err(format!("a value with {} kinds", seen.len()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::mvt_tile;

    #[test]
    fn a_well_formed_tile_passes_and_damaged_ones_do_not() {
        let tile = mvt_tile(3, 4, 5);
        assert!(
            check_tile(&tile).is_ok(),
            "control: {:?}",
            check_tile(&tile)
        );
        // Any truncation of a tile with one layer breaks it.
        for cut in 1..tile.len() {
            assert!(check_tile(&tile[..cut]).is_err(), "cut at {cut}");
        }
        // A wire type 3 (a group) where a layer belongs.
        assert!(check_tile(&[0x1b]).is_err());
        // A layer with no name: field 3, length 2, version 2.
        assert!(check_tile(&[0x1a, 0x02, 0x78, 0x02]).is_err());
        // An empty tile is valid (open sea has empty layers or none at all).
        assert!(check_tile(&[]).is_ok());
    }
}
