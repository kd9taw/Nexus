//! FlexRadio VITA-49 UDP stream decoder — the packet envelope every stream shares, DAX audio and
//! meters. The panadapter's FFT frames and waterfall tiles are decoded from the payload
//! [`parse_vita`] returns, in [`crate::flex::vita`]; DAX TX packets are built in
//! [`crate::flex::streams`].
//!
//! Each datagram is a VITA-49 packet: a 32-bit header word, an optional stream id, an optional
//! class id (Flex OUI `0x1C2D`, packet class `0x8003` = FFT), optional timestamps, then the payload.
//!
//! All parsing is PURE + unit-tested against synthetic packets.
//!
//! HONESTY NOTE: written to the published VITA-49 layout + the open-source FlexLib, unit-tested
//! synthetically, NOT yet confirmed on live hardware — the orchestration flags this until an
//! operator verifies it. (The FFT bin SENSE this note once left open is settled where the FFT is
//! now decoded: a bin is a pixel row counted from the top, not a magnitude.)

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
    /// wire. It was written on the TX side and never read on the RX side (audit #1008/#1052),
    /// so a dropped DAX audio datagram was spliced over silently and
    /// every later decode in the window sat at the wrong dt. See [`VitaSequence`].
    pub packet_count: u8,
    pub payload: &'a [u8],
}

fn be_u32(b: &[u8], off: usize) -> Option<u32> {
    b.get(off..off + 4)
        .map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
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
        // With no timestamps the header is 16 bytes: the payload starts right after the class id.
        assert_eq!(v.payload, &fft_payload(0, 2, 2, 1, &[10, 20])[..]);
    }

    #[test]
    fn short_datagram_is_none() {
        assert!(parse_vita(&[0u8; 2]).is_none());
    }

    /// THE PACKET COUNT IS THE ONLY LOSS SIGNAL ON THIS WIRE, and the RX side never read it
    /// (audit #1008/#1052). Round-tripped through our own TX builder as the positive control that
    /// the field is where we think it is.
    #[test]
    fn the_vita_packet_count_survives_a_round_trip() {
        for count in 0..16u8 {
            let dg = crate::flex::streams::dax_tx_packet(0x0600_0000, count, &[0.25, -0.25])
                .expect("one stereo frame");
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
