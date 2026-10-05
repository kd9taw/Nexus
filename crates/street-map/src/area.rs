//! "An area around my station": the square the operator picks, the tiles that touch it at each
//! zoom, and those tiles as ranges of tile ids.
//!
//! The square is 50, 100, 200 or 400 km across, centred on the station or the map centre. Its
//! box is widened in longitude by the latitude nearest the pole, so the box always contains the
//! whole square. A box that crosses the 180° meridian keeps `w > e` (the GeoJSON convention)
//! and is split in two for tiling.
//!
//! Like `pmtiles extract --bbox`, every tile that touches the box is included at every zoom from
//! 0 to the pack's deepest. The shallow zooms' covering tiles are large, so a pack carries the
//! world around it at low zoom for free.
//!
//! The ranges come from a quadtree walk: along the Hilbert curve every aligned square block of
//! tiles is one contiguous run of ids, so a block wholly inside the box is one range and only
//! the blocks on the box's edge are split further. The work grows with the box's perimeter,
//! not its area.

use std::ops::Range;

use serde::{Deserialize, Serialize};

use crate::error::{ErrorKind, StreetError};
use crate::pmtiles::{zoom_base, zxy_to_tile_id};

/// The squares offered, in km across.
pub const KM_CHOICES: [u32; 4] = [50, 100, 200, 400];

/// Web Mercator's edge: tiles stop at this latitude.
pub const MAX_LAT: f64 = 85.051_128_779_806_59;

/// km per degree of latitude on the mean Earth sphere (radius 6371.0088 km).
const KM_PER_DEGREE: f64 = 111.195;

/// "All streets" (tile zoom 14, where street names appear) or "Main roads" (zoom 12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Detail {
    Streets,
    Roads,
}

impl Detail {
    pub fn max_zoom(self) -> u8 {
        match self {
            Detail::Streets => 14,
            Detail::Roads => 12,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Detail::Streets => "streets",
            Detail::Roads => "roads",
        }
    }
}

/// What the operator asked for, as the UI sends it: `{ lat, lon, km, detail }`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreetArea {
    pub lat: f64,
    pub lon: f64,
    pub km: u32,
    pub detail: Detail,
}

/// Degrees, west/south/east/north. `w > e` when the box crosses the 180° meridian.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BBox {
    pub w: f64,
    pub s: f64,
    pub e: f64,
    pub n: f64,
}

impl BBox {
    pub fn crosses_antimeridian(&self) -> bool {
        self.w > self.e
    }

    /// `[w, s, e, n]`, each edge moved outward to a whole millionth of a degree (about 0.1 m):
    /// short decimals survive a JSON round trip exactly, and the box still holds the square.
    pub fn to_array(self) -> [f64; 4] {
        let out = |v: f64, up: bool| {
            let m = v * 1e6;
            (if up { m.ceil() } else { m.floor() }) / 1e6
        };
        [
            out(self.w, false),
            out(self.s, false),
            out(self.e, true),
            out(self.n, true),
        ]
    }
}

impl StreetArea {
    /// The area with its centre on a whole millionth of a degree (about 0.1 m). Every step
    /// works from this form. JSON numbers do not always come back at full precision, and an
    /// area read back from a journal or `packs.json` must still name the same pack and the same
    /// plan, or an interrupted download would start again instead of resuming.
    pub fn normalized(&self) -> Self {
        let r = |v: f64| (v * 1e6).round() / 1e6;
        Self {
            lat: r(self.lat),
            lon: r(self.lon),
            ..*self
        }
    }

    pub fn validate(&self) -> Result<(), StreetError> {
        let bad = |m: String| Err(StreetError::new(ErrorKind::InvalidArea, m));
        if !(self.lat.is_finite() && (-90.0..=90.0).contains(&self.lat)) {
            return bad(format!("latitude {} is not between -90 and 90", self.lat));
        }
        if !(self.lon.is_finite() && (-180.0..=180.0).contains(&self.lon)) {
            return bad(format!(
                "longitude {} is not between -180 and 180",
                self.lon
            ));
        }
        if !KM_CHOICES.contains(&self.km) {
            return bad(format!("{} km is not one of the offered sizes", self.km));
        }
        Ok(())
    }

