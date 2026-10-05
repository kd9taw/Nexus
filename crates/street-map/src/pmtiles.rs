//! PMTiles version 3: the header, the directories and the Hilbert tile id.
//!
//! Original code written from the published specification
//! (<https://github.com/protomaps/PMTiles/blob/main/spec/v3/spec.md>, public domain or CC0).
//! Nothing here is copied from a PMTiles implementation. Only what the street map needs is
//! implemented: gzip or uncompressed directories, and tiles addressed by id.
//!
//! An archive is a 127-byte header, a root directory, JSON metadata, leaf directories and tile
//! data. The header and the root directory must both lie in the first 16 KiB, so one request
//! reads them. A directory lists entries sorted by tile id. An entry with a run length of 1 or
//! more points at tile content shared by that many consecutive ids; a run length of 0 points
//! at a leaf directory covering the ids up to the next entry.
//!
//! Every function that reads bytes it did not write treats them as untrusted. Counts are
//! checked against the bytes present before anything is allocated, and arithmetic on offsets
//! is checked, so a corrupt or hostile archive fails as `InvalidArchive` instead of panicking
//! or allocating without bound.

use std::io::{Read, Write};

use flate2::read::GzDecoder;
use flate2::write::GzEncoder;

use crate::error::StreetError;

pub const MAGIC: &[u8; 7] = b"PMTiles";
pub const VERSION: u8 = 3;
pub const HEADER_LEN: usize = 127;
/// The header and the root directory together must fit here (spec), so a reader gets both in
/// one request.
pub const ROOT_LIMIT: usize = 16_384;

pub const COMPRESSION_UNKNOWN: u8 = 0;
pub const COMPRESSION_NONE: u8 = 1;
pub const COMPRESSION_GZIP: u8 = 2;
pub const TILE_TYPE_MVT: u8 = 1;

/// The deepest zoom this code addresses. Far beyond any street map (15 is the planet's), and
/// low enough that every tile id fits in 64 bits with room to spare.
pub const MAX_ZOOM: u8 = 26;

/// Directories are small, but a hostile gzip stream can expand without bound. 64 MiB is
/// several times the largest real leaf directory.
const DIRECTORY_LIMIT: u64 = 64 << 20;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Header {
    pub root_offset: u64,
    pub root_length: u64,
    pub metadata_offset: u64,
    pub metadata_length: u64,
    pub leaf_offset: u64,
    pub leaf_length: u64,
    pub data_offset: u64,
    pub data_length: u64,
    /// 0 means "not stated" in the three counts (spec).
    pub addressed_tiles: u64,
    pub tile_entries: u64,
    pub tile_contents: u64,
    pub clustered: bool,
    pub internal_compression: u8,
    pub tile_compression: u8,
    pub tile_type: u8,
    pub min_zoom: u8,
    pub max_zoom: u8,
    pub min_lon_e7: i32,
    pub min_lat_e7: i32,
    pub max_lon_e7: i32,
    pub max_lat_e7: i32,
    pub center_zoom: u8,
    pub center_lon_e7: i32,
    pub center_lat_e7: i32,
}

fn u64_at(b: &[u8], at: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[at..at + 8]);
    u64::from_le_bytes(a)
}

fn i32_at(b: &[u8], at: usize) -> i32 {
    let mut a = [0u8; 4];
    a.copy_from_slice(&b[at..at + 4]);
    i32::from_le_bytes(a)
}

impl Header {
    /// Parse the fixed 127-byte header at the start of `b`.
    pub fn parse(b: &[u8]) -> Result<Header, StreetError> {
        if b.len() < HEADER_LEN {
            return Err(StreetError::archive(format!(
                "the file is {} bytes, shorter than a PMTiles header",
                b.len()
            )));
        }
        if &b[..7] != MAGIC {
            return Err(StreetError::archive("not a PMTiles file"));
        }
        if b[7] != VERSION {
            return Err(StreetError::archive(format!(
                "PMTiles version {} (only version 3 is supported)",
                b[7]
            )));
        }
        Ok(Header {
            root_offset: u64_at(b, 8),
            root_length: u64_at(b, 16),
            metadata_offset: u64_at(b, 24),
            metadata_length: u64_at(b, 32),
            leaf_offset: u64_at(b, 40),
            leaf_length: u64_at(b, 48),
            data_offset: u64_at(b, 56),
            data_length: u64_at(b, 64),
            addressed_tiles: u64_at(b, 72),
            tile_entries: u64_at(b, 80),
            tile_contents: u64_at(b, 88),
            clustered: b[96] == 1,
            internal_compression: b[97],
            tile_compression: b[98],
            tile_type: b[99],
            min_zoom: b[100],
            max_zoom: b[101],
            min_lon_e7: i32_at(b, 102),
            min_lat_e7: i32_at(b, 106),
            max_lon_e7: i32_at(b, 110),
            max_lat_e7: i32_at(b, 114),
            center_zoom: b[118],
            center_lon_e7: i32_at(b, 119),
            center_lat_e7: i32_at(b, 123),
        })
    }

