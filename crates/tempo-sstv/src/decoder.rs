//! `SstvDecoder` — public state machine driving the decode pipeline.
//!
//! This is the V1 skeleton: state machine shell + public API surface.
//! VIS detection lands in PR-1; per-mode pixel decoding lands in PR-2.
//!
//! Translated in spirit from slowrx's `slowrx.c` `Listen()` loop +
//! `vis.c` `GetVIS()` + `video.c` `GetVideo()`. ISC License — see
//! `NOTICE.md`.

use crate::error::Result;
use crate::image::SstvImage;
use crate::modespec::SstvMode;
use crate::resample::Resampler;
use crate::sync::{find_sync, SyncTracker, SYNC_PROBE_STRIDE};

/// One observable event emitted by [`SstvDecoder::process`].
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum SstvEvent {
    /// VIS header parsed and a known mode dispatched.
    VisDetected {
        /// Mode identified by the VIS bits.
        mode: SstvMode,
        /// Working-rate (11025 Hz) sample offset where the VIS stop bit ended.
        /// Useful for callers that want to align audio captures with decoder events.
        sample_offset: u64,
        /// Radio mistuning offset in Hz: `observed_leader_hz - 1900`. The
        /// decoder applies this offset internally to per-pixel demod so the
        /// downstream pixel band shifts with the radio's tuning. Surfaced
        /// here purely for caller diagnostics; consumers do not need to do
        /// anything with it. Translated from slowrx's `CurrentPic.HedrShift`
        /// (`vis.c` line 106 → `video.c` line 406).
        hedr_shift_hz: f64,
    },
    /// A VIS header parsed and passed parity, but its 7-bit code maps to no
    /// SSTV mode this build can decode (reserved / undefined, or a mode not
    /// yet implemented). The decoder discards the burst and resumes scanning
    /// for the next VIS — equivalent to slowrx's `printf("Unknown VIS %d")`
    /// plus its retry-`GetVIS()` loop.
    UnknownVis {
        /// The 7-bit VIS code that did not resolve.
        code: u8,
        /// Radio mistuning offset in Hz: `observed_leader_hz - 1900` (the
        /// same quantity as [`SstvEvent::VisDetected`]'s `hedr_shift_hz`).
        /// Surfaced for diagnostics; the burst is dropped, so it does not
        /// feed any decode.
        hedr_shift_hz: f64,
        /// Working-rate (11025 Hz) sample offset where the VIS stop bit ended.
        sample_offset: u64,
    },
    /// One scan line completed (callers may render incrementally).
    ///
    /// For PD and Robot 72: `pixels` is fully composed at emission time
    /// (own Y/U/V or Y(odd)/Cr/Cb/Y(even) for the row).
    ///
    /// For Robot 36 / Robot 24: `pixels` reflects the image buffer state
    /// at emission time, which has a transient cross-row dependency due
    /// to chroma alternation. Each radio line writes its own Cr-or-Cb
    /// AND duplicates that chroma to the NEXT image row. So row N is
    /// emitted with: own Y, own Cr-or-Cb (per row parity), and the
    /// OTHER chroma channel duplicated from the previous radio line.
    /// Row 0 is the exception — its `Cb` channel is zero-init at
    /// emission time (no row -1 to duplicate from), giving a transient
    /// color cast on the very top row. Faithful to slowrx C, which
    /// `calloc`'s its image buffer and never writes row 0's Cb.
    /// `ImageComplete` carries the final populated state for all rows
    /// `1..image_lines` and the same row-0 Cb-zero artifact.
    LineDecoded {
        /// Mode currently being decoded.
        mode: SstvMode,
        /// 0-based row index for this line.
        line_index: u32,
        /// Row pixels in `[r, g, b]` order, length = mode's `line_pixels`.
        pixels: Vec<[u8; 3]>,
    },
    /// Image complete (`LineDecoded` for the final line was just emitted).
    /// `partial` is reserved for future mid-image VIS handling — V1 always
    /// emits `partial: false`. `reset()` discards in-flight images silently
    /// without emitting any event.
    ImageComplete {
        /// Final pixel buffer.
        image: SstvImage,
        /// Reserved for future mid-image VIS handling. V1 always sets this
        /// to `false`. See the deferred mid-image VIS TODO in
        /// [`SstvDecoder::process`] for details.
        partial: bool,
    },
}

/// Internal state of the decoder.
enum State {
    AwaitingVis,
    /// Boxed because [`DecodingState`] contains the working FFT plans +
    /// audio buffer and dwarfs the unit `AwaitingVis` variant; clippy
    /// warns about size disparity otherwise.
    Decoding(Box<DecodingState>),
}

