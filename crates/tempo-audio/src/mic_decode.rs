//! The page's microphone, decoded: one Opus packet from the stream's audio track in, 12 kHz mono
//! out, which is the rate the transmit route takes (`AudioBackend::play`).
//!
//! **This module knows nothing about keying.** It turns packets into samples and nothing else; what
//! may reach the transmitter, and when, is `tempo_app::mic`'s to decide.
//!
//! ## Why the decoder runs at 12 kHz
//!
//! The page's track is Opus at 48 kHz, as WebRTC always signals it. libopus decodes any stream at
//! any of its five rates, and asking it for 12 kHz is the whole resampling step: the decoder drops
//! every band above 6 kHz before its inverse transform, then decimates, so nothing above the new
//! Nyquist can alias down. That is "resample with what the tree already has": the codec the tree
//! already ships, and no second resampler between it and the route. 6 kHz is also above any SSB
//! transmit passband.
//!
//! ## What a lost packet becomes
//!
//! Nothing. A packet that never arrived is not concealed here: the microphone's playout fills its
//! moment with silence. Concealment can burst loud into a gap (the audio design's §7 measured a
//! full-scale burst at 10% loss), and on the transmit path a burst goes out on the air.
use crate::receive_encode::FRAME_MS;

/// The transmit route's rate.
pub const MIC_RATE_HZ: u32 = 12_000;
/// The longest Opus packet (120 ms) at [`MIC_RATE_HZ`].
const MAX_SAMPLES: usize = (MIC_RATE_HZ as usize) * 120 / 1000;

/// One stream's microphone decoder. One per session: a decoder carries state from packet to packet
/// and must never be shared across two streams.
pub struct MicDecoder {
    decoder: opus::Decoder,
    out: Vec<f32>,
}

impl MicDecoder {
    pub fn new() -> Result<Self, opus::Error> {
        Ok(Self {
            decoder: opus::Decoder::new(MIC_RATE_HZ, opus::Channels::Mono)?,
            out: vec![0.0; MAX_SAMPLES],
        })
    }

    /// Decode one packet to mono samples at [`MIC_RATE_HZ`]. `None` for a packet libopus refuses
    /// (malformed, or empty): the moment it stood for is then silence, like a lost one.
    pub fn decode(&mut self, packet: &[u8]) -> Option<Vec<f32>> {
        if packet.is_empty() {
            return None; // an empty packet is libopus's "conceal this": see the module header
        }
        let n = self
            .decoder
            .decode_float(packet, &mut self.out, false)
            .ok()?;
        Some(self.out[..n].to_vec())
    }
}

/// A packet's place on the page's media clock (the RTP timestamp, at the track's clock rate), as a
/// position in samples at [`MIC_RATE_HZ`].
pub fn media_position(rtp: u64, clock_hz: u32) -> u64 {
    if clock_hz == 0 {
        return 0;
    }
    (rtp as u128 * MIC_RATE_HZ as u128 / clock_hz as u128) as u64
}

/// Samples in one 20 ms frame at [`MIC_RATE_HZ`], the page's frame length (S6).
pub const FRAME_SAMPLES: usize = (MIC_RATE_HZ as usize) * (FRAME_MS as usize) / 1000;

#[cfg(test)]
mod tests {
    use super::*;

    /// 20 ms of a `hz` tone at 48 kHz, encoded as the page's browser would: Opus at 48 kHz, 20 ms.
    fn page_packet(encoder: &mut opus::Encoder, hz: f32, frame: usize) -> Vec<u8> {
        let pcm: Vec<f32> = (0..960)
            .map(|i| {
                let t = (frame * 960 + i) as f32 / 48_000.0;
                0.5 * (2.0 * std::f32::consts::PI * hz * t).sin()
            })
            .collect();
        let mut out = vec![0u8; 4000];
        let n = encoder.encode_float(&pcm, &mut out).unwrap();
        out.truncate(n);
        out
    }