    pub fn to_bytes(&self) -> [u8; HEADER_LEN] {
        let mut b = [0u8; HEADER_LEN];
        b[..7].copy_from_slice(MAGIC);
        b[7] = VERSION;
        let words = [
            self.root_offset,
            self.root_length,
            self.metadata_offset,
            self.metadata_length,
            self.leaf_offset,
            self.leaf_length,
            self.data_offset,
            self.data_length,
            self.addressed_tiles,
            self.tile_entries,
            self.tile_contents,
        ];
        for (i, w) in words.iter().enumerate() {
            b[8 + i * 8..16 + i * 8].copy_from_slice(&w.to_le_bytes());
        }
        b[96] = u8::from(self.clustered);
        b[97] = self.internal_compression;
        b[98] = self.tile_compression;
        b[99] = self.tile_type;
        b[100] = self.min_zoom;
        b[101] = self.max_zoom;
        b[102..106].copy_from_slice(&self.min_lon_e7.to_le_bytes());
        b[106..110].copy_from_slice(&self.min_lat_e7.to_le_bytes());
        b[110..114].copy_from_slice(&self.max_lon_e7.to_le_bytes());
        b[114..118].copy_from_slice(&self.max_lat_e7.to_le_bytes());
        b[118] = self.center_zoom;
        b[119..123].copy_from_slice(&self.center_lon_e7.to_le_bytes());
        b[123..127].copy_from_slice(&self.center_lat_e7.to_le_bytes());
        b
    }
}

/// One directory entry. `offset` is relative to the tile data section for a tile entry and to
/// the leaf directory section for a leaf pointer (`run_length == 0`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Entry {
    pub tile_id: u64,
    pub offset: u64,
    pub length: u32,
    pub run_length: u32,
}

impl Entry {
    /// One past the last tile id this entry addresses (tile entries only).
    pub fn end_id(&self) -> u64 {
        self.tile_id.saturating_add(u64::from(self.run_length))
    }
}

pub(crate) fn read_varint(b: &[u8], pos: &mut usize) -> Result<u64, StreetError> {
    let mut v: u64 = 0;
    for i in 0..10 {
        let byte = *b
            .get(*pos)
            .ok_or_else(|| StreetError::archive("a directory ends inside a number"))?;
        *pos += 1;
        if i == 9 && byte > 1 {
            return Err(StreetError::archive("a directory number overflows 64 bits"));
        }
        v |= u64::from(byte & 0x7f) << (7 * i);
        if byte & 0x80 == 0 {
            return Ok(v);
        }
    }
    Err(StreetError::archive(
        "a directory number is longer than 10 bytes",
    ))
}

