//! VITA-49 packets as the radio sends them, and the synthetic streams the simulator generates.
//!
//! Every packet here has the 28-byte header the decoders read on 4.x firmware (A: AetherSDR reads
//! all of its classes at a fixed 28-byte offset): the header word, the stream id, the class id
//! (the Flex OUI `0x1C2D`, then the information class `0x534C` above the packet class), and three
//! timestamp words, zero here. Header word: packet type, class-present bit, no trailer, TSI 3, TSF
//! 1, the 4-bit packet count, and the size in 32-bit words. Panadapter, waterfall and meter data
//! go as extension data (type 3); DAX audio as IF data (type 1), the format the port plan records
//! for DAX TX (§4.4). Which timestamp modes the radio itself sets on RX packets is not established
//! here, and no decoder reads them.
//!
//! The packet count is the only loss and ordering signal on this wire. The simulator assigns it in
//! the order packets are generated, and its VITA fault then drops or swaps packets in transit, so
//! a receiver sees exactly what a lossy network would show it.

use std::time::Duration;

/// Flex's OUI in the class id.
pub const FLEX_OUI: u32 = 0x0000_1C2D;
/// The information class above every packet class (`"SL"`).
pub const INFO_CLASS: u16 = 0x534C;
/// The header length every packet here carries.
pub const HEADER_BYTES: usize = 28;

/// Packet classes (A, `PanadapterStream.h`; port plan §4.4).
pub mod class {
    /// Meter values: `(u16 id, i16 raw)` pairs.
    pub const METER: u16 = 0x8002;
    /// Panadapter FFT bins.
    pub const FFT: u16 = 0x8003;
    /// Waterfall tiles.
    pub const WATERFALL: u16 = 0x8004;
    /// DAX audio, float32 stereo, big-endian.
    pub const AUDIO_F32_STEREO: u16 = 0x03E3;
    /// DAX audio, int16 mono, big-endian (reduced bandwidth).
    pub const AUDIO_I16_MONO: u16 = 0x0123;
}

/// VITA-49 packet type 1: IF data with a stream id.
pub const TYPE_IF_DATA: u8 = 1;
/// VITA-49 packet type 3: extension data with a stream id.
pub const TYPE_EXT_DATA: u8 = 3;

/// The meter stream's id. Not established: decoders route meters by packet class (A), so any id
/// serves; this one is only a recognisable placeholder.
pub const METER_STREAM_ID: u32 = 0x0000_0700;

/// DAX audio sample rate (port plan §4.4).
pub const DAX_RATE: u32 = 24_000;
/// Stereo frames per DAX packet: the size the plan records for DAX TX, used for RX here too.
pub const DAX_FRAMES: usize = 128;

/// Build one packet. The payload is padded with zeros to a whole number of 32-bit words.
pub fn packet(packet_type: u8, stream_id: u32, class: u16, count: u8, payload: &[u8]) -> Vec<u8> {
    let padded = payload.len().div_ceil(4) * 4;
    let words = (HEADER_BYTES + padded) / 4;
    let w0: u32 = (u32::from(packet_type & 0xF) << 28)
        | (1 << 27) // class id present
        | (0x3 << 22) // TSI: other
        | (0x1 << 20) // TSF: sample count
        | (u32::from(count & 0xF) << 16)
        | (words as u32 & 0xFFFF);
    let mut out = Vec::with_capacity(words * 4);
    out.extend_from_slice(&w0.to_be_bytes());
    out.extend_from_slice(&stream_id.to_be_bytes());
    out.extend_from_slice(&FLEX_OUI.to_be_bytes());
    out.extend_from_slice(&((u32::from(INFO_CLASS) << 16) | u32::from(class)).to_be_bytes());
    out.extend_from_slice(&[0u8; 12]);
    out.extend_from_slice(payload);
    out.resize(words * 4, 0);
    out
}

/// The header fields of a packet, read back. For tests, and for the recorder's class filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub packet_type: u8,
    pub stream_id: u32,
    pub oui: u32,
    pub info_class: u16,
    pub class: u16,
    pub count: u8,
    /// The size field, in 32-bit words.
    pub words: u16,
}

/// Read the header of a packet built with a class id and a stream id, or `None` when the bytes
/// are too short or carry no class id.
pub fn header(bytes: &[u8]) -> Option<Header> {
    let word = |i: usize| -> Option<u32> {
        let b = bytes.get(i * 4..i * 4 + 4)?;
        Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    };
    let w0 = word(0)?;
    if (w0 >> 27) & 1 == 0 {
        return None;
    }
    let class_word = word(3)?;
    Some(Header {
        packet_type: (w0 >> 28) as u8,
        stream_id: word(1)?,
        oui: word(2)? & 0x00FF_FFFF,
        info_class: (class_word >> 16) as u16,
        class: class_word as u16,
        count: ((w0 >> 16) & 0xF) as u8,
        words: w0 as u16,
    })
}

