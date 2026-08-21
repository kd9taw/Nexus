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
    /// The 4-bit VITA packet count (word0 bits 16-19) — the ONLY loss/ordering signal on this
    /// wire. It was written by [`build_dax_tx_packet`] on the TX side and never read on the RX
    /// side (audit #1008/#1052), so a dropped DAX audio datagram was spliced over silently and
    /// every later decode in the window sat at the wrong dt. See [`VitaSequence`].
    pub packet_count: u8,
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
    let packet_count = ((w0 >> 16) & 0xF) as u8;
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
        packet_count,
        payload: &dg[off..],
    })
}

/// What to do with the packet whose count was just [`observed`](VitaSequence::observe).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VitaGap {
    /// Next in sequence (or the first of a stream) — append it.
    InSequence,
    /// `n` packets were lost before this one. Append `n` packets' worth of SILENCE first, then
    /// this packet: the ring is a pure sample store where position implies time, so filling the
    /// hole with its own duration is what keeps the decode window's t=0 honest. Dropping the
    /// silence instead slides every later sample earlier by the lost airtime.
    Lost(u32),
    /// A duplicate, or a straggler that arrived after its successors — DROP it. Splicing it in
    /// would write a hard discontinuity into the middle of the audio.
    Stale,
}

/// Per-stream continuity for the 4-bit VITA packet count.
///
/// ⚠️ THE RX SIDE HAD NO ORDERING STATE AT ALL (audit #1008/#1052). DAX audio was appended to the
/// ring in arrival order — `r.extend_from_slice(&mono12)` — with nothing anywhere to notice a lost
/// or reordered datagram, while the TX builder maintained the counter faithfully. `RxRing` is a
/// pure sample ring: N lost samples make the window span N/12000 s more wall clock than the
/// decoder assumes, which shifts the dt of every decode inside it. The FFT path already resyncs on
/// loss; the audio path did not.
///
/// The counter wraps every 16 packets (~85 ms of DAX audio), so the split between "lost" and
/// "reordered" is a half-space rule: a forward jump of up to 8 reads as loss, more than that reads
/// as a straggler. INHERENT LIMIT, stated rather than hidden: a loss of exactly 16 packets (or any
/// multiple) is invisible to a 4-bit counter — no reading of the wire can see it.
#[derive(Debug, Default, Clone, Copy)]
pub struct VitaSequence {
    last: Option<u8>,
}