pub(crate) fn write_varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push((v as u8) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

/// Decode an uncompressed directory. Entry order is not judged here; [`check_order`] does that
/// for whoever needs it.
pub fn decode_directory(raw: &[u8]) -> Result<Vec<Entry>, StreetError> {
    let mut pos = 0;
    let n = read_varint(raw, &mut pos)?;
    // Every entry costs at least four bytes (four varints), so a larger count is corrupt, and
    // refusing it here means a hostile count never reaches the allocator.
    if n > ((raw.len() - pos) / 4) as u64 {
        return Err(StreetError::archive(format!(
            "a directory claims {n} entries in {} bytes",
            raw.len()
        )));
    }
    let n = n as usize;
    let mut entries = vec![Entry::default(); n];
    let mut last = 0u64;
    for e in entries.iter_mut() {
        last = last
            .checked_add(read_varint(raw, &mut pos)?)
            .ok_or_else(|| StreetError::archive("a tile id overflows 64 bits"))?;
        e.tile_id = last;
    }
    for e in entries.iter_mut() {
        e.run_length = u32::try_from(read_varint(raw, &mut pos)?)
            .map_err(|_| StreetError::archive("a run length is over 32 bits"))?;
    }
    for e in entries.iter_mut() {
        e.length = u32::try_from(read_varint(raw, &mut pos)?)
            .map_err(|_| StreetError::archive("an entry length is over 32 bits"))?;
        if e.length == 0 {
            return Err(StreetError::archive("a directory entry has length 0"));
        }
    }
    for i in 0..n {
        let v = read_varint(raw, &mut pos)?;
        entries[i].offset = if v == 0 {
            // 0 means "straight after the previous entry", which the first entry has none of.
            let prev = i
                .checked_sub(1)
                .map(|p| entries[p])
                .ok_or_else(|| StreetError::archive("the first entry has no offset"))?;
            prev.offset
                .checked_add(u64::from(prev.length))
                .ok_or_else(|| StreetError::archive("an offset overflows 64 bits"))?
        } else {
            v - 1
        };
    }
    if pos != raw.len() {
        return Err(StreetError::archive(format!(
            "{} stray bytes after a directory",
            raw.len() - pos
        )));
    }
    Ok(entries)
}

pub fn encode_directory(entries: &[Entry]) -> Vec<u8> {
    let mut out = Vec::with_capacity(entries.len() * 6 + 8);
    write_varint(&mut out, entries.len() as u64);
    let mut last = 0u64;
    for e in entries {
        write_varint(&mut out, e.tile_id - last);
        last = e.tile_id;
    }
    for e in entries {
        write_varint(&mut out, u64::from(e.run_length));
    }
    for e in entries {
        write_varint(&mut out, u64::from(e.length));
    }
    for (i, e) in entries.iter().enumerate() {
        let follows = i > 0 && e.offset == entries[i - 1].offset + u64::from(entries[i - 1].length);
        write_varint(&mut out, if follows { 0 } else { e.offset + 1 });
    }
    out
}

/// Entries must be sorted by tile id, and a tile entry's run must end before the next entry
/// starts. A leaf pointer covers up to the next entry, so it has no run to overlap.
pub fn check_order(entries: &[Entry]) -> Result<(), StreetError> {
    for pair in entries.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let end = if a.run_length == 0 {
            a.tile_id + 1
        } else {
            a.end_id()
        };
        if b.tile_id < end {
            return Err(StreetError::archive(format!(
                "directory entries out of order or overlapping at tile id {}",
                b.tile_id
            )));
        }
    }
    Ok(())
}

pub fn gzip(raw: &[u8]) -> Vec<u8> {
    let mut enc = GzEncoder::new(Vec::new(), flate2::Compression::default());
    // Writing to a Vec cannot fail.
    enc.write_all(raw).expect("gzip into memory");
    enc.finish().expect("gzip into memory")
}

/// Gunzip `b`, refusing output over `limit` bytes. The CRC-32 and length trailer are checked
/// when the stream ends, so a truncated or altered stream fails here.
pub fn gunzip(b: &[u8], limit: u64) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut dec = GzDecoder::new(b).take(limit + 1);
    dec.read_to_end(&mut out)
        .map_err(|e| format!("gunzip: {e}"))?;
    if out.len() as u64 > limit {
        return Err(format!("expands past {limit} bytes"));
    }
    Ok(out)
}

/// Undo an archive's internal compression (directories and metadata).
pub fn decompress_internal(compression: u8, b: &[u8]) -> Result<Vec<u8>, StreetError> {
    match compression {
        COMPRESSION_NONE => Ok(b.to_vec()),
        COMPRESSION_GZIP => gunzip(b, DIRECTORY_LIMIT).map_err(StreetError::archive),
        c => Err(StreetError::archive(format!(
            "internal compression {c} is not supported (only none and gzip)"
        ))),
    }
}

/// The first tile id at zoom `z`: the number of tiles at all shallower zooms, (4^z − 1) / 3.
pub fn zoom_base(z: u8) -> u64 {
    ((1u64 << (2 * u32::from(z))) - 1) / 3
}

/// The tile id of (z, x, y): every tile of the shallower zooms, then this tile's position along
/// the Hilbert curve that orders zoom `z`.
pub fn zxy_to_tile_id(z: u8, x: u32, y: u32) -> u64 {
    debug_assert!(z <= MAX_ZOOM && u64::from(x) < 1 << z && u64::from(y) < 1 << z);
    zoom_base(z) + hilbert_index(z, x, y)
}

