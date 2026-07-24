//! FlexRadio VITA-49 UDP stream decoder — the panadapter FFT frames the radio streams once a
//! `display pan` object is created (see [`crate::flexcat`]).
//!
//! Each datagram is a VITA-49 packet: a 32-bit header word, an optional stream id, an optional
//! class id (Flex OUI `0x1C2D`, packet class `0x8003` = FFT), optional timestamps, then the payload.
//! For an FFT packet the payload is FlexLib's `VitaFFTPacket`: `start_bin`, `num_bins`, `bin_size`,
//! `total_bins`, `frame_index`, then `num_bins` big-endian u16 magnitudes. A full sweep spans
//! several datagrams (MTU), reassembled by [`FftReassembler`] keyed on `frame_index`.
//!
//! All parsing is PURE + unit-tested against synthetic packets.
//!
//! HONESTY NOTE: written to the published VITA-49 layout + the open-source FlexLib
//! (`VitaFFTPacket`), unit-tested synthetically. The exact payload field order and the bin
//! magnitude SENSE (is a larger value a stronger or weaker signal?) are pinned from FlexLib but NOT
//! yet confirmed on live hardware — the orchestration flags this until an operator verifies it.

/// Flex's registered OUI in the VITA class id (24-bit).
pub const FLEX_OUI: u32 = 0x00_1C_2D;
/// VITA packet class code for panadapter FFT data.
pub const FFT_PACKET_CLASS: u16 = 0x8003;
/// DAX RX audio, uncompressed: **float32 interleaved stereo, big-endian, 24 kHz**. NOTE: this class
/// is shared with plain remote-network audio — a packet is DAX only when its stream id is one the
/// radio registered as a `dax_rx` stream, so dispatch must filter on stream id too.
pub const DAX_AUDIO_CLASS: u16 = 0x03E3;
/// DAX RX audio, reduced-bandwidth: **int16 mono, big-endian, 24 kHz**.
pub const DAX_AUDIO_REDUCED_CLASS: u16 = 0x0123;
/// DAX (and Flex network) audio sample rate.
pub const DAX_SAMPLE_RATE: u32 = 24_000;

/// A decoded VITA-49 packet envelope (header fields + payload slice). Borrows the datagram.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VitaPacket<'a> {
    pub packet_type: u8,
    pub stream_id: Option<u32>,
    /// 24-bit OUI from the class id (Flex = [`FLEX_OUI`]), if a class id was present.
    pub class_oui: Option<u32>,
    /// Packet class code (`0x8003` = FFT), if a class id was present.
    pub packet_class: Option<u16>,
    /// A 4-byte VITA trailer follows the payload (word0 bit 26). The audio decoders strip it.
    pub has_trailer: bool,
    pub payload: &'a [u8],
}

fn be_u32(b: &[u8], off: usize) -> Option<u32> {
    b.get(off..off + 4)
        .map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}
fn be_u16(b: &[u8], off: usize) -> Option<u16> {
    b.get(off..off + 2)
        .map(|s| u16::from_be_bytes([s[0], s[1]]))
}

/// Parse the VITA-49 header and return the envelope + payload slice. `None` on a short/malformed
/// datagram. Pure.
pub fn parse_vita(dg: &[u8]) -> Option<VitaPacket<'_>> {
    let w0 = be_u32(dg, 0)?;
    let packet_type = ((w0 >> 28) & 0xF) as u8;
    let class_present = (w0 >> 27) & 1 == 1;
    let has_trailer = (w0 >> 26) & 1 == 1;
    let tsi = (w0 >> 22) & 0x3; // integer-seconds timestamp mode
    let tsf = (w0 >> 20) & 0x3; // fractional-seconds timestamp mode
    let mut off = 4usize;
    // Data packet types that carry a stream id (1 = IF data w/ stream id, 3 = ext data w/ stream
    // id, 5 = ext context w/ stream id). Flex FFT rides a stream-id-bearing data packet.
    let stream_id = if matches!(packet_type, 1 | 3 | 5) {
        let s = be_u32(dg, off)?;
        off += 4;
        Some(s)
    } else {
        None
    };
    let (class_oui, packet_class) = if class_present {
        let oui_word = be_u32(dg, off)?;
        let class_word = be_u32(dg, off + 4)?;
        off += 8;
        (
            Some(oui_word & 0x00FF_FFFF),
            Some((class_word & 0xFFFF) as u16),
        )
    } else {
        (None, None)
    };
    if tsi != 0 {
        off += 4; // integer-seconds timestamp word
    }
    if tsf != 0 {
        off += 8; // fractional-seconds timestamp (two words)
    }
    if off > dg.len() {
        return None;
    }
    Some(VitaPacket {
        packet_type,
        stream_id,
        class_oui,
        packet_class,
        has_trailer,
        payload: &dg[off..],
    })
}