impl VitaSequence {
    /// Fold in the next packet's count and say what the caller should do with it.
    pub fn observe(&mut self, count: u8) -> VitaGap {
        let count = count & 0xF;
        let Some(last) = self.last.replace(count) else {
            return VitaGap::InSequence; // first packet of the stream
        };
        match count.wrapping_sub(last) & 0xF {
            0 => {
                self.last = Some(last); // duplicate: the sequence has not moved
                VitaGap::Stale
            }
            1 => VitaGap::InSequence,
            d if d <= 8 => VitaGap::Lost(u32::from(d) - 1),
            _ => {
                self.last = Some(last); // a straggler behind the stream: ignore, don't rewind
                VitaGap::Stale
            }
        }
    }
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

/// VITA meter-data packet class.
pub const METER_PACKET_CLASS: u16 = 0x8002;

/// Decode a meter VITA payload (`PCC 0x8002`) into `(meter_id, raw_value)` pairs — `uint16` id +
/// `int16` raw, both big-endian. Strips the optional VITA trailer. The raw value is scaled to a real
/// unit by [`convert_meter_raw`], keyed on the meter's UNIT (learned from the control-plane meter
/// definition, [`crate::flexcat::parse_meter_defs`]). Pure.
pub fn parse_meter_values(payload: &[u8], has_trailer: bool) -> Vec<(u16, i16)> {
    let body = if has_trailer {
        payload
            .get(..payload.len().saturating_sub(4))
            .unwrap_or(&[])
    } else {
        payload
    };
    body.chunks_exact(4)
        .map(|c| {
            (
                u16::from_be_bytes([c[0], c[1]]),
                i16::from_be_bytes([c[2], c[3]]),
            )
        })
        .collect()
}

/// Convert a raw int16 meter value to its real unit, keyed on the meter's unit string (from FlexLib
/// `Meter.cs`): `dBm`/`dB`/`dBFS`/`SWR` → ÷128; `Volts`/`Amps` → ÷256; `degF`/`degC` → ÷64; else the
/// raw value unscaled.
pub fn convert_meter_raw(unit: &str, raw: i16) -> f32 {
    let raw = raw as f32;
    match unit {
        "dBm" | "dB" | "dBFS" | "SWR" => raw / 128.0,
        "Volts" | "Amps" => raw / 256.0,
        "degF" | "degC" => raw / 64.0,
        _ => raw,
    }
}

/// FlexRadio forward/reflected power meters report **dBm**; convert to watts. `w = 10^(dBm/10)/1000`.
pub fn dbm_to_watts(dbm: f32) -> f32 {
    10f32.powf(dbm / 10.0) / 1000.0
}

/// FlexRadio VITA information-class code ("SL"), the upper half of the class-id word on TX packets.
pub const FLEX_INFO_CLASS: u16 = 0x534C;

/// Build a DAX **TX** VITA-49 packet carrying `samples` as big-endian int16 mono (PCC
/// [`DAX_AUDIO_REDUCED_CLASS`] `0x0123`, the radio-native DAX-TX route). Header per AetherSDR's
/// `buildVitaTxPacket`: type 1 (IFDataWithStream), class present, no trailer, TSI=3, TSF=1, a 4-bit
/// `packet_count`, and the 16-bit size in 32-bit words. `samples.len()` must be even (a whole number
/// of 32-bit words); the DAX-TX packetizer sends 128 samples/packet. 24 kHz. Pure.
pub fn build_dax_tx_packet(stream_id: u32, packet_count: u8, samples: &[i16]) -> Vec<u8> {
    let payload_bytes = samples.len() * 2;
    let total_words = 7 + payload_bytes / 4; // 7 header words + payload words
    let mut w0: u32 = 0;
    w0 |= 0x1 << 28; // packet_type = 1 (IFDataWithStream)
    w0 |= 1 << 27; // class id present
    w0 |= 0x3 << 22; // TSI = 3 (Other)
    w0 |= 0x1 << 20; // TSF = 1 (SampleCount)
    w0 |= (u32::from(packet_count) & 0xF) << 16;
    w0 |= (total_words as u32) & 0xFFFF;
    let class_word = (u32::from(FLEX_INFO_CLASS) << 16) | u32::from(DAX_AUDIO_REDUCED_CLASS);
    let mut out = Vec::with_capacity(total_words * 4);
    out.extend_from_slice(&w0.to_be_bytes());
    out.extend_from_slice(&stream_id.to_be_bytes());
    out.extend_from_slice(&FLEX_OUI.to_be_bytes()); // word2: OUI (upper byte 0)
    out.extend_from_slice(&class_word.to_be_bytes()); // word3: info class | PCC
    out.extend_from_slice(&[0u8; 12]); // words 4-6: timestamps zero
    for &s in samples {
        out.extend_from_slice(&s.to_be_bytes());
    }
    out
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

/// How many sweeps may be part-assembled at once. UDP reordering is a packet or two deep, not a
/// dozen; three sweeps of 2048 bins is a few KB, and the oldest is evicted rather than grown.
const IN_FLIGHT_SWEEPS: usize = 3;

/// One sweep being reassembled: its bins plus WHICH of them have actually been written.
#[derive(Debug)]
struct Sweep {
    frame_index: u32,
    total: u16,
    bins: Vec<u16>,
    /// Per-bin coverage. A running count of bins *pushed* is not the same thing — see
    /// [`FftReassembler`].
    covered: Vec<bool>,
    filled: usize,
}

/// Reassembles the multi-datagram fragments of one FFT sweep into a full row of `total_bins` values.
///
/// ⚠️ TWO DEFECTS THIS SHAPE EXISTS TO FIX (audit #1009, both reproduced against the old code).
///
/// 1. **A single current-sweep slot meant UDP reordering across sweeps produced NO row at all.**
///    The old `push` reset everything whenever `frame_index` changed, so the delivery order
///    `A1 B1 A2 B2 …` — one sweep's tail arriving after the next sweep's head, which is ordinary
///    UDP — completed zero of six sweeps in a lifted-out reproduction: a permanently blank
///    panadapter, not a degraded one. A small in-flight map absorbs that.
/// 2. **A duplicate fragment could COMPLETE a sweep.** Completion counted bins *pushed*
///    (`self.filled += f.bins.len()`), so the same fragment twice satisfied a four-bin sweep and
///    published a row whose other half was still the `vec![0u16]` fill — and `fft_to_row` maps 0
///    to 0.0, so the hole paints as the waterfall floor: a false dead-spectrum band, indis-
///    tinguishable from a quiet part of the band. Coverage is per-bin now, so re-writing a bin
///    never advances completion.
#[derive(Debug, Default)]
pub struct FftReassembler {
    /// Part-assembled sweeps in arrival order (oldest first, evicted from the front).
    inflight: Vec<Sweep>,
}

impl FftReassembler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Push a fragment. Returns `Some(row)` (length `total_bins`) once the sweep it belongs to is
    /// covered end to end. Sweeps still missing a fragment age out as newer ones arrive, so a lost
    /// fragment costs its own frame and nothing else.
    pub fn push(&mut self, f: &FftFrame) -> Option<Vec<u16>> {
        let pos = match self
            .inflight
            .iter()
            .position(|s| s.frame_index == f.frame_index)
        {
            Some(i) => i,
            None => {
                if self.inflight.len() >= IN_FLIGHT_SWEEPS {
                    self.inflight.remove(0); // oldest incomplete sweep gives up its place
                }
                self.inflight.push(Sweep {
                    frame_index: f.frame_index,
                    total: f.total_bins,
                    bins: vec![0u16; f.total_bins as usize],
                    covered: vec![false; f.total_bins as usize],
                    filled: 0,
                });
                self.inflight.len() - 1
            }
        };
        let sweep = &mut self.inflight[pos];
        let start = f.start_bin as usize;
        for (i, &b) in f.bins.iter().enumerate() {
            let Some(slot) = sweep.bins.get_mut(start + i) else {
                continue; // a fragment past the declared width — ignore, never grow
            };
            *slot = b;
            if let Some(seen) = sweep.covered.get_mut(start + i) {
                if !*seen {
                    *seen = true;
                    sweep.filled += 1;
                }
            }
        }
        if sweep.total > 0 && sweep.filled >= sweep.total as usize {
            return Some(self.inflight.remove(pos).bins);
        }
        None
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
    fn an_incomplete_sweep_never_blocks_a_later_one() {
        let mut r = FftReassembler::new();
        let stale = parse_fft(&fft_payload(0, 2, 4, 7, &[1, 2])).unwrap(); // frame 7, incomplete
        let next = parse_fft(&fft_payload(0, 4, 4, 8, &[5, 6, 7, 8])).unwrap(); // frame 8, complete
        assert_eq!(r.push(&stale), None);
        assert_eq!(r.push(&next), Some(vec![5, 6, 7, 8])); // frame 8 stands on its own
    }

    /// UDP REORDERING ACROSS SWEEPS MUST STILL PAINT A WATERFALL (audit #1009).
    ///
    /// The old single-slot reassembler reset on any `frame_index` change, so this delivery order —
    /// each sweep's tail arriving after the next sweep's head, which is ordinary UDP — completed
    /// ZERO of six sweeps: a permanently blank panadapter.
    #[test]
    fn a_sweep_still_completes_when_its_fragments_arrive_out_of_order() {
        let mut r = FftReassembler::new();
        let head = |frame: u32| parse_fft(&fft_payload(0, 2, 4, frame, &[1, 2])).unwrap();
        let tail = |frame: u32| parse_fft(&fft_payload(2, 2, 4, frame, &[3, 4])).unwrap();

        let mut rows = 0;
        // Interleaved: head(n), head(n+1), tail(n), tail(n+1), …
        for n in 0..6u32 {
            r.push(&head(n));
            if n > 0 && r.push(&tail(n - 1)).is_some() {
                rows += 1;
            }
        }
        assert!(
            rows >= 5,
            "{rows} rows from six interleaved sweeps — reordering must cost at most the frame it \
             touches, not the whole waterfall"
        );
    }

    /// A DUPLICATE FRAGMENT MUST NOT COMPLETE A SWEEP (audit #1009).
    ///
    /// Completion counted bins PUSHED, so the same fragment twice satisfied a four-bin sweep and
    /// published a row whose uncovered half was still the zero fill — and zero paints as the
    /// waterfall floor, i.e. a false dead-spectrum band rather than an obvious artifact.
    #[test]
    fn a_duplicated_fragment_cannot_complete_a_half_covered_sweep() {
        let mut r = FftReassembler::new();
        let half = parse_fft(&fft_payload(0, 2, 4, 3, &[9, 9])).unwrap();
        assert_eq!(r.push(&half), None);
        assert_eq!(
            r.push(&half),
            None,
            "the same two bins twice is still half a sweep"
        );
        // The real second half completes it, with no zero hole.
        let rest = parse_fft(&fft_payload(2, 2, 4, 3, &[7, 8])).unwrap();
        assert_eq!(r.push(&rest), Some(vec![9, 9, 7, 8]));
    }

    /// The in-flight map is BOUNDED: a radio streaming sweeps whose fragments never arrive cannot
    /// grow it (the reason the old single-slot design was safe, kept).
    #[test]
    fn part_assembled_sweeps_are_bounded() {
        let mut r = FftReassembler::new();
        for frame in 0..50u32 {
            assert_eq!(
                r.push(&parse_fft(&fft_payload(0, 1, 4, frame, &[1])).unwrap()),
                None
            );
        }
        assert_eq!(r.inflight.len(), IN_FLIGHT_SWEEPS);
    }

    /// THE PACKET COUNT IS THE ONLY LOSS SIGNAL ON THIS WIRE, and the RX side never read it
    /// (audit #1008/#1052). Round-tripped through our own TX builder as the positive control that
    /// the field is where we think it is.
    #[test]
    fn the_vita_packet_count_survives_a_round_trip() {
        for count in 0..16u8 {
            let dg = build_dax_tx_packet(0x0600_0000, count, &[1i16, 2]);
            assert_eq!(parse_vita(&dg).unwrap().packet_count, count);
        }
    }

    /// LOSS MUST BE FILLED, NOT SPLICED OVER (audit #1008/#1052). The audio ring is a pure sample
    /// store where position implies time, so a dropped datagram that is simply skipped slides
    /// every later sample earlier by its airtime — the decode window's t=0 moves and every dt in
    /// it is wrong. Duplicates and stragglers must be dropped rather than written mid-stream.
    #[test]
    fn the_dax_sequencer_separates_loss_from_reordering() {
        let mut s = VitaSequence::default();
        assert_eq!(
            s.observe(5),
            VitaGap::InSequence,
            "first packet of a stream"
        );
        assert_eq!(s.observe(6), VitaGap::InSequence);
        // Two lost: 7 and 8 never arrived.
        assert_eq!(s.observe(9), VitaGap::Lost(2));
        assert_eq!(s.observe(10), VitaGap::InSequence);
        // The 4-bit counter wraps without looking like a loss.
        for c in [11, 12, 13, 14, 15, 0, 1] {
            assert_eq!(s.observe(c), VitaGap::InSequence, "count {c} follows on");
        }
        // A duplicate is dropped and does not move the sequence on.
        assert_eq!(s.observe(1), VitaGap::Stale);
        assert_eq!(s.observe(2), VitaGap::InSequence);
        // A straggler from behind the stream is dropped, not spliced in …
        assert_eq!(s.observe(0), VitaGap::Stale);
        // … and the stream carries on from where it was.
        assert_eq!(s.observe(3), VitaGap::InSequence);
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

    #[test]
    fn parses_meter_value_pairs() {
        let mut p = Vec::new();
        for (id, raw) in [(7u16, 1280i16), (12, -256)] {
            p.extend_from_slice(&id.to_be_bytes());
            p.extend_from_slice(&raw.to_be_bytes());
        }
        assert_eq!(parse_meter_values(&p, false), vec![(7, 1280), (12, -256)]);
    }

    #[test]
    fn dax_tx_packet_round_trips_through_the_vita_parser() {
        let samples: Vec<i16> = vec![100, -200, 16384, -16384]; // even count = whole words
        let dg = build_dax_tx_packet(0x0600_0000, 3, &samples);
        let pkt = parse_vita(&dg).unwrap();
        assert_eq!(pkt.packet_type, 1);
        assert_eq!(pkt.stream_id, Some(0x0600_0000));
        assert_eq!(pkt.class_oui, Some(FLEX_OUI));
        assert_eq!(pkt.packet_class, Some(DAX_AUDIO_REDUCED_CLASS));
        assert!(!pkt.has_trailer);
        // The payload decodes back to the same samples (as normalized f32).
        let back = parse_dax_audio(DAX_AUDIO_REDUCED_CLASS, pkt.payload, pkt.has_trailer).unwrap();
        assert_eq!(back.len(), samples.len());
        for (a, b) in back.iter().zip(&samples) {
            assert!((a - *b as f32 / 32768.0).abs() < 1e-4);
        }
    }

    #[test]
    fn meter_raw_conversions_follow_the_unit() {
        assert!((convert_meter_raw("dBm", 1280) - 10.0).abs() < 1e-4); // 1280/128 dBm
        assert!((convert_meter_raw("SWR", 192) - 1.5).abs() < 1e-4); // 192/128 ratio
        assert!((convert_meter_raw("Volts", 3520) - 13.75).abs() < 1e-3); // 3520/256 V
        assert!((convert_meter_raw("degC", 1600) - 25.0).abs() < 1e-3); // 1600/64 °C
        assert_eq!(convert_meter_raw("Percent", 42), 42.0); // unscaled
                                                            // Forward power: raw 1280 → 10 dBm → 10 mW.
        assert!((dbm_to_watts(convert_meter_raw("dBm", 1280)) - 0.01).abs() < 1e-4);
    }
}