fn hilbert_index(z: u8, x: u32, y: u32) -> u64 {
    let n = 1u64 << z;
    let (mut x, mut y) = (u64::from(x), u64::from(y));
    let mut d = 0u64;
    let mut s = n / 2;
    while s > 0 {
        let rx = u64::from(x & s != 0);
        let ry = u64::from(y & s != 0);
        d += s * s * ((3 * rx) ^ ry);
        // Rotate the quadrant so the curve's next level is read in its own frame.
        if ry == 0 {
            if rx == 1 {
                x = n - 1 - x;
                y = n - 1 - y;
            }
            std::mem::swap(&mut x, &mut y);
        }
        s /= 2;
    }
    d
}

/// The inverse of [`zxy_to_tile_id`]; `None` past [`MAX_ZOOM`].
pub fn tile_id_to_zxy(id: u64) -> Option<(u8, u32, u32)> {
    let z = (0..=MAX_ZOOM).rev().find(|&z| id >= zoom_base(z))?;
    if id >= zoom_base(MAX_ZOOM + 1) {
        return None;
    }
    let n = 1u64 << z;
    let mut t = id - zoom_base(z);
    let (mut x, mut y) = (0u64, 0u64);
    let mut s = 1u64;
    while s < n {
        let rx = 1 & (t / 2);
        let ry = 1 & (t ^ rx);
        if ry == 0 {
            if rx == 1 {
                x = s - 1 - x;
                y = s - 1 - y;
            }
            std::mem::swap(&mut x, &mut y);
        }
        x += s * rx;
        y += s * ry;
        t /= 4;
        s *= 2;
    }
    Some((z, x as u32, y as u32))
}

/// Root and leaf directories for `entries` (sorted, tile entries only), gzip-compressed:
/// everything in the root if it fits in the first 16 KiB beside the header, otherwise leaves
/// of a growing number of entries until the root of leaf pointers fits.
pub fn build_directories(entries: &[Entry]) -> (Vec<u8>, Vec<u8>) {
    let root = gzip(&encode_directory(entries));
    if HEADER_LEN + root.len() <= ROOT_LIMIT {
        return (root, Vec::new());
    }
    let mut leaf_size = (entries.len() / 3500).max(4096);
    loop {
        let (root, leaves) = build_with_leaves(entries, leaf_size);
        if HEADER_LEN + root.len() <= ROOT_LIMIT {
            return (root, leaves);
        }
        leaf_size += leaf_size / 5;
    }
}

/// Root and leaves with exactly `leaf_size` entries per leaf (the last may be short).
pub(crate) fn build_with_leaves(entries: &[Entry], leaf_size: usize) -> (Vec<u8>, Vec<u8>) {
    let mut leaves = Vec::new();
    let mut pointers = Vec::new();
    for chunk in entries.chunks(leaf_size.max(1)) {
        let leaf = gzip(&encode_directory(chunk));
        pointers.push(Entry {
            tile_id: chunk[0].tile_id,
            offset: leaves.len() as u64,
            length: leaf.len() as u32,
            run_length: 0,
        });
        leaves.extend_from_slice(&leaf);
    }
    (gzip(&encode_directory(&pointers)), leaves)
}

/// Tiles read back out of an archive through a range-read function: the lookups a renderer
/// makes (header and root once, a leaf when an id falls in one, then the tile's bytes).
pub struct Reader<F> {
    read: F,
    header: Header,
    root: Vec<Entry>,
}

impl<F: Fn(u64, u64) -> Result<Vec<u8>, StreetError>> Reader<F> {
    pub fn open(read: F) -> Result<Self, StreetError> {
        let head = read(0, ROOT_LIMIT as u64)?;
        let header = Header::parse(&head)?;
        let end = header.root_offset.saturating_add(header.root_length);
        if end > head.len() as u64 {
            return Err(StreetError::archive(
                "the root directory is not in the first 16 KiB",
            ));
        }
        let raw = &head[header.root_offset as usize..end as usize];
        let root = decode_directory(&decompress_internal(header.internal_compression, raw)?)?;
        Ok(Self { read, header, root })
    }

    pub fn header(&self) -> &Header {
        &self.header
    }