/// Decode a DAX RX audio payload into **mono 24 kHz f32** samples (−1.0..1.0). `packet_class` selects
/// the format: [`DAX_AUDIO_CLASS`] `0x03E3` = big-endian float32 interleaved stereo (L+R averaged to
/// mono); [`DAX_AUDIO_REDUCED_CLASS`] `0x0123` = big-endian int16 mono. A 4-byte VITA trailer, when
/// present, is stripped first. Pure — mirrors AetherSDR's PanadapterStream audio decode. `None` for
/// a non-audio class.
pub fn parse_dax_audio(packet_class: u16, payload: &[u8], has_trailer: bool) -> Option<Vec<f32>> {
    let body = if has_trailer {
        payload.get(..payload.len().checked_sub(4)?)?
    } else {
        payload
    };
    match packet_class {
        DAX_AUDIO_CLASS => {
            // float32 big-endian, interleaved stereo → average each L/R pair to mono.
            let stereo: Vec<f32> = body
                .chunks_exact(4)
                .map(|c| f32::from_be_bytes([c[0], c[1], c[2], c[3]]))
                .collect();
            Some(
                stereo
                    .chunks_exact(2)
                    .map(|lr| 0.5 * (lr[0] + lr[1]))
                    .collect(),
            )
        }
        DAX_AUDIO_REDUCED_CLASS => {
            // int16 big-endian, mono → normalize to −1.0..1.0.
            Some(
                body.chunks_exact(2)
                    .map(|c| i16::from_be_bytes([c[0], c[1]]) as f32 / 32768.0)
                    .collect(),
            )
        }
        _ => None,
    }
}

/// One FFT payload fragment (a contiguous slice `start_bin..start_bin+num_bins` of the sweep).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FftFrame {
    pub start_bin: u16,
    pub num_bins: u16,
    pub total_bins: u16,
    pub frame_index: u32,
    pub bins: Vec<u16>,
}

/// Parse an FFT packet payload (`VitaFFTPacket`) into a fragment. Pure. `None` if truncated.
pub fn parse_fft(payload: &[u8]) -> Option<FftFrame> {
    let start_bin = be_u16(payload, 0)?;
    let num_bins = be_u16(payload, 2)?;
    let _bin_size = be_u16(payload, 4)?;
    let total_bins = be_u16(payload, 6)?;
    let frame_index = be_u32(payload, 8)?;
    let mut bins = Vec::with_capacity(num_bins as usize);
    let mut o = 12usize;
    for _ in 0..num_bins {
        bins.push(be_u16(payload, o)?);
        o += 2;
    }
    Some(FftFrame {
        start_bin,
        num_bins,
        total_bins,
        frame_index,
        bins,
    })
}

/// Reassembles the multi-datagram fragments of one FFT sweep into a full row of `total_bins` values.
#[derive(Debug, Default)]
pub struct FftReassembler {
    frame_index: Option<u32>,
    total: u16,
    bins: Vec<u16>,
    filled: usize,
}