/// Two-pass decoding state.
///
/// While `audio.len() < target_audio_samples`, the decoder accumulates
/// audio and probes the 1200 Hz sync band into `has_sync`. When the
/// buffer is full, [`find_sync`] runs once to recover the
/// slant-corrected rate + line-zero `Skip`; per-pair decode then runs in
/// a single fast burst, emitting [`SstvEvent::LineDecoded`] for every
/// row.
struct DecodingState {
    mode: SstvMode,
    spec: crate::modespec::ModeSpec,
    image: SstvImage,
    /// Working-rate audio captured from VIS-stop-bit forward.
    audio: Vec<f32>,
    /// Per-stride boolean track from [`SyncTracker::has_sync_at`]. One
    /// entry per [`SYNC_PROBE_STRIDE`] working-rate samples.
    has_sync: Vec<bool>,
    /// Next sample index in `audio` to probe. Always a multiple of
    /// [`SYNC_PROBE_STRIDE`].
    next_probe_sample: usize,
    /// Sync-band tracker. Constructed when `Decoding` is entered so the
    /// hedr-shift bin offsets match the detected mistuning.
    sync_tracker: SyncTracker,
    /// Radio mistuning offset in Hz extracted at VIS time. Plumbed to
    /// per-pixel demod so the pixel band shifts with radio tuning.
    hedr_shift_hz: f64,
    /// Total audio samples we must accumulate before running
    /// [`find_sync`] and per-pair decode. Computed at state-entry as
    /// `image_lines / 2 × line_seconds × FINDSYNC_AUDIO_HEADROOM × work_rate`.
    target_audio_samples: usize,
    /// Per-mode chroma planes side buffer.
    ///
    /// `Some([cr_plane, cb_plane])` for `SstvMode::Robot24` and
    /// `SstvMode::Robot36`. Each plane is `image_lines * line_pixels`
    /// bytes, populated as radio lines are decoded: each radio line N
    /// writes its own chroma + duplicates to the next row's chroma slot
    /// (slowrx `video.c:421-425`); RGB composition for row N reads the
    /// duplicated-from-N-1 chroma channel that the line N-1 decode
    /// wrote earlier.
    ///
    /// `None` for every other mode. PD composes RGB in-place per pair
    /// (see `mode_pd::decode_pd_line_pair`); Robot 72 and Scottie 1/2/DX
    /// also compose RGB in-place per radio line (see
    /// `mode_robot::decode_r72_line` and `mode_scottie::decode_line`).
    /// `SstvMode` is `#[non_exhaustive]`; any future mode that does
    /// need cross-radio-line chroma state will need to extend the
    /// constructor's match in `process` to opt in.
    chroma_planes: Option<[Vec<u8>; 2]>,
}

/// Headroom factor on the buffered audio length before [`find_sync`]
/// runs. 1.00 = exactly the nominal image length. The Hough transform
/// re-anchors the rate against whatever sync pulses are present, so
/// trailing audio beyond the last line is not strictly required. We
/// keep this knob in case future modes (Scottie pre-line skip) want to
/// pad the buffer to absorb additional offset.
const FINDSYNC_AUDIO_HEADROOM: f64 = 1.00;

/// How many `ModeSpec` line-times (`spec.line_seconds` — the per-radio-line
/// duration; for PD, where a radio frame carries two image rows, that's
/// twice as many *image* scan lines) of the *just-decoded* image audio to
/// keep when re-arming the VIS detector after `ImageComplete` (issue #90 D4).
/// A back-to-back transmission's VIS leader starts right after the image's
/// last line; carrying back this much audio absorbs a fast transmitter clock
/// (4 line-times ≈ 1.5–2 % of any mode's airtime — far more than any real
/// clock error, <0.1 %) so the leader is always inside the carry-forward
/// window. For a single transmission this is just the image's last few
/// lines plus trailing silence; the fresh detector finds nothing and waits
/// for more audio.
const MULTI_IMAGE_CARRYBACK_LINES: u32 = 4;

/// `|c| crate::modespec::lookup(c).is_some()` as an `fn` pointer — the
/// "is this VIS code one we can decode?" predicate handed to every
/// [`crate::vis::VisDetector`] (issue #89 A3). The closure captures nothing,
/// so it coerces to `fn(u8) -> bool` in const context.
const IS_KNOWN_VIS: fn(u8) -> bool = |c| crate::modespec::lookup(c).is_some();