    /// The page's 20 ms frame decodes to 20 ms at the route's rate, and the tone survives it: the
    /// decoded samples carry a 1 kHz tone, not silence and not an alias.
    #[test]
    fn a_page_frame_decodes_to_20_ms_at_the_routes_rate_with_its_tone() {
        let mut encoder =
            opus::Encoder::new(48_000, opus::Channels::Mono, opus::Application::Voip).unwrap();
        let mut decoder = MicDecoder::new().unwrap();
        let mut decoded = Vec::new();
        for frame in 0..25 {
            let packet = page_packet(&mut encoder, 1_000.0, frame);
            let samples = decoder.decode(&packet).expect("a valid packet was refused");
            assert_eq!(
                samples.len(),
                FRAME_SAMPLES,
                "a 20 ms frame is {FRAME_SAMPLES} samples at 12 kHz"
            );
            decoded.extend(samples);
        }
        // Past the codec's start-up, measure the tone by its zero crossings: 1 kHz at 12 kHz is
        // 2 crossings every 12 samples.
        let tail = &decoded[decoded.len() - 2_400..];
        let crossings = tail
            .windows(2)
            .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
            .count();
        let hz = crossings as f32 / 2.0 / (tail.len() as f32 / MIC_RATE_HZ as f32);
        assert!(
            (950.0..1_050.0).contains(&hz),
            "decoded a {hz:.0} Hz tone from a 1 kHz one"
        );
        let rms = (tail.iter().map(|s| s * s).sum::<f32>() / tail.len() as f32).sqrt();
        assert!(
            rms > 0.2,
            "the tone came out at RMS {rms:.3}: nearly silent"
        );
    }

    /// A tone above the route's Nyquist (7 kHz, 6 kHz being 12 kHz's half) must not come out folded
    /// down into the passband. CONTROL: the same level at 1 kHz comes out loud (the test above).
    #[test]
    fn a_tone_above_six_khz_does_not_alias_into_the_passband() {
        let mut encoder =
            opus::Encoder::new(48_000, opus::Channels::Mono, opus::Application::Audio).unwrap();
        let mut decoder = MicDecoder::new().unwrap();
        let mut decoded = Vec::new();
        for frame in 0..25 {
            decoded.extend(
                decoder
                    .decode(&page_packet(&mut encoder, 7_000.0, frame))
                    .unwrap(),
            );
        }
        let tail = &decoded[decoded.len() - 2_400..];
        let rms = (tail.iter().map(|s| s * s).sum::<f32>() / tail.len() as f32).sqrt();
        assert!(
            rms < 0.02,
            "a 7 kHz tone reached the 12 kHz route at RMS {rms:.3}: aliasing"
        );
        // CONTROL: the measure sees an alias when there is one. The same packets decoded at 48 kHz
        // and cut to 12 kHz by keeping every fourth sample (no filter) fold the 7 kHz tone down to
        // 5 kHz at full strength.
        let mut encoder =
            opus::Encoder::new(48_000, opus::Channels::Mono, opus::Application::Audio).unwrap();
        let mut wide = opus::Decoder::new(48_000, opus::Channels::Mono).unwrap();
        let mut naive: Vec<f32> = Vec::new();
        let mut out = vec![0.0f32; 5_760];
        for frame in 0..25 {
            let n = wide
                .decode_float(&page_packet(&mut encoder, 7_000.0, frame), &mut out, false)
                .unwrap();
            naive.extend(out[..n].iter().step_by(4));
        }
        let tail = &naive[naive.len() - 2_400..];
        let aliased = (tail.iter().map(|s| s * s).sum::<f32>() / tail.len() as f32).sqrt();
        assert!(
            aliased > 0.2,
            "control: a naive decimation measured only RMS {aliased:.3}, so the check is blind"
        );
    }

    /// The browser's own silence frame (CELT fullband, 20 ms) decodes to 20 ms of silence: what the
    /// page sends between words, and what the station's tests stand for a quiet page with.
    #[test]
    fn a_browsers_silence_frame_is_20_ms_of_silence() {
        let mut decoder = MicDecoder::new().unwrap();
        let samples = decoder
            .decode(&[0xF8, 0xFF, 0xFE])
            .expect("the silence frame was refused");
        assert_eq!(samples.len(), FRAME_SAMPLES);
        assert!(
            samples.iter().all(|s| s.abs() < 1e-3),
            "silence decoded as sound"
        );
    }

    #[test]
    fn a_packet_libopus_refuses_is_none() {
        let mut decoder = MicDecoder::new().unwrap();
        assert_eq!(decoder.decode(&[]), None);
        assert_eq!(
            decoder.decode(&[0xff; 3]),
            None,
            "a malformed packet was decoded"
        );
    }

    #[test]
    fn the_media_clock_maps_to_the_routes_samples() {
        // WebRTC's Opus clock is 48 kHz: 960 ticks (20 ms) is 240 samples at 12 kHz.
        assert_eq!(media_position(960, 48_000), 240);
        assert_eq!(media_position(48_000 * 3600, 48_000), 12_000 * 3600);
        assert_eq!(media_position(5, 0), 0);
    }
}
