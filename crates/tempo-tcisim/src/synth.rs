//! What the simulated receiver hears, as receive audio and as IQ.
//!
//! - **Carriers** on the band, by RF frequency. In the receive audio a carrier is heard when its
//!   offset from the dial (VFO A, plus the RIT offset when RIT is on) lies inside the receive
//!   filter, whose edges are signed as the spec writes them (`rx_filter_band:0,-2900,-70` is a
//!   lower-sideband filter), and is heard at the offset's size in Hz. In the IQ it is at its
//!   offset from the panorama's centre (`dds`) when that is inside the IQ rate's band.
//! - **Tones**: audio heard whatever the dial, for tests that only need a known tone.
//! - **A clip**: audio a test supplies (a real FT8 over from Nexus's own encoder, say), played
//!   once into each receive stream from the moment it starts.
//! - **Loopback**: the transmit audio clients send, heard in every receive stream about one
//!   frame later, as a receiver listening to its own transmitter would (as on the loop bench).
//!
//! In DIGU and DIGL with two channels the spec makes the receive audio complex (§3.4): channel 1
//! then carries the quadrature of the carriers and tones. A clip and the loopback are real signals
//! and go on channel 0 only. In every other case with two channels, channel 1 repeats channel 0.
//! Channel 0 is the audio in both cases.

use std::collections::VecDeque;
use std::f64::consts::TAU;
use std::sync::Arc;
use std::time::Instant;

/// A carrier on the band.
#[derive(Debug, Clone, PartialEq)]
pub struct Carrier {
    /// RF frequency, Hz.
    pub hz: u64,
    /// Level, dB relative to full scale.
    pub dbfs: f64,
}

/// An audio tone heard whatever the dial.
#[derive(Debug, Clone, PartialEq)]
pub struct Tone {
    /// Audio frequency, Hz.
    pub hz: f64,
    pub dbfs: f64,
}

/// Audio a test supplies, at its own rate (resampled to the stream's by nearest sample).
#[derive(Debug, Clone, PartialEq)]
pub struct Clip {
    pub rate: u32,
    pub samples: Arc<Vec<f32>>,
}

/// What the receiver hears. See the module header.
#[derive(Debug, Clone, PartialEq)]
pub struct Receive {
    pub carriers: Vec<Carrier>,
    pub tones: Vec<Tone>,
    pub clip: Option<Clip>,
    pub loopback: bool,
}

impl Default for Receive {
    /// One carrier 1 kHz above the bundled profiles' 14.074 MHz dial, at −30 dBFS.
    fn default() -> Self {
        Receive {
            carriers: vec![Carrier {
                hz: 14_075_000,
                dbfs: -30.0,
            }],
            tones: Vec::new(),
            clip: None,
            loopback: false,
        }
    }
}

/// What one transceiver is tuned to, for the receive audio.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Tuning {
    /// VFO A plus the RIT offset when RIT is on.
    pub dial: i64,
    pub filter: (i64, i64),
    /// DIGU or DIGL: two channels carry a complex signal.
    pub digital: bool,
}

fn amplitude(dbfs: f64) -> f64 {
    10f64.powf(dbfs / 20.0)
}

/// The audio tones `receive` makes at `tuning`: (Hz, amplitude).
fn audible(receive: &Receive, tuning: Tuning) -> Vec<(f64, f64)> {
    let mut out: Vec<(f64, f64)> = receive
        .tones
        .iter()
        .map(|t| (t.hz, amplitude(t.dbfs)))
        .collect();
    for c in &receive.carriers {
        let offset = i64::try_from(c.hz).unwrap_or(i64::MAX) - tuning.dial;
        if (tuning.filter.0..=tuning.filter.1).contains(&offset) && offset != 0 {
            out.push((offset.unsigned_abs() as f64, amplitude(c.dbfs)));
        }
    }
    out
}

/// One receive audio frame: `frames` frames of `channels` interleaved values at `rate`, the
/// first being the stream's sample number `first`. `stream_t0` is when the stream's sample 0 is,
/// in seconds on the loopback's clock.
#[allow(clippy::too_many_arguments)]
pub(crate) fn rx_audio(
    receive: &Receive,
    tuning: Tuning,
    loopback: &Timeline,
    rate: u32,
    channels: u32,
    first: u64,
    frames: usize,
    stream_t0: f64,
) -> Vec<f32> {
    let tones = audible(receive, tuning);
    let rate_f = f64::from(rate.max(1));
    let complex = tuning.digital && channels == 2;
    let mut out = Vec::with_capacity(frames * channels as usize);
    for i in 0..frames {
        let n = first + i as u64;
        let t = n as f64 / rate_f;
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for &(hz, a) in &tones {
            let phase = TAU * hz * t;
            re += a * phase.cos();
            im += a * phase.sin();
        }
        let mut real_only = 0.0f64;
        if let Some(clip) = &receive.clip {
            let at = (n as f64 * f64::from(clip.rate) / rate_f) as usize;
            real_only += f64::from(clip.samples.get(at).copied().unwrap_or(0.0));
        }
        if receive.loopback {
            real_only += f64::from(loopback.at_seconds(stream_t0 + t));
        }
        let ch0 = (re + real_only) as f32;
        match channels {
            1 => out.push(ch0),
            _ => {
                out.push(ch0);
                out.push(if complex { im as f32 } else { ch0 });
                out.extend(std::iter::repeat_n(
                    0.0,
                    channels.saturating_sub(2) as usize,
                ));
            }
        }
    }
    out
}