/// Streaming SSTV decoder. Push audio buffers in via
/// [`Self::process`]; consume the returned events.
pub struct SstvDecoder {
    resampler: Resampler,
    vis: crate::vis::VisDetector,
    channel_demod: crate::demod::ChannelDemod,
    /// SNR estimator. Owns its own FFT plan (separate from `channel_demod`)
    /// so the per-pixel demod's scratch buffer is never aliased. SNR
    /// is re-estimated periodically inside
    /// [`crate::mode_pd::decode_pd_line_pair`] (every
    /// [`crate::demod::SNR_REESTIMATE_STRIDE`] samples).
    snr_est: crate::snr::SnrEstimator,
    state: State,
    samples_processed: u64,
    /// Cumulative working-rate samples emitted by the resampler.
    /// Used as the unit for `SstvEvent::VisDetected.sample_offset` so
    /// that value is consistent regardless of caller's input rate.
    ///
    /// **Informational only** — this counter counts samples the resampler
    /// has produced and does NOT get decremented when
    /// [`crate::vis::VisDetector::take_residual_buffer`] transfers post-stop-bit
    /// audio back to the decoder's `Decoding` state. Those residual samples
    /// were already counted here when the resampler emitted them; the
    /// residual transfer is a borrow, not a retraction. Consequently the
    /// counter may be slightly ahead of what the image decoder has consumed.
    ///
    /// The counter is used to anchor the VIS detector each time a fresh one
    /// is constructed (initial, post-image, post-unknown-VIS). Note: for the
    /// *first* detection on a freshly-built decoder, `DetectedVis::end_sample`
    /// (→ `SstvEvent::VisDetected.sample_offset` / `UnknownVis.sample_offset`)
    /// is an absolute working-rate index from sample 0. After a *restart*
    /// (post-image — see `restart_vis_detection` — or post-unknown-VIS) the
    /// fresh detector counts hops from 0, so a later detection's `sample_offset`
    /// is relative to where the carry-forward audio began, not absolute. That
    /// is acceptable — `sample_offset` is informational only — and tracked in
    /// issue #99 (fixing it needs a `VisDetector` API change). The counter
    /// here does not gate any decode logic.
    ///
    /// If mid-image VIS detection is ever re-activated (see the TODO in
    /// `process`), and a single `SstvDecoder` is reused across detections,
    /// the slight inflation is harmless: each new detection uses the then-
    /// current resampler-output count as its anchor, and the residual buffer
    /// is handed to a fresh `VisDetector::new()`.
    ///
    /// Closes #29 and #34 (both are the same observation from different angles).
    working_samples_emitted: u64,
}

impl SstvDecoder {
    /// Construct a decoder consuming audio at `input_sample_rate_hz`.
    ///
    /// # Errors
    /// Returns [`crate::Error::InvalidSampleRate`] if the rate is 0 or
    /// > [`crate::resample::MAX_INPUT_SAMPLE_RATE_HZ`].
    pub fn new(input_sample_rate_hz: u32) -> Result<Self> {
        Ok(Self {
            resampler: Resampler::new(input_sample_rate_hz)?,
            vis: crate::vis::VisDetector::new(IS_KNOWN_VIS),
            channel_demod: crate::demod::ChannelDemod::new(),
            snr_est: crate::snr::SnrEstimator::new(),
            state: State::AwaitingVis,
            samples_processed: 0,
            working_samples_emitted: 0,
        })
    }

