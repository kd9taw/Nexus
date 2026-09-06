//! MSB-first bit packing — the one bit-I/O helper every proto pack/unpack uses.
//!
//! JS8Call builds every 72-bit payload by string-concatenating binary digits and reading them
//! back big-endian (varicode.cpp `pack*Frame` / `unpack*Frame` via QString bit strings). The
//! same order here: the first pushed bit is the most significant bit of the first byte.
//!
//! WHY overrun panics instead of returning zeros: every JS8 layout is a FIXED 72-bit field
//! (`[3][50][11] , [5][3]`, `[3][28][28][5] , [1][1][6]`, …), so a read past the end is a
//! layout bug in this crate, never something the air can cause. A zero would be a sentinel
//! that forces the consumer to guess (feedback-sentinels-force-consumers-to-guess).

/// Accumulates bits MSB-first; [`BitWriter::finish`] pads the final byte with zeros.
#[derive(Debug, Default, Clone)]
pub struct BitWriter {
    bytes: Vec<u8>,
    nbits: usize,
}

impl BitWriter {
    pub fn new() -> BitWriter {
        BitWriter::default()
    }

    /// Append the low `nbits` bits of `value` (1..=64), most significant first.
    pub fn push(&mut self, value: u64, nbits: u32) {
        assert!((1..=64).contains(&nbits), "BitWriter::push: nbits {nbits}");
        for k in (0..nbits).rev() {
            let bit = ((value >> k) & 1) as u8;
            if self.nbits.is_multiple_of(8) {
                self.bytes.push(0);
            }
            if bit == 1 {
                let last = self.bytes.len() - 1;
                self.bytes[last] |= 0x80 >> (self.nbits % 8);
            }
            self.nbits += 1;
        }
    }

    pub fn len_bits(&self) -> usize {
        self.nbits
    }

    pub fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

/// Reads bits MSB-first from a byte slice.
#[derive(Debug, Clone)]
pub struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> BitReader<'a> {
    pub fn new(data: &'a [u8]) -> BitReader<'a> {
        BitReader { data, pos: 0 }
    }

    /// Read `nbits` (1..=64) as an unsigned value. Panics on overrun — see the module header.
    pub fn read(&mut self, nbits: u32) -> u64 {
        assert!((1..=64).contains(&nbits), "BitReader::read: nbits {nbits}");
        assert!(
            self.pos + nbits as usize <= self.data.len() * 8,
            "BitReader overrun: {} bits requested at bit {} of {}",
            nbits,
            self.pos,
            self.data.len() * 8
        );
        let mut v = 0u64;
        for _ in 0..nbits {
            let bit = (self.data[self.pos / 8] >> (7 - self.pos % 8)) & 1;
            v = (v << 1) | u64::from(bit);
            self.pos += 1;
        }
        v
    }

    /// Bits not yet read.
    pub fn remaining(&self) -> usize {
        self.data.len() * 8 - self.pos
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writer_is_msb_first_and_pads_the_last_byte_with_zeros() {
        let mut w = BitWriter::new();
        w.push(0b101, 3);
        w.push(0b0001, 4);
        assert_eq!(w.len_bits(), 7);
        assert_eq!(w.finish(), vec![0b1010_0010]);
        let mut w = BitWriter::new();
        w.push(0x1FF, 9);
        w.push(0, 7);
        assert_eq!(w.finish(), vec![0xFF, 0x80]);
    }

    #[test]
    fn writer_accepts_values_wider_than_the_field_by_masking() {
        let mut w = BitWriter::new();
        w.push(0xFFFF_FFFF_FFFF_FFFF, 3);
        assert_eq!(w.finish(), vec![0b1110_0000]);
    }

    #[test]
    fn reader_round_trips_the_directed_frame_layout() {
        // [3][28][28][5][1][1][6] = 72 bits (varicode.cpp:1542-1683).
        let mut w = BitWriter::new();
        w.push(0b011, 3);
        w.push(144_467_410, 28);
        w.push(261_410_543, 28);
        w.push(29, 5);
        w.push(1, 1);
        w.push(0, 1);
        w.push(38, 6);
        let bytes = w.finish();
        assert_eq!(bytes.len(), 9);
        let mut r = BitReader::new(&bytes);
        assert_eq!(r.remaining(), 72);
        assert_eq!(r.read(3), 0b011);
        assert_eq!(r.read(28), 144_467_410);
        assert_eq!(r.read(28), 261_410_543);
        assert_eq!(r.read(5), 29);
        assert_eq!(r.read(1), 1);
        assert_eq!(r.read(1), 0);
        assert_eq!(r.read(6), 38);
        assert_eq!(r.remaining(), 0);
    }

    #[test]
    fn reader_reads_64_bit_fields() {
        let mut w = BitWriter::new();
        w.push(0xDEAD_BEEF_CAFE_F00D, 64);
        w.push(0b11, 2);
        let bytes = w.finish();
        let mut r = BitReader::new(&bytes);
        assert_eq!(r.read(64), 0xDEAD_BEEF_CAFE_F00D);
        assert_eq!(r.read(2), 0b11);
    }

    #[test]
    #[should_panic(expected = "BitReader overrun")]
    fn reader_overrun_is_a_bug_not_data() {
        let mut r = BitReader::new(&[0xFF]);
        r.read(9);
    }
}