    /// The stored bytes of tile `id` (still compressed), or `None` when the archive lacks it.
    pub fn tile(&self, id: u64) -> Result<Option<Vec<u8>>, StreetError> {
        let mut dir = self.root.clone();
        for _ in 0..4 {
            let Some(i) = dir.partition_point(|e| e.tile_id <= id).checked_sub(1) else {
                return Ok(None);
            };
            let e = dir[i];
            if e.run_length > 0 {
                if id >= e.end_id() {
                    return Ok(None);
                }
                let bytes = (self.read)(self.header.data_offset + e.offset, u64::from(e.length))?;
                return Ok(Some(bytes));
            }
            let raw = (self.read)(self.header.leaf_offset + e.offset, u64::from(e.length))?;
            dir = decode_directory(&decompress_internal(
                self.header.internal_compression,
                &raw,
            )?)?;
        }
        Err(StreetError::archive("leaf directories nest too deep"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The spec's own examples: the four zoom-1 tiles in Hilbert order, then the first tile of
    /// zoom 2. Written out by hand from the spec, not produced by this code.
    #[test]
    fn tile_ids_follow_the_specs_hilbert_order() {
        assert_eq!(zxy_to_tile_id(0, 0, 0), 0);
        assert_eq!(zxy_to_tile_id(1, 0, 0), 1);
        assert_eq!(zxy_to_tile_id(1, 0, 1), 2);
        assert_eq!(zxy_to_tile_id(1, 1, 1), 3);
        assert_eq!(zxy_to_tile_id(1, 1, 0), 4);
        assert_eq!(zxy_to_tile_id(2, 0, 0), 5);
        // The last id of a zoom is one less than the next zoom's first.
        assert_eq!(zoom_base(15), 357_913_941);
        assert_eq!(zoom_base(14) + (1 << 28) - 1, zoom_base(15) - 1);
    }

    #[test]
    fn every_tile_id_round_trips_through_zxy() {
        // Exhaustive to zoom 7 (21,845 tiles), and corners plus a stride at street zooms.
        for z in 0..=7u8 {
            for x in 0..(1u32 << z) {
                for y in 0..(1u32 << z) {
                    let id = zxy_to_tile_id(z, x, y);
                    assert_eq!(tile_id_to_zxy(id), Some((z, x, y)), "z{z} x{x} y{y}");
                }
            }
        }
        for z in [12u8, 14, 15, 26] {
            let n = 1u64 << z;
            for x in (0..n).step_by((n / 37).max(1) as usize).chain([n - 1]) {
                for y in (0..n).step_by((n / 41).max(1) as usize).chain([n - 1]) {
                    let id = zxy_to_tile_id(z, x as u32, y as u32);
                    assert_eq!(tile_id_to_zxy(id), Some((z, x as u32, y as u32)));
                }
            }
        }
        // Every id at a zoom is used exactly once: the ids of zoom 4 are a permutation.
        let mut ids: Vec<u64> = (0..16u32)
            .flat_map(|x| (0..16u32).map(move |y| zxy_to_tile_id(4, x, y)))
            .collect();
        ids.sort_unstable();
        assert_eq!(ids, (zoom_base(4)..zoom_base(5)).collect::<Vec<_>>());
        assert_eq!(tile_id_to_zxy(zoom_base(MAX_ZOOM + 1)), None);
    }

    /// Bytes worked out by hand from the spec's directory layout: a count, then the tile id
    /// deltas, run lengths, lengths and offsets as varints, an offset of 0 meaning "right
    /// after the previous entry" and any other value meaning offset + 1.
    #[test]
    fn a_directory_encodes_to_the_specs_bytes_and_back() {
        let entries = [
            Entry {
                tile_id: 0,
                offset: 0,
                length: 10,
                run_length: 1,
            },
            Entry {
                tile_id: 1,
                offset: 10,
                length: 20,
                run_length: 1,
            },
            Entry {
                tile_id: 300,
                offset: 5,
                length: 10,
                run_length: 2,
            },
        ];
        let raw = encode_directory(&entries);
        assert_eq!(
            raw,
            [3, 0, 1, 0xab, 0x02, 1, 1, 2, 10, 20, 10, 1, 0, 6],
            "count, deltas (0, 1, 299), runs, lengths, offsets (0+1, follows, 5+1)"
        );
        assert_eq!(decode_directory(&raw).unwrap(), entries);
    }

    #[test]
    fn corrupt_directories_are_refused_without_panicking() {
        let good = encode_directory(&[
            Entry {
                tile_id: 7,
                offset: 0,
                length: 3,
                run_length: 1,
            },
            Entry {
                tile_id: 8,
                offset: 3,
                length: 3,
                run_length: 1,
            },
        ]);
        assert!(
            decode_directory(&good).is_ok(),
            "control: the good directory decodes"
        );
        // Every truncation fails cleanly.
        for cut in 0..good.len() {
            assert!(decode_directory(&good[..cut]).is_err(), "cut at {cut}");
        }
        // A stray trailing byte.
        let mut long = good.clone();
        long.push(0);
        assert!(decode_directory(&long).is_err());
        // A count far beyond the bytes present is refused before any allocation.
        let mut huge = Vec::new();
        write_varint(&mut huge, u64::MAX / 2);
        huge.extend_from_slice(&[0; 16]);
        assert!(decode_directory(&huge).is_err());
        // A zero length, and a first offset of "follows the previous".
        assert!(decode_directory(&[1, 0, 1, 0, 1]).is_err());
        assert!(decode_directory(&[1, 0, 1, 5, 0]).is_err());
        // An 11-byte varint.
        assert!(read_varint(&[0xff; 11], &mut 0).is_err());
    }

    #[test]
    fn the_header_round_trips_and_rejects_what_is_not_pmtiles_v3() {
        let h = Header {
            root_offset: 127,
            root_length: 300,
            metadata_offset: 427,
            metadata_length: 50,
            leaf_offset: 477,
            leaf_length: 9,
            data_offset: 486,
            data_length: 1 << 40,
            addressed_tiles: 5,
            tile_entries: 4,
            tile_contents: 3,
            clustered: true,
            internal_compression: COMPRESSION_GZIP,
            tile_compression: COMPRESSION_GZIP,
            tile_type: TILE_TYPE_MVT,
            min_zoom: 0,
            max_zoom: 14,
            min_lon_e7: -1_049_903_000,
            min_lat_e7: 397_392_000,
            max_lon_e7: 1_800_000_000,
            max_lat_e7: -850_511_287,
            center_zoom: 8,
            center_lon_e7: -1,
            center_lat_e7: i32::MIN,
        };
        let b = h.to_bytes();
        assert_eq!(&b[..8], b"PMTiles\x03");
        assert_eq!(Header::parse(&b).unwrap(), h);
        assert!(Header::parse(&b[..126]).is_err());
        let mut v2 = b;
        v2[7] = 2;
        assert!(Header::parse(&v2).is_err());
        let mut other = b;
        other[0] = b'p';
        assert!(Header::parse(&other).is_err());
    }

    #[test]
    fn directories_split_into_leaves_only_when_the_root_would_not_fit() {
        let small: Vec<Entry> = (0..100)
            .map(|i| Entry {
                tile_id: i * 3,
                offset: i * 50,
                length: 50,
                run_length: 2,
            })
            .collect();
        let (root, leaves) = build_directories(&small);
        assert!(leaves.is_empty());
        assert_eq!(
            decode_directory(&gunzip(&root, 1 << 20).unwrap()).unwrap(),
            small
        );

        // Ids and offsets that do not compress, so the root cannot hold them all.
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        let mut id = 0;
        let big: Vec<Entry> = (0..60_000u64)
            .map(|i| {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                id += 1 + seed % 1000;
                Entry {
                    tile_id: id,
                    offset: i * 7919 % 1_000_003,
                    length: 1 + (seed % 9000) as u32,
                    run_length: 1,
                }
            })
            .collect();
        let (root, leaves) = build_directories(&big);
        assert!(
            HEADER_LEN + root.len() <= ROOT_LIMIT,
            "the root fits beside the header"
        );
        assert!(!leaves.is_empty());
        let pointers = decode_directory(&gunzip(&root, 1 << 20).unwrap()).unwrap();
        let mut back = Vec::new();
        for p in &pointers {
            assert_eq!(p.run_length, 0, "the root holds leaf pointers only");
            let leaf = &leaves[p.offset as usize..p.offset as usize + p.length as usize];
            back.extend(decode_directory(&gunzip(leaf, 64 << 20).unwrap()).unwrap());
        }
        assert_eq!(back, big);
    }
}