    /// Process a chunk of mono `f32` audio samples in caller's rate.
    ///
    /// Returns events produced during this call's processing window.
    // `too_many_lines`: `process` is the decoder's state-machine loop; splitting
    // it (e.g. extracting `DecodingState::new`) is tracked in the code-review
    // audit (B14, epic #97).
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::too_many_lines
    )]
    pub fn process(&mut self, audio: &[f32]) -> Vec<SstvEvent> {
        let working = self.resampler.process(audio);
        self.samples_processed = self.samples_processed.saturating_add(audio.len() as u64);
        self.working_samples_emitted = self
            .working_samples_emitted
            .saturating_add(working.len() as u64);

        let mut out = Vec::new();
        let mut remaining: &[f32] = working.as_slice();
        loop {
            match &mut self.state {
                State::AwaitingVis => {
                    self.vis.process(remaining, self.working_samples_emitted);
                    remaining = &[];
                    if let Some(detected) = self.vis.take_detected() {
                        if let Some(spec) = crate::modespec::lookup(detected.code) {
                            out.push(SstvEvent::VisDetected {
                                mode: spec.mode,
                                sample_offset: detected.end_sample,
                                hedr_shift_hz: detected.hedr_shift_hz,
                            });
                            let image =
                                SstvImage::new(spec.mode, spec.line_pixels, spec.image_lines);
                            // Recover any post-stop-bit audio that the VIS
                            // detector buffered but did not consume — it is
                            // the leading edge of the image data.
                            let residual = self.vis.take_residual_buffer();
                            let work_rate = f64::from(crate::resample::WORKING_SAMPLE_RATE_HZ);
                            // Audio duration depends on whether the mode packs
                            // 2 image rows per radio frame (PD) or 1 (Robot,
                            // future Scottie/Martin). Mirrors slowrx's
                            // video.c:251-254: `Length = LineTime * NumLines/2`
                            // when `NumChans == 4` (PD), else
                            // `Length = LineTime * NumLines`.
                            let radio_frames_per_image = match spec.channel_layout {
                                crate::modespec::ChannelLayout::PdYcbcr => spec.image_lines / 2,
                                // Robot (RobotYuv) and Scottie (RgbSequential)
                                // both pack one image row per radio line.
                                crate::modespec::ChannelLayout::RobotYuv
                                | crate::modespec::ChannelLayout::RgbSequential => spec.image_lines,
                            };
                            let nominal_samples =
                                (f64::from(radio_frames_per_image) * spec.line_seconds * work_rate)
                                    as usize;
                            let target =
                                ((nominal_samples as f64) * FINDSYNC_AUDIO_HEADROOM) as usize;
                            self.state = State::Decoding(Box::new(DecodingState {
                                mode: spec.mode,
                                spec,
                                image,
                                audio: residual,
                                has_sync: Vec::new(),
                                next_probe_sample: 0,
                                sync_tracker: SyncTracker::new(detected.hedr_shift_hz),
                                hedr_shift_hz: detected.hedr_shift_hz,
                                target_audio_samples: target,
                                chroma_planes: match spec.mode {
                                    SstvMode::Robot24 | SstvMode::Robot36 => {
                                        let n = (spec.image_lines as usize)
                                            * (spec.line_pixels as usize);
                                        Some([vec![0_u8; n], vec![0_u8; n]])
                                    }
                                    // PD modes compose RGB per-pair in place. Robot 72 and Scottie
                                    // 1/2/DX also compose RGB in-place per radio line (see
                                    // mode_robot::decode_r72_line, mode_scottie::decode_line); only
                                    // the R36/R24 chroma-alternation + duplication path requires the
                                    // side buffer. SstvMode is #[non_exhaustive], so the wildcard arm
                                    // is required; future modes with cross-line chroma state would
                                    // need to opt in here.
                                    _ => None,
                                },
                            }));
                            continue; // re-enter loop to process leftover audio
                        }
                        // Unknown VIS code: surface it so stream-monitoring
                        // callers know a burst arrived, then reseed the
                        // detector (the `#40` re-anchor contract) on the
                        // post-stop-bit residue and re-enter the loop — a
                        // back-to-back VIS in the residue then surfaces in
                        // this same `process` call. Mirrors the known-code
                        // branch's `continue`.
                        out.push(SstvEvent::UnknownVis {
                            code: detected.code,
                            hedr_shift_hz: detected.hedr_shift_hz,
                            sample_offset: detected.end_sample,
                        });
                        let residual = self.vis.take_residual_buffer();
                        Self::restart_vis_detection(
                            &mut self.vis,
                            self.working_samples_emitted,
                            &residual,
                        );
                        continue;
                    }
                    break;
                }
                State::Decoding(d) => {
                    // TODO(future): mid-image VIS detection. When a new VIS
                    // burst arrives during decoding the spec calls for flushing
                    // the in-flight image as `partial: true` and restarting.
                    // The straightforward approach — running `self.vis` against
                    // `audio` each call — fails because the decoding buffer is
                    // not aligned to 30 ms window boundaries: the residual from
                    // the previous VIS detection starts at an arbitrary sample
                    // offset, so the first classifier window is a mix of silence
                    // and leader tone and does not reliably pass the 5× dominance
                    // threshold. A correct implementation would re-align the VIS
                    // window scan to the next 30 ms boundary, or run a separate
                    // correlator tuned to the 1900 Hz leader. Deferred to PR-3.

                    d.audio.extend_from_slice(remaining);

                    // Probe sync-band for every newly available stride
                    // window. The probe needs SYNC_FFT_WINDOW_SAMPLES/2
                    // trailing samples beyond the center; rather than
                    // depend on that constant, we conservatively wait
                    // until the audio extends `SYNC_PROBE_STRIDE * 2`
                    // beyond the next probe center.
                    while d.next_probe_sample + SYNC_PROBE_STRIDE * 2 <= d.audio.len() {
                        let center = d.next_probe_sample + SYNC_PROBE_STRIDE / 2;
                        let has = d.sync_tracker.has_sync_at(&d.audio, center);
                        d.has_sync.push(has);
                        d.next_probe_sample += SYNC_PROBE_STRIDE;
                    }

                    if d.audio.len() < d.target_audio_samples {
                        break;
                    }

                    // Buffer is full → run FindSync once, then per-pair decode.
                    Self::run_findsync_and_decode(
                        d,
                        &mut self.channel_demod,
                        &mut self.snr_est,
                        &mut out,
                    );

                    // Image complete. Re-arm VIS detection in place (no
                    // `break`! — the loop re-iterates into `AwaitingVis`) — a
                    // back-to-back transmission's VIS leader starts right after
                    // this image's last line, so feed the fresh detector only
                    // the tail of the image audio (a few lines, to absorb a
                    // fast TX clock) plus everything past it; the rest is
                    // decoded video tones a VIS burst can't hide in. Falling
                    // through (vs the old `break`) means that next VIS — and
                    // the image after it — surface in this same `process()`
                    // call, mirroring the known/unknown-code branches above.
                    // Closes #31; #90 (A2 + D4). (`sample_offset` on detections
                    // after the first is relative to the carry-forward start,
                    // not absolute — #99.)
                    let work_rate = f64::from(crate::resample::WORKING_SAMPLE_RATE_HZ);
                    let carryback = (f64::from(MULTI_IMAGE_CARRYBACK_LINES)
                        * d.spec.line_seconds
                        * work_rate) as usize;
                    let carry_from = d.target_audio_samples.saturating_sub(carryback);
                    Self::restart_vis_detection(
                        &mut self.vis,
                        self.working_samples_emitted,
                        &d.audio[carry_from..],
                    );
                    self.state = State::AwaitingVis;
                    remaining = &[]; // already folded into d.audio → now inside the fresh detector
                }
            }
        }
        out
    }

    /// Discard `vis` and start a fresh detector on `leftover_audio`
    /// (post-stop-bit residue, or trailing image audio). Honors the `#40`
    /// re-anchor contract documented on
    /// [`crate::vis::VisDetector::take_residual_buffer`] — a spent detector's
    /// `hops_completed` / `history` state is never reset, so it must be
    /// replaced rather than re-used. `working_samples_emitted` is the
    /// decoder's running working-rate output count (used to anchor the fresh
    /// detector).
    fn restart_vis_detection(
        vis: &mut crate::vis::VisDetector,
        working_samples_emitted: u64,
        leftover_audio: &[f32],
    ) {
        *vis = crate::vis::VisDetector::new(IS_KNOWN_VIS);
        vis.process(leftover_audio, working_samples_emitted);
    }

    /// Run [`find_sync`] over the buffered sync track, then decode every
    /// PD line pair against the corrected `(rate, skip)`. Pushes
    /// [`SstvEvent::LineDecoded`] for every row + a final
    /// [`SstvEvent::ImageComplete`] into `out`.
    ///
    /// **Lookahead note (#33):** Each call to
    /// [`crate::mode_pd::decode_pd_line_pair`] receives `&d.audio` — the
    /// entire image audio buffer, not a slice ending at the pair's nominal
    /// end sample. This means the FFT window for the last pixel of the last
    /// channel of each line pair can freely extend rightward into subsequent
    /// pair audio (or zero if the buffer ends). The lookahead is therefore
    /// *implicit*: the full-buffer pass-through provides the context that a
    /// naive `&audio[..pair_end]` slice would lose. No explicit `lookahead`
    /// variable is required, and none should be added. (Issue #33 noted
    /// a now-deleted `lookahead` variable that was dead code; Phase 3's
    /// rewrite eliminated it by design.)
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_possible_wrap
    )]
    fn run_findsync_and_decode(
        d: &mut DecodingState,
        channel_demod: &mut crate::demod::ChannelDemod,
        snr_est: &mut crate::snr::SnrEstimator,
        out: &mut Vec<SstvEvent>,
    ) {
        let work_rate = f64::from(crate::resample::WORKING_SAMPLE_RATE_HZ);
        let result = find_sync(&d.has_sync, work_rate, d.spec);
        let rate = result.adjusted_rate_hz;
        let skip = result.skip_samples;

        let line_pixels = d.spec.line_pixels as usize;
        match d.spec.channel_layout {
            crate::modespec::ChannelLayout::PdYcbcr => {
                let pair_count = d.spec.image_lines / 2;
                for pair in 0..pair_count {
                    // slowrx `video.c:140-142` computes pixel time as
                    // `Skip + round(Rate * (y/2 * LineTime + ChanStart +
                    // PixelTime * (x + 0.5)))`. Compute `pair_seconds = y/2 *
                    // LineTime` here (un-rounded) and let
                    // [`crate::mode_pd::decode_pd_line_pair`] fold it into its
                    // own `round()`, so per-pair rounding error never
                    // accumulates.
                    let pair_seconds = f64::from(pair) * d.spec.line_seconds;
                    crate::mode_pd::decode_pd_line_pair(
                        d.spec,
                        pair,
                        &d.audio,
                        skip,
                        pair_seconds,
                        rate,
                        &mut d.image,
                        channel_demod,
                        snr_est,
                        d.hedr_shift_hz,
                    );
                    let row0 = pair * 2;
                    let row1 = row0 + 1;
                    for r in [row0, row1] {
                        let start = (r as usize) * line_pixels;
                        let end = start + line_pixels;
                        out.push(SstvEvent::LineDecoded {
                            mode: d.mode,
                            line_index: r,
                            pixels: d.image.pixels[start..end].to_vec(),
                        });
                    }
                }
            }
            crate::modespec::ChannelLayout::RobotYuv => {
                // Robot is per-line (no PD line-pairing). For R36/R24 the
                // chroma-duplication writes to the next image row; that's
                // handled inside mode_robot::decode_line. LineDecoded for image
                // row N is emitted after radio-line N's decode — for R36/R24
                // row 0 the Cb channel is at zero-init at this point (slowrx
                // C does the same; final ImageComplete carries the populated
                // state).
                for line in 0..d.spec.image_lines {
                    let line_seconds_offset = f64::from(line) * d.spec.line_seconds;
                    crate::mode_robot::decode_line(
                        d.spec,
                        d.mode,
                        line,
                        &d.audio,
                        skip,
                        line_seconds_offset,
                        rate,
                        &mut d.image,
                        d.chroma_planes.as_mut(),
                        channel_demod,
                        snr_est,
                        d.hedr_shift_hz,
                    );
                    let start = (line as usize) * line_pixels;
                    let end = start + line_pixels;
                    out.push(SstvEvent::LineDecoded {
                        mode: d.mode,
                        line_index: line,
                        pixels: d.image.pixels[start..end].to_vec(),
                    });
                }
            }
            crate::modespec::ChannelLayout::RgbSequential => {
                // Scottie family. Mid-line sync handling lives inside
                // mode_scottie::decode_line. No chroma_planes — RGB is composed
                // in-place per line (no deferred chroma like R36/R24).
                for line in 0..d.spec.image_lines {
                    let line_seconds_offset = f64::from(line) * d.spec.line_seconds;
                    crate::mode_scottie::decode_line(
                        d.spec,
                        line,
                        &d.audio,
                        skip,
                        line_seconds_offset,
                        rate,
                        &mut d.image,
                        channel_demod,
                        snr_est,
                        d.hedr_shift_hz,
                    );
                    let start = (line as usize) * line_pixels;
                    let end = start + line_pixels;
                    out.push(SstvEvent::LineDecoded {
                        mode: d.mode,
                        line_index: line,
                        pixels: d.image.pixels[start..end].to_vec(),
                    });
                }
            }
        }

        let final_image = std::mem::replace(
            &mut d.image,
            SstvImage::new(d.mode, d.spec.line_pixels, d.spec.image_lines),
        );
        out.push(SstvEvent::ImageComplete {
            image: final_image,
            partial: false,
        });
    }

    /// Reset to `AwaitingVis`; discard any in-flight image.
    pub fn reset(&mut self) {
        self.state = State::AwaitingVis;
        self.samples_processed = 0;
        self.working_samples_emitted = 0;
        self.vis = crate::vis::VisDetector::new(IS_KNOWN_VIS);
        self.resampler.reset_state();
        self.channel_demod = crate::demod::ChannelDemod::new();
        self.snr_est = crate::snr::SnrEstimator::new();
    }

    /// Total samples processed since construction (or last `reset`).
    #[must_use]
    pub fn samples_processed(&self) -> u64 {
        self.samples_processed
    }
}