/// A meter payload: `(id, raw)` pairs (D Metering-Protocol: the id in the upper 16 bits, the value
/// in the lower).
pub fn meter_payload(values: &[(u16, i16)]) -> Vec<u8> {
    let mut p = Vec::with_capacity(values.len() * 4);
    for &(id, raw) in values {
        p.extend_from_slice(&id.to_be_bytes());
        p.extend_from_slice(&raw.to_be_bytes());
    }
    p
}

/// An FFT fragment payload: the 12-byte sub-header (`start_bin`, `num_bins`, `bin_size` = 2,
/// `total_bins`, then `frame_index`) and the bins (A; port plan §4.7). Each bin is a pixel row
/// counted from the top of the display, not a magnitude.
pub fn fft_payload(start_bin: u16, total_bins: u16, frame_index: u32, bins: &[u16]) -> Vec<u8> {
    let mut p = Vec::with_capacity(12 + bins.len() * 2);
    p.extend_from_slice(&start_bin.to_be_bytes());
    p.extend_from_slice(&(bins.len() as u16).to_be_bytes());
    p.extend_from_slice(&2u16.to_be_bytes());
    p.extend_from_slice(&total_bins.to_be_bytes());
    p.extend_from_slice(&frame_index.to_be_bytes());
    for b in bins {
        p.extend_from_slice(&b.to_be_bytes());
    }
    p
}

/// A one-row waterfall tile payload: the 36-byte sub-header (frame low frequency and bin
/// bandwidth as `i64` Hz × 2²⁰, line duration, width, height, timecode, auto-black level, total
/// bins, first bin) and the row as `i16` (an intensity ×128, not dBm) (A; port plan §4.7).
pub fn waterfall_payload(
    low_hz: f64,
    bin_hz: f64,
    line_duration: u32,
    timecode: u32,
    auto_black: u32,
    row: &[i16],
) -> Vec<u8> {
    let fixed = |hz: f64| (hz * 1_048_576.0).round() as i64;
    let mut p = Vec::with_capacity(36 + row.len() * 2);
    p.extend_from_slice(&fixed(low_hz).to_be_bytes());
    p.extend_from_slice(&fixed(bin_hz).to_be_bytes());
    p.extend_from_slice(&line_duration.to_be_bytes());
    p.extend_from_slice(&(row.len() as u16).to_be_bytes()); // width
    p.extend_from_slice(&1u16.to_be_bytes()); // height
    p.extend_from_slice(&timecode.to_be_bytes());
    p.extend_from_slice(&auto_black.to_be_bytes());
    p.extend_from_slice(&(row.len() as u16).to_be_bytes()); // total bins in the frame
    p.extend_from_slice(&0u16.to_be_bytes()); // first bin
    for v in row {
        p.extend_from_slice(&v.to_be_bytes());
    }
    p
}

/// A DAX audio payload from mono samples (-1.0..1.0): float32 stereo with L = R for
/// [`class::AUDIO_F32_STEREO`], int16 mono for [`class::AUDIO_I16_MONO`].
pub fn audio_payload(class: u16, samples: &[f32]) -> Vec<u8> {
    let mut p = Vec::new();
    for &s in samples {
        if class == class::AUDIO_I16_MONO {
            p.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_be_bytes());
        } else {
            p.extend_from_slice(&s.to_be_bytes());
            p.extend_from_slice(&s.to_be_bytes());
        }
    }
    p
}

/// A synthetic stream the simulator sends to a client's registered UDP address.
#[derive(Debug, Clone, PartialEq)]
pub struct Stream {
    pub stream_id: u32,
    /// What each tick sends.
    pub content: Content,
    /// Time between ticks.
    pub period: Duration,
    /// How many ticks to send; `None` runs until the connection ends or [`Stream::until`] fires.
    pub ticks: Option<usize>,
    pub start: Start,
    /// Stop when a command starting with this text is answered (`stream remove 0x04000001`).
    pub until: Option<String>,
}

/// When a stream starts. Either way it waits for the client's UDP address to be known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Start {
    /// As soon as the client's UDP address is known.
    Registered,
    /// Once a command starting with this text has been answered.
    After(String),
}