/// One IQ frame: `pairs` complex samples (I then Q) at `rate`, the first being the stream's
/// sample number `first`, for a panorama centred on `dds`.
pub(crate) fn iq(receive: &Receive, dds: u64, rate: u32, first: u64, pairs: usize) -> Vec<f32> {
    let half = f64::from(rate) / 2.0;
    let lines: Vec<(f64, f64)> = receive
        .carriers
        .iter()
        .map(|c| (c.hz as f64 - dds as f64, amplitude(c.dbfs)))
        .filter(|(offset, _)| offset.abs() < half)
        .collect();
    let rate_f = f64::from(rate.max(1));
    let mut out = Vec::with_capacity(pairs * 2);
    for i in 0..pairs {
        let t = (first + i as u64) as f64 / rate_f;
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for &(offset, a) in &lines {
            let phase = TAU * offset * t;
            re += a * phase.cos();
            im += a * phase.sin();
        }
        out.push(re as f32);
        out.push(im as f32);
    }
    out
}

/// Transmit audio laid on a clock, for the loopback: each frame a client sends is placed at the
/// time it arrived, or straight after the one before when that is later.
#[derive(Debug)]
pub(crate) struct Timeline {
    start: Instant,
    rate: u32,
    /// The absolute sample number of `samples[0]`.
    base: u64,
    samples: VecDeque<f32>,
    /// How many samples to keep.
    keep: usize,
}

impl Timeline {
    pub(crate) fn new(start: Instant, rate: u32, keep_seconds: u32) -> Timeline {
        Timeline {
            start,
            rate,
            base: 0,
            samples: VecDeque::new(),
            keep: (rate as usize).saturating_mul(keep_seconds as usize),
        }
    }

    /// Lay `data` (at the timeline's rate) at `at`, or after what is already laid.
    pub(crate) fn write(&mut self, at: Instant, data: &[f32]) {
        let arrival =
            (at.saturating_duration_since(self.start).as_secs_f64() * f64::from(self.rate)) as u64;
        let end = self.base + self.samples.len() as u64;
        if arrival > end {
            let gap = arrival - end;
            if gap as usize > self.keep {
                // Far past everything held: start over there.
                self.samples.clear();
                self.base = arrival;
            } else {
                self.samples.extend(std::iter::repeat_n(0.0, gap as usize));
            }
        }
        self.samples.extend(data.iter().copied());
        while self.samples.len() > self.keep {
            self.samples.pop_front();
            self.base += 1;
        }
    }

    /// The sample at `seconds` on the timeline's clock; silence where nothing was laid.
    pub(crate) fn at_seconds(&self, seconds: f64) -> f32 {
        if seconds < 0.0 {
            return 0.0;
        }
        let n = (seconds * f64::from(self.rate)) as u64;
        n.checked_sub(self.base)
            .and_then(|i| self.samples.get(i as usize))
            .copied()
            .unwrap_or(0.0)
    }
}