/// Estimate the dominant tone frequency in `window` (working-rate samples).
/// Returns the estimated frequency in Hz, biased toward 1500-2300 Hz
/// (the SSTV video band).
///
/// Algorithm: Goertzel-bank evaluated at 25-Hz steps from 1450 to 2350 Hz,
/// then quadratic peak interpolation around the maximum bin.
#[must_use]
#[allow(clippy::cast_precision_loss, dead_code)]
pub(crate) fn estimate_freq(window: &[f32]) -> f64 {
    const STEP_HZ: f64 = 25.0;
    const FIRST_HZ: f64 = 1450.0;
    const N_BINS: usize = 37; // 1450..2350 inclusive at 25 Hz steps

    let mut powers = [0.0_f64; N_BINS];
    for (i, p) in powers.iter_mut().enumerate() {
        let f = FIRST_HZ + (i as f64) * STEP_HZ;
        *p = crate::dsp::goertzel_power(window, f);
    }
    let (mut max_i, mut max_p) = (0_usize, powers[0]);
    for (i, &p) in powers.iter().enumerate().skip(1) {
        if p > max_p {
            max_p = p;
            max_i = i;
        }
    }
    let center_hz = FIRST_HZ + (max_i as f64) * STEP_HZ;
    // Quadratic interpolation if we have both neighbours.
    if max_i > 0 && max_i < N_BINS - 1 && max_p > 0.0 {
        let a = powers[max_i - 1];
        let b = max_p;
        let c = powers[max_i + 1];
        let denom = a - 2.0 * b + c;
        if denom.abs() > 1e-12 {
            let delta = 0.5 * (a - c) / denom;
            return center_hz + delta * STEP_HZ;
        }
    }
    center_hz
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
mod tests {
    use super::*;
    use crate::error::Error;
    use crate::resample::{MAX_INPUT_SAMPLE_RATE_HZ, WORKING_SAMPLE_RATE_HZ};

    #[test]
    fn rejects_invalid_sample_rates() {
        assert!(matches!(
            SstvDecoder::new(0),
            Err(Error::InvalidSampleRate { got: 0 })
        ));
        assert!(matches!(
            SstvDecoder::new(MAX_INPUT_SAMPLE_RATE_HZ + 1),
            Err(Error::InvalidSampleRate { .. })
        ));
    }

    #[test]
    fn accepts_common_rates() {
        assert!(SstvDecoder::new(11_025).is_ok());
        assert!(SstvDecoder::new(44_100).is_ok());
        assert!(SstvDecoder::new(48_000).is_ok());
    }

    #[test]
    fn process_advances_sample_counter() {
        let mut d = SstvDecoder::new(11_025).expect("decoder");
        assert_eq!(d.samples_processed(), 0);
        let _ = d.process(&[0.0_f32; 1024]);
        assert_eq!(d.samples_processed(), 1024);
        let _ = d.process(&[0.0_f32; 256]);
        assert_eq!(d.samples_processed(), 1280);
    }

    #[test]
    fn process_returns_no_events_for_silence() {
        let mut d = SstvDecoder::new(11_025).expect("decoder");
        // Silence produces no VIS match.
        let events = d.process(&[0.5_f32; 512]);
        assert!(events.is_empty());
    }

    #[test]
    fn process_emits_vis_detected_for_pd120_burst() {
        use crate::vis::tests::synth_vis;
        let mut d = SstvDecoder::new(WORKING_SAMPLE_RATE_HZ).expect("decoder");
        // Pad with trailing silence so the polyphase FIR's ~64-sample group
        // delay still yields a full set of stop-bit windows (PR-2 T2.1).
        let mut burst = synth_vis(0x5F, 0.0);
        burst.extend(std::iter::repeat_n(0.0_f32, 512));
        let events = d.process(&burst);
        let hedr = events
            .iter()
            .find_map(|e| match e {
                SstvEvent::VisDetected {
                    mode: SstvMode::Pd120,
                    hedr_shift_hz,
                    ..
                } => Some(*hedr_shift_hz),
                _ => None,
            })
            .expect("expected VisDetected for PD120");
        assert!(
            hedr.abs() < 10.0,
            "synthetic burst should report ~0 Hz shift, got {hedr}"
        );
    }

    #[test]
    fn process_emits_vis_detected_for_pd180_burst() {
        use crate::vis::tests::synth_vis;
        let mut d = SstvDecoder::new(WORKING_SAMPLE_RATE_HZ).expect("decoder");
        let mut burst = synth_vis(0x60, 0.0);
        burst.extend(std::iter::repeat_n(0.0_f32, 512));
        let events = d.process(&burst);
        let hedr = events
            .iter()
            .find_map(|e| match e {
                SstvEvent::VisDetected {
                    mode: SstvMode::Pd180,
                    hedr_shift_hz,
                    ..
                } => Some(*hedr_shift_hz),
                _ => None,
            })
            .expect("expected VisDetected for PD180");
        assert!(hedr.abs() < 10.0);
    }

    #[test]
    fn process_emits_vis_detected_for_pd240_burst() {
        use crate::vis::tests::synth_vis;
        let mut d = SstvDecoder::new(WORKING_SAMPLE_RATE_HZ).expect("decoder");
        let mut burst = synth_vis(0x61, 0.0);
        burst.extend(std::iter::repeat_n(0.0_f32, 512));
        let events = d.process(&burst);
        let hedr = events
            .iter()
            .find_map(|e| match e {
                SstvEvent::VisDetected {
                    mode: SstvMode::Pd240,
                    hedr_shift_hz,
                    ..
                } => Some(*hedr_shift_hz),
                _ => None,
            })
            .expect("expected VisDetected for PD240");
        assert!(hedr.abs() < 10.0);
    }

    #[test]
    fn process_emits_vis_detected_for_robot24_burst() {
        use crate::vis::tests::synth_vis;
        let mut d = SstvDecoder::new(WORKING_SAMPLE_RATE_HZ).expect("decoder");
        let mut burst = synth_vis(0x04, 0.0);
        burst.extend(std::iter::repeat_n(0.0_f32, 512));
        let events = d.process(&burst);
        let hedr = events
            .iter()
            .find_map(|e| match e {
                SstvEvent::VisDetected {
                    mode: SstvMode::Robot24,
                    hedr_shift_hz,
                    ..
                } => Some(*hedr_shift_hz),
                _ => None,
            })
            .expect("expected VisDetected for Robot24");
        assert!(hedr.abs() < 10.0);
    }

    #[test]
    fn process_emits_vis_detected_for_robot36_burst() {
        use crate::vis::tests::synth_vis;
        let mut d = SstvDecoder::new(WORKING_SAMPLE_RATE_HZ).expect("decoder");
        let mut burst = synth_vis(0x08, 0.0);
        burst.extend(std::iter::repeat_n(0.0_f32, 512));
        let events = d.process(&burst);
        let hedr = events
            .iter()
            .find_map(|e| match e {
                SstvEvent::VisDetected {
                    mode: SstvMode::Robot36,
                    hedr_shift_hz,
                    ..
                } => Some(*hedr_shift_hz),
                _ => None,
            })
            .expect("expected VisDetected for Robot36");
        assert!(hedr.abs() < 10.0);
    }

    #[test]
    fn process_emits_vis_detected_for_robot72_burst() {
        use crate::vis::tests::synth_vis;
        let mut d = SstvDecoder::new(WORKING_SAMPLE_RATE_HZ).expect("decoder");
        let mut burst = synth_vis(0x0C, 0.0);
        burst.extend(std::iter::repeat_n(0.0_f32, 512));
        let events = d.process(&burst);
        let hedr = events
            .iter()
            .find_map(|e| match e {
                SstvEvent::VisDetected {
                    mode: SstvMode::Robot72,
                    hedr_shift_hz,
                    ..
                } => Some(*hedr_shift_hz),
                _ => None,
            })
            .expect("expected VisDetected for Robot72");
        assert!(hedr.abs() < 10.0);
    }

    #[test]
    fn reset_clears_sample_counter() {
        let mut d = SstvDecoder::new(11_025).expect("decoder");
        let _ = d.process(&[0.0_f32; 1024]);
        d.reset();
        assert_eq!(d.samples_processed(), 0);
    }

    // 40 ms tones make every 25-Hz bank bin map to a unique Goertzel k
    // (11025/441 = 25.0). Production windows are ~5 ms; ~50 Hz suffices.
    fn synth_tone_at_working(freq_hz: f64, secs: f64) -> Vec<f32> {
        let sr = f64::from(WORKING_SAMPLE_RATE_HZ);
        let n = (secs * sr).round() as usize;
        (0..n)
            .map(|i| (2.0 * std::f64::consts::PI * freq_hz * (i as f64) / sr).sin() as f32)
            .collect()
    }

    #[test]
    fn estimate_freq_recovers_known_tone() {
        for &f in &[1500.0_f64, 1700.0, 1900.0, 2100.0, 2300.0] {
            let window = synth_tone_at_working(f, 0.040);
            let est = estimate_freq(&window);
            assert!((est - f).abs() < 30.0, "freq={f} estimate={est}");
        }
    }

    #[test]
    fn estimate_freq_no_interp_at_left_boundary() {
        // Tone at 1450 Hz lands on bin 0; no left neighbour → no interp.
        let window = synth_tone_at_working(1450.0, 0.040);
        let est = estimate_freq(&window);
        assert!((est - 1450.0).abs() < 30.0, "expected ≈1450, got {est}");
    }

    #[test]
    fn reset_during_decoding_emits_partial_via_subsequent_process() {
        let mut d = SstvDecoder::new(crate::resample::WORKING_SAMPLE_RATE_HZ).unwrap();
        // Push a VIS so the decoder transitions to Decoding. Trailing zeros
        // accommodate the FIR group delay so the burst actually triggers
        // detection (without the padding the test would mask Finding 1
        // by never entering Decoding).
        let mut burst = crate::vis::tests::synth_vis(0x5F, 0.0);
        burst.extend(std::iter::repeat_n(0.0_f32, 512));
        let events = d.process(&burst);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, SstvEvent::VisDetected { .. })),
            "expected VIS detection before reset, got {events:?}"
        );
        // We're now in Decoding state.
        d.reset();
        // After reset, the decoder is back in AwaitingVis with FIR resampler
        // and ChannelDemod state cleared. The next process call with quiet audio
        // yields no events.
        let events = d.process(&[0.0_f32; 100]);
        assert!(
            events.is_empty(),
            "reset should clear in-flight; got {events:?}"
        );
    }

    // TODO(future/PR-3): mid_image_vis_emits_partial_then_new_vis
    //
    // When a new VIS burst arrives during Decoding the spec calls for
    // emitting `ImageComplete { partial: true }` for the in-flight image,
    // then transitioning to AwaitingVis.
    //
    // The naive approach (running `self.vis` against the decoding buffer
    // each call) fails because the residual buffer from a previous VIS
    // detection is not aligned to 30 ms window boundaries: the first
    // classifier window is a mix of silence and leader tone and does not
    // reliably pass the 5× dominance threshold. A correct implementation
    // would re-align the scan to the next 30 ms boundary or run a separate
    // 1900 Hz energy detector. Deferred to PR-3 (cross-validation).
}