/// What a stream carries.
#[derive(Debug, Clone, PartialEq)]
pub enum Content {
    /// One meter packet a tick, with these `(id, raw)` values.
    Meters(Vec<(u16, i16)>),
    /// One FFT frame a tick, cut into fragments of at most `per_packet` bins. Every bin sits at
    /// `floor_row` except `peak`, a `(bin, row)` pair. Rows count from the top: 0 is the pan's
    /// `max_dbm`, `y_pixels - 1` its `min_dbm`.
    Fft {
        total_bins: u16,
        per_packet: u16,
        floor_row: u16,
        peak: Option<(u16, u16)>,
    },
    /// One one-row tile a tick: `width` bins from `low_hz`, `bin_hz` apart, all at `level`.
    Waterfall {
        width: u16,
        low_hz: f64,
        bin_hz: f64,
        level: i16,
    },
    /// One packet of [`DAX_FRAMES`] frames a tick: a tone at `tone_hz` and `amplitude`, phase
    /// continuous across packets, at [`DAX_RATE`].
    DaxAudio {
        class: u16,
        tone_hz: f32,
        amplitude: f32,
    },
}

impl Stream {
    /// The DAX packet period: [`DAX_FRAMES`] frames at [`DAX_RATE`].
    pub fn dax_period() -> Duration {
        Duration::from_nanos(DAX_FRAMES as u64 * 1_000_000_000 / u64::from(DAX_RATE))
    }