    /// The box that contains the whole square.
    pub fn bbox(&self) -> Result<BBox, StreetError> {
        self.validate()?;
        let half = f64::from(self.km) / 2.0;
        let dlat = half / KM_PER_DEGREE;
        let s = (self.lat - dlat).max(-MAX_LAT);
        let n = (self.lat + dlat).min(MAX_LAT);
        if s >= n {
            return Err(StreetError::new(
                ErrorKind::InvalidArea,
                format!(
                    "the square around latitude {} lies beyond the map's edge at ±85°",
                    self.lat
                ),
            ));
        }
        // The square is widest in degrees where the meridians are closest: at the edge nearer
        // the pole.
        let poleward = s.abs().max(n.abs()).to_radians();
        let dlon = half / (KM_PER_DEGREE * poleward.cos());
        if !dlon.is_finite() || dlon >= 180.0 {
            return Ok(BBox {
                w: -180.0,
                s,
                e: 180.0,
                n,
            });
        }
        let mut w = self.lon - dlon;
        let mut e = self.lon + dlon;
        if w < -180.0 {
            w += 360.0;
        }
        if e > 180.0 {
            e -= 360.0;
        }
        Ok(BBox { w, s, e, n })
    }
}

/// The tile column holding longitude `lon` at zoom `z`.
pub fn lon_to_x(lon: f64, z: u8) -> u32 {
    let n = f64::from(1u32 << z);
    let x = ((lon + 180.0) / 360.0 * n).floor();
    x.clamp(0.0, n - 1.0) as u32
}

/// The tile row holding latitude `lat` at zoom `z` (rows count down from the north).
pub fn lat_to_y(lat: f64, z: u8) -> u32 {
    let n = f64::from(1u32 << z);
    let phi = lat.clamp(-MAX_LAT, MAX_LAT).to_radians();
    let y = ((1.0 - phi.tan().asinh() / std::f64::consts::PI) / 2.0 * n).floor();
    y.clamp(0.0, n - 1.0) as u32
}

/// An inclusive rectangle of tiles at one zoom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileRect {
    pub z: u8,
    pub x0: u32,
    pub x1: u32,
    pub y0: u32,
    pub y1: u32,
}

/// The tiles at zoom `z` that touch `b`: one rectangle, or two when the box crosses 180°.
pub fn tile_rects(b: &BBox, z: u8) -> Vec<TileRect> {
    let (y0, y1) = (lat_to_y(b.n, z), lat_to_y(b.s, z));
    let (xw, xe) = (lon_to_x(b.w, z), lon_to_x(b.e, z));
    let last = (1u32 << z) - 1;
    if !b.crosses_antimeridian() {
        return vec![TileRect {
            z,
            x0: xw,
            x1: xe,
            y0,
            y1,
        }];
    }
    let mut rects = vec![TileRect {
        z,
        x0: xw,
        x1: last,
        y0,
        y1,
    }];
    // At shallow zooms both halves can land in the same columns; one rectangle then covers it.
    if xe < xw {
        rects.push(TileRect {
            z,
            x0: 0,
            x1: xe,
            y0,
            y1,
        });
    } else {
        rects[0].x0 = 0;
    }
    rects
}

/// The tile ids of `r` as half-open ranges, appended in ascending order.
pub fn rect_ranges(r: &TileRect, out: &mut Vec<Range<u64>>) {
    visit(r, 0, 0, 0, out);
}

fn visit(r: &TileRect, level: u8, bx: u32, by: u32, out: &mut Vec<Range<u64>>) {
    // The block at `level` covers 2^shift × 2^shift tiles of zoom r.z.
    let shift = r.z - level;
    let (x0, y0) = (u64::from(bx) << shift, u64::from(by) << shift);
    let (x1, y1) = (x0 + (1 << shift) - 1, y0 + (1 << shift) - 1);
    let (rx0, rx1, ry0, ry1) = (
        u64::from(r.x0),
        u64::from(r.x1),
        u64::from(r.y0),
        u64::from(r.y1),
    );
    if x1 < rx0 || x0 > rx1 || y1 < ry0 || y0 > ry1 {
        return;
    }
    if x0 >= rx0 && x1 <= rx1 && y0 >= ry0 && y1 <= ry1 {
        // Wholly inside: the block's ids are one run, starting at the block's own Hilbert
        // position scaled to zoom r.z.
        let span = 1u64 << (2 * u32::from(shift));
        let start = zoom_base(r.z) + (zxy_to_tile_id(level, bx, by) - zoom_base(level)) * span;
        push_range(out, start..start + span);
        return;
    }
    // On the edge: look at the four quarters in curve order, so ranges come out ascending.
    let (cx, cy) = (bx * 2, by * 2);
    let mut kids = [(cx, cy), (cx, cy + 1), (cx + 1, cy), (cx + 1, cy + 1)];
    kids.sort_by_key(|&(x, y)| zxy_to_tile_id(level + 1, x, y));
    for (x, y) in kids {
        visit(r, level + 1, x, y, out);
    }
}

fn push_range(out: &mut Vec<Range<u64>>, r: Range<u64>) {
    if let Some(last) = out.last_mut() {
        if last.end == r.start {
            last.end = r.end;
            return;
        }
    }
    out.push(r);
}