impl FftReassembler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Push a fragment. Returns `Some(row)` (length `total_bins`) once the sweep it belongs to is
    /// complete; a fragment for a NEW frame_index discards any incomplete previous sweep (so a
    /// dropped fragment just costs one frame, then resync).
    pub fn push(&mut self, f: &FftFrame) -> Option<Vec<u16>> {
        if Some(f.frame_index) != self.frame_index {
            self.frame_index = Some(f.frame_index);
            self.total = f.total_bins;
            self.bins = vec![0u16; f.total_bins as usize];
            self.filled = 0;
        }
        let start = f.start_bin as usize;
        for (i, &b) in f.bins.iter().enumerate() {
            if let Some(slot) = self.bins.get_mut(start + i) {
                *slot = b;
            }
        }
        self.filled += f.bins.len();
        if self.total > 0 && self.filled >= self.total as usize {
            self.frame_index = None; // sweep consumed; next fragment starts fresh
            self.filled = 0;
            Some(std::mem::take(&mut self.bins))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal VITA-49 FFT datagram: type=1 (stream id), class present, no timestamps.
    fn vita_fft(stream_id: u32, payload: &[u8]) -> Vec<u8> {
        let mut d = Vec::new();
        // word0: type=1 (bits 28-31), C=1 (bit 27), tsi=0, tsf=0.
        let w0: u32 = (1 << 28) | (1 << 27);
        d.extend_from_slice(&w0.to_be_bytes());
        d.extend_from_slice(&stream_id.to_be_bytes());
        d.extend_from_slice(&FLEX_OUI.to_be_bytes()); // OUI word (upper byte 0)
        d.extend_from_slice(&(FFT_PACKET_CLASS as u32).to_be_bytes()); // class word
        d.extend_from_slice(payload);
        d
    }

    fn fft_payload(start: u16, num: u16, total: u16, frame: u32, bins: &[u16]) -> Vec<u8> {
        let mut p = Vec::new();
        p.extend_from_slice(&start.to_be_bytes());
        p.extend_from_slice(&num.to_be_bytes());
        p.extend_from_slice(&0u16.to_be_bytes()); // bin_size
        p.extend_from_slice(&total.to_be_bytes());
        p.extend_from_slice(&frame.to_be_bytes());
        for b in bins {
            p.extend_from_slice(&b.to_be_bytes());
        }
        p
    }

    #[test]
    fn parses_a_vita_fft_envelope() {
        let dg = vita_fft(0x4200_0000, &fft_payload(0, 2, 2, 1, &[10, 20]));
        let v = parse_vita(&dg).unwrap();
        assert_eq!(v.packet_type, 1);
        assert_eq!(v.stream_id, Some(0x4200_0000));
        assert_eq!(v.class_oui, Some(FLEX_OUI));
        assert_eq!(v.packet_class, Some(FFT_PACKET_CLASS));
        let f = parse_fft(v.payload).unwrap();
        assert_eq!(f.total_bins, 2);
        assert_eq!(f.bins, vec![10, 20]);
    }

    #[test]
    fn short_datagram_is_none() {
        assert!(parse_vita(&[0u8; 2]).is_none());
        assert!(parse_fft(&[0u8; 6]).is_none());
    }

    #[test]
    fn reassembles_a_multi_fragment_sweep() {
        let mut r = FftReassembler::new();
        // Sweep frame 7, total 4 bins, delivered in two fragments.
        let a = parse_fft(&fft_payload(0, 2, 4, 7, &[1, 2])).unwrap();
        let b = parse_fft(&fft_payload(2, 2, 4, 7, &[3, 4])).unwrap();
        assert_eq!(r.push(&a), None); // incomplete
        assert_eq!(r.push(&b), Some(vec![1, 2, 3, 4])); // complete row
    }

    #[test]
    fn a_new_frame_index_drops_the_incomplete_previous_sweep() {
        let mut r = FftReassembler::new();
        let stale = parse_fft(&fft_payload(0, 2, 4, 7, &[1, 2])).unwrap(); // frame 7, incomplete
        let next = parse_fft(&fft_payload(0, 4, 4, 8, &[5, 6, 7, 8])).unwrap(); // frame 8, complete
        assert_eq!(r.push(&stale), None);
        assert_eq!(r.push(&next), Some(vec![5, 6, 7, 8])); // resynced on the new frame
    }

    #[test]
    fn decodes_dax_float32_stereo_to_mono() {
        // Two stereo frames: (0.5,0.5)→0.5 and (1.0,0.0)→0.5.
        let mut p = Vec::new();
        for v in [0.5f32, 0.5, 1.0, 0.0] {
            p.extend_from_slice(&v.to_be_bytes());
        }
        let mono = parse_dax_audio(DAX_AUDIO_CLASS, &p, false).unwrap();
        assert_eq!(mono.len(), 2);
        assert!((mono[0] - 0.5).abs() < 1e-6);
        assert!((mono[1] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn decodes_dax_int16_mono() {
        let mut p = Vec::new();
        for s in [16384i16, -32768, 0] {
            p.extend_from_slice(&s.to_be_bytes());
        }
        let mono = parse_dax_audio(DAX_AUDIO_REDUCED_CLASS, &p, false).unwrap();
        assert_eq!(mono, vec![0.5, -1.0, 0.0]);
    }

    #[test]
    fn dax_strips_the_vita_trailer_before_decoding() {
        // One float32 stereo frame (0.25, 0.75) + a 4-byte trailer.
        let mut p = Vec::new();
        for v in [0.25f32, 0.75] {
            p.extend_from_slice(&v.to_be_bytes());
        }
        p.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]); // trailer, must not be decoded as audio
        let mono = parse_dax_audio(DAX_AUDIO_CLASS, &p, true).unwrap();
        assert_eq!(mono.len(), 1);
        assert!((mono[0] - 0.5).abs() < 1e-6); // (0.25 + 0.75) / 2
    }

    #[test]
    fn parse_dax_audio_rejects_a_non_audio_class() {
        assert!(parse_dax_audio(FFT_PACKET_CLASS, &[0u8; 8], false).is_none());
    }
}