    /// The packets for tick `k`, in order, as `(packet type, class, payload)`; the stream thread
    /// adds the header and the count.
    pub(crate) fn tick(&self, k: usize) -> Vec<(u8, u16, Vec<u8>)> {
        match &self.content {
            Content::Meters(values) => vec![(TYPE_EXT_DATA, class::METER, meter_payload(values))],
            Content::Fft {
                total_bins,
                per_packet,
                floor_row,
                peak,
            } => {
                let per = (*per_packet).max(1);
                let mut out = Vec::new();
                let mut start = 0u16;
                while start < *total_bins {
                    let n = per.min(total_bins - start);
                    let bins: Vec<u16> = (start..start + n)
                        .map(|b| match peak {
                            Some((pb, row)) if *pb == b => *row,
                            _ => *floor_row,
                        })
                        .collect();
                    out.push((
                        TYPE_EXT_DATA,
                        class::FFT,
                        fft_payload(start, *total_bins, k as u32, &bins),
                    ));
                    start += n;
                }
                out
            }
            Content::Waterfall {
                width,
                low_hz,
                bin_hz,
                level,
            } => {
                let row = vec![*level; usize::from(*width)];
                let payload = waterfall_payload(*low_hz, *bin_hz, 80, k as u32, 0, &row);
                vec![(TYPE_EXT_DATA, class::WATERFALL, payload)]
            }
            Content::DaxAudio {
                class,
                tone_hz,
                amplitude,
            } => {
                let first = k * DAX_FRAMES;
                let step = std::f64::consts::TAU * f64::from(*tone_hz) / f64::from(DAX_RATE);
                let samples: Vec<f32> = (first..first + DAX_FRAMES)
                    .map(|n| amplitude * (step * n as f64).sin() as f32)
                    .collect();
                vec![(TYPE_IF_DATA, *class, audio_payload(*class, &samples))]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempo_net::flex::vita;
    use tempo_net::flexvita;

    fn stream(content: Content) -> Stream {
        Stream {
            stream_id: 0x4000_0000,
            content,
            period: Duration::from_millis(10),
            ticks: None,
            start: Start::Registered,
            until: None,
        }
    }

    #[test]
    fn the_header_reads_back_and_the_shipped_decoder_agrees() {
        let p = packet(TYPE_EXT_DATA, 0x4000_0000, class::FFT, 0x1B, &[1, 2, 3]);
        assert_eq!(p.len(), 32, "padded to a whole word");
        let h = header(&p).unwrap();
        assert_eq!(
            h,
            Header {
                packet_type: 3,
                stream_id: 0x4000_0000,
                oui: FLEX_OUI,
                info_class: INFO_CLASS,
                class: class::FFT,
                count: 0xB,
                words: 8,
            }
        );
        // An independent reader: Nexus's shipped VITA parser.
        let v = flexvita::parse_vita(&p).unwrap();
        assert_eq!(v.packet_type, 3);
        assert_eq!(v.stream_id, Some(0x4000_0000));
        assert_eq!(v.class_oui, Some(flexvita::FLEX_OUI));
        assert_eq!(v.packet_class, Some(flexvita::FFT_PACKET_CLASS));
        assert_eq!(v.packet_count, 0xB);
        assert!(!v.has_trailer);
        assert_eq!(v.payload, &[1, 2, 3, 0], "the payload starts at byte 28");
    }

    #[test]
    fn meter_packets_decode_with_the_shipped_reader() {
        let s = stream(Content::Meters(vec![(1, -9000), (3, 128)]));
        let (t, c, payload) = s.tick(0).remove(0);
        let p = packet(t, METER_STREAM_ID, c, 0, &payload);
        let v = flexvita::parse_vita(&p).unwrap();
        assert_eq!(v.packet_class, Some(flexvita::METER_PACKET_CLASS));
        assert_eq!(
            flexvita::parse_meter_values(v.payload, v.has_trailer),
            vec![(1, -9000), (3, 128)]
        );
    }

    #[test]
    fn an_fft_frame_spans_fragments_and_reassembles() {
        let s = stream(Content::Fft {
            total_bins: 10,
            per_packet: 4,
            floor_row: 400,
            peak: Some((5, 20)),
        });
        let frags = s.tick(7);
        assert_eq!(frags.len(), 3, "10 bins in fragments of 4, 4 and 2");
        let mut asm = vita::FftAssembler::new();
        let mut row = None;
        for (i, (t, c, payload)) in frags.iter().enumerate() {
            let p = packet(*t, 0x4000_0000, *c, i as u8, payload);
            let v = flexvita::parse_vita(&p).unwrap();
            let f = vita::decode_fft(v.payload, v.has_trailer).unwrap();
            assert_eq!(f.frame_index, 7);
            row = asm.push(&f).map(|frame| frame.rows);
        }
        let mut expect = vec![400u16; 10];
        expect[5] = 20;
        assert_eq!(row, Some(expect));
        // A real radio's bin size is 2 bytes (A rejects any other, and so does the decoder).
        let (_, _, payload) = &frags[0];
        assert_eq!(&payload[4..6], &[0, 2]);
    }

    #[test]
    fn a_waterfall_tile_carries_fixed_point_frequencies() {
        let s = stream(Content::Waterfall {
            width: 4,
            low_hz: 14_000_000.0,
            bin_hz: 195.3125,
            level: 100 * 128,
        });
        let (t, c, payload) = s.tick(3).remove(0);
        assert_eq!((t, c), (TYPE_EXT_DATA, class::WATERFALL));
        let i64_at = |o: usize| i64::from_be_bytes(payload[o..o + 8].try_into().unwrap());
        assert_eq!(i64_at(0), 14_000_000 << 20);
        assert_eq!(i64_at(8) as f64 / 1_048_576.0, 195.3125);
        let u16_at = |o: usize| u16::from_be_bytes([payload[o], payload[o + 1]]);
        let u32_at = |o: usize| u32::from_be_bytes(payload[o..o + 4].try_into().unwrap());
        assert_eq!((u16_at(20), u16_at(22)), (4, 1), "width, height");
        assert_eq!(u32_at(24), 3, "the timecode is the tick");
        assert_eq!((u16_at(32), u16_at(34)), (4, 0), "total bins, first bin");
        assert_eq!(i16::from_be_bytes([payload[36], payload[37]]), 100 * 128);
        assert_eq!(payload.len(), 36 + 8);
    }

    #[test]
    fn dax_audio_decodes_as_the_tone_in_both_formats() {
        for class in [class::AUDIO_F32_STEREO, class::AUDIO_I16_MONO] {
            let s = stream(Content::DaxAudio {
                class,
                tone_hz: 1000.0,
                amplitude: 0.5,
            });
            let mut decoded = Vec::new();
            for k in 0..2 {
                let (t, c, payload) = s.tick(k).remove(0);
                assert_eq!(t, TYPE_IF_DATA);
                let p = packet(t, 0x0400_0001, c, k as u8, &payload);
                let v = flexvita::parse_vita(&p).unwrap();
                decoded.extend(flexvita::parse_dax_audio(class, v.payload, v.has_trailer).unwrap());
            }
            assert_eq!(decoded.len(), 2 * DAX_FRAMES);
            for (n, got) in decoded.iter().enumerate() {
                let want = 0.5 * (std::f64::consts::TAU * 1000.0 * n as f64 / 24_000.0).sin();
                assert!(
                    (f64::from(*got) - want).abs() < 1e-3,
                    "sample {n}: {got} vs {want}"
                );
            }
        }
        assert_eq!(Stream::dax_period(), Duration::from_nanos(5_333_333));
    }
}
