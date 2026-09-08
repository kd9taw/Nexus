//! Maidenhead 4-character grid ⇄ the 15-bit field of heartbeat / compound frames.
//!
//! varicode.cpp:1129-1151 read as spec. `packGrid`: `grid2deg` of the first four characters
//! (with `mm` appended: the 6-char sub-square centre), then `ilong = (int)dlong`,
//! `ilat = (int)(dlat + 90)` and `((ilong + 180) / 2) * 180 + ilat`. Every one of those casts
//! is a C truncation toward zero applied to a FLOAT — `dlong` is `x.958` for eastern squares
//! and `−x.04` for western ones, so the order "add 90 in float, THEN truncate" (not the
//! reverse) is load-bearing: AA00 packs to 32220, and 16 200 grids would be off by one the
//! other way. Reproduced with f32 arithmetic and `as i32`. `unpackGrid`: integer arithmetic
//! `v % 180 − 90` and `v / 180 * 2 − 180 + 2`, then `deg2grid`, first four characters.
//!
//! Sentinels above the grid range (varicode.cpp:210-212): `nbasegrid` 32400 is the largest
//! grid value, 32401..=32409 unused, `nusergrid` 32410 + `packCmd` carries a command in a
//! compound-directed frame, `nmaxgrid` 32767 = "no grid" (B4 uses these; only the constants
//! live here).

/// Largest value that is a grid (`180 * 180`).
pub const NBASEGRID: u16 = 32_400;
/// Compound-directed frames carry `NUSERGRID + pack_cmd(...)` in the grid field.
pub const NUSERGRID: u16 = NBASEGRID + 10;
/// "No grid" (all 15 bits set).
pub const NMAXGRID: u16 = (1 << 15) - 1;

/// varicode.cpp:1104-1125 `grid2deg` for a 4-char grid + "mm": (dlong, dlat) in degrees, f32.
fn grid2deg(g: &[u8; 4]) -> (f32, f32) {
    let nlong = 180 - 20 * i32::from(g[0] - b'A');
    let n20d = 2 * i32::from(g[2] - b'0');
    let xminlong: f32 = 5.0 * (f32::from(b'm' - b'a') + 0.5);
    let dlong = nlong as f32 - n20d as f32 - xminlong / 60.0;
    let nlat = -90 + 10 * i32::from(g[1] - b'A') + i32::from(g[3] - b'0');
    let xminlat: f32 = 2.5 * (f32::from(b'm' - b'a') + 0.5);
    let dlat = nlat as f32 + xminlat / 60.0;
    (dlong, dlat)
}

/// varicode.cpp:1070-1102 `deg2grid`, first four characters only.
fn deg2grid4(mut dlong: f32, dlat: f32) -> String {
    if dlong < -180.0 {
        dlong += 360.0;
    }
    if dlong > 180.0 {
        dlong -= 360.0;
    }
    let nlong = (60.0 * (180.0 - f64::from(dlong)) / 5.0) as i32;
    let n1 = nlong / 240;
    let n2 = (nlong - 240 * n1) / 24;
    let nlat = (60.0 * (f64::from(dlat) + 90.0) / 2.5) as i32;
    let m1 = nlat / 240;
    let m2 = (nlat - 240 * m1) / 24;
    let chars = [
        b'A' + n1 as u8,
        b'A' + m1 as u8,
        b'0' + n2 as u8,
        b'0' + m2 as u8,
    ];
    String::from_utf8_lossy(&chars).into_owned()
}

/// 4-char grid (longer grids use their first four; case-insensitive; trimmed) → 15-bit value.
/// `None` when the first four characters are not `[A-R][A-R][0-9][0-9]` (upstream would return
/// `NMAXGRID` for a short string; the frame layer decides what "no grid" means).
pub fn pack_grid15(grid: &str) -> Option<u16> {
    let t = grid.trim();
    let b = t.as_bytes();
    if b.len() < 4 {
        return None;
    }
    let g = [
        b[0].to_ascii_uppercase(),
        b[1].to_ascii_uppercase(),
        b[2],
        b[3],
    ];
    if !(b'A'..=b'R').contains(&g[0])
        || !(b'A'..=b'R').contains(&g[1])
        || !g[2].is_ascii_digit()
        || !g[3].is_ascii_digit()
    {
        return None;
    }
    let (dlong, dlat) = grid2deg(&g);
    let ilong = dlong as i32; // C truncation toward zero
    let ilat = (dlat + 90.0) as i32; // float add THEN truncate (varicode.cpp:1137)
    let packed = ((ilong + 180) / 2) * 180 + ilat;
    Some(packed as u16)
}

/// 15-bit value → 4-char grid; `None` above [`NBASEGRID`] (varicode.cpp:1142-1151).
pub fn unpack_grid15(v: u16) -> Option<String> {
    if v > NBASEGRID {
        return None;
    }
    let v = i32::from(v);
    let dlat = (v % 180 - 90) as f32;
    let dlong = (v / 180 * 2 - 180 + 2) as f32;
    Some(deg2grid4(dlong, dlat))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vectors_under_js8calls_float_then_truncate_arithmetic() {
        // varicode.cpp:1129-1140 packGrid; values computed with upstream's exact float/int order
        // (ilat = (int)(dlat + 90.0f), NOT (int)dlat + 90 — AA00 is 32220, not 32221).
        for (grid, v) in [
            ("EN52", 24252u16),
            ("FN30", 22810),
            ("JO65", 15085),
            ("AA00", 32220),
            ("RR99", 179),
            ("EM73", 23883),
            ("KO85", 12925),
        ] {
            assert_eq!(pack_grid15(grid), Some(v), "{grid}");
            assert_eq!(unpack_grid15(v).as_deref(), Some(grid), "{v}");
        }
    }

    #[test]
    fn every_four_character_grid_round_trips() {
        for a in b'A'..=b'R' {
            for b in b'A'..=b'R' {
                for c in b'0'..=b'9' {
                    for d in b'0'..=b'9' {
                        let g = String::from_utf8(vec![a, b, c, d]).unwrap();
                        let v = pack_grid15(&g).unwrap_or_else(|| panic!("{g} did not pack"));
                        assert!(v <= NBASEGRID, "{g} → {v}");
                        assert_eq!(unpack_grid15(v).as_deref(), Some(g.as_str()));
                    }
                }
            }
        }
    }

    #[test]
    fn six_character_grids_use_their_first_four_and_case_is_folded() {
        assert_eq!(pack_grid15("EN52hw"), pack_grid15("EN52"));
        assert_eq!(pack_grid15("en52"), pack_grid15("EN52"));
        assert_eq!(pack_grid15(" EN52 "), pack_grid15("EN52"), "upstream trims");
    }

    #[test]
    fn invalid_grids_do_not_pack_and_reserved_values_do_not_unpack() {
        for bad in ["", "EN5", "EN", "ZZ00", "E52A", "1234", "EN5A"] {
            assert_eq!(pack_grid15(bad), None, "{bad:?}");
        }
        assert_eq!(
            unpack_grid15(NBASEGRID).as_deref(),
            Some("RA90"),
            "32400 is still a grid"
        );
        for v in [NBASEGRID + 1, NUSERGRID, NUSERGRID + 5, NMAXGRID] {
            assert_eq!(unpack_grid15(v), None, "{v}");
        }
        assert_eq!((NBASEGRID, NUSERGRID, NMAXGRID), (32400, 32410, 32767));
    }
}