/// The level of `hz` in `samples` (at `rate`), as an amplitude: a Goertzel filter, for tests
/// that check where a tone landed.
pub fn tone_level(samples: &[f32], rate: u32, hz: f64) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let w = TAU * hz / f64::from(rate);
    let coeff = 2.0 * w.cos();
    let (mut s1, mut s2) = (0.0f64, 0.0f64);
    for &x in samples {
        let s = f64::from(x) + coeff * s1 - s2;
        s2 = s1;
        s1 = s;
    }
    let power = s1 * s1 + s2 * s2 - coeff * s1 * s2;
    2.0 * power.max(0.0).sqrt() / samples.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn usb(dial: i64) -> Tuning {
        Tuning {
            dial,
            filter: (200, 2_900),
            digital: false,
        }
    }

    fn channel(samples: &[f32], channels: usize, ch: usize) -> Vec<f32> {
        samples.iter().skip(ch).step_by(channels).copied().collect()
    }

    #[test]
    fn a_carrier_is_heard_at_its_offset_inside_the_filter_and_not_outside() {
        let receive = Receive::default(); // 14.075 MHz
        let tl = Timeline::new(Instant::now(), 48_000, 1);
        let audio = rx_audio(&receive, usb(14_074_000), &tl, 48_000, 1, 0, 4800, 0.0);
        let at_1k = tone_level(&audio, 48_000, 1_000.0);
        assert!((at_1k - amplitude(-30.0)).abs() < 0.002, "{at_1k}");
        assert!(tone_level(&audio, 48_000, 1_500.0) < 0.001);
        // Lower sideband with the filter mirrored: the carrier above the dial is not heard.
        let lsb = Tuning {
            filter: (-2_900, -200),
            ..usb(14_074_000)
        };
        let audio = rx_audio(&receive, lsb, &tl, 48_000, 1, 0, 4800, 0.0);
        assert!(tone_level(&audio, 48_000, 1_000.0) < 0.001);
        // Tuned 2 kHz above in LSB it is 2 kHz below the dial: heard at 2 kHz.
        let lsb_above = Tuning {
            dial: 14_077_000,
            ..lsb
        };
        let audio = rx_audio(&receive, lsb_above, &tl, 48_000, 1, 0, 4800, 0.0);
        assert!(tone_level(&audio, 48_000, 2_000.0) > 0.02);
    }

    #[test]
    fn frames_are_phase_continuous_and_two_channels_are_stereo_or_complex() {
        let receive = Receive {
            carriers: Vec::new(),
            tones: vec![Tone {
                hz: 1_500.0,
                dbfs: -6.0,
            }],
            ..Receive::default()
        };
        let tl = Timeline::new(Instant::now(), 48_000, 1);
        let whole = rx_audio(&receive, usb(0), &tl, 48_000, 2, 0, 2048, 0.0);
        let mut parts = rx_audio(&receive, usb(0), &tl, 48_000, 2, 0, 1024, 0.0);
        parts.extend(rx_audio(&receive, usb(0), &tl, 48_000, 2, 1024, 1024, 0.0));
        assert_eq!(whole, parts);
        assert_eq!(
            channel(&whole, 2, 0),
            channel(&whole, 2, 1),
            "stereo repeats channel 0"
        );
        let digital = Tuning {
            digital: true,
            ..usb(0)
        };
        let complex = rx_audio(&receive, digital, &tl, 48_000, 2, 0, 2048, 0.0);
        let (i, q) = (channel(&complex, 2, 0), channel(&complex, 2, 1));
        assert_eq!(
            i,
            channel(&whole, 2, 0),
            "channel 0 is the audio in both cases"
        );
        // The quadrature: I² + Q² is the tone's amplitude squared throughout.
        let a = amplitude(-6.0);
        for (x, y) in i.iter().zip(&q) {
            assert!((f64::from(x * x + y * y) - a * a).abs() < 1e-4);
        }
    }

    #[test]
    fn an_iq_carrier_sits_at_its_offset_from_the_centre_with_its_sign() {
        let receive = Receive::default(); // 14.075 MHz
        let below = iq(&receive, 14_080_000, 48_000, 0, 4800); // -5 kHz
        let (i, q) = (channel(&below, 2, 0), channel(&below, 2, 1));
        // A negative frequency: Q leads I by a quarter turn the other way. Check the sign by
        // correlating with a positive-frequency rotor.
        let w = TAU * 5_000.0 / 48_000.0;
        let positive: f64 = i
            .iter()
            .zip(&q)
            .enumerate()
            .map(|(n, (x, y))| {
                f64::from(*x) * (w * n as f64).cos() + f64::from(*y) * (w * n as f64).sin()
            })
            .sum();
        let negative: f64 = i
            .iter()
            .zip(&q)
            .enumerate()
            .map(|(n, (x, y))| {
                f64::from(*x) * (w * n as f64).cos() - f64::from(*y) * (w * n as f64).sin()
            })
            .sum();
        assert!(
            negative.abs() > 10.0 * positive.abs(),
            "{negative} vs {positive}"
        );
        // Outside the band: nothing.
        let far = iq(&receive, 14_200_000, 48_000, 0, 480);
        assert!(far.iter().all(|v| *v == 0.0));
    }

    #[test]
    fn the_timeline_lays_frames_at_arrival_and_back_to_back() {
        let start = Instant::now();
        let mut tl = Timeline::new(start, 1_000, 10);
        tl.write(start + Duration::from_millis(100), &[1.0; 50]);
        tl.write(start + Duration::from_millis(110), &[2.0; 50]); // before the first ends: appended
        assert_eq!(tl.at_seconds(0.099), 0.0);
        assert_eq!(tl.at_seconds(0.1005), 1.0);
        assert_eq!(tl.at_seconds(0.1495), 1.0);
        assert_eq!(tl.at_seconds(0.1505), 2.0);
        assert_eq!(tl.at_seconds(0.2005), 0.0);
        // Far beyond what it keeps: it starts over.
        tl.write(start + Duration::from_secs(100), &[3.0; 10]);
        assert_eq!(tl.at_seconds(100.0005), 3.0);
        assert_eq!(tl.at_seconds(0.1005), 0.0);
    }
}