/// Every tile id from zoom 0 to `max_zoom` touching `b`, as sorted, merged ranges.
pub fn area_ranges(b: &BBox, max_zoom: u8) -> Vec<Range<u64>> {
    let mut all = Vec::new();
    for z in 0..=max_zoom {
        for r in tile_rects(b, z) {
            rect_ranges(&r, &mut all);
        }
    }
    all.sort_by_key(|r| r.start);
    let mut merged: Vec<Range<u64>> = Vec::with_capacity(all.len());
    for r in all {
        match merged.last_mut() {
            Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
            _ => merged.push(r),
        }
    }
    merged
}

pub fn count_tiles(ranges: &[Range<u64>]) -> u64 {
    ranges.iter().map(|r| r.end - r.start).sum()
}

/// The six-character Maidenhead locator of a point, for a pack's name.
pub fn maidenhead(lat: f64, lon: f64) -> String {
    let lon = (lon + 180.0).clamp(0.0, 359.999_999);
    let lat = (lat + 90.0).clamp(0.0, 179.999_999);
    let field = |v: f64, size: f64| (v / size) as u8;
    [
        (b'A' + field(lon, 20.0)) as char,
        (b'A' + field(lat, 10.0)) as char,
        (b'0' + field(lon % 20.0, 2.0)) as char,
        (b'0' + field(lat % 10.0, 1.0)) as char,
        (b'a' + field((lon % 2.0) * 12.0, 1.0)) as char,
        (b'a' + field((lat % 1.0) * 24.0, 1.0)) as char,
    ]
    .iter()
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pmtiles::tile_id_to_zxy;

    fn area(lat: f64, lon: f64, km: u32) -> StreetArea {
        StreetArea {
            lat,
            lon,
            km,
            detail: Detail::Streets,
        }
    }

    /// The brute-force answer: every tile of the rectangle, its id, sorted and merged.
    fn brute(r: &TileRect) -> Vec<Range<u64>> {
        let mut ids: Vec<u64> = (r.x0..=r.x1)
            .flat_map(|x| (r.y0..=r.y1).map(move |y| zxy_to_tile_id(r.z, x, y)))
            .collect();
        ids.sort_unstable();
        let mut out: Vec<Range<u64>> = Vec::new();
        for id in ids {
            push_range(&mut out, id..id + 1);
        }
        out
    }

    #[test]
    fn the_quadtree_walk_gives_exactly_the_rectangles_tiles() {
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut next = |n: u32| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % u64::from(n)) as u32
        };
        for _ in 0..400 {
            let z = next(9) as u8;
            let n = 1u32 << z;
            let (a, b, c, d) = (next(n), next(n), next(n), next(n));
            let r = TileRect {
                z,
                x0: a.min(b),
                x1: a.max(b),
                y0: c.min(d),
                y1: c.max(d),
            };
            let mut got = Vec::new();
            rect_ranges(&r, &mut got);
            assert_eq!(got, brute(&r), "{r:?}");
        }
        // The whole of a zoom is one range.
        let mut whole = Vec::new();
        rect_ranges(
            &TileRect {
                z: 3,
                x0: 0,
                x1: 7,
                y0: 0,
                y1: 7,
            },
            &mut whole,
        );
        assert_eq!(whole, vec![zoom_base(3)..zoom_base(4)]);
    }

    #[test]
    fn the_square_has_the_right_size_and_contains_its_centre() {
        // Denver, 200 km: about ±0.9° of latitude, and wider in longitude at 40° N.
        let b = area(39.7392, -104.9903, 200).bbox().unwrap();
        assert!((b.n - b.s - 200.0 / KM_PER_DEGREE).abs() < 1e-9);
        let km_ew_at_north_edge = (b.e - b.w) * KM_PER_DEGREE * b.n.to_radians().cos();
        assert!(
            (km_ew_at_north_edge - 200.0).abs() < 1e-6,
            "{km_ew_at_north_edge}"
        );
        assert!(b.w < -104.9903 && -104.9903 < b.e && b.s < 39.7392 && 39.7392 < b.n);
        assert!(!b.crosses_antimeridian());
    }

    #[test]
    fn a_square_across_the_antimeridian_wraps_and_tiles_in_two_parts() {
        // Taveuni, Fiji sits on the 180° meridian.
        let b = area(-16.9, 179.9, 100).bbox().unwrap();
        assert!(b.crosses_antimeridian(), "{b:?}");
        assert!(b.w > 179.0 && b.e < -179.0);
        let rects = tile_rects(&b, 10);
        assert_eq!(rects.len(), 2);
        assert_eq!(rects[0].x1, 1023);
        assert_eq!(rects[1].x0, 0);
        // At zoom 0 both halves are the one world tile.
        assert_eq!(
            tile_rects(&b, 0),
            vec![TileRect {
                z: 0,
                x0: 0,
                x1: 0,
                y0: 0,
                y1: 0
            }]
        );
        // No tile is counted twice where the halves meet.
        let ranges = area_ranges(&b, 8);
        for pair in ranges.windows(2) {
            assert!(pair[0].end < pair[1].start);
        }
    }

    #[test]
    fn near_the_poles_the_box_is_clipped_and_beyond_them_refused() {
        let b = area(84.9, 10.0, 400).bbox().unwrap();
        assert_eq!(b.n, MAX_LAT);
        let polar = area(-89.0, 0.0, 400).bbox().unwrap_err();
        assert_eq!(polar.kind, ErrorKind::InvalidArea);
        // Control: the same size well inside the map is fine.
        assert!(area(-60.0, 0.0, 400).bbox().is_ok());
    }

    #[test]
    fn bad_areas_are_refused() {
        for a in [
            area(91.0, 0.0, 100),
            area(0.0, 181.0, 100),
            area(f64::NAN, 0.0, 100),
            area(0.0, 0.0, 150),
        ] {
            assert_eq!(
                a.validate().unwrap_err().kind,
                ErrorKind::InvalidArea,
                "{a:?}"
            );
        }
        for km in KM_CHOICES {
            assert!(area(0.0, 0.0, km).validate().is_ok(), "control: {km} km");
        }
    }

    /// The tile set for a real place, cross-checked by hand: every tile of zoom 0..=14 whose
    /// box touches the square, and nothing else. Done by decoding every id in the ranges back
    /// to (z, x, y) and checking it against the edges, then counting the rectangles directly.
    #[test]
    fn the_tile_set_for_an_area_is_every_tile_touching_the_square() {
        let b = area(51.5074, -0.1278, 50).bbox().unwrap();
        let ranges = area_ranges(&b, 14);
        let mut expected = 0u64;
        for z in 0..=14u8 {
            let r = tile_rects(&b, z)[0];
            expected += u64::from(r.x1 - r.x0 + 1) * u64::from(r.y1 - r.y0 + 1);
        }
        assert_eq!(count_tiles(&ranges), expected);
        for r in &ranges {
            for id in [r.start, r.end - 1] {
                let (z, x, y) = tile_id_to_zxy(id).unwrap();
                assert!(x >= lon_to_x(b.w, z) && x <= lon_to_x(b.e, z), "z{z} x{x}");
                assert!(y >= lat_to_y(b.n, z) && y <= lat_to_y(b.s, z), "z{z} y{y}");
            }
        }
        // Central London's z14 tile is inside; one just beyond the square's east edge is not.
        let inside = zxy_to_tile_id(14, 8185, 5447);
        assert!(ranges.iter().any(|r| r.contains(&inside)));
        let outside = zxy_to_tile_id(14, lon_to_x(b.e, 14) + 1, 5447);
        assert!(!ranges.iter().any(|r| r.contains(&outside)));
        // Zoom 15 is not part of a zoom-14 pack.
        assert!(ranges.last().unwrap().end <= zoom_base(15));
    }

    #[test]
    fn a_normalized_area_survives_a_json_round_trip_exactly() {
        // A subsquare's centre worked out in full precision, as a grid conversion produces it:
        // 17 significant digits, which serde_json's default parser does not always return
        // exactly.
        let raw = area(
            39.0 + 44.0 / 60.0 + 21.15 / 3600.0,
            -105.0 + 35.0 / 3600.0,
            200,
        );
        let n = raw.normalized();
        assert_eq!((n.lat, n.lon), (39.739208, -104.990278));
        assert_eq!(n.normalized(), n, "normalizing twice changes nothing");
        let back: StreetArea = serde_json::from_str(&serde_json::to_string(&n).unwrap()).unwrap();
        assert_eq!(back, n);
        let b = n.bbox().unwrap().to_array();
        let back: [f64; 4] = serde_json::from_str(&serde_json::to_string(&b).unwrap()).unwrap();
        assert_eq!(back, b);
        // Outward: the rounded box still holds the exact one.
        let exact = n.bbox().unwrap();
        assert!(b[0] <= exact.w && b[1] <= exact.s && b[2] >= exact.e && b[3] >= exact.n);
    }

    #[test]
    fn the_locator_matches_known_stations() {
        assert_eq!(maidenhead(41.714775, -72.727260), "FN31pr"); // W1AW, Newington CT
        assert_eq!(maidenhead(-33.8688, 151.2093), "QF56od"); // Sydney
        assert_eq!(maidenhead(90.0, 180.0), "RR99xx");
        assert_eq!(maidenhead(-90.0, -180.0), "AA00aa");
    }
}
