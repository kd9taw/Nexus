//! The two JS8 Costas 7×7 sync kernels — transcribed FACTS.
//!
//! JS8Call JS8.hpp:30-46 / lib/js8/genjs8.f90:24-32: JS8 originally used FT8's Costas array
//! and Normal still does (`ORIGINAL`, the same row three times); every other speed uses three
//! DIFFERENT rows (`MODIFIED`). Note this is FT8's ORIGINAL array {4,2,5,6,1,3,0}, not the
//! "flipped" {3,1,4,0,6,5,2} in Nexus's vendored sync8.f90 — the vendored FT8 sync cannot be
//! reused for any JS8 speed. Nothing else lives here: sync and demod are table-driven through
//! [`crate::phy::Speed::costas`], never branched on speed inside the DSP.
//!
//! Both tables are `static`, not the `const` interfaces.md §1.1/§1.2 literally names — a
//! deliberate, reported deviation. A `const`'s address is not guaranteed stable across
//! reference sites (each `&CONST` may promote to its own anonymous allocation; confirmed here
//! — the unit test's `&COSTAS_ORIGINAL` and this table's own address differed under a plain
//! `const`), so [`crate::phy::Speed::costas`]'s `std::ptr::eq` contract — "this speed's kernel
//! IS `COSTAS_ORIGINAL`, not a copy of it" — needs the single fixed location only `static`
//! guarantees. Every value and call-site usage is identical either way; only address
//! stability differs.

/// Normal (submode A): FT8's original array in all three sync blocks.
pub static COSTAS_ORIGINAL: [[u8; 7]; 3] = [[4, 2, 5, 6, 1, 3, 0]; 3];

/// Slow / Fast / Turbo (submodes E / B / C): beginning, middle and end blocks differ.
pub static COSTAS_MODIFIED: [[u8; 7]; 3] = [
    [0, 6, 2, 3, 5, 4, 1],
    [1, 5, 0, 2, 3, 6, 4],
    [2, 5, 0, 6, 4, 1, 3],
];

#[cfg(test)]
mod tests {
    use super::*;

    /// A Costas array is a permutation of the 7 tones — the sync correlator relies on it.
    fn is_permutation_of_0_to_6(row: &[u8; 7]) -> bool {
        let mut seen = [false; 7];
        for &t in row {
            if t > 6 || seen[t as usize] {
                return false;
            }
            seen[t as usize] = true;
        }
        true
    }

    #[test]
    fn every_costas_row_is_a_permutation_of_the_seven_tones() {
        for row in COSTAS_ORIGINAL.iter().chain(COSTAS_MODIFIED.iter()) {
            assert!(is_permutation_of_0_to_6(row), "row {row:?}");
        }
    }

    #[test]
    fn original_repeats_ft8s_array_and_modified_uses_three_distinct_arrays() {
        // JS8.hpp:30-46 — Normal keeps FT8's original array three times; the others differ per block.
        assert_eq!(COSTAS_ORIGINAL, [[4, 2, 5, 6, 1, 3, 0]; 3]);
        assert_ne!(COSTAS_MODIFIED[0], COSTAS_MODIFIED[1]);
        assert_ne!(COSTAS_MODIFIED[1], COSTAS_MODIFIED[2]);
        assert_ne!(COSTAS_MODIFIED[0], COSTAS_MODIFIED[2]);
    }
}
